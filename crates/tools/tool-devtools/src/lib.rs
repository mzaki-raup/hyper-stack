//! HyperStack devtools (docs/tools/devtools.md): small utilities developers
//! otherwise do on random websites with production secrets. Everything runs
//! locally; the only network calls are the two Firebase token exchanges,
//! each behind its own button. One tab shows one utility; the sidebar lists
//! them by category and a tab can switch between them.

pub mod base64_tool;
pub mod crypto;
pub mod firebase;
pub mod hash;
pub mod jasypt;
pub mod jwt;
pub mod pki;
mod util;

use std::borrow::Cow;

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, Speakable, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "devtools",
    title_key: "tool-devtools",
    icon: ph::TOOLBOX,
    category: ToolCategory::Utilities,
    accepts: &[Accept::Text],
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Utility {
    #[default]
    Base64,
    Hash,
    Jwt,
    Firebase,
    Jasypt,
    Pki,
}

impl Utility {
    /// (utility, category key, title key, icon), in sidebar order.
    pub const ALL: [(Utility, &'static str, &'static str, &'static str); 6] = [
        (Utility::Base64, "dt-cat-encoding", "dt-base64", ph::BINARY),
        (Utility::Hash, "dt-cat-encoding", "dt-hash", ph::FINGERPRINT),
        (
            Utility::Jwt,
            "dt-cat-tokens",
            "dt-jwt",
            ph::IDENTIFICATION_BADGE,
        ),
        (Utility::Firebase, "dt-cat-tokens", "dt-firebase", ph::FIRE),
        (Utility::Jasypt, "dt-cat-crypto", "dt-jasypt", ph::KEY),
        (Utility::Pki, "dt-cat-crypto", "dt-pki", ph::SHIELD_CHECK),
    ];
    pub fn key(self) -> &'static str {
        match self {
            Utility::Base64 => "base64",
            Utility::Hash => "hash",
            Utility::Jwt => "jwt",
            Utility::Firebase => "firebase",
            Utility::Jasypt => "jasypt",
            Utility::Pki => "pki",
        }
    }
    pub fn by_key(key: &str) -> Option<Utility> {
        Utility::ALL
            .iter()
            .map(|(u, ..)| *u)
            .find(|u| u.key() == key)
    }
    fn title_key(self) -> &'static str {
        Utility::ALL
            .iter()
            .find(|(u, ..)| *u == self)
            .map(|(_, _, t, _)| *t)
            .unwrap_or("tool-devtools")
    }
}

pub struct DevtoolsFactory;

impl ToolFactory for DevtoolsFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, _services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let utility = restore
            .as_ref()
            .and_then(|v| v.get("utility"))
            .and_then(|v| v.as_str())
            .and_then(Utility::by_key)
            .unwrap_or_default();
        Box::new(DevtoolsTool {
            utility,
            title: String::new(),
            base64: Default::default(),
            hash: Default::default(),
            jwt: Default::default(),
            firebase: Default::default(),
            jasypt: Default::default(),
            pki: Default::default(),
        })
    }
}

pub struct DevtoolsTool {
    utility: Utility,
    title: String,
    base64: base64_tool::State,
    hash: hash::State,
    jwt: jwt::State,
    firebase: firebase::State,
    jasypt: jasypt::State,
    pki: pki::State,
}

impl Tool for DevtoolsTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        if self.title.is_empty() {
            Cow::Borrowed("Devtools")
        } else {
            Cow::Borrowed(&self.title)
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        form::header_bar(ui, &theme, |ui| {
            ui.label(
                egui::RichText::new(s.t("tool-devtools"))
                    .strong()
                    .size(15.0),
            );
        });
        let mut last_cat = "";
        for (u, cat, title, icon) in Utility::ALL {
            if cat != last_cat {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(s.t(cat))
                        .small()
                        .strong()
                        .color(theme.text_muted),
                );
                last_cat = cat;
            }
            if form::list_row(ui, &theme, self.utility == u, icon, &s.t(title), None).clicked() {
                self.utility = u;
            }
            ui.add_space(2.0);
        }
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        self.title = s.t(self.utility.title_key());
        egui::ScrollArea::vertical()
            .id_salt("hs-devtools")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        match self.utility {
                            Utility::Base64 => base64_tool::ui(&mut self.base64, ui, s, &theme),
                            Utility::Hash => hash::ui(&mut self.hash, ui, s, &theme),
                            Utility::Jwt => jwt::ui(&mut self.jwt, ui, s, &theme),
                            Utility::Firebase => firebase::ui(&mut self.firebase, ui, s, &theme),
                            Utility::Jasypt => jasypt::ui(&mut self.jasypt, ui, s, &theme),
                            Utility::Pki => pki::ui(&mut self.pki, ui, s, &theme),
                        }
                    });
            });
    }

    fn printable(&self) -> Option<hs_core::tool::Printable> {
        let items = self.speakable();
        let first = items.first()?;
        Some(hs_core::tool::Printable {
            title: self.title().to_string(),
            subtitle: "Devtools".into(),
            html: hs_core::html::pre(&first.text),
        })
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        Utility::ALL
            .iter()
            .map(|(u, _, title, _)| CommandDescriptor {
                id: format!("devtools.{}", u.key()),
                title: format!("Devtools: {}", title.trim_start_matches("dt-")),
                shortcut_action: None,
            })
            .collect()
    }

    fn run_command(&mut self, id: &str, _cx: &mut ToolContext) -> bool {
        if let Some(key) = id.strip_prefix("devtools.")
            && let Some(u) = Utility::by_key(key)
        {
            self.utility = u;
            return true;
        }
        false
    }

    /// Text handed over from another tool lands in the current utility's input.
    fn open(&mut self, item: OpenRequest, _cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Text { body, .. } => {
                match self.utility {
                    Utility::Base64 => self.base64.input = body,
                    Utility::Hash => self.hash.input = body,
                    Utility::Jwt => self.jwt.token = body,
                    Utility::Firebase => self.firebase.service_json = body,
                    Utility::Jasypt => self.jasypt.cipher_in = body,
                    Utility::Pki => self.pki.payload_in = body,
                }
                true
            }
            _ => false,
        }
    }

    fn speakable(&self) -> Vec<Speakable> {
        let last =
            |r: &Option<Result<String, String>>| r.as_ref().and_then(|x| x.as_ref().ok()).cloned();
        let text = match self.utility {
            Utility::Jwt => last(&self.jwt.encoded),
            Utility::Jasypt => {
                last(&self.jasypt.decrypted_out).or(last(&self.jasypt.encrypted_out))
            }
            Utility::Pki => last(&self.pki.opened).or(last(&self.pki.sealed)),
            Utility::Firebase => last(&self.firebase.custom),
            _ => None,
        };
        text.map(|t| vec![Speakable::new("Result", t)])
            .unwrap_or_default()
    }

    /// Only which utility is open; inputs may hold keys and are never saved.
    fn save_state(&self) -> serde_json::Value {
        serde_json::json!({ "utility": self.utility.key() })
    }
}
