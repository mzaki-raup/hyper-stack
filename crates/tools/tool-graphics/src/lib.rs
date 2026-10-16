//! HyperStack graphics tool: open a picture (from the editor's project tree
//! or a file dialog), view it, stack operations on it — resize, scale,
//! extend the canvas, rotate, flip, crop, brightness, contrast, hue,
//! saturation, grayscale, invert, blur, sharpen — convert between formats,
//! extract the frames of an icon or build a multi-size ICO, and save. The
//! operations are re-applied to the original, so any step can be removed.
//! The review-only AI assistant sees the picture's metadata and the
//! operation list, never the pixels.

pub mod ops;

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, Printable, Speakable, ToastLevel, Tool, ToolCategory,
    ToolContext, ToolDescriptor, ToolFactory,
};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use image::{DynamicImage, GenericImageView};

use ops::{Format, IMAGE_EXTS, Op};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "graphics",
    title_key: "tool-graphics",
    icon: ph::IMAGE,
    category: ToolCategory::Utilities,
    accepts: &[Accept::LocalFile],
};

pub struct GraphicsFactory;

impl ToolFactory for GraphicsFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, _services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let mut t = GraphicsTool::default();
        if let Some(p) = restore
            .as_ref()
            .and_then(|v| v.get("path"))
            .and_then(|v| v.as_str())
        {
            let _ = t.load(Path::new(p));
        }
        if let Some(ops) = restore
            .as_ref()
            .and_then(|v| v.get("ops"))
            .and_then(|v| serde_json::from_value::<Vec<Op>>(v.clone()).ok())
        {
            t.ops = ops;
            t.dirty = true;
        }
        Box::new(t)
    }
}

pub use hs_core::sniff::is_image_path;

#[derive(Default)]
struct Controls {
    width: u32,
    height: u32,
    keep_aspect: bool,
    scale: f32,
    extend: [u32; 4],
    extend_colour: [u8; 4],
    crop: [u32; 4],
    brightness: i32,
    contrast: f32,
    hue: i32,
    saturation: f32,
    blur: f32,
    sharpen: f32,
    ico_sizes: [bool; 7],
}

/// Which part of the crop placeholder a drag took hold of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CropGrip {
    Move,
    N,
    S,
    E,
    W,
    Ne,
    Nw,
    Se,
    Sw,
    /// A drag that started outside the rectangle draws a new one.
    New,
}

pub struct GraphicsTool {
    path: Option<PathBuf>,
    original: Option<DynamicImage>,
    original_bytes: Vec<u8>,
    ops: Vec<Op>,
    current: Option<DynamicImage>,
    texture: Option<egui::TextureHandle>,
    dirty: bool,
    /// The picture after the committed operations; the Adjust sliders are
    /// previewed on top of it without re-running the whole list.
    base: Option<DynamicImage>,
    /// A slider moved: re-render the preview layer (debounced a little so
    /// a blur does not run on every pointer event).
    preview_dirty: bool,
    preview_at: Option<std::time::Instant>,
    zoom: f32,
    format: Format,
    quality: u8,
    controls: Controls,
    frames: Vec<(u32, u32, DynamicImage)>,
    error: Option<String>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
    /// The crop placeholder is shown on the canvas and can be dragged.
    crop_select: bool,
    /// An active drag: the grip, the rectangle when it started and the
    /// press position in image pixels.
    crop_drag: Option<(CropGrip, [u32; 4], egui::Pos2)>,
}

impl Default for GraphicsTool {
    fn default() -> Self {
        Self {
            path: None,
            original: None,
            original_bytes: Vec::new(),
            ops: Vec::new(),
            current: None,
            texture: None,
            dirty: false,
            base: None,
            preview_dirty: false,
            preview_at: None,
            zoom: 0.0,
            format: Format::Png,
            quality: 90,
            controls: Controls {
                keep_aspect: true,
                scale: 2.0,
                extend_colour: [255, 255, 255, 255],
                blur: 0.0,
                sharpen: 0.0,
                ico_sizes: [true, false, true, true, false, false, true],
                ..Default::default()
            },
            frames: Vec::new(),
            error: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
            crop_select: false,
            crop_drag: None,
        }
    }
}

const ICO_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

