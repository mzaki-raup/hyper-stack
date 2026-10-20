use hs_diagram::convert::convert;
use hs_diagram::io;
use hs_diagram::layout;
use hs_diagram::model::{DiagramDoc, DiagramKind, Node, NodeKind};
use hs_diagram::schema::Ir;
use hs_diagram::svg;
use hs_diagram::template;
use hs_diagram::text::approx_measure;
use hs_diagram::validate;

#[test]
fn flow_template_lays_out_deterministically() {
    let doc = template::flow();
    assert!(validate::is_valid(&validate::validate(&doc)));
    let a = layout::layout(&doc, &approx_measure);
    let b = layout::layout(&doc, &approx_measure);
    assert_eq!(a, b);
    assert!(a.size.w > 150.0 && a.size.h > 400.0, "{:?}", a.size);
    assert!(a.prims.iter().any(|p| matches!(
        p,
        hs_diagram::Prim::Shape { label, .. } if label == "Start"
    )));
}

#[test]
fn architecture_template_uses_its_geometry() {
    let doc = template::architecture();
    let issues = validate::validate(&doc);
    assert!(validate::is_valid(&issues), "{issues:?}");
    let scene = layout::layout(&doc, &approx_measure);
    assert!(scene.prims.iter().any(|p| matches!(
        p,
        hs_diagram::Prim::Frame { label, .. } if label == "VPC"
    )));
}

#[test]
fn validation_reports_unknown_and_duplicate_ids() {
    let mut doc = DiagramDoc::new(DiagramKind::Flow, "Bad");
    doc.nodes.push(Node {
        id: "one".into(),
        label: "One".into(),
        kind: NodeKind::Process,
        ..Default::default()
    });
    doc.nodes.push(Node {
        id: "one".into(),
        label: "Again".into(),
        kind: NodeKind::Process,
        ..Default::default()
    });
    doc.edges.push(hs_diagram::Edge {
        id: "e1".into(),
        from: "one".into(),
        to: "missing".into(),
        ..Default::default()
    });
    let issues = validate::validate(&doc);
    let codes: Vec<_> = issues.iter().map(|i| i.code).collect();
    assert!(codes.contains(&validate::CODE_DUPLICATE_ID));
    assert!(codes.contains(&validate::CODE_UNKNOWN_NODE));
}

#[test]
fn svg_escapes_user_text() {
    let mut doc = template::flow();
    doc.nodes[0].label = "A & B <C>".into();
    let scene = layout::layout(&doc, &approx_measure);
    let out = svg::to_svg(&scene, &svg::SvgOptions::default());
    assert!(out.contains("A &amp; B &lt;C&gt;"));
    assert!(!out.contains("A & B <C>"));
}

#[test]
fn workflow_ir_parses_and_converts() {
    let text = r#"{
      "schema_version": 1,
      "diagram_type": "workflow",
      "meta": { "title": "Checkout", "subtitle": "" },
      "lanes": [{ "id": "client", "label": "Client" }],
      "nodes": [
        { "id": "browser", "lane": "client", "col": 0, "type": "frontend", "label": "Browser" },
        { "id": "api", "lane": "client", "col": 1, "type": "backend", "label": "API" }
      ],
      "edges": [{ "from": "browser", "to": "api", "label": "HTTPS", "variant": "default", "role": "main", "route": "auto" }],
      "groups": []
    }"#;
    let ir = Ir::parse(text).unwrap();
    let issues = hs_diagram::schema::validate(&ir);
    assert!(hs_diagram::schema::is_valid(&issues), "{issues:?}");
    let doc = convert(&ir);
    assert_eq!(doc.title, "Checkout");
    assert_eq!(doc.nodes.len(), 2);
    assert_eq!(doc.groups[0].label, "Client");
    assert!(layout::layout(&doc, &approx_measure).size.w > 100.0);
}

#[test]
fn architecture_ir_parses_and_places_components() {
    let text = r#"{
      "schema_version": 1,
      "diagram_type": "architecture",
      "meta": { "title": "Production" },
      "layout": { "mode": "grid", "cols": 3 },
      "components": [
        { "id": "web", "type": "frontend", "label": "Web", "row": 0, "col": 0 },
        { "id": "api", "type": "backend", "label": "API", "row": 0, "col": 1 },
        { "id": "db", "type": "database", "label": "DB", "row": 1, "col": 1 }
      ],
      "boundaries": [{ "kind": "security-group", "label": "VPC", "wraps": ["api", "db"] }],
      "connections": [{ "from": "web", "to": "api", "label": "HTTPS", "route": "orthogonal-h" }]
    }"#;
    let ir = Ir::parse(text).unwrap();
    assert!(hs_diagram::schema::is_valid(&hs_diagram::schema::validate(
        &ir
    )));
    let doc = convert(&ir);
    assert_eq!(doc.nodes.len(), 3);
    assert_eq!(doc.nodes[1].x, 236.0);
    assert_eq!(doc.nodes[2].y, 112.0);
    assert_eq!(doc.groups[0].label, "VPC");
    assert_eq!(doc.edges[0].label, "HTTPS");
}

#[test]
fn ir_rejects_unknown_fields() {
    let text = r#"{"diagram_type":"workflow","nodes":[],"surprise":true}"#;
    let err = Ir::parse(text).unwrap_err();
    assert!(err.contains("unknown field"), "{err}");
}

#[test]
fn adapted_schemas_are_valid_json() {
    let docs = [
        include_str!("../schema/archify/workflow.schema.json"),
        include_str!("../schema/archify/architecture.schema.json"),
        include_str!("../schema/archify/common.schema.json"),
    ];
    for text in docs {
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(value["title"].as_str().is_some());
        assert!(value["$comment"].as_str().is_some());
    }
    for text in docs {
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        if value.get("additionalProperties").is_some() {
            assert_eq!(value["additionalProperties"], false);
        }
    }
}

#[test]
fn hdiag_round_trips_through_disk() {
    let doc = template::flow();
    let dir = std::env::temp_dir().join(format!("hs-diagram-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("flow.hdiag");
    io::save(&path, &doc).unwrap();
    let (back, issues) = io::load(&path).unwrap();
    assert_eq!(back, doc);
    assert!(validate::is_valid(&issues));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn erd_and_sequence_templates_validate_and_lay_out() {
    for doc in [
        hs_diagram::template::erd(),
        hs_diagram::template::sequence(),
    ] {
        let issues = hs_diagram::validate::validate(&doc);
        assert!(
            issues
                .iter()
                .all(|i| i.severity != hs_diagram::Severity::Error),
            "{:?}: {issues:?}",
            doc.kind
        );
        let scene = hs_diagram::layout_approx(&doc);
        assert_eq!(
            scene
                .prims
                .iter()
                .filter(|p| matches!(p, hs_diagram::Prim::Shape { .. }))
                .count(),
            doc.nodes.len()
        );
    }
}
