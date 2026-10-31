//! Copy and export formats for a result (docs/tools/database.md §7):
//! TSV (clipboard), CSV, JSON (array of objects), Markdown, SQL `INSERT`
//! and an `IN (…)` list. NULL stays NULL in every format that has one.

use crate::driver::{Column, Engine, Row, Value};
use crate::sql::{quote_ident, quote_literal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Tsv,
    Csv,
    Json,
    Markdown,
    Insert,
    InList,
    /// Excel, file export only (never text).
    Xlsx,
}

impl Format {
    pub const ALL: [Format; 6] = [
        Format::Tsv,
        Format::Csv,
        Format::Json,
        Format::Markdown,
        Format::Insert,
        Format::InList,
    ];

    /// Fluent key of the menu label.
    pub fn label_key(self) -> &'static str {
        match self {
            Format::Tsv => "db-format-tsv",
            Format::Csv => "db-format-csv",
            Format::Json => "db-format-json",
            Format::Markdown => "db-format-markdown",
            Format::Insert => "db-format-insert",
            Format::InList => "db-format-in-list",
            Format::Xlsx => "db-format-xlsx",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Tsv => "tsv",
            Format::Csv => "csv",
            Format::Json => "json",
            Format::Markdown => "md",
            Format::Insert | Format::InList => "sql",
            Format::Xlsx => "xlsx",
        }
    }
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// One value as JSON text. Exact decimals are written with every digit the
/// server sent (serde_json's `Number` would reformat `10.50` as `10.5`).
fn json_value(v: &Value) -> String {
    let string = |s: &str| serde_json::Value::String(s.to_string()).to_string();
    match v {
        Value::Null => "null".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) if f.is_finite() => serde_json::Value::from(*f).to_string(),
        Value::Float(f) => string(&f.to_string()),
        Value::Number(s) if serde_json::from_str::<serde_json::Number>(s).is_ok() => s.clone(),
        Value::Number(s) | Value::Text(s) => string(s),
        Value::Bytes(_) => string(&v.display()),
    }
}

/// A SQL literal for `v` in `engine`'s dialect.
pub fn sql_literal(v: &Value, engine: Engine) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) if f.is_finite() => f.to_string(),
        Value::Float(f) => quote_literal(&f.to_string(), engine),
        Value::Number(s) => s.clone(),
        Value::Text(s) => quote_literal(s, engine),
        Value::Bytes(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            match engine {
                Engine::Postgres => format!("'\\x{hex}'::bytea"),
                Engine::Mssql => format!("0x{hex}"),
                Engine::Oracle => format!("HEXTORAW('{hex}')"),
                Engine::Db2 => format!("BX'{hex}'"),
                Engine::ClickHouse => format!("unhex('{hex}')"),
                Engine::DuckDb => format!("from_hex('{hex}')"),
                Engine::Sqlite | Engine::Mysql => format!("X'{hex}'"),
            }
        }
    }
}

fn cell_text(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        other => other.display(),
    }
}

/// Text before the first row.
pub fn header(format: Format, columns: &[Column]) -> String {
    let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    match format {
        Format::Tsv => names.join("\t"),
        Format::Csv => names
            .iter()
            .map(|n| csv_field(n))
            .collect::<Vec<_>>()
            .join(","),
        Format::Json => "[".into(),
        Format::Markdown => {
            let esc = |s: &str| s.replace('|', "\\|").replace(['\n', '\r'], " ");
            format!(
                "| {} |\n|{}|",
                names.iter().map(|n| esc(n)).collect::<Vec<_>>().join(" | "),
                columns
                    .iter()
                    .map(|_| " --- ")
                    .collect::<Vec<_>>()
                    .join("|")
            )
        }
        Format::Insert | Format::Xlsx => String::new(),
        Format::InList => "IN (".into(),
    }
}