impl GraphicsTool {
    fn load(&mut self, path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
        self.frames = ops::ico_frames(&bytes);
        let (w, h) = img.dimensions();
        self.controls.width = w;
        self.controls.height = h;
        self.controls.crop = [0, 0, w, h];
        self.format = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Format::from_ext)
            .unwrap_or(Format::Png);
        self.path = Some(path.to_path_buf());
        self.original = Some(img);
        self.original_bytes = bytes;
        self.ops.clear();
        self.current = None;
        self.texture = None;
        self.dirty = true;
        self.error = None;
        Ok(())
    }

    fn push(&mut self, op: Op) {
        self.ops.push(op);
        self.dirty = true;
    }

    /// The Adjust sliders that are not at their neutral value, as the
    /// operations they would commit, in panel order.
    fn pending_ops(&self) -> Vec<Op> {
        let c = &self.controls;
        let mut out = Vec::new();
        if c.brightness != 0 {
            out.push(Op::Brightness {
                value: c.brightness,
            });
        }
        if c.contrast != 0.0 {
            out.push(Op::Contrast { value: c.contrast });
        }
        if c.hue != 0 {
            out.push(Op::Hue { degrees: c.hue });
        }
        if c.saturation != 0.0 {
            out.push(Op::Saturation {
                value: c.saturation,
            });
        }
        if c.blur > 0.0 {
            out.push(Op::Blur { sigma: c.blur });
        }
        if c.sharpen > 0.0 {
            out.push(Op::Sharpen {
                sigma: c.sharpen,
                threshold: 2,
            });
        }
        out
    }

    /// Turn the live slider preview into committed operations.
    fn commit_pending(&mut self) {
        let pending = self.pending_ops();
        if pending.is_empty() {
            return;
        }
        self.ops.extend(pending);
        let c = &mut self.controls;
        c.brightness = 0;
        c.contrast = 0.0;
        c.hue = 0;
        c.saturation = 0.0;
        c.blur = 0.0;
        c.sharpen = 0.0;
        self.dirty = true;
    }

    /// A slider moved: show it on the picture shortly.
    fn preview_changed(&mut self) {
        self.preview_dirty = true;
        self.preview_at = Some(std::time::Instant::now());
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        const SETTLE: std::time::Duration = std::time::Duration::from_millis(60);
        if self.dirty {
            self.dirty = false;
            self.base = self.original.as_ref().map(|o| ops::apply_all(o, &self.ops));
            self.preview_dirty = true;
            self.preview_at = None;
        }
        if !self.preview_dirty {
            return;
        }
        if let Some(at) = self.preview_at {
            let since = at.elapsed();
            if since < SETTLE {
                ctx.request_repaint_after(SETTLE - since);
                return;
            }
        }
        self.preview_dirty = false;
        let Some(base) = &self.base else {
            self.current = None;
            self.texture = None;
            return;
        };
        let img = ops::apply_all(base, &self.pending_ops());
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let colour =
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
        self.texture = Some(ctx.load_texture(
            format!("hs-graphics-{}", hs_core::new_id()),
            colour,
            egui::TextureOptions::LINEAR,
        ));
        self.controls.width = w;
        self.controls.height = h;
        // The crop placeholder follows the picture: a rectangle left over
        // from a bigger picture is clamped, an empty or out-of-range one
        // becomes the whole picture.
        let [x, y, cw, ch] = self.controls.crop;
        self.controls.crop = if cw == 0 || ch == 0 || x >= w || y >= h {
            [0, 0, w, h]
        } else {
            [x, y, cw.min(w - x), ch.min(h - y)]
        };
        self.current = Some(img);
    }

    fn info_line(&self) -> String {
        match (&self.original, &self.current) {
            (Some(o), Some(c)) => format!(
                "{}×{} → {}×{} · {} · {}",
                o.width(),
                o.height(),
                c.width(),
                c.height(),
                human_size(self.original_bytes.len()),
                self.path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            ),
            _ => String::new(),
        }
    }

    fn save_as(&mut self, cx: &mut ToolContext) {
        self.commit_pending();
        let Some(img) = &self.current else {
            return;
        };
        let s = cx.services;
        let stem = self
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "image".into());
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.{}", self.format.ext()))
            .add_filter(self.format.label(), &[self.format.ext()])
            .save_file()
        else {
            return;
        };
        match ops::encode(img, self.format, self.quality)
            .and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string()))
        {
            Ok(()) => cx.toast(
                ToastLevel::Success,
                s.i18n
                    .t_args("gfx-saved", &[("path", &path.display().to_string())]),
            ),
            Err(e) => cx.toast(ToastLevel::Error, e),
        }
    }

    fn save_ico(&mut self, cx: &mut ToolContext) {
        self.commit_pending();
        let Some(img) = &self.current else {
            return;
        };
        let sizes: Vec<u32> = ICO_SIZES
            .iter()
            .zip(self.controls.ico_sizes.iter())
            .filter(|(_, on)| **on)
            .map(|(s, _)| *s)
            .collect();
        if sizes.is_empty() {
            cx.toast(ToastLevel::Warning, cx.services.t("gfx-ico-pick-size"));
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("icon.ico")
            .add_filter("ICO", &["ico"])
            .save_file()
        else {
            return;
        };
        match ops::multi_ico(img, &sizes)
            .and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string()))
        {
            Ok(()) => cx.toast(
                ToastLevel::Success,
                cx.services
                    .i18n
                    .t_args("gfx-saved", &[("path", &path.display().to_string())]),
            ),
            Err(e) => cx.toast(ToastLevel::Error, e),
        }
    }

    fn sidebar_sections(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        let has = self.original.is_some();
        let apply = s.t("gfx-apply");
        // Transform
        form::section(ui, theme, &s.t("gfx-transform"), |ui| {
            ui.add_enabled_ui(has, |ui| {
                Rows::show(ui, theme, "gfx-rows-transform", |rows| {
                    rows.title(&s.t("gfx-resize"));
                    let (ow, oh) = self
                        .current
                        .as_ref()
                        .map(|i| i.dimensions())
                        .unwrap_or((1, 1));
                    let before = (self.controls.width, self.controls.height);
                    if rows.row(&s.t("gfx-size"), Some(&apply), |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.controls.width)
                                .range(1..=20000)
                                .prefix("W "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.controls.height)
                                .range(1..=20000)
                                .prefix("H "),
                        );
                        ui.checkbox(&mut self.controls.keep_aspect, s.t("gfx-keep-aspect"));
                    }) {
                        self.push(Op::Resize {
                            width: self.controls.width,
                            height: self.controls.height,
                        });
                    }
                    if self.controls.keep_aspect {
                        if self.controls.width != before.0 {
                            self.controls.height = ((self.controls.width as f64) * (oh as f64)
                                / (ow as f64))
                                .round()
                                .max(1.0) as u32;
                        } else if self.controls.height != before.1 {
                            self.controls.width = ((self.controls.height as f64) * (ow as f64)
                                / (oh as f64))
                                .round()
                                .max(1.0) as u32;
                        }
                    }
                    if rows.row(&s.t("gfx-scale"), Some(&apply), |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.controls.scale)
                                .range(0.05..=16.0)
                                .speed(0.05)
                                .suffix("×"),
                        );
                        let (w, h) = (
                            ((ow as f32) * self.controls.scale).round() as u32,
                            ((oh as f32) * self.controls.scale).round() as u32,
                        );
                        ui.label(
                            egui::RichText::new(format!("→ {w}×{h}"))
                                .small()
                                .color(theme.text_muted),
                        );
                    }) {
                        self.push(Op::Scale {
                            factor: self.controls.scale,
                        });
                    }
                    rows.space(6.0);
                    rows.title(&s.t("gfx-extend"));
                    rows.row(&s.t("gfx-margins"), None, |ui| {
                        for (i, l) in ["L", "T", "R", "B"].iter().enumerate() {
                            ui.add(
                                egui::DragValue::new(&mut self.controls.extend[i])
                                    .range(0..=4000)
                                    .prefix(format!("{l} ")),
                            );
                        }
                    });
                    if rows.row(&s.t("gfx-extend-colour"), Some(&apply), |ui| {
                        let mut c = egui::Color32::from_rgba_unmultiplied(
                            self.controls.extend_colour[0],
                            self.controls.extend_colour[1],
                            self.controls.extend_colour[2],
                            self.controls.extend_colour[3],
                        );
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            self.controls.extend_colour = c.to_srgba_unmultiplied();
                        }
                        ui.label(
                            egui::RichText::new(format!(
                                "#{:02X}{:02X}{:02X}",
                                self.controls.extend_colour[0],
                                self.controls.extend_colour[1],
                                self.controls.extend_colour[2]
                            ))
                            .small()
                            .monospace()
                            .color(theme.text_muted),
                        );
                    }) {
                        let [l, t, r, b] = self.controls.extend;
                        self.push(Op::Extend {
                            left: l,
                            top: t,
                            right: r,
                            bottom: b,
                            colour: self.controls.extend_colour,
                        });
                    }
                    rows.space(6.0);
                    rows.title(&s.t("gfx-crop"));
                    let (iw, ih) = (ow, oh);
                    rows.row(&s.t("gfx-crop-select"), None, |ui| {
                        if ui
                            .selectable_label(
                                self.crop_select,
                                format!("{} {}", ph::SELECTION, s.t("gfx-crop-on-canvas")),
                            )
                            .on_hover_text(s.t("gfx-crop-on-canvas-tip"))
                            .clicked()
                        {
                            self.crop_select = !self.crop_select;
                            let [_, _, w, h] = self.controls.crop;
                            if self.crop_select && (w == 0 || h == 0) {
                                // Start from the whole picture, inset a little.
                                let (mx, my) = (iw / 8, ih / 8);
                                self.controls.crop = [mx, my, iw - 2 * mx, ih - 2 * my];
                            }
                        }
                    });
                    if rows.row(&s.t("gfx-area"), Some(&apply), |ui| {
                        for (i, l) in ["X", "Y", "W", "H"].iter().enumerate() {
                            ui.add(
                                egui::DragValue::new(&mut self.controls.crop[i])
                                    .range(0..=20000)
                                    .prefix(format!("{l} ")),
                            );
                        }
                    }) {
                        let [x, y, w, h] = self.controls.crop;
                        self.push(Op::Crop {
                            x,
                            y,
                            width: w.max(1),
                            height: h.max(1),
                        });
                        self.crop_select = false;
                    }
                    rows.space(6.0);
                    rows.title(&s.t("gfx-orientation"));
                    let mut pushed = None;
                    rows.row(&s.t("gfx-rotate"), None, |ui| {
                        for (icon, tip, op) in [
                            (ph::ARROW_CLOCKWISE, "gfx-rotate-cw", Op::Rotate90),
                            (ph::ARROW_COUNTER_CLOCKWISE, "gfx-rotate-ccw", Op::Rotate270),
                            (ph::ARROWS_CLOCKWISE, "gfx-rotate-180", Op::Rotate180),
                        ] {
                            if ui.button(icon).on_hover_text(s.t(tip)).clicked() {
                                pushed = Some(op);
                            }
                        }
                    });
                    rows.row(&s.t("gfx-flip"), None, |ui| {
                        for (icon, tip, op) in [
                            (ph::ARROWS_HORIZONTAL, "gfx-flip-h", Op::FlipH),
                            (ph::ARROWS_VERTICAL, "gfx-flip-v", Op::FlipV),
                        ] {
                            if ui.button(icon).on_hover_text(s.t(tip)).clicked() {
                                pushed = Some(op);
                            }
                        }
                    });
                    if let Some(op) = pushed {
                        self.push(op);
                    }
                });
            });
        });
        // Adjust
        form::section(ui, theme, &s.t("gfx-adjust"), |ui| {
            ui.add_enabled_ui(has, |ui| {
                form::hint(ui, theme, &s.t("gfx-adjust-hint"));
                let mut changed = false;
                let mut commit: Option<usize> = None;
                let c = &mut self.controls;
                let rows: [(&str, String, egui::Slider<'_>); 6] = [
                    (
                        "gfx-brightness",
                        c.brightness.to_string(),
                        egui::Slider::new(&mut c.brightness, -100..=100),
                    ),
                    (
                        "gfx-contrast",
                        format!("{:.0}", c.contrast),
                        egui::Slider::new(&mut c.contrast, -100.0..=100.0),
                    ),
                    (
                        "gfx-hue",
                        format!("{}°", c.hue),
                        egui::Slider::new(&mut c.hue, 0..=360),
                    ),
                    (
                        "gfx-saturation",
                        format!("{:.0}", c.saturation),
                        egui::Slider::new(&mut c.saturation, -100.0..=100.0),
                    ),
                    (
                        "gfx-blur",
                        format!("{:.1}", c.blur),
                        egui::Slider::new(&mut c.blur, 0.0..=20.0),
                    ),
                    (
                        "gfx-sharpen",
                        format!("{:.1}", c.sharpen),
                        egui::Slider::new(&mut c.sharpen, 0.0..=10.0),
                    ),
                ];
                for (i, (key, value, slider)) in rows.into_iter().enumerate() {
                    let r = slider_block(ui, theme, &s.t(key), &value, &apply, slider);
                    changed |= r.changed;
                    if r.applied {
                        commit = Some(i);
                    }
                }
                if changed {
                    self.preview_changed();
                }
                if let Some(i) = commit {
                    // Commit this slider only: its operation joins the list
                    // and the slider returns to neutral.
                    let c = &mut self.controls;
                    let op = match i {
                        0 => (c.brightness != 0).then(|| {
                            let v = std::mem::take(&mut c.brightness);
                            Op::Brightness { value: v }
                        }),
                        1 => (c.contrast != 0.0).then(|| {
                            let v = std::mem::take(&mut c.contrast);
                            Op::Contrast { value: v }
                        }),
                        2 => (c.hue != 0).then(|| {
                            let v = std::mem::take(&mut c.hue);
                            Op::Hue { degrees: v }
                        }),
                        3 => (c.saturation != 0.0).then(|| {
                            let v = std::mem::take(&mut c.saturation);
                            Op::Saturation { value: v }
                        }),
                        4 => (c.blur > 0.0).then(|| {
                            let v = std::mem::take(&mut c.blur);
                            Op::Blur { sigma: v }
                        }),
                        _ => (c.sharpen > 0.0).then(|| {
                            let v = std::mem::take(&mut c.sharpen);
                            Op::Sharpen {
                                sigma: v,
                                threshold: 2,
                            }
                        }),
                    };
                    if let Some(op) = op {
                        self.push(op);
                    }
                }
                ui.add_space(2.0);
                // Grayscale and Invert are toggles: clicking again takes the
                // operation back off the list.
                let gray_on = self.ops.iter().any(|o| matches!(o, Op::Grayscale));
                let inv_on = self.ops.iter().filter(|o| matches!(o, Op::Invert)).count() % 2 == 1;
                let mut toggle: Option<Op> = None;
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [LABEL_W, 22.0],
                        egui::Label::new(
                            egui::RichText::new(s.t("gfx-colour")).color(theme.text_muted),
                        ),
                    );
                    if ui.selectable_label(gray_on, s.t("gfx-grayscale")).clicked() {
                        toggle = Some(Op::Grayscale);
                    }
                    if ui.selectable_label(inv_on, s.t("gfx-invert")).clicked() {
                        toggle = Some(Op::Invert);
                    }
                });
                if let Some(op) = toggle {
                    let last_is_same = self.ops.last().is_some_and(|o| *o == op);
                    let on = match op {
                        Op::Grayscale => gray_on,
                        _ => inv_on,
                    };
                    if on && last_is_same {
                        self.ops.pop();
                        self.dirty = true;
                    } else if on {
                        // Switched on earlier in the list: take that one out.
                        if let Some(i) = self.ops.iter().rposition(|o| *o == op) {
                            self.ops.remove(i);
                            self.dirty = true;
                        }
                    } else {
                        self.push(op);
                    }
                }
            });
        });
        // Operations
        form::section(ui, theme, &s.t("gfx-operations"), |ui| {
            if self.ops.is_empty() {
                form::hint(ui, theme, &s.t("gfx-no-ops"));
            }
            let mut remove = None;
            for (i, op) in self.ops.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{}.", i + 1))
                            .monospace()
                            .color(theme.text_muted),
                    );
                    ui.label(op.label());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(hs_ui::icons::TRASH)
                            .on_hover_text(s.t("gfx-remove-op"))
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                self.ops.remove(i);
                self.dirty = true;
            }
            if !self.ops.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(s.t("gfx-undo")).clicked() {
                        self.ops.pop();
                        self.dirty = true;
                    }
                    if ui.button(s.t("gfx-reset")).clicked() {
                        self.ops.clear();
                        self.dirty = true;
                    }
                });
            }
        });
        // Icons
        form::section(ui, theme, &s.t("gfx-icons"), |ui| {
            ui.add_enabled_ui(has, |ui| {
                Rows::show(ui, theme, "gfx-rows-icons", |rows| {
                    if self.frames.len() > 1 {
                        let mut pick = None;
                        rows.row(&s.t("gfx-frames-label"), None, |ui| {
                            for (i, (w, h, _)) in self.frames.iter().enumerate() {
                                if ui.small_button(format!("{w}×{h}")).clicked() {
                                    pick = Some(i);
                                }
                            }
                        });
                        rows.hint(
                            &s.i18n
                                .t_args("gfx-frames", &[("count", &self.frames.len().to_string())]),
                        );
                        if let Some(i) = pick {
                            let img = self.frames[i].2.clone();
                            self.original = Some(img);
                            self.ops.clear();
                            self.dirty = true;
                        }
                        rows.space(4.0);
                    }
                    rows.title(&s.t("gfx-make-ico"));
                    rows.row(&s.t("gfx-sizes"), None, |ui| {
                        for (i, size) in ICO_SIZES.iter().enumerate() {
                            ui.checkbox(&mut self.controls.ico_sizes[i], size.to_string());
                        }
                    });
                });
                ui.add_space(4.0);
                if ui
                    .button(format!("{} {}", ph::FLOPPY_DISK, s.t("gfx-save-ico")))
                    .clicked()
                {
                    self.save_ico(cx);
                }
                form::hint(ui, theme, &s.t("gfx-ico-hint"));
            });
        });
        // Export
        form::section(ui, theme, &s.t("gfx-export"), |ui| {
            ui.add_enabled_ui(has, |ui| {
                Rows::show(ui, theme, "gfx-rows-export", |rows| {
                    rows.row(&s.t("gfx-format"), None, |ui| {
                        egui::ComboBox::from_id_salt("gfx-format")
                            .width(120.0)
                            .selected_text(self.format.label())
                            .show_ui(ui, |ui| {
                                for f in Format::ALL {
                                    ui.selectable_value(&mut self.format, f, f.label());
                                }
                            });
                    });
                });
                if self.format == Format::Jpeg {
                    let q = self.quality.to_string();
                    let _ = slider_block(
                        ui,
                        theme,
                        &s.t("gfx-quality"),
                        &q,
                        "",
                        egui::Slider::new(&mut self.quality, 1..=100),
                    );
                }
                ui.add_space(4.0);
                if form::primary_button(
                    ui,
                    theme,
                    &format!("{} {}", ph::FLOPPY_DISK, s.t("gfx-save-as")),
                )
                .clicked()
                {
                    self.save_as(cx);
                }
            });
        });
    }
}

