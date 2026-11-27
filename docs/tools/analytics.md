# Tool spec — Analytics

**Status:** draft, decisions recorded (§10) · **Last updated:** 2026-11-26 · **Phase:** Q
(`docs/10-depth-programme.md`) · **Decisions:** ADR-034

## 1. Purpose

One place to read a team's business-intelligence content without leaving
HyperStack: browse what is saved on a BI server, run a question and read
its result as a table or a chart, open a dashboard with its filters, and
try a query against the server's databases. **Metabase first**, through
its REST API, drawn natively so a Metabase tab looks and reads like
Metabase's own web interface. **Tableau second** (§4.8), where the API
gives pictures and data rather than the interactive chart. **SAS Viya
third** (§4.9), the same way: folders and reports as pictures, with the
data tables behind them. **Power BI Report Server** (§4.10), **Apache
Superset** (§4.11, drawn natively like Metabase) and **Grafana** (§4.12,
dashboards here and log queries in Logs) follow.

It is **not** a BI authoring tool: it does not build or edit questions,
dashboards, models or permissions, does not administer the server, and
does not replace the product's web interface for what it cannot draw -
every item has **Open in Browser** for that. It reads data the signed-in
account may read, through the BI server, so the server's row-level
rules and sandboxes apply unchanged.

## 2. User stories

- As a developer, I want to run the team's "orders per day" question and
  see its chart beside my code, so that I can check a change against
  production numbers without opening another window.
- As a support engineer, I want a dashboard with its filters set to one
  customer, so that I can answer a ticket from the same app I read the
  logs in.
- As a data engineer, I want a saved question's SQL in the Database
  editor, so that I can rework it against the warehouse directly.
- As a lead, I want a Tableau workbook's views as pictures with their
  data, and its extracts' refresh state, so that I can see whether last
  night's refresh failed (phase 2).
- As anyone, I want **Open in Browser** on every item, so that what this
  tab does not draw is one click away.

## 3. Layout

### 3.1 Dashboard (the permanent home tab)

Through `hs_ui::dashboard`, like the other activities: the band; the
summaries *Servers*, *Dashboards*, *Questions*, *Open Tabs* (figures
from the servers open in tabs); **Start Here** with **Add BI Server**;
**Saved Servers** (a grid, each with its product's mark); **Recent** (the
items opened in this workspace, with Clear History and Remove behind a
confirmation, CR-0067); **Open Here**.

### 3.2 Sidebar

The saved servers of every kind, one list, each row its product's mark,
name and address; the grip to reorder (`hs_ui::reorder`); right-click
Open, Edit, Delete (confirmed).

### 3.3 A tab per server

```
┌ header band: [Metabase mark] Sales Metabase · https://bi.example.com · v0.52.4 · signed in as ada@example.com ┐
│ [⇤ Browse] [⟳ Refresh] [🔍 Search] [↗ Open in Browser]                                   [✦ AI] │
├──────────────────────┬────────────────────────────────────────────────────────────────────────┤
│ [Filter…          ]  │ 📊 Orders per Day                                    [⟳ Run] [⤓ CSV]  │
│ ▾ 📁 Our analytics   │ Saved question · Sample Database · 1,000 rows · 0.4 s                  │
│   ▾ 📁 Sales         │ [ Chart | Table | SQL ]                                                │
│      📊 Orders/day   │ ┌───────────────────────────────────────────────────────────────────┐ │
│      📈 Revenue      │ │  ▇                                                                 │ │
│      🖥 Sales board  │ │  ▇ ▇     ▇                                                        │ │
│   📁 Marketing       │ │  ▇ ▇ ▇ ▇ ▇ ▇                                                      │ │
│ ▸ 📁 ada's personal  │ └───────────────────────────────────────────────────────────────────┘ │
│ ─────────────────    │ Created At: Day ▾   Category [Gizmo ▾]                                │
│ 🗄 Databases         │                                                                        │
└──────────────────────┴────────────────────────────────────────────────────────────────────────┘
```

- **Header band** (`hs_core::tool::ToolHeader`): the product's mark and
  colour, the connection's name, URL, the server's version, and *signed
  in as* the account (an API key shows the key's name). A version
  outside §8.3's range adds a warning chip.
- **Toolbar** (main size): the **Browse** toggle first (CR-0073), then
  **Refresh**, **Search**, **Open in Browser** (the selected item, or
  the server's home); the AI assistant hard right through
  `toolbar::hard_right`. The item's own actions (Run, CSV, Open in
  Database) are in the item's sub-toolbar.
- **Left browse column** — resizable, hideable, keyboard-navigable, never
  narrower than its widest section name: a filter field, then the
  **collection tree** (`hs_ui::tree`; folders first, then items, each by
  name; "Our analytics" at the top, the account's personal collection,
  then other users' personal collections where the account is an
  admin), each item with its kind's coloured picture (question, model,
  metric, dashboard; Material pictures when on), then a **Databases**
  section listing the databases the account can query (§4.6).
