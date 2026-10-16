//! The little icon a site puts beside its name.
//!
//! WebView2 does not hand one to us through wry, so the browser asks the
//! site directly: `/favicon.ico` on the page's own origin, which every site
//! that has one answers. What comes back is normalised to a small PNG here,
//! because a favicon can be an ICO with several sizes in it and the UI wants
//! one bitmap at one size.

use std::time::Duration;

/// Pixels on a side. A tab draws it at 14 points; 32 covers that on a
/// 2x display without carrying a 256-pixel icon around.
const SIZE: u32 = 32;

/// The favicon URL for a page, or `None` when the page has no origin to ask
/// (a local file, `about:blank`, a data URL).
pub fn url_for(page_url: &str) -> Option<String> {
    let (scheme, rest) = page_url.split_once("://")?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{authority}/favicon.ico"))
}

/// The origin a favicon belongs to, used as the cache key so every page on
/// one site shares one icon.
pub fn origin_of(page_url: &str) -> Option<String> {
    let (scheme, rest) = page_url.split_once("://")?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (!authority.is_empty()).then(|| format!("{scheme}://{authority}"))
}

/// Fetch one favicon and re-encode it as a small PNG.
///
/// Anything that is not an image, or is too big to be a favicon, is an
/// error rather than a guess: the tab falls back to its glyph.
pub async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("{}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    // A favicon is kilobytes. Anything far past that is not one, and
    // decoding it would be work done for nothing.
    if bytes.len() > 1024 * 1024 {
        return Err("too large for a favicon".into());
    }
    to_png(&bytes)
}

/// Decode whatever came back and write it out as a `SIZE`-square PNG.
pub fn to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let small = image.resize_exact(SIZE, SIZE, image::imageops::FilterType::Lanczos3);
    let mut out = Vec::new();
    small
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_favicon_lives_on_the_pages_own_origin() {
        assert_eq!(
            url_for("https://example.com/a/b?c=1#d").as_deref(),
            Some("https://example.com/favicon.ico")
        );
        assert_eq!(
            url_for("http://127.0.0.1:8080/").as_deref(),
            Some("http://127.0.0.1:8080/favicon.ico"),
            "the port is part of the origin"
        );
        assert_eq!(
            origin_of("https://example.com/a").as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn a_page_with_no_origin_has_no_favicon_to_ask_for() {
        for url in [
            "file:///C:/tmp/page.html",
            "about:blank",
            "data:text/html,hi",
            "",
            "https://",
        ] {
            assert_eq!(url_for(url), None, "{url}");
            assert_eq!(origin_of(url), None, "{url}");
        }
    }

    #[test]
    fn whatever_comes_back_leaves_as_one_small_png() {
        // A 1x1 PNG, which is a legitimate favicon and the wrong size.
        let src = image::RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]));
        let mut raw = Vec::new();
        image::DynamicImage::ImageRgba8(src)
            .write_to(&mut std::io::Cursor::new(&mut raw), image::ImageFormat::Png)
            .expect("the fixture encodes");
        let out = to_png(&raw).expect("a PNG decodes");
        let back = image::load_from_memory(&out).expect("the result is an image");
        assert_eq!((back.width(), back.height()), (SIZE, SIZE));
    }

    #[test]
    fn something_that_is_not_an_image_is_an_error_not_a_guess() {
        assert!(to_png(b"<!doctype html><title>not an icon</title>").is_err());
    }
}
