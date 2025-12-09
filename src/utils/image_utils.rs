//! Image utility functions.

use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};
use std::path::Path;

/// Get image dimensions without loading the full image.
pub fn get_image_dimensions(path: &Path) -> Result<(u32, u32)> {
    let img = image::open(path).context("Failed to open image")?;
    Ok(img.dimensions())
}

/// Resize an image to fit within max dimensions while maintaining aspect ratio.
pub fn resize_to_fit(img: DynamicImage, max_width: u32, max_height: u32) -> DynamicImage {
    let (width, height) = img.dimensions();

    // Calculate scaling factor
    let scale = (max_width as f64 / width as f64).min(max_height as f64 / height as f64);

    if scale >= 1.0 {
        return img;
    }

    let new_width = (width as f64 * scale) as u32;
    let new_height = (height as f64 * scale) as u32;

    img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3)
}

/// Create a thumbnail from an image.
pub fn create_thumbnail(path: &Path, thumb_size: u32) -> Result<DynamicImage> {
    let img = image::open(path).context("Failed to open image")?;
    Ok(img.thumbnail(thumb_size, thumb_size))
}

/// Calculate aspect ratio as a string (e.g., "16:9", "21:9").
pub fn aspect_ratio_str(width: u32, height: u32) -> String {
    let gcd = gcd(width, height);
    let w = width / gcd;
    let h = height / gcd;

    // Simplify common ratios
    match (w, h) {
        (16, 9) | (32, 18) | (64, 36) => "16:9".to_string(),
        (21, 9) | (64, 27) => "21:9".to_string(),
        (16, 10) | (8, 5) => "16:10".to_string(),
        (4, 3) => "4:3".to_string(),
        (1, 1) => "1:1".to_string(),
        _ => format!("{}:{}", w, h),
    }
}

/// Calculate greatest common divisor.
fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Convert bytes to human-readable size.
pub fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Size presets for image generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizePreset {
    Hd,        // 1920x1080
    Qhd,       // 2560x1440
    Laptop,    // 2560x1600
    Uhd4k,     // 3840x2160
    Ultrawide, // 5120x2160
    Custom(u32, u32),
}

impl SizePreset {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Hd => (1920, 1080),
            Self::Qhd => (2560, 1440),
            Self::Laptop => (2560, 1600),
            Self::Uhd4k => (3840, 2160),
            Self::Ultrawide => (5120, 2160),
            Self::Custom(w, h) => (*w, *h),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Hd => "HD (1920x1080)",
            Self::Qhd => "QHD (2560x1440)",
            Self::Laptop => "Laptop (2560x1600)",
            Self::Uhd4k => "4K UHD (3840x2160)",
            Self::Ultrawide => "Ultrawide (5120x2160)",
            Self::Custom(_, _) => "Custom",
        }
    }

    pub fn all() -> Vec<Self> {
        vec![
            Self::Hd,
            Self::Qhd,
            Self::Laptop,
            Self::Uhd4k,
            Self::Ultrawide,
        ]
    }
}

impl std::fmt::Display for SizePreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (w, h) = self.dimensions();
        write!(f, "{}x{}", w, h)
    }
}
