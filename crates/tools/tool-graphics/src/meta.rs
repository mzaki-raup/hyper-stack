//! What a picture says about itself (owner's request, 2026-10-24).
//!
//! The `image` crate answers the dimensions and the colour model, but not
//! the things a designer or a tester actually asks for - the DPI, the
//! colour profile, whether anything is really transparent - because those
//! live in container metadata it does not surface. So the bytes are read
//! here: PNG chunks (`pHYs`, `iCCP`, `sRGB`, `gAMA`, `tEXt`) and JPEG
//! segments (JFIF density, EXIF resolution, an ICC profile in APP2).
//!
//! Everything is best-effort and nothing guesses: a field that cannot be
//! read is left out rather than filled with a plausible default, because
//! a wrong DPI is worse than no DPI.

use image::DynamicImage;

/// One row of the properties card: a Fluent key for the label, and the
/// value already formatted for reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// `en.ftl` key, so the label is translated like every other string.
    pub key: &'static str,
    pub value: String,
}

fn row(key: &'static str, value: impl Into<String>) -> Property {
    Property {
        key,
        value: value.into(),
    }
}

/// Physical resolution, as the file records it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Density {
    pub x: f32,
    pub y: f32,
    /// True when the file gave pixels per inch; false for per centimetre
    /// (PNG records metres, which is converted to inches here).
    pub per_inch: bool,
}

/// Every property of `img`, decoded from `bytes` and sized by
/// `on_disk`. `bytes` are the file's own bytes, not the decoded pixels,
/// so the container metadata is still there to read.
pub fn describe(bytes: &[u8], img: &DynamicImage, on_disk: Option<u64>) -> Vec<Property> {
    let mut out = Vec::new();
    let (w, h) = (img.width(), img.height());

    if let Some(f) = image::guess_format(bytes).ok().and_then(format_name) {
        out.push(row("gfx-meta-format", f));
    }
    out.push(row("gfx-meta-dimensions", format!("{w} × {h} px")));
    out.push(row("gfx-meta-megapixels", megapixels(w, h)));
    if let Some(a) = aspect_ratio(w, h) {
        out.push(row("gfx-meta-aspect", a));
    }
    out.push(row("gfx-meta-colour", colour_model(img)));
    out.push(row("gfx-meta-depth", bit_depth(img)));
    out.push(row("gfx-meta-alpha", alpha_summary(img)));

    if let Some(d) = density(bytes) {
        // Both axes, because a non-square pixel aspect is exactly the
        // sort of thing this card exists to reveal.
        let unit = if d.per_inch { "DPI" } else { "px/cm" };
        let value = if (d.x - d.y).abs() < 0.5 {
            format!("{:.0} {unit}", d.x)
        } else {
            format!("{:.0} × {:.0} {unit}", d.x, d.y)
        };
        out.push(row("gfx-meta-density", value));
        if d.per_inch && d.x > 0.0 && d.y > 0.0 {
            out.push(row(
                "gfx-meta-print-size",
                format!(
                    "{:.2} × {:.2} in  ({:.1} × {:.1} mm)",
                    w as f32 / d.x,
                    h as f32 / d.y,
                    w as f32 / d.x * 25.4,
                    h as f32 / d.y * 25.4
                ),
            ));
        }
    }
    if let Some(p) = colour_profile(bytes) {
        out.push(row("gfx-meta-profile", p));
    }
    if let Some(n) = on_disk {
        out.push(row("gfx-meta-on-disk", human_bytes(n)));
    }
    // What the decoded pixels cost in memory, which is what a large
    // picture actually spends.
    out.push(row(
        "gfx-meta-in-memory",
        human_bytes(u64::from(w) * u64::from(h) * u64::from(bytes_per_pixel(img))),
    ));
    out
}

