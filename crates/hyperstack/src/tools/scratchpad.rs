//! Scratchpad: a collection of Markdown notes per workspace. Every note is a
//! file under `<workspace>/notes/<id>.md`; the sidebar lists them, one tab
//! shows one note (`note://<id>`) with the source on the left and the
//! rendered preview on the right, and edits are saved a moment after they
//! stop (Ctrl+S saves at once).

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, Printable, Speakable, ToastLevel, Tool, ToolCategory,
    ToolContext, ToolDescriptor, ToolFactory,
};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use serde::{Deserialize, Serialize};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "scratchpad",
    title_key: "tool-scratchpad",
    icon: hs_ui::icons::NOTE,
    category: ToolCategory::Utilities,
    accepts: &[Accept::Text, Accept::LocalFile],
};

/// Idle time after the last keystroke before a note is written.
const AUTOSAVE_AFTER: Duration = Duration::from_millis(1500);
/// How often the notes folder is re-read while the sidebar is shown.
const RESCAN_EVERY: Duration = Duration::from_secs(10);

pub struct ScratchpadFactory;

impl ToolFactory for ScratchpadFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let mut state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        // A saved note is read back from its file; the layout only carries
        // the draft of a note that was never saved.
        if let Some(id) = state.id.clone() {
            match std::fs::read_to_string(note_path(&notes_dir(services), &id)) {
                Ok(text) => state.text = text,
                Err(_) => state.id = None,
            }
        }
        Box::new(Scratchpad {
            saved: state.text.clone(),
            state,
            notes: Vec::new(),
            pins: Vec::new(),
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
            insert_pending: None,
            scanned: None,
            filter: String::new(),
            last_edit: None,
            confirm_delete: None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum View {
    Edit,
    #[default]
    Split,
    Preview,
}

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(default)]
struct State {
    /// The note's file id; `None` for a note that was never saved.
    id: Option<String>,
    text: String,
    view: View,
    environment: Option<String>,
}

/// A note as listed in the sidebar.
#[derive(Clone)]
struct NoteMeta {
    id: String,
    title: String,
    modified: SystemTime,
    pinned: bool,
}

pub struct Scratchpad {
    state: State,
    saved: String,
    notes: Vec<NoteMeta>,
    pins: Vec<String>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
    /// Text to put into the note at the cursor on the next frame.
    insert_pending: Option<String>,
    scanned: Option<Instant>,
    filter: String,
    last_edit: Option<Instant>,
    confirm_delete: Option<String>,
}

fn notes_dir(s: &Services) -> PathBuf {
    s.workspace.read().root().join("notes")
}

fn note_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.md"))
}

/// The first heading, or the first non-empty line, trimmed to 60 chars.
fn title_of(text: &str) -> Option<String> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(60).collect())
}

/// Read the notes folder: id, title (from the first lines) and mtime,
/// newest first. Only the first 4 KB of each file is read for the title.
fn list_notes(dir: &Path) -> Vec<NoteMeta> {
    use std::io::Read;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|x| x.to_str()) else {
            continue;
        };
        let modified = e
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mut head = Vec::with_capacity(4096);
        if let Ok(f) = std::fs::File::open(&path) {
            let _ = f.take(4096).read_to_end(&mut head);
        }
        let title = title_of(&String::from_utf8_lossy(&head)).unwrap_or_default();
        out.push(NoteMeta {
            id: id.to_string(),
            title,
            modified,
            pinned: false,
        });
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.modified));
    out
}

/// Ids of the notes pinned to the top, in `notes/pins.json`.
fn load_pins(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("pins.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_pins(dir: &Path, pins: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("pins.json"),
        serde_json::to_string_pretty(pins).unwrap_or_default(),
    )
}

/// Pinned notes first (in pin order), then the rest newest first.
fn order_notes(mut notes: Vec<NoteMeta>, pins: &[String]) -> Vec<NoteMeta> {
    for n in &mut notes {
        n.pinned = pins.contains(&n.id);
    }
    let rank = |n: &NoteMeta| pins.iter().position(|p| *p == n.id).unwrap_or(usize::MAX);
    notes.sort_by(|a, b| match (a.pinned, b.pinned) {
        (true, true) => rank(a).cmp(&rank(b)),
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => b.modified.cmp(&a.modified),
    });
    notes
}

