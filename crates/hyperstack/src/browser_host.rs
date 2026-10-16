//! The native web views behind the browser tool (`tool-browser`). A web
//! view needs the window handle and must live on the UI thread, so the
//! shell owns them: one `wry` child view per page, positioned over the
//! rectangle the tool reports each frame, hidden while the tab is not on
//! screen or a modal covers it. Windows uses WebView2; other platforms get
//! a stub that tells the tool web views are unavailable in this build.

use tool_browser::SharedBrowser;

#[cfg(windows)]
mod imp {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    use parking_lot::{Mutex, RwLock};
    use tool_browser::{Cmd, HostEvent, INIT_SCRIPT, PageDesire, SharedBrowser, is_blocked};

    struct View {
        webview: wry::WebView,
        javascript: bool,
        adblock: bool,
        dark: bool,
        zoom: f64,
        visible: bool,
        bounds: egui::Rect,
        url: String,
    }

    pub struct BrowserHost {
        views: HashMap<u64, View>,
        events: Arc<Mutex<Vec<HostEvent>>>,
        blocklist: Arc<RwLock<Vec<String>>>,
        failed: Option<String>,
        /// The Chrome DevTools Protocol port the web views listen on, when
        /// the docked inspector is switched on. `None` keeps the port shut.
        debug_port: Option<u16>,
        /// The inspector web view docked under a page, by page id.
        inspectors: HashMap<u64, View>,
        /// Inspector front-end URLs found by the lookup thread.
        found: Arc<Mutex<HashMap<u64, String>>>,
        /// Pages whose lookup is in flight, so it is asked once.
        looking: HashSet<u64>,
    }

    impl Default for BrowserHost {
        fn default() -> Self {
            Self {
                views: HashMap::new(),
                events: Arc::new(Mutex::new(Vec::new())),
                blocklist: Arc::new(RwLock::new(Vec::new())),
                failed: None,
                debug_port: None,
                inspectors: HashMap::new(),
                found: Arc::new(Mutex::new(HashMap::new())),
                looking: HashSet::new(),
            }
        }
    }

    use wry::WebViewBuilderExtWindows as _;

    impl BrowserHost {
        /// `silent` builds a view whose own navigation is not reported as
        /// the page's: the docked inspector loads a URL of its own, and the
        /// address bar must not follow it.
        fn create(
            &mut self,
            frame: &eframe::Frame,
            page: &PageDesire,
            bounds: egui::Rect,
            ppp: f32,
            silent: bool,
        ) -> Result<View, String> {
            let events = self.events.clone();
            let blocklist = self.blocklist.clone();
            let id = page.id;
            let adblock = page.adblock;
            let mut builder = wry::WebViewBuilder::new()
                // Giving arguments replaces wry's own, so its two are
                // repeated here. The debugging port is only added when the
                // docked inspector is on.
                .with_additional_browser_args(match self.debug_port {
                    Some(port) => format!(
                        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection                          --remote-debugging-port={port}                          --remote-allow-origins=http://127.0.0.1:{port}"
                    ),
                    None => {
                        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection".to_string()
                    }
                })
                .with_devtools(true)
                .with_hotkeys_zoom(true)
                .with_initialization_script(format!(
                    "{INIT_SCRIPT}\nwindow.__hs.ads({}); window.__hs.dark({});",
                    page.adblock, page.dark
                ))
                .with_bounds(to_bounds(bounds, ppp));
            if !silent {
                builder = builder
                    .with_document_title_changed_handler({
                        let events = events.clone();
                        move |title| events.lock().push(HostEvent::Title(id, title))
                    })
                    .with_on_page_load_handler({
                        let events = events.clone();
                        move |event, url| {
                            let loading = matches!(event, wry::PageLoadEvent::Started);
                            let mut ev = events.lock();
                            ev.push(HostEvent::Loading(id, loading));
                            ev.push(HostEvent::Url(id, url));
                        }
                    })
                    .with_navigation_handler({
                        let events = events.clone();
                        let blocklist = blocklist.clone();
                        move |url| {
                            if adblock && is_blocked(&url, &blocklist.read()) {
                                events.lock().push(HostEvent::Blocked(id, url));
                                false
                            } else {
                                true
                            }
                        }
                    })
                    .with_new_window_req_handler({
                        let events = events.clone();
                        move |url, _features| {
                            events.lock().push(HostEvent::NewWindow(id, url));
                            wry::NewWindowResponse::Deny
                        }
                    });
            }
            if !page.javascript {
                builder = builder.with_javascript_disabled();
            }
            if !page.url.is_empty() {
                builder = builder.with_url(&page.url);
            }
            let webview = builder
                .build_as_child(frame)
                .map_err(|e| format!("WebView2: {e}"))?;
            if (page.zoom - 1.0).abs() > 0.01 {
                let _ = webview.zoom(page.zoom);
            }
            Ok(View {
                webview,
                javascript: page.javascript,
                adblock: page.adblock,
                dark: page.dark,
                zoom: page.zoom,
                visible: true,
                bounds,
                url: page.url.clone(),
            })
        }

