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
| Ctrl+Alt+D | ER diagram of the schema |
| Ctrl+Alt+G | generate DDL for the schema |
| Ctrl+Alt+K | compare schema against another connection |
| Ctrl+Alt+Shift+K | compare the current table's rows |

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
    would abort the transaction. Portals are not used. **Transaction state
    is asked of the server** after every statement (2026-10-22): `SELECT
    now() <> statement_timestamp()` is true only inside a block (`now()` is
    the block's start), and error 25P02 ("current transaction is aborted")
    is a block too; the BEGIN/COMMIT text classifier is only the fallback
    when the probe cannot run.
  - *MySQL / MariaDB* (`mysql_async`, text protocol, rustls/ring): cancel is
    `KILL QUERY` from a second short-lived connection, awaited before the
    result is drained; warnings are read with `SHOW WARNINGS`. Transaction
    state is asked of the server too: `@@in_transaction` where it exists
    (MariaDB), else the session's ACTIVE row in
    `performance_schema.events_transactions_current` (MySQL 8); the text
    classifier remains the fallback.
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

## Addendum (2026-10-17): one connection per tab, many queries inside it

A window tab holds **one database connection**; the queries inside it are
**query tabs** in a multi-row strip above the editor, the same strip the
editor uses for its files. Owner's requirement, 2026-10-17.

- Each query tab is a `QueryDoc` with its own buffer, cursors and view, so
  moving between them keeps every caret and scroll where it was, and the
  strip shows the unsaved dot per query rather than per connection.
- A tab is named by the **saved query** it holds, or **Untitled** until it
  is saved. Saving names the tab; opening a saved query from the schema
  column opens **another query tab** on the same connection, rather than
  asking the shell for another window tab — unless the tab in front is
  empty and unsaved, in which case there is no reason to leave a blank one
  behind.
- **+** opens a new query on this connection; closing the last one empties
  it rather than removing it, because a connection tab with no query in it
  has nothing to show.
- The layout remembers which saved query each tab held and which was in
  front (`State::queries`, `State::active_query`). The layout keeps one
  buffer's text, and it is the **front tab's** — a saved tab comes back
  from its file, an unsaved one comes back empty unless it was the tab in
  front. Putting that text into the first tab instead of the active one
  was the one thing this had to get right.
- Running, completion, Explain, history and Save Query all act on the tab
  in front; `DbTool::doc()`/`doc_mut()` is the one place that decides which
  that is.

### Prettify (2026-10-17)

**Prettify** in the toolbar lays the open query tab out again: one clause
to a line, indented, statements kept apart. It formats the **whole
buffer**, not the statement under the caret — a script is a script, and
`sqlformat` keeps its statements separate. Keywords are left in whatever
case they were typed: this is a layout tool, not an opinion about style.
The indent follows the editor's own tab setting (two spaces, or four when
the editor inserts tabs). Nothing happens when the text is already laid
out that way, so the button never dirties a tab for no reason. Owner's
requirement, 2026-10-17.

### Open Data opens a query tab (2026-10-17)

**Open Data** and **Count Rows** in the schema tree no longer run a
statement the user cannot see. Each opens a query tab holding the text it
is about to run — `SELECT * FROM <table>` — and the grid underneath is
that tab's result, so the statement can be edited, extended with a
`WHERE`, and run again without retyping it. An Untitled tab that is still
empty is filled rather than added to, the way the API client fills an
empty tab before opening another; anything already typed is left alone and
a new tab is added beside it. The destructive templates (TRUNCATE, DROP)
keep their old behaviour of inserting into the current tab without
running, since the point of those is that the user runs them deliberately.
Owner's requirement, 2026-10-17.

### The grid scrolls sideways (2026-10-17)

A result wider than the pane scrolls horizontally. This had never worked:
`egui_extras`' table scrolls vertically only and shares out whatever width
it is given, so a wide result was quietly squeezed and then clipped, and
the columns past the edge could not be reached at all — a twelve-column
table showed five. Opening the **AI Assistant** takes 390 px and made it
unmistakable, which is why it was first reported as the assistant floating
on top of the data; the assistant was always a column beside the grid, the
grid just had no way to give ground.

The table now sits in a horizontal `ScrollArea` whose content is as wide as
the columns want to be — worked out from the same measurements the columns
are built with, since inside the table sizing only ever divides up the
width it is handed. A column dragged wider than its natural size is carried
in `GridState::wanted_width`, read back from `TableBody::widths()` so the
next frame makes room for it. When the columns do not fill the pane the
trailing remainder column absorbs the slack exactly as before, so a narrow
result looks unchanged and shows no scrollbar. Owner's report, 2026-10-17.

