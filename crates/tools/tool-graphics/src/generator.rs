//! Diagram Generator: prompt -> semantic IR -> `DiagramDoc` -> preview.
//!
//! The AI is asked for the strict Archify-adapted IR (no pixels); the preview
//! is laid out by `hs-diagram` like the editor canvas. With no provider the
//! offline template is a real result, not an error.

use std::path::PathBuf;
use std::sync::Arc;

use hs_core::Services;
use hs_core::ai::{self, Message, Role};
use hs_core::tool::{OpenRequest, ToastLevel, ToolContext};
use hs_diagram::export::{PngOptions, pdf, png};
use hs_diagram::model::{DiagramDoc, DiagramKind, Point};
use hs_diagram::schema::{Ir, is_valid, validate};
use hs_diagram::{Rect, Scene, layout_approx};
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use crate::diagram::sanitise;

const SYSTEM: &str = r#"You design HyperStack diagrams. Return ONLY one JSON object, no Markdown fences, no prose.
The object must use schema_version 1 and one of these shapes.

Workflow / flowchart:
{"schema_version":1,"diagram_type":"workflow","meta":{"title":"...","subtitle":""},
 "lanes":[{"id":"client","label":"Client"}],
 "nodes":[{"id":"browser","lane":"client","col":0,"type":"frontend","label":"Browser"}],
 "edges":[{"from":"browser","to":"api","label":"HTTPS","variant":"default","role":"main","route":"auto"}],
 "groups":[]}

Architecture:
{"schema_version":1,"diagram_type":"architecture","meta":{"title":"..."},
 "layout":{"mode":"grid","cols":4},
 "components":[{"id":"api","type":"backend","label":"API","row":0,"col":1}],
 "boundaries":[{"kind":"security-group","label":"VPC","wraps":["api"]}],
 "connections":[{"from":"web","to":"api","label":"HTTPS","route":"orthogonal-h"}]}

Rules: ids match ^[a-zA-Z][a-zA-Z0-9_-]*$ and are unique; every edge endpoint /
connection endpoint / boundary wrap exists; no empty labels; workflow lanes and
nodes are required, architecture components are required; 200 nodes/components
and 500 edges/connections maximum; never emit x/y, size, viewBox, HTML, brand,
views or any field outside the shapes above. Use "decision" for a decision,
"start"/"end" for terminators, "database" for a store, "queue" for a broker."#;

pub struct GeneratorState {
    pub kind: DiagramKind,
    pub prompt: String,
    pub doc: Option<DiagramDoc>,
    pub status: Option<String>,
    pub error: Option<String>,
    running: Option<Running>,
    repair_sent: bool,
    zoom: f32,
    saved: Option<PathBuf>,
}

struct Running {
    text: Arc<Mutex<String>>,
    done: Arc<Mutex<Option<Result<String, String>>>>,
    abort: tokio::task::AbortHandle,
}

impl Default for GeneratorState {
    fn default() -> Self {
        Self::new()
    }
}

impl GeneratorState {
    pub fn new() -> Self {
        Self {
            kind: DiagramKind::Flow,
            prompt: String::new(),
            doc: None,
            status: None,
            error: None,
            running: None,
            repair_sent: false,
            zoom: 0.0,
            saved: None,
        }
    }

