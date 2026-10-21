//! Validation for the editor document and the generator IR. Codes are stable
//! and map to Fluent messages in the UI; the core stays stringly so it has no
//! locale dependency.

use crate::model::DiagramDoc;

pub const CODE_REQUIRED: &str = "diagram/required";
pub const CODE_UNKNOWN_FIELD: &str = "diagram/unknown-field";
pub const CODE_INVALID_ID: &str = "diagram/invalid-id";
pub const CODE_DUPLICATE_ID: &str = "diagram/duplicate-id";
pub const CODE_UNKNOWN_NODE: &str = "diagram/unknown-node";
pub const CODE_UNKNOWN_LANE: &str = "diagram/unknown-lane";
pub const CODE_UNKNOWN_BOUNDARY: &str = "diagram/unknown-boundary-target";
pub const CODE_OUT_OF_RANGE: &str = "diagram/out-of-range";
pub const CODE_TOO_MANY: &str = "diagram/too-many-elements";
pub const CODE_EMPTY_LABEL: &str = "diagram/empty-label";
pub const CODE_UNSUPPORTED: &str = "diagram/unsupported-type";

pub const MAX_NODES: usize = 500;
pub const MAX_EDGES: usize = 1000;
pub const MAX_GROUPS: usize = 64;
pub const MAX_LABEL: usize = 500;
pub const MAX_TITLE: usize = 160;
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub code: &'static str,
    pub path: String,
    pub message: String,
    pub severity: Severity,
}

impl Issue {
    pub fn error(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
            severity: Severity::Error,
        }
    }

    pub fn warning(
        code: &'static str,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
            severity: Severity::Warning,
        }
    }
}

/// `^[a-zA-Z][a-zA-Z0-9_-]*$` without a regex dependency.
pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn is_valid(issues: &[Issue]) -> bool {
    !issues.iter().any(|i| i.severity == Severity::Error)
}

pub fn validate(doc: &DiagramDoc) -> Vec<Issue> {
    let mut issues = Vec::new();
    if doc.schema != crate::model::SCHEMA_VERSION {
        issues.push(Issue::error(
            CODE_UNSUPPORTED,
            "schema",
            format!("unsupported schema version {}", doc.schema),
        ));
    }
    if doc.nodes.len() > MAX_NODES {
        issues.push(Issue::error(
            CODE_TOO_MANY,
            "nodes",
            format!("{} nodes, the limit is {MAX_NODES}", doc.nodes.len()),
        ));
    }
    if doc.edges.len() > MAX_EDGES {
        issues.push(Issue::error(
            CODE_TOO_MANY,
            "edges",
            format!("{} edges, the limit is {MAX_EDGES}", doc.edges.len()),
        ));
    }
    if doc.groups.len() > MAX_GROUPS {
        issues.push(Issue::error(
            CODE_TOO_MANY,
            "groups",
            format!("{} groups, the limit is {MAX_GROUPS}", doc.groups.len()),
        ));
    }
    if doc.title.chars().count() > MAX_TITLE {
        issues.push(Issue::warning(
            CODE_OUT_OF_RANGE,
            "title",
            format!("the title is longer than {MAX_TITLE} characters"),
        ));
    }

    let mut node_ids = std::collections::HashSet::new();
    for (i, node) in doc.nodes.iter().enumerate() {
        let path = format!("nodes[{i}]");
        if node.id.is_empty() {
            issues.push(Issue::error(
                CODE_REQUIRED,
                format!("{path}.id"),
                "id is required",
            ));
        } else if !valid_id(&node.id) {
            issues.push(Issue::error(
                CODE_INVALID_ID,
                format!("{path}.id"),
                format!("invalid id {:?}", node.id),
            ));
        } else if !node_ids.insert(node.id.as_str()) {
            issues.push(Issue::error(
                CODE_DUPLICATE_ID,
                format!("{path}.id"),
                format!("duplicate node id {:?}", node.id),
            ));
        }
        if node.label.chars().count() > MAX_LABEL {
            issues.push(Issue::warning(
                CODE_OUT_OF_RANGE,
                format!("{path}.label"),
                format!("label is longer than {MAX_LABEL} characters"),
            ));
        }
        if node.label.trim().is_empty() {
            issues.push(Issue::warning(
                CODE_EMPTY_LABEL,
                format!("{path}.label"),
                "the node has no label",
            ));
        }
        if !node.x.is_finite() || !node.y.is_finite() || !node.w.is_finite() || !node.h.is_finite()
        {
            issues.push(Issue::error(
                CODE_OUT_OF_RANGE,
                path,
                "geometry must be finite",
            ));
        } else if !doc.auto_layout && (node.w <= 0.0 || node.h <= 0.0) {
            issues.push(Issue::error(
                CODE_OUT_OF_RANGE,
                path,
                "a fixed node needs a positive width and height",
            ));
        }
    }

    let mut edge_ids = std::collections::HashSet::new();
    for (i, edge) in doc.edges.iter().enumerate() {
        let path = format!("edges[{i}]");
        if edge.id.is_empty() {
            issues.push(Issue::error(
                CODE_REQUIRED,
                format!("{path}.id"),
                "id is required",
            ));
        } else if !valid_id(&edge.id) {
            issues.push(Issue::error(
                CODE_INVALID_ID,
                format!("{path}.id"),
                format!("invalid id {:?}", edge.id),
            ));
        } else if !edge_ids.insert(edge.id.as_str()) {
            issues.push(Issue::error(
                CODE_DUPLICATE_ID,
                format!("{path}.id"),
                format!("duplicate edge id {:?}", edge.id),
            ));
        }
        // An edge may start or end at a frame (group) as well as a node.
        for (side, id) in [("from", &edge.from), ("to", &edge.to)] {
            if !node_ids.contains(id.as_str()) && !doc.groups.iter().any(|g| &g.id == id) {
                issues.push(Issue::error(
                    CODE_UNKNOWN_NODE,
                    format!("{path}.{side}"),
                    format!("unknown node or frame {id:?}"),
                ));
            }
        }
        if edge.from == edge.to {
            issues.push(Issue::warning(
                CODE_OUT_OF_RANGE,
                path.clone(),
                "the edge starts and ends at the same node",
            ));
        }
    }

    let mut group_ids = std::collections::HashSet::new();
    for (i, group) in doc.groups.iter().enumerate() {
        let path = format!("groups[{i}]");
        if group.id.is_empty() {
            issues.push(Issue::error(
                CODE_REQUIRED,
                format!("{path}.id"),
                "id is required",
            ));
        } else if !valid_id(&group.id) {
            issues.push(Issue::error(
                CODE_INVALID_ID,
                format!("{path}.id"),
                format!("invalid id {:?}", group.id),
            ));
        } else if !group_ids.insert(group.id.as_str()) {
            issues.push(Issue::error(
                CODE_DUPLICATE_ID,
                format!("{path}.id"),
                format!("duplicate group id {:?}", group.id),
            ));
        }
        for id in &group.nodes {
            if !node_ids.contains(id.as_str()) {
                issues.push(Issue::error(
                    CODE_UNKNOWN_NODE,
                    format!("{path}.nodes"),
                    format!("unknown node {id:?}"),
                ));
            }
        }
    }
    issues
}
