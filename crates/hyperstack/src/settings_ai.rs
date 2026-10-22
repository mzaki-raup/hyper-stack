//! Settings → AI Assistant (ADR-019, docs/tools/editor.md §10): the services
//! the assistants may call and the image servers the Graphics activity may
//! draw on. A provider is one of eleven kinds in two capabilities - chat
//! (OpenAI-compatible, Anthropic, Google Gemini, Azure OpenAI, AWS Bedrock,
//! 1min.AI) and image (the Stable Diffusion WebUI API, ComfyUI, SwarmUI,
//! InvokeAI, Fooocus-API; 1min.AI does both) - and adding one starts from
//! a gallery of known services rather than from an empty form. Keys go
//! into the vault, never into `settings.toml`; an image server's workflow
//! settings (checkpoint, sampling, upscaler, ComfyUI workflows) live on
//! the provider.

use std::sync::Arc;

use hs_core::Services;
use hs_core::ai::{self, Capability, Provider, UpscaleMode};
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
    /// The gallery is open instead of a form.
    picking: bool,
}

struct Draft {
    provider: AiProvider,
    /// A newly typed key; empty keeps the stored one.
    key: String,
    /// A newly typed AWS secret access key (Bedrock).
    secret: String,
    models: Option<Probe>,
    test: Option<Probe>,
}

impl Draft {
    fn new(provider: AiProvider) -> Self {
        Self {
            provider,
            key: String::new(),
            secret: String::new(),
            models: None,
            test: None,
        }
    }
}

/// A service the gallery offers. `name` is a brand, so it is not
/// translated; `blurb` is a Fluent key. An empty `base` or `model` means
/// the kind's own default.
struct Service {
    name: &'static str,
    kind: Provider,
    base: &'static str,
    model: &'static str,
    blurb: &'static str,
    icon: &'static str,
}

