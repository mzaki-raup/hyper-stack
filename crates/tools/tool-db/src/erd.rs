//! An ER diagram from a [`Snapshot`] (docs/10-depth-programme.md phase H).
//!
//! `hs_diagram::ddl::import` already builds an ERD by parsing DDL text, but
//! going schema → SQL → parser → diagram would throw away what the catalog
//! already knows and depend on the generator's output being re-parseable.
//! The catalog's structure maps onto `hs_diagram`'s model directly, so this
//! builds the document itself.
//!
//! The document is handed to the Graphics tool as an
//! [`OpenRequest::Text`](hs_core::tool::OpenRequest::Text) with
//! [`GRAPHICS_MIME`](hs_core::tool::GRAPHICS_MIME), so the two tools stay
//! uncoupled.

use hs_diagram::model::{
    Cardinality, Column, ColumnKey, DiagramDoc, DiagramKind, Edge, EdgeRoute, Entity, Node,
    NodeKind, Style,
};

use crate::schema::{Snapshot, TableSchema};

/// An id for a table node. Tables of one schema are unique by name, so the
/// name alone identifies the node; non-identifier characters are folded so
/// the id stays usable in the document's own references.
fn node_id(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    out
}

/// The key badge a column earns. A primary key wins over a foreign key, which
/// wins over a unique index, matching how `hs_diagram::ddl` reads DDL.
fn column_key(t: &TableSchema, name: &str) -> ColumnKey {
    if t.primary_key.iter().any(|c| c == name) {
        ColumnKey::Primary
    } else if t
        .foreign_keys
        .iter()
        .any(|fk| fk.columns.iter().any(|c| c == name))
    {
        ColumnKey::Foreign
    } else if t
        .indexes
        .iter()
        .any(|ix| ix.unique && ix.columns.len() == 1 && ix.columns[0] == name)
    {
        ColumnKey::Unique
    } else {
        ColumnKey::None
    }
}

/// Whether a key's own columns are unique on this side, which makes the
/// relationship one-to-one rather than one-to-many.
fn one_to_one(t: &TableSchema, columns: &[String]) -> bool {
    if columns.is_empty() {
        return false;
    }
    let same = |cols: &[String]| cols.len() == columns.len() && cols.iter().eq(columns.iter());
    same(&t.primary_key) || t.indexes.iter().any(|ix| ix.unique && same(&ix.columns))
}

