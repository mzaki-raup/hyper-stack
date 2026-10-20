//! Semantic IR -> editor document. The generator never asks a model for
//! pixels: geometry appears here only for the architecture grid.

use crate::model::{
    Arrow, DiagramDoc, DiagramKind, Edge, EdgeRoute, Group, Node, NodeKind, Point, Style,
};
use crate::schema::{ArchitectureIr, Component, Ir, WorkflowIr};

pub fn convert(ir: &Ir) -> DiagramDoc {
    match ir {
        Ir::Workflow(w) => workflow(w),
        Ir::Architecture(a) => architecture(a),
    }
}

fn workflow(w: &WorkflowIr) -> DiagramDoc {
    let mut doc = DiagramDoc::new(DiagramKind::Flow, &w.meta.title);
    doc.subtitle = w.meta.subtitle.clone();
    doc.auto_layout = true;
    for lane in &w.lanes {
        doc.groups.push(Group {
            id: lane.id.clone(),
            label: lane.label.clone(),
            nodes: w
                .nodes
                .iter()
                .filter(|n| n.lane == lane.id)
                .map(|n| n.id.clone())
                .collect(),
            style: Style::default(),
        });
    }
    for node in &w.nodes {
        doc.nodes.push(Node {
            id: node.id.clone(),
            label: node.label.clone(),
            kind: node_kind(&node.kind),
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            style: Style::default(),
        });
    }
    for (i, edge) in w.edges.iter().enumerate() {
        doc.edges.push(Edge {
            id: format!("e{}", i + 1),
            from: edge.from.clone(),
            to: edge.to.clone(),
            label: edge.label.clone(),
            route: route(&edge.route),
            arrow: Arrow::End,
            dashed: edge.variant == "dashed",
            via: Vec::new(),
        });
    }
    for group in &w.groups {
        doc.groups.push(Group {
            id: group.id.clone(),
            label: group.label.clone(),
            nodes: group.nodes.clone(),
            style: Style::default(),
        });
    }
    doc
}

fn architecture(a: &ArchitectureIr) -> DiagramDoc {
    let mut doc = DiagramDoc::new(DiagramKind::Architecture, &a.meta.title);
    doc.subtitle = a.meta.subtitle.clone();
    doc.auto_layout = false;
    let cols = a.layout.cols.max(1);
    let w = 180.0;
    let h = 64.0;
    let gap_x = 56.0;
    let gap_y = 48.0;
    for (i, component) in a.components.iter().enumerate() {
        let (row, col) = component_cell(component, i, cols);
        doc.nodes.push(Node {
            id: component.id.clone(),
            label: component.label.clone(),
            kind: node_kind(&component.kind),
            x: col as f32 * (w + gap_x),
            y: row as f32 * (h + gap_y),
            w,
            h,
            style: Style::default(),
        });
    }
    for boundary in &a.boundaries {
        doc.groups.push(Group {
            id: format!("boundary{}", doc.groups.len() + 1),
            label: boundary.label.clone(),
            nodes: boundary.wraps.clone(),
            style: Style::default(),
        });
    }
    for (i, connection) in a.connections.iter().enumerate() {
        doc.edges.push(Edge {
            id: format!("c{}", i + 1),
            from: connection.from.clone(),
            to: connection.to.clone(),
            label: connection.label.clone(),
            route: route(&connection.route),
            arrow: Arrow::End,
            dashed: false,
            via: Vec::new(),
        });
    }
    doc
}

fn component_cell(component: &Component, index: usize, cols: u32) -> (u32, u32) {
    match (component.row, component.col) {
        (Some(row), Some(col)) => (row, col),
        (Some(row), None) => (row, (index as u32) % cols),
        (None, Some(col)) => ((index as u32) / cols, col),
        (None, None) => ((index as u32) / cols, (index as u32) % cols),
    }
}

fn node_kind(kind: &str) -> NodeKind {
    match kind {
        "decision" | "condition" => NodeKind::Decision,
        "start" | "end" | "terminator" => NodeKind::StartEnd,
        "input" | "output" | "io" => NodeKind::InputOutput,
        "database" | "store" | "storage" | "datastore" => NodeKind::Database,
        "note" => NodeKind::Note,
        "text" => NodeKind::Text,
        "container" | "boundary" | "group" => NodeKind::Container,
        "frontend" | "client" | "web" => NodeKind::Frontend,
        "backend" | "service" | "api" | "server" => NodeKind::Backend,
        "queue" | "broker" | "topic" => NodeKind::Queue,
        "external" | "third-party" => NodeKind::External,
        "ellipse" => NodeKind::Ellipse,
        "rounded" | "rounded-rectangle" => NodeKind::RoundedRectangle,
        _ => NodeKind::Process,
    }
}

fn route(route: &str) -> EdgeRoute {
    match route {
        "straight" => EdgeRoute::Straight,
        "curved" => EdgeRoute::Curved,
        "orthogonal" | "orthogonal-h" | "orthogonal-v" | "elbow" => EdgeRoute::Orthogonal,
        _ => EdgeRoute::Orthogonal,
    }
}

/// Edges from a simple A -> B list, used by the offline templates.
pub fn chain(doc: &mut DiagramDoc, pairs: &[(&str, &str)]) {
    for (i, (from, to)) in pairs.iter().enumerate() {
        doc.edges.push(Edge {
            id: format!("e{}", i + 1),
            from: (*from).to_string(),
            to: (*to).to_string(),
            label: String::new(),
            route: EdgeRoute::Orthogonal,
            arrow: Arrow::End,
            dashed: false,
            via: Vec::<Point>::new(),
        });
    }
}
