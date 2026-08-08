use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};

use wayland_client::{
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
    Connection, Dispatch, EventQueue, QueueHandle,
};

use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

struct State {
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    layer_shell: Option<ZwlrLayerShellV1>,
    surface: Option<WlSurface>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,

    pool: Option<WlShmPool>,
    buffer: Option<WlBuffer>,
    file: Option<File>,

    configure_serial: Option<u32>,
    width: u32,
    height: u32,
    closed: bool,
}

fn create_shm_file(size: usize) -> Result<File, Box<dyn std::error::Error>> {
    let name = CString::new("wallman-backdrop")?;

    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };

    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    let file = unsafe { File::from_raw_fd(fd) };
    file.set_len(u64::try_from(size)?)?;

    Ok(file)
}

fn load_image(
    path: impl AsRef<std::path::Path>,
    expected_width: u32,
    expected_height: u32,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let path = path.as_ref();

    let image = image::open(path)
    .map_err(|e| format!("failed to open {}: {e}", path.display()))?;

    let image = image.resize_to_fill(
        expected_width,
        expected_height,
        image::imageops::FilterType::Lanczos3,
    );

    let rgba = image.to_rgba8();

    let mut pixels = Vec::with_capacity(
        usize::try_from(expected_width)?
        .checked_mul(usize::try_from(expected_height)?)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or("XRGB buffer size overflow")?,
    );

    for chunk in rgba.chunks_exact(4) {
        let r = chunk[0] as u32;
        let g = chunk[1] as u32;
        let b = chunk[2] as u32;

        let pixel = (r << 16) | (g << 8) | b;

        pixels.extend_from_slice(&pixel.to_ne_bytes());
    }

    Ok(pixels)
}

