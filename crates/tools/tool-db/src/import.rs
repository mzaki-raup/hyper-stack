//! CSV import into a table (docs/tools/database.md §7, depth programme
//! phase B): a preview of the file with a column mapping, then a Job that
//! reads the file on its own connection and inserts in batches inside one
//! transaction, with progress and cancel (cancel rolls back).

use std::path::Path;

use crate::driver::{self, ConnectConfig, DbError, Engine};
use crate::sql::{quote_ident, quote_literal};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The first record names the columns.
    pub header: bool,
    pub delimiter: u8,
    /// Rows per INSERT statement.
    pub batch: usize,
    /// An empty field becomes NULL rather than ''.
    pub empty_as_null: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            header: true,
            delimiter: b',',
            batch: 200,
            empty_as_null: true,
        }
    }
}

/// The first records of the file, for the mapping dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preview {
    /// Column names: the header record, or "column 1", "column 2", ….
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// The delimiter the first line suggests: the most frequent of `, ; \t |`.
pub fn sniff_delimiter(first_line: &str) -> u8 {
    b",;\t|"
        .iter()
        .copied()
        .max_by_key(|d| first_line.matches(*d as char).count())
        .unwrap_or(b',')
}

fn reader(path: &Path, opts: &Options) -> Result<csv::Reader<std::fs::File>, String> {
    csv::ReaderBuilder::new()
        .has_headers(opts.header)
        .delimiter(opts.delimiter)
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())
}

/// Read the header and the first `n` records.
pub fn preview(path: &Path, opts: &Options, n: usize) -> Result<Preview, String> {
    let mut rdr = reader(path, opts)?;
    let mut out = Preview::default();
    if opts.header {
        out.headers = rdr
            .headers()
            .map_err(|e| e.to_string())?
            .iter()
            .map(str::to_string)
            .collect();
    }
    for rec in rdr.records().take(n) {
        let rec = rec.map_err(|e| e.to_string())?;
        out.rows.push(rec.iter().map(str::to_string).collect());
    }
    if !opts.header {
        let width = out.rows.iter().map(Vec::len).max().unwrap_or(0);
        out.headers = (1..=width).map(|i| format!("column {i}")).collect();
    }
    Ok(out)
}

/// A first mapping: each table column to the file column of the same name
/// (case-insensitive), else by position when the file has no header.
pub fn guess_mapping(
    table_columns: &[String],
    headers: &[String],
    header: bool,
) -> Vec<Option<usize>> {
    table_columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if header {
                headers.iter().position(|h| h.eq_ignore_ascii_case(c))
            } else {
                (i < headers.len()).then_some(i)
            }
        })
        .collect()
}

/// One multi-row INSERT for `records`, or `None` when nothing is mapped.
pub fn insert_sql(
    table: &str,
    table_columns: &[String],
    mapping: &[Option<usize>],
    records: &[Vec<String>],
    engine: Engine,
    empty_as_null: bool,
) -> Option<String> {
    let mapped: Vec<(usize, usize)> = mapping
        .iter()
        .enumerate()
        .filter_map(|(ci, m)| m.map(|fi| (ci, fi)))
        .collect();
    if mapped.is_empty() || records.is_empty() {
        return None;
    }
    let cols = mapped
        .iter()
        .map(|(ci, _)| quote_ident(&table_columns[*ci], engine))
        .collect::<Vec<_>>()
        .join(", ");
    let rows = records
        .iter()
        .map(|r| {
            let vals = mapped
                .iter()
                .map(|(_, fi)| match r.get(*fi).map(String::as_str) {
                    None => "NULL".to_string(),
                    Some("") if empty_as_null => "NULL".to_string(),
                    Some(v) => quote_literal(v, engine),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("({vals})")
        })
        .collect::<Vec<_>>()
        .join(",\n");
    Some(format!("INSERT INTO {table} ({cols}) VALUES\n{rows};"))
}

/// What a finished import reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub rows: u64,
    pub batches: u64,
}

/// Run the import on a connection of its own, inside one transaction.
/// `progress(rows)` is called after every batch and returns false to stop,
/// which rolls everything back.
pub async fn run(
    cfg: ConnectConfig,
    path: &Path,
    table: &str,
    table_columns: &[String],
    mapping: &[Option<usize>],
    opts: &Options,
    mut progress: impl FnMut(u64) -> bool,
) -> Result<Outcome, String> {
    let mut rdr = reader(path, opts)?;
    let mut session = driver::connect(&cfg).await.map_err(|e| e.to_string())?;
    let engine = session.engine();
    let mut sink = driver::Collect::default();
    session
        .execute("BEGIN", &mut sink)
        .await
        .map_err(|e: DbError| e.to_string())?;
    let mut out = Outcome::default();
    let mut batch: Vec<Vec<String>> = Vec::with_capacity(opts.batch);
    let mut pending: Option<String> = None;
    for rec in rdr.records() {
        match rec {
            Ok(r) => batch.push(r.iter().map(str::to_string).collect()),
            Err(e) => {
                pending = Some(format!("line {}: {e}", out.rows + batch.len() as u64 + 1));
                break;
            }
        }
        if batch.len() >= opts.batch {
            if let Err(e) = flush(
                &mut session,
                &mut sink,
                table,
                table_columns,
                mapping,
                &batch,
                engine,
                opts,
                &mut out,
            )
            .await
            {
                pending = Some(e);
                break;
            }
            batch.clear();
            if !progress(out.rows) {
                pending = Some("cancelled; nothing was written".into());
                break;
            }
        }
    }
    if pending.is_none() && !batch.is_empty() {
        pending = flush(
            &mut session,
            &mut sink,
            table,
            table_columns,
            mapping,
            &batch,
            engine,
            opts,
            &mut out,
        )
        .await
        .err();
        if pending.is_none() && !progress(out.rows) {
            pending = Some("cancelled; nothing was written".into());
        }
    }
    if let Some(e) = pending {
        let _ = session.execute("ROLLBACK", &mut sink).await;
        return Err(e);
    }
    session
        .execute("COMMIT", &mut sink)
        .await
        .map_err(|e: DbError| e.to_string())?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)] // one call site, inside `run`