fn when(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = t.into();
    let now = chrono::Local::now();
    if dt.date_naive() == now.date_naive() {
        dt.format("%H:%M").to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

impl Scratchpad {
    fn rescan(&mut self, s: &Services) {
        let dir = notes_dir(s);
        self.pins = load_pins(&dir);
        self.notes = order_notes(list_notes(&dir), &self.pins);
        self.scanned = Some(Instant::now());
    }

    /// Pin a note to the top of the list, or unpin it.
    fn toggle_pin(&mut self, id: &str, cx: &mut ToolContext) {
        tracing::debug!(id, pins = ?self.pins, "scratchpad: toggle pin");
        match self.pins.iter().position(|p| p == id) {
            Some(i) => {
                self.pins.remove(i);
            }
            None => self.pins.push(id.to_string()),
        }
        if let Err(e) = save_pins(&notes_dir(cx.services), &self.pins) {
            cx.toast(ToastLevel::Error, e.to_string());
        }
        self.rescan(cx.services);
    }

    /// Write the note (giving it an id on the first save).
    fn save(&mut self, cx: &mut ToolContext) -> bool {
        let dir = notes_dir(cx.services);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            cx.toast(ToastLevel::Error, e.to_string());
            return false;
        }
        let id = self.state.id.get_or_insert_with(hs_core::new_id).clone();
        match std::fs::write(note_path(&dir, &id), &self.state.text) {
            Ok(()) => {
                self.saved = self.state.text.clone();
                self.last_edit = None;
                self.rescan(cx.services);
                true
            }
            Err(e) => {
                cx.toast(ToastLevel::Error, e.to_string());
                false
            }
        }
    }

    fn delete(&mut self, id: &str, cx: &mut ToolContext) {
        let path = note_path(&notes_dir(cx.services), id);
        if let Err(e) = std::fs::remove_file(&path) {
            cx.toast(ToastLevel::Error, e.to_string());
            return;
        }
        if self.state.id.as_deref() == Some(id) {
            self.state.id = None;
            self.state.text.clear();
            self.saved.clear();
        }
        if self.pins.iter().any(|p| p == id) {
            self.pins.retain(|p| p != id);
            let _ = save_pins(&notes_dir(cx.services), &self.pins);
        }
        self.rescan(cx.services);
        cx.toast(ToastLevel::Info, cx.services.t("scratchpad-deleted"));
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let id = ui.id().with("scratchpad-text");
        if let Some(text) = self.insert_pending.take() {
            // At the cursor when the editor has one, else at the end, as
            // its own paragraph.
            let at = egui::TextEdit::load_state(ui.ctx(), id)
                .and_then(|st| st.cursor.char_range())
                .map(|r| r.primary.index.0)
                .unwrap_or(self.state.text.chars().count());
            let byte = self
                .state
                .text
                .char_indices()
                .nth(at)
                .map(|(b, _)| b)
                .unwrap_or(self.state.text.len());
            let before = &self.state.text[..byte];
            let mut piece = String::new();
            if !before.is_empty() && !before.ends_with("\n\n") {
                piece.push_str(if before.ends_with('\n') { "\n" } else { "\n\n" });
            }
            piece.push_str(text.trim_end());
            piece.push('\n');
            self.state.text.insert_str(byte, &piece);
            self.last_edit = Some(Instant::now());
        }
        egui::ScrollArea::vertical()
            .id_salt("scratchpad-edit")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let r = ui.add_sized(
                    [ui.available_width(), ui.available_height().max(200.0)],
                    egui::TextEdit::multiline(&mut self.state.text)
                        .id(id)
                        .code_editor()
                        .frame(egui::Frame::NONE),
                );
                if r.changed() {
                    self.last_edit = Some(Instant::now());
                }
            });
    }

    fn preview(&self, ui: &mut egui::Ui, theme: &Theme) {
        let style = hs_editor::markdown::MarkdownStyle {
            dark: theme.dark,
            text: theme.text,
            muted: theme.text_muted,
            link: theme.secondary,
            code_bg: theme.surface,
            quote_bar: theme.border,
            base_size: 14.0,
        };
        egui::ScrollArea::vertical()
            .id_salt("scratchpad-preview")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        if self.state.text.trim().is_empty() {
                            ui.label(egui::RichText::new("…").color(theme.text_muted));
                        } else {
                            hs_editor::markdown::render(ui, &self.state.text, &style);
                        }
                    });
            });
    }
}