/// One row, including its separator from the previous one (`first` = no
/// separator).
pub fn row(
    format: Format,
    columns: &[Column],
    r: &Row,
    engine: Engine,
    table: &str,
    first: bool,
) -> String {
    match format {
        Format::Tsv => {
            let cells: Vec<String> = r
                .iter()
                .map(|v| cell_text(v).replace(['\t', '\n', '\r'], " "))
                .collect();
            format!("\n{}", cells.join("\t"))
        }
        Format::Csv => {
            let cells: Vec<String> = r
                .iter()
                .map(|v| match v {
                    Value::Null => String::new(),
                    other => csv_field(&other.display()),
                })
                .collect();
            format!("\r\n{}", cells.join(","))
        }
        Format::Json => {
            let fields: Vec<String> = columns
                .iter()
                .zip(r.iter())
                .map(|(c, v)| {
                    format!(
                        "    {}: {}",
                        serde_json::Value::String(c.name.clone()),
                        json_value(v)
                    )
                })
                .collect();
            format!(
                "{}\n  {{\n{}\n  }}",
                if first { "" } else { "," },
                fields.join(",\n")
            )
        }
        Format::Markdown => {
            let esc = |s: &str| s.replace('|', "\\|").replace(['\n', '\r'], " ");
            format!(
                "\n| {} |",
                r.iter()
                    .map(|v| esc(&cell_text(v)))
                    .collect::<Vec<_>>()
                    .join(" | ")
            )
        }
        Format::Insert => {
            let target = table
                .split('.')
                .map(|p| quote_ident(p, engine))
                .collect::<Vec<_>>()
                .join(".");
            let cols = columns
                .iter()
                .map(|c| quote_ident(&c.name, engine))
                .collect::<Vec<_>>()
                .join(", ");
            let vals = r
                .iter()
                .map(|v| sql_literal(v, engine))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{}INSERT INTO {target} ({cols}) VALUES ({vals});",
                if first { "" } else { "\n" }
            )
        }
        Format::InList => format!(
            "{}{}",
            if first { "" } else { ", " },
            r.first()
                .map(|v| sql_literal(v, engine))
                .unwrap_or_default()
        ),
        Format::Xlsx => String::new(),
    }
}

/// Text after the last row (`empty` = there were none).
pub fn footer(format: Format, empty: bool) -> String {
    match format {
        Format::Csv => "\r\n".into(),
        Format::Json if empty => "]".into(),
        Format::Json => "\n]".into(),
        Format::InList => ")".into(),
        _ => String::new(),
    }
}

/// Render `rows` (all of them, in order) in `format`. `table` names the
/// target of `INSERT` statements.
pub fn render(
    format: Format,
    columns: &[Column],
    rows: &[&Row],
    engine: Engine,
    table: &str,
) -> String {
    let mut out = header(format, columns);
    for (i, r) in rows.iter().enumerate() {
        out.push_str(&row(format, columns, r, engine, table, i == 0));
    }
    out.push_str(&footer(format, rows.is_empty()));
    out
}

/// Streams a result into a file as it arrives (export of all rows).
pub struct FileSink<W: std::io::Write + Send> {
    pub out: W,
    pub format: Format,
    pub engine: Engine,
    pub table: String,
    pub columns: Vec<Column>,
    pub written: u64,
    pub error: Option<std::io::Error>,
    /// Called with the row count after each batch; returns false to stop.
    pub progress: Box<dyn FnMut(u64) -> bool + Send>,
}

impl<W: std::io::Write + Send> FileSink<W> {
    pub fn finish(mut self) -> std::io::Result<u64> {
        if let Some(e) = self.error.take() {
            return Err(e);
        }
        self.out
            .write_all(footer(self.format, self.written == 0).as_bytes())?;
        self.out.flush()?;
        Ok(self.written)
    }
}

impl<W: std::io::Write + Send> crate::driver::ResultSink for FileSink<W> {
    fn columns(&mut self, cols: Vec<Column>) {
        if let Err(e) = self.out.write_all(header(self.format, &cols).as_bytes()) {
            self.error = Some(e);
        }
        self.columns = cols;
    }
    fn rows(&mut self, batch: Vec<Row>) -> crate::driver::Flow {
        if self.error.is_some() {
            return crate::driver::Flow::Stop;
        }
        for r in &batch {
            let text = row(
                self.format,
                &self.columns,
                r,
                self.engine,
                &self.table,
                self.written == 0,
            );
            if let Err(e) = self.out.write_all(text.as_bytes()) {
                self.error = Some(e);
                return crate::driver::Flow::Stop;
            }
            self.written += 1;
        }
        if (self.progress)(self.written) {
            crate::driver::Flow::Continue
        } else {
            crate::driver::Flow::Stop
        }
    }
}

