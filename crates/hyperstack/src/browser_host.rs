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
    }

    impl Default for BrowserHost {
        fn default() -> Self {
            Self {
                views: HashMap::new(),
                events: Arc::new(Mutex::new(Vec::new())),
                blocklist: Arc::new(RwLock::new(Vec::new())),
                failed: None,
            }
        }
    }

    impl BrowserHost {
        fn create(
            &mut self,
            frame: &eframe::Frame,
            page: &PageDesire,
            bounds: egui::Rect,
            ppp: f32,
        ) -> Result<View, String> {
            let events = self.events.clone();
            let blocklist = self.blocklist.clone();
            let id = page.id;
            let adblock = page.adblock;
            let mut builder = wry::WebViewBuilder::new()
                .with_devtools(true)
                .with_hotkeys_zoom(true)
                .with_initialization_script(format!(
                    "{INIT_SCRIPT}\nwindow.__hs.ads({}); window.__hs.dark({});",
                    page.adblock, page.dark
                ))
                .with_bounds(to_bounds(bounds, ppp))
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
                    let rect = tab.rect.filter(|_| is_active && !overlay);
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
                match self.create(frame, &page, rect, ppp) {
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
            let gone: Vec<u64> = self
                .views
                .keys()
                .filter(|id| !wanted.contains(id))
                .copied()
                .collect();
            for id in gone {
                self.views.remove(&id);
            }
            for (id, v) in self.views.iter_mut() {
                if !shown.contains(id) && v.visible {
                    v.visible = false;
                    let _ = v.webview.set_visible(false);
                }
            }
            hub.events.append(&mut self.events.lock());
            if !hub.events.is_empty() {
                ctx.request_repaint();
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
