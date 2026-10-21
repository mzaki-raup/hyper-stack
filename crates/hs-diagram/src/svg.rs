//! Scene to SVG. The same scene feeds the egui painter, PNG and PDF, so this
//! writer is the single source of exported geometry.

use crate::model::{NodeKind, Point, Style};
use crate::scene::{Prim, Scene, default_fill, default_stroke};
use crate::text::{Rect, Size, approx_measure};

#[derive(Debug, Clone)]
pub struct SvgOptions {
    pub background: Option<String>,
    pub font_family: String,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            background: Some("#ffffff".into()),
            font_family: "Segoe UI, Helvetica Neue, Arial, sans-serif".into(),
        }
    }
}

pub fn to_svg(scene: &Scene, options: &SvgOptions) -> String {
    let mut out = String::new();
    let w = scene.size.w.max(1.0);
    let h = scene.size.h.max(1.0);
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.2}\" height=\"{h:.2}\" viewBox=\"0 0 {w:.2} {h:.2}\">\n"
    ));
    out.push_str(
        "<defs><marker id=\"hs-arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\"><path d=\"M 0 0 L 10 5 L 0 10 z\" fill=\"#5b6472\"/></marker></defs>\n",
    );
    if let Some(bg) = &options.background {
        out.push_str(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{w:.2}\" height=\"{h:.2}\" fill=\"{}\"/>\n",
            escape(bg)
        ));
    }
    out.push_str(&format!(
        "<g font-family=\"{}\" fill=\"#1f2328\">\n",
        escape(&options.font_family)
    ));
    for prim in &scene.prims {
        match prim {
            Prim::Frame {
                rect,
                label,
                style,
                label_placement,
                label_align,
                ..
            } => {
                let fill = style.fill.as_deref().unwrap_or("none");
                let stroke = style.stroke.as_deref().unwrap_or("#8a94a6");
                out.push_str(&format!(
                    "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" rx=\"10\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.2\" stroke-dasharray=\"6 4\"/>\n",
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    escape(fill),
                    escape(stroke)
                ));
                if !label.is_empty() {
                    let (x, anchor) = match label_align {
                        crate::model::LabelAlign::Left => (rect.x + 10.0, "start"),
                        crate::model::LabelAlign::Centre => (rect.center().x, "middle"),
                        crate::model::LabelAlign::Right => (rect.right() - 10.0, "end"),
                    };
                    let y = match label_placement {
                        crate::model::LabelPlacement::Top => rect.y - 6.0,
                        crate::model::LabelPlacement::Bottom => rect.bottom() + 14.0,
                    };
                    text(
                        &mut out,
                        Point { x, y },
                        label,
                        12.0,
                        style.text.as_deref().unwrap_or("#5b6472"),
                        anchor,
                        true,
                        false,
                        false,
                    );
                }
            }
            Prim::Shape {
                rect,
                kind,
                label: _,
                style,
            } => draw_shape(&mut out, *rect, *kind, style),
            Prim::Line {
                points,
                dashed,
                dotted,
                arrow_start,
                arrow_end,
                style,
                ..
            } => {
                if points.len() < 2 {
                    continue;
                }
                let stroke = style.stroke.as_deref().unwrap_or("#5b6472");
                let points: Vec<String> = points
                    .iter()
                    .map(|p| format!("{:.2},{:.2}", p.x, p.y))
                    .collect();
                let dash = if *dotted {
                    " stroke-dasharray=\"2 4\""
                } else if *dashed || style.dashed {
                    " stroke-dasharray=\"6 4\""
                } else {
                    ""
                };
                let start = if *arrow_start {
                    " marker-start=\"url(#hs-arrow)\""
                } else {
                    ""
                };
                let end = if *arrow_end {
                    " marker-end=\"url(#hs-arrow)\""
                } else {
                    ""
                };
                out.push_str(&format!(
                    "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.6\" stroke-linejoin=\"round\" stroke-linecap=\"round\"{dash}{start}{end}/>\n",
                    points.join(" "),
                    escape(stroke)
                ));
            }
            Prim::Text {
                at,
                label,
                boxed,
                style,
                ..
            } => {
                let size = style.font_size.unwrap_or(12.0);
                if *boxed {
                    let m = approx_measure(label);
                    out.push_str(&format!(
                        "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" rx=\"4\" fill=\"#ffffffee\" stroke=\"#d0d5dd\" stroke-width=\"0.8\"/>\n",
                        at.x - m.w / 2.0 - 6.0,
                        at.y - 9.0,
                        m.w + 12.0,
                        18.0
                    ));
                }
                text(
                    &mut out,
                    *at,
                    label,
                    size,
                    style.text.as_deref().unwrap_or("#1f2328"),
                    "middle",
                    style.bold,
                    style.italic,
                    true,
                );
            }
        }
        // The label of a shape is drawn after its geometry; the shape helper
        // needs the style for the text colour.
        if let Prim::Shape {
            rect,
            kind: _,
            label,
            style,
        } = prim
        {
            text(
                &mut out,
                rect.center(),
                label,
                style.font_size.unwrap_or(14.0),
                style.text.as_deref().unwrap_or("#1f2328"),
                "middle",
                style.bold,
                style.italic,
                true,
            );
        }
    }
    out.push_str("</g>\n</svg>\n");
    out
}

