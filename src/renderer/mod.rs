mod worker;

use std::collections::HashMap;
use std::fs::File;
use std::os::fd::AsFd;
use std::io::Write;

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

use calloop::generic::Generic;
use calloop::signals::{Signal, Signals};
use calloop::{EventLoop, Interest, Mode, PostAction};

pub enum RendererCommand {
    SetWallpaper {
        image: std::path::PathBuf,
        mode: String,
        monitor: String, // empty string means all monitors
        blur: u32,
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
    old_buffers: Vec<(WlBuffer, WlShmPool, File)>,
    configure_serial: Option<u32>,
    width: u32,
    height: u32,
    closed: bool,
    needs_rerender: bool,
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
            needs_rerender: false,
        }
    }
}

struct Monitor {
    global_id: u32,
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
    pending_outputs: Vec<(u32, WlOutput)>,
    monitor_names: HashMap<ObjectId, String>,
    current_mode: String,
    default_image: Option<std::path::PathBuf>,
    monitor_overrides: HashMap<String, (std::path::PathBuf, String)>,
    current_blur: u32,
    loop_signal: Option<calloop::LoopSignal>,
    per_monitor_blur: HashMap<String, u32>,
    worker_tx: Option<std::sync::mpsc::Sender<WorkerCommand>>,
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

    let old_buffer = ss.buffer.take();
    let old_pool = ss.pool.take();
    let old_file = ss.file.take();

    if let (Some(ob), Some(op), Some(of)) = (old_buffer, old_pool, old_file) {
        ss.old_buffers.push((ob, op, of));
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
                    state.pending_outputs.push((name, output));
                    println!("bound: wl_output v{bind_version}");
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(pos) = state.monitors.iter().position(|m| m.global_id == name) {
                    let monitor = state.monitors.remove(pos);
                    if let Some(ls) = monitor.wallpaper.layer_surface.as_ref() { ls.destroy(); }
                    if let Some(s) = monitor.wallpaper.surface.as_ref() { s.destroy(); }
                    if let Some(ls) = monitor.backdrop.layer_surface.as_ref() { ls.destroy(); }
                    if let Some(s) = monitor.backdrop.surface.as_ref() { s.destroy(); }
                    println!("Monitor {} disconnected", monitor.name);
                }
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
            state.monitor_names.insert(output.id(), name.clone());
            println!("Discovered monitor: {}", name);
        }
    }
}