/// What a [`slider_block`] reported this frame.
#[derive(Default, Clone, Copy)]
struct SliderBlock {
    /// The Apply button was clicked.
    applied: bool,
    /// The value moved (live preview).
    changed: bool,
}

/// A slider that reads like a photo editor's: the label, the current
/// value and the Apply button on one line, the slider itself full width
/// underneath (`apply` empty = no button).
fn slider_block(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    value: &str,
    apply: &str,
    slider: egui::Slider<'_>,
) -> SliderBlock {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(theme.text_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !apply.is_empty() {
                clicked = ui.small_button(apply).clicked();
            }
            ui.label(egui::RichText::new(value).monospace());
        });
    });
    ui.spacing_mut().slider_width = (ui.available_width() - 6.0).max(60.0);
    // The theme's inactive fill matches the panel, which hides the rail;
    // give the rail and the handle a visible fill for this widget only.
    let v = ui.visuals_mut();
    v.widgets.inactive.bg_fill = theme.border;
    v.widgets.hovered.bg_fill = theme.surface_hover;
    let changed = ui
        .add(slider.show_value(false).trailing_fill(true))
        .changed();
    ui.add_space(6.0);
    SliderBlock {
        applied: clicked,
        changed,
    }
}

/// Width of the label column in the sidebar rows.
const LABEL_W: f32 = 76.0;
/// Room kept for the Apply button column.
const APPLY_W: f32 = 56.0;