fn draw_shape(out: &mut String, rect: Rect, kind: NodeKind, style: &Style) {
    let fill = style.fill.as_deref().unwrap_or(default_fill(kind));
    let stroke = style.stroke.as_deref().unwrap_or(default_stroke(kind));
    let dash = if style.dashed {
        " stroke-dasharray=\"6 4\""
    } else {
        ""
    };
    let (x, y, w, h) = (rect.x, rect.y, rect.w, rect.h);
    let right = rect.right();
    let bottom = rect.bottom();
    match kind {
        NodeKind::Text => {}
        NodeKind::Ellipse => {
            out.push_str(&format!(
                "<ellipse cx=\"{:.2}\" cy=\"{:.2}\" rx=\"{:.2}\" ry=\"{:.2}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.4\"{dash}/>\n",
                x + w / 2.0,
                y + h / 2.0,
                w / 2.0,
                h / 2.0,
                escape(fill),
                escape(stroke)
            ));
        }
        NodeKind::Decision => {
            out.push_str(&format!(
                "<polygon points=\"{:.2},{:.2} {:.2},{:.2} {:.2},{:.2} {:.2},{:.2}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.4\"{dash}/>\n",
                x + w / 2.0,
                y,
                right,
                y + h / 2.0,
                x + w / 2.0,
                bottom,
                x,
                y + h / 2.0,
                escape(fill),
                escape(stroke)
            ));
        }
        NodeKind::StartEnd => {
            out.push_str(&format!(
                "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\" rx=\"{:.2}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.4\"{dash}/>\n",
                h / 2.0,
                escape(fill),
                escape(stroke)
            ));
        }
        NodeKind::InputOutput => {
            let skew = 14.0f32.min(w * 0.2);
            out.push_str(&format!(
                "<polygon points=\"{:.2},{y:.2} {right:.2},{y:.2} {:.2},{bottom:.2} {x:.2},{bottom:.2}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.4\"{dash}/>\n",
                x + skew,
                right - skew,
                escape(fill),
                escape(stroke)
            ));
        }
        NodeKind::Database => {
            let ry = (h * 0.12).min(10.0);
            out.push_str(&format!(
                "<path d=\"M {x:.2} {:.2} L {x:.2} {:.2} A {:.2} {ry:.2} 0 0 0 {right:.2} {:.2} L {right:.2} {:.2} A {:.2} {ry:.2} 0 0 1 {x:.2} {:.2} Z\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.4\"/>\n",
                y + ry,
                bottom - ry,
                w / 2.0,
                bottom - ry,
                y + ry,
                w / 2.0,
                y + ry,
                escape(fill),
                escape(stroke)
            ));
            out.push_str(&format!(
                "<ellipse cx=\"{:.2}\" cy=\"{:.2}\" rx=\"{:.2}\" ry=\"{ry:.2}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.4\"/>\n",
                x + w / 2.0,
                y + ry,
                w / 2.0,
                escape(fill),
                escape(stroke)
            ));
        }
        NodeKind::Note => {
            let fold = 16.0f32.min(w * 0.25).min(h * 0.25);
            out.push_str(&format!(
                "<polygon points=\"{x:.2},{y:.2} {:.2},{y:.2} {right:.2},{:.2} {right:.2},{bottom:.2} {x:.2},{bottom:.2}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.4\"{dash}/>\n",
                right - fold,
                y + fold,
                escape(fill),
                escape(stroke)
            ));
        }
        NodeKind::Container => {
            out.push_str(&format!(
                "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\" rx=\"10\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.2\" stroke-dasharray=\"6 4\"/>\n",
                escape(stroke)
            ));
        }
        _ => {
            let r = style
                .radius
                .unwrap_or(if kind == NodeKind::RoundedRectangle {
                    14.0
                } else {
                    6.0
                });
            out.push_str(&format!(
                "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\" rx=\"{r:.2}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.4\"{dash}/>\n",
                escape(fill),
                escape(stroke)
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn text(
    out: &mut String,
    at: Point,
    label: &str,
    size: f32,
    fill: &str,
    anchor: &str,
    bold: bool,
    italic: bool,
    boxed_offset: bool,
) {
    let lines: Vec<&str> = label.lines().collect();
    if lines.is_empty() {
        return;
    }
    let lh = size * 1.25;
    let first = if boxed_offset {
        at.y - (lines.len() as f32 - 1.0) * lh / 2.0
    } else {
        at.y
    };
    let weight = if bold { " font-weight=\"600\"" } else { "" };
    let italic = if italic { " font-style=\"italic\"" } else { "" };
    out.push_str(&format!(
        "<text x=\"{:.2}\" y=\"{:.2}\" font-size=\"{size:.2}\" fill=\"{}\" text-anchor=\"{anchor}\" dominant-baseline=\"middle\"{weight}{italic}>",
        at.x,
        first,
        escape(fill)
    ));
    for (i, line) in lines.iter().enumerate() {
        let x = at.x;
        let y = first + i as f32 * lh;
        out.push_str(&format!(
            "<tspan x=\"{x:.2}\" y=\"{y:.2}\">{}</tspan>",
            escape(line)
        ));
    }
    out.push_str("</text>\n");
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// A scene with a single shape, for tests and the exporter.
pub fn probe_scene(size: Size) -> Scene {
    let mut scene = Scene::new(size);
    scene.push(Prim::Shape {
        rect: Rect::new(10.0, 10.0, size.w - 20.0, size.h - 20.0),
        kind: NodeKind::Process,
        label: "Probe".into(),
        style: Style::default(),
    });
    scene
}
