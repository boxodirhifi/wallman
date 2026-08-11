use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

// ── Worker messages ────────────────────────────────────────────────

pub enum WorkerCommand {
    Process {
        path: PathBuf,
        mode: String,
        wp_width: u32,
        wp_height: u32,
        bd_width: u32,
        bd_height: u32,
    },
}

pub enum WorkerResponse {
    Ready {
        wallpaper_file: File,
        backdrop_file: File,
        wp_width: u32,
        wp_height: u32,
        bd_width: u32,
        bd_height: u32,
        mode: String,
    },
    Failed(String),
}

// ── Worker thread entry point ──────────────────────────────────────

pub fn spawn_worker() -> (mpsc::Sender<WorkerCommand>, mpsc::Receiver<WorkerResponse>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (resp_tx, resp_rx) = mpsc::channel();

    thread::spawn(move || {
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                WorkerCommand::Process {
                    path,
                    mode,
                    wp_width,
                    wp_height,
                    bd_width,
                    bd_height,
                } => {
                    match process_image_to_files(
                        &path, &mode, wp_width, wp_height, bd_width, bd_height,
                    ) {
                        Ok((wp_file, bd_file)) => {
                            let _ = resp_tx.send(WorkerResponse::Ready {
                                wallpaper_file: wp_file,
                                backdrop_file: bd_file,
                                wp_width,
                                wp_height,
                                bd_width,
                                bd_height,
                                mode,
                            });
                        }
                        Err(e) => {
                            let _ = resp_tx.send(WorkerResponse::Failed(e.to_string()));
                        }
                    }
                }
            }
        }
    });

    (cmd_tx, resp_rx)
}

// ── Image processing pipeline (runs on worker thread) ─────────────

fn process_image_to_files(
    path: &std::path::Path,
    mode: &str,
    wp_width: u32,
    wp_height: u32,
    bd_width: u32,
    bd_height: u32,
) -> Result<(File, File), Box<dyn std::error::Error>> {
    let image = image::open(path)
        .map_err(|e| format!("failed to open {}: {e}", path.display()))?;

    let resize = |img: &image::DynamicImage, w: u32, h: u32| -> image::RgbaImage {
        match mode {
            "fit" => {
                let resized = img
                    .resize(w, h, image::imageops::FilterType::Lanczos3)
                    .to_rgba8();
                let mut bg = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
                let x = (w.saturating_sub(resized.width())) / 2;
                let y = (h.saturating_sub(resized.height())) / 2;
                image::imageops::overlay(&mut bg, &resized, x as i64, y as i64);
                bg
            }
            "stretch" => img
                .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
                .to_rgba8(),
            "center" => {
                let resized = img.to_rgba8();
                let mut bg = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
                let x = (w as i32 - resized.width() as i32) / 2;
                let y = (h as i32 - resized.height() as i32) / 2;
                image::imageops::overlay(&mut bg, &resized, x as i64, y as i64);
                bg
            }
            _ => img
                .resize_to_fill(w, h, image::imageops::FilterType::Lanczos3)
                .to_rgba8(),
        }
    };

    // ── Sharp wallpaper ────────────────────────────────────────────
    let sharp_rgba = resize(&image, wp_width, wp_height);
    let wallpaper_pixels = rgba_to_xrgb(&sharp_rgba);

    // ── Blurred backdrop ───────────────────────────────────────────
    let backdrop_rgba = resize(&image, bd_width, bd_height);
    let blurred_rgba = image::imageops::blur(&backdrop_rgba, 8.0);
    let backdrop_pixels = rgba_to_xrgb(&blurred_rgba);

    // ── Write to shared memory files ───────────────────────────────
    let mut wp_file = create_shm_file("wallman-wallpaper", wallpaper_pixels.len())?;
    wp_file.write_all(&wallpaper_pixels)?;
    wp_file.flush()?;

    let mut bd_file = create_shm_file("wallman-backdrop", backdrop_pixels.len())?;
    bd_file.write_all(&backdrop_pixels)?;
    bd_file.flush()?;

    Ok((wp_file, bd_file))
}

// ── Helpers ────────────────────────────────────────────────────────

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

fn create_shm_file(name: &str, size: usize) -> Result<File, Box<dyn std::error::Error>> {
    let cname = CString::new(name)?;

    let fd = unsafe { libc::memfd_create(cname.as_ptr(), libc::MFD_CLOEXEC) };

    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    let file = unsafe { File::from_raw_fd(fd) };
    file.set_len(u64::try_from(size)?)?;

    Ok(file)
}
