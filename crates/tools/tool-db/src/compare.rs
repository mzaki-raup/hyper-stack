//! Schema and data compare (docs/10-depth-programme.md phase H).
//!
//! Both compares are one-directional: a **source** and a **target**, and
//! every verdict reads as "what the target would need to match the source".
//! That is the direction a migration script has to run in, and saying it one
//! way round stops the report meaning two things at once.
//!
//! The schema compare is pure: it takes two [`Snapshot`]s and returns the
//! differences, so it is tested without a server. The data compare needs
//! rows, so the module only prepares its statements and judges the rows the
//! caller fetched.

use crate::driver::Engine;
use crate::schema::{ColumnDef, ForeignKey, IndexDef, Snapshot, TableSchema};
use crate::sql::{quote_ident, quote_literal};

/// Which side an item is on, from the target's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    /// In the source, not in the target: the target needs it.
    New,
    /// In the target, not in the source.
    Missing,
    /// In both, but not the same.
    Differs,
}

impl Side {
    /// The Fluent key for the badge.
    pub fn label_key(self) -> &'static str {
        match self {
            Side::New => "db-cmp-new",
            Side::Missing => "db-cmp-missing",
            Side::Differs => "db-cmp-differs",
        }
    }
}

/// One difference, flat so the report is a table rather than a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub side: Side,
    pub table: String,
    /// The column, index or key the row is about; empty for the table itself.
    pub item: String,
    /// What kind of thing `item` names, for the report's second column.
    pub kind: DiffKind,
    /// Source and target spellings, shown side by side. Empty where the item
    /// is absent on that side.
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Table,
    Column,
    PrimaryKey,
    Index,
    ForeignKey,
}

impl DiffKind {
    pub fn label_key(self) -> &'static str {
        match self {
            DiffKind::Table => "db-cmp-table",
            DiffKind::Column => "db-cmp-column",
            DiffKind::PrimaryKey => "db-cmp-pk",
            DiffKind::Index => "db-cmp-index",
            DiffKind::ForeignKey => "db-cmp-fk",
        }
    }

    /// Orders a table's rows: the table itself, then columns, key, indexes,
    /// keys - the order a migration has to apply them in.
    fn cmp_key(self) -> u8 {
        match self {
            DiffKind::Table => 0,
            DiffKind::Column => 1,
            DiffKind::PrimaryKey => 2,
            DiffKind::Index => 3,
            DiffKind::ForeignKey => 4,
        }
    }
}

/// How a column reads in the report: the parts that make two columns differ.
fn column_summary(c: &ColumnDef) -> String {
    let mut s = c.data_type.clone();
    if !c.nullable {
        s.push_str(" NOT NULL");
    }
    if let Some(d) = &c.default {
        s.push_str(&format!(" DEFAULT {d}"));
    }
    if c.auto_increment {
        s.push_str(" AUTO INCREMENT");
    }
    s
}

fn index_summary(ix: &IndexDef) -> String {
    let unique = if ix.unique { "UNIQUE " } else { "" };
    format!("{unique}({})", ix.columns.join(", "))
}

fn fk_summary(fk: &ForeignKey) -> String {
    let mut s = format!(
        "({}) → {}({})",
        fk.columns.join(", "),
        fk.ref_table,
        fk.ref_columns.join(", ")
    );
    if let Some(a) = &fk.on_delete {
        s.push_str(&format!(" ON DELETE {a}"));
    }
    if let Some(a) = &fk.on_update {
        s.push_str(&format!(" ON UPDATE {a}"));
    }
    s
}

