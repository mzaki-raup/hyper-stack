//! The AI assistant panel (ADR-019, docs/tools/editor.md §10): chat about
//! the open file or the selection, review it, and let the model change it.
//! Changes arrive as SEARCH/REPLACE blocks drawn as diffs with Apply and
//! Reject; an applied change is one undo step. **Cascade** applies the
//! blocks itself and sends the updated file back for another round, up to
//! the configured limit, until the model answers DONE.
//!
//! The request runs on the tokio runtime; the streamed answer lands in a
//! shared buffer drained each frame (ADR-013).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use hs_core::Services;
use hs_core::ai::{self, EditBlock, Message, Role, Segment};
use hs_core::secrets::SecretRef;
use hs_core::settings::AiProvider;
use hs_editor::buffer::{Buffer, Edit, EditKind};
use hs_editor::cursor::{Cursor, Cursors, byte_to_char};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;

/// Larger files are cut to this many characters (a note tells the model).
const MAX_CONTEXT_CHARS: usize = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Context {
    /// The selection when there is one, else the whole file.
    #[default]
    Auto,
    File,
    Selection,
    None,
}

#[derive(Debug, Clone, PartialEq)]
enum EditState {
    Pending,
    Applied,
    Rejected,
    Failed(String),
}

struct Turn {
    role: Role,
    /// What the chat shows (the request, or the answer).
    shown: String,
    /// What the model was sent (the request plus the code context).
    sent: String,
    segments: Vec<Segment>,
    edits: Vec<EditState>,
}

#[derive(Default)]
struct Stream {
    text: String,
    done: Option<Result<(), String>>,
}

struct Running {
    stream: Arc<Mutex<Stream>>,
    abort: tokio::task::AbortHandle,
}

/// The models a provider offers: `None` while the request runs.
type ModelList = Arc<Mutex<Option<Result<Vec<String>, String>>>>;

/// Model lists by provider (id, kind and endpoint), shared by every editor
/// tab so a provider is asked once per session unless the user refreshes.
static MODELS: LazyLock<Mutex<HashMap<String, ModelList>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The provider's API key from the vault (empty for a local server).
fn provider_key(s: &Services, p: &AiProvider) -> Result<String, String> {
    match SecretRef::parse(&p.api_key) {
        None => Ok(String::new()),
        Some(_) if s.secrets.is_locked() => Err(s.t("settings-vault-unlock-first")),
        Some(r) => s
            .secrets
            .get(&r)
            .map(|v| secrecy::ExposeSecret::expose_secret(&v).to_string())
            .map_err(|e| e.to_string()),
    }
}

/// The provider's model list, asking the service the first time (or again
/// on `refresh`). Runs on the runtime; the list fills in when it answers.
fn models_for(s: &Services, p: &AiProvider, refresh: bool) -> ModelList {
    let cache_key = format!("{}|{:?}|{}", p.id, p.kind, p.base_url);
    if !refresh && let Some(l) = MODELS.lock().get(&cache_key) {
        return l.clone();
    }
    let list: ModelList = Arc::new(Mutex::new(None));
    MODELS.lock().insert(cache_key, list.clone());
    let key = match p.kind {
        // 1min.AI's list is built in: no key, no unlocked vault needed.
        ai::Provider::OneMin => Ok(String::new()),
        ai::Provider::OpenAi => provider_key(s, p),
    };
    let key = match key {
        Ok(k) => k,
        Err(e) => {
            *list.lock() = Some(Err(e));
            return list;
        }
    };
    let cfg = p.config("");
    let out = list.clone();
    let waker = s.runtime.waker();
    s.runtime.spawn(async move {
        let http = ai::http_client();
        let r = ai::list_models(&http, &cfg, &key)
            .await
            .map_err(|e| e.to_string());
        *out.lock() = Some(r);
        waker.request_repaint();
    });
    list
}

#[derive(Default)]
pub struct AiPanel {
    pub open: bool,
    provider: String,
    model: String,
    input: String,
    context: Context,
    cascade: bool,
    turns: Vec<Turn>,
    running: Option<Running>,
    /// Cascade rounds run since the user last asked something.
    rounds: u32,
    focus_input: bool,
    /// Speech to text for the input box (the microphone toggle).
    stt: hs_core::stt::Stt,
    /// A microphone error, shown under the input for a few seconds.
    notice: Option<(String, std::time::Instant)>,
    stick_to_bottom: bool,
}

