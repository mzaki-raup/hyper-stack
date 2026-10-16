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
| SQLite | `rusqlite` (bundled) | `spawn_blocking`; WAL/immutable-URI handling for files in use by another process (Eden Explorer's `sqlite.rs` documents the edge cases worth reading) |
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
| Ctrl+Enter | run the selection, else the statement at the cursor |
| Ctrl+Shift+Enter, F5 | run script |
| Ctrl+Shift+X | explain |
| Esc | cancel running |
| Ctrl+Shift+R | refresh schema |
| Ctrl+Space | completion |
| Ctrl+Shift+F | format SQL (not yet; the combo is the terminal's Find) |
| Alt+C / Alt+R | commit / rollback (manual tx) |
| Ctrl+Alt+E | export result as CSV (Ctrl+Shift+E is the global environment switcher) |

## 14. Tests

Classifier and splitter fixtures per dialect; driver integration tests against
containers (Postgres 16, MySQL 8, MariaDB, SQL Server 2022) covering
streaming, cancel, introspection, type decoding (dates, decimals, JSON,
arrays, UUID, binary); grid tests on 1 M synthetic rows.

## 15. Implementation status (M3, 2026-10-12)

### Cell viewer formatting (2026-10-16)

A text cell that holds JSON or XML is laid out in the viewer, and so is a
`bytes` column that sniffs as text. The text is re-indented rather than
re-serialised, so the column's key order, its numbers and its duplicate
keys are exactly what the database returned.

### AI assistant (implemented 2026-10-14)

The SQL toolbar's **AI Assistant** button opens help with the current
connection, database engine, SQL buffer and latest query error. Context is
previewed in the panel and is sent only when the user submits a question.

Quick actions under the chat: **Select All**, **Count Rows**, **Select
Ordered**, **Explain Query** and **Improve Query**. A SQL block offers Copy,
**Insert** (at the cursor) and **Insert and Run**, which sends exactly the
inserted text through `run_text` — the same classification, confirmation and
protected-environment guards as the Run button, so nothing bypasses them
(ADR-019, amended 2026-10-16). Insert and Run is hidden until the tab has a
connection.

What exists, and where it differs from the sections above:

- **Drivers** (`tool-db::driver`): one `DbSession` trait object per tab
  (`connect(&ConnectConfig)` picks the engine; there is no separate
  `DbDriver`/`Dialect` object, the `Engine` enum carries the dialect's few
  differences). Rows stream in batches of 100 into a `ResultSink`.
  - *PostgreSQL* (`tokio-postgres` + rustls/ring): statements run over the
    **simple-query protocol**, so every value arrives in the server's own
    text form (arrays, ranges, JSON, intervals, extensions) and any
    statement works, transaction control included; a Describe of the same
    statement supplies column types for alignment and unquoted numbers.
    Error positions and SQLSTATE come back with the error; notices land in
    Messages. Reaching the row limit outside a transaction block sends a
    cancel request (awaited before the next statement) instead of reading
    the rest; inside a block the rest is read and dropped, since a cancel
    would abort the transaction. Portals are not used.
  - *MySQL / MariaDB* (`mysql_async`, text protocol, rustls/ring): cancel is
    `KILL QUERY` from a second short-lived connection, awaited before the
    result is drained; warnings are read with `SHOW WARNINGS`.
  - *SQLite* (`rusqlite`, bundled): the connection lives on its own thread;
    cancel is `sqlite3_interrupt`; read-only profiles open the file
    read-only with `query_only`.
  - TLS modes as in docs/06 §5: `prefer`/`require` encrypt without checking
    the certificate; `verify-ca`/`verify-full` check it against the platform
    store plus `ssl_ca` (Postgres checks the host name in both; MySQL skips
    it for `verify-ca`).
  - Read-only profiles, and protected environments until writes are
    allowed, also make the **server session** read-only (`SET SESSION
    CHARACTERISTICS AS TRANSACTION READ ONLY`, `SET SESSION TRANSACTION
    READ ONLY`, SQLite read-only open), behind the client-side refusal.
  - Statement timeout (30 s, 5 min or none from the toolbar) is enforced by
    the client: on expiry the statement is cancelled. One session per tab;
    no pool of four.
- **Splitter and classifier** (`tool-db::sql`): a dialect-aware tokenizer
  (strings, `E''`, `$tag$` bodies, quoted identifiers, `#`/`--`/nested
  `/* */` comments), splitting on `;`, MySQL `DELIMITER`, `GO` lines and
  `BEGIN … END` bodies of triggers and routines. The classifier is written
  on that tokenizer, not on `sqlparser` (see docs/06 §4).
- **Schema browser**: schemas › Tables/Views/Functions/Sequences (by engine)
  › columns with PK/NOT NULL/FK badges and an Indexes branch, loaded per
  level; filter box; refresh; double-click a table or view to run
  `SELECT *`; context menu Open Data, New SELECT, Count Rows, Copy Name,
  Truncate…/Drop… (these put the statement in the editor, where running it
  goes through the guard). DDL generation, dependencies and exact counts
  are not done.
- **Completion**: tables after FROM/JOIN/INTO/UPDATE (other schemas
  qualified), columns after `alias.` with the statement's aliases tracked,
  tables after `schema.`, else the statement's columns, tables and
  keywords; opens while typing a word of two or more characters or after
  `.`, Ctrl+Space forces it. The catalog (one query per engine) is cached in
  `HyperStackData/cache/db-catalog/<connection id>.json` so completion works
  before the server answers.
- **Running**: Ctrl+Enter runs the selection or the statement at the cursor,
  F5/Ctrl+Shift+Enter the script (stopping at the first error, later
  statements marked skipped), Explain prefixes `EXPLAIN` (`EXPLAIN QUERY
  PLAN` on SQLite) and shows the plan as rows; the tree view is P1.
  `{{variables}}` are substituted from the profile's environment first.
  Each statement gets a result tab; a failed run selects the failing tab and
  the server's error position is underlined in the editor. Cancel button
  and Escape. `:name`/`$1` parameter prompts are not done.
- **Result grid** (`tool-db::grid`): virtualised, resizable columns, NULL in
  grey italics, numbers right-aligned, header click sorts (numeric-aware,
  NULLs last), quick filter (`null` finds NULLs), cell selection, a cell
  viewer on double-click (JSON pretty-printed, bytes as a hex dump), cell
  context menu (copy cell, copy row, view, filter by value), footer with row
  count, **Fetch More** (re-runs the read with the limit raised by 500),
  **Copy As** (TSV, CSV, JSON, Markdown, SQL INSERT, `IN (…)`) of the
  loaded rows, and **Export** (CSV, JSON, INSERT, Markdown, TSV) of *all*
  rows: a Job that re-runs the statement on a second connection and
  streams it to the file. Column reorder/hide, transpose, statistics and
  Excel are not done; data editing (§8) is P1.
- **Transactions**: Auto-Commit / Manual Commit toggle; manual mode opens a
  transaction before the first statement; Commit/Rollback buttons, a
  "Transaction open" badge, and the tab counts as unsaved while one is open.
- **Guards**: see docs/06 §3–4. Read-only refusal and the confirm dialog
  (statement, class, connection and environment; the target's name typed
  on a protected environment, the connection name when the statement has
  no single target). **Allow Writes** in the toolbar lifts a protected
  environment's read-only for that tab and reconnects. A profile without an
  environment follows the active one.
- **History**: every executed statement in `history.sqlite`
  (`query_history`, password redacted, 90 days / 50,000 rows), searchable
  in the sidebar; a click inserts it into the editor.
- **Saved queries**: Ctrl+S saves the editor to
  `<workspace>/queries/<connection-slug>/<name>.sql` with the JSON header;
  listed in the sidebar and in Ctrl+P as `# name`.
- **Connections**: Settings → Connections form for kind DB (engine,
  host/port/database or SQLite file, user, password into the vault on save,
  TLS mode and CA file, SSH tunnel through any SSH profile via the
  `Tunnels` service) and a **Paste a URL** field (`postgres://`,
  `mysql://`, `mariadb://`, `jdbc:…`, `sqlite:`). Test-connection button and
  the DBeaver/`.pgpass`/`.my.cnf` importers are not done.
- **Tests**: `tests/drivers.rs` runs one scenario on every engine (types,
  row limit + session still usable, transactions, error position,
  introspection, catalog, cancel, read-only) — SQLite always, PostgreSQL
  with `HS_PG`, MySQL/MariaDB with `HS_MYSQL`, TLS modes with `HS_PG_TLS`;
  verified against PostgreSQL 16 and MariaDB 10.11. Unit tests cover the
  splitter and classifier, completion, export formats, URL parsing,
  history redaction and the grid (egui_kittest).

## Addendum (2026-10-17): the schema is a column, not a sidebar block

The server's version, the object filter and the schema tree now sit in
their own **column beside the editor** rather than nested under the selected
row of the connections list. Owner's requirement, 2026-10-17.

The reason is what each one is for: the sidebar is for *choosing a server*,
and hiding it should not take away the schema of the server you are already
working in. The column is a resizable `egui::Panel::left` inside the tool,
160–460 points, and it appears only once a session is open. The connections
list, History and Saved Queries stay in the sidebar, which can be hidden
without touching the schema.

This is the shape the NoSQL tool already had (its container tree is a column
in the tab, not a sidebar block), so the two now read the same way.
