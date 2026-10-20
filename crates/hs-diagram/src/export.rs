//! PNG and PDF export through the SVG writer, so screen and files agree.

use crate::scene::Scene;
use crate::svg::{SvgOptions, to_svg};

#[derive(Debug, Clone)]
pub struct PngOptions {
    pub scale: f32,
    /// `None` makes the background transparent.
    pub background: Option<String>,
}

impl Default for PngOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            background: Some("#ffffff".into()),
        }
    }
}

fn usvg_options() -> resvg::usvg::Options<'static> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    options
}

pub fn svg(scene: &Scene, background: Option<&str>) -> String {
    to_svg(
        scene,
        &SvgOptions {
            background: background.map(str::to_string),
            ..Default::default()
        },
    )
}

pub fn png(scene: &Scene, options: &PngOptions) -> Result<Vec<u8>, String> {
    let svg = svg(scene, options.background.as_deref());
    let tree = resvg::usvg::Tree::from_str(&svg, &usvg_options()).map_err(|e| e.to_string())?;
    let scale = options.scale.clamp(0.25, 4.0);
    let size = tree.size();
    let w = ((size.width() * scale).ceil() as u32).max(1);
    let h = ((size.height() * scale).ceil() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| format!("cannot allocate a {w}×{h} pixmap"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| e.to_string())
}

pub fn pdf(scene: &Scene, background: Option<&str>) -> Result<Vec<u8>, String> {
    let svg = svg(scene, background);
    let mut options = svg2pdf::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = svg2pdf::usvg::Tree::from_str(&svg, &options).map_err(|e| e.to_string())?;
    svg2pdf::to_pdf(
        &tree,
        svg2pdf::ConversionOptions::default(),
        svg2pdf::PageOptions::default(),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::probe_scene;
    use crate::text::Size;

    #[test]
    fn png_has_the_png_signature() {
        let scene = probe_scene(Size::new(200.0, 120.0));
        let bytes = png(&scene, &PngOptions::default()).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn pdf_starts_with_a_pdf_header() {
        let scene = probe_scene(Size::new(200.0, 120.0));
        let bytes = pdf(&scene, Some("#ffffff")).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }
}
