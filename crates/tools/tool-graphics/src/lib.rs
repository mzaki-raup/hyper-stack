//! HyperStack graphics tool: open a picture (from the editor's project tree
//! or a file dialog), view it, stack operations on it — resize, scale,
//! extend the canvas, rotate, flip, crop, brightness, contrast, hue,
//! saturation, grayscale, invert, blur, sharpen — convert between formats,
//! extract the frames of an icon or build a multi-size ICO, and save. The
//! operations are re-applied to the original, so any step can be removed.
//! The review-only AI assistant sees the picture's metadata and the
//! operation list, never the pixels.

pub mod activity;
pub mod diagram;
pub mod generator;
pub mod ops;

use std::borrow::Cow;
use std::path::{Path, PathBuf};

pub use activity::{Function, GraphicsHub, Kind, SharedGraphics};
use hs_core::Services;
use hs_core::tool::{
    Accept, Action, CloseDecision, CommandDescriptor, OpenRequest, Printable, Speakable,
    ToastLevel, Tool, ToolCategory, ToolContext, ToolDescriptor, ToolFactory,
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

pub struct GraphicsFactory {
    pub hub: SharedGraphics,
}

impl ToolFactory for GraphicsFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let mut t = GraphicsTool {
            hub: self.hub.clone(),
            drafts: Some(services.paths.root().join("graphics").join("drafts")),
            ..Default::default()
        };
        if let Some(kind) = restore
            .as_ref()
            .and_then(|v| v.get("kind"))
            .and_then(|v| v.as_str())
            .and_then(Kind::parse)
        {
            t.kind = kind;
        }
        if let Some(p) = restore
            .as_ref()
            .and_then(|v| v.get("path"))
            .and_then(|v| v.as_str())
        {
            let path = Path::new(p);
            // An older saved tab carries no kind; the extension decides
            // whether it was an icon or a picture.
            if restore
                .as_ref()
                .and_then(|v| v.get("kind"))
                .and_then(|v| v.as_str())
                .is_none()
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("ico"))
            {
                t.kind = Kind::Icon;
            }
            let _ = t.load(path);
        }
        if t.kind == Kind::Diagram && t.diagram.is_none() {
            if let Some(doc) = restore
                .as_ref()
                .and_then(|v| v.get("doc"))
                .and_then(|v| serde_json::from_value::<hs_diagram::DiagramDoc>(v.clone()).ok())
            {
                t.diagram = Some(Box::new(diagram::DiagramState::from_doc(doc)));
            } else {
                t.diagram = Some(Box::new(diagram::DiagramState::blank()));
            }
        }
        if t.kind == Kind::Generator {
            let mut g = generator::GeneratorState::new();
            if let Some(v) = restore.as_ref() {
                if let Some(prompt) = v.get("prompt").and_then(|p| p.as_str()) {
                    g.prompt = prompt.to_string();
                }
                if let Some(kind) = v.get("diagram_kind").and_then(|k| k.as_str()) {
                    g.kind = if kind == "architecture" {
                        hs_diagram::DiagramKind::Architecture
                    } else {
                        hs_diagram::DiagramKind::Flow
                    };
                }
                if let Some(doc) = v
                    .get("doc")
                    .and_then(|d| serde_json::from_value::<hs_diagram::DiagramDoc>(d.clone()).ok())
                {
                    g.doc = Some(doc);
                }
            }
            t.generator = Some(Box::new(g));
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
    transparent_colour: [u8; 4],
    transparent_tolerance: u8,
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
    hub: SharedGraphics,
    kind: Kind,
    path: Option<PathBuf>,
    original: Option<DynamicImage>,
    original_bytes: Vec<u8>,
    ops: Vec<Op>,
    /// Snapshots before each edit, capped by Settings → General → Behaviour.
    undo_stack: Vec<Vec<Op>>,
    redo_stack: Vec<Vec<Op>>,
    undo_limit: usize,
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
    /// The next click on the picture picks the colour to make transparent.
    pick_transparent: bool,
    /// The diagram editor, when this tab is a `.hdiag` document.
    diagram: Option<Box<diagram::DiagramState>>,
    /// The diagram generator, when this tab is a prompt/preview surface.
    generator: Option<Box<generator::GeneratorState>>,
    /// Where a diagram tab writes its close-time draft.
    drafts: Option<PathBuf>,
    /// The stable draft file this tab has been autosaving into.
    draft_path: Option<PathBuf>,
    draft_at: Option<std::time::Instant>,
    draft_hash: u64,
    /// An active drag: the grip, the rectangle when it started and the
    /// press position in image pixels.
    crop_drag: Option<(CropGrip, [u32; 4], egui::Pos2)>,
    /// Bumped whenever the shown picture is re-rendered, so the assistant's
    /// attachment (a PNG of it) is encoded once per change, not per frame.
    render_version: u64,
    attach_cache: Option<(u64, hs_core::ai::ImagePart)>,
    /// The Generate Picture form in the assistant column.
    generate: GenState,
    /// The New Image dialog, while it is open.
    new_image: Option<NewImage>,
}

/// Where a picture generation lands: the bytes, or why it failed.
type GenSlot = std::sync::Arc<parking_lot::Mutex<Option<Result<Vec<u8>, String>>>>;

/// The Generate Picture form: what to ask for and the request in flight.
struct GenState {
    prompt: String,
    /// The image model name the provider takes, one per job (1min.AI: SDXL,
    /// Flux or GPT Image for generation; Dzine, Clipdrop, Ideogram… for an
    /// edit; Stable Image, Dzine, Recraft… for an enhancement). Empty means
    /// the job's default.
    models: [String; 3],
    /// The provider `models` were typed for.
    models_for: String,
    provider: String,
    job: hs_core::ai::ImageJob,
    /// How far an edit may depart from the picture (0-1).
    strength: f32,
    running: Option<GenSlot>,
}

impl Default for GenState {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            models: Default::default(),
            models_for: String::new(),
            provider: String::new(),
            job: hs_core::ai::ImageJob::Generate,
            strength: 0.5,
            running: None,
        }
    }
}

impl GenState {
    fn job_index(job: hs_core::ai::ImageJob) -> usize {
        match job {
            hs_core::ai::ImageJob::Generate => 0,
            hs_core::ai::ImageJob::Edit => 1,
            hs_core::ai::ImageJob::Enhance => 2,
        }
    }
    /// The model for the selected job: what was typed, else (for 1min.AI)
    /// the job's default; a self-hosted server's empty model means the
    /// provider's own checkpoint.
    fn model(&self, hosted: bool) -> String {
        let typed = self.models[Self::job_index(self.job)].trim();
        if typed.is_empty() && hosted {
            self.job.default_model().to_string()
        } else {
            typed.to_string()
        }
    }
}

/// An image-capable provider as the Generate Picture form lists it.
#[derive(Clone)]
struct ImageProvider {
    id: String,
    name: String,
    kind: hs_core::ai::Provider,
    checkpoint: String,
}

/// The New Image dialog's fields.
struct NewImage {
    width: u32,
    height: u32,
    background: [u8; 4],
    transparent: bool,
}

impl Default for NewImage {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            background: [255, 255, 255, 255],
            transparent: false,
        }
    }
}

impl Default for GraphicsTool {
    fn default() -> Self {
        Self {
            hub: GraphicsHub::memory(),
            kind: Kind::Image,
            path: None,
            original: None,
            original_bytes: Vec::new(),
            ops: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            undo_limit: 20,
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
                transparent_colour: [255, 255, 255, 255],
                transparent_tolerance: 24,
                ..Default::default()
            },
            frames: Vec::new(),
            error: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
            crop_select: false,
            pick_transparent: false,
            diagram: None,
            generator: None,
            drafts: None,
            draft_path: None,
            draft_at: None,
            draft_hash: 0,
            crop_drag: None,
            render_version: 0,
            attach_cache: None,
            generate: GenState::default(),
            new_image: None,
        }
    }
}

const ICO_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// Shared instruction for the Graphics assistant quick actions: ask for a
/// complete document we can Replace or Append, with a concrete example.
const DIAGRAM_JSON_HINT: &str = r##"