/// The gallery: a capability, then its groups. Everything OpenAI-shaped
/// shares one kind and differs only in its endpoint; likewise every
/// AUTOMATIC1111-shaped image server.
type Group = (&'static str, &'static [Service]);
const CATALOGUE: &[(Capability, &[Group])] = &[
    (
        Capability::Chat,
        &[
            (
                "ai-group-hosted",
                &[
                    Service {
                        name: "OpenAI",
                        kind: Provider::OpenAi,
                        base: ai::OPENAI_BASE,
                        model: "gpt-4o-mini",
                        blurb: "ai-svc-openai",
                        icon: ph::CIRCLES_THREE,
                    },
                    Service {
                        name: "Anthropic",
                        kind: Provider::Anthropic,
                        base: "",
                        model: "claude-sonnet-4-5",
                        blurb: "ai-svc-anthropic",
                        icon: ph::ASTERISK,
                    },
                    Service {
                        name: "Google Gemini",
                        kind: Provider::Gemini,
                        base: "",
                        model: "gemini-2.5-flash",
                        blurb: "ai-svc-gemini",
                        icon: ph::DIAMOND,
                    },
                    Service {
                        name: "Azure OpenAI",
                        kind: Provider::Azure,
                        base: "",
                        model: "",
                        blurb: "ai-svc-azure",
                        icon: ph::CLOUD,
                    },
                    Service {
                        name: "AWS Bedrock",
                        kind: Provider::Bedrock,
                        base: "",
                        model: "",
                        blurb: "ai-svc-bedrock",
                        icon: ph::CLOUD_ARROW_UP,
                    },
                    Service {
                        name: "Mistral",
                        kind: Provider::OpenAi,
                        base: "https://api.mistral.ai/v1",
                        model: "mistral-large-latest",
                        blurb: "ai-svc-mistral",
                        icon: ph::WIND,
                    },
                    Service {
                        name: "DeepSeek",
                        kind: Provider::OpenAi,
                        base: "https://api.deepseek.com",
                        model: "deepseek-flash",
                        blurb: "ai-svc-deepseek",
                        icon: ph::FISH_SIMPLE,
                    },
                    Service {
                        name: "DeepSeek (Anthropic API)",
                        kind: Provider::Anthropic,
                        base: "https://api.deepseek.com/anthropic",
                        model: "deepseek-flash",
                        blurb: "ai-svc-deepseek-anthropic",
                        icon: ph::FISH_SIMPLE,
                    },
                    Service {
                        name: "Moonshot (Kimi)",
                        kind: Provider::OpenAi,
                        base: "https://api.moonshot.ai/v1",
                        model: "kimi-k2-0905-preview",
                        blurb: "ai-svc-moonshot",
                        icon: ph::MOON,
                    },
                    Service {
                        name: "Tencent Hunyuan",
                        kind: Provider::OpenAi,
                        base: "https://api.hunyuan.cloud.tencent.com/v1",
                        model: "hunyuan-turbos-latest",
                        blurb: "ai-svc-hunyuan",
                        icon: ph::TRANSLATE,
                    },
                    Service {
                        name: "1min.AI",
                        kind: Provider::OneMin,
                        base: "",
                        model: "gpt-4o-mini",
                        blurb: "ai-svc-1min",
                        icon: ph::TIMER,
                    },
                ],
            ),
            (
                "ai-group-gateway",
                &[
                    Service {
                        name: "OpenRouter",
                        kind: Provider::OpenAi,
                        base: "https://openrouter.ai/api/v1",
                        model: "",
                        blurb: "ai-svc-openrouter",
                        icon: ph::ARROWS_SPLIT,
                    },
                    Service {
                        name: "1min.AI (OpenAI-Compatible)",
                        kind: Provider::OpenAi,
                        base: "https://api.1min.ai/openai/v1",
                        model: "gpt-4o-mini",
                        blurb: "ai-svc-1min-openai",
                        icon: ph::TIMER,
                    },
                ],
            ),
            (
                "ai-group-local",
                &[
                    Service {
                        name: "LM Studio",
                        kind: Provider::OpenAi,
                        base: "http://localhost:1234/v1",
                        model: "",
                        blurb: "ai-svc-lmstudio",
                        icon: ph::DESKTOP,
                    },
                    Service {
                        name: "Ollama",
                        kind: Provider::OpenAi,
                        base: "http://localhost:11434/v1",
                        model: "",
                        blurb: "ai-svc-ollama",
                        icon: ph::DESKTOP,
                    },
                    Service {
                        name: "vLLM",
                        kind: Provider::OpenAi,
                        base: "http://localhost:8000/v1",
                        model: "",
                        blurb: "ai-svc-vllm",
                        icon: ph::DESKTOP,
                    },
                    Service {
                        name: "Other OpenAI-Compatible",
                        kind: Provider::OpenAi,
                        base: "",
                        model: "",
                        blurb: "ai-svc-other",
                        icon: ph::PLUGS_CONNECTED,
                    },
                ],
            ),
        ],
    ),
    (
        Capability::Image,
        &[
            (
                "ai-group-image-hosted",
                &[Service {
                    name: "1min.AI (Images)",
                    kind: Provider::OneMin,
                    base: "",
                    model: "gpt-4o-mini",
                    blurb: "ai-svc-1min-images",
                    icon: ph::IMAGE,
                }],
            ),
            (
                "ai-group-image-local",
                &[
                    Service {
                        name: "AUTOMATIC1111",
                        kind: Provider::SdWebUi,
                        base: "http://127.0.0.1:7860",
                        model: "",
                        blurb: "ai-svc-a1111",
                        icon: ph::DESKTOP,
                    },
                    Service {
                        name: "SD.Next",
                        kind: Provider::SdWebUi,
                        base: "http://127.0.0.1:7860",
                        model: "",
                        blurb: "ai-svc-sdnext",
                        icon: ph::DESKTOP,
                    },
                    Service {
                        name: "Forge",
                        kind: Provider::SdWebUi,
                        base: "http://127.0.0.1:7860",
                        model: "",
                        blurb: "ai-svc-forge",
                        icon: ph::DESKTOP,
                    },
                    Service {
                        name: "ComfyUI",
                        kind: Provider::ComfyUi,
                        base: "http://127.0.0.1:8188",
                        model: "",
                        blurb: "ai-svc-comfyui",
                        icon: ph::GRAPH,
                    },
                    Service {
                        name: "SwarmUI",
                        kind: Provider::SwarmUi,
                        base: "http://127.0.0.1:7801",
                        model: "",
                        blurb: "ai-svc-swarmui",
                        icon: ph::DESKTOP,
                    },
                    Service {
                        name: "SwarmUI (ComfyUI Backend)",
                        kind: Provider::ComfyUi,
                        base: "http://127.0.0.1:7801/ComfyBackendDirect",
                        model: "",
                        blurb: "ai-svc-swarmui-comfy",
                        icon: ph::GRAPH,
                    },
                    Service {
                        name: "InvokeAI",
                        kind: Provider::InvokeAi,
                        base: "http://127.0.0.1:9090",
                        model: "",
                        blurb: "ai-svc-invokeai",
                        icon: ph::DESKTOP,
                    },
                    Service {
                        name: "Fooocus (Fooocus-API)",
                        kind: Provider::Fooocus,
                        base: "http://127.0.0.1:8888",
                        model: "",
                        blurb: "ai-svc-fooocus",
                        icon: ph::DESKTOP,
                    },
                    Service {
                        name: "Stability Matrix (WebUI Package)",
                        kind: Provider::SdWebUi,
                        base: "http://127.0.0.1:7860",
                        model: "",
                        blurb: "ai-svc-stability-matrix-webui",
                        icon: ph::SQUARES_FOUR,
                    },
                    Service {
                        name: "Stability Matrix (ComfyUI Package)",
                        kind: Provider::ComfyUi,
                        base: "http://127.0.0.1:8188",
                        model: "",
                        blurb: "ai-svc-stability-matrix-comfy",
                        icon: ph::SQUARES_FOUR,
                    },
                    Service {
                        name: "Other WebUI API",
                        kind: Provider::SdWebUi,
                        base: "",
                        model: "",
                        blurb: "ai-svc-other-webui",
                        icon: ph::PLUGS_CONNECTED,
                    },
                ],
            ),
        ],
    ),
];

