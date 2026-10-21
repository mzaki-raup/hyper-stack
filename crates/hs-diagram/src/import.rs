//! Importers for the Release 2 interchange formats.
//!
//! Archify JSON goes through the same strict IR and validator the generator
//! uses, so an import can never bypass the schema. draw.io XML is read from
//! the uncompressed `mxGraphModel` form: vertices become nodes at their own
//! geometry, edges become connections, and the first page is the one imported.

use base64::Engine as _;
use std::io::Read;

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

/// Import a Mermaid `sequenceDiagram` into the basic sequence document.
pub fn mermaid_sequence(text: &str) -> Result<DiagramDoc, String> {
    let mut doc = DiagramDoc::new(
        crate::model::DiagramKind::Sequence,
        "Imported Mermaid sequence",
    );
    let mut labels: std::collections::HashMap<String, String> = Default::default();
    let mut order: Vec<String> = Vec::new();
    let mut messages = 0usize;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with("sequenceDiagram")
            || line.starts_with("autonumber")
            || line.starts_with("title ")
            || line.starts_with("Note ")
            || line.starts_with("note ")
            || line.starts_with("%%")
        {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("participant ")
            .or_else(|| line.strip_prefix("actor "))
        {
            let (id, label) = match rest.split_once(" as ") {
                Some((id, label)) => (id.trim(), label.trim()),
                None => (rest.trim(), rest.trim()),
            };
            let id = valid_id(id);
            labels
                .entry(id.clone())
                .or_insert_with(|| label.to_string());
            if !order.contains(&id) {
                order.push(id);
            }
            continue;
        }
        let Some((head, label)) = line.split_once(':') else {
            continue;
        };
        let Some((left, arrow, right)) = split_arrow(head) else {
            continue;
        };
        let left = valid_id(left.trim());
        let right = valid_id(right.trim());
        if left.is_empty() || right.is_empty() || left == right {
            continue;
        }
        for id in [&left, &right] {
            if !order.contains(id) {
                order.push(id.clone());
                labels.entry(id.clone()).or_insert_with(|| id.clone());
            }
        }
        messages += 1;
        doc.edges.push(Edge {
            id: format!("m{messages}"),
            from: left,
            to: right,
            label: label.trim().to_string(),
            route: EdgeRoute::Straight,
            arrow: Arrow::End,
            dashed: arrow.starts_with("--"),
            dotted: false,
            via: Vec::new(),
            style: Style::default(),
            label_at: None,
            cardinality: None,
        });
    }
    if messages == 0 {
        return Err("the Mermaid sequence has no messages".into());
    }
    for id in order {
        doc.nodes.push(Node {
            id: id.clone(),
            label: labels.remove(&id).unwrap_or(id),
            kind: NodeKind::Backend,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            style: Style::default(),
            erd: None,
        });
    }
    Ok(doc)
}

fn split_arrow(head: &str) -> Option<(&str, &str, &str)> {
    for arrow in ["-->>", "->>", "--x", "-x", "-->", "->", "--)", "-)"] {
        if let Some(i) = head.find(arrow) {
            return Some((&head[..i], arrow, &head[i + arrow.len()..]));
        }
    }
    None
}
/// One page of a draw.io document, already decoded.
#[derive(Debug, Clone)]
pub struct DrawioPage {
    pub name: String,
    pub xml: String,
}

/// The pages in a draw.io document, decoded where they are compressed.
pub fn drawio_pages(text: &str) -> Result<Vec<DrawioPage>, String> {
    let doc = roxmltree::Document::parse(text)
        .map_err(|e| format!("draw.io XML is not well formed: {e}"))?;
    let diagrams: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("diagram"))
        .collect();
    if diagrams.is_empty() {
        return Ok(vec![DrawioPage {
            name: "Page 1".into(),
            xml: text.to_string(),
        }]);
    }
    let mut pages = Vec::new();
    for (i, diagram) in diagrams.into_iter().enumerate() {
        let name = diagram
            .attribute("name")
            .map(str::to_string)
            .unwrap_or_else(|| format!("Page {}", i + 1));
        let xml = if diagram.descendants().any(|n| n.has_tag_name("mxCell")) {
            text[diagram.range()].to_string()
        } else if let Some(raw) = diagram.text().map(str::trim)
            && !raw.is_empty()
            && !raw.contains('<')
        {
            match decode_text(raw) {
                Some(xml) => xml,
                None => return Err("compressed .drawio page could not be decoded".into()),
            }
        } else {
            continue;
        };
        pages.push(DrawioPage { name, xml });
    }
    if pages.is_empty() {
        return Err("no draw.io pages found".into());
    }
    Ok(pages)
}

/// Import page `page` (zero-based) of a draw.io document.
pub fn drawio_xml_page(text: &str, page: usize) -> Result<DiagramDoc, String> {
    let pages = drawio_pages(text)?;
    let page = pages
        .get(page)
        .ok_or_else(|| format!("draw.io page {} does not exist", page + 1))?;
    drawio_doc(&page.xml)
}

/// Import the first page of a draw.io document.
pub fn drawio_xml(text: &str) -> Result<DiagramDoc, String> {
    drawio_xml_page(text, 0)
}

