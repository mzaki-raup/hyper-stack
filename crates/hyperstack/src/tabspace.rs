//! The tab space model (docs/04 §2): tabs holding tool instances, the
//! active tab, and conversion to/from the persisted `Layout`.

use std::collections::HashMap;

use egui::Color32;
use hs_core::Services;
use hs_core::tool::{Tool, ToolFactory};
use hs_core::workspace::{Layout, LayoutTab};
use hs_ui::tabs::TabItem;

pub struct TabEntry {
    pub id: u64,
    pub factory_id: String,
    pub tool: Box<dyn Tool>,
    pub pinned: bool,
    pub colour: Option<Color32>,
    /// This activity's permanent home tab: the first one opened for a tool
    /// that wants one. It shows a dashboard of what the activity has open
    /// and cannot be closed, pinned or duplicated. Owner's requirement,
    /// 2026-10-17.
    pub home: bool,
}

/// A closed tab kept for Ctrl+Shift+T.
pub struct ClosedTab {
    pub factory_id: String,
    pub state: serde_json::Value,
    pub title: String,
}

pub struct TabSpace {
    pub tabs: Vec<TabEntry>,
    pub active: Option<u64>,
    pub closed: Vec<ClosedTab>,
    pub dirty: bool,
}

/// Tab ids are handed out process-wide, not per `TabSpace`: a workspace
/// that is switched away from keeps its tabs alive, parked beside the
/// live ones, and everything keyed on a tab id - the browser host's web
/// views, the open-tabs registry, the thumbnails - would collide if each
/// space counted from one (2026-10-29).
static NEXT_TAB_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn alloc_tab_id() -> u64 {
    NEXT_TAB_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Make sure no future id collides with one read back from a layout.
fn reserve_tab_id(id: u64) {
    NEXT_TAB_ID.fetch_max(id + 1, std::sync::atomic::Ordering::Relaxed);
}

/// Read-only view for the strip.
#[derive(Clone)]
pub struct TabView {
    pub id: u64,
    pub factory_id: String,
    pub title: String,
    pub icon: &'static str,
    /// An image to draw instead of `icon`, when the tool offers one.
    pub icon_uri: Option<String>,
    /// A coloured piece before the title, when the tool offers one.
    pub prefix: Option<(String, Color32)>,
    pub pinned: bool,
    pub dirty: bool,
    pub colour: Option<Color32>,
    pub tooltip: String,
    /// False when the tool refuses a second view of itself, or when a tab
    /// bound to a connection has used up that connection's budget of tabs.
    pub can_duplicate: bool,
    /// The activity's permanent home tab.
    pub permanent: bool,
    /// Whether the tab offers a context menu. Home and the browser do not:
    /// there is nothing on it that applies to them. Owner's requirement,
    /// 2026-10-17.
    pub has_menu: bool,
    /// Whether Duplicate is in the menu at all (owner's rule for the
    /// Scratchpad, CR-0029).
    pub shows_duplicate: bool,
    /// Where the tab sits among the pinned ones, when its tool keeps
    /// that order itself (CR-0041).
    pub pin_rank: Option<usize>,
    /// Whether the tab can be closed (CR-0041: a pinned note cannot).
    pub can_close: bool,
    /// Whether the icon is drawn as the pin (CR-0041).
    pub pin_icon: bool,
    /// The activity's Material icon for the kind of item this tab holds,
    /// when that activity has Material icons on (CR-0041).
    pub icon_image: Option<egui::ImageSource<'static>>,
    /// The glyph's own colour: a fixed tab's activity colour (CR-0043).
    pub icon_colour: Option<Color32>,
}

impl TabItem for TabView {
    fn id(&self) -> u64 {
        self.id
    }
    fn title(&self) -> String {
        self.title.clone()
    }
    fn icon(&self) -> &str {
        self.icon
    }
    fn icon_uri(&self) -> Option<String> {
        self.icon_uri.clone()
    }
    fn title_prefix(&self) -> Option<(String, Color32)> {
        self.prefix.clone()
    }
    fn pinned(&self) -> bool {
        self.pinned
    }
    fn can_unpin(&self) -> bool {
        !crate::app::is_mandatory(&self.factory_id)
    }
    fn can_duplicate(&self) -> bool {
        self.can_duplicate && !self.permanent
    }
    fn shows_duplicate(&self) -> bool {
        self.shows_duplicate
    }
    fn pin_rank(&self) -> Option<usize> {
        self.pin_rank
    }
    fn can_close(&self) -> bool {
        self.can_close
    }
    fn pin_icon(&self) -> bool {
        self.pin_icon
    }
    fn icon_image(&self) -> Option<egui::ImageSource<'static>> {
        self.icon_image.clone()
    }
    fn icon_colour(&self) -> Option<Color32> {
        self.icon_colour
    }
    fn permanent(&self) -> bool {
        self.permanent
    }
    fn has_menu(&self) -> bool {
        self.has_menu
    }
    fn dirty(&self) -> bool {
        self.dirty
    }
    fn colour(&self) -> Option<Color32> {
        self.colour
    }
    fn tooltip(&self) -> String {
        self.tooltip.clone()
    }
}