- **Right: the item page** — a structured header (the item's picture and
  name; its kind, database, row count and run time; the sub-toolbar),
  then the item's view (§4.3–§4.6). With nothing selected it says so
  ("Pick a question or a dashboard to see it here.").
- `hs_ui::env_banner` and the 2 px frame when the profile's environment
  is protected; the tab is then read-only (§6).

### 3.4 Settings → Analytics

Kind buttons across the top (Metabase; Tableau when phase 2 lands), the
saved servers listed left and the form right, like Settings → Tracker
(§5); Test Connection answers with the product, version and account.
Below the list, the limits of §7 (rows read per run, the run timeout,
how many dashboard cards run at once).

### 3.5 Every screen keeps the layout standard

`.claude/rules/ui.md`'s *Layout and controls standard* applies to each
screen here: field heights and middle lines, `PANE_MARGIN`, `PANE_GAP`,
`ROW_GAP`, `DataTable` for every read-only table, the empty / loading /
error states through the shared helpers, Title Case labels and sentence-
case hints from Fluent, and the *Checking a screen* list before a screen
is reported done.

## 4. The sections

### 4.1 The provider model

One `tool-analytics` crate with a `BiProvider` trait (ADR-034), shaped
like `IdmProvider`: the screens are shared, each product is a module,
and what a product cannot do is said through `Caps` (a section drawn
disabled with the reason, never hidden). The trait's questions:

| Call | Metabase | Tableau (phase 2) |
|---|---|---|
| `info()` | `/api/session/properties` version, `/api/user/current` | `/serverinfo`, the signed-in user |
| `tree(parent)` | collections and their items | projects, workbooks, views |
| `item(id)` | a card or a dashboard with its settings | a workbook or a view |
| `run(item, params)` | a card's query: columns and rows | a view's data as CSV |
| `picture(item, params)` | — | a view's PNG |
| `dashboard(id)` | the grid, cards and filters | — (a workbook's views) |
| `export(item, format)` | CSV, JSON, XLSX | CSV, PDF, PNG |
| `databases()`, `native(db, sql)` | the databases and an ad-hoc SQL run | — |
| `extract_tasks()`, `jobs()` | — | read-only (§6) |

### 4.2 Browsing and search

- The tree loads a level at a time (`/api/collection/:id/items`, `root`
  for the top; each collection is read once, when it is opened).
  Collections come first, then the items, each by name, each wearing its
  kind's picture (Material when on, a glyph when off). Archived items are
  left out; a collection the account cannot read is not listed by the
  server. A person's own **personal collection** is added to the root
  (it is not among the root's items); an **API key's user has none**.
- A **filter field** above the tree narrows what is loaded by name and
  opens the way to each match; a collection not yet opened is not
  searched (the empty state says so - use Search for the whole server).
- **Search** (toolbar, `Ctrl+Alt+B`): a field over `/api/search?q=…`,
  Enter or the Search button runs it; results in the left column, 50 a
  page with **More**, each with its kind's picture and its collection;
  **kind filters** (Questions, Models, Metrics, Dashboards, Collections)
  combine, none chosen meaning all. An empty result says so; an older
  search's late answer is dropped.
- **Picking an item** (tree or result) shows its page on the right: name,
  kind, Open in Browser, Copy Link and its facts (id, collection, editor,
  last edited, description, where the server gives them). The question,
  dashboard and collection pages themselves come with FEAT-0128 and
  FEAT-0129.
- **Recent** on the home dashboard records what was opened here, not
  Metabase's own recent views.

### 4.3 Questions, models and metrics

- **Run** (`F5`): `POST /api/card/:id/query` with the question's
  parameters, as a Job with cancel (§7); the result is the card's
  columns (`display_name`, `base_type`, `semantic_type`) and rows.
- The page has a segmented strip **Chart | Table | SQL**, opening on the
  card's own display:
  - **Table** — `hs_ui::table::DataTable` with the column titles
    Metabase shows, numbers right-aligned and formatted by their
    `base_type` (integers, decimals, currency from the column settings
    where given, dates in the column's unit), sort and filter local to
    the rows read, Copy Row.
  - **Chart** — drawn natively from the card's `display` and
    `visualization_settings` (§4.4); a display this tab does not draw
    shows the table with a hint and **Open in Browser**.
  - **SQL** — a native question's query in the read-only editor engine
    (`hs-editor`, SQL colours) with **Open in Database**: a new Database
    query tab with the SQL, on the saved connection that matches the
    question's database (owner, 2026-11-20). The match: Metabase's
    database `engine`, `details.host`, `details.port` and
    `details.dbname` (from `/api/database/:id`) against each saved `db`
    connection's engine, host, port and database, host compared without
    case and `localhost` equal to `127.0.0.1`. One match opens on it; no
    match or several (or details the account may not read, which
    Metabase hides from non-admins) opens the tab with the connection
    picker on the candidates and a hint saying why. The SQL is passed as
    an `OpenRequest` carrying the connection id, never a password. A
    question built in Metabase's
    query builder shows the SQL Metabase compiles it to
    (`POST /api/dataset/native`), marked as generated.
- **Parameters**: a native question's template tags and a question's
  filters become fields above the result — a `form::pick_field` filled
  from the field's values where Metabase lists them
  (`/api/card/:id/params/:param-key/values`), a `datefield::date_field`
  for dates, plain text otherwise. Changing one and pressing **Run**
  reruns.
- **Export**: **CSV** (and JSON, XLSX in the menu) through
  `POST /api/card/:id/query/:format`, saved with the shared download
  dialog; Metabase's export limit applies, not the run limit.
- A **model** and a **metric** run and show like a question.
- **As built (FEAT-0128)**: opening a question runs it at once, as Metabase
  does; **Run** (`Ctrl+Enter`) reruns and **Cancel** stops the job (the
  request is dropped; a late answer is ignored). A result keeps at most
  *Rows per Run* rows (Settings -> Analytics) and says when it was cut.
  The SQL view shows a native question's own SQL, or the SQL Metabase
  compiled a query-builder question to (`data.native_form.query` of the
  result, marked as generated); **Open in Database** is drawn disabled
  until FEAT-0130, which owns the connection match. Parameters are text,
  number and date fields prefilled with the card's defaults; filling a
  field from the server's value list (`pick_field`) is not built yet.
  Numbers show without a thousands separator, so a column still sorts as
  numbers; decimals show up to two places.

### 4.4 Charts

Drawn with `egui_plot` (ADR-034) in the theme's colours; the series
colours follow the card's `series_settings` colour where set, else the
shared palette of eight. Hover shows the point's values as Metabase's
tooltip does; the legend is clickable to hide a series.

| Metabase `display` | Drawn as | Notes |
|---|---|---|
| `table` | `DataTable` | |
| `scalar` | the number, large, with its title | formatted as the column says |
| `smartscalar` | the number and its change against the previous period, coloured up / down | |
| `progress`, `gauge` | a bar / an arc against the goal or segments | hand-drawn |
| `line`, `area` | `egui_plot::Line` (filled for area), time or category on X | stacking and `graph.dimensions` / `graph.metrics` honoured |
| `bar`, `row` | `egui_plot::BarChart`, vertical or horizontal | stacked and grouped |
| `combo` | lines and bars on one plot | |
| `scatter` | `egui_plot::Points` | bubble size when set |
| `pie` | hand-drawn sectors with the legend and percentages | |
| `funnel`, `waterfall` | hand-drawn bars | phase 1b |
| `map`, `pivot`, `sankey`, `object` | the table, with "This chart is drawn in Metabase" and Open in Browser | not drawn |

Charts are laid out once per result and redrawn from the cached points,
never recomputed per frame (`.claude/rules/rust.md`).

**As built (FEAT-0128)**: `chart::plan` turns a result and the card's
`graph.dimensions`, `graph.metrics`, `stackable.stack_type`, `series_settings`
and `scalar.field` / `progress.goal` / `gauge.segments` into a plan, tested
per kind from recorded results; a second dimension with one metric splits
into a series per value; a row chart lists the first category at the top;
a gauge is drawn as a progress bar against its last segment, or as a number
where it has no goal. Series colours follow the shared palette of eight, not
the card's own colours yet. A combo draws lines and bars on **one** axis (a
count beside a sum shows as a flat line: Metabase's second axis is not
built). Funnel and waterfall (phase 1b), map, pivot, sankey and object are
the table with the hint and Open in Browser.

### 4.5 Dashboards

- `GET /api/dashboard/:id` gives the **grid**: each dashcard's `row`,
  `col`, `size_x`, `size_y` on Metabase's 24-column grid, its card, its
  `visualization_settings` overrides and `parameter_mappings`, plus the
  dashboard's **tabs** and **parameters** (filters).
- The page draws the same grid at the pane's width (a column is the
  pane's width / 24; a row a fixed height), each card in a framed tile
  with its title, the chart or table of §4.4, and its own loading /
  error state. Text, heading and link cards draw their Markdown with
  the shared Markdown renderer.
- **Filters** across the top as Metabase shows them, each a
  `pick_field` from `/api/dashboard/:id/params/:param-key/values` or a
  date field; **Apply** reruns every card with the values through
  `POST /api/dashboard/:id/dashcard/:dashcard-id/card/:card-id/query`.
  Defaults come from the dashboard.
- Cards run in parallel, at most four at once by default (Settings),
  each a Job, cancelled when the tab leaves the dashboard; a card's
  title opens its question on the item page.
- Dashboard **tabs** are a segmented strip; only the shown tab's cards
  run.
- Auto-refresh is offered as Metabase does (1, 5, 10, 15, 30, 60
  minutes), off by default, and only while the tab is on screen.

### 4.6 Databases and SQL

- The **Databases** section lists `/api/database` (the ones the account
  may query natively say so); a database opens its tables and fields
  from `/api/database/:id/metadata`.
- **New SQL** on a database opens a query page: the editor engine with
  completion from that metadata, **Run** (`Ctrl+Enter`) through
  `POST /api/dataset` (`type: native`), the result as §4.3's Table and a
  Chart picker (line, bar, pie, scalar) for a quick look. It is
  Metabase's SQL editor, not a saved question: nothing is saved on the
  server (§6.3).
- The SQL page refuses a database the account has no native access to,
  with the server's words.

### 4.7 What every page has

Open in Browser for the item; Copy Link; the item's facts (collection
path, creator, last edited, database, Metabase id) in a
`form::detail_table` under a disclosure; the AI assistant's context is
the item's name, kind, SQL and the first 50 rows (values never sent for
a column whose name the secret-word rule masks).

