//! Importers for the Release 2 interchange formats.
//!
//! Archify JSON goes through the same strict IR and validator the generator
//! uses, so an import can never bypass the schema. draw.io XML is read from
//! the uncompressed `mxGraphModel` form: vertices become nodes at their own
//! geometry, edges become connections, and the first page is the one imported.

use crate::convert;
use crate::model::{Arrow, DiagramDoc, Edge, EdgeRoute, Node, NodeKind, Style};
use crate::schema::{Ir, validate};
use crate::{Severity, validate::is_valid};

/// Import an Archify workflow/architecture document.
pub fn archify_json(text: &str) -> Result<DiagramDoc, String> {
    let ir = Ir::parse(text)?;
    let issues = validate(&ir);
    if !is_valid(&issues) {
        let errors = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .take(4)
            .map(|i| format!("{} at {}", i.code, i.path))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("the Archify IR is invalid: {errors}"));
    }
    Ok(convert::convert(&ir))
}

/// Import the first page of an uncompressed draw.io document.
pub fn drawio_xml(text: &str) -> Result<DiagramDoc, String> {
    let doc = roxmltree::Document::parse(text)
        .map_err(|e| format!("draw.io XML is not well formed: {e}"))?;
    let mut out = DiagramDoc::new(crate::model::DiagramKind::Flow, "Imported draw.io diagram");
    out.auto_layout = false;
    let mut node_count = 0usize;
    for cell in doc.descendants().filter(|n| n.has_tag_name("mxCell")) {
        if cell.attribute("vertex") != Some("1") {
            continue;
        }
        let Some(id) = cell.attribute("id") else {
            continue;
        };
        let id = valid_id(id);
        if out.nodes.iter().any(|n| n.id == id) {
            continue;
        }
        let label = cell
            .attribute("value")
            .map(strip_html)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Node".to_string());
        let style = cell.attribute("style").unwrap_or("").to_ascii_lowercase();
        let kind = if style.contains("rhombus") {
            NodeKind::Decision
        } else if style.contains("ellipse")
            && (label.eq_ignore_ascii_case("start") || label.eq_ignore_ascii_case("end"))
        {
            NodeKind::StartEnd
        } else if style.contains("cylinder") {
            NodeKind::Database
        } else if style.contains("shape=note") {
            NodeKind::Note
        } else if style.contains("text") {
            NodeKind::Text
        } else {
            NodeKind::Process
        };
        let (x, y, w, h) = geometry(cell);
        out.nodes.push(Node {
            id,
            label,
            kind,
            x,
            y,
            w,
            h,
            style: Style::default(),
        });
        node_count += 1;
    }
    if node_count == 0 {
        return Err(
            "no draw.io vertices found (compressed .drawio pages are not supported yet)".into(),
        );
    }
    let ids: Vec<String> = out.nodes.iter().map(|n| n.id.clone()).collect();
    let mut edge_count = 0usize;
    for cell in doc.descendants().filter(|n| n.has_tag_name("mxCell")) {
        if cell.attribute("edge") != Some("1") {
            continue;
        }
        let (Some(from), Some(to)) = (cell.attribute("source"), cell.attribute("target")) else {
            continue;
        };
        let from = valid_id(from);
        let to = valid_id(to);
        if !ids.contains(&from) || !ids.contains(&to) || from == to {
            continue;
        }
        let label = cell.attribute("value").map(strip_html).unwrap_or_default();
        edge_count += 1;
        out.edges.push(Edge {
            id: format!("e{edge_count}"),
            from,
            to,
            label,
            route: EdgeRoute::Orthogonal,
            arrow: Arrow::End,
            dashed: false,
            via: Vec::new(),
        });
    }
    Ok(out)
}

fn geometry(cell: roxmltree::Node<'_, '_>) -> (f32, f32, f32, f32) {
    let g = cell.children().find(|n| n.has_tag_name("mxGeometry"));
    let num = |name: &str, fallback: f32| -> f32 {
        g.and_then(|g| g.attribute(name))
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(fallback)
    };
    (
        num("x", 0.0),
        num("y", 0.0),
        num("width", 120.0).max(40.0),
        num("height", 60.0).max(36.0),
    )
}

fn valid_id(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 1);
    for (i, c) in raw.chars().enumerate() {
        let ok = c.is_ascii_alphanumeric() || c == '_' || c == '-';
        if i == 0 && !c.is_ascii_alphabetic() {
            out.push('n');
        }
        out.push(if ok { c } else { '_' });
    }
    if out.is_empty() { "node".into() } else { out }
}

fn strip_html(raw: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARCHIFY: &str = r#"{
      "schema_version": 1,
      "diagram_type": "workflow",
      "meta": { "title": "Imported", "subtitle": "" },
      "lanes": [{ "id": "main", "label": "Main" }],
      "nodes": [
        { "id": "start", "lane": "main", "col": 0, "type": "start", "label": "Start" },
        { "id": "done", "lane": "main", "col": 1, "type": "end", "label": "Done" }
      ],
      "edges": [{ "from": "start", "to": "done" }]
    }"#;

    const DRAWIO: &str = r#"<mxfile><diagram name="p"><mxGraphModel><root>
      <mxCell id="0"/><mxCell id="1" parent="0"/>
      <mxCell id="a" value="Receive" vertex="1" style="rounded=1" parent="1">
        <mxGeometry x="10" y="20" width="140" height="60" as="geometry"/>
      </mxCell>
      <mxCell id="b" value="Done?" vertex="1" style="rhombus" parent="1">
        <mxGeometry x="220" y="20" width="120" height="80" as="geometry"/>
      </mxCell>
      <mxCell id="e" value="yes" edge="1" source="a" target="b" parent="1"/>
    </root></mxGraphModel></diagram></mxfile>"#;

    #[test]
    fn archify_json_goes_through_the_validator() {
        let doc = archify_json(ARCHIFY).unwrap();
        assert_eq!(doc.title, "Imported");
        assert_eq!(doc.nodes.len(), 2);
        assert_eq!(doc.edges.len(), 1);
    }

    #[test]
    fn drawio_vertices_and_edges_become_a_flow() {
        let doc = drawio_xml(DRAWIO).unwrap();
        assert_eq!(doc.nodes.len(), 2);
        assert_eq!(doc.nodes[1].kind, NodeKind::Decision);
        assert_eq!(doc.edges.len(), 1);
        assert_eq!(doc.edges[0].from, "a");
        assert!(!doc.auto_layout);
    }

    #[test]
    fn compressed_drawio_is_refused_with_a_clear_message() {
        let err = drawio_xml("<mxfile><diagram>N4Ig...</diagram></mxfile>").unwrap_err();
        assert!(err.contains("compressed"), "{err}");
    }
}