/// Aligned rows inside a section: a label column, a controls column and
/// a button column, laid out on one `egui::Grid` so every row lines up.
struct Rows<'a> {
    ui: &'a mut egui::Ui,
    theme: &'a Theme,
    width: f32,
}

impl Rows<'_> {
    fn show(ui: &mut egui::Ui, theme: &Theme, id: &str, add_rows: impl FnOnce(&mut Rows<'_>)) {
        let width = ui.available_width();
        egui::Grid::new(id)
            .num_columns(3)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                let mut rows = Rows { ui, theme, width };
                add_rows(&mut rows);
            });
    }

    /// Width of the controls column.
    fn controls_width(&self) -> f32 {
        (self.width - LABEL_W - APPLY_W - 16.0).max(120.0)
    }

    /// A bold group heading on its own row.
    fn title(&mut self, text: &str) {
        self.ui
            .label(egui::RichText::new(text).strong().color(self.theme.text));
        self.ui.label("");
        self.ui.label("");
        self.ui.end_row();
    }

    fn hint(&mut self, text: &str) {
        self.ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .small()
                    .color(self.theme.text_muted),
            )
            .wrap(),
        );
        self.ui.label("");
        self.ui.label("");
        self.ui.end_row();
    }

    /// An empty row of height `h` (`add_space` is not allowed in a grid).
    fn space(&mut self, h: f32) {
        self.ui
            .allocate_exact_size(egui::vec2(1.0, h), egui::Sense::hover());
        self.ui.label("");
        self.ui.label("");
        self.ui.end_row();
    }

    /// One row: label, controls, and (when `apply` is given) a button in
    /// the last column. Returns whether that button was clicked.
    fn row(
        &mut self,
        label: &str,
        apply: Option<&str>,
        controls: impl FnOnce(&mut egui::Ui),
    ) -> bool {
        let theme = self.theme;
        self.ui.scope(|ui| {
            ui.set_min_width(LABEL_W);
            ui.set_max_width(LABEL_W);
            ui.add(egui::Label::new(egui::RichText::new(label).color(theme.text_muted)).truncate())
                .on_hover_text(label);
        });
        let controls_w = self.controls_width();
        self.ui.scope(|ui| {
            ui.set_max_width(controls_w);
            ui.horizontal_wrapped(|ui| controls(ui));
        });
        let clicked = match apply {
            Some(text) => self.ui.small_button(text).clicked(),
            None => {
                self.ui.label("");
                false
            }
        };
        self.ui.end_row();
        clicked
    }
}