### 4.8 Tableau (phase 2, specified, not built now)

What Tableau's REST API allows, and so what a Tableau tab is:

- **Tableau Server** (owner, 2026-11-20), not Cloud.
- **Sign-in**: `POST /api/{version}/auth/signin` with a username and
  password (a Server using Active Directory or local authentication) or a
  **Personal Access Token** (name + secret; the way in when the Server
  signs people in through SAML or OpenID), into one **site**; the token
  returned lives in memory only and is sent back as `X-Tableau-Auth`;
  `auth/signout` when the tab closes. A PAT unused for the Server's idle
  limit (15 days by default) expires: the tab says so and asks for a new
  one.
- **Browse**: projects → workbooks → views, data sources, with owners and
  last-updated; search.
- **A view**: its **picture** (`GET …/views/:id/image`, PNG, at the
  pane's width, filters as `vf_<field>=` parameters entered as fields
  above it), its **data** (`…/views/:id/data`, CSV) in a `DataTable`,
  exports (PDF, PNG, CSV). The interactive viz is rendered by Tableau's
  servers and is **not** drawn here: **Open Interactive** opens the view
  in the Browser activity, where the user signs in to Tableau as usual.
- **Extracts and jobs**: a data source's or workbook's extract refresh
  tasks with their schedule and last result, and the recent jobs with
  failures and their messages - read-only (owner, 2026-11-20): there is
  no Refresh Now; **Open in Browser** leads to the task on the server.
- **Version gating** by the REST API version `GET /api/{v}/serverinfo`
  reports (`productVersion`, `restApiVersion`): the provider asks with a
  low version first, then talks the server's own, so an older Server
  answers in the version it knows; calls a version lacks are disabled
  with the reason.
- Testing has no container: Tableau Server is licensed. Because the
  provider only reads, the live test may run against the owner's Server
  when `HS_TABLEAU` names it; everything else runs against a mock written
  from the REST API reference.

### 4.9 SAS Viya (phase 3, specified, not built now)

Added at the owner's request, 2026-11-26 ("add SAS BI"), and narrowed by
the answer: **SAS Viya**, whose services are REST. **SAS 9.4 BI** (Web
Report Studio, the BI Server and its metadata server) has no equivalent
and is not covered (§10.7). The endpoints below are from SAS's REST API
reference and are to be confirmed against a real Viya on the first
build; like Tableau, a Viya tab is pictures and data, not the
interactive report.

- **Sign-in**: SAS Logon, `POST /SASLogon/oauth/token` with
  `grant_type=password` (username and password) for a registered OAuth
  **client** (id and secret; the site's own, or `sas.cli` with an empty
  secret where the site allows it), or a pasted **access token** where
  the site signs people in through SSO. The token lives in memory only,
  is sent as `Authorization: Bearer`, and is renewed with the refresh
  token while the tab is open.