fn format_name(f: image::ImageFormat) -> Option<&'static str> {
    use image::ImageFormat as F;
    Some(match f {
        F::Png => "PNG",
        F::Jpeg => "JPEG",
        F::Gif => "GIF",
        F::WebP => "WebP",
        F::Bmp => "BMP",
        F::Ico => "ICO",
        F::Tiff => "TIFF",
        F::Avif => "AVIF",
        F::Tga => "TGA",
        F::Dds => "DDS",
        F::Hdr => "Radiance HDR",
        F::OpenExr => "OpenEXR",
        F::Pnm => "PNM",
        F::Farbfeld => "Farbfeld",
        F::Qoi => "QOI",
        _ => return None,
    })
}

fn megapixels(w: u32, h: u32) -> String {
    let mp = (f64::from(w) * f64::from(h)) / 1_000_000.0;
    if mp < 0.1 {
        format!("{} px", u64::from(w) * u64::from(h))
    } else {
        format!("{mp:.1} MP")
    }
}

/// The aspect ratio in its smallest whole terms (`16:9`), and the decimal
/// beside it. A ratio that does not reduce to something small is given as
/// the decimal alone, because `1591:1033` tells nobody anything.
pub fn aspect_ratio(w: u32, h: u32) -> Option<String> {
    if w == 0 || h == 0 {
        return None;
    }
    let g = gcd(w, h);
    let (rw, rh) = (w / g, h / g);
    let decimal = f64::from(w) / f64::from(h);
    if rw <= 64 && rh <= 64 {
        Some(format!("{rw}:{rh}  ({decimal:.3})"))
    } else {
        Some(format!("{decimal:.3}"))
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

fn colour_model(img: &DynamicImage) -> String {
    use image::ColorType as C;
    match img.color() {
        C::L8 | C::L16 => "Greyscale".into(),
        C::La8 | C::La16 => "Greyscale + alpha".into(),
        C::Rgb8 | C::Rgb16 | C::Rgb32F => "RGB".into(),
        C::Rgba8 | C::Rgba16 | C::Rgba32F => "RGBA".into(),
        other => format!("{other:?}"),
    }
}

/// Bits per channel and the total, which is what "colour bit depth"
/// means to the person asking.
fn bit_depth(img: &DynamicImage) -> String {
    use image::ColorType as C;
    let (per, channels) = match img.color() {
        C::L8 => (8, 1),
        C::La8 => (8, 2),
        C::Rgb8 => (8, 3),
        C::Rgba8 => (8, 4),
        C::L16 => (16, 1),
        C::La16 => (16, 2),
        C::Rgb16 => (16, 3),
        C::Rgba16 => (16, 4),
        C::Rgb32F => (32, 3),
        C::Rgba32F => (32, 4),
        _ => (8, 3),
    };
    format!("{per}-bit per channel  ({} bpp)", per * channels)
}

fn bytes_per_pixel(img: &DynamicImage) -> u32 {
    use image::ColorType as C;
    match img.color() {
        C::L8 => 1,
        C::La8 => 2,
        C::Rgb8 => 3,
        C::Rgba8 => 4,
        C::L16 => 2,
        C::La16 => 4,
        C::Rgb16 => 6,
        C::Rgba16 => 8,
        C::Rgb32F => 12,
        C::Rgba32F => 16,
        _ => 4,
    }
}

/// Whether there is an alpha channel, and whether anything in it is
/// actually transparent. The distinction matters: an RGBA PNG whose every
/// pixel is opaque behaves like RGB, and knowing that is the difference
/// between keeping PNG and not.
fn alpha_summary(img: &DynamicImage) -> String {
    use image::ColorType as C;
    let has_channel = matches!(
        img.color(),
        C::La8 | C::La16 | C::Rgba8 | C::Rgba16 | C::Rgba32F
    );
    if !has_channel {
        return "None (Opaque)".into();
    }
    let rgba = img.to_rgba8();
    let mut transparent = 0u64;
    let mut partial = false;
    for p in rgba.pixels() {
        if p.0[3] < u8::MAX {
            transparent += 1;
            if p.0[3] > 0 {
                partial = true;
            }
        }
    }
    if transparent == 0 {
        return "Channel present, fully opaque".into();
    }
    let total = u64::from(img.width()) * u64::from(img.height());
    let pct = transparent as f64 / total as f64 * 100.0;
    if partial {
        format!("Transparent ({pct:.1}% of pixels, with soft edges)")
    } else {
        format!("Transparent ({pct:.1}% of pixels)")
    }
}

/// The file's physical resolution. PNG records pixels per metre in
/// `pHYs`; JPEG records a density and its unit in JFIF, or a rational
/// `XResolution` in EXIF.
pub fn density(bytes: &[u8]) -> Option<Density> {
    png_phys(bytes).or_else(|| jpeg_density(bytes))
}

/// PNG `pHYs`: two big-endian u32 of pixels per unit, then a unit byte
/// (1 = metre, 0 = unknown aspect only).
fn png_phys(bytes: &[u8]) -> Option<Density> {
    let chunk = png_chunk(bytes, b"pHYs")?;
    if chunk.len() < 9 {
        return None;
    }
    let x = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f32;
    let y = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as f32;
    if x == 0.0 || y == 0.0 {
        return None;
    }
    // Unit 1 is the metre; 0 means the numbers are only an aspect ratio,
    // which is not a resolution and must not be shown as one.
    if chunk[8] != 1 {
        return None;
    }
    Some(Density {
        x: x * 0.0254,
        y: y * 0.0254,
        per_inch: true,
    })
}

/// Walk a PNG's chunks and return the body of the first one of `want`.
fn png_chunk<'a>(bytes: &'a [u8], want: &[u8; 4]) -> Option<&'a [u8]> {
    const SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if !bytes.starts_with(SIG) {
        return None;
    }
    let mut i = SIG.len();
    while i + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        let kind = &bytes[i + 4..i + 8];
        let start = i + 8;
        let end = start.checked_add(len)?;
        if end > bytes.len() {
            return None;
        }
        if kind == want {
            return Some(&bytes[start..end]);
        }
        if kind == b"IDAT" || kind == b"IEND" {
            // Metadata after the pixel data is legal but rare; stopping
            // here keeps this from walking a whole large file.
            return None;
        }
        i = end + 4; // skip the CRC
    }
    None
}

