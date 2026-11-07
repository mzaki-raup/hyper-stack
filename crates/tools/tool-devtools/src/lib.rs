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
pub mod envfile;
pub mod firebase;
mod generate;
pub mod hash;
pub mod jasypt;
pub mod jwt;
mod markup;
pub mod net;
pub mod pki;
pub mod ports;
mod regex_tool;
pub mod snippets;
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
    // ---- depth programme, phase G ----
    Net,
    Ports,
    Snippets,
    EnvFile,
}

impl Utility {
    /// (utility, category key, title key, icon), in sidebar order.
    pub const ALL: [(Utility, &'static str, &'static str, &'static str); 19] = [
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
        (Utility::Net, "dt-cat-network", "dt-net", ph::GLOBE_SIMPLE),
        (Utility::Ports, "dt-cat-network", "dt-ports", ph::PLUGS),
        (
            Utility::Snippets,
            "dt-cat-workspace",
            "dt-snippets",
            ph::CODE_SIMPLE,
        ),
        (
            Utility::EnvFile,
            "dt-cat-workspace",
            "dt-env",
            ph::FILE_TEXT,
        ),
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
            Utility::Net => "net",
            Utility::Ports => "ports",
            Utility::Snippets => "snippets",
            Utility::EnvFile => "env",
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
            net: part(&restore, "net"),
            // Neither reads back: a port list and an open file belong to
            // the session that made them, not to a restored tab.
            ports: ports::State::default(),
            snippets: snippets::State::default(),
            envfile: envfile::State::default(),
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
    net: net::State,
    ports: ports::State,
    snippets: snippets::State,
    envfile: envfile::State,
}

impl DevtoolsTool {
    /// The home tab: what this activity has open, with the last picture of
    /// each pane. Owner's requirement, 2026-10-17.
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = hs_ui::theme::Theme::from_settings(&s.settings.read().appearance);
        let tabs = s.open_tabs.of("devtools");
        // The utilities, each under the category the sidebar files it
        // in, from the one table both read - so the two can never
        // disagree about what Devtools offers. Each card wears the
        // utility's own picture (CR-0076).
        register_pictures(ui.ctx());
        let tools: Vec<hs_ui::dashboard::Entry> = Utility::ALL
            .iter()
            .map(|(u, cat, title, _)| {
                let (picture, glyph, colour) = tool_picture(*u);
                hs_ui::dashboard::Entry {
                    id: u.key().to_string(),
                    name: s.t(title),
                    detail: s.t(cat),
                    icon: glyph.to_string(),
                    image: picture.as_ref().and_then(picture_uri),
                    tint: Some(colour),
                }
            })
            .collect();
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        let page = hs_ui::dashboard::Page {
            icon: DESCRIPTOR.icon,
            title: &s.t("tool-devtools"),
            subtitle: &s.t("devtools-dashboard-sub"),
            saved: &s.t("dash-tools"),
            empty: "",
            // Nothing to start here: every utility is in the grid below,
            // and a button row would only repeat it (owner's
            // requirement, 2026-10-29).
            start_here: None,
            salt: "devtools-dashboard",
            tabs: &tabs,
            labels: &labels,
            extra: None,
            entry_menu: None,
        };
        let pick = hs_ui::dashboard::saved_dashboard(
            ui,
            &theme,
            &page,
            &[
                hs_ui::dashboard::Stat {
                    icon: ph::WRENCH,
                    value: &tools.len().to_string(),
                    label: &s.t("dash-tools"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::COLUMNS,
                    value: &tabs.len().to_string(),
                    label: &s.t("dash-open-tabs"),
                },
            ],
            &[],
            &tools,
        );
        match pick {
            Some(hs_ui::dashboard::Pick::Open(key)) => {
                cx.open(
                    "devtools",
                    Some(hs_core::tool::OpenRequest::File {
                        uri: format!("devtools://{key}"),
                        line: None,
                    }),
                );
            }
            // No second grid on this dashboard (CR-0056).
            Some(hs_ui::dashboard::Pick::OpenExtra(_)) => {}
            Some(hs_ui::dashboard::Pick::Focus(tab_id)) => {
                cx.actions.push(hs_core::tool::Action::FocusTab { tab_id });
            }
            _ => {}
        }
    }
}

impl Tool for DevtoolsTool {
    /// A utility tab has a toolbar row - the utility's name on the left
    /// and the hard-right corner kept free, so a utility that grows an
    /// AI Assistant has its control where every other activity puts it
    /// (owner's requirement, 2026-10-29). The dashboard draws none.
    fn draws_toolbar(&self) -> bool {
        !self.is_home
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if self.is_home {
            return;
        }
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if let Some((_, _, title, icon)) =
            Utility::ALL.iter().find(|(u, _, _, _)| *u == self.utility)
        {
            ui.label(
                egui::RichText::new(format!("{icon} {}", s.t(title)))
                    .strong()
                    .color(theme.accent),
            );
        }
    }

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

    /// The utility's own picture, as its sidebar row and dashboard card
    /// draw it (CR-0076). The sidebar and the tab's `ui` register the
    /// bytes behind this URI.
    fn tab_icon_uri(&self) -> Option<String> {
        if self.is_home {
            return None;
        }
        tool_picture(self.utility).0.as_ref().and_then(picture_uri)
    }

    fn tab_icon_colour(&self) -> Option<egui::Color32> {
        // The utility's own colour, as on its dashboard card (CR-0076).
        if self.is_home {
            return None;
        }
        Some(tool_picture(self.utility).2)
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    fn tab_title_whole(&self) -> bool {
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
        // The tab strip draws these pictures by URI (CR-0076).
        register_pictures(ui.ctx());
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
                for (u, cat, title, _) in Utility::ALL {
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
                    // The utility's own picture, or its glyph in its own
                    // colour, as the dashboard card and the tab draw it
                    // (CR-0076).
                    let (picture, glyph, colour) = tool_picture(u);
                    let row = match picture {
                        Some(src) => form::list_row_icon(
                            ui,
                            &theme,
                            current == Some(u),
                            &hs_ui::tree::TreeIcon::Image(src),
                            &s.t(title),
                            None,
                        ),
                        None => form::conn_list_row(
                            ui,
                            &theme,
                            current == Some(u),
                            hs_ui::icons::ConnIcon {
                                glyph,
                                image: None,
                                tint: hs_ui::icons::ConnTint::Brand(colour),
                            },
                            &s.t(title),
                            None,
                        ),
                    };
                    if row.clicked() && current != Some(u) {
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
        register_pictures(ui.ctx());
        egui::ScrollArea::vertical()
            .id_salt("hs-devtools")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        // A tool's hints and notes line up with its cards
                        // (`CARD_MAX_WIDTH`) rather than running to the
                        // window edge; Text Diff's two sides use the whole
                        // width (2026-11-07).
                        let width = if matches!(self.utility, Utility::Diff) {
                            ui.available_width()
                        } else {
                            ui.available_width().min(hs_ui::form::CARD_MAX_WIDTH)
                        };
                        ui.set_width(width);
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
                            Utility::Net => net::ui(&mut self.net, ui, s, &theme),
                            Utility::Ports => ports::ui(&mut self.ports, ui, s, &theme),
                            Utility::Snippets => snippets::ui(&mut self.snippets, ui, s, &theme),
                            Utility::EnvFile => envfile::ui(&mut self.envfile, ui, s, &theme),
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
                    // The network checks take a host or a URL rather
                    // than a block of text; the port list and the .env
                    // editor read the machine and a file.
                    Utility::Net => self.net.host = body.trim().to_string(),
                    Utility::Ports | Utility::Snippets | Utility::EnvFile => {}
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
            "net": serde_json::to_value(&self.net).unwrap_or_default(),
        })
    }
}

/// Which bundled Material picture shows a utility, if one does, and the
/// colour of its glyph (`0xRRGGBB`) - the glyph is what a utility with no
/// fitting picture wears (CR-0076).
fn picture_spec(u: Utility) -> (Option<&'static str>, u32) {
    match u {
        Utility::Base64 => (None, 0x3b82f6),
        Utility::Url => (Some("http"), 0x0ea5e9),
        Utility::Yaml => (Some("yaml"), 0xef4444),
        Utility::Hash => (None, 0x8b5cf6),
        Utility::Toml => (Some("toml"), 0x9ca3af),
        Utility::Xml => (Some("xml"), 0xf97316),
        Utility::Jwt => (None, 0xd946ef),
        Utility::Firebase => (None, 0xf59e0b),
        Utility::Jasypt => (Some("lock"), 0xeab308),
        Utility::Pki => (Some("certificate"), 0xf43f5e),
        Utility::Generate => (None, 0x22c55e),
        Utility::Cron => (Some("history"), 0x84cc16),
        Utility::Convert => (None, 0x14b8a6),
        Utility::Regex => (None, 0xa855f7),
        Utility::Diff => (Some("diff"), 0x10b981),
        Utility::Net => (Some("network"), 0x06b6d4),
        Utility::Ports => (None, 0x6366f1),
        Utility::Snippets => (None, 0xec4899),
        Utility::EnvFile => (Some("tune"), 0xfacc15),
    }
}

/// A utility's own picture, the same in the sidebar row, the dashboard
/// card and the tab (CR-0076): the Material picture when one fits, the
/// utility's glyph, and that glyph's colour for when there is no picture.
pub fn tool_picture(
    u: Utility,
) -> (
    Option<egui::ImageSource<'static>>,
    &'static str,
    egui::Color32,
) {
    let (name, rgb) = picture_spec(u);
    let glyph = Utility::ALL
        .iter()
        .find(|(x, _, _, _)| *x == u)
        .map_or(DESCRIPTOR.icon, |(_, _, _, g)| *g);
    let [_, r, g, b] = rgb.to_be_bytes();
    (
        name.and_then(hs_ui::fileicons::by_name),
        glyph,
        egui::Color32::from_rgb(r, g, b),
    )
}

/// The URI a picture is drawn by where an API takes a string (the tab,
/// a dashboard card).
fn picture_uri(src: &egui::ImageSource<'static>) -> Option<String> {
    match src {
        egui::ImageSource::Bytes { uri, .. } | egui::ImageSource::Uri(uri) => Some(uri.to_string()),
        egui::ImageSource::Texture(_) => None,
    }
}

/// Hand every utility picture's bytes to `ctx`, so one drawn by bare URI
/// (a tab, a dashboard card) loads even before its source was drawn.
/// Cheap after the first call: the loader keeps the first copy.
fn register_pictures(ctx: &egui::Context) {
    for (u, _, _, _) in Utility::ALL {
        if let Some(egui::ImageSource::Bytes { uri, bytes }) = tool_picture(u).0 {
            ctx.include_bytes(uri, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Utility, keep_reference, picture_spec, picture_uri, tool_picture};

    #[test]
    fn typed_secrets_are_dropped_but_references_kept() {
        assert_eq!(keep_reference("{{api_key}}"), "{{api_key}}");
        assert_eq!(keep_reference("  {{ k }} "), "{{ k }}");
        assert_eq!(keep_reference("sk-live-123"), "");
        assert_eq!(keep_reference("{{a}} and {{b}}"), "");
    }

    /// CR-0076: every utility wears a picture of its own, or its glyph in a
    /// colour of its own, and no two utilities look alike.
    #[test]
    fn every_utility_has_its_own_picture_or_coloured_glyph() {
        let mut seen = std::collections::HashSet::new();
        for (u, _, _, _) in Utility::ALL {
            let (name, _) = picture_spec(u);
            let (picture, glyph, colour) = tool_picture(u);
            assert_eq!(
                name.is_some(),
                picture.is_some(),
                "{u:?}: {name:?} is bundled"
            );
            let look = match &picture {
                Some(src) => picture_uri(src).expect("an embedded picture has a URI"),
                None => {
                    assert!(!glyph.is_empty(), "{u:?} has a glyph");
                    assert_ne!(colour, egui::Color32::TRANSPARENT, "{u:?} has a colour");
                    format!("{glyph}{colour:?}")
                }
            };
            assert!(seen.insert(look), "{u:?} looks like another utility");
        }
    }

    /// CR-0062: every field is a `form::line_edit` / `text_field` (a bare
    /// single-line `TextEdit` draws short), every button a `form::button`
    /// with an icon and a label, and every table goes through
    /// `DataTable` / `edit_table`. The needles are split so this test's
    /// own source does not match them.
    #[test]
    fn no_bare_text_field_button_or_table_is_left_in_the_crate() {
        let sources = [
            ("base64_tool.rs", include_str!("base64_tool.rs")),
            ("convert.rs", include_str!("convert.rs")),
            ("cron.rs", include_str!("cron.rs")),
            ("crypto.rs", include_str!("crypto.rs")),
            ("diff_tool.rs", include_str!("diff_tool.rs")),
            ("envfile.rs", include_str!("envfile.rs")),
            ("firebase.rs", include_str!("firebase.rs")),
            ("generate.rs", include_str!("generate.rs")),
            ("hash.rs", include_str!("hash.rs")),
            ("jasypt.rs", include_str!("jasypt.rs")),
            ("jwt.rs", include_str!("jwt.rs")),
            ("lib.rs", include_str!("lib.rs")),
            ("markup.rs", include_str!("markup.rs")),
            ("net.rs", include_str!("net.rs")),
            ("pki.rs", include_str!("pki.rs")),
            ("ports.rs", include_str!("ports.rs")),
            ("regex_tool.rs", include_str!("regex_tool.rs")),
            ("snippets.rs", include_str!("snippets.rs")),
            ("url_tool.rs", include_str!("url_tool.rs")),
            ("util.rs", include_str!("util.rs")),
            ("yaml.rs", include_str!("yaml.rs")),
        ];
        let needles = [
            concat!("TextEdit::", "singleline"),
            concat!("ui.", "button("),
            concat!(".small_", "button("),
            concat!("Button::new(", "s.t("),
            concat!("TableBuilder::", "new"),
            concat!("egui::Grid::", "new"),
        ];
        for (file, text) in sources {
            // Whitespace is dropped, so a method call split over two lines
            // counts too.
            let joined: String = text.split_whitespace().collect();
            for needle in needles {
                let compact: String = needle.split_whitespace().collect();
                assert!(
                    !joined.contains(&compact) && !joined.contains(&compact.replace("ui.", ".")),
                    "{file} still has a bare `{needle}`"
                );
            }
        }
    }
}