- **Browse**: folders and their members (`/folders/folders/@myFolder`,
  `…/{id}/members`: folders, reports, data flows, files, each with its
  type and `uri`), paged with `start` and `limit`; search over
  `/reports/reports?filter=contains(name,'…')`.
- **A report**: its **picture** from the Report Images service
  (`POST /reportImages/jobs` for a report's section, polled until the
  job completes, then the image fetched), shown at the pane's width;
  its **details** (`/reports/reports/{id}`: owner, modified, sections).
  The interactive report is **not** drawn here: **Open Interactive**
  opens it in SAS Visual Analytics (`/SASVisualAnalyticsViewer/…`) in
  the Browser activity, where the user signs in as usual.
- **Data**: the caslibs and their tables (`/casManagement/servers/…/
  caslibs/…/tables`) with their columns, and a **row preview**
  (`/casRowSets/…/rows?limit=…`, capped by Rows per Run) in a
  `DataTable`.
- **Jobs**: recent jobs (`/jobExecution/jobs`) with their state and
  messages, read-only.
- **Version gating**: Viya has no one version call; the provider reads
  what the server states about itself and probes each service once,
  and a service that answers 404 is drawn disabled with that reason.
- Testing has no container: Viya is licensed. The bench has a **mock**
  written from the reference (`bench/mock/sasviya`), as for Tableau,
  and a read-only live test may run against the owner's Viya when
  `HS_SASVIYA` names it.

### 4.10 Power BI Report Server (phase 3, specified, not built now)

Added at the owner's request, 2026-11-26 ("add Power BI"), narrowed by
the answer to **Power BI Report Server** (the on-premises product, its
REST API v2.0 at `/reports/api/v2.0`). **Power BI Service** (the cloud,
Microsoft Entra sign-in) is not covered now (§10.8). As with Tableau and
SAS, the interactive report belongs to the product: a Report Server tab is
the catalogue, its data sources and KPIs, with **Open Interactive**.

- **Sign-in**: Report Server accepts Windows authentication (NTLM or
  Kerberos) or, over HTTPS, Basic. The profile offers **Username and
  Password** (Basic, or NTLM with a domain `DOMAIN\user`) and, on
  Windows only, **the current Windows user** (SSPI, nothing stored). On a
  plain-HTTP server the form warns that a password travels in the clear
  and asks to confirm.
- **Browse**: folders, Power BI reports (`PowerBIReports`), paginated
  reports (`Reports`), mobile reports, KPIs, data sources and datasets
  (`CatalogItems`, `Folders({id})/CatalogItems`), with owner, modified
  date and size; search by name; paged with `$top` / `$skip`, filtered
  with `$filter` (OData).
- **An item**: its details; a **paginated report's picture** and data
  through `Reports({id})/Export/IMAGE` and `.../CSV` where the server's
  rendering extensions allow it; a **Power BI report (.pbix) has no
  server-side picture or data API**, so it shows its details, data
  sources and refresh plan, and **Open Interactive** opens it in the
  Browser activity (`/powerbi/<path>`).
- **KPIs**: value, goal, status and trend (`Kpis`), read-only, drawn as
  the dashboard's tiles.
- **Data sources and refresh**: each data source's type, connection
  string with the secret masked, and the cache refresh plans and their
  last result (`CacheRefreshPlans`, read-only; no Refresh Now).
- **Version gating**: `GET /reports/api/v2.0/System` names the product and
  version; v2.0 calls a build lacks are drawn disabled with the reason.
- Testing has no container: Report Server is a licensed Windows Server
  product. The bench has a **mock** (`bench/mock/powerbirs`), and a
  read-only live test may run against the owner's server when
  `HS_POWERBIRS` names it.

### 4.11 Apache Superset (phase 3, specified, not built now)

**Apache Superset** has a public container, so the bench runs the real
product, and it is the second provider whose charts can be drawn natively,
as Metabase's are (REST API v1, `/api/v1`).

- **Sign-in**: `POST /api/v1/security/login` with a username and password
  (`provider: db`, or `ldap` where the server uses it), returning an
  access token (sent as `Authorization: Bearer`) and a refresh token
  (renewed in memory while the tab is open). Superset has no long-lived
  API key; a **pasted access token** is accepted for a server that signs
  in through OAuth or SSO. Reads need no CSRF token. Who is signed in is
  the user name typed: `/api/v1/me/` answered *Not authorized* to an admin's
  bearer token on the bench's 4.1.1, so the tab does not depend on it.
