//! Settings → AI Assistant (ADR-019, docs/tools/editor.md §10): the services
//! the editor's assistant may call. Each provider is 1min.AI or any
//! OpenAI-compatible endpoint (OpenAI, LM Studio, Ollama, OpenRouter, …);
//! API keys go into the vault, never into `settings.toml`.

use std::sync::Arc;

use hs_core::Services;
use hs_core::ai::{self, Provider};
use hs_core::secrets::SecretRef;
use hs_core::settings::AiProvider;
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;

use crate::settings_ui::SettingsEffect;

/// A background check's outcome: `None` while it runs.
type Probe = Arc<Mutex<Option<Result<Vec<String>, String>>>>;

#[derive(Default)]
pub struct AiPageState {
    selected: Option<String>,
    draft: Option<Draft>,
}

struct Draft {
    provider: AiProvider,
    /// A newly typed key; empty keeps the stored one.
    key: String,
    models: Option<Probe>,
    test: Option<Probe>,
}

impl Draft {
    fn new(provider: AiProvider) -> Self {
        Self {
            provider,
            key: String::new(),
            models: None,
            test: None,
        }
    }
}

/// Presets for the endpoint field: (label, kind, base URL, needs a key).
const PRESETS: &[(&str, Provider, &str)] = &[
    ("1min.AI", Provider::OneMin, ""),
    ("OpenAI", Provider::OpenAi, ai::OPENAI_BASE),
    ("LM Studio", Provider::OpenAi, "http://localhost:1234/v1"),
    ("Ollama", Provider::OpenAi, "http://localhost:11434/v1"),
    (
        "OpenRouter",
        Provider::OpenAi,
        "https://openrouter.ai/api/v1",
    ),
];

fn kind_label(s: &Services, k: Provider) -> String {
    match k {
        Provider::OneMin => s.t("ai-kind-1min"),
        Provider::OpenAi => s.t("ai-kind-openai"),
    }
}

pub fn page(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut AiPageState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-ai"),
        Some(&s.t("settings-ai-sub")),
    );
    let mut changed = false;
    form::section(ui, theme, &s.t("ai-section-cascade"), |ui| {
        let mut st = s.settings.write();
        form::field_grid(ui, "ai-general", |ui| {
            form::label(ui, &s.t("ai-max-rounds"));
            changed |= ui
                .add(egui::Slider::new(&mut st.ai.max_rounds, 1..=12))
                .changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("ai-max-rounds-hint"));
    });
    form::note(ui, theme, hs_ui::icons::INFO, &s.t("ai-privacy-note"));
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(240.0);
            if form::primary_button(ui, theme, &format!("{} {}", ph::PLUS, s.t("ai-add"))).clicked()
            {
                let p = AiProvider {
                    id: hs_core::new_id(),
                    name: "1min.AI".into(),
                    kind: Provider::OneMin,
                    model: Provider::OneMin.default_model().into(),
                    ..Default::default()
                };
                state.selected = Some(p.id.clone());
                state.draft = Some(Draft::new(p));
            }
            ui.add_space(8.0);
            let (list, default_id) = {
                let st = s.settings.read();
                (
                    st.ai.providers.clone(),
                    st.ai.chosen().map(|p| p.id.clone()),
                )
            };
            if list.is_empty() {
                form::hint(ui, theme, &s.t("ai-none"));
            }
            for p in list {
                let star = if default_id.as_deref() == Some(&p.id) {
                    "★ "
                } else {
                    ""
                };
                let what = if p.model.is_empty() {
                    kind_label(s, p.kind)
                } else {
                    p.model.clone()
                };
                let mut sub = format!("{star}{what}");
                if sub.chars().count() > 30 {
                    sub = format!("{}…", sub.chars().take(29).collect::<String>());
                }
                if form::list_row(
                    ui,
                    theme,
                    state.selected.as_deref() == Some(&p.id),
                    ph::SPARKLE,
                    &p.name,
                    Some(&sub),
                )
                .clicked()
                {
                    state.selected = Some(p.id.clone());
                    state.draft = Some(Draft::new(p));
                }
                ui.add_space(2.0);
            }
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            let Some(d) = &mut state.draft else {
                form::note(ui, theme, hs_ui::icons::INFO, &s.t("ai-select-hint"));
                return;
            };
            let mut close = false;
            form_body(ui, theme, s, d, effects, &mut changed, &mut close);
            if close {
                state.draft = None;
                state.selected = None;
            }
        });
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

