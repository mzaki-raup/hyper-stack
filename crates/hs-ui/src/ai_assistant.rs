//! Reusable review-only AI chat panel for non-editor tool surfaces.

use std::sync::Arc;

use crate::tip::TipExt;
use hs_core::Services;
use hs_core::ai::{self, Message, Role};
use parking_lot::Mutex;

#[derive(Default)]
struct Stream {
    text: String,
    done: Option<Result<(), String>>,
}

struct Running {
    stream: Arc<Mutex<Stream>>,
    abort: tokio::task::AbortHandle,
    /// When the request went out, so a slow service shows as waiting
    /// rather than as a frozen pane (owner's report, 2026-10-24).
    started: std::time::Instant,
}

/// What the user asked the panel to do with an answer this frame. The
/// panel itself never touches the host tool; the tool applies the block.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct AssistantOutput {
    /// A fenced code block from an answer the user clicked **Insert**,
    /// **Insert and Run** or **Apply** (the host's action language) on.
    pub use_block: Option<String>,
    /// The fence language of that block (`hs-api`, `sql`, …).
    pub use_block_lang: String,
    /// The user clicked **Insert and Run**: the host carries the block
    /// out through its own Run path, with the guards that path has.
    pub run: bool,
}

/// A one-click prompt the host tool offers under the chat (the editor's
/// Explain / Review / Improve). Both strings are already translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickAction {
    pub label: String,
    /// What is sent as the user's message.
    pub prompt: String,
    /// Hover text; empty for none.
    pub tip: String,
}

impl QuickAction {
    /// The prompt buttons an activity shows, as the user has them: the
    /// shipped list, with their renames, rewrites, order and hiding from
    /// Settings → AI Assistant → Prompt Buttons applied
    /// (`hs_core::prompts`, owner's request 2026-10-23).
    ///
    /// A tool calls this instead of writing its own list, so changing a
    /// prompt is a setting rather than an edit to the source.
    pub fn for_activity(s: &Services, activity: &str) -> Vec<Self> {
        let text = |k: &str| s.t(k);
        s.settings
            .read()
            .ai
            .prompts
            .visible(activity, &text)
            .into_iter()
            .map(|r| Self {
                label: r.label,
                prompt: r.prompt,
                tip: String::new(),
            })
            .collect()
    }

    /// As [`QuickAction::for_activity`], with `extra` appended to every
    /// prompt: the Graphics diagram tools add a schema reminder that the
    /// answer has to follow.
    pub fn for_activity_with(s: &Services, activity: &str, extra: &str) -> Vec<Self> {
        Self::for_activity(s, activity)
            .into_iter()
            .map(|mut a| {
                a.prompt.push_str(extra);
                a
            })
            .collect()
    }

    /// A quick action from a label key and a prompt key in `en.ftl`.
    pub fn new(s: &Services, label_key: &str, prompt_key: &str, tip_key: &str) -> Self {
        Self {
            label: s.t(label_key),
            prompt: s.t(prompt_key),
            tip: if tip_key.is_empty() {
                String::new()
            } else {
                s.t(tip_key)
            },
        }
    }
}

/// A piece of an answer: prose, or a fenced code block with its language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Code { lang: String, body: String },
}

/// Split Markdown into prose and fenced code blocks (``` or ~~~ fences).
/// An unterminated fence runs to the end, which is what a streaming answer
/// looks like mid-way.
pub fn split_fences(text: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut prose = String::new();
    let mut code: Option<(String, String, String)> = None; // fence, lang, body
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        match &mut code {
            None => {
                let t = trimmed.trim_start();
                if let Some(fence) = ["```", "~~~"].iter().find(|f| t.starts_with(*f)) {
                    if !prose.trim().is_empty() {
                        out.push(Segment::Text(prose.trim_end().to_string()));
                    }
                    prose.clear();
                    let lang = t[fence.len()..].trim().to_string();
                    code = Some((fence.to_string(), lang, String::new()));
                } else {
                    prose.push_str(line);
                }
            }
            Some((fence, lang, body)) => {
                if trimmed.trim_start().starts_with(fence.as_str()) {
                    out.push(Segment::Code {
                        lang: lang.clone(),
                        body: body.trim_end_matches(['\n', '\r']).to_string(),
                    });
                    code = None;
                } else {
                    body.push_str(line);
                }
            }
        }
    }
    if let Some((_, lang, body)) = code {
        out.push(Segment::Code {
            lang,
            body: body.trim_end_matches(['\n', '\r']).to_string(),
        });
    }
    if !prose.trim().is_empty() {
        out.push(Segment::Text(prose.trim_end().to_string()));
    }
    out
}

/// The one-click prompts a shell surface offers: the terminal and SSH
/// ask the same things of their output.
pub fn shell_actions(s: &Services) -> Vec<QuickAction> {
    // Kept for the activity the caller names; both shells ship the same
    // four prompts but the user may change them apart.
    let _ = &s;
    vec![
        QuickAction::new(s, "ai-qa-explain-output", "ai-qa-explain-output-prompt", ""),
        QuickAction::new(s, "ai-qa-explain-error", "ai-qa-explain-error-prompt", ""),
        QuickAction::new(s, "ai-qa-next-command", "ai-qa-next-command-prompt", ""),
        QuickAction::new(s, "ai-qa-summarise", "ai-qa-summarise-prompt", ""),
    ]
}

