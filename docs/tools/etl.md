# Tool spec — ETL

**Status:** draft, decisions recorded (§10) · **Last updated:** 2026-11-26 · **Phase:** R
(`docs/10-depth-programme.md`) · **Decisions:** ADR-035

## 1. Purpose

Read a team's **Apache Hop** work inside HyperStack: open a Hop project,
walk its pipelines and workflows, and see each one drawn as the canvas
Hop GUI draws — the real transforms at the real positions, with the real
hops between them — beside the project's metadata, its variables and its
unit tests.

Hop's design files are an open format that carries each node's own
coordinates, so the canvas is not a re-layout or a picture fetched from a
server: it is the designer's own drawing, rebuilt from the file. That is
what this activity is for.

It is **not** an authoring tool. It never writes a `.hpl` or a `.hwf`, it
does not create or edit transforms, hops, metadata or variables, and it
does not replace Hop GUI for designing. Every item has **Open in Editor**
for the file itself.

**Phase 1 reads files only** and needs no server and no credential at all
(§5). Hop Server — run a pipeline, read its log — is phase 2 (§4.9).

## 2. User stories

- As a data engineer, I want to open a teammate's pipeline and see its
  canvas without launching Hop GUI, so that I can follow what it does
  while reading the ticket beside it.
- As a developer, I want a workflow's actions and the success and failure
  paths between them, so that I can see what happens when a step fails.
- As a reviewer, I want a transform's own fields listed, so that I can
  check a changed SQL statement or file path in a pull request.
- As a data engineer, I want the project's database connections with
  their host and database, so that I can open one in the Database
  activity and query it directly.
- As anyone, I want the project's variables and unit tests listed, so
  that I can see what the pipelines are parameterised by and what is
  covered.
- As a lead, I want to export a pipeline's canvas as an SVG, so that I
  can put it in a design document.

## 3. Layout

### 3.1 Dashboard (the permanent home tab)

Through `hs_ui::dashboard`, like the other activities: the band; the
summaries *Projects*, *Pipelines*, *Workflows*, *Open Tabs* (figures from
the projects open in tabs); **Start Here** with **Add Hop Project**;
**Saved Projects** (a grid, each with the Hop mark); **Recent** (the
files opened in this workspace, with Clear History and Remove behind a
confirmation, CR-0067); **Open Here**.

### 3.2 Sidebar

The saved Hop projects, one list, each row the Hop mark, the project's
name and its path; the grip to reorder (`hs_ui::reorder`); right-click
Open, Edit, Delete (confirmed, CR-0067).

### 3.3 A tab per project

```
┌ header band: [Hop mark] Sales ETL · D:\work\sales-etl · 14 pipelines · 3 workflows ┐
│ [⇤ Browse] [⟳ Refresh] [🔍 Search] [⤓ Export SVG] [✎ Open in Editor]      [✦ AI] │
├──────────────────────┬────────────────────────────────────────────────────────────┤
│ [Filter…          ]  │ 🔀 load-customers.hpl                                      │
│ ▾ 📁 Pipelines       │ Pipeline · 7 transforms · 6 hops                           │
│   ▾ 📁 staging       │ ┌────────────────────────────────────────────────────────┐ │
│      🔀 load-cust…   │ │   ▭ Table input ──▶ ▭ Filter rows ──▶ ▭ Table output   │ │
│      🔀 load-orders  │ │                           ╲                            │ │
│   🔀 build-marts     │ │                            ╌▶ ▭ Write to log (disabled)│ │
│ ▾ 📁 Workflows       │ └────────────────────────────────────────────────────────┘ │
│      ⛭ nightly.hwf   │ ── Transform: Filter rows ──────────────────────────────── │
│ ▾ 📁 Metadata        │ Type        FilterRows                                     │
│   🗄 Connections (4) │ Condition   amount > 0                                      │
│   ⚙ Run Configs (2) │ Send true   Table output                                    │
│   🧪 Unit Tests (6)  │ Send false  Write to log                                    │
│ 📋 Variables (9)     │                                                            │
└──────────────────────┴────────────────────────────────────────────────────────────┘
```