/// What the panel needs from the editor for one frame.
pub struct EditorCtx<'a> {
    pub buf: &'a mut Buffer,
    pub cursors: &'a mut Cursors,
    pub read_only: bool,
    pub file_label: String,
    pub language: &'static str,
}

impl AiPanel {
    /// The assistant's latest complete answer, for read-aloud.
    pub fn last_answer(&self) -> Option<&str> {
        self.turns
            .iter()
            .rev()
            .find(|t| t.role == Role::Assistant && !t.shown.is_empty())
            .map(|t| t.shown.as_str())
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.focus_input = self.open;
    }

    /// Open the panel and ask a canned question about the code.
    pub fn quick(&mut self, s: &Services, ed: &mut EditorCtx, key: &str) {
        self.open = true;
        let text = s.t(key);
        self.send(s, ed, text);
    }

    fn stop(&mut self) {
        if let Some(r) = self.running.take() {
            r.abort.abort();
            if let Some(t) = self.turns.last_mut()
                && t.role == Role::Assistant
            {
                t.shown = r.stream.lock().text.clone();
                t.sent = t.shown.clone();
                t.segments = ai::segments(&t.shown);
                t.edits = vec![EditState::Pending; count_edits(&t.segments)];
            }
        }
        self.rounds = 0;
    }

    /// The code context for the next request and a short label for it.
    fn context_text(&self, ed: &EditorCtx) -> (String, Option<(usize, usize, String)>) {
        let mut text = ed.buf.text();
        if text.chars().count() > MAX_CONTEXT_CHARS {
            text = text.chars().take(MAX_CONTEXT_CHARS).collect();
            text.push_str("\n… (file truncated)");
        }
        let c = ed.cursors.primary();
        let sel = c.has_selection().then(|| {
            let (a, b) = c.range();
            let (l1, _) = ed.buf.char_to_line_col(a);
            let (l2, _) = ed.buf.char_to_line_col(b);
            (l1 + 1, l2 + 1, ed.buf.slice(a, b))
        });
        (text, sel)
    }

    fn send(&mut self, s: &Services, ed: &mut EditorCtx, request: String) {
        if self.running.is_some() || request.trim().is_empty() {
            return;
        }
        let (text, sel) = self.context_text(ed);
        let sent = match self.context {
            Context::None => request.clone(),
            Context::File => {
                ai::context_message(&ed.file_label, ed.language, &text, None, &request)
            }
            Context::Selection | Context::Auto => match &sel {
                Some((a, b, t)) => ai::context_message(
                    &ed.file_label,
                    ed.language,
                    &text,
                    Some((*a, *b, t)),
                    &request,
                ),
                None if self.context == Context::Auto => {
                    ai::context_message(&ed.file_label, ed.language, &text, None, &request)
                }
                None => request.clone(),
            },
        };
        self.turns.push(Turn {
            role: Role::User,
            shown: request,
            sent,
            segments: Vec::new(),
            edits: Vec::new(),
        });
        self.start(s);
    }