/// What the strip needs from the shell beyond the tabs themselves.
pub struct ViewLook {
    /// Settings -> Appearance -> Sidebar -> Coloured Activity Icons.
    pub coloured: bool,
    /// The open workspace's logo, as an image uri (CR-0043).
    pub logo: Option<String>,
}

/// The colour a fixed tab's glyph wears: its activity's rail colour while
/// Coloured Activity Icons is on, and never the rail's outline - a tab's
/// icon is too small for a ring to read as anything but a smudge. The
/// Editor, the API Client and the Browser are left out: their fixed tabs
/// wear a picture (the workspace's logo, a page's favicon), not a glyph
/// (owner's rule, CR-0043).
fn fixed_tab_colour(factory_id: &str, fixed: bool, look: &ViewLook) -> Option<Color32> {
    let own_picture = matches!(factory_id, "editor" | "api" | "browser");
    (look.coloured && fixed && !own_picture).then(|| crate::app::activity_colour(factory_id))
}

impl Default for TabSpace {
    fn default() -> Self {
        Self::new()
    }
}

impl TabSpace {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            closed: Vec::new(),
            dirty: false,
        }
    }

    /// `material` says whether an activity draws Material item icons, so
    /// a tab can wear the same picture its sidebar row does (CR-0041).
    /// `look` carries the rest of what the strip needs from the shell:
    /// whether fixed tabs are coloured, and the workspace's logo.
    pub fn views(
        &self,
        activity_title: &dyn Fn(&str) -> String,
        material: &dyn Fn(&str) -> bool,
        look: &ViewLook,
    ) -> Vec<TabView> {
        // How many tabs each connection already has. Duplicating one would
        // start a second session against the same server, so it is refused
        // once the tool's budget is used up - one for most, three for SSH.
        // A tool that reports no connection is unaffected.
        let mut per_connection: std::collections::HashMap<(&str, &str), usize> =
            std::collections::HashMap::new();
        for t in &self.tabs {
            if let Some(c) = t.tool.current_connection() {
                *per_connection
                    .entry((t.factory_id.as_str(), c))
                    .or_default() += 1;
            }
        }
        self.tabs
            .iter()
            .map(|t| TabView {
                id: t.id,
                factory_id: t.factory_id.clone(),
                title: t.tool.title().to_string(),
                icon: t.tool.tab_icon().unwrap_or(t.tool.descriptor().icon),
                // A home tab that is the workspace's front page wears the
                // workspace's logo; the shell loads it, once (CR-0043).
                icon_uri: if t.tool.tab_wears_workspace_logo() {
                    look.logo.clone()
                } else {
                    t.tool.tab_icon_uri()
                },
                prefix: t.tool.tab_title_prefix(),
                // A tool that keeps its own pin answers for the tab: the
                // Scratchpad's note is pinned in the sidebar, and a tab
                // showing that note has to agree (CR-0041).
                pinned: t.tool.tab_pinned().unwrap_or(t.pinned),
                pin_rank: t.tool.pin_rank(),
                can_close: t.tool.tab_closable(),
                pin_icon: t.tool.tab_icon_is_pin(),
                icon_colour: fixed_tab_colour(
                    &t.factory_id,
                    t.home || crate::app::is_mandatory(&t.factory_id),
                    look,
                ),
                icon_image: t
                    .tool
                    .tab_item_kind()
                    .filter(|_| material(&t.factory_id))
                    .and_then(hs_ui::fileicons::for_kind),
                dirty: t.tool.dirty(),
                // The tab's own colour, when the user chose one. It used
                // to fall back to the colour of the environment the tool
                // named, which is the same tab-to-environment tie the
                // banner had (2026-10-25).
                colour: t.colour,
                // The activity's display name ("API Client"), not its id
                // ("api"): the id is a key, not something to read.
                tooltip: format!("{}\n{}", t.tool.title(), activity_title(&t.factory_id)),
                permanent: t.home,
                // Settings joins Home and the browser's host tab: it is
                // a fixture of the window, and every entry in the menu
                // either acts on it or on tabs it has no business
                // closing (owner's report, CR-0029).
                has_menu: !matches!(t.factory_id.as_str(), "home" | "browser" | "settings"),
                // A note tab is the note. A second tab on the same note
                // would be two views of one document with one cursor
                // between them, so the Scratchpad does not offer the
                // entry at all (owner's report, CR-0029).
                shows_duplicate: t.factory_id != "scratchpad",
                can_duplicate: t.tool.can_duplicate_tab()
                    && match t.tool.current_connection() {
                        None => true,
                        Some(c) => {
                            per_connection
                                .get(&(t.factory_id.as_str(), c))
                                .copied()
                                .unwrap_or(0)
                                < t.tool.tabs_per_connection()
                        }
                    },
            })
            .collect()
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut TabEntry> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    pub fn open(
        &mut self,
        factory: &dyn ToolFactory,
        services: &Services,
        restore: Option<serde_json::Value>,
    ) -> u64 {
        let id = alloc_tab_id();
        let mut tool = factory.create(services, restore);
        let factory_id = factory.descriptor().id.to_string();
        // The first tab of an activity that wants a home becomes it. There
        // is never a second: this is asked before the tab is pushed, so a
        // later tab of the same activity sees one already there.
        let home = tool.wants_home_tab() && !self.tabs.iter().any(|t| t.factory_id == factory_id);
        // Told before the first frame, so the tab is named right even if
        // the user never opens it.
        tool.set_home_tab(home, services);
        self.tabs.push(TabEntry {
            id,
            factory_id,
            tool,
            pinned: false,
            colour: None,
            home,
        });
        self.active = Some(id);
        self.dirty = true;
        id
    }

    pub fn activate(&mut self, id: u64) {
        if self.tabs.iter().any(|t| t.id == id) {
            self.active = Some(id);
            self.dirty = true;
        }
    }

    pub fn close(&mut self, id: u64) -> Option<ClosedTab> {
        let idx = self.index_of(id)?;
        let entry = self.tabs.remove(idx);
        let closed = ClosedTab {
            factory_id: entry.factory_id.clone(),
            state: entry.tool.save_state(),
            title: entry.tool.title().to_string(),
        };
        self.closed.push(ClosedTab {
            factory_id: closed.factory_id.clone(),
            state: closed.state.clone(),
            title: closed.title.clone(),
        });
        if self.closed.len() > 20 {
            self.closed.remove(0);
        }
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(idx.min(self.tabs.len().saturating_sub(1)))
                .map(|t| t.id);
        }
        self.dirty = true;
        Some(closed)
    }

    pub fn close_others(&mut self, keep: u64) -> Vec<u64> {
        let ids: Vec<u64> = self
            .tabs
            .iter()
            .filter(|t| t.id != keep && !t.pinned)
            .map(|t| t.id)
            .collect();
        ids
    }

    pub fn close_side(&mut self, id: u64, left: bool) -> Vec<u64> {
        let Some(idx) = self.index_of(id) else {
            return Vec::new();
        };
        self.tabs
            .iter()
            .enumerate()
            .filter(|(i, t)| !t.pinned && if left { *i < idx } else { *i > idx })
            .map(|(_, t)| t.id)
            .collect()
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if from < self.tabs.len() && to < self.tabs.len() && from != to {
            let t = self.tabs.remove(from);
            self.tabs.insert(to, t);
            let id = self.tabs[to.min(self.tabs.len() - 1)].factory_id.clone();
            self.regroup(&id);
            self.dirty = true;
        }
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) {
        let Some(t) = self.get_mut(id) else { return };
        t.pinned = pinned;
        let factory_id = t.factory_id.clone();
        self.regroup(&factory_id);
        self.dirty = true;
    }

    /// Order one activity's tabs: its permanent tab first, then the pinned
    /// ones, then the rest, each group keeping the order it had. Only the
    /// slots this activity already occupies are rewritten, so the other
    /// activities do not move. Owner's requirement, 2026-10-17.
    fn regroup(&mut self, factory_id: &str) {
        let slots: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.factory_id == factory_id)
            .map(|(i, _)| i)
            .collect();
        if slots.len() < 2 {
            return;
        }
        let mut order = slots.clone();
        // A stable sort, so a drag inside one of the three groups holds.
        order.sort_by_key(|&i| {
            let t = &self.tabs[i];
            if t.home {
                0
            } else if t.pinned {
                1
            } else {
                2
            }
        });
        if order == slots {
            return;
        }
        let mut from: Vec<Option<TabEntry>> = std::mem::take(&mut self.tabs)
            .into_iter()
            .map(Some)
            .collect();
        let mut out: Vec<TabEntry> = Vec::with_capacity(from.len());
        for i in 0..from.len() {
            let src = match slots.iter().position(|&s| s == i) {
                Some(p) => order[p],
                None => i,
            };
            out.push(from[src].take().expect("every tab is moved once"));
        }
        self.tabs = out;
    }

    pub fn to_layout(&self, active_environment: Option<String>) -> Layout {
        Layout {
            schema: hs_core::workspace::SCHEMA,
            tabs: self
                .tabs
                .iter()
                .map(|t| LayoutTab {
                    id: t.id,
                    tool: t.factory_id.clone(),
                    pinned: t.pinned,
                    colour: t.colour.map(|c| [c.r(), c.g(), c.b()]),
                    home: t.home,
                    state: t.tool.save_state(),
                })
                .collect(),
            active: self.active,
            active_environment,
            ..Default::default()
        }
    }

    /// Give every activity in the restored strip the home tab it wants.
    ///
    /// A layout written before an activity had a home tab has none saved,
    /// and the tab that happens to come first is a working tab - a
    /// connection, a note - which must not be turned into a dashboard.
    /// So a dashboard activity gets a *new* first tab, and only an
    /// activity whose home is a working tab anyway (the browser) adopts the
    /// one that is there. An activity that has just gained a dashboard
    /// (Graphics, 2026-10-19) clears the old flag and gets a new one.
    /// Owner's report, 2026-10-17.
    fn ensure_home_tabs(
        &mut self,
        factories: &HashMap<String, &dyn ToolFactory>,
        services: &Services,
    ) {
        let mut seen: Vec<String> = Vec::new();
        // Indexed by hand: an insert below shifts every later tab right,
        // and a `for 0..len` evaluated once never reached the last one.
        let mut i = 0;
        while i < self.tabs.len() {
            let here = i;
            i += 1;
            let i = here;
            let id = self.tabs[i].factory_id.clone();
            if seen.contains(&id) {
                continue;
            }
            seen.push(id.clone());
            if self
                .tabs
                .iter()
                .any(|t| t.factory_id == id && t.home && t.tool.home_is_dashboard())
            {
                continue;
            }
            let (wants_home, is_dashboard, wants_dashboard) = {
                let t = &self.tabs[i];
                (
                    t.tool.wants_home_tab(),
                    t.tool.home_is_dashboard(),
                    t.tool.wants_dashboard_home(),
                )
            };
            if !wants_home {
                continue;
            }
            if !is_dashboard {
                if !wants_dashboard {
                    self.tabs[i].home = true;
                    self.tabs[i].tool.set_home_tab(true, services);
                    continue;
                }
                // An older working home tab (Graphics before the dashboard)
                // must not stay home; clear it and insert a dashboard below.
                self.tabs[i].home = false;
                self.tabs[i].tool.set_home_tab(false, services);
            }
            let Some(factory) = factories.get(id.as_str()) else {
                continue;
            };
            let mut tool = factory.create(services, None);
            tool.set_home_tab(true, services);
            let new_id = alloc_tab_id();
            self.tabs.insert(
                i,
                TabEntry {
                    id: new_id,
                    factory_id: id,
                    tool,
                    pinned: false,
                    colour: None,
                    home: true,
                },
            );
        }
    }

    pub fn restore(
        &mut self,
        layout: &Layout,
        factories: &HashMap<String, &dyn ToolFactory>,
        services: &Services,
    ) {
        for lt in &layout.tabs {
            let Some(factory) = factories.get(lt.tool.as_str()) else {
                continue;
            };
            let mut tool = factory.create(services, Some(lt.state.clone()));
            // Which tab is the home tab was decided when it was opened and
            // saved with the layout; a tab restored from an older layout,
            // or from one written before an activity had a home, is never
            // one. `ensure_home_tabs` makes the missing ones instead, so a
            // saved connection keeps its own tab.
            let home = lt.home && !self.tabs.iter().any(|t| t.factory_id == lt.tool && t.home);
            tool.set_home_tab(home, services);
            self.tabs.push(TabEntry {
                id: lt.id,
                factory_id: lt.tool.clone(),
                tool,
                pinned: lt.pinned,
                colour: lt.colour.map(|[r, g, b]| Color32::from_rgb(r, g, b)),
                home,
            });
            reserve_tab_id(lt.id);
        }
        self.ensure_home_tabs(factories, services);
        let ids: Vec<String> = {
            let mut seen: Vec<String> = Vec::new();
            for t in &self.tabs {
                if !seen.contains(&t.factory_id) {
                    seen.push(t.factory_id.clone());
                }
            }
            seen
        };
        for id in ids {
            self.regroup(&id);
        }
        self.active = layout
            .active
            .filter(|a| self.tabs.iter().any(|t| t.id == *a))
            .or_else(|| self.tabs.first().map(|t| t.id));
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use hs_core::paths::DataDir;
    use hs_core::runtime::NoopWaker;
    use hs_core::secrets::KdfParams;

    /// A fixed tab wears its activity's rail colour while Coloured
    /// Activity Icons is on, except the three whose fixed tab wears a
    /// picture; and the Editor's home tab is handed the workspace's logo
    /// (CR-0043).
    #[test]
    fn fixed_tabs_wear_the_activity_colour_and_the_editor_the_logo() {
        let tmp = std::env::temp_dir().join(format!("hs-tabs-{}", hs_core::new_id()));
        let paths = DataDir::resolve(Some(&tmp)).unwrap();
        let boot = crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap();
        let s = &boot.services;

        let mut space = TabSpace::new();
        let dev = space.open(&tool_devtools::DevtoolsFactory, s, None);
        let editor = space.open(&tool_editor::EditorFactory, s, None);
        let api = space.open(&tool_api::ApiFactory, s, None);
        let title = |id: &str| id.to_string();
        let material = |_: &str| false;
        let view = |look: &ViewLook, id: u64| {
            space
                .views(&title, &material, look)
                .into_iter()
                .find(|v| v.id == id)
                .unwrap()
        };

        let on = ViewLook {
            coloured: true,
            logo: Some("bytes://workspace-logo/1.png".into()),
        };
        assert_eq!(
            view(&on, dev).icon_colour,
            Some(crate::app::activity_colour("devtools")),
            "Devtools' home tab takes its rail colour"
        );
        assert_eq!(
            view(&on, api).icon_colour,
            None,
            "the API Client is left out"
        );
        let ed = view(&on, editor);
        assert_eq!(ed.icon_colour, None, "and so is the Editor");
        assert_eq!(
            ed.icon_uri.as_deref(),
            Some("bytes://workspace-logo/1.png"),
            "the Editor's home tab wears the workspace's logo"
        );

        let off = ViewLook {
            coloured: false,
            logo: None,
        };
        assert_eq!(
            view(&off, dev).icon_colour,
            None,
            "plain again with Coloured Activity Icons off"
        );
        assert_eq!(view(&off, editor).icon_uri, None, "no logo, no picture");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The logo is read when the workspace is first seen and when the
    /// logo changes - not on the frames in between (owner's rule,
    /// CR-0043). Deleting the file without saying so proves the second
    /// call never looked at the disk.
    #[test]
    fn the_workspace_logo_is_read_once_until_it_changes() {
        let tmp = std::env::temp_dir().join(format!("hs-logo-{}", hs_core::new_id()));
        let paths = DataDir::resolve(Some(&tmp)).unwrap();
        let boot = crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap();
        let s = &boot.services;
        let ctx = egui::Context::default();
        let root = s.workspace.read().root().to_path_buf();
        let logo = root.join("icon.png");
        std::fs::write(&logo, b"not really a png").unwrap();

        let first = hs_ui::icons::workspace_logo_uri(&ctx, s);
        assert!(first.is_some(), "a workspace with a logo has one");

        std::fs::remove_file(&logo).unwrap();
        assert_eq!(
            hs_ui::icons::workspace_logo_uri(&ctx, s),
            first,
            "the next frame takes the loaded one and does not look again"
        );

        hs_ui::icons::reload_workspace_logo(&ctx);
        assert_eq!(
            hs_ui::icons::workspace_logo_uri(&ctx, s),
            None,
            "after Settings says the logo changed, it is read again"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
