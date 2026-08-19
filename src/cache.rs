use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"WALLMAN1";

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RawHeader {
    magic: [u8; 8],
    width: u32,
    height: u32,
    stride: u32,
    blur: u32,
    mode: [u8; 16], // Store mode as fixed-size string
}

impl RawHeader {
    pub fn new(width: u32, height: u32, blur: u32, mode: &str) -> Self {
        let mut mode_bytes = [0u8; 16];
        let mode_str = mode.as_bytes();
        let len = mode_str.len().min(15);
        mode_bytes[..len].copy_from_slice(&mode_str[..len]);

        Self {
            magic: *MAGIC,
            width,
            height,
            stride: width * 4,
            blur,
            mode: mode_bytes,
        }
    }

    pub fn matches(&self, width: u32, height: u32, blur: u32, mode: &str) -> bool {
        let mut mode_bytes = [0u8; 16];
        let mode_str = mode.as_bytes();
        let len = mode_str.len().min(15);
        mode_bytes[..len].copy_from_slice(&mode_str[..len]);

        self.magic == *MAGIC
        && self.width == width
        && self.height == height
        && self.blur == blur
        && self.mode == mode_bytes
    }
}

pub fn raw_path(cache_dir: &Path, monitor: &str, kind: &str) -> PathBuf {
    cache_dir.join(format!("{}.{}.raw", monitor, kind))
}

/// Write final XRGB pixels after processing
pub fn write_raw_cache(
    cache_dir: &Path,
    monitor: &str,
    kind: &str,
    width: u32,
    height: u32,
    blur: u32,
    mode: &str,
    pixels: &[u8],
) -> std::io::Result<()> {
    fs::create_dir_all(cache_dir)?;
    let path = raw_path(cache_dir, monitor, kind);
    let tmp = path.with_extension("tmp");

    let mut f = File::create(&tmp)?;
    let header = RawHeader::new(width, height, blur, mode);

    let header_bytes = unsafe {
        std::slice::from_raw_parts(
            &header as *const _ as *const u8,
            std::mem::size_of::<RawHeader>(),
        )
    };
    f.write_all(header_bytes)?;
    f.write_all(pixels)?;
    f.sync_all()?;
    fs::rename(tmp, path)?;

    println!("[cache] wrote {}.{}.raw ({}x{})", monitor, kind, width, height);
    Ok(())
}

/// Try to load a previously written buffer
pub fn try_load_raw_cache(
    cache_dir: &Path,
    monitor: &str,
    kind: &str,
    expected_w: u32,
    expected_h: u32,
    expected_blur: u32,
    expected_mode: &str,
) -> Option<(Vec<u8>, u32, u32)> {
    let path = raw_path(cache_dir, monitor, kind);
    let mut f = File::open(&path).ok()?;
    let mut header_buf = [0u8; std::mem::size_of::<RawHeader>()];
    f.read_exact(&mut header_buf).ok()?;

    let header: RawHeader = unsafe { std::ptr::read(header_buf.as_ptr() as *const _) };

    if !header.matches(expected_w, expected_h, expected_blur, expected_mode) {
        return None;
    }

    let mut pixels = Vec::with_capacity((header.stride * header.height) as usize);
    f.read_to_end(&mut pixels).ok()?;

    if pixels.len() != (header.stride * header.height) as usize {
        return None;
    }

    println!("[cache] loaded {}.{}.raw ({}x{})", monitor, kind, header.width, header.height);
    Some((pixels, header.width, header.height))
}