/// Tool-agnostic assistant. It discusses explicitly supplied context but
/// never edits a buffer or invokes host tool actions.
#[derive(Default)]
pub struct AssistantPanel {
    pub open: bool,
    /// Whether answers offer an **Insert** button (the host tool has a
    /// place to put a command or query). Copy is always offered.
    pub can_insert: bool,
    /// Label for the host's whole-answer action (Graphics: Update Diagram).
    /// None hides the button.
    pub apply_label: Option<String>,
    /// Label for the append variant (Graphics: Append JSON). None hides it.
    pub append_label: Option<String>,
    /// Whether answers also offer **Insert and Run**: the host puts the
    /// block where it belongs and runs it the way its own Run button
    /// does, guards included (ADR-019).
    pub can_run: bool,
    /// One-click prompts for this tool, set by the host every frame.
    pub quick_actions: Vec<QuickAction>,
    /// A fence language the host tool treats as actions (the API client's
    /// `hs-api`): such blocks get an **Apply** button instead of Insert.
    pub action_lang: Option<String>,
    /// Host-specific instructions appended to the system prompt (an action
    /// schema, what is open). Set every frame; sent with the next message.
    pub extra_system: String,
    /// A picture the host attaches to the next question (the Graphics
    /// picture tab sends the picture itself, not a description of it).
    /// Set by the host every frame; sent with the next user turn.
    pub attachment: Option<hs_core::ai::ImagePart>,
    tool: String,
    context: String,
    context_label: String,
    provider: String,
    model: String,
    input: String,
    turns: Vec<(Role, String)>,
    running: Option<Running>,
    focus_input: bool,
    /// Speech to text for the input box (the microphone toggle).
    stt: hs_core::stt::Stt,
    /// A microphone error, shown under the input for a few seconds.
    notice: Option<(String, std::time::Instant)>,
    /// Frame time of the last quick action. egui lays a panel out twice
    /// when its size changes, and `Response::clicked()` is pointer state
    /// rather than an event, so the second pass sees the same click: an
    /// action that re-fills the input would send itself twice.
    last_quick_at: Option<f64>,
}

/// How wide the assistant's column is.
pub const PANEL_WIDTH: f32 = 390.0;

