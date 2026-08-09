use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use std::sync::mpsc::Receiver;

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

pub enum RendererCommand {
    SetWallpaper {
        image: std::path::PathBuf,
        mode: String,
    },
    Reload,
}

// ── Per-surface state ──────────────────────────────────────────────

struct SurfaceState {
    namespace: String,
    surface: Option<WlSurface>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    pool: Option<WlShmPool>,
    buffer: Option<WlBuffer>,
    file: Option<File>,
    buffer_released: bool,
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
            buffer_released: true,
            configure_serial: None,
            width: 0,
            height: 0,
            closed: false,
        }
    }
}

// ── Global state ───────────────────────────────────────────────────

struct State {
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    layer_shell: Option<ZwlrLayerShellV1>,
    wallpaper: SurfaceState,
    backdrop: SurfaceState,
    current_mode: String,
}

// ── Image processing pipeline ──────────────────────────────────────

struct ProcessedImages {
    wallpaper_pixels: Vec<u8>,
    backdrop_pixels: Vec<u8>,
}

fn rgba_to_xrgb(rgba: &image::RgbaImage) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(rgba.len());

    for chunk in rgba.chunks_exact(4) {
        let r = chunk[0] as u32;
        let g = chunk[1] as u32;
        let b = chunk[2] as u32;

        let pixel = (r << 16) | (g << 8) | b;
        pixels.extend_from_slice(&pixel.to_ne_bytes());
    }

    pixels
}

fn process_image(
    path: impl AsRef<std::path::Path>,
    mode: &str,
    wp_width: u32,
    wp_height: u32,
    bd_width: u32,
    bd_height: u32,
) -> Result<ProcessedImages, Box<dyn std::error::Error>> {
    let path = path.as_ref();

    let image = image::open(path)
    .map_err(|e| format!("failed to open {}: {e}", path.display()))?;

    let resize = |img: &image::DynamicImage, w: u32, h: u32| -> image::RgbaImage {
        match mode {
            "fit" => {
                let resized = img.resize(w, h, image::imageops::FilterType::Lanczos3).to_rgba8();
                let mut bg = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
                let x = (w.saturating_sub(resized.width())) / 2;
                let y = (h.saturating_sub(resized.height())) / 2;
                image::imageops::overlay(&mut bg, &resized, x as i64, y as i64);
                bg
            }
            "stretch" => {
                img.resize_exact(w, h, image::imageops::FilterType::Lanczos3).to_rgba8()
            }
            "center" => {
                let resized = img.to_rgba8();
                let mut bg = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
                let x = (w as i32 - resized.width() as i32) / 2;
                let y = (h as i32 - resized.height() as i32) / 2;
                image::imageops::overlay(&mut bg, &resized, x as i64, y as i64);
                bg
            }
            _ => { // "fill" or unknown
                img.resize_to_fill(w, h, image::imageops::FilterType::Lanczos3).to_rgba8()
            }
        }
    };

    // ── Sharp wallpaper ────────────────────────────────────────────
    let sharp_rgba = resize(&image, wp_width, wp_height);
    let wallpaper_pixels = rgba_to_xrgb(&sharp_rgba);

    // ── Blurred backdrop ───────────────────────────────────────────
    let backdrop_rgba = resize(&image, bd_width, bd_height);
    let blurred_rgba = image::imageops::blur(&backdrop_rgba, 8.0);
    let backdrop_pixels = rgba_to_xrgb(&blurred_rgba);

    Ok(ProcessedImages {
        wallpaper_pixels,
       backdrop_pixels,
    })
}

// ── Wayland helpers ────────────────────────────────────────────────

fn create_shm_file(
    name: &str,
    size: usize,
) -> Result<File, Box<dyn std::error::Error>> {
    let cname = CString::new(name)?;

    let fd = unsafe { libc::memfd_create(cname.as_ptr(), libc::MFD_CLOEXEC) };

    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    let file = unsafe { File::from_raw_fd(fd) };
    file.set_len(u64::try_from(size)?)?;

    Ok(file)
}