fn form_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    d: &mut Draft,
    effects: &mut Vec<SettingsEffect>,
    changed: &mut bool,
    close: &mut bool,
) {
    form::section(ui, theme, &s.t("ai-section-provider"), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(s.t("ai-presets")).color(theme.text_muted));
            for (label, kind, base) in PRESETS {
                if ui.small_button(*label).clicked() {
                    d.provider.kind = *kind;
                    d.provider.base_url = base.to_string();
                    d.provider.name = label.to_string();
                    if d.provider.model.is_empty() || *kind == Provider::OneMin {
                        d.provider.model = kind.default_model().into();
                    }
                    d.models = None;
                }
            }
        });
        ui.add_space(4.0);
        form::field_grid(ui, "ai-form", |ui| {
            form::label(ui, &s.t("ai-name"));
            form::text_field(ui, &mut d.provider.name, &s.t("ai-name"));
            ui.end_row();
            form::label(ui, &s.t("ai-kind"));
            egui::ComboBox::from_id_salt("ai-kind")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(kind_label(s, d.provider.kind))
                .show_ui(ui, |ui| {
                    for k in Provider::all() {
                        ui.selectable_value(&mut d.provider.kind, k, kind_label(s, k));
                    }
                });
            ui.end_row();
            form::label(ui, &s.t("ai-base-url"));
            form::text_field(ui, &mut d.provider.base_url, d.provider.kind.default_base());
            ui.end_row();
            form::label(ui, &s.t("ai-api-key"));
            let stored = SecretRef::parse(&d.provider.api_key).is_some();
            let hint = if stored {
                s.t("ai-api-key-stored")
            } else {
                s.t("ai-api-key-hint")
            };
            form::secret_field(ui, &mut d.key, &hint, form::FIELD_MIN_WIDTH);
            ui.end_row();
            form::label(ui, &s.t("ai-model"));
            ui.horizontal(|ui| {
                let models = d
                    .models
                    .as_ref()
                    .and_then(|m| m.lock().clone())
                    .and_then(Result::ok)
                    .unwrap_or_default();
                form::text_field_width(
                    ui,
                    &mut d.provider.model,
                    d.provider.kind.default_model(),
                    form::FIELD_MIN_WIDTH - 34.0,
                );
                if !models.is_empty() {
                    egui::ComboBox::from_id_salt("ai-models")
                        .width(24.0)
                        .selected_text("")
                        .show_ui(ui, |ui| {
                            for m in models {
                                ui.selectable_value(&mut d.provider.model, m.clone(), m);
                            }
                        });
                }
            });
            ui.end_row();
        });
        form::hint(
            ui,
            theme,
            &s.t(match d.provider.kind {
                Provider::OneMin => "ai-model-hint-1min",
                Provider::OpenAi => "ai-model-hint-openai",
            }),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} {}", ph::LIST, s.t("ai-fetch-models")))
                .tip(s.t("ai-fetch-models-tip"))
                .clicked()
            {
                d.models = Some(probe(s, d, false));
            }
            if ui
                .button(format!("{} {}", ph::PLUGS_CONNECTED, s.t("ai-test")))
                .tip(s.t("ai-test-tip"))
                .clicked()
            {
                d.test = Some(probe(s, d, true));
            }
        });
        for (p, ok_key) in [(&d.models, "ai-models-found"), (&d.test, "ai-test-ok")] {
            let Some(p) = p else { continue };
            match &*p.lock() {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(s.t("ai-checking"));
                    });
                }
                Some(Ok(v)) => {
                    let text = if ok_key == "ai-test-ok" {
                        s.i18n
                            .t_args(ok_key, &[("answer", v.first().map_or("", |x| x))])
                    } else {
                        s.i18n.t_args(ok_key, &[("count", &v.len().to_string())])
                    };
                    ui.colored_label(theme.success, text);
                }
                Some(Err(e)) => {
                    ui.colored_label(theme.error, e);
                }
            }
        }
    });
    ui.horizontal(|ui| {
        if form::primary_button(ui, theme, &s.t("save")).clicked() {
            let mut key = std::mem::take(&mut d.key);
            let name = d.provider.name.clone();
            if key.is_empty()
                || crate::settings_ui::store_typed_secrets(
                    s,
                    &format!("AI {name}"),
                    [("api key", &mut key)],
                    effects,
                )
            {
                if !key.is_empty() {
                    d.provider.api_key = key;
                }
                let mut st = s.settings.write();
                match st.ai.providers.iter_mut().find(|p| p.id == d.provider.id) {
                    Some(p) => *p = d.provider.clone(),
                    None => st.ai.providers.push(d.provider.clone()),
                }
                *changed = true;
                effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Success,
                    s.t("ai-saved"),
                ));
            } else {
                d.key = key;
            }
        }
        let exists = s
            .settings
            .read()
            .ai
            .providers
            .iter()
            .any(|p| p.id == d.provider.id);
        if exists && ui.button(format!("★ {}", s.t("ai-make-default"))).clicked() {
            s.settings.write().ai.default_provider = d.provider.id.clone();
            *changed = true;
        }
        if ui.button(s.t("cancel")).clicked() {
            *close = true;
        }
        if exists && form::danger_button(ui, theme, &s.t("ai-remove")).clicked() {
            let mut st = s.settings.write();
            st.ai.providers.retain(|p| p.id != d.provider.id);
            if let Some(r) = SecretRef::parse(&d.provider.api_key) {
                let _ = s.secrets.delete(&r);
            }
            *changed = true;
            *close = true;
        }
    });
}