/// Carve a tool's area into the part it draws in and the column the
/// assistant gets, **before** anything else is laid out.
///
/// The assistant used to be drawn last, into whatever rectangle was left
/// over — which after a results panel had taken its share was often
/// nothing, so running a query made the assistant appear to close itself.
/// It is a column now: the tool's own content is narrower while it is
/// open, and the only thing that closes it is the toggle.
///
/// Returns the `Ui` the tool should use for everything else, and the
/// assistant's rectangle when it is open and there is room for it.
pub fn split_for_assistant(ui: &mut egui::Ui, open: bool) -> (egui::Ui, Option<egui::Rect>) {
    let full = ui.available_rect_before_wrap();
    // The panel gives way before the tool does: it shrinks to keep
    // MIN_CONTENT for the tool and only steps aside when even its own
    // minimum would not fit (a split half in a small window).
    const MIN_CONTENT: f32 = 320.0;
    const MIN_PANEL: f32 = 260.0;
    if !open || full.width() < MIN_CONTENT + MIN_PANEL {
        let child = ui.new_child(egui::UiBuilder::new().max_rect(full).layout(*ui.layout()));
        return (child, None);
    }
    let panel_w = PANEL_WIDTH.min(full.width() - MIN_CONTENT);
    let panel = egui::Rect::from_min_max(
        egui::pos2(full.right() - panel_w, full.top()),
        full.right_bottom(),
    );
    let content = egui::Rect::from_min_max(
        full.left_top(),
        egui::pos2(panel.left() - 6.0, full.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(*ui.layout()),
    );
    child.set_clip_rect(content.intersect(ui.clip_rect()));
    (child, Some(panel))
}

impl AssistantPanel {
    pub fn toggle(&mut self, tool: &str) {
        self.open = !self.open;
        self.tool = tool.to_string();
        self.focus_input = self.open;
        // The host fills these again on the next frame it draws, so a
        // language change is picked up without a per-frame rebuild.
        self.quick_actions.clear();
    }

    /// Set context for the next explicit user request. This method never
    /// sends data to a provider.
    pub fn set_context(&mut self, label: impl Into<String>, context: impl Into<String>) {
        self.context_label = label.into();
        self.context = context.into();
    }

    fn send(&mut self, s: &Services) {
        if self.running.is_some() {
            return;
        }
        let prompt = std::mem::take(&mut self.input).trim().to_string();
        if prompt.is_empty() {
            return;
        }
        let provider = {
            let settings = s.settings.read();
            settings
                .ai
                .providers
                .iter()
                .find(|p| p.id == self.provider)
                .or(settings.ai.chosen())
                .cloned()
        };
        let Some(provider) = provider else {
            self.turns.push((Role::User, prompt));
            self.turns.push((Role::Assistant, s.t("ai-no-provider")));
            return;
        };
        let key = match s.ai_credentials(&provider) {
            Ok(key) => key,
            Err(error) => {
                self.turns.push((Role::User, prompt));
                self.turns.push((Role::Assistant, error));
                return;
            }
        };
        let display_content = prompt.clone();
        let user_content = if self.context.trim().is_empty() {
            prompt.clone()
        } else {
            format!(
                "User-selected context ({}):\n```\n{}\n```\n\n{}",
                self.context_label, self.context, prompt
            )
        };
        self.turns.push((Role::User, display_content));
        let cfg = provider.config(&self.model);
        let mut system = ai::assistant_system_prompt(if self.tool.is_empty() {
            "tool"
        } else {
            &self.tool
        });
        if !self.extra_system.trim().is_empty() {
            system.push_str("\n\n");
            system.push_str(self.extra_system.trim());
        }
        let mut messages = vec![Message::new(Role::System, system)];
        for (index, (role, content)) in self.turns.iter().enumerate() {
            let last = index + 1 == self.turns.len() && *role == Role::User;
            let message_content = if last {
                user_content.clone()
            } else {
                content.clone()
            };
            let mut message = Message::new(*role, message_content);
            // Only when the model can actually read one: a service handed
            // a picture its model does not understand may tokenise the
            // base64 as text and never answer (owner's report,
            // 2026-10-24).
            if last
                && let Some(att) = &self.attachment
                && provider.sends_images(&self.model)
            {
                message = message.with_images(vec![att.clone()]);
            }
            messages.push(message);
        }
        let stream = Arc::new(Mutex::new(Stream::default()));
        let out = stream.clone();
        let waker = s.runtime.waker();
        let task = s.runtime.spawn(async move {
            let http = ai::http_client();
            let sink = out.clone();
            let repaint = waker.clone();
            let result = ai::chat(&http, &cfg, &key, &messages, &mut move |text| {
                sink.lock().text.push_str(text);
                repaint.request_repaint();
            })
            .await;
            out.lock().done = Some(result.map(|_| ()).map_err(|e| e.to_string()));
            waker.request_repaint();
        });
        self.turns.push((Role::Assistant, String::new()));
        self.running = Some(Running {
            stream,
            abort: task.abort_handle(),
            started: std::time::Instant::now(),
        });
    }

    /// Whether an attached picture would actually be sent with the next
    /// question: the provider kind takes pictures and the model can read
    /// one. Used to warn instead of silently dropping it.
    fn attachment_goes_out(&self, s: &Services) -> bool {
        let settings = s.settings.read();
        settings
            .ai
            .providers
            .iter()
            .find(|p| p.id == self.provider)
            .or(settings.ai.chosen())
            .is_some_and(|p| p.sends_images(&self.model))
    }

    /// The assistant's latest complete answer, for read-aloud and copy.
    pub fn last_answer(&self) -> Option<&str> {
        self.turns
            .iter()
            .rev()
            .find(|(role, text)| *role == Role::Assistant && !text.is_empty())
            .map(|(_, text)| text.as_str())
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &crate::theme::Theme,
        tool: &str,
    ) -> AssistantOutput {
        let mut output = AssistantOutput::default();
        self.tool = tool.to_string();
        if let Some(r) = &self.running {
            let (text, done) = {
                let st = r.stream.lock();
                (st.text.clone(), st.done.clone())
            };
            if let Some((Role::Assistant, answer)) = self.turns.last_mut() {
                *answer = text;
            }
            if let Some(done) = done {
                self.running = None;
                if let Err(error) = done
                    && let Some((Role::Assistant, answer)) = self.turns.last_mut()
                {
                    *answer = format!("⚠ {error}");
                }
            }
        }
        let providers = s.settings.read().ai.chat_providers();
        if !providers.iter().any(|p| p.id == self.provider)
            && let Some(p) = s.settings.read().ai.chosen().cloned()
        {
            self.provider = p.id;
            self.model = p.model;
        }
        crate::form::header_bar(ui, theme, |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} {}",
                    crate::icons::ph::SPARKLE,
                    s.t("ai-assistant")
                ))
                .strong(),
            );
            // Right to left, so the close button is added first and sits
            // rightmost: the strip reads "new chat, close" like the editor's.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(crate::icons::ph::X)
                    .tip(s.t("ai-close"))
                    .clicked()
                {
                    self.open = false;
                }
                if ui
                    .small_button(crate::icons::ph::BROOM)
                    .tip(s.t("ai-new-chat"))
                    .clicked()
                {
                    if let Some(r) = self.running.take() {
                        r.abort.abort();
                    }
                    self.turns.clear();
                }
            });
        });
        if providers.is_empty() {
            ui.add_space(8.0);
            crate::form::note(ui, theme, crate::icons::INFO, &s.t("ai-setup-hint"));
            return output;
        }
        ui.horizontal(|ui| {
            let selected = providers
                .iter()
                .find(|p| p.id == self.provider)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            egui::ComboBox::from_id_salt(ui.id().with("assistant-provider"))
                .width(130.0)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for p in &providers {
                        if ui
                            .selectable_label(p.id == self.provider, &p.name)
                            .clicked()
                        {
                            self.provider = p.id.clone();
                            self.model = p.model.clone();
                        }
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.model)
                    .hint_text(s.t("ai-model"))
                    .desired_width(180.0),
            );
        });
        if !self.context.is_empty() {
            egui::CollapsingHeader::new(
                s.i18n
                    .t_args("ai-context-shared-label", &[("label", &self.context_label)]),
            )
            .default_open(false)
            .show(ui, |ui| {
                ui.label(egui::RichText::new(&self.context).monospace().small());
                ui.label(
                    egui::RichText::new(s.t("ai-context-shared-note"))
                        .weak()
                        .small(),
                );
            });
        }
        // The input block is measured first, in an invisible sizing pass at
        // the column's width, so the conversation gets exactly what is left
        // above it. A nested bottom panel sized itself from the previous
        // frame and a bottom-up layout cannot size a wrapping row, so the
        // quick actions painted over the field (owner's screenshots,
        // 2026-10-22, twice).
        let full = ui.available_rect_before_wrap();
        let block_h = {
            let mut measure = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(full)
                    .id_salt("ai-input-measure")
                    .sizing_pass()
                    .invisible(),
            );
            self.input_block(&mut measure, s, theme);
            measure.min_rect().height()
        };
        // A column too short for both (something above it expanded) must
        // still produce two rectangles that are the right way up: an
        // inverted one is drawn at its origin, which puts the composer on
        // top of whatever is there (owner's screenshot, 2026-10-23).
        let block_h = block_h.min(full.height().max(0.0));
        let split = (full.max.y - block_h - 6.0).max(full.min.y);
        let top = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, split));
        let bottom_rect = egui::Rect::from_min_max(
            egui::pos2(full.min.x, (split + 6.0).min(full.max.y)),
            full.max,
        );
        let mut bottom = ui.new_child(egui::UiBuilder::new().max_rect(bottom_rect));
        let send = self.input_block(&mut bottom, s, theme);
        if send && self.running.is_none() {
            self.send(s);
        }
        ui.allocate_rect(full, egui::Sense::hover());
        let mut top_ui = ui.new_child(egui::UiBuilder::new().max_rect(top));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(&mut top_ui, |ui| {
                if self.turns.is_empty() {
                    crate::form::hint(ui, theme, &s.t("ai-shared-empty"));
                }
                for (role, text) in &self.turns {
                    egui::Frame::new()
                        .fill(if *role == Role::User {
                            theme.surface
                        } else {
                            theme.panel_bg
                        })
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(if *role == Role::User {
                                    s.t("ai-you")
                                } else {
                                    s.t("ai-assistant")
                                })
                                .strong()
                                .small()
                                .color(theme.text_muted),
                            );
                            if text.is_empty() && self.running.is_some() {
                                ui.spinner();
                            } else if *role == Role::User {
                                markdown_text(ui, text, theme);
                            } else {
                                for (n, seg) in split_fences(text).into_iter().enumerate() {
                                    match seg {
                                        Segment::Text(t) => {
                                            markdown_text(ui, &t, theme);
                                        }
                                        Segment::Code { lang, body } => {
                                            let is_action = self
                                                .action_lang
                                                .as_deref()
                                                .is_some_and(|l| l.eq_ignore_ascii_case(&lang));
                                            ui.push_id(n, |ui| {
                                                if let Some((action, run)) = code_block(
                                                    ui,
                                                    s,
                                                    theme,
                                                    &lang,
                                                    &body,
                                                    self.can_insert || is_action,
                                                    self.can_run && !is_action,
                                                    is_action,
                                                ) {
                                                    output.use_block = Some(action);
                                                    output.use_block_lang = lang.clone();
                                                    output.run = run;
                                                }
                                            });
                                        }
                                    }
                                }
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button(s.t("ai-copy-answer")).clicked() {
                                        ui.ctx().copy_text(text.clone());
                                    }
                                    if let Some(label) = &self.apply_label
                                        && ui.button(label).clicked()
                                    {
                                        output.use_block = Some(text.clone());
                                        output.use_block_lang = String::new();
                                        output.run = false;
                                    }
                                    if let Some(label) = &self.append_label
                                        && ui.button(label).clicked()
                                    {
                                        output.use_block = Some(text.clone());
                                        output.use_block_lang = String::new();
                                        output.run = true;
                                    }
                                });
                            }
                        });
                    ui.add_space(6.0);
                }
            });
        output
    }

    /// The composer under the conversation: quick actions, the attachment
    /// note, the text field, Send/Stop with the microphone, and a notice.
    /// Returns true when a message should be sent. Also run in a sizing
    /// pass (no interaction, no focus) to measure its height.
    fn input_block(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &crate::theme::Theme,
    ) -> bool {
        let mut send = false;
        let measuring = ui.is_sizing_pass();
        // A tool that attaches a picture (Graphics) should say so when the
        // chosen model cannot read one, rather than answering about a
        // picture it never saw (owner's report, 2026-10-24).
        if self.attachment.is_some() && !self.attachment_goes_out(s) {
            ui.label(
                egui::RichText::new(s.t("ai-picture-not-sent"))
                    .small()
                    .color(theme.warning),
            );
            ui.add_space(2.0);
        }
        if !self.quick_actions.is_empty() && self.running.is_none() {
            let now = ui.input(|i| i.time);
            let fresh = self.last_quick_at != Some(now);
            // The row wraps: a narrow panel must not grow to fit it.
            ui.scope(|ui| {
                ui.set_max_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    for a in &self.quick_actions {
                        let b = ui.small_button(&a.label);
                        let b = if a.tip.is_empty() { b } else { b.tip(&a.tip) };
                        if b.clicked() && fresh && !measuring {
                            self.last_quick_at = Some(now);
                            self.input = a.prompt.clone();
                            send = true;
                        }
                    }
                });
            });
            ui.add_space(4.0);
        }
        if let Some(att) = &self.attachment {
            ui.label(
                egui::RichText::new(format!(
                    "{} {}",
                    crate::icons::ph::PAPERCLIP,
                    s.i18n.t_args(
                        "ai-picture-attached",
                        &[("size", &format!("{} KB", att.data.len() / 1024))]
                    )
                ))
                .small()
                .color(theme.text_muted),
            );
        }
        let r = ui.add(
            egui::TextEdit::multiline(&mut self.input)
                .hint_text(s.t("ai-shared-input-hint"))
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if self.focus_input && !measuring {
            r.request_focus();
            self.focus_input = false;
        }
        if !measuring
            && r.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command)
        {
            send = true;
        }
        ui.horizontal(|ui| {
            // Read the clock before anything may `take()` the request.
            let waiting = self.running.as_ref().map(|r| r.started.elapsed().as_secs());
            if let Some(secs) = waiting {
                ui.spinner();
                // A service that is slow rather than broken looks the same
                // without this; the read timeout only fires on a silent
                // connection, so a long grind has to be visible.
                if secs >= 3 {
                    ui.label(
                        egui::RichText::new(
                            s.i18n.t_args("ai-waiting", &[("secs", &secs.to_string())]),
                        )
                        .small()
                        .color(theme.text_muted),
                    );
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(500));
                }
                if ui.button(s.t("ai-stop")).clicked()
                    && !measuring
                    && let Some(r) = self.running.take()
                {
                    r.abort.abort();
                }
            } else if crate::form::primary_button(ui, theme, &s.t("ai-send")).clicked()
                && !measuring
            {
                send = true;
            }
            if let Some(e) = crate::mic::mic_button(ui, theme, s, &self.stt, &mut self.input)
                && !measuring
            {
                self.notice = Some((e, std::time::Instant::now()));
            }
        });
        if let Some((text, at)) = &self.notice {
            if at.elapsed() > std::time::Duration::from_secs(8) {
                if !measuring {
                    self.notice = None;
                }
            } else {
                ui.add(
                    egui::Label::new(egui::RichText::new(text).color(theme.error).small()).wrap(),
                );
            }
        }
        ui.add_space(4.0);
        send
    }
}