        /// Turn the docked inspector on or off. The web views listen for the
        /// DevTools protocol only while it is on, and the port is chosen once
        /// per run; flipping it rebuilds the views, because the arguments a
        /// web view starts with cannot be changed afterwards.
        pub fn set_devtools_panel(&mut self, on: bool) {
            match (on, self.debug_port) {
                (true, None) => {
                    // A free port, asked for by binding and letting go.
                    let port = std::net::TcpListener::bind("127.0.0.1:0")
                        .ok()
                        .and_then(|l| l.local_addr().ok())
                        .map(|a| a.port());
                    match port {
                        Some(p) => {
                            tracing::info!("browser: devtools protocol on 127.0.0.1:{p}");
                            self.debug_port = Some(p);
                            self.views.clear();
                        }
                        None => tracing::warn!("browser: no free port for the inspector"),
                    }
                }
                (false, Some(_)) => {
                    self.debug_port = None;
                    self.inspectors.clear();
                    self.found.lock().clear();
                    self.looking.clear();
                    self.views.clear();
                }
                _ => {}
            }
        }

        /// Ask the web view's debugging port which target is this page, and
        /// remember the inspector URL for it. One short-lived thread: the UI
        /// thread never waits on the network (ADR-013).
        fn start_lookup(&mut self, page_id: u64, url: String) {
            let Some(port) = self.debug_port else {
                return;
            };
            if !self.looking.insert(page_id) {
                return;
            }
            let found = self.found.clone();
            std::thread::spawn(move || {
                // Up to a few seconds: the target appears once the page has
                // loaded, which may be after the button was pressed.
                for _ in 0..20 {
                    if let Some(u) = super::inspector_url(port, &url) {
                        found.lock().insert(page_id, u);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                tracing::warn!("inspector: gave up looking for the target of {url}");
            });
        }

        /// The page's rectangle, and the inspector's under it when it is open.
        fn split(rect: egui::Rect) -> (egui::Rect, egui::Rect) {
            let line = rect.top() + (rect.height() * 0.6).max(80.0).min(rect.height() - 80.0);
            (
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, line)),
                egui::Rect::from_min_max(egui::pos2(rect.min.x, line), rect.max),
            )
        }