/// Fetch the model list (`test == false`) or send a one-line chat, on the
/// runtime; the typed key is used when there is one, else the vault's.
fn probe(s: &Services, d: &Draft, test: bool) -> Probe {
    let out: Probe = Arc::new(Mutex::new(None));
    let typed = d.key.clone();
    let stored = SecretRef::parse(&d.provider.api_key);
    let secrets = s.secrets.clone();
    let cfg = d.provider.config("");
    let unlock = s.t("settings-vault-unlock-first");
    let waker = s.runtime.waker();
    let slot = out.clone();
    s.runtime.spawn(async move {
        let key = if !typed.is_empty() {
            Ok(typed)
        } else if let Some(r) = stored {
            if secrets.is_locked() {
                Err(unlock)
            } else {
                secrets
                    .get(&r)
                    .map(|v| secrecy::ExposeSecret::expose_secret(&v).to_string())
                    .map_err(|e| e.to_string())
            }
        } else {
            Ok(String::new())
        };
        let r = match key {
            Err(e) => Err(e),
            Ok(key) => {
                let http = ai::http_client();
                if test {
                    let msgs = [ai::Message::new(
                        ai::Role::User,
                        "Reply with the single word OK.",
                    )];
                    ai::chat(&http, &cfg, &key, &msgs, &mut |_| {})
                        .await
                        .map(|a| vec![a.trim().chars().take(80).collect()])
                        .map_err(|e| e.to_string())
                } else {
                    ai::list_models(&http, &cfg, &key)
                        .await
                        .map_err(|e| e.to_string())
                }
            }
        };
        *slot.lock() = Some(r);
        waker.request_repaint();
    });
    out
}
