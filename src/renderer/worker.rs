use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::collections::HashMap;
use image::GenericImageView;

#[derive(Clone)]
pub struct MonitorJob {
    pub name: String,
    pub path: PathBuf,
    pub mode: String,
    pub blur: u32,
    pub wp_width: u32,
    pub wp_height: u32,
    pub bd_width: u32,
    pub bd_height: u32,
}

pub struct MonitorResult {
    pub name: String,
    pub wallpaper_file: File,
    pub backdrop_file: File,
    pub wp_width: u32,
    pub wp_height: u32,
    pub bd_width: u32,
    pub bd_height: u32,
}

pub enum WorkerCommand {
    Process {
        jobs: Vec<MonitorJob>,
    },
}

pub enum WorkerResponse {
    Ready {
        colors: Vec<(u8, u8, u8)>,
        monitors: Vec<MonitorResult>,
    },
    Failed(String),
}

pub fn spawn_worker() -> (mpsc::Sender<WorkerCommand>, calloop::channel::Channel<WorkerResponse>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (resp_tx, resp_rx) = calloop::channel::channel();

    thread::spawn(move || {
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                WorkerCommand::Process { jobs } => {
                    match process_images(&jobs) {
                        Ok((colors, monitor_results)) => {
                            let _ = resp_tx.send(WorkerResponse::Ready {
                                colors,
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
    jobs: &[MonitorJob],
) -> Result<(Vec<(u8, u8, u8)>, Vec<MonitorResult>), Box<dyn std::error::Error>> {
    if jobs.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // ── 1. Decode each unique image path only once ─────────────────
    let mut decoded_images: HashMap<std::path::PathBuf, image::DynamicImage> = HashMap::new();

    for job in jobs {
        if !decoded_images.contains_key(&job.path) {
            let img = image::open(&job.path)
            .map_err(|e| format!("failed to open {}: {e}", job.path.display()))?;
            decoded_images.insert(job.path.clone(), img);
        }
    }

    // ── 2. Extract colors from the first job's image ───────────────
    let first_path = &jobs[0].path;
    let first_image = decoded_images.get(first_path).unwrap();
    let color_img = first_image.resize(800, 600, image::imageops::FilterType::Nearest);
    let colors = extract_colors(&color_img.to_rgba8());

    let mut results = Vec::with_capacity(jobs.len());

    // ── 3. Process each monitor using the cached image ─────────────
    for job in jobs {
        let image = decoded_images.get(&job.path).unwrap();

        let resize = |img: &image::DynamicImage, w: u32, h: u32| -> image::RgbaImage {
            match job.mode.as_str() {
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
                    let (img_w, img_h) = img.dimensions();
                    let mut bg = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));

                    // 1. Calculate crop boundaries if image is LARGER than screen
                    let crop_x = if img_w > w { (img_w - w) / 2 } else { 0 };
                    let crop_y = if img_h > h { (img_h - h) / 2 } else { 0 };
                    let crop_w = img_w.min(w);
                    let crop_h = img_h.min(h);

                    // 2. Crop the image
                    let cropped = image::imageops::crop_imm(img, crop_x, crop_y, crop_w, crop_h).to_image();

                    // 3. Calculate paste offset if image is SMALLER than screen
                    let paste_x = if img_w < w { (w - img_w) / 2 } else { 0 };
                    let paste_y = if img_h < h { (h - img_h) / 2 } else { 0 };

                    // 4. Overlay the cropped/centered image
                    image::imageops::overlay(&mut bg, &cropped, paste_x as i64, paste_y as i64);
                    bg
                }
                "tile" => {
                    let (img_w, img_h) = img.dimensions();
                    let mut bg = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
                    // Stamp the image across the background canvas
                    for y in (0..h).step_by(img_h as usize) {
                        for x in (0..w).step_by(img_w as usize) {
                            image::imageops::overlay(&mut bg, img, x as i64, y as i64);
                        }
                    }
                    bg
                }
                _ => img
                .resize_to_fill(w, h, image::imageops::FilterType::Lanczos3)
                .to_rgba8(),
            }
        };

        let sharp_rgba = resize(image, job.wp_width, job.wp_height);
        let wp_pixels = rgba_to_xrgb(&sharp_rgba);

        // Downscale before blur for 4x speedup, then upscale back
        let bd_scale = 2u32; // Change to 3 for more speed, less quality
        let small_bd_w = job.bd_width / bd_scale;
        let small_bd_h = job.bd_height / bd_scale;

        // 1. Resize to small dimensions
        let small_backdrop = resize(image, small_bd_w, small_bd_h);

        // 2. Blur the small image (radius scaled down proportionally)
        let scaled_blur = (job.blur as f32) / bd_scale as f32;
        let small_blurred = image::imageops::blur(&small_backdrop, scaled_blur);

        // 3. Upscale back to full size with Lanczos3 (smooth, no blocky artifacts)
        let blurred_rgba = image::imageops::resize(
            &small_blurred,
            job.bd_width,
            job.bd_height,
            image::imageops::FilterType::Lanczos3,
        );

        let bd_pixels = rgba_to_xrgb(&blurred_rgba);

        let mut wp_file = create_shm_file("wallman-wallpaper", wp_pixels.len())?;
        wp_file.write_all(&wp_pixels)?;
        wp_file.flush()?;

        let mut bd_file = create_shm_file("wallman-backdrop", bd_pixels.len())?;
        bd_file.write_all(&bd_pixels)?;
        bd_file.flush()?;

        results.push(MonitorResult {
            name: job.name.clone(),
                     wallpaper_file: wp_file,
                     backdrop_file: bd_file,
                     wp_width: job.wp_width,
                     wp_height: job.wp_height,
                     bd_width: job.bd_width,
                     bd_height: job.bd_height,
        });
    }
    // Force glibc to return freed memory to the OS
    #[cfg(target_os = "linux")]
    unsafe {
        libc::malloc_trim(0);
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