impl Dispatch<WlBuffer, ()> for State {
    fn event(state: &mut State, buffer: &WlBuffer, event: wl_buffer::Event, _d: &(), _c: &Connection, _q: &QueueHandle<Self>) {
        if let wl_buffer::Event::Release = event {
            for monitor in &mut state.monitors {
                monitor.wallpaper.old_buffers.retain(|(b, _, _)| b.id() != buffer.id());
                monitor.backdrop.old_buffers.retain(|(b, _, _)| b.id() != buffer.id());
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
                        if ss.width != width || ss.height != height {
                            ss.width = width;
                            ss.height = height;
                            ss.needs_rerender = true;
                        }
                        ss.configure_serial = Some(serial);
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

fn build_jobs(state: &State) -> Vec<MonitorJob> {
    state.monitors.iter().filter_map(|m| {
        let (path, mode) = if let Some((p, m_mode)) = state.monitor_overrides.get(&m.name) {
            (p.clone(), m_mode.clone())
        } else if let Some(p) = &state.default_image {
            (p.clone(), state.current_mode.clone())
        } else {
            return None;
        };

        let job_blur = state.per_monitor_blur.get(&m.name).copied().unwrap_or(state.current_blur);

        Some(MonitorJob {
            name: m.name.clone(),
             path,
             mode,
             blur: job_blur,
             wp_width: m.wallpaper.width,
             wp_height: m.wallpaper.height,
             bd_width: m.backdrop.width,
             bd_height: m.backdrop.height,
        })
    }).collect()
}

fn setup_pending_outputs(
    state: &mut State,
    event_queue: &mut EventQueue<State>,
    qh: &QueueHandle<State>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if state.pending_outputs.is_empty() {
        return Ok(false);
    }

    let compositor = state.compositor.as_ref().ok_or("no compositor")?;
    let layer_shell = state.layer_shell.as_ref().ok_or("no layer_shell")?;

    let pending_outputs = std::mem::take(&mut state.pending_outputs);

    for (global_id, output) in pending_outputs {
        let wp_surface = compositor.create_surface(qh, ());
        let wp_layer = layer_shell.get_layer_surface(&wp_surface, Some(&output), zwlr_layer_shell_v1::Layer::Background, "wallpaper".to_string(), qh, ());
        wp_layer.set_size(0, 0);
        wp_layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        wp_layer.set_exclusive_zone(-1);
        wp_layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);

        let bd_surface = compositor.create_surface(qh, ());
        let bd_layer = layer_shell.get_layer_surface(&bd_surface, Some(&output), zwlr_layer_shell_v1::Layer::Background, "wallman-backdrop".to_string(), qh, ());
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

        let name = state.monitor_names.get(&output.id()).cloned().unwrap_or_else(|| format!("output-{}", output.id()));

        state.monitors.push(Monitor {
            global_id,
            name,
            _output: output,
            wallpaper: wp_state,
            backdrop: bd_state,
        });
    }

    for monitor in &state.monitors {
        if monitor.wallpaper.configure_serial.is_none() {
            if let Some(s) = monitor.wallpaper.surface.as_ref() { s.commit(); }
        }
        if monitor.backdrop.configure_serial.is_none() {
            if let Some(s) = monitor.backdrop.surface.as_ref() { s.commit(); }
        }
    }

    let mut attempts = 0;
    while state.monitors.iter().any(|m| m.wallpaper.configure_serial.is_none() || m.backdrop.configure_serial.is_none()) && attempts < 10 {
        event_queue.roundtrip(state)?;
        attempts += 1;
    }

    for monitor in &mut state.monitors {
        if let Some(serial) = monitor.wallpaper.configure_serial.take() {
            if let Some(ls) = monitor.wallpaper.layer_surface.as_ref() { ls.ack_configure(serial); }
        }
        if let Some(serial) = monitor.backdrop.configure_serial.take() {
            if let Some(ls) = monitor.backdrop.layer_surface.as_ref() { ls.ack_configure(serial); }
        }
    }

    let shm = state.shm.as_ref().ok_or("wl_shm not bound")?.clone();
    for monitor in &mut state.monitors {
        if monitor.wallpaper.buffer.is_none() {
            let w = monitor.wallpaper.width;
            let h = monitor.wallpaper.height;
            let size = (w as usize) * (h as usize) * 4;
            if size > 0 {
                if let Ok(memfd) = worker::create_shm_file("wallman-solid", size) {
                    let _ = prepare_surface(&mut monitor.wallpaper, &shm, qh, memfd, w, h);
                }
            }
        }
        if monitor.backdrop.buffer.is_none() {
            let w = monitor.backdrop.width;
            let h = monitor.backdrop.height;
            let size = (w as usize) * (h as usize) * 4;
            if size > 0 {
                if let Ok(memfd) = worker::create_shm_file("wallman-solid", size) {
                    let _ = prepare_surface(&mut monitor.backdrop, &shm, qh, memfd, w, h);
                }
            }
        }
    }

    Ok(true)
}

pub fn run(
    image: impl AsRef<std::path::Path>,
    initial_mode: String,
    initial_blur: u32,
    initial_per_monitor_blur: HashMap<String, u32>,
    initial_overrides: HashMap<String, (std::path::PathBuf, String)>,
           receiver: calloop::channel::Channel<RendererCommand>,
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
        current_mode: initial_mode.clone(),
        default_image: Some(image.as_ref().to_path_buf()),
        monitor_overrides: initial_overrides,
        current_blur: initial_blur,
        loop_signal: None,
        per_monitor_blur: initial_per_monitor_blur,
        worker_tx: None,
    };

    event_queue.roundtrip(&mut state)?;
    event_queue.roundtrip(&mut state)?;

    if state.compositor.is_none() { return Err("wl_compositor global not found".into()); }
    if state.shm.is_none() { return Err("wl_shm global not found".into()); }
    if state.layer_shell.is_none() { return Err("zwlr_layer_shell_v1 global not found".into()); }

    setup_pending_outputs(&mut state, &mut event_queue, &qh)?;
    println!("created surfaces for {} monitor(s)", state.monitors.len());

    let cache_dir = directories::ProjectDirs::from("", "", "wallman")
    .map(|d| d.cache_dir().to_path_buf());

    let shm = state.shm.as_ref().ok_or("wl_shm not bound")?.clone();
    let mut cache_hits = 0;
    let total_monitors = state.monitors.len();

    if let Some(ref dir) = cache_dir {
        for monitor in &mut state.monitors {
            let (mode, blur) = if let Some((_, m)) = state.monitor_overrides.get(&monitor.name) {
                let monitor_blur = state.per_monitor_blur.get(&monitor.name).copied().unwrap_or(state.current_blur);
                (m.clone(), monitor_blur)
            } else {
                (state.current_mode.clone(), state.current_blur)
            };

            let wp_width = monitor.wallpaper.width;
            let wp_height = monitor.wallpaper.height;
            let bd_width = monitor.backdrop.width;
            let bd_height = monitor.backdrop.height;

            let mut wp_loaded = false;
            let mut bd_loaded = false;

            if let Some((wp_pixels, w, h)) = crate::cache::try_load_raw_cache(
                dir, &monitor.name, "wp", wp_width, wp_height, blur, &mode
            ) {
                if let Ok(mut memfd) = worker::create_shm_file("wallman-wp-cache", wp_pixels.len()) {
                    if memfd.write_all(&wp_pixels).is_ok() {
                        if prepare_surface(&mut monitor.wallpaper, &shm, &qh, memfd, w, h).is_ok() {
                            wp_loaded = true;
                        }
                    }
                }
            }

            if let Some((bd_pixels, w, h)) = crate::cache::try_load_raw_cache(
                dir, &monitor.name, "bd", bd_width, bd_height, blur, &mode
            ) {
                if let Ok(mut memfd) = worker::create_shm_file("wallman-bd-cache", bd_pixels.len()) {
                    if memfd.write_all(&bd_pixels).is_ok() {
                        if prepare_surface(&mut monitor.backdrop, &shm, &qh, memfd, w, h).is_ok() {
                            bd_loaded = true;
                        }
                    }
                }
            }

            if wp_loaded && bd_loaded {
                cache_hits += 1;
                println!("[{}] loaded cached wallpaper & backdrop instantly", monitor.name);
            }
        }
    }

    let all_cached = cache_hits == total_monitors && total_monitors > 0;

    if all_cached {
        println!("[renderer] all monitors loaded from cache, skipping initial worker pass");
    } else {
        println!("[renderer] cache miss, worker will process images");
    }

    let (worker_tx, worker_rx) = spawn_worker();
    state.worker_tx = Some(worker_tx.clone());

    if !all_cached {
        let jobs = build_jobs(&state);
        worker_tx.send(WorkerCommand::Process { jobs }).expect("worker thread crashed");

        match worker_rx.recv().expect("worker thread crashed") {
            WorkerResponse::Ready { colors, monitors: results } => {
                write_colors_file(&colors);
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
                println!("initial wallpapers loaded");
            }
            WorkerResponse::Failed(e) => {
                eprintln!("Failed to process initial wallpapers: {}", e);
            }
        }
    }

    event_queue.roundtrip(&mut state)?;
    println!("Wallman renderer is running (multi-monitor)");

    let mut event_loop: EventLoop<State> = EventLoop::try_new().unwrap();
    let handle = event_loop.handle();
    let loop_signal = event_loop.get_signal();

    state.loop_signal = Some(loop_signal.clone());

    let qh_worker = qh.clone();
    let shm_worker = shm.clone();
    let worker_tx_ipc = worker_tx.clone();
    let worker_tx_wl = worker_tx.clone();
    let qh_wl = qh.clone();
    let signal_wl = loop_signal.clone();
    let conn_idle = conn.clone();

    handle.insert_source(receiver, move |event, _, state| {
        if let calloop::channel::Event::Msg(command) = event {
            match command {
                RendererCommand::Reload => {
                    let jobs = build_jobs(state);
                    if !jobs.is_empty() {
                        let _ = worker_tx_ipc.send(WorkerCommand::Process { jobs });
                    }
                }
                RendererCommand::SetWallpaper { image, mode, monitor, blur } => {
                    if monitor.is_empty() {
                        state.default_image = Some(image.clone());
                        state.current_mode = mode.clone();
                        state.current_blur = blur;
                        state.monitor_overrides.clear();
                        state.per_monitor_blur.clear();
                    } else {
                        state.monitor_overrides.insert(monitor.clone(), (image.clone(), mode.clone()));
                        state.per_monitor_blur.insert(monitor.clone(), blur);
                    }
                    let jobs = build_jobs(state);
                    if !jobs.is_empty() {
                        let _ = worker_tx_ipc.send(WorkerCommand::Process { jobs });
                    }
                }
            }
        }
    }).unwrap();

    handle.insert_source(worker_rx, move |event, _, state| {
        match event {
            calloop::channel::Event::Msg(response) => {
                match response {
                    WorkerResponse::Ready { colors, monitors: results } => {
                        write_colors_file(&colors);
                        for result in results {
                            if let Some(monitor) = state.monitors.iter_mut().find(|m| m.name == result.name) {
                                if let Err(e) = prepare_surface(&mut monitor.wallpaper, &shm_worker, &qh_worker, result.wallpaper_file, result.wp_width, result.wp_height) {
                                    eprintln!("wp prep failed: {e}");
                                }
                                if let Err(e) = prepare_surface(&mut monitor.backdrop, &shm_worker, &qh_worker, result.backdrop_file, result.bd_width, result.bd_height) {
                                    eprintln!("bd prep failed: {e}");
                                }
                            }
                        }
                        println!("updated wallpaper (mode: {})", state.current_mode);
                    }
                    WorkerResponse::Failed(e) => eprintln!("process failed: {e}"),
                }
            }
            calloop::channel::Event::Closed => {
                eprintln!("Worker channel closed unexpectedly");
            }
        }
    }).unwrap();

    let signals = Signals::new(&[Signal::SIGTERM, Signal::SIGINT]).unwrap();
    handle.insert_source(signals, move |event, _, state| {
        let sig = event.signal();
        eprintln!("Received signal {:?}, shutting down gracefully...", sig);

        for monitor in &mut state.monitors {
            if let Some(ls) = monitor.wallpaper.layer_surface.as_ref() { ls.destroy(); }
            if let Some(s) = monitor.wallpaper.surface.as_ref() { s.destroy(); }
            if let Some(ls) = monitor.backdrop.layer_surface.as_ref() { ls.destroy(); }
            if let Some(s) = monitor.backdrop.surface.as_ref() { s.destroy(); }
        }

        if let Some(signal) = &state.loop_signal {
            signal.stop();
        }
    }).unwrap();

    let conn_source = conn.clone();
    handle.insert_source(
        Generic::new(conn_source, Interest::READ, Mode::Level),
                         move |_, _, state| {
                             if let Err(e) = event_queue.dispatch_pending(state) {
                                 eprintln!("Wayland dispatch error: {e}");
                                 signal_wl.stop();
                                 return Ok(PostAction::Continue);
                             }

                             if state.monitors.iter().any(|m| m.wallpaper.closed || m.backdrop.closed) {
                                 signal_wl.stop();
                                 return Ok(PostAction::Continue);
                             }

                             if !state.pending_outputs.is_empty() {
                                 if let Ok(true) = setup_pending_outputs(state, &mut event_queue, &qh_wl) {
                                     println!("Detected new monitor! Setting up surfaces...");
                                     let jobs = build_jobs(state);
                                     if !jobs.is_empty() {
                                         let _ = worker_tx_wl.send(WorkerCommand::Process { jobs });
                                     }
                                 }
                             }

                             let mut any_rerender_needed = false;

                             for monitor in &mut state.monitors {
                                 if monitor.wallpaper.needs_rerender || monitor.backdrop.needs_rerender {
                                     monitor.wallpaper.needs_rerender = false;
                                     monitor.backdrop.needs_rerender = false;
                                     any_rerender_needed = true;
                                 }

                                 if let Some(serial) = monitor.wallpaper.configure_serial.take() {
                                     if let Some(ls) = monitor.wallpaper.layer_surface.as_ref() { ls.ack_configure(serial); }
                                 }
                                 if let Some(serial) = monitor.backdrop.configure_serial.take() {
                                     if let Some(ls) = monitor.backdrop.layer_surface.as_ref() { ls.ack_configure(serial); }
                                 }
                             }

                             if any_rerender_needed {
                                 if let Some(tx) = &state.worker_tx {
                                     let jobs = build_jobs(&state);
                                     if !jobs.is_empty() {
                                         let _ = tx.send(WorkerCommand::Process { jobs });
                                         println!("Resolution changed, re-rendering...");
                                     }
                                 }
                             }

                             if let Some(read_guard) = event_queue.prepare_read() {
                                 if let Err(e) = read_guard.read() {
                                     eprintln!("Wayland read error: {e}");
                                     eprintln!("Wayland connection lost, exiting daemon.");
                                     std::process::exit(0);
                                 }
                             }

                             Ok(PostAction::Continue)
                         }
    ).unwrap();

    event_loop.run(None, &mut state, |_state| {
        let _ = conn_idle.flush();
    }).unwrap();

    Ok(())
}
