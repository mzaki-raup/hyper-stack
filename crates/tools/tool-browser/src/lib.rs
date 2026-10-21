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

mod favbar;
pub mod side;
pub mod store;

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
    /// Dock the inspector under this page rather than opening a window.
    pub devtools: bool,
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
    /// Take keyboard focus off the web view and give it back to the main
    /// window. A WebView2 child keeps focus even while egui thinks one of
    /// its own fields has it, and every keystroke disappears into the page
    /// — which is what made the address bar look read-only.
    /// The address bar has the keyboard (`true`) or has given it up.
    AddressFocus(bool),
    /// Run a script in a page. Used by Fill in Password, which has to put
    /// values into the page's own form fields; there is no other way in
    /// from outside the web view. Owner's requirement, 2026-10-17.
    Script(u64, String),
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
    /// The tool has a dialog of its own open. A native web view paints
    /// above everything egui draws, so the host has to hide it or the
    /// dialog is invisible — which is exactly what happened to Add to
    /// Favorites. The shell's own modals are known to `app.rs`; a tool's
    /// are not, so the tool says so here. Owner's report, 2026-10-17.
    pub overlay: bool,
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
      style('__hs_dark', on ? 'html{background:#111 !important} body{filter:invert(1) hue-rotate(180deg) !important;background:#fff !important;min-height:100vh !important} img,video,picture,canvas,iframe,svg image,[style*="background-image"]{filter:invert(1) hue-rotate(180deg) !important}' : null);
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
    /// The inspector is docked under this page.
    #[serde(skip)]
    pub devtools: bool,
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
            devtools: false,
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
            last_inspect_at: None,
            hub: self.hub.clone(),
            state,
            tab_id: 0,
            focus_address: false,
            address_focused: false,
            pending: Vec::new(),
            active_favicon: None,
            favicons: Arc::new(Mutex::new(HashMap::new())),
            workspace: String::new(),
            workspace_icon: None,
            synced_frame: None,
            root: std::path::PathBuf::new(),
            favorites: store::Favorites::default(),
            passwords: store::Passwords::default(),
            side: side::SideState::default(),
            adding_favorite: None,
            filling: None,
        })
    }
}

pub struct BrowserTool {
    hub: SharedBrowser,
    state: State,
    tab_id: u64,
    focus_address: bool,
    /// The address bar had keyboard focus last frame, so the page's own
    /// URL reports must not overwrite what is being typed.
    address_focused: bool,
    pending: Vec<Cmd>,
    /// When Inspect was last acted on, so one click is one toggle.
    last_inspect_at: Option<f64>,
    /// The active page's favicon, for the tool's own tab in the shell's
    /// strip. Kept here because the shell asks for it outside `ui`.
    active_favicon: Option<String>,
    /// One favicon per origin, so every page on a site shares the fetch.
    /// `Loading` while it is out, `Failed` for a site that has none — both
    /// are answers, and neither is asked twice.
    favicons: Arc<Mutex<HashMap<String, Favicon>>>,
    /// The workspace this browser belongs to. The window tab carries the
    /// workspace's name and logo rather than the open page's, because there
    /// is one browser per workspace and its pages are its own inner tabs.
    /// Owner's requirement, 2026-10-17.
    workspace: String,
    /// The workspace logo, registered with egui as an image URI.
    workspace_icon: Option<String>,
    /// The frame `sync_workspace` last ran in.
    synced_frame: Option<u64>,
    /// Where this workspace keeps its favourites and logins.
    root: std::path::PathBuf,
    /// This workspace's saved sites and logins. Both are per workspace:
    /// the sites that go with one project are not the ones that go with
    /// another. Owner's requirement, 2026-10-17.
    favorites: store::Favorites,
    passwords: store::Passwords,
    side: side::SideState,
    /// The Add to Favorites dialog, while it is open.
    adding_favorite: Option<side::AddFavorite>,
    /// The Fill in Password dialog, while it is open.
    filling: Option<side::Filling>,
}

/// Where one origin's favicon has got to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Favicon {
    Loading,
    /// Fetched and normalised, waiting for a frame to hand it to egui.
    Fetched(Vec<u8>),
    /// Registered with egui under this image URI.
    Ready(String),
    /// The site has none, or it could not be read. Asked once.
    Failed,
}

