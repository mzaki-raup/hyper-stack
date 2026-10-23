//! What the Graphics assistant actually sends with a question.
//!
//! The assistant hung on a local service because the attachment was a
//! PNG at quality 100: a photograph came to 2.8 MB of base64 in the
//! request body, and a model that cannot read pictures tokenised it as
//! text (owner's report, 2026-10-24). These tests pin the size down, and
//! pin PNG in place for the pictures where JPEG would smear small text.

use image::{DynamicImage, Rgba, RgbaImage};
use tool_graphics::ops::{Format, encode};

/// The rule `attachment_part` follows, kept here so the numbers it turns
/// on are checked against real encodings rather than assumed.
const PNG_BUDGET: usize = 400 * 1024;

fn photo(w: u32, h: u32) -> DynamicImage {
    // Every pixel different, so PNG cannot run-length anything away.
    let mut img = image::RgbImage::new(w, h);
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    for p in img.pixels_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let b = seed.to_le_bytes();
        *p = image::Rgb([b[0] / 2 + 60, b[1] / 2 + 70, b[2] / 2 + 80]);
    }
    DynamicImage::ImageRgb8(img)
}

fn screenshot(w: u32, h: u32) -> DynamicImage {
    // Flat panels and rules: what a screenshot, diagram or icon looks
    // like to an encoder.
    let mut img = RgbaImage::from_pixel(w, h, Rgba([246, 246, 248, 255]));
    for y in 0..h {
        for x in 0..w {
            let on_rule = y % 40 == 0 || x % 160 == 0;
            let in_panel = x > w / 8 && x < w / 2 && y > h / 8 && y < h / 2;
            if on_rule {
                img.put_pixel(x, y, Rgba([200, 200, 208, 255]));
            } else if in_panel {
                img.put_pixel(x, y, Rgba([32, 34, 40, 255]));
            }
        }
    }
    DynamicImage::ImageRgba8(img)
}

/// The downscale `attachment_part` applies before encoding.
fn shrink(img: &DynamicImage) -> DynamicImage {
    if img.width() > 1024 || img.height() > 1024 {
        img.thumbnail(1024, 1024)
    } else {
        img.clone()
    }
}

/// base64 is four bytes out for every three in: what lands in the body.
fn base64_len(n: usize) -> usize {
    n.div_ceil(3) * 4
}

#[test]
fn a_photograph_goes_as_a_jpeg_small_enough_for_any_service() {
    let small = shrink(&photo(4000, 3000));
    let png = encode(&small, Format::Png, 100).unwrap();
    assert!(
        png.len() > PNG_BUDGET,
        "a 12 MP photo's PNG is what blew the body up; got {} KB",
        png.len() / 1024
    );
    let jpeg = encode(&small, Format::Jpeg, 82).unwrap();
    assert!(
        base64_len(jpeg.len()) < 512 * 1024,
        "the picture must fit comfortably in a request; got {} KB of base64",
        base64_len(jpeg.len()) / 1024
    );
    // The point of the change: an order of magnitude, not a trim.
    assert!(
        jpeg.len() * 5 < png.len(),
        "jpeg {} KB vs png {} KB is not the saving this is for",
        jpeg.len() / 1024,
        png.len() / 1024
    );
}

#[test]
fn a_screenshot_stays_a_png_so_small_text_survives() {
    let small = shrink(&screenshot(1400, 960));
    let png = encode(&small, Format::Png, 100).unwrap();
    assert!(
        png.len() <= PNG_BUDGET,
        "a flat picture should stay under the budget and keep its edges; \
         got {} KB",
        png.len() / 1024
    );
    assert!(
        base64_len(png.len()) < 512 * 1024,
        "and still be a reasonable body: {} KB",
        base64_len(png.len()) / 1024
    );
}

#[test]
fn the_budget_sits_between_the_two_kinds_of_picture() {
    let flat = encode(&shrink(&screenshot(1400, 960)), Format::Png, 100)
        .unwrap()
        .len();
    let rich = encode(&shrink(&photo(4000, 3000)), Format::Png, 100)
        .unwrap()
        .len();
    assert!(
        flat < PNG_BUDGET && PNG_BUDGET < rich,
        "the budget must separate them, not clip one fine: \
         flat {} KB, budget {} KB, photo {} KB",
        flat / 1024,
        PNG_BUDGET / 1024,
        rich / 1024
    );
}
