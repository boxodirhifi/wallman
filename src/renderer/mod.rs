mod worker;

use std::collections::HashMap;
use std::fs::File;
use std::os::fd::AsFd;
use std::sync::mpsc::Receiver;

use wayland_client::{
    backend::ObjectId,
    protocol::{
        wl_buffer::{self, WlBuffer},
        wl_callback::{self, WlCallback},
        wl_compositor::{self, WlCompositor},
        wl_output::{self, WlOutput},
        wl_registry::{self, WlRegistry},
        wl_shm::{self, WlShm},
        wl_shm_pool::{self, WlShmPool},
        wl_surface::{self, WlSurface},
    },
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};

use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

use worker::{spawn_worker, WorkerCommand, WorkerResponse, MonitorJob};

pub enum RendererCommand {
    SetWallpaper {
        image: std::path::PathBuf,
        mode: String,
    },
    Reload,
}

struct SurfaceState {
    namespace: String,
    surface: Option<WlSurface>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    pool: Option<WlShmPool>,
    buffer: Option<WlBuffer>,
    file: Option<File>,
    old_buffers: Vec<WlBuffer>,
    configure_serial: Option<u32>,
    width: u32,
    height: u32,
    closed: bool,
}

impl SurfaceState {
    fn new(namespace: &str) -> Self {
        SurfaceState {
            namespace: namespace.to_string(),
            surface: None,
            layer_surface: None,
            pool: None,
            buffer: None,
            file: None,
            old_buffers: Vec::new(),
            configure_serial: None,
            width: 0,
            height: 0,
            closed: false,
        }
    }
}

struct Monitor {
    name: String,
    _output: WlOutput,
    wallpaper: SurfaceState,
    backdrop: SurfaceState,
}

struct State {
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    layer_shell: Option<ZwlrLayerShellV1>,
    monitors: Vec<Monitor>,
    pending_outputs: Vec<WlOutput>,
    monitor_names: HashMap<ObjectId, String>, // Maps output ID to name
    current_mode: String,
}

fn prepare_surface(
    ss: &mut SurfaceState,
    shm: &WlShm,
    qh: &QueueHandle<State>,
    file: File,
    width: u32,
    height: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let width_i32 = i32::try_from(width)?;
    let height_i32 = i32::try_from(height)?;
    let stride = usize::try_from(width)?.checked_mul(4).ok_or("stride overflow")?;
    let expected_size = stride.checked_mul(usize::try_from(height)?).ok_or("buffer size overflow")?;
    let stride_i32 = i32::try_from(stride)?;
    let size_i32 = i32::try_from(expected_size)?;

    let pool: WlShmPool = shm.create_pool(file.as_fd(), size_i32, qh, ());
    let buffer: WlBuffer = pool.create_buffer(0, width_i32, height_i32, stride_i32, wl_shm::Format::Xrgb8888, qh, ());

    if let Some(surface) = ss.surface.as_ref() {
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, width_i32, height_i32);
        surface.commit();
    } else {
        return Err("wl_surface not created".into());
    }

    if let Some(old_buffer) = ss.buffer.take() {
        ss.old_buffers.push(old_buffer);
    }

    ss.pool = Some(pool);
    ss.buffer = Some(buffer);
    ss.file = Some(file);
    Ok(())
}