fn drawio_doc(text: &str) -> Result<DiagramDoc, String> {
    let doc = roxmltree::Document::parse(text)
        .map_err(|e| format!("draw.io XML is not well formed: {e}"))?;
    if !doc.descendants().any(|n| n.has_tag_name("mxCell"))
        && let Some(decoded) = decode_page(&doc)
    {
        return drawio_doc(&decoded);
    }
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
            erd: None,
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
            dotted: false,
            via: Vec::new(),
            style: Style::default(),
            label_at: None,
            cardinality: None,
        });
    }
    Ok(out)
}

fn decode_page(doc: &roxmltree::Document<'_>) -> Option<String> {
    let raw = doc
        .descendants()
        .find(|n| n.has_tag_name("diagram"))
        .and_then(|n| n.text())?
        .trim();
    decode_text(raw)
}

fn decode_text(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.contains('<') {
        return None;
    }
    let mut encoded = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            encoded.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            encoded.push(bytes[i]);
            i += 1;
        }
    }
    let packed = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let mut inflate = flate2::read::DeflateDecoder::new(packed.as_slice());
    let mut xml = String::new();
    inflate.read_to_string(&mut xml).ok()?;
    Some(xml)
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
    use std::io::Write;

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
    fn compressed_drawio_is_decoded() {
        let inner = r#"<mxGraphModel><root><mxCell id="a" value="A" vertex="1" parent="0"><mxGeometry x="0" y="0" width="120" height="60" as="geometry"/></mxCell></root></mxGraphModel>"#;
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(inner.as_bytes()).unwrap();
        let packed = enc.finish().unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(packed);
        let xml = format!("<mxfile><diagram name=\"p\" id=\"x\">{encoded}</diagram></mxfile>");
        let doc = drawio_xml(&xml).unwrap();
        assert_eq!(doc.nodes.len(), 1);
        assert_eq!(doc.nodes[0].label, "A");
    }

    #[test]
    fn compressed_drawio_is_refused_with_a_clear_message() {
        let err = drawio_xml("<mxfile><diagram>N4Ig...</diagram></mxfile>").unwrap_err();
        assert!(err.contains("compressed"), "{err}");
    }

    /// A file with one inline page and one compressed page, as draw.io
    /// writes when pages were added at different times.
    fn two_pages() -> String {
        let inner = r#"<mxGraphModel><root><mxCell id="z" value="Second" vertex="1" parent="0"><mxGeometry x="0" y="0" width="120" height="60" as="geometry"/></mxCell></root></mxGraphModel>"#;
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(inner.as_bytes()).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(enc.finish().unwrap());
        format!(
            r#"<mxfile><diagram name="First" id="1"><mxGraphModel><root>
              <mxCell id="0"/><mxCell id="1" parent="0"/>
              <mxCell id="a" value="A" vertex="1" parent="1"><mxGeometry x="0" y="0" width="100" height="40" as="geometry"/></mxCell>
              <mxCell id="b" value="B" vertex="1" parent="1"><mxGeometry x="200" y="0" width="100" height="40" as="geometry"/></mxCell>
            </root></mxGraphModel></diagram><diagram name="Second" id="2">{encoded}</diagram></mxfile>"#
        )
    }

    #[test]
    fn drawio_pages_lists_every_page_by_name() {
        let pages = drawio_pages(&two_pages()).unwrap();
        let names: Vec<&str> = pages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["First", "Second"]);
        // The compressed page was decoded on the way.
        assert!(pages[1].xml.contains("mxCell"));
        // A file with no <diagram> wrapper is one page.
        let bare = r#"<mxGraphModel><root><mxCell id="a" vertex="1" parent="0"><mxGeometry width="1" height="1" as="geometry"/></mxCell></root></mxGraphModel>"#;
        assert_eq!(drawio_pages(bare).unwrap().len(), 1);
    }

    #[test]
    fn a_chosen_page_is_imported_and_a_missing_one_is_refused() {
        let text = two_pages();
        let first = drawio_xml_page(&text, 0).unwrap();
        assert_eq!(first.nodes.len(), 2);
        let second = drawio_xml_page(&text, 1).unwrap();
        assert_eq!(second.nodes.len(), 1);
        assert_eq!(second.nodes[0].label, "Second");
        // The first page is what the plain import takes.
        assert_eq!(drawio_xml(&text).unwrap().nodes.len(), 2);
        let err = drawio_xml_page(&text, 5).unwrap_err();
        assert!(err.contains("page 6"), "{err}");
        let junk = "<mxfile><diagram name=\"x\">N4Ig...</diagram></mxfile>";
        assert!(drawio_pages(junk).unwrap_err().contains("decoded"));
    }
}

#[test]
fn mermaid_sequence_imports_messages() {
    let doc = mermaid_sequence(
        "sequenceDiagram\n  participant C as Client\n  participant A as API\n  C->>A: POST /orders\n  A-->>C: 201 Created\n",
    )
    .unwrap();
    assert_eq!(doc.nodes.len(), 2);
    assert_eq!(doc.edges.len(), 2);
    assert_eq!(doc.edges[1].label, "201 Created");
    assert!(doc.edges[1].dashed);
}