/// A fenced block from an answer: monospace body with Copy and, when the
/// host tool can take it, Insert and (for a tool that runs things)
/// Insert and Run. Returns the body and whether it should be run.
#[allow(clippy::too_many_arguments)]
fn code_block(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &crate::theme::Theme,
    lang: &str,
    body: &str,
    can_insert: bool,
    can_run: bool,
    is_action: bool,
) -> Option<(String, bool)> {
    let mut used = None;
    egui::Frame::new()
        .fill(theme.window_bg)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(if lang.is_empty() { "code" } else { lang })
                        .small()
                        .color(theme.text_muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Rightmost first: Copy · Insert · Insert and Run.
                    if can_run
                        && crate::form::primary_button(
                            ui,
                            theme,
                            &format!("{} {}", crate::icons::ph::PLAY, s.t("ai-insert-run-block")),
                        )
                        .tip(s.t("ai-insert-run-block-tip"))
                        .clicked()
                    {
                        used = Some((body.to_string(), true));
                    }
                    if can_insert {
                        let (icon, label, tip) = if is_action {
                            (
                                crate::icons::ph::LIGHTNING,
                                s.t("ai-apply-block"),
                                s.t("ai-apply-block-tip"),
                            )
                        } else {
                            (
                                crate::icons::ph::ARROW_LINE_DOWN,
                                s.t("ai-insert-block"),
                                s.t("ai-insert-block-tip"),
                            )
                        };
                        let button = if is_action {
                            crate::form::primary_button(ui, theme, &format!("{icon} {label}"))
                        } else {
                            ui.small_button(format!("{icon} {label}"))
                        };
                        if button.tip(tip).clicked() {
                            used = Some((body.to_string(), false));
                        }
                    }
                    if ui
                        .small_button(format!("{} {}", crate::icons::COPY, s.t("ai-copy-block")))
                        .clicked()
                    {
                        ui.ctx().copy_text(body.to_string());
                    }
                });
            });
            ui.add(egui::Label::new(egui::RichText::new(body).monospace()).wrap());
        });
    used
}