fn kind_label(s: &Services, k: Provider) -> String {
    match k {
        Provider::OneMin => s.t("ai-kind-1min"),
        Provider::OpenAi => s.t("ai-kind-openai"),
        Provider::Anthropic => s.t("ai-kind-anthropic"),
        Provider::Gemini => s.t("ai-kind-gemini"),
        Provider::Azure => s.t("ai-kind-azure"),
        Provider::Bedrock => s.t("ai-kind-bedrock"),
        Provider::SdWebUi => s.t("ai-kind-sdwebui"),
        Provider::ComfyUi => s.t("ai-kind-comfyui"),
        Provider::SwarmUi => s.t("ai-kind-swarmui"),
        Provider::InvokeAi => s.t("ai-kind-invokeai"),
        Provider::Fooocus => s.t("ai-kind-fooocus"),
    }
}

fn capability_label(s: &Services, c: Capability) -> String {
    match c {
        Capability::Chat => s.t("ai-cap-chat"),
        Capability::Image => s.t("ai-cap-image"),
    }
}

fn capability_icon(c: Capability) -> &'static str {
    match c {
        Capability::Chat => ph::CHAT_CIRCLE_TEXT,
        Capability::Image => ph::IMAGE,
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
            changed |=
                form::slider(ui, theme, egui::Slider::new(&mut st.ai.max_rounds, 1..=12)).changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("ai-max-rounds-hint"));
    });
    form::note(ui, theme, hs_ui::icons::INFO, &s.t("ai-privacy-note"));
    ui.add_space(8.0);
    // The right column needs a real width: inside a horizontal layout
    // `available_width` is unbounded, and a wrapped row would never wrap.
    let list_width = 230.0;
    let right_width = (ui.available_width() - list_width - 16.0).max(320.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(list_width);
            if form::primary_button(ui, theme, &format!("{} {}", ph::PLUS, s.t("ai-add"))).clicked()
            {
                state.picking = true;
                state.draft = None;
                state.selected = None;
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
            // One list per capability, so a chat service and an image
            // server never read as interchangeable.
            for cap in [Capability::Chat, Capability::Image] {
                let rows: Vec<&AiProvider> =
                    list.iter().filter(|p| p.kind.capability() == cap).collect();
                if rows.is_empty() {
                    continue;
                }
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} {}",
                        capability_icon(cap),
                        capability_label(s, cap)
                    ))
                    .small()
                    .strong()
                    .color(theme.text_muted),
                );
                ui.add_space(2.0);
                for p in rows {
                    let star = if default_id.as_deref() == Some(&p.id) {
                        "★ "
                    } else {
                        ""
                    };
                    let what = match cap {
                        Capability::Chat if !p.model.is_empty() => p.model.clone(),
                        Capability::Image if !p.image.checkpoint.is_empty() => {
                            p.image.checkpoint.clone()
                        }
                        _ => kind_label(s, p.kind),
                    };
                    let mut sub = format!("{star}{what}");
                    if sub.chars().count() > 30 {
                        sub = format!("{}…", sub.chars().take(29).collect::<String>());
                    }
                    if form::list_row(
                        ui,
                        theme,
                        state.selected.as_deref() == Some(&p.id),
                        if cap == Capability::Chat {
                            ph::SPARKLE
                        } else {
                            ph::IMAGE
                        },
                        &p.name,
                        Some(&sub),
                    )
                    .clicked()
                    {
                        state.picking = false;
                        state.selected = Some(p.id.clone());
                        state.draft = Some(Draft::new(p.clone()));
                    }
                    ui.add_space(2.0);
                }
            }
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.set_width(right_width);
            if state.picking {
                if let Some(p) = gallery(ui, theme, s) {
                    state.picking = false;
                    state.selected = Some(p.id.clone());
                    state.draft = Some(Draft::new(p));
                }
                return;
            }
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