- **Browse**: dashboards (`/dashboard/`), charts (`/chart/`), datasets
  (`/dataset/`) and databases (`/database/`), as the flat lists Superset
  keeps, with owners, changed-by and date, **tags** and **certified**
  marks; search and filter by owner through its `q` (Rison) filters.
  There is no folder tree: the tree shows the four lists as sections and
  a dashboard's charts under it.
- **A chart**: `GET /chart/{id}/data/` runs the chart's saved query
  context and returns its rows with the chart's `viz_type` and params,
  which pick the drawing. A chart made over the API without a query
  context answers *Chart has no query context saved*; the tab then builds
  the context from the chart's params and posts it to `/chart/data` (a
  read), and says so when it cannot. The drawing: line, area, bar, scatter, pie, big number,
  table, pivot (as a table), gauge, funnel and the rest as the table with
  Open in Browser, through the same chart code as Metabase's (§4.4).
- **A dashboard**: `/dashboard/{id}` gives `position_json` (the grid:
  rows and columns of chart, markdown, header and tabs) and
  `json_metadata` (native filters and their defaults); the tiles are laid
  out on Superset's 12-column grid, each chart run as above, four at a
  time (§4.5), the native filters as pick and date fields with Apply.
- **SQL**: SQL Lab through `POST /sqllab/execute/` against a database the
  account may query, capped by Rows per Run, **read-only** (§6): a
  statement that is not a `SELECT`, `WITH` or `SHOW`/`DESCRIBE` is
  refused before it is sent. Open in Database picks the matching saved
  connection (§4.3).
- **Version gating**: Superset names its version in the page footer and
  in its bootstrap data only; the provider reads what it can and
  otherwise shows the version as unknown and probes each endpoint once.
  Superset 3.x and 4.x are the targets; the bench runs 4.1.

### 4.12 Grafana (phase 3, specified, not built now)

**Grafana** is dashboards over *other* systems' data (Prometheus, Loki,
Elasticsearch, SQL, ...), so it appears in two activities from **one
connection kind** (`grafana`): here as dashboards, and in **Logs** as a
source of log lines (`docs/tools/logs.md` §21). Both use one client,
`hs_core::grafana` (health, search, dashboards, data sources, the query
API), because tools never depend on each other (ADR-034).

- **Sign-in**: a **service-account token** (`glsa_...`, sent as
  `Authorization: Bearer`; the way a server is meant to be read) or a
  **username and password** (HTTP Basic); an optional **Organisation id**
  (`X-Grafana-Org-Id`). `GET /api/health` is public and gives the version
  and database state; `GET /api/user` names who signed in.
- **Browse**: folders (`/api/folders`, nested folders where the server has
  them) and dashboards (`/api/search?type=dash-db`), each with its tags and
  starred mark; search by name and tag; the **data sources**
  (`/api/datasources`) as a section, with their type.
- **A dashboard**: `/api/dashboards/uid/{uid}` is the JSON model: panels
  with their `gridPos` (24 columns), `type`, `targets` and field options,
  rows that collapse, and **template variables**, drawn as pick fields
  filled from their queries. Each panel runs its targets through the
  unified query API (`POST /api/ds/query`, with `from`/`to` from the time
  range and the `intervalMs` and `maxDataPoints` the panel width gives)
  and draws the returned data frames natively: time series, bar chart,
  stat, gauge, bar gauge, table, pie and logs as a list; panels of any
  other type (heatmap, geomap, node graph, plugins) show their title with
  **Open in Browser**. A time-range picker (Last 5 minutes ... Last 30
  days, absolute) and an auto-refresh interval sit in the toolbar.
- **Pictures**: where the server has the Image Renderer, a panel's PNG
  (`/render/d-solo/{uid}/...`) is offered as **Open Picture** beside the
  native drawing; it is never required.
- **Explore**: **Query** (read-only) takes a data source, a query in that
  source's language (PromQL, LogQL, SQL, ...) and a range and shows the data
  frames as a table or a chart; a data source that can write is never
  asked to.
- **Version gating**: `/api/health` gives `version` (Grafana 9 to 11 are
  the targets; the bench runs 11.3); the provider uses `/api/ds/query` on
  9+ and says so on an older server.

### 4.13 Later (not specified further now)

Power BI Service (REST and the embed token, Microsoft Entra sign-in),
Looker, Redash; Metabase alerts and subscriptions (read-only), and a
question's revision history.

## 5. Connections and credentials

### 5.1 The profile

Connection kind `metabase` (later `tableau`), listed by
`hs_core::connections::is_bi_kind`, the `[bi]` profile table:

| Field | Metabase | Notes |
|---|---|---|
| URL | `https://bi.example.com` | the site root; a path prefix kept |
| Credential | **API Key** (default) · Username and Password | `hs_core::service_auth`, the secret as `secret:<ulid>` |
| Verify TLS, CA File | on, empty | `hs_core::tls` |
| Rows per Run | 2,000 | Metabase's own cap for a normal run; lower only |
| Timeout | 60 s | per request |