- **Header band** (`hs_core::tool::ToolHeader`): the Hop mark and colour,
  the project's name, its path, and its pipeline and workflow counts. The
  band wears the same mark as the tab's icon and the sidebar row (owner,
  2026-11-06).
- **Toolbar** (main size): the **Browse** toggle first (CR-0073), then
  **Refresh**, **Search**, **Export SVG**, **Open in Editor**; the AI
  assistant hard right through `toolbar::hard_right`.
- **Left browse column** — resizable, hideable, keyboard-navigable, never
  narrower than its widest section name: a filter field, then the tree of
  §4.2 (Pipelines, Workflows, Metadata, Variables), folders first then
  items by name, each with its kind's coloured picture (Material pictures
  when on, CR-0068).
- **Right: the item page** — a structured header (the item's picture and
  name; its kind and counts), then the canvas (§4.3) with the selected
  node's detail below it (§4.4). With nothing selected it says so ("Pick
  a pipeline or a workflow to see it here.").
- `hs_ui::env_banner` and the 2 px frame when the project's environment
  is protected. Phase 1 writes nothing, so the banner is informational;
  the guard bites in phase 2 (§6).

### 3.4 Settings → ETL

The saved projects listed left and the form right, like Settings →
Tracker; the form is a name, a path with a Browse button
(`form::centred_row_wrapped`, the field leaving the button room —
BUG-0069) and an optional environment. Below the list, the limits of §7
(the largest file parsed, the most nodes drawn).

### 3.5 Every screen keeps the layout standard

`.claude/rules/ui.md`'s *Layout and controls standard* applies to each
screen here: field heights and middle lines, `PANE_MARGIN`, `PANE_GAP`,
`ROW_GAP`, `DataTable` for every read-only table, `form::detail_table`
for item facts, the empty / loading / error states through the shared
helpers, Title Case labels and sentence-case hints from Fluent, and the
*Checking a screen* list before a screen is reported done.

## 4. The sections

### 4.1 The project model

A Hop project is a folder. What this activity reads from it:

| File | Holds |
|---|---|
| `project-config.json` | `metadataBaseFolder`, `unitTestsBasePath`, `dataSetsCsvFolder`, `enforcingExecutionInHome`, and `config.variables` (name, value, description) |
| `*.hpl` | a pipeline (§4.3) |
| `*.hwf` | a workflow (§4.3) |
| `<metadataBaseFolder>/rdbms/*.json` | database connections (§4.5) |
| `<metadataBaseFolder>/pipeline-run-configuration/*.json` | how a pipeline runs (local, remote, Beam) |
| `<metadataBaseFolder>/workflow-run-configuration/*.json` | how a workflow runs |
| `<metadataBaseFolder>/dataset/*.json` | the datasets unit tests use |
| `<metadataBaseFolder>/unit-test/*.json` | Hop's own unit tests (§4.5) |

`metadataBaseFolder` is read from `project-config.json` and may contain
`${PROJECT_HOME}`, which resolves to the project's folder. A project
folder with no `project-config.json` is still opened: the files are
listed and `metadata/` is assumed, with a hint saying so.

A project is added by path; there is no server and no credential (§5).

### 4.2 The tree

- **Pipelines** and **Workflows** are two sections, each the project's
  folder structure beneath it, **folders first, then items, each by
  name** (CR-0038) — the Redis key tree is the model.
- **Metadata** has a child per kind that exists (Connections, Run
  Configurations, Datasets, Unit Tests) with its count; **Variables** is
  a leaf with its count.
- Each row carries its kind's coloured picture from `hs_ui::fileicons`
  when `material_icons(etl)` is on, else a coloured glyph (CR-0065,
  CR-0068). A pipeline and a workflow have distinct pictures, the same
  ones their tabs wear.
- **Search** (toolbar, `Ctrl+Alt+H`): a field over the project's file
  names **and** the transform and action names inside them, results in
  the left column with their file path. The index is built once per
  Refresh, not per keystroke (§7).

### 4.3 The canvas