async fn flush(
    session: &mut Box<dyn driver::DbSession>,
    sink: &mut driver::Collect,
    table: &str,
    table_columns: &[String],
    mapping: &[Option<usize>],
    batch: &[Vec<String>],
    engine: Engine,
    opts: &Options,
    out: &mut Outcome,
) -> Result<(), String> {
    if let Some(sql) = insert_sql(
        table,
        table_columns,
        mapping,
        batch,
        engine,
        opts.empty_as_null,
    ) && let Err(e) = session.execute(&sql, sink).await
    {
        return Err(format!("batch {}: {e}", out.batches + 1));
    }
    out.rows += batch.len() as u64;
    out.batches += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delimiter_is_sniffed_from_the_first_line() {
        assert_eq!(sniff_delimiter("a,b,c"), b',');
        assert_eq!(sniff_delimiter("a;b;c,d"), b';');
        assert_eq!(sniff_delimiter("a\tb"), b'\t');
    }

    #[test]
    fn preview_reads_headers_and_rows() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.csv");
        std::fs::write(&p, "id,name\n1,ann\n2,\"b,c\"\n3,d\n").unwrap();
        let pv = preview(&p, &Options::default(), 2).unwrap();
        assert_eq!(pv.headers, vec!["id", "name"]);
        assert_eq!(pv.rows, vec![vec!["1", "ann"], vec!["2", "b,c"]]);
        let no_header = preview(
            &p,
            &Options {
                header: false,
                ..Default::default()
            },
            1,
        )
        .unwrap();
        assert_eq!(no_header.headers, vec!["column 1", "column 2"]);
        assert_eq!(no_header.rows[0], vec!["id", "name"]);
    }

    #[test]
    fn mapping_is_guessed_by_name_then_position() {
        let cols = vec!["ID".to_string(), "name".to_string(), "extra".to_string()];
        let heads = vec!["name".to_string(), "id".to_string()];
        assert_eq!(
            guess_mapping(&cols, &heads, true),
            vec![Some(1), Some(0), None]
        );
        assert_eq!(
            guess_mapping(&cols, &heads, false),
            vec![Some(0), Some(1), None]
        );
    }

    #[test]
    fn a_batch_becomes_one_insert() {
        let cols = vec!["id".to_string(), "name".to_string(), "note".to_string()];
        let mapping = vec![Some(1), Some(0), None];
        let recs = vec![
            vec!["ann".to_string(), "1".to_string()],
            vec!["b'c".to_string(), String::new()],
        ];
        let sql = insert_sql("public.t", &cols, &mapping, &recs, Engine::Postgres, true).unwrap();
        assert_eq!(
            sql,
            "INSERT INTO public.t (id, name) VALUES\n('1', 'ann'),\n(NULL, 'b''c');"
        );
        assert!(insert_sql("t", &cols, &[None, None, None], &recs, Engine::Sqlite, true).is_none());
    }

    #[test]
    fn imports_into_sqlite_in_one_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db");
        let csv_path = dir.path().join("in.csv");
        let mut body = String::from("name,qty\n");
        for i in 0..450 {
            body.push_str(&format!("item {i},{i}\n"));
        }
        std::fs::write(&csv_path, body).unwrap();
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, qty INTEGER)")
                .unwrap();
        }
        let cfg = ConnectConfig {
            engine: Engine::Sqlite,
            path: db.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let cols = vec!["id".to_string(), "name".to_string(), "qty".to_string()];
        let mapping = vec![None, Some(0), Some(1)];
        let mut seen = Vec::new();
        let out = rt
            .block_on(run(
                cfg.clone(),
                &csv_path,
                "items",
                &cols,
                &mapping,
                &Options::default(),
                |n| {
                    seen.push(n);
                    true
                },
            ))
            .unwrap();
        assert_eq!(out.rows, 450);
        assert_eq!(out.batches, 3);
        assert_eq!(seen, vec![200, 400, 450]);
        let c = rusqlite::Connection::open(&db).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 450);
        let q: i64 = c
            .query_row("SELECT qty FROM items WHERE name = 'item 7'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(q, 7);
        // A cancelled import leaves nothing behind.
        let r = rt.block_on(run(
            cfg,
            &csv_path,
            "items",
            &cols,
            &mapping,
            &Options::default(),
            |_| false,
        ));
        assert!(r.is_err());
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 450);
    }
}