- **API key** (Metabase 49 and later): the `x-api-key` header; works on a
  server whose people sign in through SSO, where a password cannot. The
  form's hint says where keys are made (Admin → Settings → Authentication
  → API Keys) and that a key carries its group's permissions.
- **Username and password**: `POST /api/session` gives a session id sent
  as `X-Metabase-Session`; it is kept in memory only, renewed once on a
  401, and `DELETE /api/session` is sent when the tab closes. Refused
  with SSO-only sign-in: the error says to use an API key.
- **Use Saved Credential** (FEAT-0125) and Test Connection work as on
  every connection form.

### 5.2 Tableau (phase 2)

Kind `tableau` (Tableau Server): Server URL, **Site** (content URL; empty
for the default site), Credential **Username and Password** or
**Personal Access Token** (name + secret), the API version (detected;
overridable), Verify TLS and CA File (a company CA is usual on an
internal Server).

### 5.3 SAS Viya (phase 3)

Kind `sasviya`: Server URL (the ingress root), Credential **Username and
Password** with the OAuth **client id** and **client secret**, or an
**Access Token**; Verify TLS and CA File. Secrets are `secret:<ulid>` in
the vault.

### 5.4 Power BI Report Server (phase 3)

Kind `powerbi`: Server URL (the web portal root, e.g.
`https://reports.example.com`), Credential **Username and Password**
(Basic, or `DOMAIN\user` for NTLM) or, on Windows, **Current Windows
user**; Verify TLS and CA File.

### 5.5 Apache Superset (phase 3)

Kind `superset`: Server URL, Credential **Username and Password** with
the **provider** (`db` or `ldap`), or an **Access Token**; Verify TLS and
CA File.

### 5.6 Grafana (phase 3)

Kind `grafana`: Server URL (with its sub-path where Grafana is served
from one), Credential **Service Account Token** or **Username and
Password**, an optional **Organisation id**; Verify TLS and CA File. The
same profile serves the Analytics tab and Logs' Grafana sources (§4.12).

## 6. Guards