The centre of the activity. One model, `FlowGraph`, holds what both file
kinds reduce to, and one renderer draws it:

- a **node**: id, label, kind, position, and an optional status overlay
  (phase 2);
- an **edge**: from, to, `via` waypoints, a label, and a style.

Each file kind converts into it, so neither parser knows the renderer and
the renderer knows neither parser.

#### 4.3.1 A pipeline (`.hpl`)

XML with a `<pipeline>` root:

- `<transform>` per node — `<name>`, `<type>` (for example
  `RowGenerator`, `FilterRows`, `TableInput`), `<description>`,
  `<distribute>`, the type's own fields, and `<GUI><xloc>`/`<yloc>` for
  the position.
- `<order>` holds the hops: `<hop>` with `<from>`, `<to>` and
  `<enabled>`.
- `<transform_error_handling>` holds the error hops, drawn apart (§4.3.3).
- `<notepads>` become labels on the canvas.

#### 4.3.2 A workflow (`.hwf`)

XML with a `<workflow>` root, and a slightly different shape — the
coordinates sit directly on the action, not inside a `<GUI>` element:

- `<actions><action>` per node — `<name>`, `<description>`, `<type>`
  (for example `SPECIAL`, `JOIN`, `SUCCESS`, `DELAY`, `FILE_EXISTS`),
  `<parallel>`, the type's own fields, and `<xloc>`/`<yloc>`.
- `<hops><hop>` with `<from>`, `<to>`, `<enabled>`, `<evaluation>` and
  `<unconditional>`.

The edge style follows those two flags:

| `unconditional` | `evaluation` | Drawn as |
|---|---|---|
| `Y` | — | unconditional (plain) |
| `N` | `Y` | the success path |
| `N` | `N` | the failure path |

This mapping is read from Hop's sample workflows, not from a documented
table; the parser keeps both raw flags on the edge so the drawing can be
corrected without reparsing, and an unexpected combination draws as
unconditional with the flags shown in the edge's tooltip.

#### 4.3.3 Drawing

- Positions come from the file. A node with no coordinates (a
  hand-edited file) falls back to `hs_diagram::layout_approx` for the
  whole graph, with a hint saying the layout is computed.
- **Edge styles:** plain for a normal hop, **dashed** for `enabled=N`,
  the success and failure paths of §4.3.2 apart from each other, and
  error-handling hops apart again. Colours come from `hs_ui::theme` and
  the shared palette; the success and failure paths use the shared
  log-level colours (CR-0077), never a local palette.
- **Node kinds:** a picture per transform or action type where one is
  known, else the generic node picture in the kind's colour. Hop ships
  about 250 transform types; an unknown type is drawn, named and listed,
  never dropped.
- **Interaction:** pan and zoom, click to select (the detail below,
  §4.4), arrow keys to move between nodes in reading order, Enter to
  open the selected node's detail, `Home` to fit. Hover highlights a
  node's fill or draws a border — it never grows it or scales its
  picture (`ui.md`; `Theme::apply` zeroes widget expansion).
- **Export SVG** writes the canvas through `hs_diagram::export::svg`,
  saved with the shared download dialog.
- The graph is converted and laid out **once per file change**, keyed on
  a hash of the file's bytes, and drawn from the cached geometry — never
  reparsed or re-laid-out inside `ui()` (`.claude/rules/rust.md`).

### 4.4 Transform and action detail

Under the canvas, the selected node's facts in a
`hs_ui::form::detail_table` (a table with no header, CR-0028): its name,
type, description, and the type's own fields as name and value pairs,
read generically from the XML so a type this activity has never heard of
still shows everything it carries.

- A field holding a `${variable}` reference shows its value from the
  project's variables on hover, as `varfield` does elsewhere (owner,
  2026-11-07); an unset one says so.
- A field whose key is a secret by the **whole-word** rule (§6) is
  masked, with the eye button to reveal — and nothing is revealed that
  came from Hop's encoded password form, which is never decoded (§6).
- Long values (an SQL statement, a path) go in a `form::well`; SQL is
  shown through the read-only editor engine with SQL colours.

### 4.5 Metadata and unit tests

