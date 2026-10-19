//! Image operations as pure functions over `image::DynamicImage`, with the
//! encoders for every format the tool writes. The UI keeps an operation
//! list and re-applies it to the original, so every step is undoable.

use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Png,
    Jpeg,
    Webp,
    Bmp,
    Gif,
    Ico,
}

impl Format {
    pub const ALL: [Format; 6] = [
        Format::Png,
        Format::Jpeg,
        Format::Webp,
        Format::Bmp,
        Format::Gif,
        Format::Ico,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Webp => "WebP",
            Format::Bmp => "BMP",
            Format::Gif => "GIF",
            Format::Ico => "ICO",
        }
    }
    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Webp => "webp",
            Format::Bmp => "bmp",
            Format::Gif => "gif",
            Format::Ico => "ico",
        }
    }
    fn image_format(self) -> ImageFormat {
        match self {
            Format::Png => ImageFormat::Png,
            Format::Jpeg => ImageFormat::Jpeg,
            Format::Webp => ImageFormat::WebP,
            Format::Bmp => ImageFormat::Bmp,
            Format::Gif => ImageFormat::Gif,
            Format::Ico => ImageFormat::Ico,
        }
    }
    pub fn from_ext(ext: &str) -> Option<Format> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(Format::Png),
            "jpg" | "jpeg" => Some(Format::Jpeg),
            "webp" => Some(Format::Webp),
            "bmp" => Some(Format::Bmp),
            "gif" => Some(Format::Gif),
            "ico" => Some(Format::Ico),
            _ => None,
        }
    }
}

/// Extensions the editor hands to the graphics tool instead of opening as text.
pub use hs_core::sniff::IMAGE_EXTS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Scale to `width` × `height` (0 keeps the aspect ratio for that side).
    Resize {
        width: u32,
        height: u32,
    },
    /// Scale by a factor (2.0 doubles).
    Scale {
        factor: f32,
    },
    /// Grow the canvas by padding on each side, filled with a colour.
    Extend {
        left: u32,
        top: u32,
        right: u32,
        bottom: u32,
        colour: [u8; 4],
    },
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Rotate90,
    Rotate180,
    Rotate270,
    FlipH,
    FlipV,
    /// -100 … 100
    Brightness {
        value: i32,
    },
    /// -100 … 100
    Contrast {
        value: f32,
    },
    /// Degrees 0 … 360
    Hue {
        degrees: i32,
    },
    /// -100 … 100 (0 = unchanged, -100 = grey)
    Saturation {
        value: f32,
    },
    Grayscale,
    Invert,
    Blur {
        sigma: f32,
    },
    Sharpen {
        sigma: f32,
        threshold: i32,
    },
    /// Make every pixel within `tolerance` (0…100, per channel) of `colour`
    /// fully transparent. Used by the Icon tab's eyedropper (owner's
    /// request, 2026-10-19).
    TransparentColor {
        colour: [u8; 4],
        tolerance: u8,
    },
}

impl Op {
    pub fn label(&self) -> String {
        match self {
            Op::Resize { width, height } => format!("Resize to {width}×{height}"),
            Op::Scale { factor } => format!("Scale ×{factor:.2}"),
            Op::Extend {
                left,
                top,
                right,
                bottom,
                ..
            } => format!("Extend canvas {left}/{top}/{right}/{bottom}"),
            Op::Crop {
                x,
                y,
                width,
                height,
            } => format!("Crop {width}×{height} at {x},{y}"),
            Op::Rotate90 => "Rotate 90°".into(),
            Op::Rotate180 => "Rotate 180°".into(),
            Op::Rotate270 => "Rotate 270°".into(),
            Op::FlipH => "Flip horizontally".into(),
            Op::FlipV => "Flip vertically".into(),
            Op::Brightness { value } => format!("Brightness {value:+}"),
            Op::Contrast { value } => format!("Contrast {value:+.0}"),
            Op::Hue { degrees } => format!("Hue {degrees}°"),
            Op::Saturation { value } => format!("Saturation {value:+.0}"),
            Op::Grayscale => "Grayscale".into(),
            Op::Invert => "Invert".into(),
            Op::Blur { sigma } => format!("Blur σ{sigma:.1}"),
            Op::Sharpen { sigma, threshold } => format!("Sharpen σ{sigma:.1} t{threshold}"),
            Op::TransparentColor { colour, tolerance } => format!(
                "Transparent #{:02X}{:02X}{:02X} (tol {tolerance})",
                colour[0], colour[1], colour[2]
            ),
        }
    }
}