impl Drop for AssistantPanel {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.abort.abort();
        }
        self.stt.abort();
    }
}

/// One block of assistant prose, as `parse_blocks` reads it. Fenced code
/// never reaches here (`split_fences` takes it out first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading(u8, String),
    /// Consecutive plain lines joined with spaces, as Markdown reads them.
    Paragraph(String),
    /// `depth` is the nesting level (two spaces or a tab per level);
    /// `check` is a task item's box.
    Bullet {
        depth: usize,
        text: String,
        check: Option<bool>,
    },
    Numbered {
        depth: usize,
        label: String,
        text: String,
    },
    Quote(String),
    Rule,
    Table {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

/// Split a pipe row into its cells; a `\|` stays in the cell.
fn table_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cur.push('|');
            chars.next();
        } else if c == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

/// `| --- | :---: |` under a header row.
fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    if !t.contains('-') || !t.contains('|') && !t.starts_with(':') {
        return false;
    }
    table_cells(t).iter().all(|c| {
        let c = c.trim_start_matches(':').trim_end_matches(':');
        !c.is_empty() && c.chars().all(|ch| ch == '-')
    })
}

fn list_depth(line: &str) -> usize {
    let mut spaces = 0;
    for c in line.chars() {
        match c {
            ' ' => spaces += 1,
            '\t' => spaces += 4,
            _ => break,
        }
    }
    spaces / 2
}