Each metadata kind is a `DataTable` with the clicked row's detail below
it:

- **Connections** (`rdbms`): name, engine (`pluginId`), host, port,
  database, username. The `password` field is **never shown and never
  decoded** (§6). **Open in Database** matches the connection against the
  saved `db` connections on engine, host, port and database — host
  compared without case, `localhost` equal to `127.0.0.1` — and opens a
  new Database tab on the match, exactly as Analytics does
  (`analytics.md` §4.3). No match or several opens the tab with the
  connection picker on the candidates and a hint saying why. The
  `OpenRequest` carries the saved connection's id, **never a credential**.
- **Run Configurations** (pipeline and workflow): name, engine type, and
  its fields in the detail table.
- **Datasets:** name, the CSV folder's file, and its columns.
- **Unit Tests:** name, the pipeline it covers, its type (unit test or
  golden data set), and its tweaks and golden rows. Listed in phase 1;
  **running** one is phase 2 (§4.9).

An empty kind shows its header and a one-line sentence-case hint ("This
project has no datasets yet."), never a bare header (CR-0061).

### 4.6 Variables

The project's variables in a `DataTable`: name, value, description, and
where it came from (`project-config.json` or an environment file). A
value whose **name** is a secret by the whole-word rule is masked (§6).

### 4.7 What every page has

**Open in Editor** (the file in the Editor activity through
`OpenRequest`), **Copy Path**, and the item's facts (path, size, when it
changed, transform or action count) in a `form::detail_table` under a
disclosure. The AI assistant's context is the file's name, kind, the node
names and types, and the selected node's fields — never a masked value
and never an encoded password.

### 4.8 Phase 1 needs no server

Stated plainly because it shapes everything above: phase 1 opens a folder
and reads files. There is no connection to test, no credential to store,
no token to refresh, no network error state, and no container in the
bench (§8.4). The whole activity's phase 1 is exercised against Hop's own
sample files.

### 4.9 Hop Server (phase 2, specified, not built now)

What the server adds, and what to expect of it:

- **Connection kind `hop-server`:** URL, Credential **Username and
  Password** (Hop Server's authentication is on by default and refuses an
  unauthenticated request with a 401), Verify TLS and CA File
  (`hs_core::tls`).
- **Status and runs:** the server's registered and running pipelines and
  workflows, a run's status and its log; **Run** a pipeline or workflow
  with its run configuration and parameters, as a Job with cancel; run a
  unit test.
- **The API is version-gated**, as Metabase's is (`analytics.md` §8.3).
  The released manual (2.18.0) documents the older `/hop/...` web
  services — `hop/status`, `hop/pipelineStatus`, `hop/workflowStatus`,
  `hop/execPipeline`, `hop/execWorkflow` — which return HTML or XML with
  `&json=Y` as an optional format. A JSON API under `/hop/api/v1/` is
  documented only in the pre-release manual, which also notes that the
  earlier separate `hop-rest` module has been removed; whether a given
  server has it is read from the server and the unavailable calls are
  drawn disabled with the reason. A Hop developer has said the older
  endpoints were never designed to be used without Hop GUI or Hop Run, so
  the provider treats a missing or differently-shaped answer as a
  version fact, not an error.
- **Hop's environments** (its own dev / test / prod variable sets) map
  onto HyperStack's environment tiers (FEAT-0106), so a Hop `prod`
  environment takes the **protected-environment guard** and its runs are
  refused until Allow Writes is set for the tab.

### 4.10 Later (not specified further now)

Hop's `hop-import`, which converts Pentaho / Kettle `.ktr` and `.kjb`
files into Hop's format, gives a PDI shop partial coverage here if they
run the import themselves; reading `.ktr`/`.kjb` directly is not planned.
Beam, Spark and Flink run configurations are listed in phase 1 but their
remote runs are not driven. Hop Web (`apache/hop-web`) as an **Open in
Browser** target, where a team runs it.

## 5. Connections and credentials

**Phase 1 stores no credential.** A project is an entry in the ETL
activity's own list — a name, a folder path and an optional environment —
not in `hs_core::connections` and not in the vault.