impl GraphicsTool {
    /// The crop placeholder: a rectangle over the picture that can be
    /// moved by its inside and resized by its edges and corners; a drag
    /// outside it draws a new one. Every change lands in the X/Y/W/H
    /// inputs of the Crop group (image pixels).
    fn crop_overlay(&mut self, ui: &mut egui::Ui, theme: &Theme, img_rect: egui::Rect, z: f32) {
        let Some((iw, ih)) = self.current.as_ref().map(|i| i.dimensions()) else {
            return;
        };
        let to_screen = |x: u32, y: u32| img_rect.min + egui::vec2(x as f32 * z, y as f32 * z);
        let to_image = |p: egui::Pos2| {
            let v = (p - img_rect.min) / z;
            egui::pos2(v.x.clamp(0.0, iw as f32), v.y.clamp(0.0, ih as f32))
        };
        let [cx, cy, cw, ch] = self.controls.crop;
        let rect = egui::Rect::from_min_max(to_screen(cx, cy), to_screen(cx + cw, cy + ch));
        let grip = 7.0;
        // A little outside the picture too, so an edge grip that sits on
        // the picture's own border can still be grabbed.
        let resp = ui.interact(
            img_rect.expand(grip),
            ui.id().with("gfx-crop-drag"),
            egui::Sense::click_and_drag(),
        );

        // Which grip is under the pointer (for the cursor and the drag).
        let zone_at = |p: egui::Pos2| -> CropGrip {
            let near = |a: f32, b: f32| (a - b).abs() <= grip;
            let inside_x = p.x >= rect.left() - grip && p.x <= rect.right() + grip;
            let inside_y = p.y >= rect.top() - grip && p.y <= rect.bottom() + grip;
            if !(inside_x && inside_y) {
                return CropGrip::New;
            }
            let (l, r, t, b) = (
                near(p.x, rect.left()),
                near(p.x, rect.right()),
                near(p.y, rect.top()),
                near(p.y, rect.bottom()),
            );
            match (l, r, t, b) {
                (true, _, true, _) => CropGrip::Nw,
                (_, true, true, _) => CropGrip::Ne,
                (true, _, _, true) => CropGrip::Sw,
                (_, true, _, true) => CropGrip::Se,
                (true, _, _, _) => CropGrip::W,
                (_, true, _, _) => CropGrip::E,
                (_, _, true, _) => CropGrip::N,
                (_, _, _, true) => CropGrip::S,
                _ => CropGrip::Move,
            }
        };
        if let Some(p) = resp.hover_pos() {
            let cursor = match self.crop_drag.map(|d| d.0).unwrap_or_else(|| zone_at(p)) {
                CropGrip::Move => egui::CursorIcon::Grab,
                CropGrip::N | CropGrip::S => egui::CursorIcon::ResizeVertical,
                CropGrip::E | CropGrip::W => egui::CursorIcon::ResizeHorizontal,
                CropGrip::Ne | CropGrip::Sw => egui::CursorIcon::ResizeNeSw,
                CropGrip::Nw | CropGrip::Se => egui::CursorIcon::ResizeNwSe,
                CropGrip::New => egui::CursorIcon::Crosshair,
            };
            ui.ctx().set_cursor_icon(cursor);
        }
        // egui reports a drag only once the pointer has moved past its
        // threshold, so the grip is judged from where the button went down,
        // not from where the pointer is by then (10 px off an edge reads as
        // "move the whole rectangle" otherwise).
        if resp.drag_started() {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| resp.interact_pointer_pos());
            if let Some(p) = origin {
                self.crop_drag = Some((zone_at(p), self.controls.crop, to_image(p)));
            }
        }
        if resp.dragged()
            && let Some((zone, start, from)) = self.crop_drag
            && let Some(p) = resp.interact_pointer_pos()
        {
            let now = to_image(p);
            let (dx, dy) = (
                (now.x - from.x).round() as i64,
                (now.y - from.y).round() as i64,
            );
            let [sx, sy, sw, sh] = start.map(|v| v as i64);
            let (iw, ih) = (iw as i64, ih as i64);
            let (mut x0, mut y0, mut x1, mut y1) = (sx, sy, sx + sw, sy + sh);
            match zone {
                CropGrip::Move => {
                    let dx = dx.clamp(-sx, iw - (sx + sw));
                    let dy = dy.clamp(-sy, ih - (sy + sh));
                    x0 += dx;
                    x1 += dx;
                    y0 += dy;
                    y1 += dy;
                }
                CropGrip::N => y0 += dy,
                CropGrip::S => y1 += dy,
                CropGrip::W => x0 += dx,
                CropGrip::E => x1 += dx,
                CropGrip::Nw => {
                    x0 += dx;
                    y0 += dy;
                }
                CropGrip::Ne => {
                    x1 += dx;
                    y0 += dy;
                }
                CropGrip::Sw => {
                    x0 += dx;
                    y1 += dy;
                }
                CropGrip::Se => {
                    x1 += dx;
                    y1 += dy;
                }
                CropGrip::New => {
                    x0 = from.x.round() as i64;
                    y0 = from.y.round() as i64;
                    x1 = now.x.round() as i64;
                    y1 = now.y.round() as i64;
                }
            }
            let (x0, x1) = (x0.min(x1).clamp(0, iw), x0.max(x1).clamp(0, iw));
            let (y0, y1) = (y0.min(y1).clamp(0, ih), y0.max(y1).clamp(0, ih));
            self.controls.crop = [
                x0 as u32,
                y0 as u32,
                (x1 - x0).max(1) as u32,
                (y1 - y0).max(1) as u32,
            ];
        }
        if resp.drag_stopped() {
            self.crop_drag = None;
        }