/// Excel holds this many data rows under the header.
pub const XLSX_MAX_ROWS: u64 = 1_048_575;

/// Streams a result into a worksheet; the workbook is written by `finish`.
pub struct XlsxSink {
    sheet: rust_xlsxwriter::Worksheet,
    pub written: u64,
    pub error: Option<String>,
    /// Called with the row count after each batch; returns false to stop.
    pub progress: Box<dyn FnMut(u64) -> bool + Send>,
}

impl XlsxSink {
    pub fn new(progress: Box<dyn FnMut(u64) -> bool + Send>) -> Self {
        Self {
            sheet: rust_xlsxwriter::Worksheet::new(),
            written: 0,
            error: None,
            progress,
        }
    }

    pub fn finish(self, path: &std::path::Path) -> Result<u64, String> {
        if let Some(e) = self.error {
            return Err(e);
        }
        let mut wb = rust_xlsxwriter::Workbook::new();
        wb.push_worksheet(self.sheet);
        wb.save(path).map_err(|e| e.to_string())?;
        Ok(self.written)
    }
}

/// Keep the first write error; later cells are still attempted.
fn note(
    error: &mut Option<String>,
    r: Result<&mut rust_xlsxwriter::Worksheet, rust_xlsxwriter::XlsxError>,
) {
    if let Err(e) = r
        && error.is_none()
    {
        *error = Some(e.to_string());
    }
}

impl crate::driver::ResultSink for XlsxSink {
    fn columns(&mut self, cols: Vec<Column>) {
        let bold = rust_xlsxwriter::Format::new().set_bold();
        for (i, c) in cols.iter().enumerate() {
            let r = self
                .sheet
                .write_string_with_format(0, i as u16, &c.name, &bold);
            note(&mut self.error, r);
        }
    }
    fn rows(&mut self, batch: Vec<Row>) -> crate::driver::Flow {
        for r in &batch {
            if self.written >= XLSX_MAX_ROWS {
                self.error = Some(format!(
                    "Excel holds at most {XLSX_MAX_ROWS} data rows; the rest were left out"
                ));
                return crate::driver::Flow::Stop;
            }
            let row = (self.written + 1) as u32;
            for (ci, v) in r.iter().enumerate() {
                let col = ci as u16;
                let res = match v {
                    Value::Null => continue,
                    Value::Int(i) => self.sheet.write_number(row, col, *i as f64),
                    Value::Float(f) if f.is_finite() => self.sheet.write_number(row, col, *f),
                    // A decimal with more digits than a double keeps stays
                    // text, so nothing is rounded behind the user's back.
                    Value::Number(s) if s.len() <= 15 && s.parse::<f64>().is_ok() => self
                        .sheet
                        .write_number(row, col, s.parse::<f64>().unwrap_or(0.0)),
                    other => self.sheet.write_string(row, col, other.display()),
                };
                note(&mut self.error, res);
            }
            self.written += 1;
            if self.error.is_some() {
                return crate::driver::Flow::Stop;
            }
        }
        if (self.progress)(self.written) {
            crate::driver::Flow::Continue
        } else {
            crate::driver::Flow::Stop
        }
    }
}