The list reuses the **type** `hs_core::projects::Projects` for its shape,
storage and reordering, loaded from the activity's **own file**
(`etl-projects.toml`) with `Projects::load`. It is deliberately **not**
`Services.projects`, the shared list the Editor and Git read: putting a
Hop project there would make it appear in the Editor's and Git's project
sidebars, which is a cross-tool side effect nobody asked for. Adding a
Hop project changes nothing outside this activity.

Phase 2 adds connection kind `hop-server` to `hs_core::connections`
(§4.9) with its secret as a `secret:<ulid>` through
`hs_core::service_auth`, Use Saved Credential (FEAT-0125) and Test
Connection, like every other connection form.

## 6. Guards

- **Read-only, entirely** (owner, 2026-11-26). Nothing this activity does
  writes a `.hpl`, a `.hwf`, a metadata file or `project-config.json`.
  Editing, creating, renaming and deleting a pipeline, a workflow, a
  transform, a hop, a connection or a variable are **never offered**.
  Export SVG writes only the file the user picks. With no write there is
  no audit log here.
- **Hop's stored passwords are never decoded.** A connection's
  `password` field in `metadata/rdbms/*.json` holds Hop's encoded form —
  the prefix `Encrypted ` followed by hex, produced by Hop's
  `HopTwoWayPasswordEncoder`, which is reversible by design. This
  activity:
  - never decodes one, although it could;
  - never shows one, not even encoded, and never offers an eye button
    for it;
  - never caches one — the field is dropped at the end of parsing and
    never reaches a cache, the AI context or the clipboard;
  - redacts the pattern `Encrypted\s+[0-9a-fA-F]{8,}` from every log
    line and error message, which is the same pattern Hop's own
    `CredentialRedactor` uses.
- **A variable can be a secret too.** A password field may hold
  `${DB_PASSWORD}` instead of an encoded value, and variables carry real
  values. A variable or field is masked when its **key** holds a secret
  word by whole words — `SMTP_PASS` and `dbPassword` masked,
  `AUTH_LOCKOUT_THRESHOLD` not (owner, 2026-11-07) — through the shared
  function, never a substring match.
- **Open in Database passes identity, not credentials:** engine, host,
  port and database to match a saved `db` connection, and the
  `OpenRequest` carries that connection's id. A Hop connection's own
  username and password never travel (§4.5).
- **Protected environments** matter in phase 2: a project whose
  environment is protected refuses every run verb, as other activities'
  query editors do.
- **Portability:** the activity stores only the project list and the
  recent files, under `HyperStackData/`. It never writes inside the
  user's Hop project folder.

## 7. Work and progress

- Parsing and the file walk run on the tokio runtime
  (`hs_core::runtime`); results reach the UI through **bounded** channels
  drained at frame start; nothing blocks the UI thread.
- Opening a project (the walk, `project-config.json`, the metadata) and
  parsing a large file are **Jobs** (`hs_core::jobs`) with progress and
  **cancel**; cancel stops the walk rather than hiding it.
- Bounds, in Settings (§3.4): the largest file parsed (default 16 MB,
  refused above with the reason), the most nodes drawn (default 2,000,
  beyond which the canvas says so and offers the node list instead), and
  the search index capped at the project's files.
- A file's graph is cached by a hash of its bytes and dropped when its
  tab closes; Refresh re-walks the project and rebuilds the search index.
- The canvas is drawn from cached geometry. Nothing is parsed, laid out
  or measured inside `ui()`.

## 8. Implementation notes

### 8.1 Crate and files

`crates/tools/tool-etl/` (`EtlFactory`, descriptor id `etl`, category
Data, rail after Analytics):

```
project.rs              project-config.json, the folder walk, metadata, variables
hopfile/mod.rs          the shared XML helpers and the file kind
hopfile/pipeline.rs     .hpl → nodes, hops, notepads
hopfile/workflow.rs     .hwf → actions, hops with their flags
graph.rs                FlowGraph and the conversion from either file kind
secrets.rs              the whole-word mask and Hop's Encrypted redaction
ui/{mod,tab,dashboard,sidebar,tree,canvas,detail,metadata,variables}.rs
```

