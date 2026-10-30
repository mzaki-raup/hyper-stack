# The depth programme (Phase 2 / Phase 3 items, planned 2026-10-30)

The owner asked (2026-10-30) for the remaining Phase 2 and Phase 3 items
below to be planned by phase and implemented. This document is the plan
and the record: each phase is a branch, gated, exercised in the running
app, documented and released from `main` before the next begins -
exactly as the Git activity was (`docs/tools/git-activity.md`). The
tool specs stay the requirements; a phase that changes a design changes
its spec in the same commit, and `CHECKLIST.md` carries the state.

The items, as asked:

- SQL: SQL Server, inline editing, explain, CSV import, Excel export
- Redis: dashboard, slowlog, pub/sub, streams, big-key finder, bulk ops
- Logs: Docker / k8s / journalctl / Event Log sources, merge, statistics, alerts
- SSH: port-forward UI, broadcast, server monitor
- S3: metadata / tags / versions, sync, policy viewer
- Devtools: network tools, process viewer, snippets, `.env` editor
- SQL: ER diagram, DDL, schema / data compare, ClickHouse / DuckDB
- NoSQL: Cassandra / ScyllaDB
- Forge integration (GitHub / GitLab / Forgejo)
- A new **Automation** activity for builders and configuration
  management: Jenkins, TeamCity, GitHub Actions, GitLab CI, Forgejo
  Actions, Ansible (AWX / Automation Platform), Puppet - with its own
  credentials, connections, Settings page and dashboard
- (added 2026-10-30, same day) a new **Queue** activity: manage, view
  and add message-queue services such as RabbitMQ and Kafka, with
  Settings for their connections and credentials and a dashboard
- (added 2026-10-30, same day) **ELK**: Elasticsearch and Kibana as a
  logs source and a search activity
- (added 2026-10-30, same day) **Scratchpad folders**: folders and
  subfolders for notes, draggable in the sidebar (pinned notes excepted),
  drop into any folder, name clashes get the existing `name 001` suffix
- (added 2026-10-30, same day) a new activity for **tickets**: manage and
  view connections to Redmine, GitLab, Jira, Confluence and the like;
  display, edit and change the status of items. The owner asked for a
  name suggestion: it is called **Tracker** here, because it holds issue
  trackers *and* Confluence pages, which "Ticket" would not cover
  (alternatives: *Issues*, *Work Items*; renaming is one locale key)

## Order and rationale

Phases are ordered by value against risk: the ones that reuse clients
the app already has (Redis commands, the Logs command source, the SSH
exec channel, the object store, the Git hosts) come first and land
quickly; the ones that bring a new wire protocol and a heavy dependency
(SQL Server, ClickHouse, DuckDB, Cassandra) come last, each behind a
`cargo deny` check and a container-backed driver test, because a driver
that cannot be exercised here is a liability rather than a feature.

