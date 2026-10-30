# HyperStack — work ledger

Every unit of work has an **id**, lives under exactly one **task version**, and
carries its own **checklist**.

```
/dev-task v0.7.1       every item under that task version
/dev-task FEAT-0062    that one item, and nothing else
```

## Task versions are not app versions

A heading here is **my own batch label** — a bag of work I want done together.
It is not the application's release number, it does not have to match it, and
nothing in this file moves `[workspace.package] version`. The app's number is
decided by the release pipeline from the Conventional Commit types on `main`
(`docs/09-ci-cd-and-updates.md`), whatever this file is called.

So `## v0.7.1` here means "task batch v0.7.1". A batch may ship across two
app releases, or three batches may ship in one. Where it is worth recording
which release carried a batch, the heading gets a `> **Shipped in:**` note
after the fact.

Companion files, and what each is for:

| File | Holds |
|---|---|
| **TASKS.md** (this file) | what is to be done, by id and by task version, with its checklist |
| `CHANGELOG.md` | what was done, one line per change, under Unreleased until the app releases |
| `CHECKLIST.md` | how far the roadmap phases have got |
| `docs/07-roadmap.md`, `docs/10-depth-programme.md` | why, and in what order |

> **Built 2026-10-31.** `v0.7.3` and `v0.7.2` are the live depth-programme
> work; `v0.7.1` down to `v0.0.1` were reconstructed from the `mynotes`
> backlog, one item per horizontal rule, and carry no tick boxes — see
> [Reconstructed history](#reconstructed-history) for why.

---

## Format

### Ids

`FEAT-0001` · `BUG-0001` · `CR-0001` — four digits, allocated once, **never
reused and never renumbered**, because commits, branches and `CHANGELOG` lines
point at them.

| Prefix | Means |
|---|---|
| `FEAT` | new capability the user can see |
| `BUG` | behaviour that contradicts its spec or crashes |
| `CR` | change request: a deliberate change to existing behaviour that is neither new capability nor a defect (a changed default, a renamed label, a tightened guard, a dropped option) |

The next free number per prefix:

```bash
grep -oE '^### (FEAT|BUG|CR)-[0-9]{4}' TASKS.md | sort | tail -5
```

### Task version headings

```
  ## sprint-11 — Planned
  > **Shipped in:** app v0.108.0, 2026-11-04
```

(Indented here only so this example isn't itself matched as a heading — real
headings start at column 0.)

- The label is mine to choose: `v0.7.1`, `v3`, `sprint-11`, `2026-11`. One
  token, no spaces. Keep one style across the file so sorting stays sane.
- Status is `Planned`, `In-Progress`, or `Done <YYYY-MM-DD>` — "done" meaning
  every item in the batch is `Completed`, not that the app released. A
  reconstructed batch whose finish date nobody recorded says `Done` with no
  date rather than inventing one.
- The optional `> **Shipped in:**` line records which app release carried the
  batch, filled in afterwards. The task version is **never** renamed to match
  the app's number, and `[workspace.package] version` is never edited by hand.
- Unscheduled work sits under `## Backlog`. An item belongs to exactly one
  heading; moving it means moving the whole `###` block.

### An item

```markdown
  ### FEAT-0012 · SQL Server driver
  - **State:** Pending · **Spec:** docs/tools/database.md · **Phase:** C
  - **Branch:** feat/depth-c-sql-engines
  - **Depends on:** FEAT-0011
  - [ ] <step>
  - [ ] <step>
  - [ ] Test: <what it asserts>
  - [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
  - [ ] App: `--data-dir ./tmp-data` — <what to click>
  - [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
  - **Notes:** <anything that would otherwise be lost>
```

Rules that keep the file honest:

- **State** is `Pending`, `In-Progress`, `Completed` or `Blocked` — the same
  vocabulary as `CHECKLIST.md`, plus `Blocked`. `Completed` only once every box
  is ticked, with one exception: when I close something with boxes still open,
  the boxes **stay open** and the heading or the item's **Notes** says I closed
  it and what went unchecked. The State is my call; the boxes are the record of
  what was actually verified, and nothing may tick them on my say-so.
- The last four boxes (**Test**, **Gate**, **App**, **Docs**) are on every
  item **that is still to be done**. A box is ticked only when that check
  actually ran and passed. A check that cannot be made on this machine (no
  server here) is left unticked with the reason in **Notes** — never ticked
  optimistically.
- `Blocked` always has a reason in **Notes**.
- **Depends on** names ids. A batch runs dependencies first.
- `BUG` items get a reproducing test before the fix, so their **Test** box is
  the first thing that goes green.
- Optional fields: **Spec**, **Phase**, **Branch**, **Depends on**, **Source**
  (where the request came from), **Kind** (`major`, on the items that moved the
  minor digit) and **Verified** (what the `Completed` claim rests on).

### Reconstructed history

Everything from `v0.7.1` down to `v0.0.1` was reconstructed on 2026-10-31 from
the `mynotes` backlog — one item per horizontal rule in that file, newest at
the top, which is where the version ladder comes from (a patch step per rule
from the bottom up, a minor bump for each item marked **Kind: major**).

Those items carry **State**, **Source** (the `mynotes` line range and how many
requests it held), **Scope** and **Verified** — and deliberately **no boxes**.
Their `Completed` state is the owner's record that the work shipped, not a
check any session re-ran; boxes exist to be ticked by whoever does the work,
and ticking 112 of them after the fact would make every tick in this file
worthless. Finish dates were not recorded, so those headings say `Done` with
no date.

`FEAT-0004`, `BUG-0001` and `CR-0001` were format examples in the file's first
draft, deleted the same day. `BUG-0001` and `CR-0001` were then given to real
items — a stub that never stood for any work is not an id worth retiring.
`FEAT-0004` stayed unused, so the `FEAT` run has a gap there: a gap, not a
renumbering.

### Commits

Reference the id in the footer so history maps back to the ledger:

```
fix(tool-db): keep the schema cache across a reconnect

Refs: BUG-0003
```

---

## v0.7.4 — Planned

### FEAT-0068 · Oracle, DB2 and the other common relational engines
- **State:** Pending · **Source:** `mynotes` L1-7 (1 request)
- **Scope:** Driver, connection form, type mapping and catalog introspection per engine, the way SQL Server / ClickHouse / DuckDB were planned.
- [ ] Add Oracle, DB2 and the other common relational databases.
- [ ] Add refresh button or auto refresh on the database/table list column to refresh list of databases/table when user create/drop database/table in query.
- [ ] All newly added databases can generate DDL and erd diagram.
- [ ] Test: a `drivers` case per engine, running when its `HS_*` variable names a server
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — connect, browse the catalog, run and cancel a query
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** the one item from `mynotes` that is not done. Needs an ADR
  per engine (client crate, licence, `cargo deny`) and a `drivers` test that
  runs when its `HS_*` variable names a server.

---

## v0.7.3 — Planned

### FEAT-0067 · Scratchpad folders and subfolders
- **State:** Pending · **Spec:** docs/tools/scratchpad.md · **Phase:** O
- [ ] folders and subfolders at any depth in the sidebar
- [ ] drag to reorder within a level, drop onto a folder to move; pinned notes keep their place
- [ ] name clash on move or create gets the existing `name 001` suffix
- [ ] folders collapse and remember it; quick-open and search show the path
- [ ] notes-on-disk layout gains directories
- [ ] Test: <what it asserts>
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — create nested folders, drag a note in, clash a name
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** phase O of docs/10-depth-programme.md.

### FEAT-0066 · Queue activity (RabbitMQ, Kafka)
- **State:** Pending · **Spec:** docs/tools/queue.md (to be written) · **Phase:** L
- [ ] spec written from .claude/templates/spec-tool.md before any code
- [ ] ADR for the client crates (`lapin`, `rskafka` — not the C-bound `rdkafka`)
- [ ] Test: <what it asserts>
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — connect to a local broker, peek, publish
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** several steps; split into further FEAT ids once the spec exists.

---

## v0.7.2 — Done 2026-10-31

> Closed on the owner's instruction, 2026-10-31. The three items' **App** boxes
> stay unticked: the features were driven headlessly against SQLite, never on
> screen and never against PostgreSQL or MySQL. ERD **export** was never
> implemented and leaves this batch unbuilt.

### FEAT-0065 · ER diagram from the catalog's foreign keys
- **State:** Completed · **Spec:** docs/tools/database.md · **Phase:** H
- **Branch:** feat/depth-h-sql-2
- [x] `DbSession::table_schema` returns columns, primary key, indexes and foreign keys
- [x] build the ERD from the foreign keys into `hs-diagram`
- [x] open it in the Diagram Editor
- [x] export it — `erd::to_svg`, palette `db.erd_export`, Ctrl+Alt+Shift+D
- [x] Test: `drivers.rs::sqlite_table_schema` reads a real SQLite catalog
  (autoincrement, composite key, unique index, cascade rule, the cap);
  `erd.rs` unit tests cover the cardinality and the edges left out;
  `db_tests.rs::the_er_diagram_reaches_the_graphics_tool` asserts the
  hand-over is a real document with its key badges
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — connect to Postgres, open the ER diagram, export it
- [x] Docs: spec · CHANGELOG · CHECKLIST · GOTCHAS (README needed no change:
  it lists the activity, not its individual tools)
- **Notes:** `crates/tools/tool-db/src/{schema,erd}.rs`. Code landed in
  `6707780`, tests in `e02986e`. **Export landed afterwards** (2026-10-31):
  `erd::to_svg` writes a standalone SVG, laid out with `layout_approx` so it
  needs no fonts and is byte-identical each run. It goes out of the Database
  tab rather than the Diagram Editor, which exports PNG and PDF itself once
  the document is open there. Writing it exposed a defect in `hs-diagram`
  affecting every ERD — entities were measured by their label but drawn with
  their column list, so identifiers wrapped mid-word; `layout::node_size` now
  measures what it draws. **Live servers now verified** by
  `drivers.rs::{postgres_table_schema, mysql_table_schema}` against
  PostgreSQL 16 and MySQL 8. The App box stays open because it asks for the
  running app: everything here is tests, and nothing has been seen on screen.

### FEAT-0064 · DDL generator per engine dialect
- **State:** Completed · **Spec:** docs/tools/database.md · **Phase:** H
- **Branch:** feat/depth-h-sql-2
- **Depends on:** FEAT-0065
- [x] CREATE TABLE / INDEX / FK from the catalog, per engine dialect
- [x] Test: `ddlgen.rs` unit tests cover each dialect's identity spelling,
  SQLite's inlined keys and missing schema prefix, the options and the
  statement order; `db_tests.rs::the_ddl_dialog_opens_with_the_schemas_statements`
  drives the dialog and checks the script it puts in the editor
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — generate DDL for a table on each engine
- [x] Docs: spec · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `crates/tools/tool-db/src/ddlgen.rs`. Code landed in `6707780`,
  tests in `e02986e`. **Live catalogs now verified** (2026-10-31):
  `drivers.rs::{postgres_table_schema, mysql_table_schema}` generate the DDL
  from real PostgreSQL 16 and MySQL 8 catalogs and check the composite key
  and the `ADD CONSTRAINT` form. The App box stays open because it asks for
  the running app, and nothing has been seen on screen. Column types pass
  through as the source spelled them, so cross-dialect output — a PostgreSQL
  schema written out as MySQL — is still the case worth checking by hand.

### FEAT-0063 · Schema and data compare between two connections
- **State:** Completed · **Spec:** docs/tools/database.md · **Phase:** H
- **Branch:** feat/depth-h-sql-2
- **Depends on:** FEAT-0065
- [x] schema compare: tables, columns, types, indexes as New / Missing / Differs
- [x] generated migration script from the differences
- [x] data compare of one table by primary key, sampled, with a bound
- [x] Test: `compare.rs` unit tests cover each verdict, keys matched on
  columns rather than names, the deferred NOT NULL and that no drop or
  delete is ever live SQL; `db_tests.rs` runs both compares against a
  second SQLite database and checks the SQL they generate
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — compare two Postgres schemas, then one table's rows
- [x] Docs: spec · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `crates/tools/tool-db/src/compare.rs`. Code landed in `6707780`,
  tests in `e02986e`. Both compares ran end to end between two SQLite files,
  headlessly, and a live PostgreSQL and MySQL schema each compare clean
  against themselves in `drivers.rs` (2026-10-31) — but a compare between two
  *different* live schemas has still only been done on SQLite. The App box
  stays open: it asks for the running app, and nothing has been seen on
  screen. A target reached through an SSH tunnel is refused rather than
  connected to directly, which is worth a look on screen since it is the one
  path that shows an error instead of a result.

---
  
## v0.7.1 — Done 2026-10-30
> **Shipped in:** app v0.107.0, 2026-10-30

### FEAT-0062 · Devtools depth: network checks, listening ports, snippets, .env editor
- **State:** Completed · **Spec:** docs/tools/devtools.md · **Phase:** G
- [x] DNS, TCP/HTTP ping, TLS certificate viewer, whois
- [x] listening ports with the owning process, filter, copy, guarded kill
- [x] per-workspace snippets with tags and search
- [x] `.env` editor with masked secrets and environment-variable linking
- [x] Test: unit and integration tests in `tool-devtools`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: exercised before merge
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** merged as `60b5247`. This batch happens to share its label with
  the app release that carried it; that is a coincidence of being seeded from
  history, not a rule. Reconstructed into this ledger on 2026-10-31, after the
  fact — later entries are written before the work, not after.

---

## v0.7.0 — Done

### FEAT-0061 · Git activity: rail entry, repository list and a SourceTree-shaped work pane
- **State:** Completed · **Source:** `mynotes` L9-45 (27 requests) · **Kind:** major
- **Scope:** New activity and Settings page; global Git credentials in the vault; per-workspace repositories with icon, project fetch and local path; graph, blame, diff, branches, stashes, staging, commit, pull/push/fetch, merge, tag, git flow, history, project terminal; dashboard, right-click edit/remove, AI assistant prompts.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.6.0 — Done

### FEAT-0060 · One dashboard language across every activity
- **State:** Completed · **Source:** `mynotes` L47-64 (14 requests) · **Kind:** major
- **Scope:** Header band, summary cards in rounded boxes, Start Here, saved-items grid and Open Here, applied from Scratchpad to Editor (which had none), Terminal, SSH, Containers, Database, NoSQL, Redis, S3, API client, Graphics, Devtools and Logs.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.5.1 — Done

### BUG-0035 · Toolbar tooltips, log sidebar proportions, header rows and S3 hosts
- **State:** Completed · **Source:** `mynotes` L66-79 (10 requests)
- **Scope:** Tooltip position above the cursor; saved logs/running services/investigations/filter presets at 30/30/20/20 and scrollable; method-only colouring in the API client header; full-width header bars; Devtools unit-conversion result width; Nexus and WebDAV default hosts; Open Folder As Project routing; Remove From List closing its tab.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.5.0 — Done

### FEAT-0059 · A header row above every work pane toolbar
- **State:** Completed · **Source:** `mynotes` L81-129 (46 requests) · **Kind:** major
- **Scope:** Thick-filled header carrying the item's icon, emphasised name and its own facts (language and version, OS, session, container type, database, dimensions, path) for Scratchpad, Editor, Terminal, SSH, Containers, Database, NoSQL, Redis, S3, API client, Graphics and Logs; the duplicated labels removed from the toolbars; Logs and Diagram toolbars split across rows; empty toolbar rows removed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.4.0 — Done

### FEAT-0058 · Editor activity: project list, workspace sidebar and project settings
- **State:** Completed · **Source:** `mynotes` L131-144 (11 requests) · **Kind:** major
- **Scope:** Sidebar column listing saved projects with language icons and drag ordering; permanent workspace tab and dashboard; Settings -> Editor -> Project List with add/edit in the right column; right-click edit and remove-from-list that closes the tab without touching the folder; per-project Git user info; one tab per project.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.11 — Done

### CR-0023 · Labels beside the icons on every toolbar button
- **State:** Completed · **Source:** `mynotes` L146-155 (8 requests)
- **Scope:** Scratchpad, Editor, SSH, Containers, Database, Graphics and Logs toolbars given icon-plus-label buttons; SSH dimension text dropped and the connection LED moved left of Connect.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.10 — Done

### CR-0022 · Real product icons for terminals, containers, connections and AI providers
- **State:** Completed · **Source:** `mynotes` L157-169 (11 requests)
- **Scope:** Dashboard tab icons matched to their rail icons; WSL, PowerShell 7 and Windows PowerShell distinguished; Docker, Podman and Kubernetes; every connection kind; every AI provider; Workspace and Environment labels in the main toolbar.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.9 — Done

### FEAT-0057 · Multiple SSH forwards per host, and host OS detection
- **State:** Completed · **Source:** `mynotes` L171-177 (5 requests)
- **Scope:** Tunnel card taking several forwards per connection; the target OS detected on add so the sidebar and tab icons follow it; icon properties above transparency; terminal toolbar cleanup; Devtools sidebar selection fixed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.8 — Done

### CR-0021 · Tab icons follow the sidebar icon of what they opened
- **State:** Completed · **Source:** `mynotes` L179-185 (4 requests)
- **Scope:** Terminal shells, containers, Graphics tools and Devtools tools all show in their tab the same icon the sidebar column uses.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.7 — Done

### FEAT-0056 · Icon outlines, bigger dashboard activity icons and cursor-aware tooltips
- **State:** Completed · **Source:** `mynotes` L187-192 (4 requests)
- **Scope:** Theme-aware outline on every icon with a toggle in Settings -> Appearance -> Sidebar; larger spaced activity icons with a hover indicator; tooltips positioned above the cursor without overlapping it.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.6 — Done

### BUG-0034 · Freeze on close, freeze on update, and the toolbar/pin layout
- **State:** Completed · **Source:** `mynotes` L194-203 (7 requests)
- **Scope:** Hang on the window close button and on restart-and-install; sidebar rail and column moved up with toolbars aligned to the work pane; pinned tabs tied to the workspace rather than the environment; S3 tab text following the connection name; home dashboard activity icons; New Note creating a note.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.5 — Done

### BUG-0033 · Container detail views, S3 two-column browsing and one tab per container
- **State:** Completed · **Source:** `mynotes` L205-211 (5 requests)
- **Scope:** Detail view for images, volumes, networks, stacks, services and nodes; S3 buckets moved out of the connection tree into a list-plus-content work pane; a single tab per container; Pin To Home in the container list; bigger add-host icons on one row.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.4 — Done

### BUG-0032 · Drag handles, right-click edit and a Connect button in the connection settings
- **State:** Completed · **Source:** `mynotes` L213-220 (6 requests)
- **Scope:** Draggable icon in the container list; coloured container rail icon; right-click to edit a connection or container straight into its Settings page; Connect beside Test Connection that saves and opens the activity tab; two-column connection settings questioned.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.3 — Done

### FEAT-0055 · Qdrant, Valkey, Nexus and WebDAV connections, plus registry credentials
- **State:** Completed · **Source:** `mynotes` L222-226 (3 requests)
- **Scope:** Four new connection kinds; Docker Hub and Helm credentials stored for authenticated pulls; Parallels Desktop viability for Mac beta testing answered.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.2 — Done

### CR-0020 · Workspace dropdown width and whole-row click
- **State:** Completed · **Source:** `mynotes` L228-231 (2 requests)
- **Scope:** Dropdown halved in width, the entire row including the icon clickable before the delete button, and the list auto-sized to the longest workspace name.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.1 — Done

### FEAT-0054 · Container milestone breakdown
- **State:** Completed · **Source:** `mynotes` L233-242 (0 requests)
- **Scope:** DataTable scrolls, container kinds, Settings -> Containers and vault wiring; the bollard Docker/Podman driver; activity, rail, dashboard and sidebar with one tab per host; the Portainer-shaped Docker pane with Logs and Exec handoffs; Compose and Swarm; the Kubernetes driver and Lens-shaped pane; apply with dry-run diff, port-forward and CRD discovery; AI assistant with action blocks and the Problems feed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.0 — Done

### FEAT-0053 · Container activity: Docker, Podman, Kubernetes and Windows containers
- **State:** Completed · **Source:** `mynotes` L244-257 (11 requests) · **Kind:** major
- **Scope:** New activity between SSH and Database with its own Settings page, dashboard and permanent tab; container list and saved configs in the sidebar at 40/60; everything sensitive in the vault; Portainer-shaped Docker pane and Lens-shaped Kubernetes pane; AI assistant that writes and applies YAML.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.2.1 — Done

### BUG-0031 · Logs sources: icons, path auto-population and permission errors
- **State:** Completed · **Source:** `mynotes` L259-275 (14 requests)
- **Scope:** Service-appropriate icons for saved logs and running services; Node path auto-population; saved logs synchronised across tabs; MySQL and Postgres access-denied; external cmd replaced by the built-in terminal; image metadata card; AI assistant hangs while describing an image; thread-dump reading, upload and paste; per-state thread counts and deadlock cycles.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.2.0 — Done

### FEAT-0052 · Configurable AI assistant prompt buttons
- **State:** Completed · **Source:** `mynotes` L277-284 (5 requests) · **Kind:** major
- **Scope:** Settings -> AI Assistant -> Prompt Button listing every activity's prompts, reorderable and hideable within an activity, with editable name and prompt text; the hardcoded prompts migrated into it; global rather than per workspace.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.32 — Done

### BUG-0030 · Running-service detection, log tab titles and sidebar flicker
- **State:** Completed · **Source:** `mynotes` L286-293 (6 requests)
- **Scope:** Keycloak and Docker-in-WSL detection; saved log name on the tab; path auto-population when editing a running service; flicker when switching saved logs and the performance work behind it; AI assistant limited to filter actions in Logs.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.31 — Done

### BUG-0029 · Log level colouring, source auto-detection and markdown tables
- **State:** Completed · **Source:** `mynotes` L295-299 (3 requests)
- **Scope:** `level=info` and every other level coloured whatever the format; source type, path, command and format auto-filled when editing a running service; markdown table previews made responsive to the AI assistant pane width.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.30 — Done

### FEAT-0051 · Logs: horizontal scrolling, Running Service section and editable saved logs
- **State:** Completed · **Source:** `mynotes` L301-311 (9 requests)
- **Scope:** Horizontal scroll so long lines are readable with the AI pane open; environment column dropped from the saved list in favour of the toolbar selector; draggable and editable saved logs; a Running Service section detecting local and container services with pre-filled configuration; one tab per saved log.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.29 — Done

### CR-0019 · Logs AI assistant in its own column pane
- **State:** Completed · **Source:** `mynotes` L313-315 (1 request)
- **Scope:** The floating assistant replaced with a proper column pane, as the other activities already had.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.28 — Done

### BUG-0028 · Auto format detection for journalctl, level colours and the Logs dashboard
- **State:** Completed · **Source:** `mynotes` L317-321 (3 requests)
- **Scope:** `format = auto` picking the right parser for `journalctl -f -u docker`; `level=<level>` coloured red/green/yellow; a dashboard page for the activity, which had none.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.27 — Done

### CR-0018 · Split Right, Split Down and Close Split removed
- **State:** Completed · **Source:** `mynotes` L323-326 (2 requests)
- **Scope:** The split-pane commands, actions and palette entries taken out of the app after weighing whether to keep them with explicit buttons instead.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.26 — Done

### CR-0017 · A strict 1720x860 minimum window, and window size in Settings
- **State:** Completed · **Source:** `mynotes` L328-332 (3 requests)
- **Scope:** Three-quarter, two-third, half and default screen presets removed leaving full screen and custom; width and height fields in Settings -> General -> Startup that track a resize; bigger Graphics sidebar entries.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.25 — Done

### FEAT-0050 · Image upscaler and enhancer workflows
- **State:** Completed · **Source:** `mynotes` L334-340 (5 requests)
- **Scope:** Upscaler workflow JSON settings including ratio-based services; a proper enhancer workflow distinct from upscaling; an Upscale button beside Generate, Edit and Enhance; the Generate Picture dropdown overlap fixed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.24 — Done

### BUG-0027 · ComfyUI generation failures and model discovery
- **State:** Completed · **Source:** `mynotes` L342-818 (3 requests)
- **Scope:** HIP out-of-memory from CLIPLoader on the supplied workflow JSON; the image-edit model `qwen_image_edit_2509_fp8_e4m3fn.safetensors` not listed when fetching endpoints, only generation models.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.23 — Done

### FEAT-0049 · Signed zip updates, and appearance settings for rail and sidebar
- **State:** Completed · **Source:** `mynotes` L820-826 (5 requests)
- **Scope:** Update download as a zip with its SHA, extracted locally and swapped in; bigger workspace list entries; a workspace selector at the top of Settings -> Appearance -> Activity Rail; a Sidebar section card for the icon toggles.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.22 — Done

### FEAT-0048 · A Logs tab in the bottom dock
- **State:** Completed · **Source:** `mynotes` L828-831 (2 requests)
- **Scope:** Every info, error and debug line in one dock tab with the level set in Settings -> General, and failures from any action surfaced there as they happen.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.21 — Done

### BUG-0026 · ComfyUI execution_start answer, and overlapping controls
- **State:** Completed · **Source:** `mynotes` L833-836 (2 requests)
- **Scope:** The unexpected `execution_start` payload from ComfyUI handled; overlapping actions and information in the generator fixed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.20 — Done

### FEAT-0047 · Screenshots, demo animations and the M4/M5 milestones
- **State:** Completed · **Source:** `mynotes` L838-847 (8 requests)
- **Scope:** Every screenshot and GIF refreshed for the new and changed features; self-hosted request shapes against a real server; DB transaction-state inference; async import/export jobs; the API client milestone (auth including OAuth2 and SigV4, response viewer, history, curl import/export) and the Logs milestone (file engine, SSH source, parsers, filters, highlights, structured view, find, bookmarks, saved sources).
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.19 — Done

### CR-0016 · Manage Collection as tabbed categories
- **State:** Completed · **Source:** `mynotes` L849-855 (5 requests)
- **Scope:** The single settings page split into General, Variables and script tabs; the method-coloured icon dropped from the manage-collection tab; collection variables editable in place; icons on the Graphics dashboard function buttons.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.18 — Done

### BUG-0025 · Raw markdown in AI assistant replies
- **State:** Completed · **Source:** `mynotes` L857-859 (1 request)
- **Scope:** Responses rendered as markdown instead of raw text wherever the assistant answers.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.17 — Done

### FEAT-0046 · Self-hosted image providers: ComfyUI, SwarmUI, Stability Matrix, InvokeAI
- **State:** Completed · **Source:** `mynotes` L861-865 (3 requests)
- **Scope:** Provider entries for the self-hosted stacks, selectable for generation, editing and upscaling, with the AI Assistant settings page rearranged around chat, image and video provider types.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.16 — Done

### FEAT-0045 · DeepSeek provider, and 1min AI image models
- **State:** Completed · **Source:** `mynotes` L867-872 (4 requests)
- **Scope:** DeepSeek with OpenAI-compatible and Anthropic-style placeholders; describe-picture payloads for DeepSeek Flash and Pro; Stable Diffusion XL 1.0 as the 1min AI default for generation and editing.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.15 — Done

### FEAT-0044 · Tab tooltips, per-activity sidebar widths, API client scripts and image AI
- **State:** Completed · **Source:** `mynotes` L874-898 (22 requests)
- **Scope:** Sentence-case tab tooltips; drag handles vertically centred; smaller pinned-note pins; the Scratchpad assistant given its own pane; user-set sidebar width per activity, persisted; lowercase UI text corrected; a functions section in the API client sidebar; package libraries callable from collection and request scripts; Manage Collection from the right-click menu; script inheritance from collection to request; coloured rail icons; sessions disconnected on close; ERD entity text visibility; multi-provider image generation.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.14 — Done

### CR-0015 · Diagram editor toolbar reduced to icons
- **State:** Completed · **Source:** `mynotes` L900-902 (1 request)
- **Scope:** Add Entity, Import DDL, Add Parent and the rest shown as icons with tooltips rather than icon-and-text.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.13 — Done

### FEAT-0043 · Release 2 diagrams: ERD entities and sequence lifelines
- **State:** Completed · **Source:** `mynotes` L904-907 (0 requests)
- **Scope:** `NodeKind::Entity` table nodes with structured columns, and sequence lifeline layout with participants and messages.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.12 — Done

### FEAT-0042 · draw.io import verified against the diagram editor
- **State:** Completed · **Source:** `mynotes` L909-922 (8 requests)
- **Scope:** `drawio_pages()` decoding compressed and inline pages with their names, and `drawio_xml_page(text, page)` importing a chosen page, checked against what was actually implemented.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.11 — Done

### BUG-0024 · Five days of new code reviewed, and window responsiveness
- **State:** Completed · **Source:** `mynotes` L924-928 (3 requests)
- **Scope:** Every function and feature added since the previous session checked; toolbar buttons and multi-pane layouts behaving as the window is resized.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.10 — Done

### BUG-0023 · API client folder rename collisions
- **State:** Completed · **Source:** `mynotes` L930-933 (1 request)
- **Scope:** Renaming a folder to a name that already exists resolved with the suffix rule instead of failing.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.9 — Done

### BUG-0022 · Diagram editor frames, edge labels and properties pane
- **State:** Completed · **Source:** `mynotes` L935-963 (25 requests)
- **Scope:** Outer-frame title position and alignment; inner frames from the toolbar; the doubled separator in the properties pane; edge labels draggable reliably with a selection indicator; tab text overlapping the close button; collection and folder name collisions; scratchpad note reordering; indicators for arrow, frame and edge selection; frame drag and resize; the assistant in its own pane; undo in the image and icon tools; align and distribute actually working.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.8 — Done

### BUG-0021 · Align and distribute in the diagram editor
- **State:** Completed · **Source:** `mynotes` L965-968 (2 requests)
- **Scope:** The non-working align and distribute actions fixed with tests, and the editor toolbar reduced to icons.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.7 — Done

### BUG-0020 · Diagram frames, edge labels and the R2 enhancements
- **State:** Completed · **Source:** `mynotes` L970-980 (9 requests)
- **Scope:** The frame, edge-label and properties-pane defects again, plus API client name collisions, tab text overlap, and the remaining R2 work on ERD and sequence diagrams and the `.diagram.json` alias and schema migration.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.6 — Done

### CR-0014 · Scratchpad reordering, coloured collection icons and the demo GIF
- **State:** Completed · **Source:** `mynotes` L982-989 (6 requests)
- **Scope:** Notes reorderable except pinned ones; coloured material icons for collections and folders; the README demo covering every activity rather than only the Graphics tools, with pauses between steps.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.5 — Done

### BUG-0019 · Disabled dashboard buttons and missing inline AI actions
- **State:** Completed · **Source:** `mynotes` L991-996 (4 requests)
- **Scope:** New Diagram and New Generation enabled; inline action buttons on diagram-editor suggestions; markdown previews everywhere the assistant answers.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.4 — Done

### BUG-0018 · Diagram title box, selection indicators and dashed edges
- **State:** Completed · **Source:** `mynotes` L998-1008 (9 requests)
- **Scope:** Diagram title shown above or below the frame box; vertically centred What To Do cards; indicators when selecting an arrow, frame or edge; edge label and frame repositioning; dashed and dotted edge styles; the assistant in its own pane with inline actions; the Devtools tab-strip close button.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.3 — Done

### FEAT-0041 · Offline diagram drafts and per-node colours
- **State:** Completed · **Source:** `mynotes` L1010-1018 (7 requests)
- **Scope:** An Offline Draft button beside Generate building a diagram from the prompt with local heuristics and type templates; outline, fill, text and edge colours per node; the database shape corrected; separators hidden when nothing is selected.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.2 — Done

### CR-0013 · One UI/UX standard across every tool, written down as a rule
- **State:** Completed · **Source:** `mynotes` L1020-1040 (16 requests)
- **Scope:** Spacing, alignment and component style made consistent starting from the Graphics dashboard; node icons in the outline; properties grouped into sections; source, preview and split views for diagrams; assistant suggestions applied straight to the diagram; proper node shapes; selectable arrows; the licence settled as MIT.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.1 — Done

### CR-0012 · Plan, branch and document before coding, as a standing rule
- **State:** Completed · **Source:** `mynotes` L1042-1048 (3 requests)
- **Scope:** Every new request planned and written into the checklist, README and roadmap before code; a proper branch name first; documentation, screenshots and animations updated after the work, with gotchas and lessons recorded.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.0 — Done

### FEAT-0040 · Graphics activity: image, icon, diagram editor and diagram generator
- **State:** Completed · **Source:** `mynotes` L1050-1083 (8 requests) · **Kind:** major
- **Scope:** Scope settled from the feasibility questions: Archify ideas reimplemented with the MIT notice, `.hdiag` editor-native with a separate IR, draw.io XML import, flowchart and architecture for v1 with ERD and sequence in the first release, diagrams saved under the project's docs folder, drafts kept in Recent, the generator working offline from templates, icon editing with ICO sizes and colour-picker transparency, and an AI assistant with prompt buttons in all four tools.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.60 — Done

### FEAT-0039 · Terminal settings of its own, and a documentation pass
- **State:** Completed · **Source:** `mynotes` L1085-1088 (2 requests)
- **Scope:** Settings page for the terminal to carry future CLIs such as mysql-cli, postgres-cli, Claude Code and opencode; every screenshot and animation refreshed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.59 — Done

### FEAT-0038 · Scrollable sidebars, and the future CLI list
- **State:** Completed · **Source:** `mynotes` L1090-1105 (14 requests)
- **Scope:** Scrolling for the Home, Terminal, SSH, Database, NoSQL, Redis, S3, API client, Browser, Devtools and Scratchpad sidebars, and a plan for adding the CLI entries.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.58 — Done

### FEAT-0037 · Devtools tool list scrollable
- **State:** Completed · **Source:** `mynotes` L1107-1109 (1 request)
- **Scope:** The sidebar tool list scrollable so tools cannot overflow out of reach as more are added.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.57 — Done

### CR-0011 · Checklist entries by date, and a documentation pass
- **State:** Completed · **Source:** `mynotes` L1111-1114 (2 requests)
- **Scope:** The owner's requests recorded in CHECKLIST.md by date, and the documentation, screenshots and animations brought up to date.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.56 — Done

### FEAT-0036 · Credentials revealed and editable in Settings -> Security
- **State:** Completed · **Source:** `mynotes` L1116-1122 (5 requests)
- **Scope:** Unlock showing both password and key, editable and saveable; and the plan-first, branch-first rules restated before the remaining M4 Git work.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.55 — Done

### BUG-0017 · Favicons and spacing on the browser favourites bar
- **State:** Completed · **Source:** `mynotes` L1124-1127 (2 requests)
- **Scope:** Real site favicons on the bar, with left and right padding between folders and links.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.54 — Done

### FEAT-0035 · A setting to show or hide the favourites bar
- **State:** Completed · **Source:** `mynotes` L1129-1131 (1 request)
- **Scope:** Settings -> Browser toggle, with the bar sitting in a row below the toolbar.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.53 — Done

### BUG-0016 · Dragging API client requests between folders
- **State:** Completed · **Source:** `mynotes` L1133-1135 (1 request)
- **Scope:** Requests draggable into other collections, folders, sibling folders and sub-folders, not only within the same one.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.52 — Done

### BUG-0015 · Base64 tool opening, the dock terminal and the Java language server
- **State:** Completed · **Source:** `mynotes` L1137-1143 (4 requests)
- **Scope:** Clicking Base64 opening the tool rather than the fixed tab; the bottom-dock terminal removed as purposeless; jdtls status surviving a restart without a manual refresh; the Maven and Gradle work continued.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.51 — Done

### CR-0010 · Pinned tabs ordered first come, first served
- **State:** Completed · **Source:** `mynotes` L1145-1147 (1 request)
- **Scope:** The first tab pinned sits immediately right of the permanent tab, and later pins follow in order.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.50 — Done

### CR-0009 · Pinned tabs start to the right of the permanent tab
- **State:** Completed · **Source:** `mynotes` L1149-1151 (1 request)
- **Scope:** Applied to every activity that has a permanent fixed tab.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.49 — Done

### FEAT-0034 · Project language detection in the editor
- **State:** Completed · **Source:** `mynotes` L1153-1155 (1 request)
- **Scope:** The workspace detecting the project language so the tab shows it and the matching tooling is offered.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.48 — Done

### CR-0008 · The permanent tab is never overwritten by a connection
- **State:** Completed · **Source:** `mynotes` L1157-1160 (2 requests)
- **Scope:** The fixed first tab reserved for the dashboard, summary or home view, and a terminal in the editor defaulting to PowerShell 7 with the choice in Settings -> Editor.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.47 — Done

### FEAT-0033 · A permanent first tab in every activity
- **State:** Completed · **Source:** `mynotes` L1162-1181 (15 requests)
- **Scope:** Fixed, unclosable, unpinnable first tab for the API client, Terminal, SSH, Database, NoSQL, Redis, S3, Browser, Home, Graphics, Devtools and Scratchpad, carrying the workspace logo and name; collection and folder duplication with the suffix rule; bandwidth units in the unit converter.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.46 — Done

### BUG-0014 · Home recent list alignment
- **State:** Completed · **Source:** `mynotes` L1183-1185 (1 request)
- **Scope:** The Recent section left-aligned instead of centred.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.45 — Done

### CR-0007 · No duplicate main tab in the Graphics tools
- **State:** Completed · **Source:** `mynotes` L1187-1189 (1 request)
- **Scope:** Duplicating the outer tab disallowed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.44 — Done

### FEAT-0032 · Pinned connections grouped by type on Home
- **State:** Completed · **Source:** `mynotes` L1191-1195 (1 request)
- **Scope:** Separators, type headings and colour-coded pills per connection type, ordered to follow Settings -> Appearance -> Activity Rail.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.43 — Done

### FEAT-0031 · Unit Conversion in Devtools
- **State:** Completed · **Source:** `mynotes` L1197-1205 (5 requests)
- **Scope:** Time conversions including epoch, numeric base conversions with a dedicated result per type, and the other conversions worth having, built to the same UI standard.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.42 — Done

### BUG-0013 · Pin To Home, duplicate naming and request dragging
- **State:** Completed · **Source:** `mynotes` L1207-1221 (12 requests)
- **Scope:** Pin To Home missing from the Database, NoSQL, Redis and S3 sidebars; workspace icon on the Home header; pinned connections grouped by type; right-click menus disabled on outer tabs where they do not apply; duplicate and new-request naming with the space-suffix rule; a request-name row above the action bar; dragging requests between folders; one browser tab per workspace; Split Right and Split Down removed from tab menus.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.41 — Done

### FEAT-0030 · Open Data creating its own query tab
- **State:** Completed · **Source:** `mynotes` L1223-1225 (1 request)
- **Scope:** Opening data from the table list creating an Untitled query tab rather than replacing the current one.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.40 — Done

### BUG-0012 · Request method dropdown alignment
- **State:** Completed · **Source:** `mynotes` L1227-1229 (1 request)
- **Scope:** The dropdown aligned with the URL field on the same line.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.39 — Done

### FEAT-0029 · SQL prettifier, and standard method colours
- **State:** Completed · **Source:** `mynotes` L1231-1233 (2 requests)
- **Scope:** The prettifier button landed with the query word-wrap toggle deliberately held back; GET green, POST blue and the rest standard, colouring only the method.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.38 — Done

### FEAT-0028 · Query prettifier, wrap toggle and the space-suffix rule
- **State:** Completed · **Source:** `mynotes` L1235-1239 (3 requests)
- **Scope:** Multi-line indented SQL formatting in the open query tab; a wrap toggle for horizontally scrolling queries; duplicate naming restored to " 001" with the leading space.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.37 — Done

### BUG-0011 · Result grid not resizing when the AI pane opens
- **State:** Completed · **Source:** `mynotes` L1241-1243 (1 request)
- **Scope:** The table view reflowing instead of being overlapped by the assistant column.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.36 — Done

### CR-0006 · Get Started order, script pane heights and one tab per request
- **State:** Completed · **Source:** `mynotes` L1245-1252 (6 requests)
- **Scope:** Cards ordered New Workspace, Environments, New Connection, Open Folder as Project, New Terminal, Quick Open; pre-request and post-response script sections given equal height; one tab per request with duplication handled; requests draggable between parent and sub-folders; misaligned dialog buttons fixed; middle-click slowness deferred as not reproducible.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.35 — Done

### FEAT-0027 · One database connection per tab, with query tabs inside it
- **State:** Completed · **Source:** `mynotes` L1254-1260 (5 requests)
- **Scope:** A single connection per outer tab with multiple query tabs beneath it; history and saved queries moved beside the version and filter columns; the reveal-password dialog buttons aligned.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.34 — Done

### CR-0005 · Connection string column removed from the credentials table
- **State:** Completed · **Source:** `mynotes` L1262-1264 (1 request)
- **Scope:** The unused column dropped and the remaining columns given sensible widths.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.33 — Done

### CR-0004 · AI Assistant button hard right in every toolbar
- **State:** Completed · **Source:** `mynotes` L1266-1268 (1 request)
- **Scope:** Applied across every connection and activity toolbar where it appears.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.32 — Done

### BUG-0010 · Sentence case in parentheses, and SFTP operations
- **State:** Completed · **Source:** `mynotes` L1270-1273 (2 requests)
- **Scope:** "None (Direct connection)" and its kind corrected throughout; new folder, new file, permission, owner and group changes working in the SFTP pane.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.31 — Done

### CR-0003 · Generator results tied to their own type
- **State:** Completed · **Source:** `mynotes` L1275-1277 (1 request)
- **Scope:** UUID, number, password and passphrase each keeping their own result rather than sharing one field.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.30 — Done

### FEAT-0026 · Cron expression explained in detail
- **State:** Completed · **Source:** `mynotes` L1279-1281 (1 request)
- **Scope:** Checking an expression also showing the schedule it describes and the next run times.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.29 — Done

### FEAT-0025 · Generators, and TOML and XML verifiers in Devtools
- **State:** Completed · **Source:** `mynotes` L1283-1287 (2 requests)
- **Scope:** Time-based and random UUIDs, seeded random numbers with bounds, password and passphrase generators with criteria, and TOML and XML verification and prettifying.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.28 — Done

### FEAT-0024 · A Credentials section in Settings -> Security
- **State:** Completed · **Source:** `mynotes` L1289-1291 (1 request)
- **Scope:** Every encrypted connection credential listed below the timer section, revealable with the master password.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.27 — Done

### BUG-0009 · Three sessions per SSH host, tab favicons and pinned tab text
- **State:** Completed · **Source:** `mynotes` L1293-1309 (15 requests)
- **Scope:** Up to three tabs per host as separate sessions; Home links focusing an already-open connection; site favicons on browser tabs; pinned tabs keeping their text with an indicator; one tab per image in Graphics; the six-dot drag handle; the browser address field editable; bigger Home section headings with Pinned Connections above Recent in a grid; real pinned connections rather than mock ones; the database list showing only configured connections; SFTP new-file errors; the Database assistant keeping its pane.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.26 — Done

### FEAT-0023 · Cron expression generator in Devtools
- **State:** Completed · **Source:** `mynotes` L1311-1313 (1 request)
- **Scope:** Generating expressions for Unix cron, Spring and Quartz.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.25 — Done

### FEAT-0022 · Test Connection in the connection settings
- **State:** Completed · **Source:** `mynotes` L1315-1317 (1 request)
- **Scope:** A Test Connection button between Save and Delete, reporting success or the failure reason.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.24 — Done

### FEAT-0021 · Browser favourites and passwords, and a full SFTP context menu
- **State:** Completed · **Source:** `mynotes` L1319-1376 (51 requests)
- **Scope:** Favourites and Passwords tabs with folders and favicons; Add To Favorites and Fill In Password on the toolbar; show/hide checkboxes in the activity rail with auto-reveal when a hidden activity is needed; New File and Permission in the SFTP pane; a full right-click menu with Open In Editor, Rename, Download, Download Zipped, Copy, Copy Path, Delete and Permission; double-click on empty space going up a directory; middle-click tab opening speed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.23 — Done

### FEAT-0020 · Browser favourites and passwords, and the SFTP context menu (restated)
- **State:** Completed · **Source:** `mynotes` L1378-1435 (51 requests)
- **Scope:** The same batch restated with British spellings; kept as its own entry because it was sent as its own request.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.
- **Notes:** a restatement of FEAT-0021; kept as its own entry because it
  arrived as its own request.

## v0.0.22 — Done

### BUG-0008 · Docked inspector, sentence case and Redis alignment
- **State:** Completed · **Source:** `mynotes` L1437-1447 (9 requests)
- **Scope:** The developer tools pinned below the page instead of floating; parenthesised wordings corrected; the Redis key list aligned and indented; the Redis version in sentence case; sidebar headers aligned with their add buttons; clipboard copy to and from the SFTP pane.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.21 — Done

### CR-0002 · A connection column in the Redis work pane
- **State:** Completed · **Source:** `mynotes` L1449-1453 (3 requests)
- **Scope:** A proper Connection header with its options, the redundant new-connection button removed from the toolbar, and sidebar headers aligned across SSH, Database, Redis and S3.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.20 — Done

### FEAT-0019 · Redis connection list in the work pane
- **State:** Completed · **Source:** `mynotes` L1455-1457 (1 request)
- **Scope:** A column listing every Redis connection so one can be chosen to connect to.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.19 — Done

### FEAT-0018 · Sidebar rail scrollable with fixed anchors
- **State:** Completed · **Source:** `mynotes` L1459-1461 (1 request)
- **Scope:** Overflow scrolling while Home stays at the top and the sidebar toggle and settings stay in place.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.18 — Done

### BUG-0007 · Devtools YAML input leaking into the converter
- **State:** Completed · **Source:** `mynotes` L1463-1465 (1 request)
- **Scope:** Text typed into the Check tab no longer copied into the YAML to Properties field.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.17 — Done

### BUG-0006 · Workspace cards misaligned on Home
- **State:** Completed · **Source:** `mynotes` L1467-1469 (1 request)
- **Scope:** The workspace list cards aligned to each other instead of drifting up and down.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.16 — Done

### FEAT-0017 · AI providers beyond OpenAI-compatible
- **State:** Completed · **Source:** `mynotes` L1471-1473 (1 request)
- **Scope:** Anthropic, Gemini, Microsoft, Kimi, Tencent and the others added as first-class provider options.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.15 — Done

### FEAT-0016 · YAML and properties conversion, and auto-prettifying
- **State:** Completed · **Source:** `mynotes` L1475-1479 (3 requests)
- **Scope:** Two-way YAML and properties conversion in Devtools; JSON and js.map prettified on open in the editor; first-character selection in the terminal fixed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.14 — Done

### BUG-0005 · Workspace icons missing from the dropdowns
- **State:** Completed · **Source:** `mynotes` L1481-1484 (2 requests)
- **Scope:** The workspace or folder icon shown beside the name in the startup dropdown and the environment comparison header.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.13 — Done

### FEAT-0015 · AI suggestions in every tool, Insert and Run, and a URL codec
- **State:** Completed · **Source:** `mynotes` L1486-1495 (8 requests)
- **Scope:** Explain, review and improve options beyond the editor; Insert And Run for shell and query suggestions; request and response bodies prettified; a percent-encoding URL encoder and decoder; language-server install buttons; standardised "AI Assistant" button text; visible selection highlight in the terminal.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.12 — Done

### FEAT-0014 · Deleting a workspace, and workspace icons everywhere
- **State:** Completed · **Source:** `mynotes` L1497-1500 (2 requests)
- **Scope:** A workspace deletable with its environments and data while one always remains, and its icon shown in every dropdown, list and comparison header.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.11 — Done

### BUG-0004 · Speech to text, Home workspace list and syntax highlighting
- **State:** Completed · **Source:** `mynotes` L1502-1507 (4 requests)
- **Scope:** Dictation returning nothing after Stop Listening; bigger Home workspace entries with their own icons; syntax highlighting for editor files, scratchpad notes, database binaries, API bodies and decoded base64; tool names under icons on by default.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.10 — Done

### BUG-0003 · Graphics sliders and the grayscale toggle
- **State:** Completed · **Source:** `mynotes` L1509-1512 (2 requests)
- **Scope:** Adjustment sliders previewing live, and Grayscale toggling back like Invert.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.9 — Done

### BUG-0002 · Crop handles limited to left and right
- **State:** Completed · **Source:** `mynotes` L1514-1516 (1 request)
- **Scope:** The crop placeholder resizable from the top, bottom and corners as well.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.8 — Done

### FEAT-0013 · Dictation into the AI assistant
- **State:** Completed · **Source:** `mynotes` L1518-1520 (1 request)
- **Scope:** A toggle in every assistant that types spoken instructions into the prompt field.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.7 — Done

### CR-0001 · Renaming the app considered and declined
- **State:** Completed · **Source:** `mynotes` L1522-1524 (1 request)
- **Scope:** Alternative names weighed because HyperStack exists as another product; the decision recorded as no rename.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.
- **Notes:** outcome was "no need to rename" — the entry records the decision.

## v0.0.6 — Done

### FEAT-0012 · Crop placeholder, scratchpad assistant and context menu wrapping
- **State:** Completed · **Source:** `mynotes` L1526-1530 (3 requests)
- **Scope:** A resizable crop area that fills the input boxes; an assistant in the scratchpad writing notes, checklists, mermaid diagrams and code fences into the markdown; the long-title context menu kept on one line.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.5 — Done

### FEAT-0011 · Pinning scratchpad notes to the top
- **State:** Completed · **Source:** `mynotes` L1532-1534 (1 request)
- **Scope:** Notes selectable for pinning, held above the rest of the list.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.4 — Done

### FEAT-0010 · Scratchpad dual pane, and an assistant in the API client
- **State:** Completed · **Source:** `mynotes` L1536-1543 (6 requests)
- **Scope:** Raw markdown on the left and preview on the right with the note list between sidebar and workspace; an assistant that creates collections and requests directly in the API client; tidier Graphics attribute panes; the speak-aloud button spacing; a performance pass over the new code.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.3 — Done

### FEAT-0009 · Nushell, vaulted Devtools secrets and variable peeking
- **State:** Completed · **Source:** `mynotes` L1545-1550 (4 requests)
- **Scope:** Nushell detected and offered in the shell lists; Firebase JSON, private, public and secret keys stored encrypted in the vault through the environment; more than one tab per Devtools component with values surviving a restart; a way to view and copy the value behind a `{{variable}}` in any field.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.2 — Done

### BUG-0001 · Digest list columns in Devtools
- **State:** Completed · **Source:** `mynotes` L1552-1554 (1 request)
- **Scope:** The hash digest list given proper table columns.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.1 — Done

### FEAT-0008 · The founding wishlist: browser, API client, graphics, accessibility and workspace settings
- **State:** Completed · **Source:** `mynotes` L1556-2237 (60 requests)
- **Scope:** Workspace icons set and edited after creation and shown in the dropdown; a pinned settings tab; one editor tab per sub-project with wider sub-tabs and multiple encodings; the AI assistant extended to SSH, terminal, SQL, S3 and the API client; the colour palette, Notification and Quick Open buttons; the app logo in the title bar; dark mode surviving a restart on Windows 10; binary preview in the database cell viewer; settings import and export behind the master password; text to speech per area; a welcome entry and optional labels in the sidebar; the file path row under the editor sub-tabs; a secured multi-tab browser with ad and script blocking; a Postman-compatible API client with custom scripts; and the Graphics tools for conversion, enhancement, scaling, extending and icon work.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.