When the answer changes the diagram, include the complete .hdiag document as a
```json fenced block so I can Replace or Append it. Example:
{"schema":2,"kind":"flow","title":"Checkout","subtitle":"","page":{"size":"fit","margin":24,"background":"#ffffff"},"nodes":[{"id":"start","label":"Start","kind":"start_end","x":0,"y":0,"w":0,"h":0}],"edges":[],"groups":[],"auto_layout":true}
Keep ids unique and endpoint ids valid.
"##;
const DIAGRAM_QA: [(&str, &str); 4] = [
    ("ai-qa-diagram-layout", "ai-qa-diagram-layout-prompt"),
    ("ai-qa-diagram-missing", "ai-qa-diagram-missing-prompt"),
    ("ai-qa-diagram-name", "ai-qa-diagram-name-prompt"),
    ("ai-qa-diagram-a11y", "ai-qa-diagram-a11y-prompt"),
];

const GENERATOR_QA: [(&str, &str); 4] = [
    ("ai-qa-gen-refine", "ai-qa-gen-refine-prompt"),
    ("ai-qa-gen-critique", "ai-qa-gen-critique-prompt"),
    ("ai-qa-gen-layout", "ai-qa-gen-layout-prompt"),
    ("ai-qa-gen-explain", "ai-qa-gen-explain-prompt"),
];

/// A private request body for "open this as an icon" and "make a new icon".
const GRAPHICS_MIME: &str = "application/x-hyperstack-graphics";

/// `.hdiag` or the Release 2 alias `.diagram.json`.
fn is_diagram_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("hdiag"))
        || path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().ends_with(".diagram.json"))
}

impl GraphicsTool {
    fn load(&mut self, path: &Path) -> Result<(), String> {
        if is_diagram_path(path) {
            let state = diagram::DiagramState::open_path(path)?;
            self.kind = Kind::Diagram;
            self.diagram = Some(Box::new(state));
            self.path = Some(path.to_path_buf());
            return Ok(());
        }
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
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.current = None;
        self.texture = None;
        self.dirty = true;
        self.error = None;
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.kind.as_str().to_string());
        self.hub
            .lock()
            .touch(self.kind, &title, Some(path.to_string_lossy().to_string()));
        Ok(())
    }

    fn remember(&mut self) {
        self.undo_stack.push(self.ops.clone());
        let limit = self.undo_limit.clamp(1, 100);
        while self.undo_stack.len() > limit {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn undo_edit(&mut self) {
        if let Some(previous) = self.undo_stack.pop() {
            self.redo_stack
                .push(std::mem::replace(&mut self.ops, previous));
            self.dirty = true;
        }
    }

    fn redo_edit(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(std::mem::replace(&mut self.ops, next));
            self.dirty = true;
        }
    }

    fn push(&mut self, op: Op) {
        self.remember();
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
        self.remember();
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
        self.render_version += 1;
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

    /// The shown picture as a PNG for the assistant, at most 1024 px on a
    /// side (enough to describe, small enough to send), cached per render.
    fn attachment_part(&mut self) -> Option<hs_core::ai::ImagePart> {
        if let Some((v, part)) = &self.attach_cache
            && *v == self.render_version
        {
            return Some(part.clone());
        }
        let img = self.current.as_ref()?;
        let small = if img.width() > 1024 || img.height() > 1024 {
            img.thumbnail(1024, 1024)
        } else {
            img.clone()
        };
        let png = ops::encode(&small, Format::Png, 100).ok()?;
        let part = hs_core::ai::ImagePart::png(png);
        self.attach_cache = Some((self.render_version, part.clone()));
        Some(part)
    }

    /// Where a generated or new picture is kept: a draft under the Graphics
    /// folder, which the tab then opens like any file.
    fn image_draft_path(&self, stem: &str) -> Option<PathBuf> {
        let dir = self.drafts.as_ref()?.join("images");
        std::fs::create_dir_all(&dir).ok()?;
        let safe: String = stem
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .take(40)
            .collect();
        Some(dir.join(format!("{safe}-{}.png", &hs_core::new_id()[..8])))
    }

    /// The providers that can make a picture: 1min.AI and the self-hosted
    /// image servers (`Provider::generates_images`).
    fn image_providers(s: &Services) -> Vec<ImageProvider> {
        s.settings
            .read()
            .ai
            .image_providers()
            .into_iter()
            .map(|p| ImageProvider {
                id: p.id,
                name: p.name,
                kind: p.kind,
                checkpoint: p.image.checkpoint,
            })
            .collect()
    }

    /// The Generate Picture form at the top of the assistant column.
    fn generate_section(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        let providers = Self::image_providers(s);
        egui::CollapsingHeader::new(
            egui::RichText::new(format!(
                "{} {}",
                ph::MAGIC_WAND,
                s.t("gfx-ai-generate-title")
            ))
            .strong(),
        )
        .id_salt("gfx-generate")
        .default_open(false)
        .show(ui, |ui| {
            form::hint(ui, theme, &s.t("gfx-ai-generate-hint"));
            if providers.is_empty() {
                form::note(
                    ui,
                    theme,
                    hs_ui::icons::INFO,
                    &s.t("gfx-ai-generate-no-provider"),
                );
                return;
            }
            if !providers.iter().any(|p| p.id == self.generate.provider) {
                self.generate.provider = providers[0].id.clone();
            }
            let provider = providers
                .iter()
                .find(|p| p.id == self.generate.provider)
                .cloned()
                .unwrap_or_else(|| providers[0].clone());
            // The model fields belong to one provider: a 1min.AI model id
            // means nothing to a WebUI, whose "model" is a checkpoint.
            if self.generate.models_for != provider.id {
                self.generate.models_for = provider.id.clone();
                self.generate.models = Default::default();
            }
            use hs_core::ai::ImageJob;
            // Without a picture only a new one can be asked for.
            if self.current.is_none() {
                self.generate.job = ImageJob::Generate;
            }
            ui.add_enabled_ui(self.current.is_some(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (job, key) in [
                        (ImageJob::Generate, "gfx-ai-job-generate"),
                        (ImageJob::Edit, "gfx-ai-job-edit"),
                        (ImageJob::Enhance, "gfx-ai-job-enhance"),
                    ] {
                        ui.selectable_value(&mut self.generate.job, job, s.t(key));
                    }
                });
            });
            let job = self.generate.job;
            form::hint(
                ui,
                theme,
                &s.t(match job {
                    ImageJob::Generate => "gfx-ai-job-generate-hint",
                    ImageJob::Edit => "gfx-ai-job-edit-hint",
                    ImageJob::Enhance => "gfx-ai-job-enhance-hint",
                }),
            );
            let model_slot = GenState::job_index(job);
            let hosted = provider.kind == hs_core::ai::Provider::OneMin;
            if hosted && self.generate.models[model_slot].is_empty() {
                // SDXL 1.0 / Dzine / Stable Image (owner's choice, and the
                // documented 1min.AI models for each job). A self-hosted
                // server's empty field means its provider's checkpoint.
                self.generate.models[model_slot] = job.default_model().into();
            }
            egui::Grid::new("gfx-generate-grid")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(s.t("gfx-ai-generate-provider"));
                    egui::ComboBox::from_id_salt("gfx-generate-provider")
                        .width(ui.available_width())
                        .selected_text(provider.name.clone())
                        .show_ui(ui, |ui| {
                            for p in &providers {
                                ui.selectable_value(
                                    &mut self.generate.provider,
                                    p.id.clone(),
                                    &p.name,
                                );
                            }
                        });
                    ui.end_row();
                    // A self-hosted Enhance takes the provider's upscaler,
                    // not a model, so the row would only mislead.
                    if hosted || job != ImageJob::Enhance {
                        ui.label(s.t(if hosted {
                            "gfx-ai-generate-model"
                        } else {
                            "gfx-ai-checkpoint"
                        }));
                        let hint = if hosted {
                            String::new()
                        } else if provider.checkpoint.is_empty() {
                            s.t("gfx-ai-checkpoint-server")
                        } else {
                            provider.checkpoint.clone()
                        };
                        ui.add(
                            egui::TextEdit::singleline(&mut self.generate.models[model_slot])
                                .hint_text(hint)
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();
                    }
                    if job == ImageJob::Edit {
                        ui.label(s.t("gfx-ai-edit-strength"));
                        ui.add(
                            egui::Slider::new(&mut self.generate.strength, 0.0..=1.0)
                                .show_value(false)
                                .trailing_fill(true),
                        )
                        .on_hover_text(s.t("gfx-ai-edit-strength-hint"));
                        ui.end_row();
                    }
                });
            ui.add(
                egui::TextEdit::multiline(&mut self.generate.prompt)
                    .hint_text(s.t(match job {
                        ImageJob::Generate => "gfx-ai-generate-prompt-hint",
                        ImageJob::Edit => "gfx-ai-edit-prompt-hint",
                        ImageJob::Enhance => "gfx-ai-enhance-prompt-hint",
                    }))
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                let busy = self.generate.running.is_some();
                if busy {
                    ui.spinner();
                }
                // An enhancement needs no prompt; the other jobs do.
                let can =
                    !busy && (job == ImageJob::Enhance || !self.generate.prompt.trim().is_empty());
                let label = s.t(match job {
                    ImageJob::Generate => "gfx-ai-generate",
                    ImageJob::Edit => "gfx-ai-edit",
                    ImageJob::Enhance => "gfx-ai-enhance",
                });
                if ui.add_enabled(can, egui::Button::new(label)).clicked() {
                    self.start_generation(s);
                }
            });
        });
        ui.separator();
    }

    fn start_generation(&mut self, s: &Services) {
        let provider = {
            let settings = s.settings.read();
            settings
                .ai
                .providers
                .iter()
                .find(|p| p.id == self.generate.provider)
                .cloned()
        };
        let Some(provider) = provider else {
            return;
        };
        let key = match s.ai_credentials(&provider) {
            Ok(k) => k,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let cfg = provider.config("");
        let job = self.generate.job;
        let model = self
            .generate
            .model(provider.kind == hs_core::ai::Provider::OneMin);
        let prompt = self.generate.prompt.trim().to_string();
        let strength = self.generate.strength;
        let source = if job.needs_source() {
            self.attachment_part()
        } else {
            None
        };
        let size = self
            .current
            .as_ref()
            .map(|c| (c.width(), c.height()))
            .unwrap_or((1024, 1024));
        let slot: GenSlot = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        let waker = s.runtime.waker();
        s.runtime.spawn(async move {
            let http = hs_core::ai::http_client();
            let result = hs_core::ai::generate_image(
                &http,
                &cfg,
                &key,
                job,
                &model,
                &prompt,
                size,
                source.as_ref(),
                strength,
            )
            .await
            .map_err(|e| e.to_string());
            *out.lock() = Some(result);
            waker.request_repaint();
        });
        self.generate.running = Some(slot);
    }

    /// A finished generation becomes a draft file and the tab's picture.
    fn poll_generation(&mut self, cx: &mut ToolContext) {
        let done = self.generate.running.as_ref().and_then(|r| r.lock().take());
        let Some(result) = done else {
            return;
        };
        self.generate.running = None;
        match result {
            Ok(bytes) => {
                let stem = self
                    .generate
                    .prompt
                    .split_whitespace()
                    .take(4)
                    .collect::<Vec<_>>()
                    .join("-");
                let stem = if stem.is_empty() { "generated" } else { &stem };
                match self.image_draft_path(stem) {
                    Some(path) => match std::fs::write(&path, &bytes).map_err(|e| e.to_string()) {
                        Ok(()) => match self.load(&path) {
                            Ok(()) => {
                                cx.toast(ToastLevel::Success, cx.services.t("gfx-ai-generated"))
                            }
                            Err(e) => cx.toast(ToastLevel::Error, e),
                        },
                        Err(e) => cx.toast(ToastLevel::Error, e),
                    },
                    None => cx.toast(ToastLevel::Error, "no draft folder".to_string()),
                }
            }
            Err(e) => cx.toast(ToastLevel::Error, e),
        }
    }

    /// The New Image dialog: size and background, then a blank picture
    /// saved as a draft and opened in a new tab.
    fn new_image_dialog(&mut self, ctx: &egui::Context, cx: &mut ToolContext, theme: &Theme) {
        let Some(mut form_state) = self.new_image.take() else {
            return;
        };
        let s = cx.services;
        let mut done: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::new("gfx-new-image")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.painter().rect_filled(ui.max_rect(), 8.0, theme.panel_bg);
            ui.heading(s.t("gfx-new-image-title"));
            form::hint(ui, theme, &s.t("gfx-new-image-hint"));
            ui.add_space(6.0);
            egui::Grid::new("gfx-new-image-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label(s.t("gfx-new-image-width"));
                    ui.add(
                        egui::DragValue::new(&mut form_state.width)
                            .range(1..=8192)
                            .suffix(" px"),
                    );
                    ui.end_row();
                    ui.label(s.t("gfx-new-image-height"));
                    ui.add(
                        egui::DragValue::new(&mut form_state.height)
                            .range(1..=8192)
                            .suffix(" px"),
                    );
                    ui.end_row();
                    ui.label(s.t("gfx-new-image-background"));
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(!form_state.transparent, |ui| {
                            let mut c = egui::Color32::from_rgb(
                                form_state.background[0],
                                form_state.background[1],
                                form_state.background[2],
                            );
                            if ui.color_edit_button_srgba(&mut c).changed() {
                                form_state.background = [c.r(), c.g(), c.b(), 255];
                            }
                        });
                        ui.checkbox(
                            &mut form_state.transparent,
                            s.t("gfx-new-image-transparent"),
                        );
                    });
                    ui.end_row();
                });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::primary_button(ui, theme, &s.t("gfx-new-image-create")).clicked() {
                    done = Some(true);
                }
                if ui.button(s.t("cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                let bg = if form_state.transparent {
                    [0, 0, 0, 0]
                } else {
                    form_state.background
                };
                let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                    form_state.width.max(1),
                    form_state.height.max(1),
                    image::Rgba(bg),
                ));
                let written = ops::encode(&img, Format::Png, 100).and_then(|png| {
                    let path = self
                        .image_draft_path("new-image")
                        .ok_or_else(|| "no draft folder".to_string())?;
                    std::fs::write(&path, png).map_err(|e| e.to_string())?;
                    Ok(path)
                });
                match written {
                    Ok(path) => {
                        open_image(cx, &path);
                        cx.toast(ToastLevel::Success, s.t("gfx-new-image-created"));
                    }
                    Err(e) => cx.toast(ToastLevel::Error, e),
                }
            }
            Some(false) => {}
            None => self.new_image = Some(form_state),
        }
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
        let encoded = ops::encode(img, self.format, self.quality);
        match encoded.and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => {
                let title = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "image".into());
                self.path = Some(path.clone());
                self.hub
                    .lock()
                    .touch(self.kind, &title, Some(path.to_string_lossy().to_string()));
                cx.toast(
                    ToastLevel::Success,
                    s.i18n
                        .t_args("gfx-saved", &[("path", &path.display().to_string())]),
                );
            }
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
            Ok(()) => {
                let title = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "icon.ico".into());
                self.path = Some(path.clone());
                self.hub
                    .lock()
                    .touch(Kind::Icon, &title, Some(path.to_string_lossy().to_string()));
                cx.toast(
                    ToastLevel::Success,
                    cx.services
                        .i18n
                        .t_args("gfx-saved", &[("path", &path.display().to_string())]),
                );
            }
            Err(e) => cx.toast(ToastLevel::Error, e),
        }
    }

    /// Render the current picture and open it in a new Icon tab.
    fn convert_to_icon(&mut self, cx: &mut ToolContext) {
        let Some(img) = self.current.clone() else {
            return;
        };
        self.hub.lock().set_pending_icon(img);
        cx.open_new_tab(
            "graphics",
            Some(OpenRequest::Text {
                mime: GRAPHICS_MIME.into(),
                body: "{\"new\":\"icon\"}".into(),
            }),
        );
    }

    fn adopt_icon(&mut self, img: image::DynamicImage) {
        let (w, h) = img.dimensions();
        self.kind = Kind::Icon;
        self.path = None;
        self.original_bytes.clear();
        self.original = Some(img.clone());
        self.current = Some(img);
        self.texture = None;
        self.ops.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.frames.clear();
        self.controls.width = w;
        self.controls.height = h;
        self.controls.crop = [0, 0, w, h];
        self.format = Format::Png;
        self.dirty = true;
        self.error = None;
    }

    fn transparency_section(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        let has = self.original.is_some();
        // Transparency: pick a colour on the canvas and clear every pixel
        // close to it. An icon-only tool; a picture tab keeps its ops list
        // unchanged.
        if self.kind == Kind::Icon {
            form::section(ui, theme, &s.t("gfx-transparency"), |ui| {
                ui.add_enabled_ui(has, |ui| {
                    Rows::show(ui, theme, "gfx-rows-transparency", |rows| {
                        rows.row(&s.t("gfx-transparent-colour"), None, |ui| {
                            let mut c = egui::Color32::from_rgba_unmultiplied(
                                self.controls.transparent_colour[0],
                                self.controls.transparent_colour[1],
                                self.controls.transparent_colour[2],
                                self.controls.transparent_colour[3],
                            );
                            if ui.color_edit_button_srgba(&mut c).changed() {
                                self.controls.transparent_colour = c.to_srgba_unmultiplied();
                            }
                            ui.label(
                                egui::RichText::new(format!(
                                    "#{:02X}{:02X}{:02X}",
                                    self.controls.transparent_colour[0],
                                    self.controls.transparent_colour[1],
                                    self.controls.transparent_colour[2]
                                ))
                                .small()
                                .monospace()
                                .color(theme.text_muted),
                            );
                        });
                        rows.row(&s.t("gfx-transparent-tolerance"), None, |ui| {
                            ui.spacing_mut().slider_width = 120.0;
                            let v = ui.visuals_mut();
                            v.widgets.inactive.bg_fill = theme.border;
                            v.widgets.hovered.bg_fill = theme.surface_hover;
                            ui.add(
                                egui::Slider::new(
                                    &mut self.controls.transparent_tolerance,
                                    0..=100,
                                )
                                .suffix("%")
                                .trailing_fill(true),
                            );
                        });
                    });
                    if ui
                        .checkbox(&mut self.pick_transparent, s.t("gfx-pick-on-canvas"))
                        .changed()
                        && self.pick_transparent
                    {
                        self.crop_select = false;
                    }
                    if self.pick_transparent {
                        form::hint(ui, theme, &s.t("gfx-pick-hint"));
                    }
                    ui.add_space(4.0);
                    if form::primary_button(ui, theme, &s.t("gfx-transparent-apply")).clicked() {
                        self.push(Op::TransparentColor {
                            colour: self.controls.transparent_colour,
                            tolerance: self.controls.transparent_tolerance,
                        });
                    }
                    form::hint(ui, theme, &s.t("gfx-transparent-hint"));
                });
            });
        }
    }

    fn sidebar_sections(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        if self.kind == Kind::Icon {
            self.transparency_section(ui, cx, theme);
        }
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
                                .prefix(s.t("gfx-prefix-w")),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.controls.height)
                                .range(1..=20000)
                                .prefix(s.t("gfx-prefix-h")),
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
                        self.remember();
                        self.ops.pop();
                        self.dirty = true;
                    } else if on {
                        // Switched on earlier in the list: take that one out.
                        if let Some(i) = self.ops.iter().rposition(|o| *o == op) {
                            self.remember();
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
                self.remember();
                self.ops.remove(i);
                self.dirty = true;
            }
            if !self.ops.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.undo_stack.is_empty(),
                            egui::Button::new(s.t("gfx-undo")),
                        )
                        .clicked()
                    {
                        self.undo_edit();
                    }
                    if ui
                        .add_enabled(
                            !self.redo_stack.is_empty(),
                            egui::Button::new(s.t("gfx-redo")),
                        )
                        .clicked()
                    {
                        self.redo_edit();
                    }
                    if ui.button(s.t("gfx-reset")).clicked() {
                        self.remember();
                        self.ops.clear();
                        self.dirty = true;
                    }
                });
            }
        });
        // Icons: an icon tab edits frames and ICO sizes; a picture tab does
        // not carry icon controls (owner's request, 2026-10-19).
        if self.kind == Kind::Icon {
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
                            rows.hint(&s.i18n.t_args(
                                "gfx-frames",
                                &[("count", &self.frames.len().to_string())],
                            ));
                            if let Some(i) = pick {
                                let img = self.frames[i].2.clone();
                                self.original = Some(img);
                                self.ops.clear();
                                self.undo_stack.clear();
                                self.redo_stack.clear();
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
        }
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
    /// The eyedropper: one click on the picture becomes the colour to clear.
    fn pick_overlay(&mut self, ui: &mut egui::Ui, img_rect: egui::Rect, z: f32) {
        if ui.rect_contains_pointer(img_rect) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        // Read the click from the input rather than `ui.interact`: the
        // scroll area around the canvas owns the pointer for dragging, and
        // an interaction layered under it never sees the click.
        let (clicked, pos) = ui.input(|i| (i.pointer.primary_clicked(), i.pointer.interact_pos()));
        let Some(pos) = pos else { return };
        if !clicked || !img_rect.contains(pos) {
            return;
        }
        let Some(img) = &self.current else { return };
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 || z <= 0.0 {
            return;
        }
        let x = (((pos.x - img_rect.left()) / z).floor() as i64).clamp(0, w as i64 - 1) as u32;
        let y = (((pos.y - img_rect.top()) / z).floor() as i64).clamp(0, h as i64 - 1) as u32;
        self.controls.transparent_colour = img.to_rgba8().get_pixel(x, y).0;
        self.pick_transparent = false;
    }

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

/// Open a Recent row as the kind it was saved as: a picture goes through the
/// normal file route (which focuses an open tab), an icon keeps its intent
/// even when the file is a PNG.
fn open_recent(cx: &mut ToolContext, path: &str, kind: Kind) {
    if kind == Kind::Icon {
        let body = serde_json::json!({ "open_icon": path }).to_string();
        cx.open(
            "graphics",
            Some(OpenRequest::Text {
                mime: GRAPHICS_MIME.into(),
                body,
            }),
        );
    } else {
        open_image(cx, Path::new(path));
    }
}

fn diagram_context(doc: &hs_diagram::DiagramDoc) -> String {
    let mut out = format!(
        "Diagram: {}\nType: {:?}\nNodes ({}):\n",
        doc.title,
        doc.kind,
        doc.nodes.len()
    );
    for node in &doc.nodes {
        out.push_str(&format!("- {} [{:?}] {}\n", node.id, node.kind, node.label));
    }
    out.push_str(&format!("Edges ({}):\n", doc.edges.len()));
    for edge in &doc.edges {
        out.push_str(&format!(
            "- {} -> {}{}\n",
            edge.from,
            edge.to,
            if edge.label.is_empty() {
                String::new()
            } else {
                format!(" ({})", edge.label)
            }
        ));
    }
    out
}

fn generator_context(state: &generator::GeneratorState) -> String {
    let mut out = format!(
        "Generator type: {:?}\nPrompt: {}\n",
        state.kind, state.prompt
    );
    if let Some(doc) = &state.doc {
        out.push_str(&format!(
            "Preview: {} ({} nodes, {} edges)\n",
            doc.title,
            doc.nodes.len(),
            doc.edges.len()
        ));
    } else {
        out.push_str("Preview: none yet\n");
    }
    out
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

impl GraphicsTool {
    fn settings_panel(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services.clone();
        let key = if self.kind == Kind::Icon {
            "gfx-icon-settings"
        } else {
            "gfx-image-settings"
        };
        form::header_bar(ui, theme, |ui| {
            ui.label(egui::RichText::new(s.t(key)).strong().size(14.0));
        });
        egui::ScrollArea::vertical()
            .id_salt("gfx-settings")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.sidebar_sections(ui, cx, theme);
            });
    }

    /// The shared assistant control always ends at the toolbar's hard right.
    fn assistant_right(&mut self, ui: &mut egui::Ui, s: &Services) {
        hs_ui::toolbar::hard_right(ui, |ui| {
            self.assistant_button(ui, s);
        });
    }

    fn assistant_button(&mut self, ui: &mut egui::Ui, s: &Services) {
        if ui
            .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
            .tip(s.t("ai-shared-button-tip"))
            .clicked()
        {
            self.assistant.toggle("graphics");
        }
    }

    /// The review-only assistant over a diagram or generator tab. The context
    /// is text (ids, labels, counts), never pixels or project files.
    /// Apply a diagram document or Archify IR found in an assistant answer.
    fn apply_assistant_text(&mut self, text: &str) -> bool {
        let Some(start) = text.find('{') else {
            return false;
        };
        let Some(end) = text.rfind('}') else {
            return false;
        };
        if end <= start {
            return false;
        }
        let candidate = &text[start..=end];
        let parsed = hs_diagram::import::archify_json(candidate)
            .or_else(|_| hs_diagram::io::from_json(candidate));
        let Ok(doc) = parsed else {
            return false;
        };
        match self.kind {
            Kind::Diagram => {
                if let Some(state) = &mut self.diagram {
                    state.replace(doc);
                    state.status = Some("Updated from the assistant.".into());
                }
            }
            Kind::Generator => {
                if let Some(state) = &mut self.generator {
                    state.doc = Some(doc);
                    state.status = Some("Generated from the assistant.".into());
                    state.error = None;
                }
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// The assistant as a real right-hand column, not a floating card: the
    /// canvas keeps its own width and is never painted underneath.
    fn assistant_column(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut ToolContext,
        theme: &Theme,
        context: String,
        quick: &[(&str, &str)],
    ) {
        let s = cx.services.clone();
        self.assistant.set_context("diagram state", context);
        if self.assistant.quick_actions.is_empty() {
            use hs_ui::ai_assistant::QuickAction;
            self.assistant.quick_actions = quick
                .iter()
                .map(|(label, prompt)| {
                    let hint = if matches!(self.kind, Kind::Diagram | Kind::Generator) {
                        DIAGRAM_JSON_HINT
                    } else {
                        ""
                    };
                    QuickAction::new(&s, label, &format!("{}{hint}", s.t(prompt)), "")
                })
                .collect();
        }
        // Same rule as the shared assistant: keep 320 px for the tool,
        // shrink the column to make room, and step aside entirely when even
        // its minimum would not fit (a split half in a small window).
        let avail = ui.available_width();
        if avail < 320.0 + 260.0 {
            return;
        }
        let max_w = (avail - 320.0).clamp(260.0, 560.0);
        egui::Panel::right("gfx-assistant-column")
            .default_size(390.0_f32.min(max_w))
            .size_range(260.0..=max_w)
            .resizable(true)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if let Some(answer) = self.assistant.last_answer().map(str::to_owned) {
                        if ui.button(s.t("gfx-ai-copy")).clicked() {
                            ui.ctx().copy_text(answer.clone());
                        }
                        let label = if self.kind == Kind::Generator {
                            s.t("gfx-ai-generate-diagram")
                        } else {
                            s.t("gfx-ai-update-diagram")
                        };
                        if ui
                            .button(format!("{} {}", hs_ui::icons::ph::SPARKLE, label))
                            .clicked()
                        {
                            self.apply_assistant_text(&answer);
                        }
                    }
                });
                ui.separator();
                self.assistant.can_insert = true;
                self.assistant.apply_label = Some(s.t("gfx-ai-replace"));
                self.assistant.append_label = Some(s.t("gfx-ai-append"));
                let output = self.assistant.ui(ui, &s, theme, "graphics");
                if let Some(block) = output.use_block {
                    if output.run {
                        self.append_assistant_text(&block);
                    } else {
                        self.apply_assistant_text(&block);
                    }
                }
            });
    }

    /// Append the nodes, edges and groups from an assistant answer, with
    /// fresh ids so nothing collides with what is already on the canvas.
    fn append_assistant_text(&mut self, text: &str) -> bool {
        let Some(start) = text.find('{') else {
            return false;
        };
        let Some(end) = text.rfind('}') else {
            return false;
        };
        if end <= start {
            return false;
        }
        let candidate = &text[start..=end];
        let Ok(doc) = hs_diagram::import::archify_json(candidate)
            .or_else(|_| hs_diagram::io::from_json(candidate))
        else {
            return false;
        };
        let Kind::Diagram = self.kind else {
            return false;
        };
        let Some(state) = &mut self.diagram else {
            return false;
        };
        let stamp = state.doc.nodes.len() + state.doc.edges.len() + 1;
        let prefix = |id: &str| format!("ai{stamp}_{id}");
        let mut max_x = state
            .doc
            .nodes
            .iter()
            .map(|n| n.x + n.w)
            .fold(0.0_f32, f32::max);
        let mut groups = doc.groups.clone();
        for group in &mut groups {
            group.id = prefix(&group.id);
            group.nodes = group.nodes.iter().map(|id| prefix(id)).collect();
        }
        for (i, mut node) in doc.nodes.into_iter().enumerate() {
            node.id = prefix(&node.id);
            if node.x <= 0.0 && node.y <= 0.0 {
                node.x = max_x + 40.0;
                node.y = 40.0 + i as f32 * 90.0;
                node.w = node.w.max(160.0);
                node.h = node.h.max(60.0);
            }
            max_x = max_x.max(node.x + node.w);
            state.doc.nodes.push(node);
        }
        for mut edge in doc.edges {
            edge.id = prefix(&edge.id);
            edge.from = prefix(&edge.from);
            edge.to = prefix(&edge.to);
            state.doc.edges.push(edge);
        }
        state.doc.groups.extend(groups);
        state.doc.auto_layout = false;
        state.dirty = true;
        state.status = Some("Appended from the assistant.".into());
        self.dirty = true;
        true
    }

    // Kept as the reference overlay implementation for tools that cannot
    // give the assistant a real column; Graphics now uses `assistant_column`.
    #[allow(dead_code)]
    fn assistant_overlay(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut ToolContext,
        theme: &Theme,
        context: String,
        quick: &[(&str, &str)],
    ) {
        if !self.assistant.open {
            return;
        }
        let s = cx.services.clone();
        let full = ui.max_rect();
        self.assistant.set_context("diagram state", context);
        if self.assistant.quick_actions.is_empty() {
            use hs_ui::ai_assistant::QuickAction;
            self.assistant.quick_actions = quick
                .iter()
                .map(|(label, prompt)| {
                    let hint = if matches!(self.kind, Kind::Diagram | Kind::Generator) {
                        DIAGRAM_JSON_HINT
                    } else {
                        ""
                    };
                    QuickAction::new(&s, label, &format!("{}{hint}", s.t(prompt)), "")
                })
                .collect();
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
                self.assistant.can_insert = true;
                let output = self.assistant.ui(ui, &s, theme, "graphics");
                if let Some(block) = output.use_block {
                    self.apply_assistant_text(&block);
                }
                if let Some(answer) = self.assistant.last_answer().map(str::to_owned) {
                    let label = if self.kind == Kind::Generator {
                        s.t("gfx-ai-generate-diagram")
                    } else {
                        s.t("gfx-ai-update-diagram")
                    };
                    if ui
                        .button(format!("{} {}", hs_ui::icons::ph::SPARKLE, label))
                        .on_hover_text(s.t("gfx-ai-apply-tip"))
                        .clicked()
                    {
                        self.apply_assistant_text(&answer);
                    }
                }
            });
    }

    fn function_row(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, f: Function) {
        let s = cx.services.clone();
        let title = s.t(f.title_key());
        let desc = s.t(f.description_key());
        let label = format!("{}  {}", f.icon(), title);
        let enabled = f.enabled();
        let r = ui
            .add_enabled_ui(enabled, |ui| ui.selectable_label(false, label))
            .inner
            .on_hover_text(if enabled {
                desc.clone()
            } else {
                s.t("gfx-coming-soon")
            });
        let open = s.t(f.action_key());
        let coming = s.t("gfx-coming-soon");
        let new_image = s.t("gfx-new-image");
        if f == Function::Image {
            // Image has two ways in, so a left click offers both rather
            // than guessing (owner's request, 2026-10-22); the menu needs
            // its own id or it would share the context menu's popup.
            egui::Popup::menu(&r).id(r.id.with("left")).show(|ui| {
                if ui.button(format!("{} {}", ph::IMAGE, open)).clicked() {
                    ui.close();
                    self.pick_and_open(cx, f);
                }
                if ui
                    .button(format!("{} {}", ph::FILE_PLUS, new_image))
                    .clicked()
                {
                    ui.close();
                    self.new_image = Some(NewImage::default());
                }
            });
        } else if r.clicked() {
            if enabled {
                self.pick_and_open(cx, f);
            } else {
                cx.toast(ToastLevel::Info, s.t("gfx-coming-soon"));
            }
        }
        r.context_menu(|ui| {
            if enabled {
                if ui.button(&open).clicked() {
                    ui.close();
                    self.pick_and_open(cx, f);
                }
                if f == Function::Image && ui.button(&new_image).clicked() {
                    ui.close();
                    self.new_image = Some(NewImage::default());
                }
            } else {
                let _ = ui.add_enabled(false, egui::Button::new(&coming));
            }
        });
    }

    fn pick_and_open(&mut self, cx: &mut ToolContext, f: Function) {
        if f == Function::Diagram {
            let body = serde_json::json!({ "new": "diagram" }).to_string();
            cx.open_new_tab(
                "graphics",
                Some(OpenRequest::Text {
                    mime: GRAPHICS_MIME.into(),
                    body,
                }),
            );
            return;
        }
        if f == Function::Generator {
            let body = serde_json::json!({ "new": "generator" }).to_string();
            cx.open_new_tab(
                "graphics",
                Some(OpenRequest::Text {
                    mime: GRAPHICS_MIME.into(),
                    body,
                }),
            );
            return;
        }
        let icon = f == Function::Icon;
        let filter = if icon { "Icons" } else { "Images" };
        if let Some(p) = rfd::FileDialog::new()
            .add_filter(filter, IMAGE_EXTS)
            .pick_file()
        {
            if icon {
                // A file opened from the Icon function is an icon even when it
                // is a PNG; the new tab is told so explicitly.
                let body = serde_json::json!({
                    "open_icon": p.to_string_lossy().to_string()
                })
                .to_string();
                cx.open_new_tab(
                    "graphics",
                    Some(OpenRequest::Text {
                        mime: GRAPHICS_MIME.into(),
                        body,
                    }),
                );
            } else {
                open_image(cx, &p);
            }
        }
    }

    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let tabs = s.open_tabs.of("graphics");
        let recents = self.hub.lock().recents();
        form::page_title(
            ui,
            &theme,
            &s.t("tool-graphics"),
            Some(&s.t("gfx-activity-subtitle")),
        );
        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(ui, &theme, &tabs.len().to_string(), &s.t("gfx-stat-open"));
            hs_ui::dashboard::stat(
                ui,
                &theme,
                &recents.len().to_string(),
                &s.t("gfx-stat-recent"),
            );
        });
        egui::ScrollArea::vertical()
            .id_salt("gfx-home")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new(s.t("gfx-what-to-do"))
                        .strong()
                        .size(13.0)
                        .color(theme.text_muted),
                );
                ui.add_space(6.0);
                ui.horizontal_top(|ui| {
                    let actions = Function::ALL
                        .map(|f| (f.action_icon(), f.action_key(), f.description_key(), f));
                    // "New Image" sits beside Open Image: the same tile, its
                    // own action (the dialog rather than a file picker).
                    let mut new_image_clicked = false;
                    let tiles: Vec<(&str, &str, &str, Option<Function>)> = actions
                        .iter()
                        .flat_map(|(icon, label_key, desc_key, f)| {
                            let mut v = vec![(*icon, *label_key, *desc_key, Some(*f))];
                            if *f == Function::Image {
                                v.push((
                                    ph::FILE_PLUS,
                                    "gfx-new-image",
                                    "gfx-new-image-hint",
                                    None,
                                ));
                            }
                            v
                        })
                        .collect();
                    for (icon, label_key, desc_key, f) in tiles {
                        let enabled = f.map(|f| f.enabled()).unwrap_or(true);
                        let label = s.t(label_key);
                        let (rect, r) =
                            ui.allocate_exact_size(egui::vec2(180.0, 44.0), egui::Sense::click());
                        let colour = if enabled {
                            theme.text
                        } else {
                            theme.text_muted
                        };
                        let painter = ui.painter();
                        painter.rect(
                            rect,
                            6.0,
                            if r.hovered() && enabled {
                                theme.surface_hover
                            } else {
                                theme.surface
                            },
                            egui::Stroke::new(1.0, theme.border),
                            egui::StrokeKind::Inside,
                        );
                        let icon_galley = painter.layout_no_wrap(
                            icon.to_string(),
                            egui::FontId::proportional(18.0),
                            colour,
                        );
                        let text_galley =
                            painter.layout_no_wrap(label, egui::FontId::proportional(14.0), colour);
                        let gap = 8.0;
                        let total_w = icon_galley.size().x + gap + text_galley.size().x;
                        let start_x = rect.center().x - total_w / 2.0;
                        let centre_y = rect.center().y;
                        painter.galley(
                            egui::pos2(start_x, centre_y - icon_galley.size().y / 2.0),
                            icon_galley.clone(),
                            colour,
                        );
                        painter.galley(
                            egui::pos2(
                                start_x + icon_galley.size().x + gap,
                                centre_y - text_galley.size().y / 2.0,
                            ),
                            text_galley,
                            colour,
                        );
                        if enabled && r.on_hover_text(s.t(desc_key)).clicked() {
                            match f {
                                Some(f) => self.pick_and_open(cx, f),
                                None => new_image_clicked = true,
                            }
                        }
                        ui.add_space(8.0);
                    }
                    if new_image_clicked {
                        self.new_image = Some(NewImage::default());
                    }
                });
                ui.add_space(18.0);
                ui.label(
                    egui::RichText::new(s.t("gfx-functions"))
                        .strong()
                        .size(13.0)
                        .color(theme.text_muted),
                );
                ui.add_space(6.0);
                for f in Function::ALL {
                    ui.horizontal_top(|ui| {
                        ui.add_sized(
                            [24.0, 24.0],
                            egui::Label::new(egui::RichText::new(f.icon()).size(18.0))
                                .selectable(false),
                        );
                        ui.allocate_ui_with_layout(
                            egui::vec2(330.0, 58.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_width(330.0);
                                ui.set_max_width(330.0);
                                ui.label(egui::RichText::new(s.t(f.title_key())).strong());
                                ui.label(
                                    egui::RichText::new(s.t(f.description_key()))
                                        .small()
                                        .color(theme.text_muted),
                                );
                            },
                        );
                        ui.allocate_ui_with_layout(
                            egui::vec2(150.0, 58.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(150.0);
                                ui.set_max_width(150.0);
                                ui.add_space(if f == Function::Image { 2.0 } else { 12.0 });
                                // The same icon as the What To Do tile above
                                // (owner's request, 2026-10-22).
                                if ui
                                    .add_enabled(
                                        f.enabled(),
                                        egui::Button::new(format!(
                                            "{} {}",
                                            f.action_icon(),
                                            s.t(f.action_key())
                                        ))
                                        .min_size(egui::vec2(140.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    self.pick_and_open(cx, f);
                                }
                                if f == Function::Image
                                    && ui
                                        .add(
                                            egui::Button::new(format!(
                                                "{} {}",
                                                ph::FILE_PLUS,
                                                s.t("gfx-new-image")
                                            ))
                                            .min_size(egui::vec2(140.0, 24.0)),
                                        )
                                        .clicked()
                                {
                                    self.new_image = Some(NewImage::default());
                                }
                            },
                        );
                    });
                    ui.add_space(2.0);
                }
                ui.add_space(8.0);
                let labels = hs_ui::dashboard::DashboardLabels {
                    open_heading: &s.t("dash-open"),
                    nothing_open: &s.t("dash-nothing-open"),
                    no_preview: &s.t("dash-no-preview"),
                };
                if let Some(id) = hs_ui::dashboard::open_tabs(ui, &theme, &tabs, &labels) {
                    cx.actions.push(Action::FocusTab { tab_id: id });
                }
            });
    }

    /// Debounced (1 s) autosave of a dirty diagram into the same stable draft
    /// file the close path uses. The tab's `dirty` flag stays set: a draft is
    /// not a saved document.
    fn autosave_draft(&mut self) {
        let (Some(dir), Some(state)) = (self.drafts.clone(), self.diagram.as_ref()) else {
            return;
        };
        if !state.dirty || state.doc.nodes.is_empty() {
            return;
        }
        // The change check used to serialise the whole document to a JSON
        // string every frame just to hash it; the fingerprint streams the
        // same bytes into the hasher without building the string.
        let hash = diagram::fingerprint(&state.doc);
        if hash == self.draft_hash {
            return;
        }
        let now = std::time::Instant::now();
        if let Some(at) = self.draft_at {
            if now.duration_since(at).as_millis() < 1000 {
                return;
            }
        } else {
            self.draft_at = Some(now);
            return;
        }
        let title = if state.doc.title.trim().is_empty() {
            "Diagram".to_string()
        } else {
            state.doc.title.clone()
        };
        let doc = state.doc.clone();
        let path = self.draft_path.clone().unwrap_or_else(|| {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            dir.join(format!(
                "{}-draft-{stamp}.hdiag",
                crate::diagram::sanitise(&title)
            ))
        });
        if std::fs::create_dir_all(&dir).is_err() || hs_diagram::io::save(&path, &doc).is_err() {
            return;
        }
        let first = self.draft_path.is_none();
        self.draft_path = Some(path.clone());
        self.draft_hash = hash;
        self.draft_at = Some(now);
        if first {
            self.hub.lock().touch(
                Kind::Diagram,
                &format!("{title} (draft)"),
                Some(path.to_string_lossy().to_string()),
            );
        }
    }
}

impl Tool for GraphicsTool {
    fn wants_home_tab(&self) -> bool {
        true
    }

    /// The activity's home is a dashboard, not a blank picture tab.
    fn wants_dashboard_home(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        self.kind == Kind::Home
    }

    fn set_home_tab(&mut self, home: bool, _services: &Services) {
        if home {
            // A restored working tab keeps its document and loses the home
            // flag; the shell replaces it with a dashboard.
            if self.path.is_none()
                && self.original.is_none()
                && self.diagram.is_none()
                && self.generator.is_none()
            {
                self.kind = Kind::Home;
            }
        } else if self.kind == Kind::Home {
            self.kind = Kind::Image;
        }
    }

    fn can_duplicate_tab(&self) -> bool {
        false
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        if self.kind == Kind::Home {
            return Cow::Borrowed("Graphics");
        }
        if self.kind == Kind::Diagram {
            if let Some(path) = self.path.as_ref().and_then(|p| p.file_name()) {
                return Cow::Owned(path.to_string_lossy().to_string());
            }
            if let Some(state) = &self.diagram
                && !state.doc.title.is_empty()
            {
                return Cow::Owned(state.doc.title.clone());
            }
            return Cow::Borrowed("Diagram");
        }
        match self.path.as_ref().and_then(|p| p.file_name()) {
            Some(n) => Cow::Owned(n.to_string_lossy().to_string()),
            None => Cow::Borrowed(match self.kind {
                Kind::Icon => "Icon",
                Kind::Diagram => "Diagram",
                Kind::Generator => "Diagram Generator",
                _ => "Image",
            }),
        }
    }

    fn subtitle(&self) -> Option<String> {
        self.path
            .as_ref()
            .map(|p| p.display().to_string())
            .or_else(|| match self.kind {
                Kind::Icon => Some("Icon".into()),
                Kind::Diagram => Some("Diagram".into()),
                Kind::Generator => Some("Generator".into()),
                _ => None,
            })
    }

    fn dirty(&self) -> bool {
        if let Some(state) = &self.diagram {
            return state.dirty();
        }
        !self.ops.is_empty() || !self.pending_ops().is_empty()
    }

    /// Closing a diagram tab writes a draft under `graphics/drafts/` and
    /// lists it in Recent, so the work survives the tab (owner's request).
    fn on_close(&mut self) -> CloseDecision {
        let Some(state) = &self.diagram else {
            return if self.dirty() {
                CloseDecision::AskUnsavedChanges
            } else {
                CloseDecision::Close
            };
        };
        if (!state.dirty && state.path.is_some()) || state.doc.nodes.is_empty() {
            return CloseDecision::Close;
        }
        let Some(dir) = self.drafts.clone() else {
            return CloseDecision::AskUnsavedChanges;
        };
        let title = if state.doc.title.trim().is_empty() {
            "Diagram".to_string()
        } else {
            state.doc.title.clone()
        };
        let doc = state.doc.clone();
        let path = self.draft_path.clone().unwrap_or_else(|| {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            dir.join(format!(
                "{}-draft-{stamp}.hdiag",
                crate::diagram::sanitise(&title)
            ))
        });
        if std::fs::create_dir_all(&dir).is_err() || hs_diagram::io::save(&path, &doc).is_err() {
            return CloseDecision::AskUnsavedChanges;
        }
        self.draft_path = Some(path.clone());
        self.hub.lock().touch(
            Kind::Diagram,
            &format!("{title} (draft)"),
            Some(path.to_string_lossy().to_string()),
        );
        CloseDecision::Close
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        form::header_bar(ui, &theme, |ui| {
            ui.label(
                egui::RichText::new(s.t("tool-graphics"))
                    .strong()
                    .size(15.0),
            );
        });
        ui.add_space(2.0);
        for f in Function::ALL {
            self.function_row(ui, cx, f);
        }
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(s.t("gfx-recent"))
                .strong()
                .size(12.0)
                .color(theme.text_muted),
        );
        ui.add_space(2.0);
        // About half the remaining column: the function list keeps its
        // natural height and only Recent scrolls (owner's request).
        let h = (ui.available_height() * 0.55).clamp(80.0, 360.0);
        let hub = self.hub.clone();
        let recents = hub.lock().recents();
        egui::ScrollArea::vertical()
            .id_salt("gfx-recent")
            .max_height(h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if recents.is_empty() {
                    form::note(ui, &theme, hs_ui::icons::INFO, &s.t("gfx-recent-empty"));
                    return;
                }
                ui.indent("gfx-recent-indent", |ui| {
                    for r in recents {
                        let Some(path) = r.path.clone() else { continue };
                        let row = ui
                            .selectable_label(false, format!("{}  {}", r.kind.icon(), r.title))
                            .on_hover_text(path.clone());
                        if row.clicked() {
                            open_recent(cx, &path, r.kind);
                        }
                        let remove = s.t("gfx-remove-recent");
                        row.context_menu(|ui| {
                            if ui.button(&remove).clicked() {
                                hub.lock().remove_path(&path);
                                ui.close();
                            }
                        });
                    }
                });
            });
        true
    }

    /// The picture/icon controls, inside the tab rather than the activity
    /// sidebar, so the sidebar can keep the function list.
    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if self.kind == Kind::Home {
            return;
        }
        if self.kind == Kind::Diagram {
            if let Some(state) = &mut self.diagram {
                state.toolbar(ui, cx);
                if let Some(path) = state.take_saved() {
                    let title = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Diagram".into());
                    self.path = Some(path.clone());
                    self.hub.lock().touch(
                        Kind::Diagram,
                        &title,
                        Some(path.to_string_lossy().to_string()),
                    );
                }
            }
            self.assistant_right(ui, cx.services);
            return;
        }
        if self.kind == Kind::Generator {
            if let Some(state) = &mut self.generator {
                state.toolbar(ui, cx);
            }
            self.assistant_right(ui, cx.services);
            return;
        }
        let s = cx.services;
        let info = self.info_line();
        if !info.is_empty() {
            ui.label(egui::RichText::new(info).monospace().small());
            ui.separator();
        }
        if ui
            .add_enabled(
                !self.undo_stack.is_empty(),
                egui::Button::new(ph::ARROW_COUNTER_CLOCKWISE),
            )
            .on_hover_text(s.t("gfx-undo"))
            .clicked()
        {
            self.undo_edit();
        }
        if ui
            .add_enabled(
                !self.redo_stack.is_empty(),
                egui::Button::new(ph::ARROW_CLOCKWISE),
            )
            .on_hover_text(s.t("gfx-redo"))
            .clicked()
        {
            self.redo_edit();
        }
        ui.separator();
        if self.kind == Kind::Image
            && self.original.is_some()
            && ui
                .button(format!("{} {}", ph::APP_WINDOW, s.t("gfx-convert-to-icon")))
                .clicked()
        {
            self.convert_to_icon(cx);
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
        hs_ui::toolbar::hard_right(ui, |ui| {
            // The assistant is the same control in every tool, so it sits in
            // the same place: hard right of the toolbar.
            if ui
                .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
                .tip(s.t("ai-shared-button-tip"))
                .clicked()
            {
                self.assistant.toggle("graphics");
            }
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        self.undo_limit = s.settings.read().general.undo_limit as usize;
        self.poll_generation(cx);
        self.new_image_dialog(ui.ctx(), cx, &theme);
        if matches!(self.kind, Kind::Image | Kind::Icon) && !ui.ctx().egui_wants_keyboard_input() {
            let undo =
                ui.input(|i| i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::Z));
            let redo = ui.input(|i| {
                i.modifiers.ctrl
                    && (i.key_pressed(egui::Key::Y)
                        || (i.modifiers.shift && i.key_pressed(egui::Key::Z)))
            });
            if undo {
                self.undo_edit();
            } else if redo {
                self.redo_edit();
            }
        }
        self.refresh(ui.ctx());
        if self.kind == Kind::Home {
            self.dashboard(ui, cx);
            return;
        }
        if self.kind == Kind::Diagram {
            if self.assistant.open {
                // Formatted only while the column is open; it is one
                // `format!` per node and edge, every frame.
                let context = self
                    .diagram
                    .as_ref()
                    .map(|state| diagram_context(&state.doc))
                    .unwrap_or_default();
                self.assistant_column(ui, cx, &theme, context, &DIAGRAM_QA);
            }
            if let Some(state) = &mut self.diagram {
                state.ui(ui, cx, &theme);
                if let Some(path) = state.take_saved() {
                    let title = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Diagram".into());
                    self.path = Some(path.clone());
                    self.hub.lock().touch(
                        Kind::Diagram,
                        &title,
                        Some(path.to_string_lossy().to_string()),
                    );
                }
            }
            self.autosave_draft();
            return;
        }
        if self.kind == Kind::Generator {
            if self.assistant.open {
                let context = self
                    .generator
                    .as_ref()
                    .map(|state| generator_context(state))
                    .unwrap_or_default();
                self.assistant_column(ui, cx, &theme, context, &GENERATOR_QA);
            }
            if let Some(state) = &mut self.generator {
                state.ui(ui, cx, &theme);
            }
            return;
        }
        if let Some(e) = &self.error {
            ui.colored_label(theme.error, e);
        }
        // The picture keeps the tab, but its controls move into this panel so
        // the activity sidebar can list the four tools (owner's request).
        egui::Panel::left("gfx-settings")
            .default_size(300.0)
            .size_range(220.0..=460.0)
            .resizable(true)
            .show(ui, |ui| self.settings_panel(ui, cx, &theme));
        // The shared split: the assistant column shrinks to keep room for
        // the picture and steps aside in a narrow window, like every other
        // tool, instead of a fixed cut that read as a floating card.
        let (mut child, panel) = hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        let canvas = child.max_rect();
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
                        if self.pick_transparent {
                            self.pick_overlay(ui, img_rect, z);
                        }
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
        if let Some(panel) = panel {
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
                    QuickAction::new(&s, "ai-qa-image-explain", "ai-qa-image-explain-prompt", ""),
                    QuickAction::new(&s, "ai-qa-image-edits", "ai-qa-image-edits-prompt", ""),
                    QuickAction::new(&s, "ai-qa-image-export", "ai-qa-image-export-prompt", ""),
                    QuickAction::new(&s, "ai-qa-image-a11y", "ai-qa-image-a11y-prompt", ""),
                ];
            }
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(panel)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(panel.intersect(ui.clip_rect()));
            // The picture itself goes with the question: a PNG of what is
            // shown, downscaled, encoded once per change.
            self.assistant.attachment = self.attachment_part();
            egui::Frame::new()
                .fill(theme.panel_bg)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(&mut child, |ui| {
                    ui.set_min_width(260.0);
                    self.generate_section(ui, &s, &theme);
                    self.assistant.can_insert = false;
                    let _ = self.assistant.ui(ui, &s, &theme, "graphics");
                });
        } else {
            self.assistant.attachment = None;
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
                id: "graphics.open_icon".into(),
                title: "Graphics: Open Icon…".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "graphics.convert_to_icon".into(),
                title: "Graphics: Convert to Icon".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "graphics.save_as".into(),
                title: "Graphics: Save As…".into(),
                shortcut_action: Some("file.save_as".into()),
            },
            CommandDescriptor {
                id: "graphics.new_diagram".into(),
                title: "Graphics: New Diagram…".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "graphics.new_generation".into(),
                title: "Graphics: New Generation…".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "graphics.new_image".into(),
                title: "Graphics: New Image…".into(),
                shortcut_action: None,
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
                self.pick_and_open(cx, Function::Image);
                true
            }
            "graphics.open_icon" => {
                self.pick_and_open(cx, Function::Icon);
                true
            }
            "graphics.convert_to_icon" => {
                self.convert_to_icon(cx);
                true
            }
            "graphics.new_diagram" => {
                self.pick_and_open(cx, Function::Diagram);
                true
            }
            "graphics.new_generation" => {
                self.pick_and_open(cx, Function::Generator);
                true
            }
            "graphics.new_image" => {
                self.new_image = Some(NewImage::default());
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
        if self.kind == Kind::Home {
            return None;
        }
        self.path.as_ref().map(|p| p.to_string_lossy().to_string())
    }

    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        if self.kind == Kind::Home || self.path.is_some() {
            return None;
        }
        let p = Path::new(uri.strip_prefix("file://").unwrap_or(uri));
        if is_diagram_path(p) {
            // Only a *blank* diagram tab may be taken over: an untitled
            // diagram with nodes is someone's unsaved work (a Recent click
            // replaced one, 2026-10-22).
            let blank = self
                .diagram
                .as_ref()
                .is_some_and(|d| d.doc.nodes.is_empty() && !d.dirty());
            return (self.kind == Kind::Diagram && blank).then_some(60);
        }
        if !is_image_path(p) {
            return None;
        }
        let ico = p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("ico"));
        // One tab per document: a loaded tab is found by `current_uri`
        // before affinity is consulted. An empty Icon tab takes `.ico`
        // files; an empty Image tab takes the rest.
        match self.kind {
            Kind::Icon => ico.then_some(50),
            Kind::Image => (!ico).then_some(50),
            _ => None,
        }
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::File { uri, .. } => {
                let p = PathBuf::from(uri.strip_prefix("file://").unwrap_or(&uri));
                if is_diagram_path(&p) {
                    match self.load(&p) {
                        Ok(()) => return true,
                        Err(e) => {
                            cx.toast(ToastLevel::Error, e);
                            return false;
                        }
                    }
                }
                if !is_image_path(&p) {
                    return false;
                }
                let icon = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("ico"));
                if icon {
                    self.kind = Kind::Icon;
                } else if self.kind != Kind::Icon {
                    self.kind = Kind::Image;
                }
                match self.load(&p) {
                    Ok(()) => true,
                    Err(e) => {
                        cx.toast(ToastLevel::Error, e);
                        false
                    }
                }
            }
            OpenRequest::Text { mime, body } if mime == GRAPHICS_MIME => {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
                    return false;
                };
                if v.get("new").and_then(|n| n.as_str()) == Some("icon") {
                    let Some(img) = self.hub.lock().take_pending_icon() else {
                        return false;
                    };
                    self.adopt_icon(img);
                    return true;
                }
                if v.get("new").and_then(|n| n.as_str()) == Some("diagram") {
                    self.kind = Kind::Diagram;
                    self.path = None;
                    self.diagram = Some(Box::new(diagram::DiagramState::blank()));
                    return true;
                }
                if v.get("new").and_then(|n| n.as_str()) == Some("generator") {
                    self.kind = Kind::Generator;
                    self.path = None;
                    self.generator = Some(Box::new(generator::GeneratorState::new()));
                    return true;
                }
                if let Some(doc) = v
                    .get("open_diagram")
                    .and_then(|d| serde_json::from_value::<hs_diagram::DiagramDoc>(d.clone()).ok())
                {
                    self.kind = Kind::Diagram;
                    self.path = None;
                    self.diagram = Some(Box::new(diagram::DiagramState::from_doc(doc)));
                    return true;
                }
                if let Some(path) = v.get("open_icon").and_then(|p| p.as_str()) {
                    self.kind = Kind::Icon;
                    return match self.load(Path::new(path)) {
                        Ok(()) => true,
                        Err(e) => {
                            cx.toast(ToastLevel::Error, e);
                            false
                        }
                    };
                }
                false
            }
            _ => false,
        }
    }

    fn save_state(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "kind": self.kind.as_str(),
            "path": self.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            "ops": self.ops,
        });
        if let Some(state) = &self.diagram {
            value["doc"] = serde_json::to_value(&state.doc).unwrap_or(serde_json::Value::Null);
        }
        if let Some(state) = &self.generator {
            value["prompt"] = serde_json::Value::String(state.prompt.clone());
            value["diagram_kind"] = serde_json::Value::String(
                match state.kind {
                    hs_diagram::DiagramKind::Flow => "flow",
                    hs_diagram::DiagramKind::Architecture => "architecture",
                    hs_diagram::DiagramKind::Erd => "erd",
                    hs_diagram::DiagramKind::Sequence => "sequence",
                }
                .into(),
            );
            if let Some(doc) = &state.doc {
                value["doc"] = serde_json::to_value(doc).unwrap_or(serde_json::Value::Null);
            }
        }
        value
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

#[cfg(test)]
mod close_tests {
    use super::*;

    #[test]
    fn a_dirty_diagram_autosaves_after_the_debounce() {
        let dir = std::env::temp_dir().join(format!("hs-graphics-autosave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = GraphicsTool {
            kind: Kind::Diagram,
            drafts: Some(dir.clone()),
            ..Default::default()
        };
        t.diagram = Some(Box::new(diagram::DiagramState::from_doc(
            hs_diagram::template::flow(),
        )));
        t.diagram.as_mut().unwrap().dirty = true;
        t.draft_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(2));
        t.autosave_draft();
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names.len(), 1, "one autosaved draft: {names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_assistant_answer_can_replace_the_diagram() {
        let mut t = GraphicsTool {
            kind: Kind::Diagram,
            diagram: Some(Box::new(diagram::DiagramState::new(
                hs_diagram::DiagramKind::Flow,
            ))),
            ..GraphicsTool::default()
        };
        let doc = hs_diagram::template::flow();
        let answer = format!(
            "Here is the diagram:\n```json\n{}\n```",
            hs_diagram::io::to_json(&doc)
        );
        assert!(t.apply_assistant_text(&answer));
        assert_eq!(t.diagram.as_ref().unwrap().doc.title, doc.title);
    }

    #[test]
    fn an_assistant_answer_can_append_to_the_diagram() {
        let mut t = GraphicsTool {
            kind: Kind::Diagram,
            diagram: Some(Box::new(diagram::DiagramState::new(
                hs_diagram::DiagramKind::Flow,
            ))),
            ..GraphicsTool::default()
        };
        let before = t.diagram.as_ref().unwrap().doc.nodes.len();
        let doc = hs_diagram::template::flow();
        let answer = format!(
            "Add these:\n```json\n{}\n```",
            hs_diagram::io::to_json(&doc)
        );
        assert!(t.append_assistant_text(&answer));
        let state = t.diagram.as_ref().unwrap();
        assert!(state.doc.nodes.len() > before);
        assert!(state.doc.nodes.iter().any(|n| n.id.starts_with("ai")));
    }

    #[test]
    fn undo_and_redo_restore_the_image_operations() {
        let mut t = GraphicsTool::default();
        t.push(Op::FlipH);
        t.push(Op::Grayscale);
        assert_eq!(t.ops.len(), 2);
        t.undo_edit();
        assert_eq!(t.ops, vec![Op::FlipH]);
        t.undo_edit();
        assert!(t.ops.is_empty());
        t.redo_edit();
        assert_eq!(t.ops, vec![Op::FlipH]);
        t.redo_edit();
        assert_eq!(t.ops, vec![Op::FlipH, Op::Grayscale]);
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut t = GraphicsTool::default();
        t.push(Op::FlipH);
        t.undo_edit();
        assert_eq!(t.redo_stack.len(), 1);
        t.push(Op::FlipV);
        assert!(t.redo_stack.is_empty());
        assert_eq!(t.ops, vec![Op::FlipV]);
    }

    #[test]
    fn the_diagram_alias_is_recognised() {
        assert!(is_diagram_path(Path::new("a.hdiag")));
        assert!(is_diagram_path(Path::new("a.diagram.json")));
        assert!(is_diagram_path(Path::new("A.DIAGRAM.JSON")));
        assert!(!is_diagram_path(Path::new("a.json")));
    }

    #[test]
    fn closing_a_diagram_writes_a_draft() {
        let dir = std::env::temp_dir().join(format!("hs-graphics-draft-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = GraphicsTool {
            kind: Kind::Diagram,
            drafts: Some(dir.clone()),
            ..Default::default()
        };
        t.diagram = Some(Box::new(diagram::DiagramState::from_doc(
            hs_diagram::template::flow(),
        )));
        assert!(matches!(t.on_close(), CloseDecision::Close));
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names.len(), 1, "one draft: {names:?}");
        assert!(names[0].ends_with(".hdiag"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