/// A numbered item: `1. text`, `12) text`.
fn numbered(line: &str) -> Option<(String, &str)> {
    let t = line.trim_start();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    let rest = &t[digits.len()..];
    let rest = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))?;
    Some((format!("{digits}."), rest))
}

/// Read assistant prose into blocks: headings, paragraphs (lines joined),
/// nested bullet and numbered lists, task items, quotes, rules and pipe
/// tables.
pub fn parse_blocks(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    let mut i = 0;
    let flush = |para: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !para.is_empty() {
            let joined = para.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
            blocks.push(Block::Paragraph(joined));
            para.clear();
        }
    };
    while i < lines.len() {
        let line = lines[i].trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut para, &mut blocks);
            i += 1;
            continue;
        }
        // A table: a pipe row followed by its separator row.
        if trimmed.contains('|') && i + 1 < lines.len() && is_table_separator(lines[i + 1]) {
            flush(&mut para, &mut blocks);
            let header = table_cells(trimmed);
            let mut rows = Vec::new();
            i += 2;
            while i < lines.len() {
                let r = lines[i].trim();
                if r.is_empty() || !r.contains('|') {
                    break;
                }
                let mut cells = table_cells(r);
                cells.resize(header.len(), String::new());
                rows.push(cells);
                i += 1;
            }
            blocks.push(Block::Table { header, rows });
            continue;
        }
        if trimmed.len() >= 3
            && (trimmed.chars().all(|c| c == '-')
                || trimmed.chars().all(|c| c == '*')
                || trimmed.chars().all(|c| c == '_'))
        {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Rule);
            i += 1;
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Heading(
                hashes as u8,
                trimmed[hashes..]
                    .trim()
                    .trim_end_matches('#')
                    .trim()
                    .to_string(),
            ));
            i += 1;
            continue;
        }
        if let Some(q) = trimmed.strip_prefix('>') {
            flush(&mut para, &mut blocks);
            // Quote lines run together like a paragraph.
            let mut parts = vec![q.trim().to_string()];
            while i + 1 < lines.len() && lines[i + 1].trim().starts_with('>') {
                i += 1;
                parts.push(lines[i].trim()[1..].trim().to_string());
            }
            blocks.push(Block::Quote(parts.join(" ")));
            i += 1;
            continue;
        }
        let depth = list_depth(line);
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            flush(&mut para, &mut blocks);
            let (check, text) = if let Some(t) = item.strip_prefix("[ ] ") {
                (Some(false), t)
            } else if let Some(t) = item
                .strip_prefix("[x] ")
                .or_else(|| item.strip_prefix("[X] "))
            {
                (Some(true), t)
            } else {
                (None, item)
            };
            blocks.push(Block::Bullet {
                depth,
                text: text.to_string(),
                check,
            });
            i += 1;
            continue;
        }
        if let Some((label, rest)) = numbered(line) {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Numbered {
                depth,
                label,
                text: rest.to_string(),
            });
            i += 1;
            continue;
        }
        para.push(line);
        i += 1;
    }
    flush(&mut para, &mut blocks);
    blocks
}