Depends on `hs-core`, `hs-ui`, `hs-diagram` and `roxmltree`; **no other
tool crate**. **Open in Editor** and **Open in Database** go through
`Services` and `OpenRequest`.

**There is no provider trait** (ADR-035). One product needs no
abstraction (CLAUDE.md §2, `.claude/rules/rust.md`): the parsers, the
graph and the pages are Hop's. If a second ETL product is ever added, the
trait is extracted then, from two working implementations.

### 8.2 Shared pieces reused

`hs_core::projects::Projects` as the list's type, loaded from the
activity's own `etl-projects.toml` and **not** from `Services.projects`
(§5), `hs_core::jobs`, `hs_core::runtime`, `hs_ui::{dashboard, tree,
table::DataTable, form, toolbar, reorder, env_banner, fileicons,
ai_assistant, varfield}`, the read-only editor engine (`hs-editor`) for
SQL and long values, `hs_core::download` for Export SVG.

From `hs-diagram`: `model` (`Node`, `Edge`, `Point` — `Edge::via` is
already a waypoint list), `layout_approx` for the missing-coordinates
fallback, `export::svg` for Export SVG, and
`scene::default_fill`/`default_stroke` for kind colours.

**`hs-diagram` has no egui painter.** `tool-db`'s ERD goes
`model` → `layout_approx` → `export::svg`, and the interactive painter
lives inside `tool-graphics`. So the canvas's egui drawing, hit-testing,
pan, zoom and keyboard navigation are **new code in `tool-etl`**, not
inherited. `hs-diagram` is not on `.claude/rules/rust.md`'s list of
crates a tool may depend on; `tool-db` and `tool-graphics` already depend
on it, and ADR-035 records the dependency and amends that list.

### 8.3 Reading files tolerantly

The formats change between Hop versions and a project may be older than
the reader:

- an unknown element or attribute is ignored, never an error;
- an unknown `<type>` is kept, named and drawn with the generic picture;
- a missing `<GUI>`, `<xloc>` or `<yloc>` falls back to
  `layout_approx` for the whole graph;
- a hop naming a transform that does not exist is dropped and counted,
  and the count is shown in the file's facts;
- a malformed file reports the parse error with the file's path and the
  line, and the tab stays usable.

### 8.4 Bench

**None for phase 1.** The fixtures are Apache Hop's own sample
`.hpl`/`.hwf` files, vendored under
`crates/tools/tool-etl/tests/fixtures/` with their ASF licence headers
intact and their provenance recorded in a `README.md` beside them (Hop is
Apache-2.0). This is the cheapest test story in the project: no
container, no server, no credential.

Phase 2 adds `apache/hop` (a JVM on Java 21, memory-capped) to a new
`etl` profile of the bench kit, started on demand like `heavy`, with
`HS_HOP` naming it for the live test; `bench/README.md` gains its row.

### 8.5 Tests

Unit: each sample file parses; a `.hpl` and a `.hwf` into `FlowGraph`
(nodes, positions, edges); the `.hwf` edge-style table of §4.3.2 for
every flag combination, including an unexpected one; a disabled hop and
an error-handling hop; a file with no coordinates falls back to
`layout_approx`; an unknown transform type survives; a hop to a missing
transform is dropped and counted; `project-config.json` with
`${PROJECT_HOME}` in `metadataBaseFolder`; the metadata kinds and the
variables; the Open in Database match (host case, `localhost` equal to
`127.0.0.1`, no match, several matches).

Secrets: a fixture whose `rdbms` connection holds a real
`Encrypted <hex>` value — the test asserts that the string never appears
in the parsed model that reaches the UI, in the AI context, in an error
message or in a log line, and **greps the data folder** afterwards
(`.claude/rules/security.md`); the whole-word mask over a list including
`SMTP_PASS`, `dbPassword` and `AUTH_LOCKOUT_THRESHOLD`.

Widgets: `egui_kittest` snapshots of the canvas at two pane widths, the
empty state, the too-many-nodes state and the parse-error state.