    /// Start the request for the conversation so far.
    fn start(&mut self, s: &Services) {
        let st = s.settings.read();
        let provider = st
            .ai
            .providers
            .iter()
            .find(|p| p.id == self.provider)
            .or(st.ai.chosen())
            .cloned();
        drop(st);
        let Some(provider) = provider else {
            self.fail(s.t("ai-no-provider"));
            return;
        };
        let cfg = provider.config(&self.model);
        // Only the newest request carries the code; earlier ones are
        // reduced to their text so the conversation stays small.
        let last_user = self.turns.iter().rposition(|t| t.role == Role::User);
        let mut msgs = vec![Message::new(Role::System, ai::code_system_prompt())];
        for (i, t) in self.turns.iter().enumerate() {
            let content = if Some(i) == last_user || t.role == Role::Assistant {
                t.sent.clone()
            } else {
                t.shown.clone()
            };
            msgs.push(Message::new(t.role, content));
        }
        let key = match provider_key(s, &provider) {
            Ok(k) => k,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let stream = Arc::new(Mutex::new(Stream::default()));
        let out = stream.clone();
        let waker = s.runtime.waker();
        let handle = s.runtime.spawn(async move {
            let http = ai::http_client();
            let sink = out.clone();
            let w = waker.clone();
            let r = ai::chat(&http, &cfg, &key, &msgs, &mut move |t: &str| {
                sink.lock().text.push_str(t);
                w.request_repaint();
            })
            .await;
            out.lock().done = Some(r.map(|_| ()).map_err(|e| e.to_string()));
            waker.request_repaint();
        });
        self.turns.push(Turn {
            role: Role::Assistant,
            shown: String::new(),
            sent: String::new(),
            segments: Vec::new(),
            edits: Vec::new(),
        });
        self.running = Some(Running {
            stream,
            abort: handle.abort_handle(),
        });
        self.stick_to_bottom = true;
    }

    fn fail(&mut self, msg: String) {
        self.turns.push(Turn {
            role: Role::Assistant,
            shown: format!("⚠ {msg}"),
            sent: String::new(),
            segments: vec![Segment::Text(format!("⚠ {msg}"))],
            edits: Vec::new(),
        });
        self.rounds = 0;
    }

    /// Take what streamed in; at the end, parse the edits and run Cascade.
    fn poll(&mut self, s: &Services, ed: &mut EditorCtx) {
        let Some(r) = &self.running else { return };
        let (text, done) = {
            let st = r.stream.lock();
            (st.text.clone(), st.done.clone())
        };
        let Some(turn) = self.turns.last_mut() else {
            return;
        };
        if turn.shown.len() != text.len() {
            turn.shown = text.clone();
            turn.segments = ai::segments(&text);
            self.stick_to_bottom = true;
        }
        let Some(done) = done else { return };
        self.running = None;
        turn.sent = text.clone();
        turn.segments = ai::segments(&text);
        turn.edits = vec![EditState::Pending; count_edits(&turn.segments)];
        if let Err(e) = done {
            if text.is_empty() {
                self.turns.pop();
            }
            self.fail(e);
            return;
        }
        let idx = self.turns.len() - 1;
        let n = self.turns[idx].edits.len();
        if !self.cascade || n == 0 || is_done(&text) {
            self.rounds = 0;
            return;
        }
        let applied = self.apply_all(idx, ed);
        let max = s.settings.read().ai.max_rounds.max(1);
        self.rounds += 1;
        if applied == 0 || self.rounds >= max {
            if self.rounds >= max {
                self.fail(
                    s.i18n
                        .t_args("ai-cascade-limit", &[("count", &max.to_string())]),
                );
            }
            self.rounds = 0;
            return;
        }
        let (text, _) = self.context_text(ed);
        let note = s.t("ai-cascade-continue");
        let sent = ai::context_message(&ed.file_label, ed.language, &text, None, &note);
        self.turns.push(Turn {
            role: Role::User,
            shown: note,
            sent,
            segments: Vec::new(),
            edits: Vec::new(),
        });
        self.start(s);
    }

    /// Apply one block of turn `t` to the buffer as one undo step.
    fn apply(&mut self, t: usize, e: usize, ed: &mut EditorCtx) -> bool {
        let Some(block) = nth_edit(&self.turns[t].segments, e).cloned() else {
            return false;
        };
        let state = &mut self.turns[t].edits[e];
        if ed.read_only {
            *state = EditState::Failed("read-only".into());
            return false;
        }
        let text = ed.buf.text();
        match ai::locate(&text, &block.search) {
            Err(err) => {
                *state = EditState::Failed(
                    match err {
                        ai::LocateError::NotFound => "not found in the file",
                        ai::LocateError::Ambiguous => "matches more than one place",
                    }
                    .into(),
                );
                false
            }
            Ok((bs, be)) => {
                let mut inserted = block.replace.clone();
                if block.search.trim().is_empty() && !text.is_empty() && !text.ends_with('\n') {
                    inserted.insert(0, '\n');
                }
                let start = byte_to_char(&text, bs);
                let removed = text[bs..be].to_string();
                let end_after = start + inserted.chars().count();
                let before = ed.cursors.snapshot();
                ed.buf.apply(
                    vec![Edit {
                        pos: start,
                        removed,
                        inserted,
                    }],
                    EditKind::Other,
                    before,
                    vec![(end_after, end_after)],
                );
                ed.cursors.set_single(Cursor::at(end_after));
                ed.buf.break_undo_group();
                *state = EditState::Applied;
                true
            }
        }
    }

    fn apply_all(&mut self, t: usize, ed: &mut EditorCtx) -> usize {
        let mut n = 0;
        for e in 0..self.turns[t].edits.len() {
            if self.turns[t].edits[e] == EditState::Pending && self.apply(t, e, ed) {
                n += 1;
            }
        }
        n
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme, ed: &mut EditorCtx) {
        self.poll(s, ed);
        let all: Vec<AiProvider> = s.settings.read().ai.providers.clone();
        let providers: Vec<(String, String, String)> = all
            .iter()
            .map(|p| (p.id.clone(), p.name.clone(), p.model.clone()))
            .collect();
        let chosen = s.settings.read().ai.chosen().cloned();
        if (self.provider.is_empty() || !providers.iter().any(|p| p.0 == self.provider))
            && let Some(p) = chosen
        {
            self.provider = p.id;
            self.model = p.model;
        }
        hs_ui::form::header_bar(ui, theme, |ui| {
            ui.label(
                egui::RichText::new(format!("{} {}", ph::SPARKLE, s.t("ai-assistant"))).strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(|ui: &mut egui::Ui| {
                        hs_ui::form::icon_button(ui, theme, ph::X, 16.0, false)
                    })
                    .tip(s.t("ai-close"))
                    .clicked()
                {
                    self.open = false;
                }
                if ui
                    .add(|ui: &mut egui::Ui| {
                        hs_ui::form::icon_button(ui, theme, ph::BROOM, 16.0, false)
                    })
                    .tip(s.t("ai-new-chat"))
                    .clicked()
                {
                    self.stop();
                    self.turns.clear();
                }
            });
        });
        if providers.is_empty() {
            ui.add_space(8.0);
            hs_ui::form::note(ui, theme, hs_ui::icons::INFO, &s.t("ai-setup-hint"));
            return;
        }
        ui.horizontal(|ui| {
            let name = providers
                .iter()
                .find(|p| p.0 == self.provider)
                .map(|p| p.1.clone())
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("ai-provider")
                .width(110.0)
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for (id, name, model) in &providers {
                        if ui.selectable_label(self.provider == *id, name).clicked() {
                            self.provider = id.clone();
                            self.model = model.clone();
                        }
                    }
                });
            if let Some(p) = all.iter().find(|p| p.id == self.provider) {
                self.model_picker(ui, s, theme, p);
            }
        });
        ui.horizontal(|ui| {
            let has_sel = ed.cursors.primary().has_selection();
            let label = |c: Context| match c {
                Context::Auto if has_sel => s.t("ai-context-auto-selection"),
                Context::Auto => s.t("ai-context-auto-file"),
                Context::File => s.t("ai-context-file"),
                Context::Selection => s.t("ai-context-selection"),
                Context::None => s.t("ai-context-none"),
            };
            egui::ComboBox::from_id_salt("ai-context")
                .width(150.0)
                .selected_text(label(self.context))
                .show_ui(ui, |ui| {
                    for c in [
                        Context::Auto,
                        Context::File,
                        Context::Selection,
                        Context::None,
                    ] {
                        ui.selectable_value(&mut self.context, c, label(c));
                    }
                })
                .response
                .tip(s.t("ai-context-tip"));
            ui.toggle_value(
                &mut self.cascade,
                format!("{} {}", ph::LIGHTNING, s.t("ai-cascade")),
            )
            .tip(s.t("ai-cascade-tip"));
        });
        ui.horizontal_wrapped(|ui| {
            let enabled = self.running.is_none();
            for (key, prompt) in [
                ("ai-explain", "ai-explain-prompt"),
                ("ai-review", "ai-review-prompt"),
                ("ai-improve", "ai-improve-prompt"),
                ("ai-add-comments", "ai-add-comments-prompt"),
            ] {
                if ui
                    .add_enabled(enabled, egui::Button::new(s.t(key)).small())
                    .clicked()
                {
                    self.send(s, ed, s.t(prompt));
                }
            }
        });
        ui.separator();
        // Input at the bottom, conversation above.
        let mut send_now = false;
        egui::Panel::bottom(egui::Id::new("ai-input").with(ui.id()))
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(4.0);
                let r = ui.add(
                    egui::TextEdit::multiline(&mut self.input)
                        .hint_text(s.t("ai-input-hint"))
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                if self.focus_input {
                    r.request_focus();
                    self.focus_input = false;
                }
                if r.has_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command)
                {
                    send_now = true;
                }
                ui.horizontal(|ui| {
                    if self.running.is_some() {
                        ui.spinner();
                        if ui
                            .button(format!("{} {}", ph::STOP, s.t("ai-stop")))
                            .clicked()
                        {
                            self.stop();
                        }
                    } else if hs_ui::form::primary_button(
                        ui,
                        theme,
                        &format!("{} {}", ph::PAPER_PLANE_RIGHT, s.t("ai-send")),
                    )
                    .tip(s.t("ai-send-tip"))
                    .clicked()
                    {
                        send_now = true;
                    }
                    if let Some(e) =
                        hs_ui::mic::mic_button(ui, theme, s, &self.stt, &mut self.input)
                    {
                        self.notice = Some((e, std::time::Instant::now()));
                    }
                });
                if let Some((text, at)) = &self.notice {
                    if at.elapsed() > std::time::Duration::from_secs(8) {
                        self.notice = None;
                    } else {
                        ui.add(
                            egui::Label::new(egui::RichText::new(text).color(theme.error).small())
                                .wrap(),
                        );
                    }
                }
                ui.add_space(4.0);
            });
        if send_now && self.running.is_none() {
            let text = std::mem::take(&mut self.input);
            // A Ctrl+Enter newline is not part of the question.
            self.send(s, ed, text.trim_end().to_string());
            self.focus_input = true;
        }
        let mut actions = Vec::new();
        let stick = std::mem::take(&mut self.stick_to_bottom);
        egui::ScrollArea::vertical()
            .id_salt("ai-chat")
            .auto_shrink([false, false])
            .stick_to_bottom(stick)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.turns.is_empty() {
                    hs_ui::form::hint(ui, theme, &s.t("ai-empty"));
                }
                for (ti, t) in self.turns.iter().enumerate() {
                    self.turn_ui(ui, s, theme, ti, t, &mut actions);
                    ui.add_space(8.0);
                }
                if stick {
                    ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                }
            });
        for a in actions {
            match a {
                Action::Apply(t, e) => {
                    self.apply(t, e, ed);
                }
                Action::Reject(t, e) => self.turns[t].edits[e] = EditState::Rejected,
                Action::ApplyAll(t) => {
                    self.apply_all(t, ed);
                }
                Action::Insert(code) => {
                    if !ed.read_only {
                        insert_at_cursor(ed, &code);
                    }
                }
                Action::Copy(code) => ui.ctx().copy_text(code),
            }
        }
    }

    /// The model dropdown: what the provider lists (fetched when the
    /// provider is first picked), plus a field for an id it does not list.
    fn model_picker(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme, p: &AiProvider) {
        let list = models_for(s, p, false);
        let shown = if self.model.trim().is_empty() {
            p.kind.default_model().to_string()
        } else {
            self.model.clone()
        };
        // Leave room for the refresh button so the panel never grows.
        let width = (ui.available_width() - 40.0).max(80.0);
        let state = list.lock().clone();
        egui::ComboBox::from_id_salt("ai-model")
            .width(width)
            .truncate()
            .height(420.0)
            .selected_text(shown)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                ui.set_min_width(width.max(220.0));
                match &state {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(s.t("ai-models-loading"));
                        });
                    }
                    Some(Err(e)) => {
                        ui.set_max_width(width.max(220.0));
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("{}: {e}", s.t("ai-models-failed")))
                                    .color(theme.error),
                            )
                            .wrap(),
                        );
                    }
                    Some(Ok(v)) if v.is_empty() => {
                        ui.label(egui::RichText::new(s.t("ai-models-none")).weak());
                    }
                    Some(Ok(v)) => {
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
                                for (heading, models) in ai::group_models(p.kind, v) {
                                    if !heading.is_empty() {
                                        ui.label(
                                            egui::RichText::new(heading)
                                                .size(11.0)
                                                .strong()
                                                .color(theme.text_muted),
                                        );
                                    }
                                    for m in models {
                                        let r = ui.selectable_label(self.model == m, &m);
                                        let r = match ai::onemin_model_name(&m) {
                                            Some(name) if p.kind == ai::Provider::OneMin => {
                                                r.on_hover_text(name)
                                            }
                                            _ => r,
                                        };
                                        if r.clicked() {
                                            self.model = m;
                                            ui.close();
                                        }
                                    }
                                }
                            });
                    }
                }
                ui.separator();
                ui.label(egui::RichText::new(s.t("ai-model-other")).size(12.0));
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.model)
                        .hint_text(p.kind.default_model())
                        .desired_width(f32::INFINITY),
                );
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    ui.close();
                }
            })
            .response
            .tip(s.t("ai-model-tip"));
        if ui
            .add(|ui: &mut egui::Ui| {
                hs_ui::form::icon_button(ui, theme, ph::ARROW_CLOCKWISE, 16.0, false)
            })
            .tip(s.t("ai-models-refresh"))
            .clicked()
        {
            models_for(s, p, true);
        }
    }

    fn turn_ui(
        &self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        ti: usize,
        t: &Turn,
        actions: &mut Vec<Action>,
    ) {
        let (who, fill) = match t.role {
            Role::User => (s.t("ai-you"), theme.surface),
            _ => (s.t("ai-assistant"), theme.panel_bg),
        };
        egui::Frame::new()
            .fill(fill)
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new(who)
                        .strong()
                        .color(theme.text_muted)
                        .size(12.0),
                );
                if t.role == Role::User {
                    ui.add(egui::Label::new(&t.shown).wrap());
                    return;
                }
                if t.shown.is_empty() && self.running.is_some() {
                    ui.spinner();
                    return;
                }
                let pending = t.edits.iter().filter(|e| **e == EditState::Pending).count();
                let mut ei = 0;
                for seg in &t.segments {
                    match seg {
                        Segment::Text(text) => prose(ui, s, theme, text, actions),
                        Segment::Edit(b) => {
                            let st = t.edits.get(ei).cloned().unwrap_or(EditState::Pending);
                            edit_ui(ui, s, theme, b, &st, ti, ei, actions);
                            ei += 1;
                        }
                    }
                }
                if pending > 1
                    && ui
                        .button(format!("{} {}", ph::CHECKS, s.t("ai-apply-all")))
                        .clicked()
                {
                    actions.push(Action::ApplyAll(ti));
                }
            });
    }
}