/// Compares two tables that exist on both sides.
fn compare_table(source: &TableSchema, target: &TableSchema, out: &mut Vec<Diff>) {
    let row = |side, kind, item: &str, s: String, t: String| Diff {
        side,
        table: source.name.clone(),
        item: item.to_string(),
        kind,
        source: s,
        target: t,
    };

    for c in &source.columns {
        match target.column(&c.name) {
            None => out.push(row(
                Side::New,
                DiffKind::Column,
                &c.name,
                column_summary(c),
                String::new(),
            )),
            Some(t) if t != c => out.push(row(
                Side::Differs,
                DiffKind::Column,
                &c.name,
                column_summary(c),
                column_summary(t),
            )),
            Some(_) => {}
        }
    }
    for c in &target.columns {
        if source.column(&c.name).is_none() {
            out.push(row(
                Side::Missing,
                DiffKind::Column,
                &c.name,
                String::new(),
                column_summary(c),
            ));
        }
    }

    if source.primary_key != target.primary_key {
        out.push(row(
            Side::Differs,
            DiffKind::PrimaryKey,
            "",
            source.primary_key.join(", "),
            target.primary_key.join(", "),
        ));
    }

    // The key's own index is implied by the key, which is compared above.
    let real = |ix: &&IndexDef| !ix.primary;
    for ix in source.indexes.iter().filter(real) {
        match target.indexes.iter().find(|t| t.name == ix.name) {
            None => out.push(row(
                Side::New,
                DiffKind::Index,
                &ix.name,
                index_summary(ix),
                String::new(),
            )),
            Some(t) if t.columns != ix.columns || t.unique != ix.unique => out.push(row(
                Side::Differs,
                DiffKind::Index,
                &ix.name,
                index_summary(ix),
                index_summary(t),
            )),
            Some(_) => {}
        }
    }
    for ix in target.indexes.iter().filter(real) {
        if !source.indexes.iter().any(|s| s.name == ix.name) {
            out.push(row(
                Side::Missing,
                DiffKind::Index,
                &ix.name,
                String::new(),
                index_summary(ix),
            ));
        }
    }

    // Keys are matched on their columns, not their names: a key the two
    // servers named differently is still the same relationship, and SQLite
    // does not name keys at all.
    let same =
        |a: &ForeignKey, b: &ForeignKey| a.columns == b.columns && a.ref_table == b.ref_table;
    for fk in &source.foreign_keys {
        match target.foreign_keys.iter().find(|t| same(fk, t)) {
            None => out.push(row(
                Side::New,
                DiffKind::ForeignKey,
                &fk.columns.join(", "),
                fk_summary(fk),
                String::new(),
            )),
            Some(t) if fk_summary(t) != fk_summary(fk) => out.push(row(
                Side::Differs,
                DiffKind::ForeignKey,
                &fk.columns.join(", "),
                fk_summary(fk),
                fk_summary(t),
            )),
            Some(_) => {}
        }
    }
    for fk in &target.foreign_keys {
        if !source.foreign_keys.iter().any(|s| same(s, fk)) {
            out.push(row(
                Side::Missing,
                DiffKind::ForeignKey,
                &fk.columns.join(", "),
                String::new(),
                fk_summary(fk),
            ));
        }
    }
}

/// Every difference between two snapshots, tables in name order.
pub fn schemas(source: &Snapshot, target: &Snapshot) -> Vec<Diff> {
    let mut out = Vec::new();
    for s in &source.tables {
        match target.table(&s.name) {
            Some(t) => compare_table(s, t, &mut out),
            None => out.push(Diff {
                side: Side::New,
                table: s.name.clone(),
                item: String::new(),
                kind: DiffKind::Table,
                source: format!("{} columns", s.columns.len()),
                target: String::new(),
            }),
        }
    }
    for t in &target.tables {
        if source.table(&t.name).is_none() {
            out.push(Diff {
                side: Side::Missing,
                table: t.name.clone(),
                item: String::new(),
                kind: DiffKind::Table,
                source: String::new(),
                target: format!("{} columns", t.columns.len()),
            });
        }
    }
    out.sort_by(|a, b| {
        a.table
            .cmp(&b.table)
            .then(a.kind.cmp_key().cmp(&b.kind.cmp_key()))
    });
    out
}

