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
pub struct TabView {
    pub id: u64,
    pub title: String,
    pub icon: &'static str,
    pub pinned: bool,
    pub dirty: bool,
    pub colour: Option<Color32>,
    pub tooltip: String,
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
    fn pinned(&self) -> bool {
        self.pinned
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
        self.tabs
            .iter()
            .map(|t| TabView {
                id: t.id,
                title: t.tool.title().to_string(),
                icon: t.tool.descriptor().icon,
                pinned: t.pinned,
                dirty: t.tool.dirty(),
                colour: t.colour.or_else(|| env_colour(t.tool.environment())),
                tooltip: format!("{}\n{}", t.tool.title(), t.factory_id),
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
        let tool = factory.create(services, restore);
        self.tabs.push(TabEntry {
            id,
            factory_id: factory.descriptor().id.to_string(),
            tool,
            pinned: false,
            colour: None,
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
            self.dirty = true;
        }
    }

    pub fn next(&mut self, step: i32) {
        if self.tabs.is_empty() {
            return;
        }
        let cur = self.active.and_then(|a| self.index_of(a)).unwrap_or(0) as i32;
        let n = self.tabs.len() as i32;
        let idx = ((cur + step) % n + n) % n;
        self.active = Some(self.tabs[idx as usize].id);
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) {
        if let Some(t) = self.get_mut(id) {
            t.pinned = pinned;
            self.dirty = true;
        }
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
                    state: t.tool.save_state(),
                })
                .collect(),
            active: self.active,
            split: self.split.as_ref().map(to_layout_node),
            active_environment,
            ..Default::default()
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
            let tool = factory.create(services, Some(lt.state.clone()));
            self.tabs.push(TabEntry {
                id: lt.id,
                factory_id: lt.tool.clone(),
                tool,
                pinned: lt.pinned,
                colour: lt.colour.map(|[r, g, b]| Color32::from_rgb(r, g, b)),
            });
            self.next_id = self.next_id.max(lt.id + 1);
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
