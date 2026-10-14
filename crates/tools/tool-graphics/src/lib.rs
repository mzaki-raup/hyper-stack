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

pub struct GraphicsTool {
    path: Option<PathBuf>,
    original: Option<DynamicImage>,
    original_bytes: Vec<u8>,
    ops: Vec<Op>,
    current: Option<DynamicImage>,
    texture: Option<egui::TextureHandle>,
    dirty: bool,
    zoom: f32,
    format: Format,
    quality: u8,
    controls: Controls,
    frames: Vec<(u32, u32, DynamicImage)>,
    error: Option<String>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
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
            zoom: 0.0,
            format: Format::Png,
            quality: 90,
            controls: Controls {
                keep_aspect: true,
                scale: 2.0,
                extend_colour: [255, 255, 255, 255],
                blur: 1.5,
                sharpen: 1.0,
                ico_sizes: [true, false, true, true, false, false, true],
                ..Default::default()
            },
            frames: Vec::new(),
            error: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
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

    fn refresh(&mut self, ctx: &egui::Context) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let Some(orig) = &self.original else {
            return;
        };
        let img = ops::apply_all(orig, &self.ops);
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let colour =
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
        self.texture = Some(ctx.load_texture(
            format!("hs-graphics-{}", hs_core::new_id()),
            colour,
            egui::TextureOptions::LINEAR,
        ));
        if let Some((w, h)) = Some(img.dimensions()) {
            self.controls.width = w;
            self.controls.height = h;
        }
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
        // Transform
        form::section(ui, theme, &s.t("gfx-transform"), |ui| {
            ui.add_enabled_ui(has, |ui| {
                ui.label(egui::RichText::new(s.t("gfx-resize")).strong());
                ui.horizontal(|ui| {
                    let before = (self.controls.width, self.controls.height);
                    let (ow, oh) = self
                        .current
                        .as_ref()
                        .map(|i| i.dimensions())
                        .unwrap_or((1, 1));
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
                    ui.checkbox(&mut self.controls.keep_aspect, s.t("gfx-keep-aspect"));
                    if ui.button(s.t("gfx-apply")).clicked() {
                        self.push(Op::Resize {
                            width: self.controls.width,
                            height: self.controls.height,
                        });
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(s.t("gfx-scale"));
                    ui.add(
                        egui::DragValue::new(&mut self.controls.scale)
                            .range(0.05..=16.0)
                            .speed(0.05)
                            .suffix("×"),
                    );
                    if ui.button(s.t("gfx-apply")).clicked() {
                        self.push(Op::Scale {
                            factor: self.controls.scale,
                        });
                    }
                });
                ui.add_space(4.0);
                ui.label(egui::RichText::new(s.t("gfx-extend")).strong());
                ui.horizontal(|ui| {
                    for (i, l) in ["L", "T", "R", "B"].iter().enumerate() {
                        ui.add(
                            egui::DragValue::new(&mut self.controls.extend[i])
                                .range(0..=4000)
                                .prefix(format!("{l} ")),
                        );
                    }
                });
                ui.horizontal(|ui| {
                    let mut c = egui::Color32::from_rgba_unmultiplied(
                        self.controls.extend_colour[0],
                        self.controls.extend_colour[1],
                        self.controls.extend_colour[2],
                        self.controls.extend_colour[3],
                    );
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        self.controls.extend_colour = c.to_srgba_unmultiplied();
                    }
                    ui.label(s.t("gfx-extend-colour"));
                    if ui.button(s.t("gfx-apply")).clicked() {
                        let [l, t, r, b] = self.controls.extend;
                        self.push(Op::Extend {
                            left: l,
                            top: t,
                            right: r,
                            bottom: b,
                            colour: self.controls.extend_colour,
                        });
                    }
                });
                ui.add_space(4.0);
                ui.label(egui::RichText::new(s.t("gfx-crop")).strong());
                ui.horizontal(|ui| {
                    for (i, l) in ["X", "Y", "W", "H"].iter().enumerate() {
                        ui.add(
                            egui::DragValue::new(&mut self.controls.crop[i])
                                .range(0..=20000)
                                .prefix(format!("{l} ")),
                        );
                    }
                    if ui.button(s.t("gfx-apply")).clicked() {
                        let [x, y, w, h] = self.controls.crop;
                        self.push(Op::Crop {
                            x,
                            y,
                            width: w.max(1),
                            height: h.max(1),
                        });
                    }
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for (icon, tip, op) in [
                        (ph::ARROW_CLOCKWISE, "gfx-rotate-cw", Op::Rotate90),
                        (ph::ARROW_COUNTER_CLOCKWISE, "gfx-rotate-ccw", Op::Rotate270),
                        (ph::ARROWS_CLOCKWISE, "gfx-rotate-180", Op::Rotate180),
                        (ph::ARROWS_HORIZONTAL, "gfx-flip-h", Op::FlipH),
                        (ph::ARROWS_VERTICAL, "gfx-flip-v", Op::FlipV),
                    ] {
                        if ui.button(icon).on_hover_text(s.t(tip)).clicked() {
                            self.push(op);
                        }
                    }
                });
            });
        });
        // Adjust
        form::section(ui, theme, &s.t("gfx-adjust"), |ui| {
            ui.add_enabled_ui(has, |ui| {
                ui.spacing_mut().slider_width = (ui.available_width() - 190.0).max(80.0);
                let mut row =
                    |ui: &mut egui::Ui, label: &str, slider: egui::Slider<'_>, op: Op| -> bool {
                        let mut apply = false;
                        ui.horizontal(|ui| {
                            ui.label(label);
                            ui.add(slider);
                            if ui.button(s.t("gfx-apply")).clicked() {
                                apply = true;
                            }
                        });
                        if apply {
                            self.ops.push(op);
                            self.dirty = true;
                        }
                        apply
                    };
                let b = self.controls.brightness;
                row(
                    ui,
                    &s.t("gfx-brightness"),
                    egui::Slider::new(&mut self.controls.brightness, -100..=100),
                    Op::Brightness { value: b },
                );
                let c = self.controls.contrast;
                row(
                    ui,
                    &s.t("gfx-contrast"),
                    egui::Slider::new(&mut self.controls.contrast, -100.0..=100.0),
                    Op::Contrast { value: c },
                );
                let h = self.controls.hue;
                row(
                    ui,
                    &s.t("gfx-hue"),
                    egui::Slider::new(&mut self.controls.hue, 0..=360).suffix("°"),
                    Op::Hue { degrees: h },
                );
                let sat = self.controls.saturation;
                row(
                    ui,
                    &s.t("gfx-saturation"),
                    egui::Slider::new(&mut self.controls.saturation, -100.0..=100.0),
                    Op::Saturation { value: sat },
                );
                let bl = self.controls.blur;
                row(
                    ui,
                    &s.t("gfx-blur"),
                    egui::Slider::new(&mut self.controls.blur, 0.1..=20.0),
                    Op::Blur { sigma: bl },
                );
                let sh = self.controls.sharpen;
                row(
                    ui,
                    &s.t("gfx-sharpen"),
                    egui::Slider::new(&mut self.controls.sharpen, 0.1..=10.0),
                    Op::Sharpen {
                        sigma: sh,
                        threshold: 2,
                    },
                );
                ui.horizontal(|ui| {
                    if ui.button(s.t("gfx-grayscale")).clicked() {
                        self.push(Op::Grayscale);
                    }
                    if ui.button(s.t("gfx-invert")).clicked() {
                        self.push(Op::Invert);
                    }
                });
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
                    ui.label(format!("{}. {}", i + 1, op.label()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(hs_ui::icons::TRASH).clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                self.ops.remove(i);
                self.dirty = true;
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.ops.is_empty(), egui::Button::new(s.t("gfx-undo")))
                    .clicked()
                {
                    self.ops.pop();
                    self.dirty = true;
                }
                if ui
                    .add_enabled(!self.ops.is_empty(), egui::Button::new(s.t("gfx-reset")))
                    .clicked()
                {
                    self.ops.clear();
                    self.dirty = true;
                }
            });
        });
        // Icons
        form::section(ui, theme, &s.t("gfx-icons"), |ui| {
            ui.add_enabled_ui(has, |ui| {
                if self.frames.len() > 1 {
                    ui.label(
                        s.i18n
                            .t_args("gfx-frames", &[("count", &self.frames.len().to_string())]),
                    );
                    let mut pick = None;
                    ui.horizontal_wrapped(|ui| {
                        for (i, (w, h, _)) in self.frames.iter().enumerate() {
                            if ui.button(format!("{w}×{h}")).clicked() {
                                pick = Some(i);
                            }
                        }
                    });
                    if let Some(i) = pick {
                        let img = self.frames[i].2.clone();
                        self.original = Some(img);
                        self.ops.clear();
                        self.dirty = true;
                    }
                    ui.add_space(4.0);
                }
                ui.label(egui::RichText::new(s.t("gfx-make-ico")).strong());
                ui.horizontal_wrapped(|ui| {
                    for (i, size) in ICO_SIZES.iter().enumerate() {
                        ui.checkbox(&mut self.controls.ico_sizes[i], size.to_string());
                    }
                });
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
                ui.horizontal(|ui| {
                    ui.label(s.t("gfx-format"));
                    egui::ComboBox::from_id_salt("gfx-format")
                        .selected_text(self.format.label())
                        .show_ui(ui, |ui| {
                            for f in Format::ALL {
                                ui.selectable_value(&mut self.format, f, f.label());
                            }
                        });
                });
                if self.format == Format::Jpeg {
                    ui.add(egui::Slider::new(&mut self.quality, 1..=100).text(s.t("gfx-quality")));
                }
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
        !self.ops.is_empty()
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
                && let Err(e) = self.load(&p)
            {
                cx.toast(ToastLevel::Error, e);
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
            .button(format!("{} AI", ph::SPARKLE))
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
        match &self.texture {
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
                    && let Err(e) = self.load(&p)
                {
                    cx.toast(ToastLevel::Error, e);
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
        is_image_path(p).then_some(if self.ops.is_empty() { 50 } else { 150 })
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