    pub fn take_saved(&mut self) -> Option<PathBuf> {
        self.saved.take()
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// The output of an `Open in Editor` request, for the parent tab.
    pub fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        if ui
            .add_enabled(
                self.doc.is_some(),
                egui::Button::new(s.t("gfx-gen-open-editor")),
            )
            .clicked()
        {
            self.open_in_editor(cx);
        }
        ui.add_enabled_ui(self.doc.is_some(), |ui| {
            ui.menu_button(s.t("gfx-gen-export"), |ui| {
                if ui.button("PNG").clicked() {
                    self.export(cx, ExportKind::Png);
                    ui.close();
                }
                if ui.button("PDF").clicked() {
                    self.export(cx, ExportKind::Pdf);
                    ui.close();
                }
            });
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services.clone();
        self.poll(&s);
        egui::Panel::left("gfx-generator-prompt")
            .default_size(370.0)
            .size_range(300.0..=520.0)
            .resizable(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("gfx-generator-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(s.t("gfx-gen-title"))
                                .strong()
                                .size(15.0)
                                .color(theme.text),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(s.t("gfx-gen-type"));
                            egui::ComboBox::from_id_salt("gfx-gen-kind")
                                .selected_text(match self.kind {
                                    DiagramKind::Flow => s.t("gfx-diagram-flow-template"),
                                    DiagramKind::Architecture => {
                                        s.t("gfx-diagram-arch-template")
                                    }
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.kind,
                                        DiagramKind::Flow,
                                        s.t("gfx-diagram-flow-template"),
                                    );
                                    ui.selectable_value(
                                        &mut self.kind,
                                        DiagramKind::Architecture,
                                        s.t("gfx-diagram-arch-template"),
                                    );
                                });
                        });
                        ui.add_space(4.0);
                        ui.add(
                            egui::TextEdit::multiline(&mut self.prompt)
                                .hint_text(s.t("gfx-gen-prompt-hint"))
                                .desired_rows(7)
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(s.t("gfx-gen-examples"))
                                .small()
                                .color(theme.text_muted),
                        );
                        let examples = match self.kind {
                            DiagramKind::Flow => [
                                "Checkout flow: receive order, validate stock, charge card, reserve stock, send confirmation; on payment failure release the stock.",
                                "CI pipeline: checkout, build, test, scan, deploy to staging, approve, deploy to production.",
                                "Support triage: ticket arrives, classify, known issue? then reply with a runbook or escalate.",
                            ],
                            DiagramKind::Architecture => [
                                "A web app with browser, API, worker, Redis and PostgreSQL; group the API, worker and databases behind a private network.",
                                "Mobile client, CDN, API gateway, two services, a queue and a database, with the services inside a VPC.",
                                "Event-driven orders: web, order service, payment service, event bus and two databases.",
                            ],
                        };
                        for example in examples {
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new(example).small(),
                                ))
                                .clicked()
                            {
                                self.prompt = example.to_string();
                            }
                        }
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .button(s.t("gfx-gen-template"))
                                .clicked()
                            {
                                self.template(&s);
                            }
                            if self.running.is_some() {
                                if ui.button(s.t("gfx-gen-stop")).clicked() {
                                    self.stop();
                                }
                            } else if ui
                                .button(format!("{} {}", hs_ui::icons::ph::SPARKLE, s.t("gfx-gen-generate")))
                                .clicked()
                            {
                                self.start(&s);
                            }
                        });
                        ui.add_space(6.0);
                        if let Some(error) = &self.error {
                            ui.colored_label(theme.error, error);
                        }
                        if let Some(status) = &self.status {
                            ui.label(
                                egui::RichText::new(status)
                                    .small()
                                    .color(theme.text_muted),
                            );
                        }
                        if let Some(doc) = &self.doc {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} · {} nodes · {} edges",
                                    doc.title,
                                    doc.nodes.len(),
                                    doc.edges.len()
                                ))
                                .small()
                                .color(theme.text_muted),
                            );
                        }
                        if self.doc.is_none() {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(s.t("gfx-gen-empty-hint"))
                                    .small()
                                    .color(theme.text_muted),
                            );
                        }
                    });
            });

        let rect = ui.available_rect_before_wrap();
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect);
        self.preview(&mut child, theme, &s);
    }

    fn preview(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let Some(doc) = &self.doc else {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new(s.t("gfx-gen-empty")).color(theme.text_muted));
            });
            return;
        };
        let scene: Scene = layout_approx(doc);
        let rect = ui.max_rect();
        let z = if self.zoom == 0.0 {
            ((rect.width() - 48.0) / scene.size.w.max(1.0))
                .min((rect.height() - 48.0) / scene.size.h.max(1.0))
                .clamp(0.1, 4.0)
        } else {
            self.zoom
        };
        let origin = rect.center() - egui::vec2(scene.size.w, scene.size.h) * (z / 2.0);
        let to_screen =
            |p: Point| -> egui::Pos2 { egui::pos2(origin.x + p.x * z, origin.y + p.y * z) };
        let srect = |r: Rect| -> egui::Rect {
            egui::Rect::from_min_max(
                to_screen(Point { x: r.x, y: r.y }),
                to_screen(Point {
                    x: r.right(),
                    y: r.bottom(),
                }),
            )
        };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme.panel_bg);
        crate::diagram::DiagramState::paint_scene(&painter, &scene, &to_screen, &srect, z, theme);
        painter.text(
            rect.left_bottom() + egui::vec2(12.0, -12.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{:.0}%", z * 100.0),
            egui::FontId::proportional(11.0),
            theme.text_muted,
        );
    }

    fn poll(&mut self, s: &Services) {
        let Some(run) = &self.running else {
            return;
        };
        let done = run.done.lock().clone();
        let text = run.text.lock().clone();
        if let Some(result) = done {
            self.running = None;
            match result {
                Ok(text) => {
                    if let Some(reason) = self.apply(&text) {
                        self.repair(s, &text, &reason);
                    }
                }
                Err(e) => {
                    self.error = Some(e);
                    self.status = None;
                }
            }
        } else {
            self.status = Some(format!("Generating… {} characters", text.chars().count()));
        }
    }

    /// Parse and validate a reply. `Some(reason)` means it was rejected and
    /// the caller should try one repair round; the last good document stays.
    fn apply(&mut self, text: &str) -> Option<String> {
        let Some(json) = json_object(text) else {
            let reason = "the response contained no JSON object".to_string();
            self.error = Some(format!("The generated IR is invalid: {reason}"));
            return Some(reason);
        };
        match Ir::parse(json) {
            Ok(ir) => {
                let issues = validate(&ir);
                if !is_valid(&issues) {
                    let errors = issues
                        .iter()
                        .filter(|i| i.severity == hs_diagram::Severity::Error)
                        .take(6)
                        .map(|i| format!("{} at {}", i.code, i.path))
                        .collect::<Vec<_>>()
                        .join("; ");
                    let reason = format!("validation failed: {errors}");
                    self.error = Some(format!("The generated IR is invalid: {reason}"));
                    return Some(reason);
                }
                self.doc = Some(hs_diagram::convert::convert(&ir));
                self.error = None;
                self.repair_sent = false;
                self.status =
                    Some("Generated; opening it in the editor keeps the geometry editable.".into());
                None
            }
            Err(e) => {
                self.error = Some(format!("The generated IR is invalid: {e}"));
                Some(e)
            }
        }
    }

    /// One repair round: hand the rejected output and the reason back to the
    /// model and ask for the corrected JSON only. A second failure is shown.
    fn repair(&mut self, s: &Services, rejected: &str, reason: &str) {
        if self.repair_sent {
            self.error = Some(format!(
                "The generated IR is still invalid after one repair: {reason}"
            ));
            self.status = None;
            return;
        }
        let Some((provider, cfg, key)) = self.provider(s) else {
            self.error = Some(s.t("gfx-gen-no-provider"));
            return;
        };
        self.repair_sent = true;
        let kind = match self.kind {
            DiagramKind::Flow => "workflow",
            DiagramKind::Architecture => "architecture",
        };
        let msgs = vec![
            Message::new(Role::System, SYSTEM),
            Message::new(
                Role::User,
                format!("Diagram type: {kind}\n\n{}", self.prompt.trim()),
            ),
            Message::new(Role::Assistant, rejected.to_string()),
            Message::new(
                Role::User,
                format!(
                    "That output was rejected because {reason}. Reply with only the corrected JSON object, no fences and no prose."
                ),
            ),
        ];
        self.spawn_chat(s, cfg, key, msgs, provider.name);
        self.status = Some("Repairing the generated JSON…".into());
    }

    fn provider(
        &self,
        s: &Services,
    ) -> Option<(
        hs_core::settings::AiProvider,
        hs_core::ai::AiConfig,
        hs_core::ai::Credentials,
    )> {
        let provider = {
            let settings = s.settings.read();
            settings
                .ai
                .providers
                .iter()
                .find(|p| p.id == settings.ai.default_provider)
                .or_else(|| settings.ai.chosen())
                .cloned()
        }?;
        let key = s.ai_credentials(&provider).ok()?;
        let cfg = provider.config("");
        Some((provider, cfg, key))
    }

    fn spawn_chat(
        &mut self,
        s: &Services,
        cfg: hs_core::ai::AiConfig,
        key: hs_core::ai::Credentials,
        msgs: Vec<Message>,
        provider_name: String,
    ) {
        let text = Arc::new(Mutex::new(String::new()));
        let done = Arc::new(Mutex::new(None));
        let out_text = text.clone();
        let out_done = done.clone();
        let waker = s.runtime.waker();
        let repaint = waker.clone();
        let handle = s.runtime.spawn(async move {
            let http = ai::http_client();
            let sink = out_text.clone();
            let r = ai::chat(&http, &cfg, &key, &msgs, &mut move |t: &str| {
                sink.lock().push_str(t);
                repaint.request_repaint();
            })
            .await;
            *out_done.lock() = Some(r.map_err(|e| e.to_string()));
            waker.request_repaint();
        });
        self.running = Some(Running {
            text,
            done,
            abort: handle.abort_handle(),
        });
        self.status = Some(format!("Asking {provider_name}…"));
    }

    fn start(&mut self, s: &Services) {
        self.error = None;
        self.repair_sent = false;
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() {
            self.error = Some(s.t("gfx-gen-need-prompt"));
            return;
        }
        let provider = {
            let settings = s.settings.read();
            settings
                .ai
                .providers
                .iter()
                .find(|p| p.id == settings.ai.default_provider)
                .or_else(|| settings.ai.chosen())
                .cloned()
        };
        let Some(provider) = provider else {
            let hint = s.t("gfx-gen-no-provider");
            self.template_with(s, &hint);
            return;
        };
        let key = match s.ai_credentials(&provider) {
            Ok(key) => key,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let cfg = provider.config("");
        let msgs = vec![
            Message::new(Role::System, SYSTEM),
            Message::new(
                Role::User,
                format!(
                    "Diagram type: {}\n\n{}",
                    match self.kind {
                        DiagramKind::Flow => "workflow",
                        DiagramKind::Architecture => "architecture",
                    },
                    prompt
                ),
            ),
        ];
        let text = Arc::new(Mutex::new(String::new()));
        let done = Arc::new(Mutex::new(None));
        let out_text = text.clone();
        let out_done = done.clone();
        let waker = s.runtime.waker();
        let repaint = waker.clone();
        let handle = s.runtime.spawn(async move {
            let http = ai::http_client();
            let sink = out_text.clone();
            let r = ai::chat(&http, &cfg, &key, &msgs, &mut move |t: &str| {
                sink.lock().push_str(t);
                repaint.request_repaint();
            })
            .await;
            *out_done.lock() = Some(r.map_err(|e| e.to_string()));
            waker.request_repaint();
        });
        self.running = Some(Running {
            text,
            done,
            abort: handle.abort_handle(),
        });
        self.status = Some(format!("Asking {}…", provider.name));
    }

    fn stop(&mut self) {
        if let Some(run) = &self.running {
            run.abort.abort();
        }
        self.running = None;
        self.status = Some("Stopped.".into());
    }

    fn template(&mut self, s: &Services) {
        let doc = match self.kind {
            DiagramKind::Flow => hs_diagram::template::flow(),
            DiagramKind::Architecture => hs_diagram::template::architecture(),
        };
        self.doc = Some(doc);
        self.error = None;
        self.status = Some(s.t("gfx-gen-template-used"));
    }

    fn template_with(&mut self, s: &Services, status: &str) {
        self.template(s);
        self.status = Some(status.into());
    }

    fn open_in_editor(&mut self, cx: &mut ToolContext) {
        let Some(doc) = &self.doc else {
            return;
        };
        let s = cx.services;
        let dir = s.paths.root().join("graphics").join("generated");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            cx.toast(ToastLevel::Error, e.to_string());
            return;
        }
        let base = sanitise(&doc.title);
        let mut path = dir.join(format!("{base}.hdiag"));
        let mut n = 2;
        while path.exists() {
            path = dir.join(format!("{base}-{n}.hdiag"));
            n += 1;
        }
        if let Err(e) = hs_diagram::io::save(&path, doc) {
            cx.toast(ToastLevel::Error, e);
            return;
        }
        self.saved = Some(path.clone());
        cx.open_new_tab(
            "graphics",
            Some(OpenRequest::File {
                uri: path.to_string_lossy().to_string(),
                line: None,
            }),
        );
    }

    fn export(&mut self, cx: &mut ToolContext, kind: ExportKind) {
        let Some(doc) = &self.doc else {
            return;
        };
        let stem = sanitise(&doc.title);
        let (filter, ext) = match kind {
            ExportKind::Png => ("PNG image", "png"),
            ExportKind::Pdf => ("PDF document", "pdf"),
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter(filter, &[ext])
            .set_file_name(format!("{stem}.{ext}"))
            .save_file()
        else {
            return;
        };
        let scene = layout_approx(doc);
        let bytes = match kind {
            ExportKind::Png => png(
                &scene,
                &PngOptions {
                    scale: 2.0,
                    background: Some("#ffffff".into()),
                },
            ),
            ExportKind::Pdf => pdf(&scene, Some("#ffffff")),
        };
        match bytes.and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => cx.toast(
                ToastLevel::Success,
                cx.services
                    .i18n
                    .t_args("gfx-saved", &[("path", &path.display().to_string())]),
            ),
            Err(e) => cx.toast(ToastLevel::Error, e),
        }
    }
}