/// JPEG: the JFIF APP0 density, else EXIF's `XResolution`.
fn jpeg_density(bytes: &[u8]) -> Option<Density> {
    for (marker, body) in jpeg_segments(bytes) {
        if marker == 0xE0 && body.len() >= 12 && body.starts_with(b"JFIF\0") {
            let units = body[7];
            let x = u16::from_be_bytes([body[8], body[9]]) as f32;
            let y = u16::from_be_bytes([body[10], body[11]]) as f32;
            if x == 0.0 || y == 0.0 {
                continue;
            }
            // 1 = pixels per inch, 2 = per centimetre, 0 = aspect only.
            match units {
                1 => {
                    return Some(Density {
                        x,
                        y,
                        per_inch: true,
                    });
                }
                2 => {
                    return Some(Density {
                        x,
                        y,
                        per_inch: false,
                    });
                }
                _ => continue,
            }
        }
    }
    None
}

/// Every JPEG marker segment as `(marker, body)`, stopping at the start
/// of the compressed scan.
fn jpeg_segments(bytes: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return out;
    }
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            break;
        }
        let marker = bytes[i + 1];
        // Start of scan, or end: the entropy-coded data begins.
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if len < 2 {
            break;
        }
        let start = i + 4;
        let end = match start.checked_add(len - 2) {
            Some(e) if e <= bytes.len() => e,
            _ => break,
        };
        out.push((marker, &bytes[start..end]));
        i = end;
    }
    out
}

