//! HyperStack browser tool (docs/tools/browser.md): tabs of web pages in a
//! multi-row strip, an address bar, back/forward/reload, an ad and tracker
//! blocker (navigation blocklist plus cosmetic hiding), a per-page
//! JavaScript switch, forced dark or light rendering, zoom, developer
//! tools, and Print. Pages are restored with the workspace.
//!
//! The pages themselves are native web views (WebView2 on Windows) owned by
//! the shell's `browser_host`, because a web view needs the window handle
//! and lives on the UI thread. This crate only draws the chrome and talks
//! to the host through `SharedBrowser`: each frame the tool says where its
//! active page should show and what it wants done; the host positions the
//! native view there and reports titles, URLs and blocked navigations back.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, Printable, Speakable, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use hs_ui::icons::ph;
use hs_ui::tabs::{TabItem, TabMenuAction, TabStrip, TabStripLabels};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "browser",
    title_key: "tool-browser",
    icon: ph::GLOBE,
    category: ToolCategory::Network,
    accepts: &[Accept::Text],
};

// ---------------------------------------------------------------------------
// The contract with the host

/// What one page should be: the host creates or updates the native view.
#[derive(Debug, Clone, PartialEq)]
pub struct PageDesire {
    pub id: u64,
    pub url: String,
    pub javascript: bool,
    pub adblock: bool,
    pub dark: bool,
    pub zoom: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Navigate(u64, String),
    Back(u64),
    Forward(u64),
    Reload(u64),
    Stop(u64),
    DevTools(u64),
    Print(u64),
    ClearData(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    Title(u64, String),
    Url(u64, String),
    Loading(u64, bool),
    Blocked(u64, String),
    /// A page asked for a new window (target=_blank): open it as a page.
    NewWindow(u64, String),
    Error(u64, String),
}

#[derive(Debug, Default)]
pub struct TabDesire {
    /// Where the active page shows this frame, in egui points; `None` =
    /// hidden (the tab is not on screen).
    pub rect: Option<egui::Rect>,
    pub active_page: Option<u64>,
    pub pages: Vec<PageDesire>,
    pub commands: Vec<Cmd>,
}

#[derive(Default)]
pub struct BrowserHub {
    /// Per browser tool tab (the shell's tab id).
    pub tabs: HashMap<u64, TabDesire>,
    pub events: Vec<HostEvent>,
    /// `Some(false)` when native web views are not available in this build
    /// or on this machine, with the reason in `host_error`.
    pub available: Option<bool>,
    pub host_error: Option<String>,
}

pub type SharedBrowser = Arc<Mutex<BrowserHub>>;

/// Hosts that serve ads and trackers; a navigation to one is refused and
/// counted. Subresources inside a page are hidden cosmetically instead
/// (`INIT_SCRIPT`), since the host cannot filter them.
pub const DEFAULT_BLOCKLIST: &[&str] = &[
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "adservice.google.com",
    "pagead2.googlesyndication.com",
    "google-analytics.com",
    "googletagmanager.com",
    "googletagservices.com",
    "adnxs.com",
    "taboola.com",
    "outbrain.com",
    "criteo.com",
    "criteo.net",
    "scorecardresearch.com",
    "quantserve.com",
    "moatads.com",
    "amazon-adsystem.com",
    "ads.yahoo.com",
    "advertising.com",
    "rubiconproject.com",
    "pubmatic.com",
    "openx.net",
    "casalemedia.com",
    "adsrvr.org",
    "bidswitch.net",
    "media.net",
    "zedo.com",
    "2mdn.net",
    "serving-sys.com",
    "chartbeat.com",
    "hotjar.com",
    "mixpanel.com",
    "segment.io",
    "fullstory.com",
    "mouseflow.com",
    "clarity.ms",
    "adform.net",
    "smartadserver.com",
    "yieldmo.com",
    "sharethrough.com",
    "teads.tv",
    "revcontent.com",
    "mgid.com",
    "popads.net",
    "propellerads.com",
    "exoclick.com",
    "trafficjunky.net",
    "adcolony.com",
    "applovin.com",
    "unityads.unity3d.com",
    "branch.io",
    "appsflyer.com",
    "adjust.com",
    "bluekai.com",
    "demdex.net",
    "krxd.net",
    "rlcdn.com",
    "tapad.com",
    "agkn.com",
    "liveramp.com",
];

/// Whether `url` points at a host on the blocklist (exact host or a
/// subdomain of it).
pub fn is_blocked(url: &str, extra: &[String]) -> bool {
    let host = url
        .split("//")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    let hit = |d: &str| host == d || host.ends_with(&format!(".{d}"));
    DEFAULT_BLOCKLIST.iter().any(|d| hit(d))
        || extra
            .iter()
            .map(|d| d.trim().to_ascii_lowercase())
            .filter(|d| !d.is_empty())
            .any(|d| hit(&d))
}

/// Runs at document start on every page: `window.__hs.dark(on)` forces a
/// dark (or light) rendering by inverting colours; `window.__hs.ads(on)`
/// hides the usual ad containers and third-party ad frames.
pub const INIT_SCRIPT: &str = r#"
(function () {
  if (window.__hs) return;
  const ADS = '.adsbygoogle, ins.adsbygoogle, [id^="google_ads"], [id^="div-gpt-ad"], [class*="ad-slot"], [class*="ad_slot"], [class*="advert"], [id*="advert"], [class*="sponsored"], iframe[src*="doubleclick"], iframe[src*="googlesyndication"], iframe[src*="adnxs"], iframe[src*="taboola"], iframe[src*="outbrain"], iframe[src*="criteo"], [data-ad-slot], [data-ad-client], .taboola, .OUTBRAIN, #taboola-below-article, .trc_related_container';
  const style = (id, css) => {
    let s = document.getElementById(id);
    if (css === null) { if (s) s.remove(); return; }
    if (!s) { s = document.createElement('style'); s.id = id; (document.head || document.documentElement).appendChild(s); }
    s.textContent = css;
  };
  window.__hs = {
    dark(on) {
      style('__hs_dark', on ? 'html{filter:invert(1) hue-rotate(180deg) !important;background:#111 !important} img,video,picture,canvas,iframe,svg image,[style*="background-image"]{filter:invert(1) hue-rotate(180deg) !important}' : null);
      try { document.documentElement.style.colorScheme = on ? 'dark' : ''; } catch (e) {}
    },
    light(on) {
      style('__hs_light', on ? 'html{background:#fff !important;color:#111 !important} body{background:#fff !important;color:#111 !important}' : null);
    },
    ads(on) {
      style('__hs_ads', on ? ADS + '{display:none !important;visibility:hidden !important}' : null);
    },
  };
})();
"#;

// ---------------------------------------------------------------------------
// The tool

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Page {
    pub id: u64,
    /// What the address bar shows (editable).
    pub address: String,
    /// What the page is at.
    pub url: String,
    pub title: String,
    pub javascript: bool,
    pub adblock: bool,
    pub dark: bool,
    pub zoom: f64,
    #[serde(skip)]
    pub loading: bool,
    #[serde(skip)]
    pub blocked: usize,
    #[serde(skip)]
    pub last_blocked: String,
    #[serde(skip)]
    pub error: Option<String>,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            id: next_page_id(),
            address: String::new(),
            url: String::new(),
            title: String::new(),
            javascript: true,
            adblock: true,
            dark: false,
            zoom: 1.0,
            loading: false,
            blocked: 0,
            last_blocked: String::new(),
            error: None,
        }
    }
}