/// The "which service?" gallery. Returns the provider to edit when one is
/// chosen.
fn gallery(ui: &mut egui::Ui, theme: &Theme, s: &Services) -> Option<AiProvider> {
    let mut chosen = None;
    form::section(ui, theme, &s.t("ai-add-title"), |ui| {
        form::hint(ui, theme, &s.t("ai-add-hint"));
        for (cap, groups) in CATALOGUE {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(capability_icon(*cap))
                        .size(16.0)
                        .color(theme.accent),
                );
                ui.label(
                    egui::RichText::new(capability_label(s, *cap))
                        .size(14.0)
                        .strong(),
                );
            });
            form::hint(
                ui,
                theme,
                &s.t(match cap {
                    Capability::Chat => "ai-cap-chat-hint",
                    Capability::Image => "ai-cap-image-hint",
                }),
            );
            for (group, services) in *groups {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(s.t(group))
                        .small()
                        .strong()
                        .color(theme.text_muted),
                );
                ui.add_space(4.0);
                // A computed grid, not `horizontal_wrapped`: that wraps on the
                // size a widget reports *before* it is drawn, which a frame
                // cannot do, so a row of cards would run past the panel.
                let spacing = 8.0;
                let columns = (((ui.available_width() + spacing) / (CARD_WIDTH + spacing)).floor()
                    as usize)
                    .max(1);
                for row in services.chunks(columns) {
                    // Top-aligned: `horizontal` centres its items vertically,
                    // which would stagger cards whose text wraps differently.
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = spacing;
                        for svc in row {
                            if service_card(ui, theme, s, svc).clicked() {
                                chosen = Some(AiProvider {
                                    id: hs_core::new_id(),
                                    name: svc.name.to_string(),
                                    kind: svc.kind,
                                    base_url: svc.base.to_string(),
                                    model: svc.model.to_string(),
                                    ..Default::default()
                                });
                            }
                        }
                    });
                    ui.add_space(spacing);
                }
            }
            ui.add_space(4.0);
            ui.separator();
        }
    });
    chosen
}

/// How wide one gallery card is, margins included.
const CARD_WIDTH: f32 = 236.0;
/// The height of a card's content: a name and two rows of description.
const CARD_HEIGHT: f32 = 68.0;