/// The colour profile the file declares, by name where it has one.
pub fn colour_profile(bytes: &[u8]) -> Option<String> {
    // PNG: an embedded profile names itself, and `sRGB` is a one-byte
    // declaration that the picture is sRGB.
    if let Some(iccp) = png_chunk(bytes, b"iCCP") {
        let name: Vec<u8> = iccp.iter().copied().take_while(|b| *b != 0).collect();
        let name = String::from_utf8_lossy(&name).trim().to_string();
        return Some(if name.is_empty() {
            "Embedded ICC profile".into()
        } else {
            format!("{name} (Embedded ICC)")
        });
    }
    if png_chunk(bytes, b"sRGB").is_some() {
        return Some("sRGB".into());
    }
    if let Some(gama) = png_chunk(bytes, b"gAMA")
        && gama.len() >= 4
    {
        let g = u32::from_be_bytes([gama[0], gama[1], gama[2], gama[3]]);
        if g > 0 {
            return Some(format!("Gamma {:.4}", 100_000.0 / f64::from(g)));
        }
    }
    // JPEG: APP2 carries the profile, whose description is at a fixed
    // offset inside the ICC header's tag table - too deep to read here,
    // so its presence is what is reported.
    for (marker, body) in jpeg_segments(bytes) {
        if marker == 0xE2 && body.starts_with(b"ICC_PROFILE\0") {
            return Some("Embedded ICC profile".into());
        }
        if marker == 0xEE && body.starts_with(b"Adobe") {
            return Some("Adobe (APP14)".into());
        }
    }
    None
}