        // Paint: dim outside, outline, grips and the size.
        let [cx, cy, cw, ch] = self.controls.crop;
        let rect = egui::Rect::from_min_max(to_screen(cx, cy), to_screen(cx + cw, cy + ch));
        let p = ui.painter();
        let dim = egui::Color32::from_black_alpha(110);
        for r in [
            egui::Rect::from_min_max(img_rect.min, egui::pos2(img_rect.right(), rect.top())),
            egui::Rect::from_min_max(egui::pos2(img_rect.left(), rect.bottom()), img_rect.max),
            egui::Rect::from_min_max(egui::pos2(img_rect.left(), rect.top()), rect.left_bottom()),
            egui::Rect::from_min_max(
                rect.right_top(),
                egui::pos2(img_rect.right(), rect.bottom()),
            ),
        ] {
            p.rect_filled(r.intersect(img_rect), 0.0, dim);
        }
        p.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.5, theme.accent),
            egui::StrokeKind::Outside,
        );
        // Rule-of-thirds guides.
        for i in 1..3 {
            let f = i as f32 / 3.0;
            let x = rect.left() + rect.width() * f;
            let y = rect.top() + rect.height() * f;
            p.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(0.5, theme.accent.linear_multiply(0.5)),
            );
            p.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(0.5, theme.accent.linear_multiply(0.5)),
            );
        }
        for c in [
            rect.left_top(),
            rect.center_top(),
            rect.right_top(),
            rect.left_center(),
            rect.right_center(),
            rect.left_bottom(),
            rect.center_bottom(),
            rect.right_bottom(),
        ] {
            p.rect_filled(
                egui::Rect::from_center_size(c, egui::vec2(grip, grip)),
                1.0,
                theme.accent,
            );
        }
        p.text(
            rect.left_top() + egui::vec2(4.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{cw}×{ch} @ {cx},{cy}"),
            egui::FontId::proportional(11.0),
            theme.accent,
        );
    }
}

