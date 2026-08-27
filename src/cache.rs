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
    mode: [u8; 16],
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

    pub fn to_bytes(&self) -> [u8; 40] {
        let mut bytes = [0u8; 40];
        bytes[0..8].copy_from_slice(&self.magic);
        bytes[8..12].copy_from_slice(&self.width.to_ne_bytes());
        bytes[12..16].copy_from_slice(&self.height.to_ne_bytes());
        bytes[16..20].copy_from_slice(&self.stride.to_ne_bytes());
        bytes[20..24].copy_from_slice(&self.blur.to_ne_bytes());
        bytes[24..40].copy_from_slice(&self.mode);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 40 { return None; }
        let mut magic = [0u8; 8];
        magic.copy_from_slice(&bytes[0..8]);

        // Validate magic bytes immediately
        if magic != *MAGIC { return None; }

        let width = u32::from_ne_bytes(bytes[8..12].try_into().ok()?);
        let height = u32::from_ne_bytes(bytes[12..16].try_into().ok()?);
        let stride = u32::from_ne_bytes(bytes[16..20].try_into().ok()?);
        let blur = u32::from_ne_bytes(bytes[20..24].try_into().ok()?);
        let mut mode = [0u8; 16];
        mode.copy_from_slice(&bytes[24..40]);

        // Validate stride and cap max resolution to 16K to prevent massive allocations
        if stride != width.checked_mul(4)? { return None; }
        if width > 16384 || height > 16384 { return None; }

        Some(Self { magic, width, height, stride, blur, mode })
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
    let header_bytes = header.to_bytes();
    f.write_all(&header_bytes)?;
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
    let mut header_buf = [0u8; 40];
    f.read_exact(&mut header_buf).ok()?;

    let header = match RawHeader::from_bytes(&header_buf) {
        Some(h) => h,
        None => return None,
    };

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