- **Read-only, both providers** (owner, 2026-11-20). Nothing the
  Metabase or Tableau provider sends changes the server: running a question, a dashboard card or ad-hoc SQL is a
  read on the server's side. An ad-hoc SQL page states that statements
  run as the Metabase account against the warehouse, so a database
  connected with write rights could still be written by an `UPDATE`; on
  a **protected environment** the SQL page is disabled ("PROD is
  read-only. Allow Writes for this tab…"), as other activities' query
  editors are.
- **Never offered:** editing, archiving or deleting items; permissions;
  user and group administration; alerts and subscriptions; caching and
  sync settings; Tableau's extract Refresh Now and any other job start.
  With no write there is no audit log here.
- **Secrets:** the API key, password and session never reach logs or
  history; result rows are not written to disk except by an explicit
  Export; the AI context masks secret-named columns.

## 7. Work and progress

- Every request runs on the tokio runtime; results reach the UI through
  bounded channels drained at frame start; nothing blocks the UI thread.
- A question run, an export and each dashboard card are **Jobs** with
  progress and **cancel** — cancel drops the HTTP request (Metabase
  stops the query when the client goes away on most drivers).
- Bounds: rows per run (Settings, Metabase's cap at most); dashboard
  cards at most four at once; the tree a level at a time; search results
  50 per page with More; a result kept per item page while it is open,
  dropped when the tab closes.
- Dashboards and charts refresh only on Run, Apply or the auto-refresh
  interval, and only while on screen.

## 8. Implementation notes

### 8.1 Crate and files

`crates/tools/tool-analytics/` (`AnalyticsFactory`, descriptor id
`analytics`, category Data, rail after Database): `provider.rs` (the
trait, `Caps`, `Item`, `Grid`, `Result`), `provider/metabase/{mod,
api, chart}.rs`, `ui/{mod, tab, dashboard, sidebar, tree, question,
board, sql, chart}.rs`, `profile.rs`. Depends on `hs-core`, `hs-ui`,
`hs-editor` (SQL view and editor), `egui_plot` (ADR-034); no other tool
crate. **Open in Database** goes through `OpenRequest::Text` with the
`sql` language.

### 8.2 Shared pieces reused

`hs_core::service_http` (the HTTP client, errors in words, TLS),
`hs_core::service_auth` (credentials, Use Saved Credential),
`hs_core::connections`, `hs_core::jobs`, `hs_ui::{dashboard, tree,
table::DataTable, form, toolbar, env_banner, ai_assistant}`, the shared
Markdown renderer for text cards, `hs_core::download` for exports.

### 8.3 Version gating

Metabase's API is documented as internal and changes between releases;
the provider reads `version.tag` from `/api/session/properties` (public)
and supports **v0.48 to the current release**: API keys from v0.49 (the
form says so on older servers), dashboard tabs from v0.47, the card
query paths as they are from v0.50 with the older forms where they
differ. An unknown newer version is tried and a failing call reported
with the version named.

### 8.4 Bench

A `metabase` service in a new `bi` profile of the bench kit
(`metabase/metabase`, port 3030, about 1.5 GB), seeded on first start
through its setup API: an admin, the bundled **Sample Database**, a
collection with a question per chart kind of §4.4, a native question
with a template tag, and a dashboard with two tabs and a filter; an API
key for the tests. Passwords in the gitignored `bench/.env`.

### 8.5 Tests

Unit: the profile round trip; a result's columns and rows into the table
model; each chart kind's points from a recorded result; the dashboard
grid's layout at two widths; parameter values into the request body;
version gating. A live test (`tests/metabase_live.rs`, when
`HS_METABASE` names the bench server) signs in both ways, walks the
tree, runs the questions, a dashboard with a filter and an ad-hoc SQL
query, and greps the data folder for the password and the key.

## 9. Keyboard

| Command | Default | Palette |
|---|---|---|
| `analytics.refresh` | `F5` (`run.alt`) | Analytics: Refresh |
| `analytics.run` | `Ctrl+Enter` (`run`) | Analytics: Run |
| `analytics.search` | `Ctrl+Alt+B` | Analytics: Search… |
| `analytics.browse` | `Ctrl+Alt+Shift+B` | Analytics: Show or Hide Browse |
| `analytics.browser` | — | Analytics: Open in Browser |
| `analytics.export` | — | Analytics: Export CSV… |
| `analytics.sql` | — | Analytics: New SQL Query… |
| `analytics.add` | — | Analytics: Add BI Server |
| `analytics.ai` | — | Analytics: AI Assistant |

Nothing on Ctrl+C/V/X, Ctrl+Shift+V or Shift+Insert.

## 10. Decisions (owner, 2026-11-20)

The questions this section held, answered:

1. **Tableau Server**, not Cloud. Phase 2 signs in with a username and
   password or a Personal Access Token to a Server site (§5.2); the
   Server version was not given, so the REST API version is read from
   `/serverinfo` and gated there (§4.8).
2. **Metabase: password and API key**, both offered on the form (§5.1).
3. **Read-only**, everywhere. Tableau's Refresh Now is dropped: the
   Tableau provider shows extract tasks and jobs but changes nothing
   (§4.8, §6).
4. **Open Interactive in the Browser activity** is accepted for
   Tableau's charts.
5. **Open in Database picks the matching saved connection** (§4.3).
6. **Rail after Database**, as proposed.
7. **SAS Viya** is the third provider (owner, 2026-11-26, "add SAS BI",
   then *SAS Viya*; spec, ledger item and a bench mock, built after the
   Tableau step). SAS 9.4 BI is not covered: its stack has no REST
   surface for reports.
8. **Power BI Report Server**, the on-premises product, is the fourth
   provider (owner, 2026-11-26: "add Power BI", then *Report Server*).
   Power BI Service (the cloud) is not covered now.
9. **Apache Superset** (the real product runs on the bench) and
   **Grafana** are the fifth and sixth (owner, 2026-11-26).
10. **Grafana is in both Analytics and Logs** from one connection kind
    and one client in `hs-core` (§4.12; `logs.md` §21): in Logs it is a
    source whose lines come from a Loki or Elasticsearch data source
    through Grafana's API (owner's choice, 2026-11-26).

## What is implemented (2026-11-27)

FEAT-0126 (v0.139.0): the crate, the provider trait, the Metabase
sign-in by password or API key, connections and Settings → Analytics,
the dashboard, the sidebar and a server's tab with its facts.
FEAT-0127: the collection tree, the filter and Search (§4.2), the item
page's facts, Open in Browser and Copy Link (§4.7), live-tested against
the bench Metabase (`tests/metabase_live.rs`).
FEAT-0128: a question, model or metric opens and runs, as a table, a chart
(`egui_plot`: line, area, bar, row, combo, scatter; hand-drawn pie, scalar,
smart scalar, progress and gauge) or its SQL, with parameters and an export
(§4.3, §4.4), live-tested against every seeded question. Dashboards,
databases and the other providers are FEAT-0129 onward in `TASKS.md`.
