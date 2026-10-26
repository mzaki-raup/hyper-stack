//! HyperStack devtools (docs/tools/devtools.md): small utilities developers
//! otherwise do on random websites with production secrets. Everything runs
//! locally; the only network calls are the two Firebase token exchanges,
//! each behind its own button. One tab shows one utility; the sidebar lists
//! them by category and a tab can switch between them.

pub mod base64_tool;
mod convert;
mod cron;
pub mod crypto;
mod diff_tool;
pub mod firebase;
mod generate;
pub mod hash;
pub mod jasypt;
pub mod jwt;
mod markup;
pub mod pki;
mod regex_tool;
pub mod url_tool;
mod util;
pub mod yaml;

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
    Url,
    Yaml,
    Hash,
    Jwt,
    Firebase,
    Jasypt,
    Pki,
    Toml,
    Xml,
    Generate,
    Cron,
    Convert,
    Regex,
    Diff,
}

impl Utility {
    /// (utility, category key, title key, icon), in sidebar order.
    pub const ALL: [(Utility, &'static str, &'static str, &'static str); 15] = [
        (Utility::Base64, "dt-cat-encoding", "dt-base64", ph::BINARY),
        (Utility::Url, "dt-cat-encoding", "dt-url", ph::LINK_SIMPLE),
        (
            Utility::Yaml,
            "dt-cat-encoding",
            "dt-yaml",
            ph::TREE_STRUCTURE,
        ),
        (Utility::Hash, "dt-cat-encoding", "dt-hash", ph::FINGERPRINT),
        (
            Utility::Toml,
            "dt-cat-encoding",
            "dt-toml",
            ph::BRACKETS_SQUARE,
        ),
        (Utility::Xml, "dt-cat-encoding", "dt-xml", ph::CODE),
        (
            Utility::Jwt,
            "dt-cat-tokens",
            "dt-jwt",
            ph::IDENTIFICATION_BADGE,
        ),
        (Utility::Firebase, "dt-cat-tokens", "dt-firebase", ph::FIRE),
        (Utility::Jasypt, "dt-cat-crypto", "dt-jasypt", ph::KEY),
        (Utility::Pki, "dt-cat-crypto", "dt-pki", ph::SHIELD_CHECK),
        (
            Utility::Generate,
            "dt-cat-generate",
            "dt-generate",
            ph::DICE_FIVE,
        ),
        (Utility::Cron, "dt-cat-generate", "dt-cron", ph::CLOCK),
        (
            Utility::Convert,
            "dt-cat-generate",
            "dt-convert",
            ph::ARROWS_LEFT_RIGHT,
        ),
        (Utility::Regex, "dt-cat-text", "dt-regex", ph::ASTERISK),
        (Utility::Diff, "dt-cat-text", "dt-diff", ph::GIT_DIFF),
    ];
    pub fn key(self) -> &'static str {
        match self {
            Utility::Base64 => "base64",
            Utility::Url => "url",
            Utility::Yaml => "yaml",
            Utility::Hash => "hash",
            Utility::Jwt => "jwt",
            Utility::Firebase => "firebase",
            Utility::Jasypt => "jasypt",
            Utility::Pki => "pki",
            Utility::Toml => "toml",
            Utility::Xml => "xml",
            Utility::Generate => "generate",
            Utility::Cron => "cron",
            Utility::Convert => "convert",
            Utility::Regex => "regex",
            Utility::Diff => "diff",
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

/// A secret field's value for persistence: a `{{reference}}` survives, a
/// typed secret does not.
pub fn keep_reference(value: &str) -> String {
    let t = value.trim();
    if t.starts_with("{{") && t.ends_with("}}") && !t[2..].contains("{{") {
        t.to_string()
    } else {
        String::new()
    }
}

pub struct DevtoolsFactory;

impl ToolFactory for DevtoolsFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let utility = restore
            .as_ref()
            .and_then(|v| v.get("utility"))
            .and_then(|v| v.as_str())
            .and_then(Utility::by_key)
            .unwrap_or_default();
        fn part<T: Default + serde::de::DeserializeOwned>(
            restore: &Option<serde_json::Value>,
            key: &str,
        ) -> T {
            restore
                .as_ref()
                .and_then(|v| v.get(key))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default()
        }
        Box::new(DevtoolsTool {
            utility,
            is_home: false,
            // Named before the first frame so a restored strip reads right.
            title: services.t(utility.title_key()),
            base64: part(&restore, "base64"),
            url: part(&restore, "url"),
            yaml: part(&restore, "yaml"),
            hash: part(&restore, "hash"),
            jwt: part(&restore, "jwt"),
            firebase: part(&restore, "firebase"),
            jasypt: part(&restore, "jasypt"),
            pki: part(&restore, "pki"),
            toml_state: part(&restore, "toml"),
            xml_state: part(&restore, "xml"),
            generate: part(&restore, "generate"),
            cron: part(&restore, "cron"),
            convert: convert::State::default(),
            regex: part(&restore, "regex"),
            diff: part(&restore, "diff"),
        })
    }
}

pub struct DevtoolsTool {
    utility: Utility,
    title: String,
    /// This tab is the activity's home tab: a dashboard, not a utility.
    is_home: bool,
    base64: base64_tool::State,
    url: url_tool::State,
    yaml: yaml::State,
    toml_state: markup::State,
    xml_state: markup::State,
    generate: generate::State,
    cron: cron::State,
    convert: convert::State,
    regex: regex_tool::State,
    diff: diff_tool::State,
    hash: hash::State,
    jwt: jwt::State,
    firebase: firebase::State,
    jasypt: jasypt::State,
    pki: pki::State,
}

impl DevtoolsTool {
    /// The home tab: what this activity has open, with the last picture of
    /// each pane. Owner's requirement, 2026-10-17.
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = hs_ui::theme::Theme::from_settings(&s.settings.read().appearance);
        let count = Utility::ALL.len();
        let tabs = s.open_tabs.of("devtools");
        hs_ui::form::page_title(
            ui,
            &theme,
            &s.t("tool-devtools"),
            Some(&s.t("devtools-dashboard-sub")),
        );
        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(ui, &theme, &count.to_string(), &s.t("dash-tools"));
            hs_ui::dashboard::stat(ui, &theme, &tabs.len().to_string(), &s.t("dash-open-tabs"));
        });
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        egui::ScrollArea::vertical()
            .id_salt("devtools-dashboard")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(id) = hs_ui::dashboard::open_tabs(ui, &theme, &tabs, &labels) {
                    cx.actions
                        .push(hs_core::tool::Action::FocusTab { tab_id: id });
                }
            });
    }
}