#[derive(Clone, Copy)]
enum ExportKind {
    Png,
    Pdf,
}

fn json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then_some(&text[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKFLOW: &str = r#"{
      "schema_version": 1,
      "diagram_type": "workflow",
      "meta": { "title": "Checkout" },
      "lanes": [{ "id": "client", "label": "Client" }],
      "nodes": [
        { "id": "browser", "lane": "client", "col": 0, "type": "frontend", "label": "Browser" },
        { "id": "api", "lane": "client", "col": 1, "type": "backend", "label": "API" }
      ],
      "edges": [{ "from": "browser", "to": "api", "label": "HTTPS" }],
      "groups": []
    }"#;

    #[test]
    fn json_object_strips_fences_and_prose() {
        assert_eq!(
            json_object("Here:\n```json\n{\"a\":1}\n```\nDone").unwrap(),
            "{\"a\":1}"
        );
        assert!(json_object("no object").is_none());
    }

    #[test]
    fn apply_accepts_a_valid_workflow() {
        let mut state = GeneratorState::new();
        let _ = state.apply(WORKFLOW);
        let doc = state.doc.expect("a document");
        assert_eq!(doc.title, "Checkout");
        assert_eq!(doc.nodes.len(), 2);
        assert!(state.error.is_none());
    }

    #[test]
    fn apply_keeps_the_last_good_document_on_bad_output() {
        let mut state = GeneratorState::new();
        let _ = state.apply(WORKFLOW);
        let before = state.doc.clone();
        let _ = state.apply("not json");
        assert_eq!(state.doc, before);
        assert!(state.error.is_some());
    }
}
