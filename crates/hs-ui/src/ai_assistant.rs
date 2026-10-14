//! Reusable review-only AI chat panel for non-editor tool surfaces.

use std::sync::Arc;

use hs_core::Services;
use hs_core::ai::{self, Message, Role};
use hs_core::secrets::SecretRef;
use hs_core::settings::AiProvider;
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

/// Tool-agnostic assistant. It discusses explicitly supplied context but
/// never edits a buffer or invokes host tool actions.
#[derive(Default)]
pub struct AssistantPanel {
    pub open: bool,
    tool: String,
    context: String,
    context_label: String,
    provider: String,
    model: String,
    input: String,
    turns: Vec<(Role, String)>,
    running: Option<Running>,
    focus_input: bool,
}

impl AssistantPanel {
    pub fn toggle(&mut self, tool: &str) {
        self.open = !self.open;
        self.tool = tool.to_string();
        self.focus_input = self.open;
    }

    /// Set context for the next explicit user request. This method never
    /// sends data to a provider.
    pub fn set_context(&mut self, label: impl Into<String>, context: impl Into<String>) {
        self.context_label = label.into();
        self.context = context.into();
    }

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
        let key = match Self::provider_key(s, &provider) {
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
        let mut messages = vec![Message::new(
            Role::System,
            ai::assistant_system_prompt(if self.tool.is_empty() {
                "tool"
            } else {
                &self.tool
            }),
        )];
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

    pub fn ui(&mut self, ui: &mut egui::Ui, s: &Services, theme: &crate::theme::Theme, tool: &str) {
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                if ui
                    .small_button(crate::icons::ph::X)
                    .on_hover_text(s.t("ai-close"))
                    .clicked()
                {
                    self.open = false;
                }
            });
        });
        if providers.is_empty() {
            ui.add_space(8.0);
            crate::form::note(ui, theme, crate::icons::INFO, &s.t("ai-setup-hint"));
            return;
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
                });
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
                            } else {
                                ui.add(egui::Label::new(text).wrap());
                            }
                        });
                    ui.add_space(6.0);
                }
            });
    }
}

impl Drop for AssistantPanel {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.abort.abort();
        }
    }
}