        /// Once per frame, after the tools drew: place, show, hide, create
        /// and drop views to match what the browser tabs asked for.
        pub fn sync(
            &mut self,
            frame: &eframe::Frame,
            ctx: &egui::Context,
            hub: &SharedBrowser,
            overlay: bool,
            blocklist: &[String],
        ) {
            {
                let mut bl = self.blocklist.write();
                if bl.as_slice() != blocklist {
                    *bl = blocklist.to_vec();
                }
            }
            let ppp = ctx.pixels_per_point();
            let mut hub = hub.lock();
            if let Some(e) = &self.failed {
                hub.available = Some(false);
                hub.host_error = Some(e.clone());
            } else {
                hub.available = Some(true);
            }
            let mut wanted: HashSet<u64> = HashSet::new();
            let mut creations: Vec<(PageDesire, egui::Rect)> = Vec::new();
            // (page, where its inspector goes) for this frame.
            let mut inspector_wanted: Vec<(u64, Option<egui::Rect>)> = Vec::new();
            let mut shown: HashSet<u64> = HashSet::new();
            for tab in hub.tabs.values_mut() {
                for cmd in tab.commands.drain(..) {
                    let (id, js) = match &cmd {
                        Cmd::Navigate(id, url) => {
                            if let Some(v) = self.views.get_mut(id) {
                                v.url = url.clone();
                                let _ = v.webview.load_url(url);
                            }
                            continue;
                        }
                        Cmd::Back(id) => (*id, "history.back()"),
                        Cmd::Forward(id) => (*id, "history.forward()"),
                        Cmd::Reload(id) => {
                            if let Some(v) = self.views.get(id) {
                                let _ = v.webview.reload();
                            }
                            continue;
                        }
                        Cmd::Stop(id) => (*id, "window.stop()"),
                        Cmd::DevTools(id) => {
                            if let Some(v) = self.views.get(id) {
                                v.webview.open_devtools();
                            }
                            continue;
                        }
                        Cmd::Print(id) => {
                            if let Some(v) = self.views.get(id) {
                                let _ = v.webview.print();
                            }
                            continue;
                        }
                        Cmd::FocusParent => {
                            // wry asks WebView2 itself to hand the keyboard
                            // back to the window that owns it. SetFocus on
                            // the parent alone does not hold: the child
                            // takes it straight back, and the address bar
                            // stays unusable.
                            for v in self.views.values() {
                                let _ = v.webview.focus_parent();
                            }
                            refocus_parent(frame);
                            continue;
                        }
                        Cmd::ClearData(id) => {
                            if let Some(v) = self.views.get(id) {
                                let _ = v.webview.clear_all_browsing_data();
                            }
                            continue;
                        }
                    };
                    if let Some(v) = self.views.get(&id) {
                        let _ = v.webview.evaluate_script(js);
                    }
                }
                for page in &tab.pages {
                    wanted.insert(page.id);
                    let is_active = tab.active_page == Some(page.id);
                    let whole = tab.rect.filter(|_| is_active && !overlay);
                    // With the inspector open the page keeps the top of the
                    // area and the inspector takes the bottom of it.
                    let docked = page.devtools && self.debug_port.is_some();
                    let (rect, dev_rect) = match (whole, docked) {
                        (Some(r), true) => {
                            let (top, bottom) = Self::split(r);
                            (Some(top), Some(bottom))
                        }
                        (r, _) => (r, None),
                    };
                    if docked {
                        if !self.inspectors.contains_key(&page.id) {
                            self.start_lookup(page.id, page.url.clone());
                        }
                    } else {
                        self.inspectors.remove(&page.id);
                        self.looking.remove(&page.id);
                        self.found.lock().remove(&page.id);
                    }
                    inspector_wanted.push((page.id, dev_rect));
                    match self.views.get_mut(&page.id) {
                        Some(v) => {
                            if v.javascript != page.javascript {
                                // JavaScript is fixed at creation: rebuild.
                                self.views.remove(&page.id);
                                if let Some(r) = rect {
                                    creations.push((page.clone(), r));
                                }
                                continue;
                            }
                            if v.adblock != page.adblock {
                                v.adblock = page.adblock;
                                let _ = v.webview.evaluate_script(&format!(
                                    "window.__hs && window.__hs.ads({})",
                                    page.adblock
                                ));
                            }
                            if v.dark != page.dark {
                                v.dark = page.dark;
                                let _ = v.webview.evaluate_script(&format!(
                                    "window.__hs && window.__hs.dark({})",
                                    page.dark
                                ));
                            }
                            if (v.zoom - page.zoom).abs() > 0.001 {
                                v.zoom = page.zoom;
                                let _ = v.webview.zoom(page.zoom);
                            }
                            match rect {
                                Some(r) => {
                                    if r != v.bounds {
                                        v.bounds = r;
                                        let _ = v.webview.set_bounds(to_bounds(r, ppp));
                                    }
                                    if !v.visible {
                                        v.visible = true;
                                        let _ = v.webview.set_visible(true);
                                    }
                                    shown.insert(page.id);
                                }
                                None => {
                                    if v.visible {
                                        v.visible = false;
                                        let _ = v.webview.set_visible(false);
                                        refocus_parent(frame);
                                        ctx.request_repaint();
                                    }
                                }
                            }
                        }
                        None => {
                            if let Some(r) = rect
                                && self.failed.is_none()
                            {
                                creations.push((page.clone(), r));
                            }
                        }
                    }
                }
                tab.rect = None;
            }
            for (page, rect) in creations {
                match self.create(frame, &page, rect, ppp, false) {
                    Ok(v) => {
                        shown.insert(page.id);
                        self.views.insert(page.id, v);
                    }
                    Err(e) => {
                        tracing::warn!("browser host: {e}");
                        self.failed = Some(e.clone());
                        hub.available = Some(false);
                        hub.host_error = Some(e);
                    }
                }
            }
            // The inspector views: one under each page that asked for one.
            for (page_id, dev_rect) in inspector_wanted {
                let Some(r) = dev_rect else {
                    if let Some(v) = self.inspectors.get_mut(&page_id)
                        && v.visible
                    {
                        v.visible = false;
                        let _ = v.webview.set_visible(false);
                    }
                    continue;
                };
                match self.inspectors.get_mut(&page_id) {
                    Some(v) => {
                        if r != v.bounds {
                            v.bounds = r;
                            let _ = v.webview.set_bounds(to_bounds(r, ppp));
                        }
                        if !v.visible {
                            v.visible = true;
                            let _ = v.webview.set_visible(true);
                        }
                    }
                    None => {
                        let url = self.found.lock().get(&page_id).cloned();
                        if let Some(url) = url {
                            let page = PageDesire {
                                id: page_id,
                                url,
                                javascript: true,
                                adblock: false,
                                dark: false,
                                zoom: 1.0,
                                devtools: false,
                            };
                            match self.create(frame, &page, r, ppp, true) {
                                Ok(v) => {
                                    self.inspectors.insert(page_id, v);
                                }
                                Err(e) => tracing::warn!("inspector: {e}"),
                            }
                        }
                    }
                }
            }
            let gone: Vec<u64> = self
                .views
                .keys()
                .filter(|id| !wanted.contains(id))
                .copied()
                .collect();
            for id in gone {
                self.views.remove(&id);
                self.inspectors.remove(&id);
                self.looking.remove(&id);
                self.found.lock().remove(&id);
            }
            let mut hid_any = false;
            for (id, v) in self.views.iter_mut() {
                if !shown.contains(id) && v.visible {
                    v.visible = false;
                    let _ = v.webview.set_visible(false);
                    hid_any = true;
                }
            }
            if hid_any {
                refocus_parent(frame);
                ctx.request_repaint();
            }
            hub.events.append(&mut self.events.lock());
            if !hub.events.is_empty() {
                ctx.request_repaint();
            }
        }
    }