enum Action {
    Apply(usize, usize),
    Reject(usize, usize),
    ApplyAll(usize),
    Insert(String),
    Copy(String),
}

fn count_edits(segs: &[Segment]) -> usize {
    segs.iter()
        .filter(|s| matches!(s, Segment::Edit(_)))
        .count()
}

fn nth_edit(segs: &[Segment], n: usize) -> Option<&EditBlock> {
    segs.iter()
        .filter_map(|s| match s {
            Segment::Edit(b) => Some(b),
            _ => None,
        })
        .nth(n)
}

/// The model's "nothing more to do" answer in a Cascade round.
fn is_done(answer: &str) -> bool {
    let a = answer.trim().trim_matches(['*', '.', '`']);
    a.eq_ignore_ascii_case("done")
}

fn insert_at_cursor(ed: &mut EditorCtx, code: &str) {
    let c = ed.cursors.primary();
    let (a, b) = c.range();
    let removed = ed.buf.slice(a, b);
    let end = a + code.chars().count();
    let before = ed.cursors.snapshot();
    ed.buf.apply(
        vec![Edit {
            pos: a,
            removed,
            inserted: code.to_string(),
        }],
        EditKind::Other,
        before,
        vec![(end, end)],
    );
    ed.cursors.set_single(Cursor::at(end));
    ed.buf.break_undo_group();
}

