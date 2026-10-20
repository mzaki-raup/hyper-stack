//! Offline templates. They are ordinary `DiagramDoc`s, so the generator's
//! fallback and the editor's New Diagram use the same path.

use crate::convert::chain;
use crate::model::{DiagramDoc, DiagramKind, Edge, EdgeRoute, Group, Node, NodeKind, Style};

pub fn flow() -> DiagramDoc {
    let mut doc = DiagramDoc::new(DiagramKind::Flow, "Checkout flow");
    doc.subtitle = "A small order flow: cart, stock check, payment, done.".into();
    doc.auto_layout = true;
    let nodes = [
        ("start", "Start", NodeKind::StartEnd),
        ("cart", "Add to cart", NodeKind::Process),
        ("stock", "In stock?", NodeKind::Decision),
        ("pay", "Payment", NodeKind::Process),
        ("cancel", "Cancelled", NodeKind::StartEnd),
        ("done", "Order placed", NodeKind::StartEnd),
    ];
    for (id, label, kind) in nodes {
        doc.nodes.push(Node {
            id: id.into(),
            label: label.into(),
            kind,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            style: Style::default(),
        });
    }
    chain(&mut doc, &[("start", "cart"), ("cart", "stock")]);
    doc.edges.push(Edge {
        id: "e3".into(),
        from: "stock".into(),
        to: "pay".into(),
        label: "Yes".into(),
        route: EdgeRoute::Orthogonal,
        arrow: crate::model::Arrow::End,
        dashed: false,
        via: Vec::new(),
    });
    doc.edges.push(Edge {
        id: "e4".into(),
        from: "pay".into(),
        to: "done".into(),
        label: String::new(),
        route: EdgeRoute::Orthogonal,
        arrow: crate::model::Arrow::End,
        dashed: false,
        via: Vec::new(),
    });
    doc.edges.push(Edge {
        id: "e5".into(),
        from: "stock".into(),
        to: "cancel".into(),
        label: "No".into(),
        route: EdgeRoute::Orthogonal,
        arrow: crate::model::Arrow::End,
        dashed: true,
        via: Vec::new(),
    });
    doc
}

pub fn architecture() -> DiagramDoc {
    let mut doc = DiagramDoc::new(DiagramKind::Architecture, "Service architecture");
    doc.subtitle = "A client, an API, a store and a queue behind one boundary.".into();
    doc.auto_layout = false;
    let nodes = [
        ("web", "Client", NodeKind::Frontend, 0.0, 0.0),
        ("api", "API", NodeKind::Backend, 240.0, 0.0),
        ("db", "Database", NodeKind::Database, 160.0, 160.0),
        ("queue", "Events", NodeKind::Queue, 400.0, 160.0),
    ];
    for (id, label, kind, x, y) in nodes {
        doc.nodes.push(Node {
            id: id.into(),
            label: label.into(),
            kind,
            x,
            y,
            w: 160.0,
            h: 64.0,
            style: Style::default(),
        });
    }
    doc.groups.push(Group {
        id: "vpc".into(),
        label: "VPC".into(),
        nodes: vec!["api".into(), "db".into(), "queue".into()],
        style: Style::default(),
    });
    doc.edges.push(Edge {
        id: "c1".into(),
        from: "web".into(),
        to: "api".into(),
        label: "HTTPS".into(),
        route: EdgeRoute::Straight,
        arrow: crate::model::Arrow::End,
        dashed: false,
        via: Vec::new(),
    });
    doc.edges.push(Edge {
        id: "c2".into(),
        from: "api".into(),
        to: "db".into(),
        label: "SQL".into(),
        route: EdgeRoute::Orthogonal,
        arrow: crate::model::Arrow::End,
        dashed: false,
        via: Vec::new(),
    });
    doc.edges.push(Edge {
        id: "c3".into(),
        from: "api".into(),
        to: "queue".into(),
        label: "events".into(),
        route: EdgeRoute::Orthogonal,
        arrow: crate::model::Arrow::End,
        dashed: true,
        via: Vec::new(),
    });
    doc
}

pub fn erd() -> DiagramDoc {
    let mut doc = DiagramDoc::new(DiagramKind::Erd, "Order data model");
    doc.subtitle = "Two entities and a one-to-many relationship.".into();
    doc.auto_layout = true;
    for (id, label, kind) in [
        (
            "customers",
            "customers\nid UUID PK\nemail TEXT\ncreated_at TIMESTAMP",
            NodeKind::Container,
        ),
        (
            "orders",
            "orders\nid UUID PK\ncustomer_id UUID FK\ntotal NUMERIC",
            NodeKind::Container,
        ),
    ] {
        doc.nodes.push(Node {
            id: id.into(),
            label: label.into(),
            kind,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            style: Style::default(),
        });
    }
    chain(&mut doc, &[("customers", "orders")]);
    if let Some(edge) = doc.edges.first_mut() {
        edge.label = "1 .. *".into();
        edge.route = EdgeRoute::Straight;
    }
    doc
}

pub fn sequence() -> DiagramDoc {
    let mut doc = DiagramDoc::new(DiagramKind::Sequence, "Checkout sequence");
    doc.subtitle = "A client request, the API and the database.".into();
    doc.auto_layout = true;
    for (id, label, kind) in [
        ("client", "Client", NodeKind::Frontend),
        ("api", "API", NodeKind::Backend),
        ("db", "Database", NodeKind::Database),
    ] {
        doc.nodes.push(Node {
            id: id.into(),
            label: label.into(),
            kind,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            style: Style::default(),
        });
    }
    chain(&mut doc, &[("client", "api"), ("api", "db")]);
    if let Some(edge) = doc.edges.first_mut() {
        edge.label = "POST /orders".into();
        edge.route = EdgeRoute::Straight;
    }
    if let Some(edge) = doc.edges.get_mut(1) {
        edge.label = "INSERT order".into();
        edge.route = EdgeRoute::Straight;
        edge.dashed = true;
    }
    doc
}
