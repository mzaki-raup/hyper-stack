//! A lazy tree for sidebars (project files, schema, hosts, buckets).

use std::collections::HashSet;

use egui::{Align2, CornerRadius, Sense, Vec2};

use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub struct TreeNode {
    pub id: String,
    pub label: String,
    pub icon: String,
    /// `None` = children not loaded yet (shows a caret; the shell loads on expand).
    pub children: Option<Vec<TreeNode>>,
    pub leaf: bool,
    pub badge: Option<String>,
}

impl TreeNode {
    pub fn leaf(id: impl Into<String>, label: impl Into<String>, icon: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: icon.into(),
            children: None,
            leaf: true,
            badge: None,
        }
    }
    pub fn branch(
        id: impl Into<String>,
        label: impl Into<String>,
        icon: impl Into<String>,
        children: Vec<TreeNode>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: icon.into(),
            children: Some(children),
            leaf: false,
            badge: None,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct TreeState {
    pub expanded: HashSet<String>,
    pub selected: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TreeResponse {
    pub clicked: Option<String>,
    pub double_clicked: Option<String>,
    pub secondary: Option<String>,
    /// Branches whose children must be loaded (`children == None` and now expanded).
    pub load: Vec<String>,
}

pub const ROW: f32 = 22.0;
pub const INDENT: f32 = 16.0;

pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut TreeState,
    nodes: &[TreeNode],
) -> TreeResponse {
    let mut resp = TreeResponse::default();
    for n in nodes {
        show_node(ui, theme, state, n, 0, &mut resp);
    }
    resp
}

fn show_node(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut TreeState,
    node: &TreeNode,
    depth: usize,
    resp: &mut TreeResponse,
) {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    let selected = state.selected.as_deref() == Some(node.id.as_str());
    if selected {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(3),
            theme.accent.linear_multiply(0.3),
        );
    } else if r.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(3), theme.surface_hover);
    }
    let x0 = rect.left() + 6.0 + depth as f32 * INDENT;
    let font = egui::FontId::proportional(13.0);
    let expanded = state.expanded.contains(&node.id);
    if !node.leaf {
        let caret = if expanded {
            crate::icons::CARET_DOWN
        } else {
            crate::icons::CARET_RIGHT
        };
        ui.painter().text(
            egui::pos2(x0, rect.center().y),
            Align2::LEFT_CENTER,
            caret,
            egui::FontId::proportional(11.0),
            theme.text_muted,
        );
    }
    ui.painter().text(
        egui::pos2(x0 + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        &node.icon,
        font.clone(),
        theme.text_muted,
    );
    ui.painter().text(
        egui::pos2(x0 + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        &node.label,
        font,
        theme.text,
    );
    if let Some(b) = &node.badge {
        ui.painter().text(
            egui::pos2(rect.right() - 6.0, rect.center().y),
            Align2::RIGHT_CENTER,
            b,
            egui::FontId::proportional(11.0),
            theme.text_muted,
        );
    }
    if r.clicked() {
        state.selected = Some(node.id.clone());
        resp.clicked = Some(node.id.clone());
        if !node.leaf {
            if expanded {
                state.expanded.remove(&node.id);
            } else {
                state.expanded.insert(node.id.clone());
                if node.children.is_none() {
                    resp.load.push(node.id.clone());
                }
            }
        }
    }
    if r.double_clicked() {
        resp.double_clicked = Some(node.id.clone());
    }
    if r.secondary_clicked() {
        state.selected = Some(node.id.clone());
        resp.secondary = Some(node.id.clone());
    }
    if !node.leaf
        && state.expanded.contains(&node.id)
        && let Some(children) = &node.children
    {
        for c in children {
            show_node(ui, theme, state, c, depth + 1, resp);
        }
    }
}