/// A migration script that would bring the target to the source.
///
/// Only the additive steps are written as SQL. A dropped column or table is
/// left as a commented line: the compare cannot know whether the target's
/// extra table is stale or simply not part of this schema, and generating
/// `DROP` statements for it would make running the script a data-loss risk
/// (docs/06-security.md §3). The user uncomments what they mean.
pub fn migration(source: &Snapshot, diffs: &[Diff], engine: Engine) -> String {
    use crate::ddlgen;
    let mut out = String::with_capacity(512);
    out.push_str("-- Generated by HyperStack: brings the target up to the source.\n");
    out.push_str("-- Review before running. Drops are commented out on purpose.\n\n");

    for d in diffs {
        match (d.side, d.kind) {
            (Side::New, DiffKind::Table) => {
                if let Some(t) = source.table(&d.table) {
                    out.push_str(&ddlgen::create_table(
                        t,
                        engine,
                        &ddlgen::Options::default(),
                    ));
                    out.push_str("\n\n");
                }
            }
            (Side::New, DiffKind::Column) => {
                if let Some(c) = source.table(&d.table).and_then(|t| t.column(&d.item)) {
                    let mut line = format!(
                        "ALTER TABLE {} ADD COLUMN {} {}",
                        quote_ident(&d.table, engine),
                        quote_ident(&c.name, engine),
                        c.data_type
                    );
                    if let Some(def) = &c.default {
                        line.push_str(&format!(" DEFAULT {def}"));
                    }
                    // An existing row has no value for a new NOT NULL column
                    // unless there is a default, so the constraint is left
                    // for the user to add once the column is filled.
                    if !c.nullable && c.default.is_some() {
                        line.push_str(" NOT NULL");
                    }
                    out.push_str(&format!("{line};\n"));
                    if !c.nullable && c.default.is_none() {
                        out.push_str(&format!(
                            "-- then, once {} is filled: ALTER TABLE {} ALTER COLUMN {} SET NOT NULL;\n",
                            c.name,
                            quote_ident(&d.table, engine),
                            quote_ident(&c.name, engine)
                        ));
                    }
                }
            }
            (Side::New, DiffKind::Index) => {
                if let Some((t, ix)) = source
                    .table(&d.table)
                    .and_then(|t| t.indexes.iter().find(|i| i.name == d.item).map(|i| (t, i)))
                    && let Some(sql) = ddlgen::create_index(t, ix, engine)
                {
                    out.push_str(&format!("{sql}\n"));
                }
            }
            (Side::New, DiffKind::ForeignKey) => {
                if let Some((t, fk)) = source.table(&d.table).and_then(|t| {
                    t.foreign_keys
                        .iter()
                        .find(|f| f.columns.join(", ") == d.item)
                        .map(|f| (t, f))
                }) && let Some(sql) = ddlgen::add_foreign_key(t, fk, engine)
                {
                    out.push_str(&format!("{sql}\n"));
                }
            }
            (Side::Differs, DiffKind::Column) => {
                out.push_str(&format!(
                    "-- {}.{} differs: source {} / target {}\n",
                    d.table, d.item, d.source, d.target
                ));
            }
            (Side::Differs, _) => {
                out.push_str(&format!(
                    "-- {} {} differs: source {} / target {}\n",
                    d.table, d.item, d.source, d.target
                ));
            }
            (Side::Missing, DiffKind::Table) => {
                out.push_str(&format!(
                    "-- target has {} and the source does not: DROP TABLE {};\n",
                    d.table,
                    quote_ident(&d.table, engine)
                ));
            }
            (Side::Missing, kind) => {
                out.push_str(&format!(
                    "-- target has {:?} {} on {} and the source does not\n",
                    kind, d.item, d.table
                ));
            }
            // A key is only ever reported as Differs: both sides always have
            // one, even if it is empty. Kept so the match stays total.
            (Side::New, DiffKind::PrimaryKey) => {
                out.push_str(&format!(
                    "-- {} primary key ({}) is not on the target\n",
                    d.table, d.source
                ));
            }
        }
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// The statement that reads one table for the data compare, ordered by the
/// primary key so two sides arrive in the same order and can be walked
/// together without holding either whole side in memory.
///
/// `limit` is the bound the UI shows: a data compare is a sample, never a
/// guarantee about the whole table.
pub fn data_query(t: &TableSchema, engine: Engine, limit: usize) -> Option<String> {
    if t.primary_key.is_empty() {
        return None;
    }
    let cols = t
        .columns
        .iter()
        .map(|c| quote_ident(&c.name, engine))
        .collect::<Vec<_>>()
        .join(", ");
    let order = t
        .primary_key
        .iter()
        .map(|c| quote_ident(c, engine))
        .collect::<Vec<_>>()
        .join(", ");
    let table = if engine == Engine::Sqlite || t.schema.is_empty() {
        quote_ident(&t.name, engine)
    } else {
        format!(
            "{}.{}",
            quote_ident(&t.schema, engine),
            quote_ident(&t.name, engine)
        )
    };
    Some(format!(
        "SELECT {cols} FROM {table} ORDER BY {order} LIMIT {limit}"
    ))
}

/// A row's verdict in the data compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowDiff {
    pub side: Side,
    /// The primary key, already rendered, so the report can name the row.
    pub key: String,
    /// Columns whose values differ, for a `Differs` row.
    pub columns: Vec<String>,
}

/// Walks two key-ordered samples together.
///
/// Both sides must be ordered by the same key and carry the same columns, as
/// [`data_query`] arranges. Keys are compared as the strings the servers sent,
/// which is why both sides are read with the same column order: it keeps two
/// engines' spellings of a number or a date lined up.
pub fn rows(
    source: &[(String, Vec<String>)],
    target: &[(String, Vec<String>)],
    columns: &[String],
) -> Vec<RowDiff> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < source.len() && j < target.len() {
        let (sk, sv) = &source[i];
        let (tk, tv) = &target[j];
        match sk.cmp(tk) {
            std::cmp::Ordering::Equal => {
                let differing: Vec<String> = sv
                    .iter()
                    .zip(tv.iter())
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(n, _)| {
                        columns
                            .get(n)
                            .cloned()
                            .unwrap_or_else(|| format!("column {}", n + 1))
                    })
                    .collect();
                if !differing.is_empty() {
                    out.push(RowDiff {
                        side: Side::Differs,
                        key: sk.clone(),
                        columns: differing,
                    });
                }
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => {
                out.push(RowDiff {
                    side: Side::New,
                    key: sk.clone(),
                    columns: Vec::new(),
                });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(RowDiff {
                    side: Side::Missing,
                    key: tk.clone(),
                    columns: Vec::new(),
                });
                j += 1;
            }
        }
    }
    for (k, _) in &source[i..] {
        out.push(RowDiff {
            side: Side::New,
            key: k.clone(),
            columns: Vec::new(),
        });
    }
    for (k, _) in &target[j..] {
        out.push(RowDiff {
            side: Side::Missing,
            key: k.clone(),
            columns: Vec::new(),
        });
    }
    out
}