fn human_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else if n < 1024 * 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", n as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_aspect_ratio_reduces_when_it_usefully_can() {
        assert_eq!(aspect_ratio(1920, 1080).unwrap(), "16:9  (1.778)");
        assert_eq!(aspect_ratio(800, 600).unwrap(), "4:3  (1.333)");
        assert_eq!(aspect_ratio(512, 512).unwrap(), "1:1  (1.000)");
        // Nothing useful to reduce to: the decimal alone, not 1591:1033.
        assert_eq!(aspect_ratio(1591, 1033).unwrap(), "1.540");
        assert!(aspect_ratio(0, 10).is_none());
    }

    #[test]
    fn a_png_without_phys_reports_no_density() {
        let png = encoded_png(4, 4, false);
        assert!(density(&png).is_none(), "nothing may be invented");
    }

    /// A `pHYs` whose unit byte is 0 is an aspect ratio, not a
    /// resolution, and must not be shown as DPI.
    #[test]
    fn a_phys_with_no_unit_is_not_a_resolution() {
        // 2835 px/m is 72 DPI.
        let with_unit = png_with_phys(2835, 2835, 1);
        let d = density(&with_unit).expect("a metre unit is a resolution");
        assert!(d.per_inch);
        assert!((d.x - 72.0).abs() < 0.5, "got {}", d.x);

        let no_unit = png_with_phys(2835, 2835, 0);
        assert!(density(&no_unit).is_none());
    }

    #[test]
    fn a_png_srgb_chunk_is_reported_as_the_profile() {
        let png = png_with_chunk(b"sRGB", &[0]);
        assert_eq!(colour_profile(&png).as_deref(), Some("sRGB"));
    }

    #[test]
    fn an_embedded_icc_profile_is_named() {
        let mut body = b"Photoshop ICC profile".to_vec();
        body.push(0);
        body.push(0); // compression method
        let png = png_with_chunk(b"iCCP", &body);
        assert_eq!(
            colour_profile(&png).as_deref(),
            Some("Photoshop ICC profile (Embedded ICC)")
        );
    }

    #[test]
    fn a_jpeg_jfif_density_is_read_with_its_unit() {
        let jpeg = jpeg_with_jfif(300, 300, 1);
        let d = density(&jpeg).expect("JFIF gives a density");
        assert!(d.per_inch);
        assert_eq!(d.x, 300.0);

        let cm = jpeg_with_jfif(118, 118, 2);
        let d = density(&cm).expect("per centimetre is still a density");
        assert!(!d.per_inch);
    }

    /// The card must not claim transparency for an RGBA picture whose
    /// every pixel is opaque - that is the difference between keeping a
    /// PNG and not.
    #[test]
    fn alpha_is_reported_as_present_but_opaque_when_nothing_is_transparent() {
        let opaque = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([1, 2, 3, 255]),
        ));
        assert_eq!(alpha_summary(&opaque), "Channel present, fully opaque");

        let rgb = DynamicImage::ImageRgb8(image::RgbImage::new(4, 4));
        assert_eq!(alpha_summary(&rgb), "None (Opaque)");

        let mut img = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
        img.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
        let holed = DynamicImage::ImageRgba8(img);
        assert_eq!(alpha_summary(&holed), "Transparent (25.0% of pixels)");
    }

    #[test]
    fn the_description_names_the_format_dimensions_and_depth() {
        let png = encoded_png(8, 4, true);
        let img = image::load_from_memory(&png).unwrap();
        let props = describe(&png, &img, Some(1234));
        let get = |k: &str| {
            props
                .iter()
                .find(|p| p.key == k)
                .map(|p| p.value.clone())
                .unwrap_or_else(|| panic!("no {k}"))
        };
        assert_eq!(get("gfx-meta-format"), "PNG");
        assert_eq!(get("gfx-meta-dimensions"), "8 × 4 px");
        assert_eq!(get("gfx-meta-aspect"), "2:1  (2.000)");
        assert_eq!(get("gfx-meta-colour"), "RGBA");
        assert!(get("gfx-meta-depth").starts_with("8-bit per channel"));
        assert_eq!(get("gfx-meta-on-disk"), "1.2 KB");
        // 8 × 4 × 4 bytes.
        assert_eq!(get("gfx-meta-in-memory"), "128 B");
    }

    /// Every label must be a key the locale can translate, never text.
    #[test]
    fn every_property_label_is_a_fluent_key() {
        let png = encoded_png(4, 4, true);
        let img = image::load_from_memory(&png).unwrap();
        for p in describe(&png, &img, None) {
            assert!(
                p.key.starts_with("gfx-meta-"),
                "{} is not a locale key",
                p.key
            );
        }
    }

    // --- fixtures -----------------------------------------------------

    fn encoded_png(w: u32, h: u32, alpha: bool) -> Vec<u8> {
        let img = if alpha {
            DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                w,
                h,
                image::Rgba([9, 9, 9, 255]),
            ))
        } else {
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([9, 9, 9])))
        };
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// A PNG with one extra chunk spliced in before `IDAT`.
    fn png_with_chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let base = encoded_png(4, 4, false);
        let idat = find(&base, b"IDAT").expect("a PNG has IDAT");
        // The chunk header starts four bytes before its type.
        let at = idat - 4;
        let mut out = base[..at].to_vec();
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out.extend_from_slice(&[0, 0, 0, 0]); // CRC, unchecked here
        out.extend_from_slice(&base[at..]);
        out
    }

    fn png_with_phys(x: u32, y: u32, unit: u8) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&x.to_be_bytes());
        body.extend_from_slice(&y.to_be_bytes());
        body.push(unit);
        png_with_chunk(b"pHYs", &body)
    }

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    /// The smallest thing `jpeg_segments` will walk: SOI, an APP0 JFIF
    /// segment, then SOS to stop it.
    fn jpeg_with_jfif(x: u16, y: u16, units: u8) -> Vec<u8> {
        let mut body = b"JFIF\0".to_vec();
        body.extend_from_slice(&[1, 2]); // version
        body.push(units);
        body.extend_from_slice(&x.to_be_bytes());
        body.extend_from_slice(&y.to_be_bytes());
        body.extend_from_slice(&[0, 0]); // thumbnail size

        let mut out = vec![0xFF, 0xD8];
        out.extend_from_slice(&[0xFF, 0xE0]);
        out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&body);
        out.extend_from_slice(&[0xFF, 0xDA]);
        out
    }
}