/// Prose with fenced code drawn as monospace blocks (Copy, Insert).
fn prose(ui: &mut egui::Ui, s: &Services, theme: &Theme, text: &str, actions: &mut Vec<Action>) {
    let mut in_code = false;
    let mut buf = String::new();
    let flush_text = |ui: &mut egui::Ui, t: &str| {
        let t = t.trim_matches('\n');
        if !t.is_empty() {
            ui.add(egui::Label::new(t).wrap());
        }
    };
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            if in_code {
                code_block(ui, s, theme, buf.trim_end_matches('\n'), actions);
            } else {
                flush_text(ui, &buf);
            }
            buf.clear();
            in_code = !in_code;
            continue;
        }
        buf.push_str(line);
    }
    if in_code {
        code_block(ui, s, theme, buf.trim_end_matches('\n'), actions);
    } else {
        flush_text(ui, &buf);
    }
}

fn code_block(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    code: &str,
    actions: &mut Vec<Action>,
) {
    egui::Frame::new()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if ui
                    .small_button(ph::COPY)
                    .on_hover_text(s.t("ai-copy-code"))
                    .clicked()
                {
                    actions.push(Action::Copy(code.to_string()));
                }
                if ui
                    .small_button(ph::ARROW_LINE_DOWN)
                    .on_hover_text(s.t("ai-insert-code"))
                    .clicked()
                {
                    actions.push(Action::Insert(code.to_string()));
                }
            });
            ui.add(egui::Label::new(egui::RichText::new(code).monospace()).wrap());
        });
}