impl Tool for Scratchpad {
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn title(&self) -> Cow<'_, str> {
        match title_of(&self.state.text) {
            Some(t) => Cow::Owned(t),
            None => Cow::Borrowed("New Note"),
        }
    }
    fn dirty(&self) -> bool {
        self.state.text != self.saved
    }
    fn environment(&self) -> Option<String> {
        self.state.environment.clone()
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if self.scanned.is_none_or(|t| t.elapsed() > RESCAN_EVERY) {
            self.rescan(s);
        }
        form::header_bar(ui, &theme, |ui| {
            ui.label(
                egui::RichText::new(s.t("scratchpad-notes"))
                    .strong()
                    .size(15.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(format!("{} {}", ph::PLUS, s.t("scratchpad-new")))
                    .on_hover_text(s.t("scratchpad-new-tip"))
                    .clicked()
                {
                    cx.open_new_tab("scratchpad", None);
                }
            });
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(s.t("scratchpad-search"))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        if let Some(id) = self.confirm_delete.clone() {
            let title = self
                .notes
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| s.t("scratchpad-untitled"));
            form::note(
                ui,
                &theme,
                hs_ui::icons::ph::WARNING,
                &s.i18n
                    .t_args("scratchpad-delete-confirm", &[("title", &title)]),
            );
            ui.horizontal(|ui| {
                if form::danger_button(ui, &theme, &s.t("delete")).clicked() {
                    self.confirm_delete = None;
                    self.delete(&id, cx);
                }
                if ui.button(s.t("cancel")).clicked() {
                    self.confirm_delete = None;
                }
            });
            ui.add_space(4.0);
        }
        let filter = self.filter.trim().to_lowercase();
        let shown: Vec<NoteMeta> = self
            .notes
            .iter()
            .filter(|n| filter.is_empty() || n.title.to_lowercase().contains(&filter))
            .cloned()
            .collect();
        if shown.is_empty() {
            form::hint(
                ui,
                &theme,
                &s.t(if self.notes.is_empty() {
                    "scratchpad-empty"
                } else {
                    "scratchpad-no-match"
                }),
            );
        }
        egui::ScrollArea::vertical()
            .id_salt("scratchpad-notes")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let untitled = s.t("scratchpad-untitled");
                for n in &shown {
                    let selected = self.state.id.as_deref() == Some(n.id.as_str());
                    let title = if n.title.is_empty() {
                        untitled.as_str()
                    } else {
                        n.title.as_str()
                    };
                    let sub = when(n.modified);
                    let icon = if n.pinned { ph::PUSH_PIN } else { ph::NOTE };
                    let r = form::list_row(ui, &theme, selected, icon, title, Some(&sub));
                    if r.secondary_clicked() {
                        tracing::debug!(id = %n.id, "scratchpad: row right-clicked");
                    }
                    if r.clicked() && !selected {
                        tracing::debug!(id = %n.id, "scratchpad: row clicked, opening");
                        cx.open(
                            "scratchpad",
                            Some(OpenRequest::File {
                                uri: format!("note://{}", n.id),
                                line: None,
                            }),
                        );
                    }
                    r.context_menu(|ui| {
                        let (icon, key) = if n.pinned {
                            (ph::PUSH_PIN_SLASH, "scratchpad-unpin")
                        } else {
                            (ph::PUSH_PIN, "scratchpad-pin")
                        };
                        if ui.button(format!("{icon} {}", s.t(key))).clicked() {
                            self.toggle_pin(&n.id, cx);
                            ui.close();
                        }
                        if ui
                            .button(format!("{} {}", hs_ui::icons::TRASH, s.t("delete")))
                            .clicked()
                        {
                            self.confirm_delete = Some(n.id.clone());
                            ui.close();
                        }
                    });
                }
            });
        true
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        for (view, icon, key) in [
            (View::Edit, ph::PENCIL_SIMPLE, "scratchpad-view-edit"),
            (View::Split, ph::COLUMNS, "scratchpad-view-split"),
            (View::Preview, ph::EYE, "scratchpad-view-preview"),
        ] {
            if ui
                .selectable_label(self.state.view == view, icon)
                .on_hover_text(s.t(key))
                .clicked()
            {
                self.state.view = view;
            }
        }
        ui.separator();
        if ui
            .button(format!("{} {}", hs_ui::icons::CHECK, s.t("save")))
            .clicked()
            && self.save(cx)
        {
            cx.toast(ToastLevel::Success, s.t("toast-saved"));
        }
        if let Some(id) = self.state.id.clone() {
            let pinned = self.pins.contains(&id);
            let (icon, key) = if pinned {
                (ph::PUSH_PIN_SLASH, "scratchpad-unpin")
            } else {
                (ph::PUSH_PIN, "scratchpad-pin")
            };
            if ui
                .selectable_label(pinned, icon)
                .on_hover_text(s.t(key))
                .clicked()
            {
                self.toggle_pin(&id, cx);
            }
        }
        if ui
            .button(format!("{} {}", ph::DOWNLOAD_SIMPLE, s.t("download")))
            .on_hover_text(s.t("scratchpad-download-tip"))
            .clicked()
        {
            let name = format!(
                "{}.md",
                hs_core::download::safe_name(
                    &title_of(&self.state.text).unwrap_or_else(|| "note".into())
                )
            );
            match hs_core::download::save_bytes(&name, self.state.text.as_bytes()) {
                Ok(Some(p)) => cx.toast(
                    ToastLevel::Success,
                    s.i18n
                        .t_args("download-saved", &[("path", &p.display().to_string())]),
                ),
                Ok(None) => {}
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
        let status = if self.dirty() {
            s.t("scratchpad-unsaved")
        } else if self.state.id.is_some() {
            s.t("scratchpad-saved")
        } else {
            s.t("tool-scratchpad-hint")
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // The assistant is the same control in every tool, so it sits in
            // the same place: hard right of the toolbar.
            if ui
                .selectable_label(
                    self.assistant.open,
                    format!("{} {}", ph::SPARKLE, s.t("ai-assistant")),
                )
                .on_hover_text(s.t("scratchpad-ai-tip"))
                .clicked()
            {
                self.assistant.toggle("Scratchpad");
            }
            ui.label(egui::RichText::new(status).weak());
        });
        if self.state.environment.is_none()
            && let Some(env) = cx.env_id
        {
            self.state.environment = Some(env.to_string());
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        // Autosave a moment after typing stops. A brand-new empty note is
        // not written: it only becomes a file once it has content.
        if self.dirty()
            && !self.state.text.trim().is_empty()
            && self.last_edit.is_some_and(|t| t.elapsed() > AUTOSAVE_AFTER)
        {
            self.save(cx);
        }
        let theme = Theme::from_settings(&cx.services.settings.read().appearance);
        let s = cx.services;
        let full = ui.available_rect_before_wrap();
        let main_rect = if self.assistant.open {
            egui::Rect::from_min_max(full.min, egui::pos2(full.right() - 390.0, full.bottom()))
        } else {
            full
        };
        let mut main = ui.new_child(egui::UiBuilder::new().max_rect(main_rect));
        main.set_clip_rect(main_rect);
        match self.state.view {
            View::Edit => self.editor(&mut main),
            View::Preview => self.preview(&mut main, &theme),
            View::Split => {
                let half = (main.available_width() * 0.5).round();
                egui::Panel::left(main.id().with("scratchpad-split"))
                    .resizable(true)
                    .default_size(half)
                    .size_range(160.0..=f32::INFINITY)
                    .show(&mut main, |ui| self.editor(ui));
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme.panel_bg))
                    .show(&mut main, |ui| self.preview(ui, &theme));
            }
        }
        if self.assistant.open {
            // The note is the context; the system prompt asks for Markdown
            // pieces in fences so Insert can place them at the cursor.
            self.assistant
                .set_context("the open note", self.state.text.clone());
            self.assistant.extra_system = s.t("scratchpad-ai-system");
            self.assistant.can_insert = true;
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
                    let out = self.assistant.ui(ui, s, &theme, "Scratchpad");
                    if let Some(block) = out.use_block {
                        // Markdown goes in as it is; any other language is
                        // kept as a fenced block of that language.
                        let lang = out.use_block_lang.trim().to_ascii_lowercase();
                        let text = if lang.is_empty() || lang == "markdown" || lang == "md" {
                            block
                        } else {
                            format!("```{lang}\n{block}\n```")
                        };
                        self.insert_pending = Some(text);
                    }
                });
        }
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![
            CommandDescriptor {
                id: "scratchpad.save".into(),
                title: "Scratchpad: Save Note".into(),
                shortcut_action: Some("file.save".into()),
            },
            CommandDescriptor {
                id: "scratchpad.new".into(),
                title: "Scratchpad: New Note".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "scratchpad.clear".into(),
                title: "Scratchpad: Clear Note".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "scratchpad.ai".into(),
                title: "Scratchpad: Toggle AI Assistant".into(),
                shortcut_action: None,
            },
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "scratchpad.save" => {
                if self.save(cx) {
                    cx.toast(ToastLevel::Success, cx.services.t("toast-saved"));
                }
                true
            }
            "scratchpad.new" => {
                cx.open_new_tab("scratchpad", None);
                true
            }
            "scratchpad.clear" => {
                self.state.text.clear();
                self.last_edit = Some(Instant::now());
                true
            }
            "scratchpad.ai" => {
                self.assistant.toggle("Scratchpad");
                true
            }
            _ => false,
        }
    }

    fn save_state(&self) -> serde_json::Value {
        let mut st = self.state.clone();
        if st.id.is_some() {
            // The file is the source of truth for a saved note.
            st.text.clear();
        }
        serde_json::to_value(&st).unwrap_or_default()
    }

    fn current_uri(&self) -> Option<String> {
        self.state.id.as_ref().map(|id| format!("note://{id}"))
    }

    /// The tab showing a note takes requests for it; an empty new tab takes
    /// any note; a tab with another note never does.
    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        let id = uri.strip_prefix("note://")?;
        match &self.state.id {
            Some(mine) if mine == id => Some(0),
            None if self.state.text.trim().is_empty() => Some(200),
            _ => None,
        }
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Text { body, .. } => {
                self.state.text = body;
                self.last_edit = Some(Instant::now());
                true
            }
            OpenRequest::File { uri, .. } => {
                tracing::debug!(%uri, current = ?self.state.id, "scratchpad: open");
                if let Some(id) = uri.strip_prefix("note://") {
                    let path = note_path(&notes_dir(cx.services), id);
                    return match std::fs::read_to_string(path) {
                        Ok(text) => {
                            self.state.id = Some(id.to_string());
                            self.saved = text.clone();
                            self.state.text = text;
                            self.last_edit = None;
                            true
                        }
                        Err(_) => false,
                    };
                }
                // A Markdown file from elsewhere starts a new note with its
                // content (the original file is not touched).
                let path = uri.strip_prefix("file://").unwrap_or(&uri);
                match std::fs::read_to_string(path) {
                    Ok(t) => {
                        self.state.id = None;
                        self.saved.clear();
                        self.state.text = t;
                        self.last_edit = Some(Instant::now());
                        true
                    }
                    Err(_) => false,
                }
            }
            _ => false,
        }
    }

    fn speakable(&self) -> Vec<Speakable> {
        let mut out = Vec::new();
        if !self.state.text.trim().is_empty() {
            out.push(Speakable::new("Note", self.state.text.clone()));
        }
        if let Some(a) = self.assistant.last_answer() {
            out.push(Speakable::new("Assistant's last answer", a));
        }
        out
    }

    fn printable(&self) -> Option<Printable> {
        (!self.state.text.trim().is_empty()).then(|| Printable {
            title: self.title().to_string(),
            subtitle: "Scratchpad".into(),
            html: hs_editor::markdown::to_html(&self.state.text),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_comes_from_the_first_heading_or_line() {
        assert_eq!(title_of("# Shopping\n- milk").as_deref(), Some("Shopping"));
        assert_eq!(
            title_of("\n\n  plain first line\n").as_deref(),
            Some("plain first line")
        );
        assert_eq!(title_of("   \n"), None);
        assert_eq!(title_of("###   \nnext").as_deref(), Some("next"));
    }

    #[test]
    fn pinned_notes_come_first_in_pin_order() {
        let mk = |id: &str, secs: u64| NoteMeta {
            id: id.into(),
            title: id.into(),
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
            pinned: false,
        };
        let notes = vec![
            mk("old", 1),
            mk("new", 3),
            mk("mid", 2),
            mk("pinned-late", 0),
        ];
        let out = order_notes(notes, &["mid".into(), "pinned-late".into()]);
        let ids: Vec<&str> = out.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["mid", "pinned-late", "new", "old"]);
        assert!(out[0].pinned && out[1].pinned && !out[2].pinned);
    }

    #[test]
    fn notes_are_listed_newest_first_with_titles() {
        let dir = std::env::temp_dir().join(format!("hs-notes-{}", hs_core::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# First\nbody").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("b.md"), "second line only").unwrap();
        std::fs::write(dir.join("ignored.txt"), "no").unwrap();
        let notes = list_notes(&dir);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].id, "b");
        assert_eq!(notes[0].title, "second line only");
        assert_eq!(notes[1].title, "First");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