pub fn apply(img: &DynamicImage, op: &Op) -> DynamicImage {
    match op {
        Op::Resize { width, height } => {
            let (w, h) = img.dimensions();
            let (nw, nh) = match (*width, *height) {
                (0, 0) => (w, h),
                (0, h2) => (
                    ((w as f64) * (h2 as f64) / (h as f64)).round().max(1.0) as u32,
                    h2,
                ),
                (w2, 0) => (
                    w2,
                    ((h as f64) * (w2 as f64) / (w as f64)).round().max(1.0) as u32,
                ),
                (w2, h2) => (w2, h2),
            };
            img.resize_exact(nw.max(1), nh.max(1), image::imageops::FilterType::Lanczos3)
        }
        Op::Scale { factor } => {
            let (w, h) = img.dimensions();
            let f = factor.max(0.01);
            let filter = if f >= 1.0 {
                image::imageops::FilterType::Lanczos3
            } else {
                image::imageops::FilterType::CatmullRom
            };
            img.resize_exact(
                ((w as f32) * f).round().max(1.0) as u32,
                ((h as f32) * f).round().max(1.0) as u32,
                filter,
            )
        }
        Op::Extend {
            left,
            top,
            right,
            bottom,
            colour,
        } => {
            let (w, h) = img.dimensions();
            let mut canvas =
                RgbaImage::from_pixel(w + left + right, h + top + bottom, Rgba(*colour));
            image::imageops::overlay(
                &mut canvas,
                &img.to_rgba8(),
                i64::from(*left),
                i64::from(*top),
            );
            DynamicImage::ImageRgba8(canvas)
        }
        Op::Crop {
            x,
            y,
            width,
            height,
        } => {
            let (w, h) = img.dimensions();
            let x = (*x).min(w.saturating_sub(1));
            let y = (*y).min(h.saturating_sub(1));
            let cw = (*width).clamp(1, w - x);
            let ch = (*height).clamp(1, h - y);
            img.crop_imm(x, y, cw, ch)
        }
        Op::Rotate90 => img.rotate90(),
        Op::Rotate180 => img.rotate180(),
        Op::Rotate270 => img.rotate270(),
        Op::FlipH => img.fliph(),
        Op::FlipV => img.flipv(),
        Op::Brightness { value } => img.brighten((*value as f32 * 2.55) as i32),
        Op::Contrast { value } => img.adjust_contrast(*value),
        Op::Hue { degrees } => img.huerotate(*degrees),
        Op::Saturation { value } => saturate(img, *value),
        Op::Grayscale => DynamicImage::ImageRgba8(img.grayscale().to_rgba8()),
        Op::Invert => {
            let mut i = img.clone();
            i.invert();
            i
        }
        Op::Blur { sigma } => img.blur(sigma.max(0.1)),
        Op::Sharpen { sigma, threshold } => img.unsharpen(sigma.max(0.1), *threshold),
        Op::TransparentColor { colour, tolerance } => {
            let limit = ((*tolerance as u16) * 255 / 100) as u8;
            let mut out = img.to_rgba8();
            for p in out.pixels_mut() {
                let [r, g, b, _] = p.0;
                let close = |a: u8, b: u8| a.abs_diff(b) <= limit;
                if close(r, colour[0]) && close(g, colour[1]) && close(b, colour[2]) {
                    p.0[3] = 0;
                }
            }
            DynamicImage::ImageRgba8(out)
        }
    }
}

