//! Reusable review-only AI chat panel for non-editor tool surfaces.

use std::sync::Arc;

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
    // Below twice the panel width there is no sensible split; the tool
    // keeps the room and the assistant waits for a bigger window.
    if !open || full.width() < PANEL_WIDTH * 2.0 {
        let child = ui.new_child(egui::UiBuilder::new().max_rect(full).layout(*ui.layout()));
        return (child, None);
    }
    let panel = egui::Rect::from_min_max(
        egui::pos2(full.right() - PANEL_WIDTH, full.top()),
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
            let message_content = if index + 1 == self.turns.len() && *role == Role::User {
                user_content.clone()
            } else {
                content.clone()
            };
            messages.push(Message::new(*role, message_content));
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
        });
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
        let providers = s.settings.read().ai.providers.clone();
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
                    .on_hover_text(s.t("ai-close"))
                    .clicked()
                {
                    self.open = false;
                }
                if ui
                    .small_button(crate::icons::ph::BROOM)
                    .on_hover_text(s.t("ai-new-chat"))
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
        let mut send = false;
        egui::Panel::bottom(ui.id().with("assistant-input"))
            .resizable(false)
            .show(ui, |ui| {
                if !self.quick_actions.is_empty() && self.running.is_none() {
                    let now = ui.input(|i| i.time);
                    let fresh = self.last_quick_at != Some(now);
                    // The row wraps: a narrow panel must not grow to fit it.
                    ui.scope(|ui| {
                        ui.set_max_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            for a in &self.quick_actions {
                                let b = ui.small_button(&a.label);
                                let b = if a.tip.is_empty() {
                                    b
                                } else {
                                    b.on_hover_text(&a.tip)
                                };
                                if b.clicked() && fresh {
                                    self.last_quick_at = Some(now);
                                    self.input = a.prompt.clone();
                                    send = true;
                                }
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
                let r = ui.add(
                    egui::TextEdit::multiline(&mut self.input)
                        .hint_text(s.t("ai-shared-input-hint"))
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
                    send = true;
                }
                ui.horizontal(|ui| {
                    if self.running.is_some() {
                        ui.spinner();
                        if ui.button(s.t("ai-stop")).clicked()
                            && let Some(r) = self.running.take()
                        {
                            r.abort.abort();
                        }
                    } else if crate::form::primary_button(ui, theme, &s.t("ai-send")).clicked() {
                        send = true;
                    }
                    if let Some(e) =
                        crate::mic::mic_button(ui, theme, s, &self.stt, &mut self.input)
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
            });
        if send && self.running.is_none() {
            self.send(s);
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
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
                        .on_hover_text(s.t("ai-insert-run-block-tip"))
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
                        if button.on_hover_text(tip).clicked() {
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

/// A deliberately small Markdown renderer for assistant prose: headings,
/// bullets, bold, italic and inline code. Fenced code is already split out by
/// `split_fences` (or, for a tool with its own fence-splitting loop, by the
/// caller), so this never has to guess at a fence.
pub fn markdown_text(ui: &mut egui::Ui, text: &str, theme: &crate::theme::Theme) {
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            ui.add_space(3.0);
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("### ") {
            ui.label(egui::RichText::new(heading).strong().size(14.0));
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("## ") {
            ui.label(egui::RichText::new(heading).strong().size(15.0));
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("# ") {
            ui.label(egui::RichText::new(heading).strong().size(16.0));
            continue;
        }
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            ui.horizontal_wrapped(|ui| {
                ui.label("•");
                inline_markdown(ui, item, theme);
            });
            continue;
        }
        inline_markdown(ui, trimmed, theme);
    }
}

fn inline_markdown(ui: &mut egui::Ui, text: &str, theme: &crate::theme::Theme) {
    let mut job = egui::text::LayoutJob::default();
    let base = egui::TextFormat {
        font_id: egui::FontId::proportional(13.5),
        color: theme.text,
        ..Default::default()
    };
    let mut bold = false;
    let mut italic = false;
    let mut code = false;
    let mut rest = text;
    while !rest.is_empty() {
        let next = if rest.starts_with("**") {
            bold = !bold;
            Some(2)
        } else if rest.starts_with('`') {
            code = !code;
            Some(1)
        } else if rest.starts_with('*') {
            italic = !italic;
            Some(1)
        } else {
            None
        };
        if let Some(len) = next {
            rest = &rest[len..];
            continue;
        }
        let end = rest.find(['*', '`']).unwrap_or(rest.len());
        let mut fmt = base.clone();
        fmt.italics = italic;
        fmt.font_id = if code {
            egui::FontId::monospace(12.5)
        } else {
            egui::FontId::proportional(13.5)
        };
        if bold && !code {
            fmt.color = theme.text;
        }
        if code {
            fmt.background = theme.surface;
        }
        job.append(&rest[..end], 0.0, fmt);
        rest = &rest[end..];
    }
    ui.add(egui::Label::new(job).wrap());
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
}