fn write_colors_file(colors: &[(u8, u8, u8)]) {
    if colors.is_empty() { return; }
    let project_dirs = match directories::ProjectDirs::from("", "", "wallman") {
        Some(dirs) => dirs,
        None => { eprintln!("Failed to determine cache directory for colors file"); return; }
    };
    let cache_dir = project_dirs.cache_dir();
    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        eprintln!("Failed to create cache directory: {e}"); return;
    }
    let colors_path = cache_dir.join("colors.toml");
    let mut content = String::from("# Wallman color palette\n# Generated from current wallpaper\n\n");
    let names = ["primary", "secondary", "tertiary", "quaternary", "quinary"];
    for (i, color) in colors.iter().enumerate().take(5) {
        let name = names.get(i).unwrap_or(&"extra");
        content.push_str(&format!("{} = \"#{:02x}{:02x}{:02x}\"\n", name, color.0, color.1, color.2));
    }
    if let Err(e) = std::fs::write(&colors_path, content) {
        eprintln!("Failed to write colors file: {e}");
    } else {
        println!("wrote color palette to {}", colors_path.display());
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(state: &mut State, registry: &WlRegistry, event: wl_registry::Event, _data: &(), _conn: &Connection, qh: &QueueHandle<State>) {
        match event {
            wl_registry::Event::Global { name, interface, version } => {
                if interface == "wl_compositor" && state.compositor.is_none() {
                    let bind_version = version.min(6);
                    let compositor: WlCompositor = registry.bind(name, bind_version, qh, ());
                    state.compositor = Some(compositor);
                    println!("bound: wl_compositor v{bind_version}");
                }
                if interface == "wl_shm" && state.shm.is_none() {
                    let bind_version = version.min(2);
                    let shm: WlShm = registry.bind(name, bind_version, qh, ());
                    state.shm = Some(shm);
                    println!("bound: wl_shm v{bind_version}");
                }
                if interface == "zwlr_layer_shell_v1" && state.layer_shell.is_none() {
                    let bind_version = version.min(5);
                    let layer_shell: ZwlrLayerShellV1 = registry.bind(name, bind_version, qh, ());
                    state.layer_shell = Some(layer_shell);
                    println!("bound: zwlr_layer_shell_v1 v{bind_version}");
                }
                if interface == "wl_output" {
                    let bind_version = version.min(4);
                    let output: WlOutput = registry.bind(name, bind_version, qh, ());
                    state.pending_outputs.push(output);
                    println!("bound: wl_output v{bind_version}");
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                println!("{name}: removed");
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCompositor, ()> for State { fn event(_s: &mut Self, _o: &WlCompositor, _e: wl_compositor::Event, _d: &(), _c: &Connection, _q: &QueueHandle<Self>) {} }
impl Dispatch<WlShm, ()> for State { fn event(_s: &mut Self, _o: &WlShm, _e: wl_shm::Event, _d: &(), _c: &Connection, _q: &QueueHandle<Self>) {} }
impl Dispatch<WlShmPool, ()> for State { fn event(_s: &mut Self, _o: &WlShmPool, _e: wl_shm_pool::Event, _d: &(), _c: &Connection, _q: &QueueHandle<Self>) {} }
impl Dispatch<WlSurface, ()> for State { fn event(_s: &mut Self, _o: &WlSurface, _e: wl_surface::Event, _d: &(), _c: &Connection, _q: &QueueHandle<Self>) {} }
impl Dispatch<ZwlrLayerShellV1, ()> for State { fn event(_s: &mut Self, _o: &ZwlrLayerShellV1, _e: zwlr_layer_shell_v1::Event, _d: &(), _c: &Connection, _q: &QueueHandle<Self>) {} }
impl Dispatch<WlCallback, ()> for State { fn event(_s: &mut Self, _o: &WlCallback, _e: wl_callback::Event, _d: &(), _c: &Connection, _q: &QueueHandle<Self>) {} }

impl Dispatch<WlOutput, ()> for State {
    fn event(state: &mut State, output: &WlOutput, event: wl_output::Event, _d: &(), _c: &Connection, _q: &QueueHandle<Self>) {
        if let wl_output::Event::Name { name } = event {
            // Store the name using the output's unique ID
            state.monitor_names.insert(output.id(), name.clone());
            println!("Discovered monitor: {}", name);
        }
    }
}

impl Dispatch<WlBuffer, ()> for State {
    fn event(state: &mut State, buffer: &WlBuffer, event: wl_buffer::Event, _d: &(), _c: &Connection, _q: &QueueHandle<Self>) {
        if let wl_buffer::Event::Release = event {
            for monitor in &mut state.monitors {
                monitor.wallpaper.old_buffers.retain(|b| b.id() != buffer.id());
                monitor.backdrop.old_buffers.retain(|b| b.id() != buffer.id());
            }
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for State {
    fn event(state: &mut State, layer_surface: &ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event, _d: &(), _c: &Connection, _q: &QueueHandle<Self>) {
        for monitor in &mut state.monitors {
            let is_wp = monitor.wallpaper.layer_surface.as_ref().map(|ls| ls.id() == layer_surface.id()).unwrap_or(false);
            let is_bd = monitor.backdrop.layer_surface.as_ref().map(|ls| ls.id() == layer_surface.id()).unwrap_or(false);

            let ss = if is_wp { Some(&mut monitor.wallpaper) } else if is_bd { Some(&mut monitor.backdrop) } else { None };

            if let Some(ss) = ss {
                match event {
                    zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                        println!("[{}] configure: serial={serial} width={width} height={height}", ss.namespace);
                        ss.configure_serial = Some(serial);
                        ss.width = width;
                        ss.height = height;
                    }
                    zwlr_layer_surface_v1::Event::Closed => {
                        println!("[{}] layer surface closed", ss.namespace);
                        ss.closed = true;
                    }
                    _ => {}
                }
                break;
            }
        }
    }
}

pub fn run(
    image: impl AsRef<std::path::Path>,
    initial_mode: String,
    receiver: Receiver<RendererCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Wallman renderer started");
    let conn = Connection::connect_to_env()?;
    let display = conn.display();

    let mut event_queue: EventQueue<State> = conn.new_event_queue();
    let qh = event_queue.handle();
    let _registry = display.get_registry(&qh, ());

    let mut state = State {
        compositor: None,
        shm: None,
        layer_shell: None,
        monitors: Vec::new(),
        pending_outputs: Vec::new(),
        monitor_names: HashMap::new(),
        current_mode: initial_mode,
    };

    event_queue.roundtrip(&mut state)?;
    event_queue.roundtrip(&mut state)?;

    if state.compositor.is_none() { return Err("wl_compositor global not found".into()); }
    if state.shm.is_none() { return Err("wl_shm global not found".into()); }
    if state.layer_shell.is_none() { return Err("zwlr_layer_shell_v1 global not found".into()); }

    // Create surfaces for all pending outputs
    let compositor = state.compositor.as_ref().unwrap();
    let layer_shell = state.layer_shell.as_ref().unwrap();
    let pending_outputs = std::mem::take(&mut state.pending_outputs);

    for output in pending_outputs {
        let name = state.monitor_names.get(&output.id()).cloned().unwrap_or_else(|| format!("output-{}", output.id()));
        let wp_surface = compositor.create_surface(&qh, ());
        let wp_layer = layer_shell.get_layer_surface(&wp_surface, Some(&output), zwlr_layer_shell_v1::Layer::Background, "wallpaper".to_string(), &qh, ());
        wp_layer.set_size(0, 0);
        wp_layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        wp_layer.set_exclusive_zone(-1);
        wp_layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);

        let bd_surface = compositor.create_surface(&qh, ());
        let bd_layer = layer_shell.get_layer_surface(&bd_surface, Some(&output), zwlr_layer_shell_v1::Layer::Background, "wallman-backdrop".to_string(), &qh, ());
        bd_layer.set_size(0, 0);
        bd_layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        bd_layer.set_exclusive_zone(-1);
        bd_layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);

        let mut wp_state = SurfaceState::new("wallpaper");
        wp_state.surface = Some(wp_surface);
        wp_state.layer_surface = Some(wp_layer);

        let mut bd_state = SurfaceState::new("wallman-backdrop");
        bd_state.surface = Some(bd_surface);
        bd_state.layer_surface = Some(bd_layer);

        state.monitors.push(Monitor {
            name, // Will be updated by WlOutput::Name event
            _output: output,
            wallpaper: wp_state,
            backdrop: bd_state,
        });
    }

    // Commit empty surfaces to trigger configure
    for monitor in &state.monitors {
        if let Some(s) = monitor.wallpaper.surface.as_ref() { s.commit(); }
        if let Some(s) = monitor.backdrop.surface.as_ref() { s.commit(); }
    }
    println!("created surfaces for {} monitor(s)", state.monitors.len());

    // Wait for all configures
    let mut attempts = 0;
    while state.monitors.iter().any(|m| m.wallpaper.configure_serial.is_none() || m.backdrop.configure_serial.is_none()) && attempts < 10 {
        event_queue.roundtrip(&mut state)?;
        attempts += 1;
    }

    for monitor in &mut state.monitors {
        let wp_serial = monitor.wallpaper.configure_serial.take().ok_or("no wallpaper configure")?;
        let bd_serial = monitor.backdrop.configure_serial.take().ok_or("no backdrop configure")?;
        if monitor.wallpaper.width == 0 || monitor.wallpaper.height == 0 { return Err("wallpaper zero size".into()); }
        if monitor.backdrop.width == 0 || monitor.backdrop.height == 0 { return Err("backdrop zero size".into()); }
        if let Some(ls) = monitor.wallpaper.layer_surface.as_ref() { ls.ack_configure(wp_serial); }
        if let Some(ls) = monitor.backdrop.layer_surface.as_ref() { ls.ack_configure(bd_serial); }
    }

    let (worker_tx, worker_rx) = spawn_worker();
    let shm = state.shm.as_ref().ok_or("wl_shm not bound")?.clone();

    let jobs: Vec<MonitorJob> = state.monitors.iter().map(|m| MonitorJob {
        name: m.name.clone(),
                                                          path: image.as_ref().to_path_buf(),
                                                          mode: state.current_mode.clone(),
                                                          wp_width: m.wallpaper.width,
                                                          wp_height: m.wallpaper.height,
                                                          bd_width: m.backdrop.width,
                                                          bd_height: m.backdrop.height,
    }).collect();

    worker_tx.send(WorkerCommand::Process { jobs }).expect("worker crashed");

    match worker_rx.recv().expect("worker crashed") {
        WorkerResponse::Ready { colors, monitors: results } => {
            write_colors_file(&colors);
            for result in results {
                if let Some(monitor) = state.monitors.iter_mut().find(|m| m.name == result.name) {
                    prepare_surface(&mut monitor.wallpaper, &shm, &qh, result.wallpaper_file, result.wp_width, result.wp_height)?;
                    prepare_surface(&mut monitor.backdrop, &shm, &qh, result.backdrop_file, result.bd_width, result.bd_height)?;
                }
            }
        }
        WorkerResponse::Failed(e) => return Err(format!("initial process failed: {e}").into()),
    }

    event_queue.roundtrip(&mut state)?;
    println!("Wallman renderer is running (multi-monitor)");

    loop {
        if state.monitors.iter().any(|m| m.wallpaper.closed || m.backdrop.closed) { break; }

        while let Ok(command) = receiver.try_recv() {
            match command {
                RendererCommand::Reload => {
                    let jobs: Vec<MonitorJob> = state.monitors.iter().map(|m| MonitorJob {
                        name: m.name.clone(),
                                                                          path: image.as_ref().to_path_buf(),
                                                                          mode: state.current_mode.clone(),
                                                                          wp_width: m.wallpaper.width,
                                                                          wp_height: m.wallpaper.height,
                                                                          bd_width: m.backdrop.width,
                                                                          bd_height: m.backdrop.height,
                    }).collect();
                    let _ = worker_tx.send(WorkerCommand::Process { jobs });
                }
                RendererCommand::SetWallpaper { image, mode } => {
                    state.current_mode = mode.clone();
                    let jobs: Vec<MonitorJob> = state.monitors.iter().map(|m| MonitorJob {
                        name: m.name.clone(),
                                                                          path: image.clone(),
                                                                          mode: mode.clone(),
                                                                          wp_width: m.wallpaper.width,
                                                                          wp_height: m.wallpaper.height,
                                                                          bd_width: m.backdrop.width,
                                                                          bd_height: m.backdrop.height,
                    }).collect();
                    let _ = worker_tx.send(WorkerCommand::Process { jobs });
                }
            }
        }

        while let Ok(response) = worker_rx.try_recv() {
            match response {
                WorkerResponse::Ready { colors, monitors: results } => {
                    write_colors_file(&colors);
                    let shm = match state.shm.as_ref() {
                        Some(s) => s.clone(),
                        None => { eprintln!("wl_shm lost"); continue; }
                    };
                    for result in results {
                        if let Some(monitor) = state.monitors.iter_mut().find(|m| m.name == result.name) {
                            if let Err(e) = prepare_surface(&mut monitor.wallpaper, &shm, &qh, result.wallpaper_file, result.wp_width, result.wp_height) {
                                eprintln!("wp prep failed: {e}");
                            }
                            if let Err(e) = prepare_surface(&mut monitor.backdrop, &shm, &qh, result.backdrop_file, result.bd_width, result.bd_height) {
                                eprintln!("bd prep failed: {e}");
                            }
                        }
                    }
                    println!("updated wallpaper (mode: {})", state.current_mode);
                }
                WorkerResponse::Failed(e) => eprintln!("process failed: {e}"),
            }
        }

        event_queue.dispatch_pending(&mut state)?;
        conn.flush()?;

        for monitor in &mut state.monitors {
            if let Some(serial) = monitor.wallpaper.configure_serial.take() {
                if let Some(ls) = monitor.wallpaper.layer_surface.as_ref() { ls.ack_configure(serial); }
            }
            if let Some(serial) = monitor.backdrop.configure_serial.take() {
                if let Some(ls) = monitor.backdrop.layer_surface.as_ref() { ls.ack_configure(serial); }
            }
        }

        let Some(read_guard) = event_queue.prepare_read() else { continue; };
        let fd = read_guard.connection_fd();
        let mut fds = [libc::pollfd { fd: std::os::fd::AsRawFd::as_raw_fd(&fd), events: libc::POLLIN, revents: 0 }];
        let timeout_ms = 200;
        let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, timeout_ms) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted { drop(read_guard); continue; }
            drop(read_guard);
            return Err(err.into());
        }
        if ret > 0 { read_guard.read()?; } else { drop(read_guard); }
    }
    Ok(())
}