fn prepare_surface(
    ss: &mut SurfaceState,
    shm: &WlShm,
    qh: &QueueHandle<State>,
    pixels: &[u8],
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

    if pixels.len() != expected_size {
        return Err("pixel data does not match the expected buffer size".into());
    }

    let mut file = create_shm_file(&ss.namespace, expected_size)?;
    file.write_all(pixels)?;
    file.flush()?;

    let size_i32 = i32::try_from(expected_size)?;

    let pool: WlShmPool = shm.create_pool(file.as_fd(), size_i32, qh, ());

    let buffer: WlBuffer = pool.create_buffer(
        0,
        width_i32,
        height_i32,
        stride_i32,
        wl_shm::Format::Xrgb8888,
        qh,
        (),
    );

    if let Some(surface) = ss.surface.as_ref() {
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, width_i32, height_i32);
        surface.commit();
    } else {
        return Err("wl_surface not created".into());
    }

    ss.pool = Some(pool);
    ss.buffer = Some(buffer);
    ss.file = Some(file);
    ss.buffer_released = false;

    Ok(())
}

// ── Dispatch implementations ───────────────────────────────────────

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
        state: &mut State,
        buffer: &WlBuffer,
        event: wl_buffer::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
        if let wl_buffer::Event::Release = event {
            let is_wallpaper = state
            .wallpaper
            .buffer
            .as_ref()
            .map(|b| b == buffer)
            .unwrap_or(false);

            if is_wallpaper {
                state.wallpaper.buffer_released = true;
            } else {
                state.backdrop.buffer_released = true;
            }
        }
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
        layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
             _conn: &Connection,
             _qh: &QueueHandle<State>,
    ) {
        let is_wallpaper = state
        .wallpaper
        .layer_surface
        .as_ref()
        .map(|ls| ls == layer_surface)
        .unwrap_or(false);

        let ss = if is_wallpaper {
            &mut state.wallpaper
        } else {
            &mut state.backdrop
        };

        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                println!(
                    "[{}] configure: serial={serial} width={width} height={height}",
                    ss.namespace
                );
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

// ── Renderer entry point ───────────────────────────────────────────

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
        wallpaper: SurfaceState::new("wallpaper"),
        backdrop: SurfaceState::new("wallman-backdrop"),
        current_mode: initial_mode,
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

    // ── Create wallpaper surface ───────────────────────────────────
    {
        let surface = {
            let compositor = state
            .compositor
            .as_ref()
            .ok_or("wl_compositor not bound")?;
            compositor.create_surface(&qh, ())
        };
        state.wallpaper.surface = Some(surface);
    }
    {
        let layer_surface = {
            let layer_shell = state
            .layer_shell
            .as_ref()
            .ok_or("zwlr_layer_shell_v1 not bound")?;
            let surface = state
            .wallpaper
            .surface
            .as_ref()
            .ok_or("wallpaper wl_surface not created")?;

            layer_shell.get_layer_surface(
                surface,
                None::<&WlOutput>,
                zwlr_layer_shell_v1::Layer::Background,
                state.wallpaper.namespace.clone(),
                                          &qh,
                                          (),
            )
        };

        layer_surface.set_size(0, 0);
        layer_surface.set_anchor(
            Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        );
        layer_surface.set_exclusive_zone(-1);
        layer_surface.set_keyboard_interactivity(
            zwlr_layer_surface_v1::KeyboardInteractivity::None,
        );

        state.wallpaper.layer_surface = Some(layer_surface);
        println!("[wallpaper] created layer surface");
    }

    // ── Create backdrop surface ────────────────────────────────────
    {
        let surface = {
            let compositor = state
            .compositor
            .as_ref()
            .ok_or("wl_compositor not bound")?;
            compositor.create_surface(&qh, ())
        };
        state.backdrop.surface = Some(surface);
    }
    {
        let layer_surface = {
            let layer_shell = state
            .layer_shell
            .as_ref()
            .ok_or("zwlr_layer_shell_v1 not bound")?;
            let surface = state
            .backdrop
            .surface
            .as_ref()
            .ok_or("backdrop wl_surface not created")?;

            layer_shell.get_layer_surface(
                surface,
                None::<&WlOutput>,
                zwlr_layer_shell_v1::Layer::Background,
                state.backdrop.namespace.clone(),
                                          &qh,
                                          (),
            )
        };

        layer_surface.set_size(0, 0);
        layer_surface.set_anchor(
            Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        );
        layer_surface.set_exclusive_zone(-1);
        layer_surface.set_keyboard_interactivity(
            zwlr_layer_surface_v1::KeyboardInteractivity::None,
        );

        state.backdrop.layer_surface = Some(layer_surface);
        println!("[wallman-backdrop] created layer surface");
    }

    // ── Commit both empty surfaces to trigger configure ────────────
    if let Some(ref surface) = state.wallpaper.surface {
        surface.commit();
    }
    if let Some(ref surface) = state.backdrop.surface {
        surface.commit();
    }
    println!("committed empty surfaces to request configure");

    event_queue.roundtrip(&mut state)?;

    // ── Wait for wallpaper configure ───────────────────────────────
    let mut attempts = 0;
    while state.wallpaper.configure_serial.is_none() && attempts < 10 {
        event_queue.roundtrip(&mut state)?;
        attempts += 1;
    }

    let wp_serial = state
    .wallpaper
    .configure_serial
    .take()
    .ok_or("[wallpaper] no configure event received")?;
    let wp_width = state.wallpaper.width;
    let wp_height = state.wallpaper.height;

    if wp_width == 0 || wp_height == 0 {
        return Err("[wallpaper] configure event gave a zero size".into());
    }

    if let Some(ref ls) = state.wallpaper.layer_surface {
        ls.ack_configure(wp_serial);
        println!("[wallpaper] acknowledged configure serial={wp_serial}");
    }

    // ── Wait for backdrop configure ────────────────────────────────
    let mut attempts = 0;
    while state.backdrop.configure_serial.is_none() && attempts < 10 {
        event_queue.roundtrip(&mut state)?;
        attempts += 1;
    }

    let bd_serial = state
    .backdrop
    .configure_serial
    .take()
    .ok_or("[wallman-backdrop] no configure event received")?;
    let bd_width = state.backdrop.width;
    let bd_height = state.backdrop.height;

    if bd_width == 0 || bd_height == 0 {
        return Err("[wallman-backdrop] configure event gave a zero size".into());
    }

    if let Some(ref ls) = state.backdrop.layer_surface {
        ls.ack_configure(bd_serial);
        println!("[wallman-backdrop] acknowledged configure serial={bd_serial}");
    }

    // ── Process initial image (strict: crash if cached image is bad) ──
    let shm = state
    .shm
    .as_ref()
    .ok_or("wl_shm not bound")?
    .clone();

    let images = process_image(
        image.as_ref(),
                               &state.current_mode,
                               wp_width,
                               wp_height,
                               bd_width,
                               bd_height,
    )?;

    prepare_surface(
        &mut state.wallpaper,
        &shm,
        &qh,
        &images.wallpaper_pixels,
        wp_width,
        wp_height,
    )?;
    println!(
        "[wallpaper] drew sharp image from {} (mode: {})",
             image.as_ref().display(),
             state.current_mode
    );

    prepare_surface(
        &mut state.backdrop,
        &shm,
        &qh,
        &images.backdrop_pixels,
        bd_width,
        bd_height,
    )?;
    println!(
        "[wallman-backdrop] drew blurred image from {} (mode: {})",
             image.as_ref().display(),
             state.current_mode
    );

    event_queue.roundtrip(&mut state)?;

    println!("Wallman renderer is running (wallpaper + blurred backdrop)");

    // ── Event loop ─────────────────────────────────────────────────
    loop {
        if state.wallpaper.closed || state.backdrop.closed {
            break;
        }

        while let Ok(command) = receiver.try_recv() {
            match command {
                RendererCommand::Reload => {
                    let shm = match state.shm.as_ref() {
                        Some(s) => s.clone(),
                        None => {
                            eprintln!("Failed to reload: wl_shm not bound");
                            continue;
                        }
                    };

                    let wp_w = state.wallpaper.width;
                    let wp_h = state.wallpaper.height;
                    let bd_w = state.backdrop.width;
                    let bd_h = state.backdrop.height;

                    match process_image(&image, &state.current_mode, wp_w, wp_h, bd_w, bd_h) {
                        Ok(images) => {
                            if let Err(e) = prepare_surface(
                                &mut state.wallpaper,
                                &shm,
                                &qh,
                                &images.wallpaper_pixels,
                                wp_w,
                                wp_h,
                            ) {
                                eprintln!("Failed to prepare wallpaper surface: {e}");
                                continue;
                            }
                            if let Err(e) = prepare_surface(
                                &mut state.backdrop,
                                &shm,
                                &qh,
                                &images.backdrop_pixels,
                                bd_w,
                                bd_h,
                            ) {
                                eprintln!("Failed to prepare backdrop surface: {e}");
                                continue;
                            }

                            println!(
                                "reloaded wallpaper from {} (mode: {})",
                                     image.as_ref().display(),
                                     state.current_mode
                            );
                        }
                        Err(e) => {
                            eprintln!(
                                "Failed to reload image {}: {e}",
                                image.as_ref().display()
                            );
                        }
                    }
                }
                RendererCommand::SetWallpaper { image, mode } => {
                    let shm = match state.shm.as_ref() {
                        Some(s) => s.clone(),
                        None => {
                            eprintln!("Failed to set wallpaper: wl_shm not bound");
                            continue;
                        }
                    };

                    let wp_w = state.wallpaper.width;
                    let wp_h = state.wallpaper.height;
                    let bd_w = state.backdrop.width;
                    let bd_h = state.backdrop.height;

                    match process_image(&image, &mode, wp_w, wp_h, bd_w, bd_h) {
                        Ok(images) => {
                            if let Err(e) = prepare_surface(
                                &mut state.wallpaper,
                                &shm,
                                &qh,
                                &images.wallpaper_pixels,
                                wp_w,
                                wp_h,
                            ) {
                                eprintln!("Failed to prepare wallpaper surface: {e}");
                                continue;
                            }
                            if let Err(e) = prepare_surface(
                                &mut state.backdrop,
                                &shm,
                                &qh,
                                &images.backdrop_pixels,
                                bd_w,
                                bd_h,
                            ) {
                                eprintln!("Failed to prepare backdrop surface: {e}");
                                continue;
                            }

                            // Only update mode after successful draw
                            state.current_mode = mode;

                            println!(
                                "updated wallpaper from {} (mode: {})",
                                     image.display(),
                                     state.current_mode
                            );
                        }
                        Err(e) => {
                            eprintln!("Failed to process image {}: {e}", image.display());
                        }
                    }
                }
            }
        }

        event_queue.dispatch_pending(&mut state)?;
        conn.flush()?;

        // Ack any pending configure events
        if let Some(serial) = state.wallpaper.configure_serial.take() {
            if let Some(ref ls) = state.wallpaper.layer_surface {
                ls.ack_configure(serial);
                println!("[wallpaper] acknowledged configure serial={serial}");
            }
        }
        if let Some(serial) = state.backdrop.configure_serial.take() {
            if let Some(ref ls) = state.backdrop.layer_surface {
                ls.ack_configure(serial);
                println!("[wallman-backdrop] acknowledged configure serial={serial}");
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