/// A deliberately small Markdown renderer for assistant prose: headings,
/// paragraphs, nested bullet and numbered lists, task items, quotes,
/// rules, pipe tables, and bold / italic / code / links / strikethrough
/// inline. Fenced code is already split out by `split_fences` (or, for a
/// tool with its own fence-splitting loop, by the caller), so this never
/// has to guess at a fence.
pub fn markdown_text(ui: &mut egui::Ui, text: &str, theme: &crate::theme::Theme) {
    let indent = 16.0;
    for block in parse_blocks(text) {
        match block {
            Block::Heading(level, text) => {
                let size = match level {
                    1 => 16.0,
                    2 => 15.0,
                    3 => 14.0,
                    _ => 13.5,
                };
                ui.add_space(2.0);
                ui.label(egui::RichText::new(text).strong().size(size));
            }
            Block::Paragraph(text) => {
                inline_markdown(ui, &text, theme);
                ui.add_space(3.0);
            }
            Block::Bullet { depth, text, check } => {
                ui.horizontal_top(|ui| {
                    ui.add_space(depth as f32 * indent);
                    let marker = match check {
                        Some(true) => "☑",
                        Some(false) => "☐",
                        None if depth % 2 == 1 => "◦",
                        None => "•",
                    };
                    ui.label(marker);
                    inline_markdown(ui, &text, theme);
                });
            }
            Block::Numbered { depth, label, text } => {
                ui.horizontal_top(|ui| {
                    ui.add_space(depth as f32 * indent);
                    ui.label(egui::RichText::new(label).color(theme.text_muted));
                    inline_markdown(ui, &text, theme);
                });
            }
            Block::Quote(text) => {
                ui.horizontal_top(|ui| {
                    let (bar, _) =
                        ui.allocate_exact_size(egui::vec2(3.0, 18.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar, 1.0, theme.accent);
                    ui.add_space(6.0);
                    ui.scope(|ui| {
                        ui.visuals_mut().override_text_color = Some(theme.text_muted);
                        inline_markdown(ui, &text, theme);
                    });
                });
                ui.add_space(3.0);
            }
            Block::Rule => {
                ui.separator();
            }
            Block::Table { header, rows } => {
                table(ui, theme, &header, &rows);
                ui.add_space(4.0);
            }
        }
    }
}

/// A pipe table as a real grid: bold header, a rule under it, striped
/// rows, and a horizontal scroll when the columns outgrow the panel.
fn table(ui: &mut egui::Ui, theme: &crate::theme::Theme, header: &[String], rows: &[Vec<String>]) {
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    // The width the panel actually has, measured before entering the
    // scroll area: inside one, `available_width` is unbounded, so a
    // wrapping cell wraps at an enormous width, no cell ever wraps, and
    // the table runs off the side of the pane with only its left half
    // visible (owner's report, 2026-10-23). Bounding the inner `Ui` makes
    // the cells wrap to the pane instead. The scroll area stays for the
    // case the columns cannot fit even then - a long unbreakable token,
    // or more columns than the minimum width allows.
    let avail = (ui.available_width() - 8.0).max(120.0);
    egui::ScrollArea::horizontal().id_salt(id).show(ui, |ui| {
        ui.set_max_width(avail);
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, theme.border))
            .corner_radius(egui::CornerRadius::same(4))
            .show(ui, |ui| {
                ui.set_max_width(avail - 8.0);
                // A fair share of the panel each, so three columns do not
                // become one wide and two thin.
                let columns = header.len().max(1);
                let fair = ((avail - 24.0 - 14.0 * columns as f32) / columns as f32).max(40.0);
                egui::Grid::new(id.with("grid"))
                    .striped(true)
                    .spacing([14.0, 4.0])
                    .min_col_width(40.0)
                    .max_col_width(fair)
                    .show(ui, |ui| {
                        for cell in header {
                            ui.add(egui::Label::new(inline_job(cell, theme, true)).wrap());
                        }
                        ui.end_row();
                        for row in rows {
                            for cell in row {
                                ui.add(egui::Label::new(inline_job(cell, theme, false)).wrap());
                            }
                            ui.end_row();
                        }
                    });
            });
    });
}

fn inline_markdown(ui: &mut egui::Ui, text: &str, theme: &crate::theme::Theme) {
    ui.add(egui::Label::new(inline_job(text, theme, false)).wrap());
}