/// Hand a picture to the shell rather than loading it here: one tab per
/// image, and opening one that is already open focuses its tab instead of
/// making a second.
fn open_image(cx: &mut ToolContext, path: &Path) {
    cx.open(
        "graphics",
        Some(OpenRequest::File {
            uri: path.to_string_lossy().to_string(),
            line: None,
        }),
    );
}

fn human_size(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

impl Tool for GraphicsTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        match self.path.as_ref().and_then(|p| p.file_name()) {
            Some(n) => Cow::Owned(n.to_string_lossy().to_string()),
            None => Cow::Borrowed("Graphics"),
        }
    }

    fn dirty(&self) -> bool {
        !self.ops.is_empty() || !self.pending_ops().is_empty()
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        form::header_bar(ui, &theme, |ui| {
            ui.label(
                egui::RichText::new(s.t("tool-graphics"))
                    .strong()
                    .size(15.0),
            );
        });
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} {}", ph::FOLDER_OPEN, s.t("gfx-open")))
                .clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .add_filter("Images", IMAGE_EXTS)
                    .pick_file()
            {
                // Through the shell, so the picture lands in its own tab —
                // or focuses the tab already showing it.
                open_image(cx, &p);
            }
        });
        if self.original.is_none() {
            ui.add_space(6.0);
            form::note(ui, &theme, hs_ui::icons::INFO, &s.t("gfx-empty-hint"));
        }
        egui::ScrollArea::vertical()
            .id_salt("gfx-sidebar")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.sidebar_sections(ui, cx, &theme);
            });
        true
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let info = self.info_line();
        if !info.is_empty() {
            ui.label(egui::RichText::new(info).monospace().small());
            ui.separator();
        }
        if ui.button(s.t("gfx-zoom-fit")).clicked() {
            self.zoom = 0.0;
        }
        if ui.button("100%").clicked() {
            self.zoom = 1.0;
        }
        if ui.button(ph::MAGNIFYING_GLASS_PLUS).clicked() {
            self.zoom = (if self.zoom == 0.0 { 1.0 } else { self.zoom } * 1.25).min(32.0);
        }
        if ui.button(ph::MAGNIFYING_GLASS_MINUS).clicked() {
            self.zoom = (if self.zoom == 0.0 { 1.0 } else { self.zoom } / 1.25).max(0.05);
        }
        ui.separator();
        if ui
            .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
            .tip(s.t("ai-shared-button-tip"))
            .clicked()
        {
            self.assistant.toggle("graphics");
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        self.refresh(ui.ctx());
        if let Some(e) = &self.error {
            ui.colored_label(theme.error, e);
        }
        let full = ui.available_rect_before_wrap();
        let canvas_w = if self.assistant.open {
            full.width() - 390.0
        } else {
            full.width()
        };
        let canvas =
            egui::Rect::from_min_size(full.min, egui::vec2(canvas_w.max(100.0), full.height()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(canvas));
        child.set_clip_rect(canvas);
        match self.texture.clone() {
            Some(tex) => {
                let size = tex.size_vec2();
                let fit = (canvas.width() / size.x)
                    .min(canvas.height() / size.y)
                    .min(1.0);
                let z = if self.zoom == 0.0 { fit } else { self.zoom };
                let shown = size * z;
                egui::ScrollArea::both()
                    .id_salt("gfx-canvas")
                    .auto_shrink([false, false])
                    .show(&mut child, |ui| {
                        // Checkerboard behind transparent pictures.
                        let (rect, _) =
                            ui.allocate_exact_size(shown.max(canvas.size()), egui::Sense::hover());
                        let img_rect = egui::Rect::from_center_size(
                            rect.center().max(canvas.min + shown / 2.0),
                            shown,
                        );
                        let p = ui.painter();
                        let cell = 12.0;
                        let mut y = img_rect.top();
                        let mut row = 0;
                        while y < img_rect.bottom() {
                            let mut x = img_rect.left();
                            let mut col = row % 2;
                            while x < img_rect.right() {
                                let c = if col % 2 == 0 {
                                    egui::Color32::from_gray(if theme.dark { 60 } else { 230 })
                                } else {
                                    egui::Color32::from_gray(if theme.dark { 45 } else { 200 })
                                };
                                let r = egui::Rect::from_min_size(
                                    egui::pos2(x, y),
                                    egui::vec2(cell, cell),
                                )
                                .intersect(img_rect);
                                p.rect_filled(r, 0.0, c);
                                x += cell;
                                col += 1;
                            }
                            y += cell;
                            row += 1;
                        }
                        p.image(
                            tex.id(),
                            img_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                        ui.painter().text(
                            img_rect.left_bottom() + egui::vec2(4.0, -4.0),
                            egui::Align2::LEFT_BOTTOM,
                            format!("{:.0}%", z * 100.0),
                            egui::FontId::proportional(11.0),
                            theme.text_muted,
                        );
                        if self.crop_select {
                            self.crop_overlay(ui, &theme, img_rect, z);
                        }
                    });
            }
            None => {
                child.centered_and_justified(|ui| {
                    ui.colored_label(theme.text_muted, s.t("gfx-empty"));
                });
            }
        }
        if self.assistant.open {
            let context = format!(
                "Picture: {}\nOperations applied:\n{}",
                self.info_line(),
                if self.ops.is_empty() {
                    "(none)".to_string()
                } else {
                    self.ops
                        .iter()
                        .map(|o| format!("- {}", o.label()))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            );
            self.assistant.set_context("graphics state", context);
            if self.assistant.quick_actions.is_empty() {
                use hs_ui::ai_assistant::QuickAction;
                self.assistant.quick_actions = vec![
                    QuickAction::new(s, "ai-qa-image-explain", "ai-qa-image-explain-prompt", ""),
                    QuickAction::new(s, "ai-qa-image-edits", "ai-qa-image-edits-prompt", ""),
                    QuickAction::new(s, "ai-qa-image-export", "ai-qa-image-export-prompt", ""),
                ];
            }
            let panel = egui::Rect::from_min_max(
                egui::pos2(full.right() - 390.0, full.top()),
                full.right_bottom(),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(panel)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(panel.intersect(ui.clip_rect()));
            egui::Frame::new()
                .fill(theme.panel_bg)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(&mut child, |ui| {
                    ui.set_min_width(370.0);
                    self.assistant.can_insert = false;
                    let _ = self.assistant.ui(ui, s, &theme, "graphics");
                });
        }
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![
            CommandDescriptor {
                id: "graphics.open".into(),
                title: "Graphics: Open Image…".into(),
                shortcut_action: Some("file.open".into()),
            },
            CommandDescriptor {
                id: "graphics.save_as".into(),
                title: "Graphics: Save As…".into(),
                shortcut_action: Some("file.save_as".into()),
            },
            CommandDescriptor {
                id: "graphics.ai".into(),
                title: "AI: Toggle Assistant".into(),
                shortcut_action: None,
            },
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "graphics.open" | "file.open" => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Images", IMAGE_EXTS)
                    .pick_file()
                {
                    open_image(cx, &p);
                }
                true
            }
            "graphics.save_as" | "file.save_as" => {
                self.save_as(cx);
                true
            }
            "graphics.ai" => {
                self.assistant.toggle("graphics");
                true
            }
            _ => false,
        }
    }

    fn current_uri(&self) -> Option<String> {
        self.path.as_ref().map(|p| p.to_string_lossy().to_string())
    }

    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        let p = Path::new(uri.strip_prefix("file://").unwrap_or(uri));
        if !is_image_path(p) {
            return None;
        }
        // Only a tab holding no image will take one: every image gets its
        // own tab, and an image that is already open is found by
        // `current_uri` before this is consulted. Claiming any unedited
        // tab, as this used to, meant opening a second picture quietly
        // replaced the first.
        self.path.is_none().then_some(50)
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::File { uri, .. } => {
                let p = PathBuf::from(uri.strip_prefix("file://").unwrap_or(&uri));
                if !is_image_path(&p) {
                    return false;
                }
                match self.load(&p) {
                    Ok(()) => true,
                    Err(e) => {
                        cx.toast(ToastLevel::Error, e);
                        false
                    }
                }
            }
            _ => false,
        }
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::json!({
            "path": self.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            "ops": self.ops,
        })
    }

    fn speakable(&self) -> Vec<Speakable> {
        self.assistant
            .last_answer()
            .map(|a| vec![Speakable::new("Assistant's last answer", a)])
            .unwrap_or_default()
    }

    fn printable(&self) -> Option<Printable> {
        let img = self.current.as_ref()?;
        let png = ops::encode(img, Format::Png, 100).ok()?;
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        Some(Printable {
            title: self.title().to_string(),
            subtitle: self.info_line(),
            html: format!("<img alt=\"\" src=\"data:image/png;base64,{b64}\">"),
        })
    }
}