/// One card in the gallery: icon, brand name and a line on what it is.
fn service_card(ui: &mut egui::Ui, theme: &Theme, s: &Services, svc: &Service) -> egui::Response {
    let resp = egui::Frame::new()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            // The frame's margins are inside CARD_WIDTH, so every card is
            // the same size and the columns line up.
            ui.set_width(CARD_WIDTH - 2.0 * 12.0);
            ui.set_min_height(CARD_HEIGHT);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(svc.icon).size(20.0).color(theme.accent));
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(svc.name).strong());
                    // The blurb is capped at two rows so every card in a row
                    // is the same height however long the sentence is.
                    let mut job = egui::text::LayoutJob::simple(
                        s.t(svc.blurb),
                        egui::FontId::proportional(11.0),
                        theme.text_muted,
                        ui.available_width(),
                    );
                    job.wrap.max_rows = 2;
                    job.wrap.overflow_character = Some('…');
                    ui.add(egui::Label::new(job));
                });
            });
        })
        .response;
    let resp = resp.interact(egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_stroke(
            resp.rect,
            egui::CornerRadius::same(8),
            egui::Stroke::new(1.0, theme.accent),
            egui::StrokeKind::Outside,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
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
    let kind = d.provider.kind;
    let cap = kind.capability();
    form::section(ui, theme, &s.t("ai-section-provider"), |ui| {
        form::field_grid(ui, "ai-form", |ui| {
            form::label(ui, &s.t("ai-name"));
            form::text_field(ui, &mut d.provider.name, &s.t("ai-name"));
            ui.end_row();
            form::label(ui, &s.t("ai-kind"));
            egui::ComboBox::from_id_salt("ai-kind")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(kind_label(s, d.provider.kind))
                .show_ui(ui, |ui| {
                    for c in [Capability::Chat, Capability::Image] {
                        ui.label(
                            egui::RichText::new(capability_label(s, c))
                                .small()
                                .color(theme.text_muted),
                        );
                        for k in Provider::all().into_iter().filter(|k| k.capability() == c) {
                            ui.selectable_value(&mut d.provider.kind, k, kind_label(s, k));
                        }
                    }
                });
            ui.end_row();
            form::label(
                ui,
                &s.t(if kind.needs_base() {
                    "ai-resource-url"
                } else if kind.self_hosted() {
                    "ai-server-url"
                } else {
                    "ai-base-url"
                }),
            );
            form::text_field(ui, &mut d.provider.base_url, d.provider.kind.default_base());
            ui.end_row();
            if kind.needs_region() {
                form::label(ui, &s.t("ai-region"));
                form::text_field(ui, &mut d.provider.region, "us-east-1");
                ui.end_row();
            }
            if kind.needs_api_version() {
                form::label(ui, &s.t("ai-api-version"));
                form::text_field(ui, &mut d.provider.api_version, ai::AZURE_API_VERSION);
                ui.end_row();
            }
            form::label(
                ui,
                &s.t(if kind.needs_secret() {
                    "ai-access-key"
                } else if kind.self_hosted() {
                    "ai-credential"
                } else {
                    "ai-api-key"
                }),
            );
            let stored = SecretRef::parse(&d.provider.api_key).is_some();
            let hint = if stored {
                s.t("ai-api-key-stored")
            } else {
                s.t(match kind {
                    Provider::SdWebUi => "ai-credential-hint-webui",
                    Provider::SwarmUi => "ai-credential-hint-swarm",
                    k if k.self_hosted() => "ai-credential-hint-local",
                    _ => "ai-api-key-hint",
                })
            };
            form::secret_field(ui, &mut d.key, &hint, form::FIELD_MIN_WIDTH);
            ui.end_row();
            if kind.needs_secret() {
                form::label(ui, &s.t("ai-secret-key"));
                let stored = SecretRef::parse(&d.provider.secret_key).is_some();
                let hint = if stored {
                    s.t("ai-api-key-stored")
                } else {
                    s.t("ai-secret-key-hint")
                };
                form::secret_field(ui, &mut d.secret, &hint, form::FIELD_MIN_WIDTH);
                ui.end_row();
            }
            if cap == Capability::Chat {
                form::label(
                    ui,
                    &s.t(if kind == Provider::Azure {
                        "ai-deployment"
                    } else {
                        "ai-model"
                    }),
                );
                model_field(ui, d, "ai-models", |p| &mut p.model, kind.default_model());
                ui.end_row();
            }
        });
        if cap == Capability::Chat {
            form::hint(
                ui,
                theme,
                &s.t(match kind {
                    Provider::OneMin => "ai-model-hint-1min",
                    Provider::Anthropic => "ai-model-hint-anthropic",
                    Provider::Gemini => "ai-model-hint-gemini",
                    Provider::Azure => "ai-model-hint-azure",
                    Provider::Bedrock => "ai-model-hint-bedrock",
                    _ => "ai-model-hint-openai",
                }),
            );
        } else {
            form::hint(
                ui,
                theme,
                &s.t(match kind {
                    Provider::SdWebUi => "ai-server-hint-webui",
                    Provider::ComfyUi => "ai-server-hint-comfyui",
                    Provider::SwarmUi => "ai-server-hint-swarmui",
                    Provider::InvokeAi => "ai-server-hint-invokeai",
                    _ => "ai-server-hint-fooocus",
                }),
            );
        }
        if kind.whole_answer_only() {
            form::note(ui, theme, hs_ui::icons::INFO, &s.t("ai-no-stream-note"));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let fetch_label = if cap == Capability::Image {
                s.t("ai-fetch-checkpoints")
            } else {
                s.t("ai-fetch-models")
            };
            if ui
                .add_enabled(
                    kind.lists_models(),
                    egui::Button::new(format!("{} {}", ph::LIST, fetch_label)),
                )
                .tip(s.t("ai-fetch-models-tip"))
                .clicked()
            {
                d.models = Some(probe(s, d, false));
            }
            if !kind.lists_models()
                && cap == Capability::Chat
                && ui
                    .button(format!("{} {}", ph::LIST, s.t("ai-known-models")))
                    .tip(s.t("ai-known-models-tip"))
                    .clicked()
            {
                d.models = Some(probe(s, d, false));
            }
            if ui
                .button(format!("{} {}", ph::PLUGS_CONNECTED, s.t("ai-test")))
                .tip(s.t(if cap == Capability::Image {
                    "ai-test-tip-image"
                } else {
                    "ai-test-tip"
                }))
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
    if cap == Capability::Image {
        workflow_section(ui, theme, s, d);
    }
    ui.horizontal(|ui| {
        if form::primary_button(ui, theme, &s.t("save")).clicked() {
            save(s, d, effects, changed);
        }
        let exists = s
            .settings
            .read()
            .ai
            .providers
            .iter()
            .any(|p| p.id == d.provider.id);
        if exists
            && cap == Capability::Chat
            && ui.button(format!("★ {}", s.t("ai-make-default"))).clicked()
        {
            s.settings.write().ai.default_provider = d.provider.id.clone();
            *changed = true;
        }
        if ui.button(s.t("cancel")).clicked() {
            *close = true;
        }
        if exists && form::danger_button(ui, theme, &s.t("ai-remove")).clicked() {
            let mut st = s.settings.write();
            st.ai.providers.retain(|p| p.id != d.provider.id);
            drop(st);
            for field in [&d.provider.api_key, &d.provider.secret_key] {
                if let Some(r) = SecretRef::parse(field) {
                    let _ = s.secrets.delete(&r);
                }
            }
            *changed = true;
            *close = true;
        }
    });
}

/// A text field with the fetched list beside it as a picker.
fn model_field(
    ui: &mut egui::Ui,
    d: &mut Draft,
    id: &str,
    field: impl Fn(&mut AiProvider) -> &mut String,
    placeholder: &str,
) {
    ui.horizontal(|ui| {
        let models = d
            .models
            .as_ref()
            .and_then(|m| m.lock().clone())
            .and_then(Result::ok)
            .unwrap_or_default();
        form::text_field_width(
            ui,
            field(&mut d.provider),
            placeholder,
            form::FIELD_MIN_WIDTH - 34.0,
        );
        if !models.is_empty() {
            egui::ComboBox::from_id_salt(id)
                .width(24.0)
                .selected_text("")
                .show_ui(ui, |ui| {
                    for m in models {
                        ui.selectable_value(field(&mut d.provider), m.clone(), m);
                    }
                });
        }
    });
}

/// The image server's workflow: what every request carries, then the two
/// jobs that are easy to confuse. Upscaling makes more pixels; enhancing
/// makes a better picture at the same size. They take different settings
/// and, on ComfyUI, different workflows. Owner's rule, 2026-10-23.
fn workflow_section(ui: &mut egui::Ui, theme: &Theme, s: &Services, d: &mut Draft) {
    let kind = d.provider.kind;
    form::section(ui, theme, &s.t("ai-section-workflow"), |ui| {
        form::hint(ui, theme, &s.t("ai-workflow-hint"));
        ui.add_space(4.0);
        form::field_grid(ui, "ai-workflow", |ui| {
            form::label(ui, &s.t("ai-checkpoint"));
            model_field(
                ui,
                d,
                "ai-checkpoints",
                |p| &mut p.image.checkpoint,
                &s.t("ai-checkpoint-hint"),
            );
            ui.end_row();
            let w = &mut d.provider.image;
            form::label(ui, &s.t("ai-negative-prompt"));
            ui.add(
                egui::TextEdit::multiline(&mut w.negative_prompt)
                    .desired_rows(2)
                    .desired_width(form::FIELD_MIN_WIDTH)
                    .hint_text(s.t("ai-negative-prompt-hint")),
            );
            ui.end_row();
            form::label(ui, &s.t("ai-steps"));
            ui.add(egui::DragValue::new(&mut w.steps).range(1..=150));
            ui.end_row();
            form::label(ui, &s.t("ai-cfg-scale"));
            ui.add(
                egui::DragValue::new(&mut w.cfg_scale)
                    .range(0.0..=30.0)
                    .speed(0.1)
                    .fixed_decimals(1),
            );
            ui.end_row();
            form::label(ui, &s.t("ai-sampler"));
            form::text_field(
                ui,
                &mut w.sampler,
                &s.t(match kind {
                    Provider::ComfyUi => "ai-sampler-hint-comfy",
                    _ => "ai-sampler-hint",
                }),
            );
            ui.end_row();
            form::label(ui, &s.t("ai-scheduler"));
            form::text_field(ui, &mut w.scheduler, &s.t("ai-scheduler-hint"));
            ui.end_row();
            form::label(ui, &s.t("ai-seed"));
            ui.add(egui::DragValue::new(&mut w.seed).range(-1..=i64::MAX));
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("ai-seed-hint"));
        if kind == Provider::ComfyUi {
            comfy_workflow(
                ui,
                theme,
                s,
                "ai-comfy-generate",
                &mut d.provider.image.comfy_generate,
                hs_core::ai_local::COMFY_GENERATE,
            );
            comfy_workflow(
                ui,
                theme,
                s,
                "ai-comfy-edit",
                &mut d.provider.image.comfy_edit,
                hs_core::ai_local::COMFY_EDIT,
            );
        }
        ui.add_space(8.0);
        let w = &mut d.provider.image;
        ui.label(egui::RichText::new(s.t("ai-extra-json")).strong());
        form::hint(
            ui,
            theme,
            &s.t(match kind {
                Provider::SdWebUi => "ai-extra-json-hint-webui",
                Provider::ComfyUi => "ai-extra-json-hint-comfy",
                Provider::SwarmUi => "ai-extra-json-hint-swarm",
                Provider::InvokeAi => "ai-extra-json-hint-invoke",
                _ => "ai-extra-json-hint-fooocus",
            }),
        );
        form::code_field(ui, &mut w.extra, 3);
        if !w.extra.trim().is_empty()
            && serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&w.extra).is_err()
        {
            ui.colored_label(theme.error, s.t("ai-extra-json-invalid"));
        }
    });

    upscale_section(ui, theme, s, d);
    enhance_section(ui, theme, s, d);
}

/// Upscale: a super-resolution model and how much bigger, as a ratio or a
/// target size, because servers take one shape or the other.
fn upscale_section(ui: &mut egui::Ui, theme: &Theme, s: &Services, d: &mut Draft) {
    let kind = d.provider.kind;
    form::section(ui, theme, &s.t("ai-section-upscale"), |ui| {
        form::hint(ui, theme, &s.t("ai-upscale-hint"));
        ui.add_space(4.0);
        form::field_grid(ui, "ai-upscale", |ui| {
            let w = &mut d.provider.image;
            // A plain field, not `model_field`: the probe lists
            // checkpoints, and an upscale model is a different catalogue.
            form::label(ui, &s.t("ai-upscaler"));
            form::text_field(
                ui,
                &mut w.upscaler,
                &s.t(match kind {
                    Provider::SdWebUi => "ai-upscaler-hint-webui",
                    Provider::ComfyUi | Provider::InvokeAi => "ai-upscaler-hint-model",
                    _ => "ai-upscaler-hint-none",
                }),
            );
            ui.end_row();
            form::label(ui, &s.t("ai-upscale-mode"));
            form::field_row(ui, |ui| {
                ui.horizontal(|ui| {
                    for (mode, key) in [
                        (UpscaleMode::Ratio, "ai-upscale-mode-ratio"),
                        (UpscaleMode::Dimensions, "ai-upscale-mode-dimensions"),
                    ] {
                        ui.selectable_value(&mut w.upscale_mode, mode, s.t(key));
                    }
                });
            });
            ui.end_row();
            if w.upscale_mode == UpscaleMode::Ratio {
                form::label(ui, &s.t("ai-upscale-factor"));
                ui.add(
                    egui::DragValue::new(&mut w.upscale_factor)
                        .range(1..=8)
                        .suffix("x"),
                );
                ui.end_row();
            } else {
                form::label(ui, &s.t("ai-upscale-width"));
                ui.add(egui::DragValue::new(&mut w.upscale_width).range(0..=16384));
                ui.end_row();
                form::label(ui, &s.t("ai-upscale-height"));
                ui.add(egui::DragValue::new(&mut w.upscale_height).range(0..=16384));
                ui.end_row();
            }
        });
        form::hint(ui, theme, &s.t("ai-upscale-mode-hint"));
        if d.provider.image.upscale_mode == UpscaleMode::Dimensions {
            form::hint(ui, theme, &s.t("ai-upscale-size-hint"));
        }
        if kind == Provider::ComfyUi {
            comfy_workflow(
                ui,
                theme,
                s,
                "ai-comfy-upscale",
                &mut d.provider.image.comfy_upscale,
                hs_core::ai_local::COMFY_UPSCALE,
            );
        }
    });
}

/// Enhance: the same picture, better. No size field here on purpose.
fn enhance_section(ui: &mut egui::Ui, theme: &Theme, s: &Services, d: &mut Draft) {
    let kind = d.provider.kind;
    form::section(ui, theme, &s.t("ai-section-enhance"), |ui| {
        form::hint(ui, theme, &s.t("ai-enhance-hint"));
        ui.add_space(4.0);
        let w = &mut d.provider.image;
        form::field_grid(ui, "ai-enhance", |ui| {
            form::label(ui, &s.t("ai-enhance-denoise"));
            form::slider(
                ui,
                theme,
                egui::Slider::new(&mut w.enhance_denoise, 0.0..=1.0).fixed_decimals(2),
            );
            ui.end_row();
            form::label(ui, &s.t("ai-enhance-steps"));
            ui.add(egui::DragValue::new(&mut w.enhance_steps).range(0..=150));
            ui.end_row();
            form::label(ui, &s.t("ai-enhance-face"));
            form::field_row(ui, |ui| {
                ui.checkbox(&mut w.enhance_face_restore, "");
            });
            ui.end_row();
            if w.enhance_face_restore {
                form::label(ui, &s.t("ai-enhance-face-strength"));
                form::slider(
                    ui,
                    theme,
                    egui::Slider::new(&mut w.enhance_face_strength, 0.0..=1.0).fixed_decimals(2),
                );
                ui.end_row();
            }
        });
        form::hint(ui, theme, &s.t("ai-enhance-denoise-hint"));
        form::hint(ui, theme, &s.t("ai-enhance-steps-hint"));
        form::hint(ui, theme, &s.t("ai-enhance-face-hint"));
        if kind == Provider::ComfyUi {
            comfy_workflow(
                ui,
                theme,
                s,
                "ai-comfy-enhance",
                &mut d.provider.image.comfy_enhance,
                hs_core::ai_local::COMFY_ENHANCE,
            );
        }
    });
}

/// One ComfyUI workflow slot: the built-in one until it is customised,
/// and a reset back to it.
fn comfy_workflow(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    label: &str,
    text: &mut String,
    builtin: &str,
) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(s.t("ai-comfy-workflows")).strong());
        ui.label(
            egui::RichText::new(s.t(label))
                .small()
                .color(theme.text_muted),
        );
        if text.trim().is_empty() {
            ui.label(
                egui::RichText::new(s.t("ai-comfy-builtin"))
                    .small()
                    .color(theme.text_muted),
            );
            if ui.small_button(s.t("ai-comfy-customise")).clicked() {
                *text = builtin.to_string();
            }
        } else if ui.small_button(s.t("ai-comfy-reset")).clicked() {
            text.clear();
        }
    });
    form::hint(ui, theme, &s.t("ai-comfy-workflows-hint"));
    if !text.trim().is_empty() {
        form::code_field(ui, text, 8);
    }
}