| Phase | Scope | Builds on | Release |
|---|---|---|---|
| **A — Redis depth** | Info **dashboard** cards (memory, clients, ops/s, hit rate, keyspace, replication, persistence, uptime) with 5-minute sparklines; **Slowlog** and **Client List** tables (kill guarded); **Pub/Sub** tab (subscribe to channels/patterns, publish, scrolling messages); **Streams**: XINFO groups/consumers, XRANGE paging, pending entries; **big-key finder** (SCAN + `MEMORY USAGE`, top-N by size with type and TTL, cancellable Job); **bulk ops**: delete / expire / rename-prefix by pattern with a **dry-run count** first and the protected-environment guard | `tool-redis::client` (`command`, `scan`, `info`), the runner, `hs_ui::dashboard`, Jobs | `feat:` minor |
| **B — SQL depth 1** | **Explain**: EXPLAIN / EXPLAIN ANALYZE per engine as a tree (Postgres JSON, MySQL JSON, SQLite EXPLAIN QUERY PLAN) beside the result; **inline editing**: edit a cell of a result that came from one table with a primary key, see the generated UPDATE / INSERT / DELETE, apply in a transaction (guards apply); **CSV import** into a table (column mapping, header row, delimiter, batch INSERTs as a Job with progress); **Excel export** (`.xlsx` through `rust_xlsxwriter`, pure Rust, MIT) beside the CSV / JSON exports | `tool-db::{grid, export, runner, driver::DbSession}`, `hs_core::download` | `feat:` minor |
| **C — SQL engines** | **SQL Server** through `tiberius` (rustls); **ClickHouse** through its HTTP interface (`reqwest`, `FORMAT JSONCompact`); **DuckDB** through the `duckdb` crate with the bundled feature, behind a Cargo feature `duckdb` (the bundled build adds minutes to CI; off by default until measured). Each engine: connection form fields, catalog introspection, the type → cell mapping, cancel, transaction probing, and a `drivers` test that runs when its `HS_*` variable names a server | `tool-db::driver`, `conn_test` | `feat:` minor per engine |
| **D — Logs depth** | **Sources**: `journalctl -f -o json` (local or over SSH) as a parser-aware source, **Windows Event Log** through `Get-WinEvent` streaming (local or over SSH to a Windows host), Docker and Kubernetes already arrive from the Container activity and get Start Here entries here; **merge**: one tab over several sources ordered by timestamp with a per-source colour; **statistics**: records per minute per level (the navigator sparkline exists) plus top messages and top fields in a panel; **alerts**: a highlight rule with *Alert* on posts a toast, a tab badge and an optional Problems entry, with a cooldown | `tool-logs::{source, engine, chart, shape}`, `hs_core::problems` | `feat:` minor |
| **E — SSH depth** | **Port-forward manager**: a Tunnels pane (not only the sidebar form) listing every forward with traffic, state, start/stop, edit, and saved forwards per profile that start with the connection; **broadcast input**: type into every open session of the same host (or a chosen set) from one tab, with a visible armed state; **server monitor**: a card strip over an SSH tab (uptime, load, memory, disk, top processes) refreshed every few seconds through the exec channel, off by default | `hs_ssh::tunnels`, `hs_ssh::SshClient::exec`, `hs_term` | `feat:` minor |
| **F — S3 depth** | **Object details** pane: metadata (content type, size, ETag, storage class, custom headers), **tags** (edit), **versions** (list, download, restore, delete a version); **sync** between a local folder and a prefix (both directions, dry-run diff first, then a Job with progress and cancel); **policy / CORS viewer** (bucket policy JSON and CORS rules, read-only) | `hs_core::objstore::{s3, store}`, Jobs | `feat:` minor |
| **G — Devtools depth** | **Network tools**: DNS lookup (A/AAAA/MX/TXT/CNAME through `hickory-resolver`), TCP ping and port check, HTTP ping (status, timing, headers), TLS certificate viewer (chain, SANs, expiry), whois (RDAP over HTTPS); **process / port viewer**: what listens on which port and which process owns it (`netstat -ano` + `tasklist` on Windows, `ss -ltnp` elsewhere), filter, copy, kill guarded; **snippets**: a per-workspace collection (code, SQL, shell) with tags and search, inserted into the editor, a terminal or a query tab; **`.env` editor**: parse a `.env` file, mask secrets, edit, and link a key to an environment variable | `tool-devtools`, `hs_core::paths::background_command`, `hs_core::environments` | `feat:` minor |
| **H — SQL depth 2** | **ER diagram** from the catalog's foreign keys into `hs-diagram`'s ERD (open in the Diagram Editor, export); **DDL generator** (CREATE TABLE / INDEX / FK from the catalog, per engine dialect); **schema compare** between two connections (tables, columns, types, indexes as New / Missing / Differs with a generated migration script); **data compare** of one table by primary key (sampled, with a bound) | `tool-db::driver::{introspect, catalog}`, `hs-diagram` | `feat:` minor |
| **I — Cassandra / ScyllaDB** | A `DocStore`-shaped driver on the `scylla` crate: keyspaces → tables → rows, CQL editor with completion from the schema, paging, the same guards; connection form (contact points, datacenter, credentials, TLS) | `tool-nosql::store`, `conn_test` | `feat:` minor |
| **J — Forge integration** | In the Git activity, a **Forge** section of the tree for a repository whose credential's host is GitHub, GitLab or Forgejo/Gitea: open **pull / merge requests** (list, open in the Browser, check out the head branch, create one from the current branch), **issues** (list, open, create), **CI status** of the current branch's head (checks / pipelines with their state, open the log in the Browser); refreshed on demand and on push | `tool-git::hosts`, `tool-git::auth`, the Browser activity | `feat:` minor |
| **L — Queue activity** | A new activity (`tool-queue`, rail after Automation): **connections** of kinds `rabbitmq` (management HTTP API + AMQP 0-9-1 through `lapin`), `kafka` (`rdkafka` is C-bound; use the pure-Rust `rskafka` for metadata, produce and consume), `redis-streams` (reuses `tool-redis`'s client shape through `hs-core`), `nats`, `mqtt` (`rumqttc`), `amazon-sqs` / `azure-service-bus` later; credentials in the vault, a Settings → Queue page (list left, form right); a **dashboard** (summaries: brokers, queues / topics, messages ready, consumers; Start Here; saved brokers; Open Here); one tab per broker: **queues / topics** with depth, rate, consumers and lag per consumer group, **peek** messages (headers, payload pretty-printed by content type), **publish** a message (guarded on protected environments), **purge** / **delete** guarded with a typed confirmation, **bindings / exchanges** (RabbitMQ), **partitions and offsets** (Kafka), **consumer groups** with lag, a **live tail** of a queue or topic into the Logs activity. An AI Assistant with "explain this backlog" | `hs_core::connections`, `hs_ui::dashboard`, `tool-logs` source, Jobs | `feat:` minor, several steps |
| **M — ELK** | **Elasticsearch / OpenSearch** connections (URL, basic auth or API key in the vault, TLS, a Settings → Logs or NoSQL page row) through the REST API (`reqwest`): a **logs source** in the Logs activity (an index pattern + a KQL / Lucene query, time-ordered paging with `search_after`, follow mode polling for new records, the parser-aware shape so highlights and statistics work), and a **Search** tab in the NoSQL activity for indices (list with docs / size / health, mappings viewer, a query editor with Query DSL and a results grid, aliases, index lifecycle read-only); **Kibana**: open a saved search / dashboard / Discover with the current query and time range in the Browser activity (deep link built from the Kibana base URL), list Kibana saved objects; cluster **health** cards on the dashboard | `tool-logs::source`, `tool-nosql::store`, `hs_core::ai::http_client`, the Browser activity | `feat:` minor |
| **N — Tracker activity** | A new activity (`tool-tracker`, rail after Automation): **connections** of kinds `jira` (Cloud REST v3 and Server v2, API token or PAT), `redmine` (REST, API key), `gitlab-issues`, `github-issues`, `forgejo-issues` (the Git activity's host clients, reused through `hs-core`), `confluence` (Cloud and Server REST, pages and spaces), `youtrack` and `azure-boards` later; credentials in the vault, a Settings → Tracker page (list left, form right); a **dashboard** (summaries: trackers, assigned to me, due this week, unread mentions; Start Here; saved trackers; Open Here); one tab per tracker: a **query / filter bar** (JQL for Jira, Redmine filters, GitLab / GitHub search syntax, saved filters) over a **DataTable** of items (key, summary, status, assignee, priority, updated) with an **item pane** on the right: description rendered (Markdown / Jira wiki / Textile / Confluence storage format converted to Markdown), comments, attachments, links; **edit** fields (summary, description, assignee, priority, labels, due date), **change status** through the tracker's own transitions / workflow (guarded on protected environments), **comment**, **create** an item, **open in the Browser**; Confluence shows a space → page tree, renders a page, edits it as Markdown and publishes back; an AI Assistant with "summarise this ticket", "draft a reply", "suggest a status" | `hs_core::connections`, `tool-git::hosts` (moved to `hs-core`), `hs_ui::table::DataTable`, `hs_ui::dashboard`, the Browser activity | `feat:` minor, several steps |
| **O — Scratchpad folders** | Notes live in **folders and subfolders** (any depth) in the Scratchpad sidebar; folders, subfolders and notes are **draggable** - reorder within a level, or drop onto another folder or subfolder to move - except pinned notes, which keep their place at the top; a move or a new item whose name clashes with one already in the target gets the existing suffix mechanism (`name 001`, `name 002`, …); folders collapse and remember it; quick-open and search show the path; the notes file layout gains directories so the folder is the directory on disk | `tool-scratchpad`, `hs_ui::tree`, the existing unique-name helper | `feat:` minor |
| **K — Automation activity** | A new activity (`tool-automation`, rail after Containers): **connections** of kinds `jenkins`, `teamcity`, `github-actions`, `gitlab-ci`, `forgejo-actions`, `awx` (Ansible), `puppet` with credentials in the vault and a Settings → Automation page; a **dashboard** (summaries: servers, running, failed today; Start Here; saved servers; Open Here); one tab per server: jobs / pipelines / job templates with last status, duration and who; **trigger** a build or run (parameters asked for; guarded on protected environments), **cancel**, the **queue**, and the **log** of a run streamed into the Logs activity; Puppet shows nodes, their last run status and reports. An AI Assistant with "explain this failure" | `hs_core::connections`, `hs_ui::dashboard`, `tool-logs` command source, Jobs | `feat:` minor, several steps |

Phases A, B, D, E, F, G, J, M, N and O need no new wire protocol and
are the first tranche; C, H, I, K and L follow (L, M and N were added the
same day as the plan, 2026-10-30, at the owner's request). Every phase ends with its row in
`CHECKLIST.md` (this document's table is mirrored there), its spec
updated, its lines in `CHANGELOG.md` and its README entry with
screenshots.

## Credentials for the new activities (owner's rule, 2026-10-30)

Every new activity - Automation (K), Queue (L), ELK (M), Tracker (N) -
accepts a **token** and an **API key** as credential kinds beside
username / password: a credential has a `kind` of `password`, `token`
(bearer / personal access token) or `api-key` (with the header or
query-parameter name the service wants, e.g. `X-Api-Key`,
`PRIVATE-TOKEN`, `X-Redmine-API-Key`, `Authorization: ApiKey`), the
secret itself always a `secret:<ulid>` in the vault, and the form shows
only the fields that kind needs. The Git credentials store
(`hs_core::git_hosts::GitCredential`) is the shape to reuse, widened
with the kind and the header name.

## What "done" means for each

- Exercised in the running app against a real service where one is on
  this machine (Redis and the SQL engines in WSL / Docker, the SSH test
  server, moto for S3, the fixture repositories), and against a mock or
  a fixture where it is not (the hosts' APIs, Event Log on a remote
  Windows host). A phase whose service cannot be reached here says so
  in `CHECKLIST.md` rather than claiming a check it did not make.
- Every destructive verb (bulk delete, kill client, delete version,
  cancel build, trigger on production) asks first and honours the
  protected-environment rule.
- Nothing blocks the UI thread; anything over ~200 ms is a Job.
- Secrets never touch disk in plaintext; each new credential kind has a
  test that greps the data folder.