Phase 2 adds a live test gated on `HS_HOP`.

## 9. Keyboard

| Command | Default | Palette |
|---|---|---|
| `etl.refresh` | `F5` (`run.alt`) | ETL: Refresh |
| `etl.search` | `Ctrl+Alt+H` | ETL: Search… |
| `etl.browse` | `Ctrl+Alt+Shift+H` | ETL: Show or Hide Browse |
| `etl.editor` | — | ETL: Open in Editor |
| `etl.database` | — | ETL: Open in Database |
| `etl.export` | — | ETL: Export Canvas as SVG… |
| `etl.fit` | `Home` (in the canvas) | ETL: Fit Canvas |
| `etl.add` | — | ETL: Add Hop Project… |
| `etl.ai` | — | ETL: AI Assistant |

`Ctrl+Alt+H` and `Ctrl+Alt+Shift+H` are free: the bindings in use are
`Ctrl+Alt+{B,D,E,F,G,J,K,L,M,N,P,R,S,T,U}` and
`Ctrl+Alt+Shift+{B,D,F,J,K,M,R}`, and neither `Ctrl+Alt+H` nor `Key::H`
appears anywhere in `crates/`. `Home` is handled **only while the canvas
has focus**: it is no global binding, and the two existing `Key::Home`
handlers are tool-local (`hs-term`'s keys and the Logs list), so it does
not collide. Nothing on Ctrl+C/V/X, Ctrl+Shift+V or Shift+Insert.

## 10. Decisions (owner, 2026-11-26)

The activity was proposed as "ETL" with Apache Airflow and Talend first.
What the research and the owner's answers settled:

1. **Talend is dropped.** Talend Open Studio was retired on 31 January
   2024 and never had a server API; what remains is Talend
   Administration Center or Qlik Talend Cloud, both licensed, neither
   with a container to test against, and the Studio design canvas is not
   reachable from any API.
2. **Pentaho is dropped**, for the same reason: its community edition has
   been withdrawn, leaving no testable path. `hop-import` covers a PDI
   shop partially (§4.10).
3. **Airbyte is dropped.** Its public API returns only aggregate job
   figures — per-stream statistics, attempt logs and stream
   configuration need the internal Config API — and its OSS deployment
   left Docker Compose for a `kind` cluster, which the bench is not
   built for.
4. **NiFi is dropped**, although its API is the best of the five: node
   positions, connection bends and live per-component statistics all come
   back from it and sensitive properties are masked. It was dropped for
   scope; its model (continuous flow, no runs) also shares least with
   anything else.
5. **Airflow is dropped** from this activity. Its DAG → runs → log →
   trigger → cancel shape maps almost one-to-one onto
   `tool-automation`'s `Driver` trait, so if it is wanted it belongs
   there, not here.
6. **Apache Hop alone** (owner: "ditch nifi, only hop remain"). It is
   Apache-2.0, actively released (2.20.0 on 2 November 2026), has an
   official container, and — uniquely among the five — an open design
   format carrying each node's own coordinates, so its canvas can be
   drawn faithfully.
7. **An activity, not a viewer** (owner, 2026-11-26). A `.hpl` viewer
   could have lived in the Editor; the project model — `project-config.json`
   and its variables, the metadata, the run configurations, the unit
   tests, and the server in phase 2 — is project- and connection-shaped
   and does not belong there.
8. **Files only in phase 1** (owner). Hop Server is phase 2 (§4.9). This
   keeps phase 1 off Hop's weakest surface, its older web services.
9. **Both pipelines and workflows** in phase 1 (owner); the formats
   differ only in where the coordinates sit and in the hop flags
   (§4.3.1, §4.3.2).
10. **Metadata is listed, with secrets masked** (owner) — §4.5, §6.
11. **Phase R, the rail after Analytics** (owner).
12. **No provider trait** (ADR-035): one product, so no abstraction.

## What is implemented (2026-11-26)

Nothing yet: this document is the specification, written before the code
at the owner's request. The work is FEAT-0138 to FEAT-0142 in `TASKS.md`.