fn next_page_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    // Keep ids unique across restarts: mix in the clock once.
    static BASE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let base = *BASE.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| (d.as_millis() as u64) << 16)
            .unwrap_or(0)
    });
    base + NEXT.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    pages: Vec<Page>,
    active: usize,
}

pub struct BrowserFactory {
    pub hub: SharedBrowser,
}

impl ToolFactory for BrowserFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let mut state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        for p in &mut state.pages {
            p.id = next_page_id();
        }
        if state.pages.is_empty() {
            let st = services.settings.read();
            state.pages.push(Page {
                address: st.browser.home_url.clone(),
                url: st.browser.home_url.clone(),
                javascript: st.browser.javascript,
                adblock: st.browser.adblock,
                dark: st.browser.dark,
                ..Default::default()
            });
        }
        state.active = state.active.min(state.pages.len() - 1);
        Box::new(BrowserTool {
            hub: self.hub.clone(),
            state,
            tab_id: 0,
            focus_address: false,
            pending: Vec::new(),
        })
    }
}

pub struct BrowserTool {
    hub: SharedBrowser,
    state: State,
    tab_id: u64,
    focus_address: bool,
    pending: Vec<Cmd>,
}

struct PageTab {
    id: u64,
    title: String,
    loading: bool,
}

impl TabItem for PageTab {
    fn id(&self) -> u64 {
        self.id
    }
    fn title(&self) -> String {
        self.title.clone()
    }
    fn icon(&self) -> &str {
        if self.loading {
            ph::CIRCLE_NOTCH
        } else {
            ph::GLOBE
        }
    }
    fn pinned(&self) -> bool {
        false
    }
    fn tooltip(&self) -> String {
        self.title.clone()
    }
}