impl BrowserTool {
    /// The workspace's name and logo, for the window tab. The logo is read
    /// from disk and handed to egui under a URI that carries the file's
    /// size and modification time, so replacing it shows the new image
    /// rather than the cached old one.
    fn sync_workspace(&mut self, ctx: &egui::Context, services: &Services) {
        // Both the sidebar and the page call this; once a frame is enough
        // (it stats the logo file every time).
        let frame = ctx.cumulative_frame_nr();
        if self.synced_frame == Some(frame) {
            return;
        }
        self.synced_frame = Some(frame);
        let (name, root) = {
            let w = services.workspace.read();
            (w.meta.name.clone(), w.root().to_path_buf())
        };
        if name != self.workspace {
            self.workspace = name;
        }
        if root != self.root {
            self.root = root.clone();
            self.favorites = store::Favorites::load(&root);
            self.passwords = store::Passwords::load(&root);
        }
        let Some(file) = hs_core::workspace::workspace_icon_file(&root) else {
            self.workspace_icon = None;
            return;
        };
        let stamp = std::fs::metadata(&file)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let uri = format!(
            "{}?v={stamp}",
            file.display().to_string().replace('\\', "/")
        );
        if self.workspace_icon.as_deref() == Some(uri.as_str()) {
            return;
        }
        let Ok(bytes) = std::fs::read(&file) else {
            self.workspace_icon = None;
            return;
        };
        ctx.include_bytes(uri.clone(), bytes);
        self.workspace_icon = Some(uri);
    }

    /// Keep the favicon cache up to date and hand back what each page's URL
    /// should draw. Fetching happens on the runtime; the only work here is
    /// registering arrived bytes with egui, which must be done on this
    /// thread because the context is not `Send` to the runtime's tasks.
    fn sync_favicons(
        &mut self,
        ctx: &egui::Context,
        services: &Services,
    ) -> HashMap<String, Option<String>> {
        use hs_core::favicon;
        // The open pages, and every favourite: the sidebar and the
        // favourites bar show a site's icon the way a page tab does, one
        // fetch per origin. Owner's request, 2026-10-18.
        let mut urls: Vec<String> = self.state.pages.iter().map(|p| p.url.clone()).collect();
        urls.extend(store::link_urls(&self.favorites.items));
        urls.dedup();
        let origins: Vec<(String, String)> = urls
            .into_iter()
            .filter_map(|u| favicon::origin_of(&u).map(|o| (u, o)))
            .collect();
        let mut out = HashMap::new();
        for (url, origin) in origins {
            let state = self.favicons.lock().get(&origin).cloned();
            match state {
                None => {
                    // Not asked yet: ask once, and remember that we did.
                    self.favicons
                        .lock()
                        .insert(origin.clone(), Favicon::Loading);
                    let Some(target) = favicon::url_for(&url) else {
                        self.favicons.lock().insert(origin, Favicon::Failed);
                        continue;
                    };
                    let cache = self.favicons.clone();
                    let waker = services.runtime.waker();
                    let key = origin.clone();
                    services.runtime.spawn(async move {
                        let got = favicon::fetch(&target).await;
                        let state = match got {
                            Ok(png) => Favicon::Fetched(png),
                            Err(_) => Favicon::Failed,
                        };
                        cache.lock().insert(key, state);
                        waker.request_repaint();
                    });
                }
                Some(Favicon::Fetched(png)) => {
                    let uri = format!("bytes://hs-favicon/{origin}.png");
                    ctx.include_bytes(uri.clone(), png);
                    self.favicons
                        .lock()
                        .insert(origin.clone(), Favicon::Ready(uri.clone()));
                    out.insert(url, Some(uri));
                }
                Some(Favicon::Ready(uri)) => {
                    out.insert(url, Some(uri));
                }
                Some(Favicon::Loading) | Some(Favicon::Failed) => {
                    out.insert(url, None);
                }
            }
        }
        out
    }
}