#[allow(clippy::too_many_arguments)] // one call site, all pieces needed
fn edit_ui(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    b: &EditBlock,
    st: &EditState,
    ti: usize,
    ei: usize,
    actions: &mut Vec<Action>,
) {
    egui::Frame::new()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(s.t("ai-change")).strong().size(12.0));
                match st {
                    EditState::Pending => {
                        if ui
                            .small_button(format!("{} {}", ph::CHECK, s.t("ai-apply")))
                            .clicked()
                        {
                            actions.push(Action::Apply(ti, ei));
                        }
                        if ui
                            .small_button(format!("{} {}", ph::X, s.t("ai-reject")))
                            .clicked()
                        {
                            actions.push(Action::Reject(ti, ei));
                        }
                    }
                    EditState::Applied => {
                        ui.colored_label(
                            theme.success,
                            format!("{} {}", ph::CHECK, s.t("ai-applied")),
                        );
                    }
                    EditState::Rejected => {
                        ui.colored_label(theme.text_muted, s.t("ai-rejected"));
                    }
                    EditState::Failed(why) => {
                        ui.colored_label(theme.error, format!("{} {why}", s.t("ai-not-applied")))
                            .on_hover_text(s.t("ai-not-applied-tip"));
                    }
                }
            });
            let red = egui::Color32::from_rgba_unmultiplied(220, 60, 60, 40);
            let green = egui::Color32::from_rgba_unmultiplied(60, 180, 90, 40);
            for (lines, sign, fill) in [(&b.search, "-", red), (&b.replace, "+", green)] {
                if lines.is_empty() {
                    continue;
                }
                egui::Frame::new().fill(fill).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let shown: String = lines.lines().map(|l| format!("{sign} {l}\n")).collect();
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(shown.trim_end()).monospace().size(12.0),
                        )
                        .wrap(),
                    );
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_answers() {
        assert!(is_done("DONE"));
        assert!(is_done(" **Done.** "));
        assert!(!is_done("Done with the first part, now …"));
    }

    #[test]
    fn applying_a_block_is_one_undo_step() {
        let mut buf = Buffer::from_text("fn a() {\n    1\n}\n");
        let mut cursors = Cursors::default();
        let mut ed = EditorCtx {
            buf: &mut buf,
            cursors: &mut cursors,
            read_only: false,
            file_label: "a.rs".into(),
            language: "rust",
        };
        let mut p = AiPanel::default();
        let answer = "x\n<<<<<<< SEARCH\n    1\n=======\n    2\n>>>>>>> REPLACE\n";
        p.turns.push(Turn {
            role: Role::Assistant,
            shown: answer.into(),
            sent: answer.into(),
            segments: ai::segments(answer),
            edits: vec![EditState::Pending],
        });
        assert_eq!(p.apply_all(0, &mut ed), 1);
        assert_eq!(ed.buf.text(), "fn a() {\n    2\n}\n");
        assert_eq!(p.turns[0].edits[0], EditState::Applied);
        ed.buf.undo();
        assert_eq!(ed.buf.text(), "fn a() {\n    1\n}\n");
        // Read-only buffers refuse.
        p.turns[0].edits[0] = EditState::Pending;
        ed.read_only = true;
        assert_eq!(p.apply_all(0, &mut ed), 0);
    }
}