fn normalise_address(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.contains("://") {
        return t.to_string();
    }
    if t.starts_with("localhost") || t.contains('.') && !t.contains(' ') {
        return format!("https://{t}");
    }
    format!(
        "https://duckduckgo.com/?q={}",
        t.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' =>
                    (b as char).to_string(),
                b' ' => "+".into(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    )
}

impl BrowserTool {
    fn page(&self) -> &Page {
        &self.state.pages[self.state.active]
    }
    fn page_mut(&mut self) -> &mut Page {
        let i = self.state.active;
        &mut self.state.pages[i]
    }

    pub fn open_url(&mut self, url: &str, s: &Services) {
        let st = s.settings.read();
        self.state.pages.push(Page {
            address: url.to_string(),
            url: url.to_string(),
            javascript: st.browser.javascript,
            adblock: st.browser.adblock,
            dark: st.browser.dark,
            ..Default::default()
        });
        self.state.active = self.state.pages.len() - 1;
    }

    fn navigate(&mut self, url: String) {
        let id = self.page().id;
        {
            let p = self.page_mut();
            p.address = url.clone();
            p.url = url.clone();
            p.error = None;
            p.blocked = 0;
            p.last_blocked.clear();
        }
        self.pending.push(Cmd::Navigate(id, url));
    }

    fn apply_events(&mut self) {
        let events: Vec<HostEvent> = {
            let mut hub = self.hub.lock();
            std::mem::take(&mut hub.events)
        };
        let mut reinsert = Vec::new();
        for ev in events {
            let page_id = match &ev {
                HostEvent::Title(id, _)
                | HostEvent::Url(id, _)
                | HostEvent::Loading(id, _)
                | HostEvent::Blocked(id, _)
                | HostEvent::NewWindow(id, _)
                | HostEvent::Error(id, _) => *id,
            };
            let Some(p) = self.state.pages.iter_mut().find(|p| p.id == page_id) else {
                // Another browser tab's page.
                reinsert.push(ev);
                continue;
            };
            match ev {
                HostEvent::Title(_, t) => p.title = t,
                HostEvent::Url(_, u) => {
                    p.url = u.clone();
                    p.address = u;
                }
                HostEvent::Loading(_, l) => p.loading = l,
                HostEvent::Blocked(_, u) => {
                    p.blocked += 1;
                    p.last_blocked = u;
                }
                HostEvent::Error(_, e) => p.error = Some(e),
                HostEvent::NewWindow(_, u) => {
                    let (js, ad, dark) = (p.javascript, p.adblock, p.dark);
                    self.state.pages.push(Page {
                        address: u.clone(),
                        url: u,
                        javascript: js,
                        adblock: ad,
                        dark,
                        ..Default::default()
                    });
                    self.state.active = self.state.pages.len() - 1;
                }
            }
        }
        if !reinsert.is_empty() {
            self.hub.lock().events.extend(reinsert);
        }
    }
}

impl Tool for BrowserTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        let p = self.page();
        if !p.title.is_empty() {
            Cow::Borrowed(&p.title)
        } else if !p.url.is_empty() {
            Cow::Borrowed(&p.url)
        } else {
            Cow::Borrowed("Browser")
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        self.tab_id = cx.tab_id;
        self.apply_events();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let (available, host_error) = {
            let hub = self.hub.lock();
            (hub.available, hub.host_error.clone())
        };
        // Page tabs.
        let tabs: Vec<PageTab> = self
            .state
            .pages
            .iter()
            .map(|p| PageTab {
                id: p.id,
                title: if p.title.is_empty() {
                    if p.url.is_empty() {
                        s.t("browser-new-tab")
                    } else {
                        p.url.clone()
                    }
                } else {
                    p.title.clone()
                },
                loading: p.loading,
            })
            .collect();
        let labels = TabStripLabels {
            new_tab: s.t("browser-new-tab"),
            close: s.t("tab-close"),
            close_others: s.t("tab-close-others"),
            close_left: s.t("tab-close-left"),
            close_right: s.t("tab-close-right"),
            pin: String::new(),
            unpin: String::new(),
            duplicate: s.t("tab-duplicate"),
            split_right: String::new(),
            split_down: String::new(),
            copy_path: s.t("browser-copy-url"),
        };
        let min_width = s.settings.read().appearance.min_file_tab_width;
        let active = Some(self.page().id);
        let r = ui
            .push_id(("browser-tabs", cx.tab_id), |ui| {
                TabStrip {
                    theme: &theme,
                    min_width,
                    gap: 2.0,
                    active,
                    visible: &[],
                    labels: &labels,
                    compact: true,
                }
                .show(ui, &tabs)
            })
            .inner;
        if let Some(id) = r.activated
            && let Some(i) = self.state.pages.iter().position(|p| p.id == id)
        {
            self.state.active = i;
        }
        if r.new_tab {
            let home = s.settings.read().browser.home_url.clone();
            self.open_url(&home, s);
            self.focus_address = true;
        }
        let mut close = Vec::new();
        if let Some(id) = r.close {
            close.push(id);
        }
        if let Some((id, action)) = r.menu {
            match action {
                TabMenuAction::Close => close.push(id),
                TabMenuAction::CloseOthers => {
                    close.extend(self.state.pages.iter().map(|p| p.id).filter(|x| *x != id))
                }
                TabMenuAction::Duplicate => {
                    if let Some(p) = self.state.pages.iter().find(|p| p.id == id).cloned() {
                        let url = p.url.clone();
                        self.open_url(&url, s);
                    }
                }
                TabMenuAction::CopyPath => {
                    if let Some(p) = self.state.pages.iter().find(|p| p.id == id) {
                        ui.ctx().copy_text(p.url.clone());
                    }
                }
                _ => {}
            }
        }
        if let Some((from, to)) = r.reorder
            && from < self.state.pages.len()
            && to < self.state.pages.len()
        {
            let p = self.state.pages.remove(from);
            self.state.pages.insert(to, p);
            self.state.active = to;
        }
        for id in close {
            if self.state.pages.len() > 1
                && let Some(i) = self.state.pages.iter().position(|p| p.id == id)
            {
                self.state.pages.remove(i);
                self.state.active = self.state.active.min(self.state.pages.len() - 1);
            }
        }
        // Address bar.
        let page_id = self.page().id;
        let mut go: Option<String> = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if ui.button(ph::ARROW_LEFT).tip(s.t("browser-back")).clicked() {
                self.pending.push(Cmd::Back(page_id));
            }
            if ui
                .button(ph::ARROW_RIGHT)
                .tip(s.t("browser-forward"))
                .clicked()
            {
                self.pending.push(Cmd::Forward(page_id));
            }
            let loading = self.page().loading;
            if ui
                .button(if loading { ph::X } else { ph::ARROW_CLOCKWISE })
                .tip(if loading {
                    s.t("browser-stop")
                } else {
                    s.t("browser-reload")
                })
                .clicked()
            {
                self.pending.push(if loading {
                    Cmd::Stop(page_id)
                } else {
                    Cmd::Reload(page_id)
                });
            }
            if ui.button(ph::HOUSE).tip(s.t("browser-home")).clicked() {
                go = Some(s.settings.read().browser.home_url.clone());
            }
            let w = (ui.available_width() - 330.0).max(160.0);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.page_mut().address)
                    .hint_text(s.t("browser-address-hint"))
                    .desired_width(w),
            );
            if self.focus_address {
                resp.request_focus();
                self.focus_address = false;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = Some(normalise_address(&self.page().address));
            }
            if ui.button(ph::ARROW_RIGHT).tip(s.t("browser-go")).clicked() {
                go = Some(normalise_address(&self.page().address));
            }
            ui.separator();
            let p = self.page_mut();
            let mut js = p.javascript;
            if ui
                .selectable_label(js, "JS")
                .tip(s.t("browser-js-tip"))
                .clicked()
            {
                js = !js;
            }
            p.javascript = js;
            let mut ad = p.adblock;
            if ui
                .selectable_label(ad, ph::SHIELD_CHECK)
                .tip(s.t("browser-adblock-tip"))
                .clicked()
            {
                ad = !ad;
            }
            p.adblock = ad;
            let mut dark = p.dark;
            if ui
                .selectable_label(dark, ph::MOON)
                .tip(s.t("browser-dark-tip"))
                .clicked()
            {
                dark = !dark;
            }
            p.dark = dark;
            if ui
                .button(ph::MAGNIFYING_GLASS_MINUS)
                .tip(s.t("browser-zoom-out"))
                .clicked()
            {
                p.zoom = (p.zoom / 1.1).max(0.3);
            }
            if ui
                .button(ph::MAGNIFYING_GLASS_PLUS)
                .tip(s.t("browser-zoom-in"))
                .clicked()
            {
                p.zoom = (p.zoom * 1.1).min(4.0);
            }
            if ui.button(ph::BUG).tip(s.t("browser-devtools")).clicked() {
                self.pending.push(Cmd::DevTools(page_id));
            }
            if ui.button(ph::PRINTER).tip(s.t("browser-print")).clicked() {
                self.pending.push(Cmd::Print(page_id));
            }
        });
        if let Some(url) = go {
            self.navigate(url);
        }
        // Status line.
        {
            let p = self.page();
            let mut line = String::new();
            if p.loading {
                line.push_str(&s.t("browser-loading"));
            }
            if p.blocked > 0 {
                if !line.is_empty() {
                    line.push_str(" · ");
                }
                line.push_str(&s.i18n.t_args(
                    "browser-blocked",
                    &[("count", &p.blocked.to_string()), ("url", &p.last_blocked)],
                ));
            }
            if let Some(e) = &p.error {
                if !line.is_empty() {
                    line.push_str(" · ");
                }
                line.push_str(e);
            }
            if !line.is_empty() {
                ui.label(egui::RichText::new(line).small().color(theme.text_muted));
            }
        }
        // The page area: the host paints the native view here.
        let rect = ui.available_rect_before_wrap();
        egui::Frame::new()
            .fill(if self.page().dark {
                egui::Color32::from_gray(20)
            } else {
                egui::Color32::WHITE
            })
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                ui.set_min_size(rect.size());
                if available == Some(false) {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.colored_label(theme.error, s.t("browser-unavailable"));
                        if let Some(e) = host_error {
                            ui.colored_label(theme.text_muted, e);
                        }
                    });
                } else if self.page().url.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.colored_label(theme.text_muted, s.t("browser-empty"));
                    });
                }
            });
        let mut hub = self.hub.lock();
        let tab = hub.tabs.entry(cx.tab_id).or_default();
        tab.rect = Some(rect);
        tab.active_page = Some(self.page().id);
        tab.pages = self
            .state
            .pages
            .iter()
            .map(|p| PageDesire {
                id: p.id,
                url: p.url.clone(),
                javascript: p.javascript,
                adblock: p.adblock,
                dark: p.dark,
                zoom: p.zoom,
            })
            .collect();
        tab.commands.append(&mut self.pending);
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        let c = |id: &str, title: &str| CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: None,
        };
        vec![
            c("browser.new_tab", "Browser: New Page"),
            c("browser.reload", "Browser: Reload"),
            c("browser.devtools", "Browser: Developer Tools"),
            c("browser.dark", "Browser: Toggle Forced Dark"),
            c("browser.js", "Browser: Toggle JavaScript"),
            c("browser.adblock", "Browser: Toggle Ad Blocker"),
            c("browser.clear", "Browser: Clear Browsing Data"),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        let page = self.page().id;
        match id {
            "browser.new_tab" => {
                let home = cx.services.settings.read().browser.home_url.clone();
                self.open_url(&home, cx.services);
                self.focus_address = true;
            }
            "browser.reload" => self.pending.push(Cmd::Reload(page)),
            "browser.devtools" => self.pending.push(Cmd::DevTools(page)),
            "browser.dark" => self.page_mut().dark = !self.page().dark,
            "browser.js" => self.page_mut().javascript = !self.page().javascript,
            "browser.adblock" => self.page_mut().adblock = !self.page().adblock,
            "browser.clear" => self.pending.push(Cmd::ClearData(page)),
            _ => return false,
        }
        true
    }

    fn current_uri(&self) -> Option<String> {
        Some("browser://".into())
    }

    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        (uri.starts_with("http://") || uri.starts_with("https://") || uri.starts_with("browser://"))
            .then_some(5)
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Url { url } => {
                self.open_url(&url, cx.services);
                true
            }
            OpenRequest::File { uri, .. }
                if uri.starts_with("http://") || uri.starts_with("https://") =>
            {
                self.open_url(&uri, cx.services);
                true
            }
            OpenRequest::Text { body, .. } => {
                let url = normalise_address(&body);
                if url.is_empty() {
                    return false;
                }
                self.open_url(&url, cx.services);
                true
            }
            _ => false,
        }
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or(serde_json::Value::Null)
    }

    fn speakable(&self) -> Vec<Speakable> {
        let p = self.page();
        if p.title.is_empty() && p.url.is_empty() {
            return Vec::new();
        }
        vec![Speakable::new("Page", format!("{} {}", p.title, p.url))]
    }

    fn printable(&self) -> Option<Printable> {
        // Print goes through the page's own print dialog (toolbar button);
        // this export lists the open pages.
        let rows: Vec<Vec<String>> = self
            .state
            .pages
            .iter()
            .map(|p| vec![p.title.clone(), p.url.clone()])
            .collect();
        Some(Printable {
            title: "Browser pages".into(),
            subtitle: String::new(),
            html: hs_core::html::table(&["Title".into(), "URL".into()], &rows),
        })
    }
}

impl Drop for BrowserTool {
    fn drop(&mut self) {
        // Tell the host the pages are gone.
        self.hub.lock().tabs.remove(&self.tab_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocklist_matches_hosts_and_subdomains() {
        assert!(is_blocked("https://ad.doubleclick.net/x?y", &[]));
        assert!(is_blocked("https://doubleclick.net", &[]));
        assert!(!is_blocked("https://notdoubleclick.net/", &[]));
        assert!(!is_blocked("https://example.com/doubleclick.net", &[]));
        assert!(is_blocked(
            "https://ads.example.org/",
            &["Example.org".into()]
        ));
        assert!(!is_blocked("about:blank", &[]));
    }

    #[test]
    fn addresses_normalise_to_urls_or_searches() {
        assert_eq!(normalise_address("example.com/a"), "https://example.com/a");
        assert_eq!(normalise_address("http://x.test"), "http://x.test");
        assert_eq!(
            normalise_address("localhost:3000"),
            "https://localhost:3000"
        );
        assert!(normalise_address("rust egui").starts_with("https://duckduckgo.com/?q=rust+egui"));
        assert_eq!(normalise_address("  "), "");
    }
}
