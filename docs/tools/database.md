# Tool spec — Database client

**Crate:** `tool-db` · **Priority:** P0 (SQLite, PostgreSQL, MySQL/MariaDB) · P1 (SQL Server) · P2 (ClickHouse, DuckDB, ODBC) · **Status:** draft

## 1. Purpose

Browse schemas, write and run SQL, inspect and export results, and change data
safely, with the environment always visible. Depth for DBAs (explain, DDL,
transactions, kill query) and simplicity for testers (saved queries, read-only
connections).

Redis has its own tool ([redis.md](redis.md)); document stores such as
MongoDB, CouchDB, Elasticsearch and DynamoDB have theirs ([nosql.md](nosql.md)).
This tool is SQL only.

## 2. User stories

- Open `orders — staging`, expand `public` › `orders`, double-click to see the
  first 500 rows, filter `status = 'failed'` in the grid, copy as JSON into a
  request in the API tool.
- Write a multi-statement migration script, run selection only, see each
  statement's result and timing, roll back the transaction.
- On production (protected), accidentally type `DELETE FROM orders;` — the
  guard blocks it and shows the environment name in red.
- Export a result to CSV/Excel, or generate `INSERT` statements for a fixture.

## 3. Layout

```
┌ sidebar: schema ─────────┬ main ───────────────────────────────────────────────────────┐
│ ● orders — staging  ▾    │ ▌staging▐  ▶ Run (Ctrl+Enter)  ▷ Selection  Explain  ⟲ Tx: auto ▾  Limit 500 ▾ │
│ 🔍 filter objects        │ 1 │ SELECT o.id, o.status, c.email                            │
│ ▾ orders (db)            │ 2 │ FROM orders o JOIN customers c ON c.id = o.customer_id   │
│   ▾ public               │ 3 │ WHERE o.created_at > {{since}}                            │
│     ▾ Tables (42)        │ 4 │ ORDER BY o.created_at DESC;                               │
│       ▸ orders           ├────────────────────────────────────────────────────────────── │
│       ▸ customers        │ [Result 1 ✓ 500 rows · 38 ms] [Messages] [Explain]            │
│     ▸ Views (6)          │ id      │ status  │ email                │ created_at          │
│     ▸ Functions (12)     │ 10422   │ failed  │ ada@example.com      │ 2026-10-10 14:31:02 │
│     ▸ Sequences          │ 10421   │ paid    │ …                    │ …                   │
│ ▸ History                │ ──────────────────────────────────────────────────────────── │
│ ▸ Saved queries          │ 500 rows (limited) · fetch more · 🔍 grid filter · Export ▾ · Copy as ▾ │
└──────────────────────────┴────────────────────────────────────────────────────────────── ┘
```

A tab = one query editor bound to one connection (and therefore one
environment). Several tabs may share the connection; each gets its own server
session so one long query does not block another and can be cancelled.

## 4. Driver abstraction

```rust
#[async_trait]
trait DbDriver: Send + Sync {
    fn engine(&self) -> Engine;
    async fn connect(&self, profile: &ResolvedDbProfile) -> Result<Box<dyn DbSession>>;
    fn dialect(&self) -> &dyn Dialect;        // quoting, limit syntax, explain syntax, type names, keyword list
}
#[async_trait]
trait DbSession: Send {
    async fn execute(&mut self, sql: &str, params: &[Value], sink: &mut dyn ResultSink, cancel: CancelToken) -> Result<ExecSummary>;
    async fn cancel_running(&self) -> Result<()>;      // pg_cancel_backend / KILL QUERY / sqlite interrupt
    async fn begin(&mut self) -> Result<()>; async fn commit(&mut self) -> Result<()>; async fn rollback(&mut self) -> Result<()>;
    async fn introspect(&mut self, scope: Scope) -> Result<SchemaTree>;   // lazy per level
    async fn explain(&mut self, sql: &str, analyze: bool) -> Result<Plan>;
    fn server_info(&self) -> &ServerInfo;
}
trait ResultSink { fn columns(&mut self, cols: Vec<Column>); fn rows(&mut self, batch: Vec<Row>) -> Flow; fn done(&mut self, summary: ExecSummary); }
```

Rows stream in batches (default 100) into the grid; the sink returns
`Flow::Stop` when the row limit is reached; `fetch more` resumes with a
cursor where the engine supports it (Postgres portals, MySQL unbuffered,
otherwise re-run with OFFSET).

| Engine | Crate | Notes |
|---|---|---|
| SQLite | `rusqlite` (bundled) | port Eden's `sqlite.rs`; `spawn_blocking`; WAL/immutable handling for files in use |
| PostgreSQL (+ CockroachDB, Redshift basic) | `tokio-postgres` + `rustls` | portals for paging, `LISTEN/NOTIFY` viewer (P2), `COPY` for export/import |
| MySQL / MariaDB | `mysql_async` | `KILL QUERY <id>` for cancel |
| SQL Server | `tiberius` | Windows auth via SSPI on Windows |
| ClickHouse | `clickhouse` (HTTP) | P2 |
| DuckDB | `duckdb` | P2; also lets us query CSV/Parquet files locally |
| Anything ODBC | `odbc-api` | P2 escape hatch (Oracle, DB2, Access) |

