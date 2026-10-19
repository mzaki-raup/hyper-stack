//! The tab space model (docs/04 §2): tabs holding tool instances, the active
//! tab, the split tree, and conversion to/from the persisted `Layout`.

use std::collections::HashMap;

use egui::Color32;
use hs_core::Services;
use hs_core::tool::{Tool, ToolFactory};
use hs_core::workspace::{Layout, LayoutNode, LayoutTab};
use hs_ui::split::SplitNode;
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
    pub split: Option<SplitNode>,
    pub closed: Vec<ClosedTab>,
    next_id: u64,
    pub dirty: bool,
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

impl TabSpace {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            split: None,
            closed: Vec::new(),
            next_id: 1,
            dirty: false,
        }
    }

    pub fn views(&self, env_colour: &dyn Fn(Option<String>) -> Option<Color32>) -> Vec<TabView> {
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
                icon_uri: t.tool.tab_icon_uri(),
                prefix: t.tool.tab_title_prefix(),
                pinned: t.pinned,
                dirty: t.tool.dirty(),
                colour: t.colour.or_else(|| env_colour(t.tool.environment())),
                tooltip: format!("{}\n{}", t.tool.title(), t.factory_id),
                permanent: t.home,
                has_menu: !matches!(t.factory_id.as_str(), "home" | "browser"),
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
        let id = self.next_id;
        self.next_id += 1;
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
        // Opening a tab shows it in the active pane.
        let prev = self.visible_in_pane_of_active_before(id);
        if let (Some(split), Some(prev)) = (&mut self.split, prev) {
            split.replace_leaf(prev, id);
        }
        self.dirty = true;
        id
    }

    fn visible_in_pane_of_active_before(&self, new_id: u64) -> Option<u64> {
        let split = self.split.as_ref()?;
        let leaves = split.leaves();
        // The pane that showed the previously active tab, if any; else the first pane.
        let prev_active = self
            .tabs
            .iter()
            .rev()
            .find(|t| t.id != new_id && leaves.contains(&t.id))
            .map(|t| t.id);
        prev_active.or_else(|| leaves.first().copied())
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
        if let Some(split) = &mut self.split
            && (!split.remove_leaf(id) || split.leaves().len() <= 1)
        {
            self.split = None;
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

    /// Split: the active tab stays on one side, `new_id` shows on the other.
    pub fn split_with(&mut self, target: u64, new_id: u64, horizontal: bool) {
        match &mut self.split {
            None => {
                self.split = Some(SplitNode::Split {
                    horizontal,
                    ratio: 0.5,
                    a: Box::new(SplitNode::Leaf(target)),
                    b: Box::new(SplitNode::Leaf(new_id)),
                });
            }
            Some(split) => {
                if split.depth() >= hs_ui::split::MAX_DEPTH
                    || !split.split_leaf(target, new_id, horizontal)
                {
                    return;
                }
            }
        }
        self.dirty = true;
    }

    /// Tabs currently visible: all split leaves, or just the active one.
    pub fn visible(&self) -> Vec<u64> {
        match &self.split {
            Some(s) => s.leaves(),
            None => self.active.into_iter().collect(),
        }
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
            split: self.split.as_ref().map(to_layout_node),
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
        for i in 0..self.tabs.len() {
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
            let new_id = self.next_id;
            self.next_id += 1;
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
            self.next_id = self.next_id.max(lt.id + 1);
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
        self.split = layout.split.as_ref().map(from_layout_node).filter(|s| {
            s.leaves()
                .iter()
                .all(|l| self.tabs.iter().any(|t| t.id == *l))
        });
        self.dirty = false;
    }
}

fn to_layout_node(n: &SplitNode) -> LayoutNode {
    match n {
        SplitNode::Leaf(id) => LayoutNode::Leaf { tab: *id },
        SplitNode::Split {
            horizontal,
            ratio,
            a,
            b,
        } => LayoutNode::Split {
            horizontal: *horizontal,
            ratio: *ratio,
            a: Box::new(to_layout_node(a)),
            b: Box::new(to_layout_node(b)),
        },
    }
}

fn from_layout_node(n: &LayoutNode) -> SplitNode {
    match n {
        LayoutNode::Leaf { tab } => SplitNode::Leaf(*tab),
        LayoutNode::Split {
            horizontal,
            ratio,
            a,
            b,
        } => SplitNode::Split {
            horizontal: *horizontal,
            ratio: *ratio,
            a: Box::new(from_layout_node(a)),
            b: Box::new(from_layout_node(b)),
        },
    }
}