/// Store the typed secrets in the vault, then the provider itself. A
/// locked vault stops the save, so a key is never written to settings.
fn save(s: &Services, d: &mut Draft, effects: &mut Vec<SettingsEffect>, changed: &mut bool) {
    let mut key = std::mem::take(&mut d.key);
    let mut secret = std::mem::take(&mut d.secret);
    let name = d.provider.name.clone();
    let nothing_typed = key.is_empty() && secret.is_empty();
    let stored = nothing_typed
        || crate::settings_ui::store_typed_secrets(
            s,
            &format!("AI {name}"),
            [("api key", &mut key), ("secret key", &mut secret)],
            effects,
        );
    if !stored {
        // The vault is locked: keep what was typed so it is not lost.
        d.key = key;
        d.secret = secret;
        return;
    }
    if !key.is_empty() {
        d.provider.api_key = key;
    }
    if !secret.is_empty() {
        d.provider.secret_key = secret;
    }
    let mut st = s.settings.write();
    match st.ai.providers.iter_mut().find(|p| p.id == d.provider.id) {
        Some(p) => *p = d.provider.clone(),
        None => st.ai.providers.push(d.provider.clone()),
    }
    drop(st);
    *changed = true;
    effects.push(SettingsEffect::Toast(
        hs_core::tool::ToastLevel::Success,
        s.t("ai-saved"),
    ));
}