## 5. Schema browser

Lazy `Tree`: server › databases › schemas › (tables, views, materialised
views, functions/procedures, sequences, types, triggers, extensions) › columns
(type, nullable, default, PK/FK/index badges), indexes, constraints, foreign
keys (navigable to the referenced table). Filter box, object counts, refresh
per node. Context menu: open data (SELECT with limit), new query with
`SELECT *`/`INSERT`/`UPDATE` templates, generate DDL (P1), row count (exact or
estimate), truncate/drop (guarded), export table, copy name (quoted), show
dependencies (P2). The introspection result is cached per connection on disk
for instant completion after reconnect, refreshed in the background.

## 6. SQL editor

`hs-editor` in SQL mode with the dialect's keywords and functions; completion
from the schema cache (tables after `FROM`/`JOIN`, columns after an alias or
`.`, keywords otherwise); alias tracking in the current statement; parameter
placeholders `{{var}}` resolved from the environment; `:name` and `$1`
parameters prompt a small form on run (values remembered). Statement
splitting with `sqlparser` (dialect-aware) and a fallback splitter handling
`$$` bodies, `DELIMITER`, `GO`. Run: whole script, current statement (cursor
position), selection. Each statement produces a result tab; errors highlight
the offending statement with the server's position where available.

## 7. Result grid

`hs-ui::DataTable` with: virtualised rows and columns, type-aware alignment
and formatting (dates in local or UTC toggle, numbers with locale option,
booleans, NULL rendered distinctly), column resize/reorder/hide, multi-sort
client-side, grid filter (per column quick filter + expression `status =
'failed' and total > 100` compiled client-side), row selection, cell viewer
popup for long text/JSON (pretty, `hs-editor` read-only)/binary (hex +
image preview)/arrays, copy cell/row/selection as TSV, CSV, JSON, Markdown,
`INSERT`, `IN (...)` list, transpose view for single rows, column statistics
(distinct/min/max/null count) on demand, pin columns (P1).

Export (P1 beyond CSV/JSON): CSV with options, JSON (array or lines), SQL
`INSERT`, Excel (`rust_xlsxwriter`), Markdown, HTML, clipboard. Export runs as
a Job streaming all rows, not just the loaded page.

## 8. Editing data (P1)

Grid cells become editable when the result maps to a single table with a
primary key (detected from the schema). Edits are collected and shown as
generated `UPDATE/INSERT/DELETE` statements in a review pane; the user applies
them in a transaction or copies the SQL. Add row, delete row, set NULL, paste
rows from clipboard. Refused on read-only connections.

## 9. Transactions, safety and guards

- Toolbar toggle: `Tx: auto` (auto-commit) or `Tx: manual` with Commit/Rollback
  buttons and a pending badge; closing a tab with a pending transaction asks.
- Connection read-only flag → session set read-only where supported
  (`SET default_transaction_read_only`, `SET SESSION TRANSACTION READ ONLY`)
  and client-side refusal of write statements.
- Destructive classifier (see [06-security.md](../06-security.md)) on every
  statement; `WriteUnbounded`, `DdlDestructive` and `Admin` show a dialog
  naming connection and environment; on protected environments they require
  typing the object name.
- Statement timeout default 30 s (configurable, per run override); Cancel
  button always visible while running; "Kill session" for stuck ones.

## 10. Explain (P1)

`EXPLAIN` / `EXPLAIN ANALYZE` (Postgres JSON format, MySQL `FORMAT=JSON`,
SQL Server showplan XML) rendered as a tree with cost/rows/time per node,
hottest nodes highlighted, raw text tab, copy. Suggest missing index hints
only where the server reports them (no guessing).

## 11. History and saved queries

- History (SQLite): every executed statement with connection, environment,
  duration, rows, status; search; re-run; pin to saved.
- Saved queries: `.sql` files with a header in the workspace, folders, tags,
  parameters; open from sidebar or quick-open (`#` prefix); "Save as snippet".

## 12. Connections

`kind = "db"` profiles (see [05](../05-data-model-and-storage.md)) with
engine, host/port/database/user, password (SecretRef), SSL mode, options,
SSH tunnel, read-only, colour, environment. Test connection button; import
from DBeaver, `.pgpass`, `.my.cnf`, JDBC/URL strings pasted. Connection pool
per profile (max 4 sessions by default) with idle close.

## 13. Keyboard (DB scope)

| Shortcut | Action |
|---|---|
| Ctrl+Enter | run current statement |
| Ctrl+Shift+Enter | run script |
| Ctrl+Alt+Enter | run selection |
| Ctrl+Shift+X | explain |
| Esc | cancel running |
| Ctrl+Shift+R | refresh schema |
| Ctrl+Space | completion |
| Ctrl+Shift+F | format SQL |
| Alt+C / Alt+R | commit / rollback (manual tx) |
| Ctrl+Shift+E | export result |

## 14. Tests

Classifier and splitter fixtures per dialect; driver integration tests against
containers (Postgres 16, MySQL 8, MariaDB, SQL Server 2022) covering
streaming, cancel, introspection, type decoding (dates, decimals, JSON,
arrays, UUID, binary); grid tests on 1 M synthetic rows.