    /// Give keyboard focus back to the main window: a WebView2 child keeps
    /// it even when hidden, and every keystroke would vanish into it.
    fn refocus_parent(frame: &eframe::Frame) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(h) = frame.window_handle()
            && let RawWindowHandle::Win32(w) = h.as_raw()
        {
            unsafe extern "system" {
                fn SetFocus(hwnd: isize) -> isize;
            }
            // SAFETY: a plain user32 call with the window's own handle.
            unsafe {
                SetFocus(w.hwnd.get());
            }
        }
    }

    fn to_bounds(r: egui::Rect, ppp: f32) -> wry::Rect {
        wry::Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                (r.left() * ppp).round() as i32,
                (r.top() * ppp).round() as i32,
            )),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                (r.width() * ppp).round().max(1.0) as u32,
                (r.height() * ppp).round().max(1.0) as u32,
            )),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use tool_browser::SharedBrowser;

    #[derive(Default)]
    pub struct BrowserHost;

    impl BrowserHost {
        /// There is no web view to inspect on this platform.
        pub fn set_devtools_panel(&mut self, _on: bool) {}

        pub fn sync(
            &mut self,
            _frame: &eframe::Frame,
            _ctx: &egui::Context,
            hub: &SharedBrowser,
            _overlay: bool,
            _blocklist: &[String],
        ) {
            let mut hub = hub.lock();
            hub.available = Some(false);
            hub.host_error = Some(
                "Native web views are built on Windows (WebView2) only in this release.".into(),
            );
            for tab in hub.tabs.values_mut() {
                tab.commands.clear();
                tab.rect = None;
            }
        }
    }
}

pub use imp::BrowserHost;

/// Convenience for the shell.
pub fn new_hub() -> SharedBrowser {
    SharedBrowser::default()
}

/// Ask the debugging port for the page target that is showing `url`, and
/// build the front-end URL for it. The front end is served by the web view
/// itself, so nothing leaves the machine.
#[cfg(windows)]
fn inspector_url(port: u16, url: &str) -> Option<String> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .ok()?;
    stream
        .write_all(b"GET /json/list HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .ok()?;
    // Read what arrives rather than waiting for the close: the server may
    // hold the connection open, and a read timeout would then throw away a
    // reply that is already here.
    let mut raw: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.trim_ascii_end().ends_with(b"]") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let body = String::from_utf8_lossy(&raw).into_owned();
    let json = body.split_once("\r\n\r\n").map(|(_, b)| b)?;
    let targets: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    let list = targets.as_array()?;
    let pick = list
        .iter()
        .find(|t| {
            t.get("type").and_then(|v| v.as_str()) == Some("page")
                && t.get("url").and_then(|v| v.as_str()) == Some(url)
        })
        .or_else(|| {
            list.iter()
                .find(|t| t.get("type").and_then(|v| v.as_str()) == Some("page"))
        })?;
    let id = pick.get("id").and_then(|v| v.as_str())?;
    Some(format!(
        "http://127.0.0.1:{port}/devtools/inspector.html?ws=127.0.0.1:{port}/devtools/page/{id}"
    ))
}