struct PageTab {
    id: u64,
    title: String,
    loading: bool,
    /// The favicon, as an egui image URI, once it has arrived.
    favicon: Option<String>,
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
    fn icon_uri(&self) -> Option<String> {
        // Not while loading: the spinner says more than a stale favicon.
        if self.loading {
            return None;
        }
        self.favicon.clone()
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
        let address_focused = self.address_focused;
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
                    // Not while the user is typing in it: the page reports
                    // its URL whenever it likes, and overwriting the field
                    // mid-edit looks exactly like "I cannot type here".
                    if !address_focused {
                        p.address = u;
                    }
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
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn tab_icon_uri(&self) -> Option<String> {
        self.workspace_icon.clone()
    }

    fn tab_icon(&self) -> Option<&'static str> {
        Some(hs_ui::icons::WORKSPACE)
    }

    /// One browser per workspace, so its window tab is named for the
    /// workspace. Which page is open is said by the inner strip, where the
    /// pages actually are.
    fn single_tab(&self) -> bool {
        true
    }

    fn can_duplicate_tab(&self) -> bool {
        false
    }

    fn title(&self) -> Cow<'_, str> {
        if self.workspace.is_empty() {
            Cow::Borrowed("Browser")
        } else {
            Cow::Borrowed(&self.workspace)
        }
    }

    /// The sidebar carries this workspace's favourites and saved logins.
    /// Without one the shell draws the application's tagline there, which
    /// says nothing about the browser. Owner's requirement, 2026-10-17.
    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let theme = Theme::from_settings(&cx.services.settings.read().appearance);
        self.sync_workspace(ui.ctx(), cx.services);
        let icons = self.sync_favicons(ui.ctx(), cx.services);
        let out = side::sidebar(self, ui, cx, &theme, &icons);
        if let Some(url) = out.open {
            self.open_url(&url, cx.services);
        }
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        self.tab_id = cx.tab_id;
        self.sync_workspace(ui.ctx(), s);
        self.apply_events();
        let favicons = self.sync_favicons(ui.ctx(), s);
        self.active_favicon = (!self.page().loading)
            .then(|| favicons.get(&self.page().url).cloned().flatten())
            .flatten();
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
                favicon: favicons.get(&p.url).cloned().flatten(),
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
            duplicate_blocked: String::new(),
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
                    show_new_tab: true,
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
            // The moment the field takes focus, ask the host for the
            // keyboard back; the web view does not give it up by itself.
            // Every frame it is focused, not only the first: the web view
            // takes the keyboard back as soon as it is given up, so one
            // request is not enough to keep it.
            let focused = resp.has_focus();
            // Every frame it is focused, and once more when it is not: a
            // WebView2 child takes the keyboard back the moment it is
            // allowed to, so this is a state the host has to hold, not an
            // event it can act on once.
            if focused || self.address_focused {
                self.pending.push(Cmd::AddressFocus(focused));
            }
            self.address_focused = focused;
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = Some(normalise_address(&self.page().address));
            }
            if ui.button(ph::ARROW_RIGHT).tip(s.t("browser-go")).clicked() {
                go = Some(normalise_address(&self.page().address));
            }
            ui.separator();
            let docked = s.settings.read().browser.devtools_panel;
            let last_inspect = self.last_inspect_at;
            let mut toggled: Option<f64> = None;
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
            // Inspect docks the inspector under the page when the setting
            // allows it (that needs a local debugging port); otherwise it
            // opens WebView2's own window, which is all that API offers.
            let open = p.devtools;
            let clicked = ui
                .add(egui::Button::selectable(docked && open, ph::BUG))
                .tip(s.t(if docked {
                    "browser-devtools-panel"
                } else {
                    "browser-devtools"
                }))
                .clicked();
            // Opening the panel resizes the page, which makes egui lay the
            // frame out again, and `clicked` is pointer state rather than an
            // event: without this the second pass closes it again.
            let now = ui.input(|i| i.time);
            if clicked && last_inspect != Some(now) {
                toggled = Some(now);
                if docked {
                    p.devtools = !open;
                }
            }
            if ui.button(ph::PRINTER).tip(s.t("browser-print")).clicked() {
                self.pending.push(Cmd::Print(page_id));
            }
            if ui
                .button(ph::STAR)
                .tip(s.t("browser-add-favorite"))
                .clicked()
            {
                self.adding_favorite = Some(side::AddFavorite::default());
            }
            if ui
                .button(ph::KEY)
                .tip(s.t("browser-fill-password"))
                .clicked()
            {
                self.filling = Some(side::Filling::default());
            }
            if let Some(now) = toggled {
                self.last_inspect_at = Some(now);
                if !docked {
                    // No panel: WebView2's own window is all there is.
                    self.pending.push(Cmd::DevTools(page_id));
                }
            }
        });
        if let Some(url) = go {
            self.navigate(url);
        }
        // The favourites bar, under the address bar like Chrome's bookmarks
        // bar. Owner's requirement, 2026-10-18.
        if s.settings.read().browser.favorites_bar {
            let out = favbar::show(ui, s, &theme, &self.favorites, &favicons);
            if let Some(url) = out.go {
                self.navigate(url);
            }
            if let Some(url) = out.open_background {
                // A new page beside this one, which stays in front: that is
                // what a middle click means.
                let keep = self.state.active;
                self.open_url(&url, s);
                self.state.active = keep;
            }
            if let Some(id) = out.edit {
                self.side.tab = side::SideTab::Favorites;
                self.side.editing = Some(side::Editing::Favorite(id));
            }
            if let Some(id) = out.remove {
                self.favorites.remove(&id);
                side::save_favorites(self, cx);
            }
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
        // The page area: the host paints the native view here. The
        // favourites/passwords editor takes a column out of it rather than
        // being drawn on top, because the native view paints above
        // anything egui draws.
        let full = ui.available_rect_before_wrap();
        let detail_w = side::detail_width(self);
        let rect = if detail_w > 0.0 {
            egui::Rect::from_min_max(
                full.min,
                egui::pos2(full.right() - detail_w - 6.0, full.bottom()),
            )
        } else {
            full
        };
        if detail_w > 0.0 {
            let pane = egui::Rect::from_min_max(
                egui::pos2(full.right() - detail_w, full.top()),
                full.right_bottom(),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(pane)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(pane.intersect(ui.clip_rect()));
            side::detail(self, &mut child, cx, &theme);
        }
        egui::Frame::new()
            .fill(if self.page().dark {
                egui::Color32::from_gray(20)
            } else {
                egui::Color32::WHITE
            })
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                ui.set_max_size(rect.size());
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
        side::add_favorite_dialog(self, ui, cx, &theme);
        side::fill_password_dialog(self, ui, cx, &theme);
        let mut hub = self.hub.lock();
        let tab = hub.tabs.entry(cx.tab_id).or_default();
        // Any open popup too: a favourites-bar folder menu or a chip's
        // context menu drops into the page area, and the native view
        // would paint over it. Owner's report, 2026-10-18.
        tab.overlay = self.adding_favorite.is_some()
            || self.filling.is_some()
            || self.side.confirm.is_some()
            || egui::Popup::is_any_open(ui.ctx());
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
                devtools: p.devtools,
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
            c(
                "browser.favorites_bar",
                "Browser: Show or Hide the Favorites Bar",
            ),
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
            "browser.devtools" => {
                let docked = cx.services.settings.read().browser.devtools_panel;
                match self.state.pages.iter_mut().find(|p| p.id == page) {
                    Some(p) if docked => p.devtools = !p.devtools,
                    _ => self.pending.push(Cmd::DevTools(page)),
                }
            }
            "browser.dark" => self.page_mut().dark = !self.page().dark,
            "browser.js" => self.page_mut().javascript = !self.page().javascript,
            "browser.adblock" => self.page_mut().adblock = !self.page().adblock,
            "browser.clear" => self.pending.push(Cmd::ClearData(page)),
            "browser.favorites_bar" => {
                let mut st = cx.services.settings.write();
                st.browser.favorites_bar = !st.browser.favorites_bar;
                drop(st);
                if let Err(e) = cx.services.save_settings() {
                    cx.toast(hs_core::tool::ToastLevel::Error, e.to_string());
                }
            }
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
