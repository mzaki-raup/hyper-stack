//! Structured schema metadata (docs/10-depth-programme.md phase H).
//!
//! [`driver::DbSession::introspect`] answers the schema browser: its
//! [`SchemaObject`](crate::driver::SchemaObject) carries a display string
//! and badges like `"FK → orders.id"`, which is what a tree needs and
//! nothing a generator can work from. The ER diagram, the DDL generator
//! and both compares need the parts separately, so the drivers also
//! answer [`TableSchema`] through `DbSession::table_schema`.
//!
//! The types stay engine-independent: a driver fills them from its own
//! catalog, and `ddlgen` writes them back out in a chosen dialect.

use crate::driver::{DbSession, Engine, ObjectKind, Result, Scope};
use serde::{Deserialize, Serialize};

/// One column of a table, in ordinal order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    /// The server's own spelling: `character varying(32)`, `int(11)`,
    /// `TEXT`. Compared as text, so a type change shows up as a
    /// difference without this module knowing the type systems.
    pub data_type: String,
    pub nullable: bool,
    /// The default as the server reports it, already a SQL expression.
    pub default: Option<String>,
    /// `SERIAL`, `AUTO_INCREMENT`, `AUTOINCREMENT`.
    pub auto_increment: bool,
}

/// An index, primary-key indexes included.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDef {
    pub name: String,
    /// Columns in index order.
    pub columns: Vec<String>,
    pub unique: bool,
    /// The index backing the primary key. Listed so a compare can see it,
    /// skipped by the DDL generator, which writes the key with the table.
    pub primary: bool,
}

/// A foreign key. Composite keys keep their column order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    /// `CASCADE`, `SET NULL`, … as the server spells it; `None` for the
    /// engine default (`NO ACTION`).
    pub on_delete: Option<String>,
    pub on_update: Option<String>,
}

/// One table with everything the phase-H features need.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableSchema {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnDef>,
    /// Primary-key columns in key order; empty when the table has none.
    pub primary_key: Vec<String>,
    pub indexes: Vec<IndexDef>,
    pub foreign_keys: Vec<ForeignKey>,
}

impl TableSchema {
    pub fn column(&self, name: &str) -> Option<&ColumnDef> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// `schema.table`, or just the table when the schema is empty (SQLite's
    /// `main`, which the generator leaves off).
    pub fn qualified(&self) -> String {
        if self.schema.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.schema, self.name)
        }
    }
}

/// Every table of one schema, as read from one connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub engine: Engine,
    /// Server version, so a report can say what it read.
    pub server: String,
    pub schema: String,
    /// Sorted by name, so two snapshots compare in one pass.
    pub tables: Vec<TableSchema>,
}

impl Snapshot {
    pub fn new(engine: Engine, server: impl Into<String>, schema: impl Into<String>) -> Self {
        Self {
            engine,
            server: server.into(),
            schema: schema.into(),
            tables: Vec::new(),
        }
    }

    pub fn table(&self, name: &str) -> Option<&TableSchema> {
        self.tables.iter().find(|t| t.name == name)
    }

    /// Keeps `tables` in the sorted order the compare relies on.
    pub fn sort(&mut self) {
        self.tables.sort_by(|a, b| a.name.cmp(&b.name));
    }
}

/// Reads every table of one schema. Table names come from the schema
/// browser's own listing, so a snapshot sees exactly what the tree does;
/// each table then costs one [`DbSession::table_schema`] round trip.
///
/// `cap` bounds how many tables are read, so pointing the ER diagram at a
/// schema with thousands of tables cannot hang the job. The caller is told
/// what the cap left out.
pub async fn read(sess: &mut dyn DbSession, schema: &str, cap: usize) -> Result<(Snapshot, usize)> {
    let engine = sess.engine();
    let server = sess.server_info().version.clone();
    let listed = sess
        .introspect(&Scope::Objects {
            schema: schema.to_string(),
            kind: ObjectKind::Table,
        })
        .await?;
    let skipped = listed.len().saturating_sub(cap);

    let mut snap = Snapshot::new(engine, server, schema);
    snap.tables = Vec::with_capacity(listed.len().min(cap));
    for obj in listed.iter().take(cap) {
        snap.tables
            .push(sess.table_schema(schema, &obj.name).await?);
    }
    snap.sort();
    Ok((snap, skipped))
}

/// Splits a server's `ON DELETE`/`ON UPDATE` word into `None` for the
/// default, so an engine that always reports `NO ACTION` and one that
/// reports nothing compare equal.
pub fn referential_action(word: &str) -> Option<String> {
    let w = word.trim();
    if w.is_empty() || w.eq_ignore_ascii_case("NO ACTION") || w.eq_ignore_ascii_case("RESTRICT") {
        None
    } else {
        Some(w.to_ascii_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            data_type: ty.into(),
            nullable: true,
            ..Default::default()
        }
    }

    #[test]
    fn qualified_leaves_off_an_empty_schema() {
        let mut t = TableSchema {
            name: "orders".into(),
            ..Default::default()
        };
        assert_eq!(t.qualified(), "orders");
        t.schema = "public".into();
        assert_eq!(t.qualified(), "public.orders");
    }

    #[test]
    fn column_is_found_by_name() {
        let t = TableSchema {
            columns: vec![col("id", "int"), col("total", "numeric")],
            ..Default::default()
        };
        assert_eq!(
            t.column("total").map(|c| c.data_type.as_str()),
            Some("numeric")
        );
        assert!(t.column("missing").is_none());
    }

    #[test]
    fn sort_orders_tables_by_name() {
        let mut s = Snapshot::new(Engine::Postgres, "PostgreSQL 16.4", "public");
        s.tables = vec![
            TableSchema {
                name: "orders".into(),
                ..Default::default()
            },
            TableSchema {
                name: "customers".into(),
                ..Default::default()
            },
        ];
        s.sort();
        let names: Vec<_> = s.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["customers", "orders"]);
    }

    #[test]
    fn the_default_referential_action_is_none() {
        assert_eq!(referential_action(""), None);
        assert_eq!(referential_action("NO ACTION"), None);
        assert_eq!(referential_action("restrict"), None);
        assert_eq!(referential_action("cascade"), Some("CASCADE".into()));
        assert_eq!(referential_action("set null"), Some("SET NULL".into()));
    }
}
