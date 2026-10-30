---
name: hs-db-driver
description: Add or change a database driver in tool-db or tool-nosql — a DbSession implementation (SQL Server, ClickHouse, DuckDB) or a DocStore one (Cassandra/ScyllaDB), with the engine enum, connection form, type mapping, cancel, transaction probing, introspection, catalog, table_schema and the live drivers test. Use when the task names a SQL or NoSQL engine, a connection type, a driver, or query cancellation/introspection.
---

# Adding a database driver

The SQL side is `crates/tools/tool-db/src/driver/`: `mod.rs` holds `Engine`,
`Value`, `ResultSink`, `Scope`, the `DbSession` trait and `connect()`;
`sqlite.rs`, `postgres.rs` and `mysql.rs` are the three worked examples. Read
[docs/tools/database.md](../../../docs/tools/database.md), the "Tool-Specific:
Databases (tool-db)" section of `docs/GOTCHAS.md`, and the phase row in
`docs/10-depth-programme.md` (C for SQL Server / ClickHouse / DuckDB, I for
Cassandra/ScyllaDB) before starting.

## Checklist for a new SQL engine

1. **ADR + dependency check.** A new wire protocol is a decision: add it to
   `docs/08-decisions.md` and make sure `cargo deny` passes. Prefer pure Rust
   and rustls (`tiberius` for SQL Server, `reqwest` for ClickHouse's HTTP
   interface). A heavy bundled build (DuckDB) goes behind a Cargo feature, off
   by default until its CI cost is measured.
2. **`Engine`** — the variant plus `parse`, `id`, `label`, `default_port`, and
   whichever `ObjectKind::groups` the engine's catalog has.
3. **`connect()`** — one arm, returning the new session boxed.
4. **`DbSession`** — implement all of it, not a subset:
   - `execute` streams rows into the `ResultSink` and stops reading (and asks
     the server to stop sending) when the sink returns `Flow::Stop`.
   - `canceller()` returns a real cancel for this engine, as
     `pg_cancel_backend` / `KILL QUERY` / `sqlite3_interrupt` do.
   - `introspect` answers the schema tree with display strings and badges.
   - `catalog` answers completion and the object tree.
   - `table_schema` answers the structured per-table metadata the ER diagram,
     the DDL generator and both compares need (columns with types, nullability
     and defaults; primary key in key order; indexes; foreign keys).
   - `in_transaction` — ask the connection where the protocol reports it,
     otherwise use `tx_effect` from the statement classifier.
5. **Type mapping** into `Value`, including NULL, numeric and the engine's odd
   types. `display()` is what the grid shows; don't lose precision on the way.
6. **Connection form** fields for the engine (host, port, database, auth, TLS)
   through `hs_ui::form`, with credentials as `secret:<ulid>` and the
   `token` / `api-key` kinds where the engine uses them.
7. **Guards.** Protected environments are read-only; every write or DDL verb
   asks first. Query work is a Job with cancel — never `block_on` on the UI
   thread.
8. **Tests.** Extend `crates/tools/tool-db/tests/drivers` so the engine's cases
   run when its `HS_*` variable names a server and skip cleanly when it does
   not — never edit a test to skip. Container-backed tests run under
   `HS_INTEGRATION`. Also extend `conn_test`.

```bash
HS_PG=127.0.0.1:5432:dev:dev:postgres HS_MYSQL=127.0.0.1:3306:dev:dev:shop cargo test -p tool-db --test drivers
```

## For a NoSQL store

`crates/tools/tool-nosql/src/store.rs` is the `DocStore` shape: keyspaces /
databases → collections / tables → rows, with paging, the editor's completion
fed from the schema, and the same guards. Live tests:

```bash
HS_MONGO=127.0.0.1:27017 HS_COUCH=http://admin:pw@127.0.0.1:5984 cargo test -p tool-nosql --test live
```

## Finish

Gate (`hs-gate`), exercise it in the running app against a real server in
WSL/Docker, then update `docs/tools/database.md` (or `nosql.md`),
`README.md`, `CHANGELOG.md`, `CHECKLIST.md` and `docs/GOTCHAS.md`. If the
server cannot be reached on this machine, say so in `CHECKLIST.md` instead of
claiming a check you did not make.