/// Saturation in HSL terms: -100 drains to grey, +100 doubles the distance
/// from grey (clamped).
fn saturate(img: &DynamicImage, value: f32) -> DynamicImage {
    let factor = 1.0 + (value / 100.0).clamp(-1.0, 1.0);
    let mut out = img.to_rgba8();
    for p in out.pixels_mut() {
        let [r, g, b, a] = p.0;
        let lum = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
        let adj = |c: u8| ((lum + (c as f32 - lum) * factor).round()).clamp(0.0, 255.0) as u8;
        p.0 = [adj(r), adj(g), adj(b), a];
    }
    DynamicImage::ImageRgba8(out)
}

pub fn apply_all(original: &DynamicImage, ops: &[Op]) -> DynamicImage {
    ops.iter().fold(original.clone(), |img, op| apply(&img, op))
}

/// Encode as `format`. JPEG takes a quality (1–100); ICO needs ≤ 256 px
/// sides and is written as a single-image icon.
pub fn encode(img: &DynamicImage, format: Format, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    match format {
        Format::Jpeg => {
            let mut enc =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
            enc.encode_image(&img.to_rgb8())
                .map_err(|e| e.to_string())?;
        }
        Format::Ico => {
            let (w, h) = img.dimensions();
            let img = if w > 256 || h > 256 {
                img.resize(256, 256, image::imageops::FilterType::Lanczos3)
            } else {
                img.clone()
            };
            img.write_to(&mut out, ImageFormat::Ico)
                .map_err(|e| e.to_string())?;
        }
        Format::Png | Format::Webp | Format::Bmp | Format::Gif => {
            let rgba = DynamicImage::ImageRgba8(img.to_rgba8());
            rgba.write_to(&mut out, format.image_format())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(out.into_inner())
}

/// A multi-size ICO (16, 24, 32, 48, 64, 128, 256) from one picture.
pub fn multi_ico(img: &DynamicImage, sizes: &[u32]) -> Result<Vec<u8>, String> {
    let mut dir = Vec::new();
    for &s in sizes {
        let resized = img.resize_exact(s, s, image::imageops::FilterType::Lanczos3);
        let png = encode(&resized, Format::Png, 100)?;
        dir.push((s, png));
    }
    // ICONDIR + ICONDIRENTRY per image, PNG payloads (Vista+ format).
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(dir.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * dir.len() as u32;
    for (s, png) in &dir {
        let dim = if *s >= 256 { 0u8 } else { *s as u8 };
        out.push(dim);
        out.push(dim);
        out.push(0);
        out.push(0);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &dir {
        out.extend_from_slice(png);
    }
    Ok(out)
}

/// Every frame of an ICO (or the single image of any other format), with
/// its size, for icon extraction.
pub fn ico_frames(bytes: &[u8]) -> Vec<(u32, u32, DynamicImage)> {
    let mut out = Vec::new();
    if bytes.len() >= 6 && bytes[..4] == [0, 0, 1, 0] {
        let n = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
        for i in 0..n {
            let e = 6 + i * 16;
            if bytes.len() < e + 16 {
                break;
            }
            let size =
                u32::from_le_bytes([bytes[e + 8], bytes[e + 9], bytes[e + 10], bytes[e + 11]])
                    as usize;
            let off =
                u32::from_le_bytes([bytes[e + 12], bytes[e + 13], bytes[e + 14], bytes[e + 15]])
                    as usize;
            if bytes.len() < off + size {
                continue;
            }
            let payload = &bytes[off..off + size];
            // PNG payload or BMP (DIB) payload: let the ico decoder handle a
            // one-entry icon built around it.
            let mut single = Vec::new();
            single.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
            single.extend_from_slice(&bytes[e..e + 12]);
            single.extend_from_slice(&22u32.to_le_bytes());
            single.extend_from_slice(payload);
            if let Ok(img) = image::load_from_memory_with_format(&single, ImageFormat::Ico) {
                let (w, h) = img.dimensions();
                out.push((w, h, img));
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    if let Ok(img) = image::load_from_memory(bytes) {
        let (w, h) = img.dimensions();
        out.push((w, h, img));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DynamicImage {
        let mut img = RgbaImage::new(8, 4);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgba([x as u8 * 30, y as u8 * 60, 100, 255]);
        }
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn geometry_ops() {
        let img = sample();
        assert_eq!(
            apply(
                &img,
                &Op::Resize {
                    width: 4,
                    height: 0
                }
            )
            .dimensions(),
            (4, 2)
        );
        assert_eq!(
            apply(
                &img,
                &Op::Resize {
                    width: 0,
                    height: 8
                }
            )
            .dimensions(),
            (16, 8)
        );
        assert_eq!(
            apply(&img, &Op::Scale { factor: 2.0 }).dimensions(),
            (16, 8)
        );
        assert_eq!(apply(&img, &Op::Rotate90).dimensions(), (4, 8));
        let ext = apply(
            &img,
            &Op::Extend {
                left: 1,
                top: 2,
                right: 3,
                bottom: 4,
                colour: [9, 9, 9, 255],
            },
        );
        assert_eq!(ext.dimensions(), (12, 10));
        assert_eq!(ext.get_pixel(0, 0), Rgba([9, 9, 9, 255]));
        assert_eq!(ext.get_pixel(1, 2), img.get_pixel(0, 0));
        assert_eq!(
            apply(
                &img,
                &Op::Crop {
                    x: 2,
                    y: 1,
                    width: 100,
                    height: 100
                }
            )
            .dimensions(),
            (6, 3)
        );
    }

    #[test]
    fn colour_ops_and_ordering() {
        let img = sample();
        let grey = apply(&img, &Op::Grayscale);
        let p = grey.get_pixel(3, 2);
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
        let drained = apply(&img, &Op::Saturation { value: -100.0 });
        let q = drained.get_pixel(3, 2);
        assert!((q[0] as i32 - q[1] as i32).abs() <= 1);
        let inv = apply(&img, &Op::Invert);
        assert_eq!(inv.get_pixel(0, 0)[0], 255 - img.get_pixel(0, 0)[0]);
        let chain = apply_all(&img, &[Op::Scale { factor: 0.5 }, Op::Rotate90]);
        assert_eq!(chain.dimensions(), (2, 4));
    }

    #[test]
    fn colour_key_transparency() {
        let img = DynamicImage::ImageRgba8(
            RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap(),
        );
        let exact = apply(
            &img,
            &Op::TransparentColor {
                colour: [255, 0, 0, 255],
                tolerance: 0,
            },
        );
        assert_eq!(exact.get_pixel(0, 0)[3], 0, "the picked colour is clear");
        assert_eq!(exact.get_pixel(1, 0)[3], 255, "other colours stay");
        let loose = apply(
            &img,
            &Op::TransparentColor {
                colour: [255, 0, 0, 255],
                tolerance: 100,
            },
        );
        assert_eq!(loose.get_pixel(1, 0)[3], 0, "tol 100 covers every colour");
    }

    #[test]
    fn encoders_and_icons() {
        let img = sample();
        for f in Format::ALL {
            let bytes = encode(&img, f, 80).unwrap();
            assert!(!bytes.is_empty(), "{f:?}");
            let back = image::load_from_memory(&bytes).unwrap();
            assert_eq!(back.dimensions(), (8, 4), "{f:?}");
        }
        let ico = multi_ico(&img, &[16, 32]).unwrap();
        let frames = ico_frames(&ico);
        assert_eq!(frames.len(), 2);
        assert_eq!((frames[0].0, frames[1].0), (16, 32));
        let png = encode(&img, Format::Png, 100).unwrap();
        assert_eq!(ico_frames(&png).len(), 1);
        assert_eq!(Format::from_ext("JPG"), Some(Format::Jpeg));
    }
}