/// Builds the ERD. Tables keep the snapshot's order, which [`Snapshot::sort`]
/// has already made alphabetical; layout is left to the editor.
pub fn build(snap: &Snapshot) -> DiagramDoc {
    let mut doc = DiagramDoc::new(DiagramKind::Erd, &snap.schema);
    doc.subtitle = format!("{} · {}", snap.server, snap.schema);
    doc.auto_layout = true;

    for t in &snap.tables {
        doc.nodes.push(Node {
            id: node_id(&t.name),
            label: t.name.clone(),
            kind: NodeKind::Entity,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            style: Style::default(),
            erd: Some(Entity {
                columns: t
                    .columns
                    .iter()
                    .map(|c| Column {
                        name: c.name.clone(),
                        data_type: c.data_type.clone(),
                        key: column_key(t, &c.name),
                    })
                    .collect(),
            }),
        });
    }

    // One edge per relationship. A key pointing outside the snapshot - another
    // schema, or a table the cap left out - has no node to reach, so it is
    // left out rather than drawn as a dangling edge.
    let present: std::collections::HashSet<&str> =
        snap.tables.iter().map(|t| t.name.as_str()).collect();
    let mut seen = std::collections::HashSet::new();
    for t in &snap.tables {
        for fk in &t.foreign_keys {
            if !present.contains(fk.ref_table.as_str()) {
                continue;
            }
            let (from, to) = (node_id(&t.name), node_id(&fk.ref_table));
            // A self-reference has nowhere to go; two keys between the same
            // pair of tables are one line.
            if from == to || !seen.insert((from.clone(), to.clone())) {
                continue;
            }
            // Unique key columns mean at most one row per parent; a nullable
            // one means it may also be none. Anything else is one-to-many.
            let optional = fk
                .columns
                .iter()
                .any(|c| t.column(c).is_some_and(|c| c.nullable));
            let cardinality = match (one_to_one(t, &fk.columns), optional) {
                (true, false) => Cardinality::One,
                (true, true) => Cardinality::ZeroOrOne,
                (false, _) => Cardinality::OneToMany,
            };
            doc.edges.push(Edge {
                id: format!("e{}", doc.edges.len() + 1),
                from,
                to,
                label: cardinality.label().to_string(),
                route: EdgeRoute::Straight,
                cardinality: Some(cardinality),
                ..Edge::default()
            });
        }
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Engine;
    use crate::schema::{ColumnDef, ForeignKey, IndexDef};

    fn col(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            data_type: "integer".into(),
            nullable: true,
            default: None,
            auto_increment: false,
        }
    }

    fn snap() -> Snapshot {
        let customers = TableSchema {
            schema: "public".into(),
            name: "customers".into(),
            columns: vec![col("id"), col("email")],
            primary_key: vec!["id".into()],
            indexes: vec![IndexDef {
                name: "customers_email".into(),
                columns: vec!["email".into()],
                unique: true,
                primary: false,
            }],
            foreign_keys: vec![],
        };
        let orders = TableSchema {
            schema: "public".into(),
            name: "orders".into(),
            columns: vec![col("id"), col("customer_id")],
            primary_key: vec!["id".into()],
            indexes: vec![],
            foreign_keys: vec![ForeignKey {
                name: "orders_customer".into(),
                columns: vec!["customer_id".into()],
                ref_schema: "public".into(),
                ref_table: "customers".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
        };
        let mut s = Snapshot::new(Engine::Postgres, "PostgreSQL 16.4", "public");
        s.tables = vec![customers, orders];
        s
    }

    #[test]
    fn every_table_becomes_an_entity_with_its_columns() {
        let doc = build(&snap());
        assert_eq!(doc.nodes.len(), 2);
        let orders = doc.nodes.iter().find(|n| n.label == "orders").unwrap();
        assert_eq!(orders.kind, NodeKind::Entity);
        let cols = &orders.erd.as_ref().unwrap().columns;
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].name, "id");
        assert_eq!(cols[0].key, ColumnKey::Primary);
        assert_eq!(cols[1].key, ColumnKey::Foreign);
    }

    #[test]
    fn a_unique_index_marks_its_column() {
        let doc = build(&snap());
        let customers = doc.nodes.iter().find(|n| n.label == "customers").unwrap();
        let email = &customers.erd.as_ref().unwrap().columns[1];
        assert_eq!(email.key, ColumnKey::Unique);
    }

    #[test]
    fn a_foreign_key_becomes_a_one_to_many_edge() {
        let doc = build(&snap());
        assert_eq!(doc.edges.len(), 1);
        assert_eq!(doc.edges[0].from, "orders");
        assert_eq!(doc.edges[0].to, "customers");
        assert_eq!(doc.edges[0].cardinality, Some(Cardinality::OneToMany));
    }

    #[test]
    fn a_unique_nullable_key_column_is_zero_or_one() {
        let mut s = snap();
        // Make orders.customer_id unique: at most one order per customer.
        s.tables[1].indexes.push(IndexDef {
            name: "orders_customer_uq".into(),
            columns: vec!["customer_id".into()],
            unique: true,
            primary: false,
        });
        let doc = build(&s);
        assert_eq!(doc.edges[0].cardinality, Some(Cardinality::ZeroOrOne));
        assert_eq!(doc.edges[0].label, "0 .. 1");
    }

    #[test]
    fn a_unique_mandatory_key_column_is_exactly_one() {
        let mut s = snap();
        s.tables[1].indexes.push(IndexDef {
            name: "orders_customer_uq".into(),
            columns: vec!["customer_id".into()],
            unique: true,
            primary: false,
        });
        s.tables[1].columns[1].nullable = false;
        let doc = build(&s);
        assert_eq!(doc.edges[0].cardinality, Some(Cardinality::One));
        assert_eq!(doc.edges[0].label, "1");
    }

    #[test]
    fn a_key_to_a_table_outside_the_snapshot_is_left_out() {
        let mut s = snap();
        s.tables[1].foreign_keys[0].ref_table = "elsewhere".into();
        assert!(build(&s).edges.is_empty());
    }

    #[test]
    fn a_self_reference_draws_no_edge() {
        let mut s = snap();
        s.tables[1].foreign_keys[0].ref_table = "orders".into();
        assert!(build(&s).edges.is_empty());
    }

    #[test]
    fn two_keys_between_the_same_tables_are_one_edge() {
        let mut s = snap();
        s.tables[1].foreign_keys.push(ForeignKey {
            name: "orders_shipto".into(),
            columns: vec!["ship_to".into()],
            ref_schema: "public".into(),
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        });
        assert_eq!(build(&s).edges.len(), 1);
    }

    #[test]
    fn the_id_is_folded_to_something_referenceable() {
        assert_eq!(node_id("Order Items"), "order_items");
        assert_eq!(node_id("tbl-2"), "tbl_2");
    }
}