/// The table a simple `SELECT … FROM t` reads, for INSERT export.
pub fn source_table(sql: &str, engine: Engine) -> Option<String> {
    use crate::sql::{Tok, tokenize};
    let toks = tokenize(sql, engine);
    let mut depth = 0i32;
    for (k, t) in toks.iter().enumerate() {
        match &t.tok {
            Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            Tok::Word { upper, .. } if depth == 0 && upper == "FROM" => {
                let mut parts = Vec::new();
                let mut j = k + 1;
                loop {
                    match toks.get(j).map(|t| &t.tok) {
                        Some(Tok::Word { raw, .. }) => parts.push(raw.clone()),
                        Some(Tok::Quoted(s)) => parts.push(s.clone()),
                        _ => break,
                    }
                    j += 1;
                    if matches!(toks.get(j).map(|t| &t.tok), Some(Tok::Dot)) {
                        j += 1;
                    } else {
                        break;
                    }
                }
                return (!parts.is_empty()).then(|| parts.join("."));
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> (Vec<Column>, Vec<Row>) {
        let c = |n: &str| Column {
            name: n.into(),
            type_name: String::new(),
        };
        (
            vec![c("id"), c("name"), c("price"), c("raw")],
            vec![
                vec![
                    Value::Int(1),
                    Value::Text("a, \"b\"".into()),
                    Value::Number("10.50".into()),
                    Value::Null,
                ],
                vec![
                    Value::Int(2),
                    Value::Text("it's".into()),
                    Value::Null,
                    Value::Bytes(vec![0, 255]),
                ],
            ],
        )
    }

    #[test]
    fn renders_every_format() {
        let (cols, rows) = data();
        let refs: Vec<&Row> = rows.iter().collect();
        let e = Engine::Postgres;
        assert_eq!(
            render(Format::Csv, &cols, &refs, e, ""),
            "id,name,price,raw\r\n1,\"a, \"\"b\"\"\",10.50,\r\n2,it's,,\\x00ff\r\n"
        );
        assert_eq!(
            render(Format::Tsv, &cols, &refs[..1], e, ""),
            "id\tname\tprice\traw\n1\ta, \"b\"\t10.50\tNULL"
        );
        let text = render(Format::Json, &cols, &refs, e, "");
        assert!(text.contains("\"price\": 10.50"), "{text}");
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(json[0]["raw"].is_null());
        assert_eq!(json[1]["name"], "it's");
        assert_eq!(
            render(Format::Insert, &cols, &refs, e, "public.Items"),
            "INSERT INTO public.\"Items\" (id, name, price, raw) VALUES (1, 'a, \"b\"', 10.50, NULL);\n\
             INSERT INTO public.\"Items\" (id, name, price, raw) VALUES (2, 'it''s', NULL, '\\x00ff'::bytea);"
        );
        assert!(
            render(Format::Insert, &cols, &refs[1..], Engine::Mysql, "items")
                .ends_with("X'00ff');")
        );
        assert_eq!(render(Format::InList, &cols, &refs, e, ""), "IN (1, 2)");
        assert_eq!(render(Format::Json, &cols, &[], e, ""), "[]");
        // Streaming produces the same text as rendering.
        let mut sink = FileSink {
            out: Vec::new(),
            format: Format::Csv,
            engine: e,
            table: String::new(),
            columns: Vec::new(),
            written: 0,
            error: None,
            progress: Box::new(|_| true),
        };
        use crate::driver::ResultSink;
        sink.columns(cols.clone());
        sink.rows(rows.clone());
        let n = sink.written;
        let bytes = {
            let mut s = sink;
            s.out
                .extend_from_slice(footer(Format::Csv, n == 0).as_bytes());
            s.out
        };
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            render(Format::Csv, &cols, &refs, e, "")
        );
        let md = render(Format::Markdown, &cols, &refs[..1], e, "");
        assert_eq!(
            md,
            "| id | name | price | raw |\n| --- | --- | --- | --- |\n| 1 | a, \"b\" | 10.50 | NULL |"
        );
    }

    #[test]
    fn an_xlsx_export_writes_a_workbook() {
        use crate::driver::ResultSink;
        let (cols, rows) = data();
        let mut sink = XlsxSink::new(Box::new(|_| true));
        sink.columns(cols);
        sink.rows(rows);
        assert_eq!(sink.written, 2);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.xlsx");
        assert_eq!(sink.finish(&path).unwrap(), 2);
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"PK"), "an xlsx is a zip");
        assert!(bytes.len() > 1000);
    }

    #[test]
    fn finds_the_source_table() {
        let e = Engine::Postgres;
        assert_eq!(
            source_table(
                "select * from public.orders where id in (select x from y)",
                e
            )
            .as_deref(),
            Some("public.orders")
        );
        assert_eq!(
            source_table("select (select 1 from z), a from \"T\"", e).as_deref(),
            Some("T")
        );
        assert_eq!(source_table("select 1", e), None);
    }
}