/// Inline Markdown as a layout job: `**bold**`, `*italic*`, `` `code` ``,
/// `~~struck~~`, `[text](url)` (shown as the text, underlined, with the
/// address after it in muted type).
fn inline_job(text: &str, theme: &crate::theme::Theme, strong: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let base = egui::TextFormat {
        font_id: egui::FontId::proportional(13.5),
        color: theme.text,
        ..Default::default()
    };
    let mut bold = strong;
    let mut italic = false;
    let mut code = false;
    let mut strike = false;
    let mut rest = text;
    let push = |job: &mut egui::text::LayoutJob,
                s: &str,
                bold: bool,
                italic: bool,
                code: bool,
                strike: bool,
                underline: bool,
                muted: bool| {
        let mut fmt = base.clone();
        fmt.italics = italic;
        fmt.font_id = if code {
            egui::FontId::monospace(12.5)
        } else {
            egui::FontId::proportional(13.5)
        };
        if bold && !code {
            // The proportional face has no bold weight; a size step and the
            // accent read as emphasis instead of "**bold**" rendering plain.
            fmt.font_id = egui::FontId::proportional(14.0);
            fmt.color = theme.accent;
        }
        if code {
            fmt.background = theme.surface;
        }
        if strike {
            fmt.strikethrough = egui::Stroke::new(1.0, theme.text_muted);
        }
        if underline {
            fmt.underline = egui::Stroke::new(1.0, theme.accent);
            fmt.color = theme.accent;
        }
        if muted {
            fmt.color = theme.text_muted;
            fmt.font_id = egui::FontId::proportional(12.0);
        }
        job.append(s, 0.0, fmt);
    };
    while !rest.is_empty() {
        if code {
            // Inside code only the closing tick means anything.
            let end = rest.find('`').unwrap_or(rest.len());
            push(
                &mut job,
                &rest[..end],
                bold,
                italic,
                true,
                strike,
                false,
                false,
            );
            rest = &rest[end..];
            if rest.starts_with('`') {
                code = false;
                rest = &rest[1..];
            }
            continue;
        }
        if let Some(after) = rest.strip_prefix('[')
            && let Some(close) = after.find("](")
            && let Some(end) = after[close + 2..].find(')')
        {
            let label = &after[..close];
            let url = &after[close + 2..close + 2 + end];
            push(&mut job, label, bold, italic, false, strike, true, false);
            if !url.is_empty() && url != label {
                push(
                    &mut job,
                    &format!(" ({url})"),
                    false,
                    false,
                    false,
                    false,
                    false,
                    true,
                );
            }
            rest = &after[close + 2 + end + 1..];
            continue;
        }
        let next = if rest.starts_with("**") || rest.starts_with("__") {
            bold = !bold;
            Some(2)
        } else if rest.starts_with("~~") {
            strike = !strike;
            Some(2)
        } else if rest.starts_with('`') {
            code = true;
            Some(1)
        } else if rest.starts_with('*')
            || (rest.starts_with('_')
                && !rest[1..].starts_with(|c: char| c.is_alphanumeric()) == italic)
        {
            italic = !italic;
            Some(1)
        } else {
            None
        };
        if let Some(len) = next {
            rest = &rest[len..];
            continue;
        }
        let end = rest[1..]
            .find(['*', '`', '[', '~', '_'])
            .map(|n| n + 1)
            .unwrap_or(rest.len());
        push(
            &mut job,
            &rest[..end],
            bold,
            italic,
            false,
            strike,
            false,
            false,
        );
        rest = &rest[end..];
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fences_split_into_prose_and_code_with_language() {
        let text = "Run this:\n```bash\nls -la\necho done\n```\nThen check.\n";
        let segs = split_fences(text);
        assert_eq!(
            segs,
            vec![
                Segment::Text("Run this:".into()),
                Segment::Code {
                    lang: "bash".into(),
                    body: "ls -la\necho done".into()
                },
                Segment::Text("Then check.".into()),
            ]
        );
    }

    #[test]
    fn unterminated_fence_is_a_code_block_while_streaming() {
        let segs = split_fences("```sql\nSELECT 1");
        assert_eq!(
            segs,
            vec![Segment::Code {
                lang: "sql".into(),
                body: "SELECT 1".into()
            }]
        );
        assert_eq!(split_fences("plain"), vec![Segment::Text("plain".into())]);
    }

    #[test]
    fn pipe_tables_become_table_blocks() {
        let text =
            "Summary:\n\n| Name | Size |\n|------|-----:|\n| a \\| b | 1 |\n| c | 2 |\n\nDone.";
        let blocks = parse_blocks(text);
        assert_eq!(blocks[0], Block::Paragraph("Summary:".into()));
        assert_eq!(
            blocks[1],
            Block::Table {
                header: vec!["Name".into(), "Size".into()],
                rows: vec![
                    vec!["a | b".into(), "1".into()],
                    vec!["c".into(), "2".into()]
                ],
            }
        );
        assert_eq!(blocks[2], Block::Paragraph("Done.".into()));
    }

    #[test]
    fn lists_quotes_rules_and_paragraph_joins() {
        let text = "Line one\nline two\n\n- top\n  - nested\n- [x] done\n1. first\n2) second\n> a quote\n> continues\n---\n#### Small";
        let blocks = parse_blocks(text);
        assert_eq!(blocks[0], Block::Paragraph("Line one line two".into()));
        assert_eq!(
            blocks[1],
            Block::Bullet {
                depth: 0,
                text: "top".into(),
                check: None
            }
        );
        assert_eq!(
            blocks[2],
            Block::Bullet {
                depth: 1,
                text: "nested".into(),
                check: None
            }
        );
        assert_eq!(
            blocks[3],
            Block::Bullet {
                depth: 0,
                text: "done".into(),
                check: Some(true)
            }
        );
        assert_eq!(
            blocks[4],
            Block::Numbered {
                depth: 0,
                label: "1.".into(),
                text: "first".into()
            }
        );
        assert_eq!(
            blocks[5],
            Block::Numbered {
                depth: 0,
                label: "2.".into(),
                text: "second".into()
            }
        );
        assert_eq!(blocks[6], Block::Quote("a quote continues".into()));
        assert_eq!(blocks[7], Block::Rule);
        assert_eq!(blocks[8], Block::Heading(4, "Small".into()));
    }

    #[test]
    fn a_lone_pipe_line_is_prose_not_a_table() {
        let blocks = parse_blocks("use a | b here\nnext");
        assert_eq!(blocks, vec![Block::Paragraph("use a | b here next".into())]);
    }
}
