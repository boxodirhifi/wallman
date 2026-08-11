use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

#[derive(Clone)]
pub struct MonitorConfig {
    pub wp_width: u32,
    pub wp_height: u32,
    pub bd_width: u32,
    pub bd_height: u32,
}

pub struct MonitorResult {
    pub wallpaper_file: File,
    pub backdrop_file: File,
    pub config: MonitorConfig,
}

pub enum WorkerCommand {
    Process {
        path: PathBuf,
        mode: String,
        monitors: Vec<MonitorConfig>,
    },
}

pub enum WorkerResponse {
    Ready {
        colors: Vec<(u8, u8, u8)>,
        mode: String,
        monitors: Vec<MonitorResult>,
    },
    Failed(String),
}

pub fn spawn_worker() -> (mpsc::Sender<WorkerCommand>, mpsc::Receiver<WorkerResponse>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (resp_tx, resp_rx) = mpsc::channel();

    thread::spawn(move || {
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                WorkerCommand::Process { path, mode, monitors } => {
                    match process_images(&path, &mode, &monitors) {
                        Ok((colors, monitor_results)) => {
                            let _ = resp_tx.send(WorkerResponse::Ready {
                                colors,
                                mode,
                                monitors: monitor_results,
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

fn process_images(
    path: &std::path::Path,
    mode: &str,
    monitors: &[MonitorConfig],
) -> Result<(Vec<(u8, u8, u8)>, Vec<MonitorResult>), Box<dyn std::error::Error>> {
    let image = image::open(path)
    .map_err(|e| format!("failed to open {}: {e}", path.display()))?;

    // Extract colors once from a reasonably sized version for speed
    let color_img = image.resize(800, 600, image::imageops::FilterType::Nearest);
    let colors = extract_colors(&color_img.to_rgba8());

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

    let mut results = Vec::with_capacity(monitors.len());

    for config in monitors {
        let sharp_rgba = resize(&image, config.wp_width, config.wp_height);
        let wp_pixels = rgba_to_xrgb(&sharp_rgba);

        let backdrop_rgba = resize(&image, config.bd_width, config.bd_height);
        let blurred_rgba = image::imageops::blur(&backdrop_rgba, 8.0);
        let bd_pixels = rgba_to_xrgb(&blurred_rgba);

        let mut wp_file = create_shm_file("wallman-wallpaper", wp_pixels.len())?;
        wp_file.write_all(&wp_pixels)?;
        wp_file.flush()?;

        let mut bd_file = create_shm_file("wallman-backdrop", bd_pixels.len())?;
        bd_file.write_all(&bd_pixels)?;
        bd_file.flush()?;

        results.push(MonitorResult {
            wallpaper_file: wp_file,
            backdrop_file: bd_file,
            config: config.clone(),
        });
    }

    Ok((colors, results))
}

fn extract_colors(rgba: &image::RgbaImage) -> Vec<(u8, u8, u8)> {
    let pixels = rgba.as_raw();

    match color_thief::get_palette(
        pixels,
        color_thief::ColorFormat::Rgba,
        5,
        5,
    ) {
        Ok(palette) => palette.iter().map(|c| (c.r, c.g, c.b)).collect(),
        Err(e) => {
            eprintln!("Color extraction failed: {e}");
            Vec::new()
        }
    }
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