### The row number is frozen, and the footer gives way (2026-10-17)

Scrolling sideways used to take the row-number column with it, which is
the one column a wide result cannot spare — it is what the rest is counted
from. It is drawn beside the scrolling table now rather than inside it, in
a strip the horizontal scroll never touches. Because the rows it lines up
with live inside the table's own virtualised scroll area, the strip is
painted rather than laid out: each row the table actually draws reports its
screen position back, and the gutter numbers exactly those. A selected
row's number is tinted to match.

The footer under the grid is laid out **from the right**, so Copy As and
Export keep their place and the filter box gives up width first, down to a
box still worth typing in. Left to right, the row simply ran past the clip
edge and Export was sliced in half whenever the pane was narrow — most
obviously with the AI Assistant open. Wrapping is not the alternative it
looks like: the grid reserves exactly one footer row in its height
calculation, so a wrapped second row falls off the bottom of the panel,
which is worse than a clipped button.

**Open Data** and **Count Rows** also terminate what they write
(`SELECT * FROM t;`), so the statement left in the tab is complete.
Owner's requirements, 2026-10-17.

## Depth, phase B (2026-10-30)

The depth programme's second phase (`docs/10-depth-programme.md`)
delivered §8 and §10 and the two export / import items of §7:

- **Explain tree** (`tool-db::plan`): `plan::explain_sql` builds the
  engine's JSON (PostgreSQL `EXPLAIN (FORMAT JSON)`, MySQL `EXPLAIN
  FORMAT=JSON`) or `EXPLAIN QUERY PLAN` (SQLite); `plan::parse` turns
  the rows into one `Plan` of `Node`s (label, details, cost, rows,
  actual rows, time, children) and the result panel draws it with
  `CollapsingHeader`s open by default, each node coloured by its heat
  (`Plan::heat`: time when analysed, cost otherwise). **Explain
  Analyze** (`RunMode::ExplainAnalyze`, palette `db.explain_analyze`)
  adds actual rows and times; it only runs for statements the
  classifier calls reads - a write gets a plain EXPLAIN. **Plan / Raw**
  shows the rows instead; **Copy Plan** copies the indented text.
  Plain-text plans (an EXPLAIN typed by hand) stay a grid. SQL Server
  showplan and index hints are not done.
- **Inline editing** (`tool-db::edit`): `DbTool::ensure_editable` asks
  for the source table's columns (`export::source_table`, the same
  `Scope::Columns` introspection the tree uses, cached under
  `cols:<schema>.<table>`) and `edit::key_columns` finds the primary
  key in the result; without one the result is read-only. `Edits`
  holds `Set` / `Delete` changes per row; the grid draws them tinted
  (warning) or washed (error) and offers Edit Cell, Set NULL, Delete
  Row and Revert Row in the cell menu, F2 edits the selected cell.
  `Edits::statements` generates `UPDATE … WHERE <key> = <original>` /
  `DELETE … WHERE …`; **Review & Apply** shows them and runs them as
  `BEGIN; …; COMMIT;` (or bare, in manual-commit mode) through
  `run_text`, so the read-only and protected-environment guards apply
  unchanged. Add row and paste rows are not done.
- **CSV import** (`tool-db::import`): **Import CSV…** on a table node
  opens a file, sniffs the delimiter, previews five records and the
  table's columns, guesses the mapping by header name (or position
  without a header), and runs `import::run` as a Job on a second
  connection: `BEGIN`, one multi-row `INSERT` per batch (200 rows by
  default), `COMMIT`; a reader error, a failed batch or Cancel rolls
  back. Values go as quoted literals (empty fields as NULL by option),
  so the server casts them. Refused on read-only and protected
  environments before the dialog opens.
- **Excel export** (`export::XlsxSink`, `Format::Xlsx`): a worksheet
  filled as rows stream (header bold, integers and floats as numbers,
  decimals that fit a double as numbers, everything else text), saved
  by the Job at the end; 1 048 575 data rows at most.

Verified 2026-10-30 against SQLite (the fixture `D:\Temp\hs-depth-b`)
on screen and by the unit tests; `plan::parse` for PostgreSQL and
MySQL is covered by tests on captured JSON.

## Dashboard (2026-10-29)

The activity's home tab wears the shape every activity now shares
(`hs_ui::dashboard::saved_dashboard`): a band with the activity's
mark, name and subtitle; the figures in equal rounded boxes;
**Start Here** with its first moves; **Saved Databases** - every connection over its session, or a SQLite file over its path;
and the shared **Open Here** grid, each section ruled apart.

## Depth, phase H (2026-10-31)

The depth programme's eighth phase (`docs/10-depth-programme.md`) adds the
four schema tools. All of them read through one new driver call rather than
the schema browser's own listing: `DbSession::introspect` answers the tree
with a display string and badges like `"FK → orders.id"`, which a generator
cannot work from, so `DbSession::table_schema` returns the parts separately
as `tool_db::schema::TableSchema` (columns with type, nullability, default
and auto-increment; the primary key in key order; indexes; foreign keys with
their referential actions). `schema::read` walks a whole schema through it,
listing tables with the same `introspect` call the tree uses so a snapshot
sees exactly what the tree does, and bounding the walk at 300 tables so a
large schema cannot turn into thousands of round trips; what the cap left
out is reported, not hidden.

Engine notes, each the reason for a test:

- **SQLite** keeps `AUTOINCREMENT` only in the stored DDL, so it is read
  from `sqlite_master.sql` rather than a pragma, and accepted only on an
  `INTEGER PRIMARY KEY`. It also reports `notnull = 0` for that rowid
  alias; a key column is recorded as `NOT NULL` anyway, because no engine
  permits a null in a key and PostgreSQL and MySQL both say so — otherwise
  every primary key would read as a difference in a cross-engine compare.
  SQLite does not name its foreign keys, so they are keyed by the pragma's
  `id`.
- **PostgreSQL** needs `unnest(indkey) WITH ORDINALITY` for composite keys
  and indexes to keep their order, and `confdeltype` / `confupdtype` are
  single letters, not words. A `serial`'s `nextval()` default belongs to
  its sequence, so it is recorded as auto-increment rather than a default.
- **MySQL** stores a string default unquoted (`DEFAULT 'abc'` comes back as
  `abc`), so anything that is neither numeric nor flagged
  `DEFAULT_GENERATED` is quoted on the way out; without that the generated
  DDL would name an identifier. Its driver decodes NULL and the empty
  string alike, so an empty default reads as no default.

The four features:

- **ER diagram** (`tool-db::erd`): builds `hs_diagram`'s ERD from the
  snapshot directly. `hs_diagram::ddl::import` could parse generated SQL
  instead, but that would discard what the catalog already knows and rely
  on the generator's output being re-parseable. Columns carry their
  `ColumnKey` badge (primary wins over foreign, which wins over a
  single-column unique index) and each foreign key becomes one edge:
  `One` or `ZeroOrOne` where the key's own columns are unique (mandatory
  or nullable), `OneToMany` otherwise. A key pointing outside the snapshot,
  a self-reference, and a second key between the same pair of tables draw
  nothing rather than a dangling or duplicate line. The document goes to
  the Graphics tool as an `OpenRequest::Text` with `hs_core::tool::GRAPHICS_MIME`
  and an `open_diagram` body — the constant moved out of `tool-graphics`
  into `hs-core` beside `OpenRequest`, since a tool may not depend on
  another tool and it is part of that contract.
- **DDL generator** (`tool-db::ddlgen`): the target dialect is the
  caller's choice, not the source's, so a PostgreSQL schema can be written
  out as MySQL or SQLite. Column types pass through as the source spelled
  them — this arranges statements, it does not translate type systems, and
  the dialog says so when the dialect differs. Identity spelling differs
  per engine (`GENERATED BY DEFAULT AS IDENTITY`, `AUTO_INCREMENT`,
  `INTEGER PRIMARY KEY AUTOINCREMENT`); SQLite takes no schema prefix and
  has its foreign keys inlined, because it cannot add one afterwards.
  Statements come out as tables, then indexes, then keys, so the script
  runs top to bottom whatever order the tables reference each other in.
- **Schema compare** (`tool-db::compare`): one-directional, source to
  target, every verdict reading as what the target would need. Tables,
  columns, the primary key, indexes and foreign keys come back as
  `New` / `Missing` / `Differs` with both spellings side by side. Keys are
  matched on their **columns**, not their names: SQLite does not name them
  and two servers may name the same relationship differently. The
  generated migration writes only the additive steps as SQL; a drop is
  always a commented line, because the compare cannot tell a stale table
  from one simply outside this schema and running it would be a data-loss
  risk (docs/06-security.md §3). A new `NOT NULL` column without a default
  is added nullable, with the constraint left as a commented follow-up:
  existing rows have no value for it.
- **Data compare** (`tool-db::compare`): one table, by primary key, both
  sides read with `ORDER BY` the key under a row bound and walked together,
  so neither side is held whole beyond the sample. A table with no key is
  refused with a reason rather than compared on row order. The bound being
  reached is reported. Row changes generate `INSERT` and `UPDATE`;
  deletions are commented, as above.

Both compares resolve the target connection read-only, and refuse a profile
that goes through an SSH tunnel rather than connecting straight to that host
and port.