/// Renders a row's primary key for [`rows`], joining a composite key with a
/// separator no identifier can contain.
pub fn row_key(values: &[String]) -> String {
    values.join("\u{1f}")
}

/// Turns the rows [`data_query`] returned into the keyed pairs [`rows`] walks.
///
/// The query selects `t.columns` in order, so a key column's position in the
/// row is its position in `t.columns`.
pub fn to_sample(t: &TableSchema, rows: &[crate::driver::Row]) -> Vec<(String, Vec<String>)> {
    let key_pos: Vec<usize> = t
        .primary_key
        .iter()
        .filter_map(|k| t.columns.iter().position(|c| &c.name == k))
        .collect();
    rows.iter()
        .map(|r| {
            let values: Vec<String> = r.iter().map(|v| v.display()).collect();
            let key = row_key(
                &key_pos
                    .iter()
                    .filter_map(|&i| values.get(i).cloned())
                    .collect::<Vec<_>>(),
            );
            (key, values)
        })
        .collect()
}

/// `INSERT`/`UPDATE` statements for the rows the target lacks or has wrong.
/// Deletions are commented, for the same reason as in [`migration`].
pub fn data_migration(
    t: &TableSchema,
    diffs: &[RowDiff],
    source: &[(String, Vec<String>)],
    engine: Engine,
) -> String {
    let mut out = String::with_capacity(256);
    out.push_str("-- Generated by HyperStack from a sampled compare.\n");
    out.push_str("-- Review before running: the sample is bounded, deletes are commented.\n\n");
    let table = if engine == Engine::Sqlite || t.schema.is_empty() {
        quote_ident(&t.name, engine)
    } else {
        format!(
            "{}.{}",
            quote_ident(&t.schema, engine),
            quote_ident(&t.name, engine)
        )
    };
    let find = |key: &str| source.iter().find(|(k, _)| k == key).map(|(_, v)| v);

    for d in diffs {
        match d.side {
            Side::New => {
                if let Some(values) = find(&d.key) {
                    let cols = t
                        .columns
                        .iter()
                        .map(|c| quote_ident(&c.name, engine))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let vals = values
                        .iter()
                        .map(|v| quote_literal(v, engine))
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!("INSERT INTO {table} ({cols}) VALUES ({vals});\n"));
                }
            }
            Side::Differs => {
                if let Some(values) = find(&d.key) {
                    let sets = d
                        .columns
                        .iter()
                        .filter_map(|name| {
                            let n = t.columns.iter().position(|c| &c.name == name)?;
                            Some(format!(
                                "{} = {}",
                                quote_ident(name, engine),
                                quote_literal(values.get(n)?, engine)
                            ))
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let whence = t
                        .primary_key
                        .iter()
                        .filter_map(|k| {
                            let n = t.columns.iter().position(|c| &c.name == k)?;
                            Some(format!(
                                "{} = {}",
                                quote_ident(k, engine),
                                quote_literal(values.get(n)?, engine)
                            ))
                        })
                        .collect::<Vec<_>>()
                        .join(" AND ");
                    if !sets.is_empty() && !whence.is_empty() {
                        out.push_str(&format!("UPDATE {table} SET {sets} WHERE {whence};\n"));
                    }
                }
            }
            Side::Missing => {
                out.push_str(&format!(
                    "-- target row {} is not in the source\n",
                    d.key.replace('\u{1f}', ", ")
                ));
            }
        }
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            data_type: ty.into(),
            nullable: true,
            default: None,
            auto_increment: false,
        }
    }

    fn table(name: &str, cols: Vec<ColumnDef>) -> TableSchema {
        TableSchema {
            schema: "public".into(),
            name: name.into(),
            primary_key: vec!["id".into()],
            columns: cols,
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
        }
    }

    fn snap(tables: Vec<TableSchema>) -> Snapshot {
        let mut s = Snapshot::new(Engine::Postgres, "PostgreSQL 16.4", "public");
        s.tables = tables;
        s.sort();
        s
    }

    #[test]
    fn identical_schemas_differ_in_nothing() {
        let a = snap(vec![table("orders", vec![col("id", "integer")])]);
        assert!(schemas(&a.clone(), &a).is_empty());
    }

    #[test]
    fn a_table_only_the_source_has_is_new() {
        let source = snap(vec![table("orders", vec![col("id", "integer")])]);
        let target = snap(vec![]);
        let d = schemas(&source, &target);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].side, Side::New);
        assert_eq!(d[0].kind, DiffKind::Table);
        assert_eq!(d[0].table, "orders");
    }

    #[test]
    fn a_table_only_the_target_has_is_missing() {
        let source = snap(vec![]);
        let target = snap(vec![table("stale", vec![col("id", "integer")])]);
        let d = schemas(&source, &target);
        assert_eq!(d[0].side, Side::Missing);
        assert_eq!(d[0].table, "stale");
    }

    #[test]
    fn a_changed_type_differs_and_shows_both_sides() {
        let source = snap(vec![table("orders", vec![col("total", "numeric")])]);
        let target = snap(vec![table("orders", vec![col("total", "integer")])]);
        let d = schemas(&source, &target);
        let c = d.iter().find(|x| x.kind == DiffKind::Column).unwrap();
        assert_eq!(c.side, Side::Differs);
        assert_eq!(c.source, "numeric");
        assert_eq!(c.target, "integer");
    }

    #[test]
    fn nullability_and_defaults_count_as_a_difference() {
        let mut a = col("total", "numeric");
        a.nullable = false;
        let mut b = col("total", "numeric");
        b.default = Some("0".into());
        let source = snap(vec![table("orders", vec![a])]);
        let target = snap(vec![table("orders", vec![b])]);
        let d = schemas(&source, &target);
        let c = d.iter().find(|x| x.kind == DiffKind::Column).unwrap();
        assert_eq!(c.side, Side::Differs);
        assert_eq!(c.source, "numeric NOT NULL");
        assert_eq!(c.target, "numeric DEFAULT 0");
    }

    #[test]
    fn a_different_primary_key_is_reported_once() {
        let mut source = snap(vec![table("orders", vec![col("id", "integer")])]);
        source.tables[0].primary_key = vec!["id".into(), "line".into()];
        let target = snap(vec![table("orders", vec![col("id", "integer")])]);
        let d = schemas(&source, &target);
        let pk = d.iter().find(|x| x.kind == DiffKind::PrimaryKey).unwrap();
        assert_eq!(pk.source, "id, line");
        assert_eq!(pk.target, "id");
    }

    #[test]
    fn the_primary_keys_own_index_is_not_reported() {
        let mut source = snap(vec![table("orders", vec![col("id", "integer")])]);
        source.tables[0].indexes = vec![IndexDef {
            name: "orders_pkey".into(),
            columns: vec!["id".into()],
            unique: true,
            primary: true,
        }];
        let target = snap(vec![table("orders", vec![col("id", "integer")])]);
        assert!(
            !schemas(&source, &target)
                .iter()
                .any(|d| d.kind == DiffKind::Index)
        );
    }

    #[test]
    fn keys_are_matched_on_columns_not_names() {
        let fk = |name: &str| ForeignKey {
            name: name.into(),
            columns: vec!["customer_id".into()],
            ref_schema: "public".into(),
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let mut source = snap(vec![table("orders", vec![col("id", "integer")])]);
        source.tables[0].foreign_keys = vec![fk("orders_customer_fkey")];
        let mut target = snap(vec![table("orders", vec![col("id", "integer")])]);
        // SQLite names it differently; it is still the same relationship.
        target.tables[0].foreign_keys = vec![fk("fk_0")];
        assert!(
            !schemas(&source, &target)
                .iter()
                .any(|d| d.kind == DiffKind::ForeignKey)
        );
    }

    #[test]
    fn a_changed_referential_action_differs() {
        let fk = |on_delete: Option<&str>| ForeignKey {
            name: "k".into(),
            columns: vec!["customer_id".into()],
            ref_schema: "public".into(),
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
            on_delete: on_delete.map(String::from),
            on_update: None,
        };
        let mut source = snap(vec![table("orders", vec![col("id", "integer")])]);
        source.tables[0].foreign_keys = vec![fk(Some("CASCADE"))];
        let mut target = snap(vec![table("orders", vec![col("id", "integer")])]);
        target.tables[0].foreign_keys = vec![fk(None)];
        let d = schemas(&source, &target);
        let k = d.iter().find(|x| x.kind == DiffKind::ForeignKey).unwrap();
        assert_eq!(k.side, Side::Differs);
        assert!(k.source.contains("ON DELETE CASCADE"));
    }

    #[test]
    fn the_migration_creates_a_new_table_and_comments_a_drop() {
        let source = snap(vec![table("orders", vec![col("id", "integer")])]);
        let target = snap(vec![table("stale", vec![col("id", "integer")])]);
        let d = schemas(&source, &target);
        let sql = migration(&source, &d, Engine::Postgres);
        assert!(sql.contains("CREATE TABLE public.orders"), "{sql}");
        assert!(sql.contains("-- target has stale"), "{sql}");
        // A drop must never be live SQL.
        for line in sql.lines().filter(|l| l.contains("DROP TABLE")) {
            assert!(line.trim_start().starts_with("--"), "{line}");
        }
    }

    #[test]
    fn a_new_nullable_column_becomes_an_add_column() {
        let source = snap(vec![table(
            "orders",
            vec![col("id", "integer"), col("note", "text")],
        )]);
        let target = snap(vec![table("orders", vec![col("id", "integer")])]);
        let d = schemas(&source, &target);
        let sql = migration(&source, &d, Engine::Postgres);
        assert!(
            sql.contains("ALTER TABLE orders ADD COLUMN note text;"),
            "{sql}"
        );
    }

    #[test]
    fn a_new_not_null_column_without_a_default_defers_the_constraint() {
        let mut note = col("note", "text");
        note.nullable = false;
        let source = snap(vec![table("orders", vec![col("id", "integer"), note])]);
        let target = snap(vec![table("orders", vec![col("id", "integer")])]);
        let d = schemas(&source, &target);
        let sql = migration(&source, &d, Engine::Postgres);
        // The ADD COLUMN itself must not carry NOT NULL: existing rows have
        // no value for it.
        assert!(sql.contains("ADD COLUMN note text;"), "{sql}");
        assert!(sql.contains("SET NOT NULL"), "{sql}");
        assert!(sql.contains("-- then, once note is filled"), "{sql}");
    }

    #[test]
    fn a_table_without_a_key_cannot_be_data_compared() {
        let mut t = table("log", vec![col("line", "text")]);
        t.primary_key.clear();
        assert!(data_query(&t, Engine::Postgres, 100).is_none());
    }

    #[test]
    fn the_data_query_orders_by_the_key_and_bounds_the_sample() {
        let t = table(
            "orders",
            vec![col("id", "integer"), col("total", "numeric")],
        );
        let sql = data_query(&t, Engine::Postgres, 500).unwrap();
        assert_eq!(
            sql,
            "SELECT id, total FROM public.orders ORDER BY id LIMIT 500"
        );
    }

    #[test]
    fn sqlite_leaves_the_schema_off_the_data_query() {
        let t = table("orders", vec![col("id", "integer")]);
        let sql = data_query(&t, Engine::Sqlite, 10).unwrap();
        assert!(sql.contains("FROM orders"), "{sql}");
        assert!(!sql.contains("public"), "{sql}");
    }

    fn row(key: &str, values: &[&str]) -> (String, Vec<String>) {
        (
            key.to_string(),
            values.iter().map(|v| v.to_string()).collect(),
        )
    }

    #[test]
    fn matching_rows_produce_nothing() {
        let a = vec![row("1", &["1", "x"]), row("2", &["2", "y"])];
        let cols = vec!["id".to_string(), "note".to_string()];
        assert!(rows(&a, &a.clone(), &cols).is_empty());
    }

    #[test]
    fn a_changed_value_names_its_column() {
        let s = vec![row("1", &["1", "x"])];
        let t = vec![row("1", &["1", "y"])];
        let cols = vec!["id".to_string(), "note".to_string()];
        let d = rows(&s, &t, &cols);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].side, Side::Differs);
        assert_eq!(d[0].columns, vec!["note".to_string()]);
    }

    #[test]
    fn rows_only_one_side_has_are_new_or_missing() {
        let s = vec![row("1", &["1"]), row("3", &["3"])];
        let t = vec![row("2", &["2"]), row("3", &["3"])];
        let cols = vec!["id".to_string()];
        let d = rows(&s, &t, &cols);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].side, Side::New);
        assert_eq!(d[0].key, "1");
        assert_eq!(d[1].side, Side::Missing);
        assert_eq!(d[1].key, "2");
    }

    #[test]
    fn a_longer_tail_on_either_side_is_drained() {
        let cols = vec!["id".to_string()];
        let s = vec![row("1", &["1"]), row("2", &["2"]), row("3", &["3"])];
        let t = vec![row("1", &["1"])];
        let d = rows(&s, &t, &cols);
        assert_eq!(d.len(), 2);
        assert!(d.iter().all(|x| x.side == Side::New));

        let d = rows(&t, &s, &cols);
        assert_eq!(d.len(), 2);
        assert!(d.iter().all(|x| x.side == Side::Missing));
    }

    #[test]
    fn a_composite_key_joins_with_a_separator_no_value_contains() {
        let k = row_key(&["1".into(), "a".into()]);
        assert_eq!(k, "1\u{1f}a");
    }

    #[test]
    fn the_data_migration_inserts_updates_and_comments_deletes() {
        let t = table("orders", vec![col("id", "integer"), col("note", "text")]);
        let source = vec![row("1", &["1", "x"]), row("2", &["2", "y"])];
        let diffs = vec![
            RowDiff {
                side: Side::New,
                key: "2".into(),
                columns: Vec::new(),
            },
            RowDiff {
                side: Side::Differs,
                key: "1".into(),
                columns: vec!["note".into()],
            },
            RowDiff {
                side: Side::Missing,
                key: "9".into(),
                columns: Vec::new(),
            },
        ];
        let sql = data_migration(&t, &diffs, &source, Engine::Postgres);
        assert!(
            sql.contains("INSERT INTO public.orders (id, note) VALUES ('2', 'y');"),
            "{sql}"
        );
        assert!(
            sql.contains("UPDATE public.orders SET note = 'x' WHERE id = '1';"),
            "{sql}"
        );
        assert!(sql.contains("-- target row 9"), "{sql}");
        for line in sql.lines().filter(|l| l.contains("DELETE")) {
            assert!(line.trim_start().starts_with("--"), "{line}");
        }
    }
}