fn prepare_image(
    state: &mut State,
    qh: &QueueHandle<State>,
    image: impl AsRef<std::path::Path>,
    width: u32,
    height: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let width_i32 = i32::try_from(width)?;
    let height_i32 = i32::try_from(height)?;

    let stride = usize::try_from(width)?
    .checked_mul(4)
    .ok_or("stride overflow")?;

    let expected_size = stride
    .checked_mul(usize::try_from(height)?)
    .ok_or("buffer size overflow")?;

    let stride_i32 = i32::try_from(stride)?;

    let image_data = load_image(image.as_ref(), width, height)?;

    if image_data.len() != expected_size {
        return Err("loaded image data does not match the expected buffer size".into());
    }

    let mut file = create_shm_file(expected_size)?;
    file.write_all(&image_data)?;
    file.flush()?;

    let size_i32 = i32::try_from(expected_size)?;

    let pool: WlShmPool = {
        let shm = state.shm.as_ref().ok_or("wl_shm not bound")?;
        shm.create_pool(file.as_fd(), size_i32, qh, ())
    };

    let buffer: WlBuffer = pool.create_buffer(
        0,
        width_i32,
        height_i32,
        stride_i32,
        wl_shm::Format::Xrgb8888,
        qh,
        (),
    );

    if let Some(surface) = state.surface.as_ref() {
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, width_i32, height_i32);
        surface.commit();
    } else {
        return Err("wl_surface not created".into());
    }

    state.pool = Some(pool);
    state.buffer = Some(buffer);
    state.file = Some(file);

    Ok(())
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut State,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _data: &(),
             _conn: &Connection,
             qh: &QueueHandle<State>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                println!("{name}: {interface} v{version}");

                if interface == "wl_compositor" && state.compositor.is_none() {
                    let bind_version = version.min(6);

                    let compositor: WlCompositor =
                    registry.bind(name, bind_version, qh, ());

                    state.compositor = Some(compositor);
                    println!("bound: wl_compositor v{bind_version}");
                }

                if interface == "wl_shm" && state.shm.is_none() {
                    let bind_version = version.min(2);

                    let shm: WlShm =
                    registry.bind(name, bind_version, qh, ());

                    state.shm = Some(shm);
                    println!("bound: wl_shm v{bind_version}");
                }

                if interface == "zwlr_layer_shell_v1" && state.layer_shell.is_none() {
                    let bind_version = version.min(5);

                    let layer_shell: ZwlrLayerShellV1 =
                    registry.bind(name, bind_version, qh, ());

                    state.layer_shell = Some(layer_shell);
                    println!("bound: zwlr_layer_shell_v1 v{bind_version}");
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                println!("{name}: removed");
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCompositor, ()> for State {
    fn event(
        _state: &mut State,
        _compositor: &WlCompositor,
        _event: wl_compositor::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<WlShm, ()> for State {
    fn event(
        _state: &mut State,
        _shm: &WlShm,
        _event: wl_shm::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<WlShmPool, ()> for State {
    fn event(
        _state: &mut State,
        _pool: &WlShmPool,
        _event: wl_shm_pool::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<WlBuffer, ()> for State {
    fn event(
        _state: &mut State,
        _buffer: &WlBuffer,
        _event: wl_buffer::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for State {
    fn event(
        _state: &mut State,
        _surface: &WlSurface,
        _event: wl_surface::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(
        _state: &mut State,
        _output: &WlOutput,
        _event: wl_output::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for State {
    fn event(
        _state: &mut State,
        _layer_shell: &ZwlrLayerShellV1,
        _event: zwlr_layer_shell_v1::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut State,
        _layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                println!("layer surface configure: serial={serial} width={width} height={height}");

                state.configure_serial = Some(serial);
                state.width = width;
                state.height = height;
            }
            zwlr_layer_surface_v1::Event::Closed => {
                println!("layer surface closed");
                state.closed = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, ()> for State {
    fn event(
        _state: &mut State,
        _callback: &WlCallback,
        _event: wl_callback::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
    }
}

pub fn run(
    image: impl AsRef<std::path::Path>,
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
        surface: None,
        layer_surface: None,
        pool: None,
        buffer: None,
        file: None,
        configure_serial: None,
        width: 0,
        height: 0,
        closed: false,
    };

    event_queue.roundtrip(&mut state)?;
    event_queue.roundtrip(&mut state)?;

    if state.compositor.is_none() {
        return Err("wl_compositor global not found".into());
    }

    if state.shm.is_none() {
        return Err("wl_shm global not found".into());
    }

    if state.layer_shell.is_none() {
        return Err("zwlr_layer_shell_v1 global not found".into());
    }

    let surface: WlSurface = {
        let compositor = state
        .compositor
        .as_ref()
        .ok_or("wl_compositor not bound")?;

        compositor.create_surface(&qh, ())
    };

    state.surface = Some(surface);
    println!("created wl_surface");

    let namespace = std::env::var("WALLMAN_NAMESPACE")
    .unwrap_or_else(|_| String::from("wallman-backdrop"));

    println!("using namespace: {namespace}");

    let layer_surface: ZwlrLayerSurfaceV1 = {
        let layer_shell = state
        .layer_shell
        .as_ref()
        .ok_or("zwlr_layer_shell_v1 not bound")?;

        let surface = state
        .surface
        .as_ref()
        .ok_or("wl_surface not created")?;

        layer_shell.get_layer_surface(
            surface,
            None::<&WlOutput>,
            zwlr_layer_shell_v1::Layer::Background,
            namespace,
            &qh,
            (),
        )
    };

    layer_surface.set_size(0, 0);

    layer_surface.set_anchor(
        Anchor::Top
        | Anchor::Bottom
        | Anchor::Left
        | Anchor::Right,
    );

    layer_surface.set_exclusive_zone(-1);

    layer_surface.set_keyboard_interactivity(
        zwlr_layer_surface_v1::KeyboardInteractivity::None,
    );

    state.layer_surface = Some(layer_surface);
    println!("created zwlr_layer_surface_v1");

    event_queue.roundtrip(&mut state)?;

    if state.configure_serial.is_none() {
        if let Some(surface) = state.surface.as_ref() {
            surface.commit();
            println!("committed empty wl_surface to request configure");
        }

        event_queue.roundtrip(&mut state)?;
    }

    let mut attempts = 0;
    while state.configure_serial.is_none() && attempts < 10 {
        event_queue.roundtrip(&mut state)?;
        attempts += 1;
    }

    let serial = state
    .configure_serial
    .take()
    .ok_or("no configure event received")?;

    let width = state.width;
    let height = state.height;

    if width == 0 || height == 0 {
        return Err("configure event gave a zero size".into());
    }

    if let Some(layer_surface) = state.layer_surface.as_ref() {
        layer_surface.ack_configure(serial);
        println!("acknowledged configure serial={serial}");
    }

    prepare_image(
        &mut state,
        &qh,
        image.as_ref(),
                  width,
                  height,
    )?;
    println!(
        "drew initial image from {}",
        image.as_ref().display()
    );

    event_queue.roundtrip(&mut state)?;

    println!("Backdrop renderer is running");

    loop {
        if state.closed {
            break;
        }

        event_queue.dispatch_pending(&mut state)?;
        conn.flush()?;

        if let Some(serial) = state.configure_serial.take() {
            if let Some(layer_surface) = state.layer_surface.as_ref() {
                layer_surface.ack_configure(serial);
                println!("acknowledged configure serial={serial}");
            }
        }

        let Some(read_guard) = event_queue.prepare_read() else {
            continue;
        };

        let fd = read_guard.connection_fd();
        let mut fds = [libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];

        let timeout_ms = 200;
        let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, timeout_ms) };

        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                drop(read_guard);
                continue;
            }
            drop(read_guard);
            return Err(err.into());
        }

        if ret > 0 {
            read_guard.read()?;
        } else {
            drop(read_guard);
        }
    }

    Ok(())
}