impl Tool for DevtoolsTool {
    fn wants_home_tab(&self) -> bool {
        true
    }

    /// The icon of the utility this tab holds, from the one table the
    /// sidebar lists from - so the two can never drift apart.
    fn tab_icon(&self) -> Option<&'static str> {
        // The dashboard holds no utility - `self.utility` is only its
        // default there - so it keeps the activity's own icon rather
        // than Base64's (owner's report, 2026-10-26).
        if self.is_home {
            return None;
        }
        Utility::ALL
            .iter()
            .find(|(u, _, _, _)| *u == self.utility)
            .map(|(_, _, _, icon)| *icon)
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    /// The activity's home tab is named after the activity; a working tab
    /// after its utility. Owner's report, 2026-10-17.
    fn set_home_tab(&mut self, home: bool, services: &Services) {
        self.is_home = home;
        if home {
            self.title = services.t("tool-devtools");
        }
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
        egui::ScrollArea::vertical()
            .id_salt("devtools-utilities")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Which utility the **active tab** holds, and a dashboard
                // holds none. `self.utility` is only its default (Base64)
                // there, so the row was highlighted for a tool nobody had
                // opened, and the `!=` guard below then swallowed the
                // click on that very row - including when a Base64 tab
                // already existed (owner's report, 2026-10-26).
                let current = (!self.is_home).then_some(self.utility);
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
                    // Each utility has its own tab: clicking another one focuses
                    // that tab, or opens it.
                    if form::list_row(ui, &theme, current == Some(u), icon, &s.t(title), None)
                        .clicked()
                        && current != Some(u)
                    {
                        cx.open(
                            "devtools",
                            Some(OpenRequest::File {
                                uri: format!("devtools://{}", u.key()),
                                line: None,
                            }),
                        );
                    }
                    ui.add_space(2.0);
                }
            });
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if cx.is_home {
            self.dashboard(ui, cx);
            return;
        }
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
                            Utility::Url => url_tool::ui(&mut self.url, ui, s, &theme),
                            Utility::Yaml => yaml::ui(&mut self.yaml, ui, s, &theme),
                            Utility::Hash => hash::ui(&mut self.hash, ui, s, &theme),
                            Utility::Jwt => jwt::ui(&mut self.jwt, ui, s, &theme),
                            Utility::Firebase => firebase::ui(&mut self.firebase, ui, s, &theme),
                            Utility::Toml => {
                                markup::ui(&mut self.toml_state, markup::Kind::Toml, ui, s, &theme)
                            }
                            Utility::Xml => {
                                markup::ui(&mut self.xml_state, markup::Kind::Xml, ui, s, &theme)
                            }
                            Utility::Generate => generate::ui(&mut self.generate, ui, s, &theme),
                            Utility::Cron => cron::ui(&mut self.cron, ui, s, &theme),
                            Utility::Convert => convert::ui(&mut self.convert, ui, s, &theme),
                            Utility::Regex => regex_tool::ui(&mut self.regex, ui, s, &theme),
                            Utility::Diff => diff_tool::ui(&mut self.diff, ui, s, &theme),
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

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        if let Some(key) = id.strip_prefix("devtools.")
            && let Some(u) = Utility::by_key(key)
        {
            if u != self.utility {
                cx.open(
                    "devtools",
                    Some(OpenRequest::File {
                        uri: format!("devtools://{}", u.key()),
                        line: None,
                    }),
                );
            }
            return true;
        }
        false
    }

    fn current_uri(&self) -> Option<String> {
        // The home tab shows no utility. Saying it showed Base64 - the
        // default - made clicking Base64 in the sidebar focus the
        // dashboard and appear to do nothing. Owner's report, 2026-10-18.
        if self.is_home {
            return None;
        }
        Some(format!("devtools://{}", self.utility.key()))
    }

    /// Only the tab showing the same utility takes a `devtools://` request;
    /// any other utility gets a tab of its own.
    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        if self.is_home {
            return None;
        }
        let key = uri.strip_prefix("devtools://")?;
        (Utility::by_key(key) == Some(self.utility)).then_some(0)
    }

    /// Text handed over from another tool lands in the current utility's input.
    fn open(&mut self, item: OpenRequest, _cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::File { uri, .. } => {
                match uri.strip_prefix("devtools://").and_then(Utility::by_key) {
                    Some(u) => {
                        self.utility = u;
                        true
                    }
                    None => false,
                }
            }
            OpenRequest::Text { body, .. } => {
                match self.utility {
                    Utility::Base64 => self.base64.input = body,
                    Utility::Url => self.url.input = body,
                    Utility::Yaml => self.yaml.set_text(body),
                    Utility::Hash => self.hash.input = body,
                    Utility::Jwt => self.jwt.token = body,
                    Utility::Firebase => self.firebase.service_json = body,
                    Utility::Jasypt => self.jasypt.cipher_in = body,
                    Utility::Pki => self.pki.payload_in = body,
                    Utility::Toml => self.toml_state.input = body,
                    Utility::Xml => self.xml_state.input = body,
                    // Nothing to paste into: the generators take settings,
                    // not text.
                    Utility::Generate => {}
                    // The expression field takes a pasted expression.
                    Utility::Cron => self.cron.check_input = body,
                    // Pasted text lands in whichever pane is open, since
                    // each one reads a different kind of value.
                    Utility::Convert => match self.convert.pane {
                        convert::Pane::Time => self.convert.time_input = body,
                        convert::Pane::Epoch => self.convert.epoch_input = body,
                        convert::Pane::Number => self.convert.number_input = body,
                        convert::Pane::Data => self.convert.data_input = body,
                        convert::Pane::Duration => self.convert.duration_input = body,
                        convert::Pane::Bandwidth => self.convert.rate_input = body,
                    },
                    // Pasted text is what the pattern is tried on.
                    Utility::Regex => self.regex.text = body,
                    // The side that is still empty, else the left one.
                    Utility::Diff => {
                        if self.diff.left.is_empty() {
                            self.diff.left = body;
                        } else {
                            self.diff.right = body;
                        }
                    }
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

    /// The utility and its inputs come back after a restart. Keys, passwords
    /// and service accounts are saved only when they are `{{references}}`
    /// to environment variables (which live in the vault); a typed secret
    /// is dropped.
    fn save_state(&self) -> serde_json::Value {
        serde_json::json!({
            "utility": self.utility.key(),
            "base64": self.base64.for_save(),
            "url": self.url,
            "yaml": self.yaml,
            "hash": self.hash.for_save(),
            "jwt": self.jwt.for_save(),
            "firebase": self.firebase.for_save(),
            "jasypt": self.jasypt.for_save(),
            "pki": self.pki.for_save(),
            "regex": self.regex,
            "diff": self.diff,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::keep_reference;

    #[test]
    fn typed_secrets_are_dropped_but_references_kept() {
        assert_eq!(keep_reference("{{api_key}}"), "{{api_key}}");
        assert_eq!(keep_reference("  {{ k }} "), "{{ k }}");
        assert_eq!(keep_reference("sk-live-123"), "");
        assert_eq!(keep_reference("{{a}} and {{b}}"), "");
    }
}