/// Fetch the model list (`test == false`) or send a one-line chat (an
/// image server is pinged instead), on the runtime; typed credentials are
/// used when there are any, else the vault's.
fn probe(s: &Services, d: &Draft, test: bool) -> Probe {
    let out: Probe = Arc::new(Mutex::new(None));
    let typed = d.key.clone();
    let typed_secret = d.secret.clone();
    let provider = d.provider.clone();
    let cfg = d.provider.config("");
    let waker = s.runtime.waker();
    let slot = out.clone();
    // The vault is only consulted for a field that was not typed, so a
    // locked vault does not stop a check with a freshly typed key.
    let wants_stored = (typed.is_empty() && SecretRef::parse(&provider.api_key).is_some())
        || (provider.kind.needs_secret()
            && typed_secret.is_empty()
            && SecretRef::parse(&provider.secret_key).is_some());
    let stored = if wants_stored {
        s.ai_credentials(&provider)
    } else {
        Ok(ai::Credentials::default())
    };
    s.runtime.spawn(async move {
        let creds = match stored {
            Err(e) => Err(e),
            Ok(mut c) => {
                if !typed.is_empty() {
                    c.key = typed;
                }
                if !typed_secret.is_empty() {
                    c.secret = typed_secret;
                }
                Ok(c)
            }
        };
        let r = match creds {
            Err(e) => Err(e),
            Ok(creds) => {
                let http = ai::http_client();
                if test && cfg.provider.self_hosted() {
                    hs_core::ai_local::ping(&http, &cfg, &creds)
                        .await
                        .map(|a| vec![a])
                        .map_err(|e| e.to_string())
                } else if test {
                    let msgs = [ai::Message::new(
                        ai::Role::User,
                        "Reply with the single word OK.",
                    )];
                    ai::chat(&http, &cfg, &creds, &msgs, &mut |_| {})
                        .await
                        .map(|a| vec![a.trim().chars().take(80).collect()])
                        .map_err(|e| e.to_string())
                } else {
                    ai::list_models(&http, &cfg, &creds)
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
