//! The generator's semantic IR (Archify-adapted, schema version 1). Strict
//! parsing: unknown fields are rejected so a model cannot smuggle pixels or
//! Archify-only fields into a generated document.

use crate::validate::{
    CODE_DUPLICATE_ID, CODE_EMPTY_LABEL, CODE_INVALID_ID, CODE_OUT_OF_RANGE, CODE_REQUIRED,
    CODE_TOO_MANY, CODE_UNKNOWN_BOUNDARY, CODE_UNKNOWN_LANE, CODE_UNKNOWN_NODE, CODE_UNSUPPORTED,
    Issue, MAX_EDGES, MAX_GROUPS, MAX_LABEL, MAX_NODES, Severity, valid_id,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct IrMeta {
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lane {
    pub id: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNode {
    pub id: String,
    pub lane: String,
    pub col: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowEdge {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub variant: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub route: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowGroup {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub nodes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowIr {
    pub schema_version: u32,
    pub meta: IrMeta,
    pub lanes: Vec<Lane>,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    #[serde(default)]
    pub groups: Vec<WorkflowGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutIr {
    #[serde(default = "grid_mode")]
    pub mode: String,
    #[serde(default = "four")]
    pub cols: u32,
}

fn grid_mode() -> String {
    "grid".into()
}

fn four() -> u32 {
    4
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub row: Option<u32>,
    #[serde(default)]
    pub col: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    #[serde(rename = "kind", default)]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub wraps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub route: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureIr {
    pub schema_version: u32,
    pub meta: IrMeta,
    #[serde(default)]
    pub layout: LayoutIr,
    pub components: Vec<Component>,
    #[serde(default)]
    pub boundaries: Vec<Boundary>,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

impl Default for LayoutIr {
    fn default() -> Self {
        Self {
            mode: grid_mode(),
            cols: four(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ir {
    Workflow(WorkflowIr),
    Architecture(ArchitectureIr),
}

impl Ir {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| "the IR must be a JSON object".to_string())?;
        let kind = object
            .remove("diagram_type")
            .and_then(|v| v.as_str().map(str::to_string))
            .ok_or_else(|| "diagram_type is required".to_string())?;
        match kind.as_str() {
            "workflow" => serde_json::from_value(value)
                .map(Ir::Workflow)
                .map_err(|e| format!("invalid workflow IR: {e}")),
            "architecture" => serde_json::from_value(value)
                .map(Ir::Architecture)
                .map_err(|e| format!("invalid architecture IR: {e}")),
            other => Err(format!("unsupported diagram_type {other:?}")),
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Ir::Workflow(w) => &w.meta.title,
            Ir::Architecture(a) => &a.meta.title,
        }
    }
}

pub fn validate(ir: &Ir) -> Vec<Issue> {
    match ir {
        Ir::Workflow(w) => validate_workflow(w),
        Ir::Architecture(a) => validate_architecture(a),
    }
}

fn validate_workflow(w: &WorkflowIr) -> Vec<Issue> {
    let mut issues = Vec::new();
    if w.schema_version != 1 {
        issues.push(Issue::error(
            CODE_UNSUPPORTED,
            "schema_version",
            format!("unsupported schema version {}", w.schema_version),
        ));
    }
    if w.nodes.len() > MAX_NODES || w.edges.len() > MAX_EDGES {
        issues.push(Issue::error(
            CODE_TOO_MANY,
            "nodes",
            "the workflow has too many elements",
        ));
    }
    let mut lanes = std::collections::HashSet::new();
    for (i, lane) in w.lanes.iter().enumerate() {
        if !valid_id(&lane.id) {
            issues.push(Issue::error(
                CODE_INVALID_ID,
                format!("lanes[{i}].id"),
                "invalid lane id",
            ));
        } else if !lanes.insert(lane.id.as_str()) {
            issues.push(Issue::error(
                CODE_DUPLICATE_ID,
                format!("lanes[{i}].id"),
                "duplicate lane id",
            ));
        }
        if lane.label.trim().is_empty() {
            issues.push(Issue::warning(
                CODE_EMPTY_LABEL,
                format!("lanes[{i}].label"),
                "the lane has no label",
            ));
        }
    }
    let mut ids = std::collections::HashSet::new();
    for (i, node) in w.nodes.iter().enumerate() {
        if !valid_id(&node.id) {
            issues.push(Issue::error(
                CODE_INVALID_ID,
                format!("nodes[{i}].id"),
                "invalid node id",
            ));
        } else if !ids.insert(node.id.as_str()) {
            issues.push(Issue::error(
                CODE_DUPLICATE_ID,
                format!("nodes[{i}].id"),
                "duplicate node id",
            ));
        }
        if node.label.trim().is_empty() {
            issues.push(Issue::warning(
                CODE_EMPTY_LABEL,
                format!("nodes[{i}].label"),
                "the node has no label",
            ));
        }
        if node.label.chars().count() > MAX_LABEL {
            issues.push(Issue::warning(
                CODE_OUT_OF_RANGE,
                format!("nodes[{i}].label"),
                "the node label is too long",
            ));
        }
        if !lanes.contains(node.lane.as_str()) {
            issues.push(Issue::error(
                CODE_UNKNOWN_LANE,
                format!("nodes[{i}].lane"),
                format!("unknown lane {:?}", node.lane),
            ));
        }
        if node.col > 5 {
            issues.push(Issue::error(
                CODE_OUT_OF_RANGE,
                format!("nodes[{i}].col"),
                "the column is out of range",
            ));
        }
    }
    for (i, edge) in w.edges.iter().enumerate() {
        for (side, id) in [("from", &edge.from), ("to", &edge.to)] {
            if !ids.contains(id.as_str()) {
                issues.push(Issue::error(
                    CODE_UNKNOWN_NODE,
                    format!("edges[{i}].{side}"),
                    format!("unknown node {id:?}"),
                ));
            }
        }
    }
    for (i, group) in w.groups.iter().enumerate() {
        if !valid_id(&group.id) {
            issues.push(Issue::error(
                CODE_INVALID_ID,
                format!("groups[{i}].id"),
                "invalid group id",
            ));
        }
        for id in &group.nodes {
            if !ids.contains(id.as_str()) {
                issues.push(Issue::error(
                    CODE_UNKNOWN_NODE,
                    format!("groups[{i}].nodes"),
                    format!("unknown node {id:?}"),
                ));
            }
        }
    }
    issues
}

fn validate_architecture(a: &ArchitectureIr) -> Vec<Issue> {
    let mut issues = Vec::new();
    if a.schema_version != 1 {
        issues.push(Issue::error(
            CODE_UNSUPPORTED,
            "schema_version",
            format!("unsupported schema version {}", a.schema_version),
        ));
    }
    if a.components.len() > MAX_NODES
        || a.connections.len() > MAX_EDGES
        || a.boundaries.len() > MAX_GROUPS
    {
        issues.push(Issue::error(
            CODE_TOO_MANY,
            "components",
            "the architecture has too many elements",
        ));
    }
    if a.layout.cols == 0 || a.layout.cols > 12 {
        issues.push(Issue::error(
            CODE_OUT_OF_RANGE,
            "layout.cols",
            "the column count is out of range",
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for (i, component) in a.components.iter().enumerate() {
        if !valid_id(&component.id) {
            issues.push(Issue::error(
                CODE_INVALID_ID,
                format!("components[{i}].id"),
                "invalid component id",
            ));
        } else if !ids.insert(component.id.as_str()) {
            issues.push(Issue::error(
                CODE_DUPLICATE_ID,
                format!("components[{i}].id"),
                "duplicate component id",
            ));
        }
        if component.label.trim().is_empty() {
            issues.push(Issue::warning(
                CODE_EMPTY_LABEL,
                format!("components[{i}].label"),
                "the component has no label",
            ));
        }
        if let Some(col) = component.col
            && col > 11
        {
            issues.push(Issue::error(
                CODE_OUT_OF_RANGE,
                format!("components[{i}].col"),
                "the column is out of range",
            ));
        }
    }
    for (i, boundary) in a.boundaries.iter().enumerate() {
        if boundary.wraps.is_empty() {
            issues.push(Issue::error(
                CODE_REQUIRED,
                format!("boundaries[{i}].wraps"),
                "a boundary must wrap at least one component",
            ));
        }
        for id in &boundary.wraps {
            if !ids.contains(id.as_str()) {
                issues.push(Issue::error(
                    CODE_UNKNOWN_BOUNDARY,
                    format!("boundaries[{i}].wraps"),
                    format!("unknown component {id:?}"),
                ));
            }
        }
    }
    for (i, connection) in a.connections.iter().enumerate() {
        for (side, id) in [("from", &connection.from), ("to", &connection.to)] {
            if !ids.contains(id.as_str()) {
                issues.push(Issue::error(
                    CODE_UNKNOWN_NODE,
                    format!("connections[{i}].{side}"),
                    format!("unknown component {id:?}"),
                ));
            }
        }
    }
    issues
}

/// True when the document is valid enough to render; warnings are allowed.
pub fn is_valid(issues: &[Issue]) -> bool {
    !issues.iter().any(|i| i.severity == Severity::Error)
}
