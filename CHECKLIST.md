# HyperStack — implementation checklist

Mirrors the phases and milestones of [docs/07-roadmap.md](docs/07-roadmap.md).
States: `Completed` · `In-Progress` · `Pending`. Update a line only after the
work is verified (checks pass and the feature was exercised in the running
app), then commit and push.

Last updated: 2026-10-11 (M1 complete: terminal search and links, Mermaid preview)

## Planning

| Item | State | Notes |
|---|---|---|
| Vision, scope, feature catalogue | Completed | `docs/01`, `docs/02` |
| Architecture, UI design, data model, security | Completed | `docs/03` – `docs/06` |
| Roadmap and decision records | Completed | `docs/07`, `docs/08` (ADR-001 … ADR-018) |
| Tool specs (12) | Completed | `docs/tools/` |
| CI/CD and updater design | Completed | `docs/09` |
| Forgejo workflows and release scripts committed and green without a workspace | Completed | `.forgejo/workflows/`, `scripts/`, `keys/README.md` |
| Public GitHub mirror: build-time forge parameters, `.github/workflows/`, `changelog-section.ps1`, `sync-from-github.ps1`, version authority on Forgejo | In-Progress | Code, scripts, workflows and `docs/09` §10 done and gated locally; the GitHub repository, its `HS_MINISIGN_KEY` secret and the first tag release are the owner's set-up steps |

## Phase 0 — Foundation

| Item | State | Notes |
|---|---|---|
| Cargo workspace, `[workspace.package] version`, `deny.toml`, rustfmt/clippy clean | Completed | 2026-10-10, `phase-0` branch |
| Pester tests for `scripts/next-release.ps1` | Pending | |
| App icon master and derived sizes in `assets/icon/` (`hyperstack.ico`, PNG 16–512), `scripts/make-icons.ps1` | Completed | 2026-10-10 |
| Binary flags `--version`, `--self-test`, `--data-dir`, `--finish-update`; `build.rs` embeds `hyperstack.ico` + version resources; window icon via `with_icon` | Completed | `--finish-update` only waits for the old PID; the hand-over itself is M1 |
| `hs-core`: `Tool`/`ToolFactory`, `Services`, `ToolContext`, actions queue | Completed | |
| `hs-core`: tokio runtime, `UiWaker`, `Jobs`, `Events` | Completed | |
| `hs-core`: portable paths (`HyperStackData/`, writability probe, fallback), atomic TOML/JSON io, schema versioning | Completed | SQLite history db deferred to M3 (first consumer) |
| `hs-core`: `Connections`, `Environments`, variable substitution, `Secrets` (vault; keyring opt-in) | Completed | DPAPI remember-me deferred to M1 with the Updates/Security pages |
| `hs-core`: settings, shortcuts, i18n (Fluent), `tracing` with redaction | Completed | |
| `hs-ui`: theme, multi-row `TabStrip`, `SplitLayout`, `Palette`, `Tree`, `DataTable` v1, toasts, dialogs, `EnvBanner`, `KeyValueEditor` | Completed | |
| `hyperstack` bin: window, title bar, activity rail, sidebar, dock, status bar, launcher, settings, workspaces, layout persistence | Completed | native window decorations in Phase 0; custom title bar later; workspaces as projects (create/clone/switch/delete, copy-from picker, per-workspace logo) 2026-10-11 |
| Two dummy tools; `egui_kittest` interaction test for the tab strip | Completed | image snapshots skipped: the runner has no GPU |
| `portable-pty` verified on Windows (ConPTY DSR handshake) | Completed | PowerShell 7 profile loading is checked in M1 with the real terminal |
| Minisign key pair generated, `keys/minisign.pub` committed and embedded | Completed | key id `99C2A397C6491EAB` (`--unencrypted`), 2026-10-10 |
| `HS_MINISIGN_KEY` repository secret set on the forge (Settings → Actions → Secrets) | Completed | set by the owner, 2026-10-10 |
| `v0.1.0` published automatically and passes `verify-release.ps1` | Completed | release run 11, 2026-10-10; verified locally too |
| Phase 0 exit test (USB-stick move, no plaintext secret, nothing outside the folder) | Pending | |

## Phase 1 — MVP tools (v1.0)

### M1 · Terminal + Editor
| Item | State | Notes |
|---|---|---|
| `hs-term`: grid, listener, key encoding, `portable-pty`, shell profiles | Completed | ConPTY DSR handshake and exit detection, see CLAUDE.md |
| `tool-terminal`: tabs, splits, search, links, "open here", profile settings | Completed | search (Ctrl+Shift+F, regex/case, highlights) and Ctrl+click links (URLs, `path:line:col` → editor) 2026-10-11, also in the dock terminal; in-tab splits, broadcast and the task runner are later items of the spec |
| `hs-editor`: rope buffer, cursors, undo tree, syntect highlighting, gutter, find/replace, brackets, indent | Completed | undo is grouped linear history (tree deferred); per-line syntect cache with checkpoints; multi-cursor; Alt+F3 selects all occurrences because Ctrl+Shift+L locks the vault |
| `tool-editor`: project tree + watcher, VFS open/save, encodings, EOL, dirty handling, quick-open provider | Completed | local files only; `sftp://` and `s3://` arrive with M2; files > 32 MB open read-only, > 256 MB refused until the streaming mode |
| Markdown preview with native Mermaid subset | Completed | pulldown-cmark preview side by side (Ctrl+Shift+M); Mermaid flowcharts and sequence diagrams drawn natively (`hs-editor::mermaid`), checked against the diagrams in `docs/03` 2026-10-11 |
| Settings → Updates: version, Check now, notes, download + verify, restart-and-install, Revert | Completed | live-tested 2026-10-11: a dev build downloaded the published v0.3.0, verified it, renamed itself aside, relaunched v0.3.0 (`ok = true`), and `--revert-update` swapped the kept v0.2.0 back; both via the hidden headless flags |

### M2 · SSH/SFTP + S3
| Item | State | Notes |
|---|---|---|
| `tool-ssh`: profiles, auth methods (password/key/agent), known hosts, shell tab | Completed | ~/.ssh/config import, jump hosts and agent-forwarding are P1 |
| SFTP dual pane, transfers as Jobs, folder sync with the shell, edit-in-editor | Completed | 50/50 split tracks the window width, shell stable on mouse move (2026-10-11); transfer queue/resume, chmod dialog and the `sftp://` VFS are P1 |
| `Tunnels` service (local forward) | Completed | `hs_core::tunnels::Tunnels` + `hs_ssh::tunnels::TunnelManager`; SSH sidebar list and New Tunnel form; end-to-end test through the in-process server, live-tested through OpenSSH 2026-10-11; `-R`/`-D` are P1 |
| `tool-s3`: profiles, AWS import, SigV4 client, browsing, multipart upload, download, guarded delete, presign, `s3://` VFS | Pending | |

### M3 · SQL database + NoSQL + Redis
| Item | State | Notes |
|---|---|---|
| `tool-db`: `DbDriver`; SQLite, PostgreSQL, MySQL/MariaDB | Pending | |
| Schema browser, SQL editor with completion, run statement/selection/script, cancel | Pending | |
| Result grid streaming, cell viewer, copy/export | Pending | |
| History, saved queries, transactions, read-only, destructive classifier, SSH tunnel | Pending | |
| `tool-nosql`: `DocStore`; MongoDB (tree, find, aggregation, views, Extended JSON editing, guarded writes) | Pending | |
| `tool-nosql`: CouchDB (databases, `_rev`, Mango, attachments) | Pending | |
| `tool-redis`: connection, SCAN tree, type viewers/editors, TTL, console, guards | Pending | |

### M4 · API client + Git
| Item | State | Notes |
|---|---|---|
| `tool-api`: request editor, auth (incl. OAuth2, SigV4), send, response viewer, history, curl import/export | Pending | |
| Collections as files, variable scopes, importers (Postman, Insomnia, Bruno, OpenAPI) | Pending | |
| `tool-git`: status via `gix`, stage/unstage (file + hunk), diff viewer, commit, branches, stash, fetch/pull/push via CLI, protected-remote guard | Pending | |

### M5 · Logs + Devtools core
| Item | State | Notes |
|---|---|---|
| `tool-logs`: local file engine, SSH source, parsers, filters, highlights, structured view, find, bookmarks, saved sources | Pending | |
| `tool-devtools`: JSON/YAML/TOML/XML, JWT, Base64/URL/hex, regex, cron, UUID/ULID/hash, timestamp, diff | Pending | |

### M6 · Hardening and 1.0
| Item | State | Notes |
|---|---|---|
| Protected environments in all tools; e2e secret-grep test; `cargo deny` clean | Pending | |
| Importer polish, workspace export, `.gitignore` template, `.local.toml` overrides | Pending | |
| Performance pass against budgets | Pending | |
| Updates: daily check + badge, install on exit, install from file, CHANGELOG rendering, crash auto-revert, read-only fallback | Pending | |
| Docs, release packaging per platform, `en` translations complete | Pending | |
| `release: 1.0.0` published | Pending | |

## Phase 2 — Depth

| Item | State | Notes |
|---|---|---|
| Containers: Docker/Podman, Compose, remote over SSH | Pending | |
| API: Rhai scripts, runner, WebSocket/SSE, GraphQL, cookies/proxy/certs | Pending | |
| SQL: SQL Server, inline editing, explain, CSV import, Excel export | Pending | |
| NoSQL: Elasticsearch/OpenSearch, DynamoDB, indexes, schema inference, import/export, copy collection, CouchDB views/replication | Pending | |
| Redis: dashboard, slowlog, pub/sub, streams, big-key finder, bulk ops | Pending | |
| Logs: Docker/k8s/journalctl/Event Log sources, merge, statistics, alerts | Pending | |
| Editor: tree-sitter, minimap, snippets, formatter, three-way merge | Pending | |
| Git: log graph, blame, merge/rebase/cherry-pick, conflicts | Pending | |
| SSH: port-forward UI, broadcast, server monitor | Pending | |
| S3: metadata/tags/versions, sync, policy viewer | Pending | |
| Shell: drag and drop, global search, installed mode | Pending | |
| Delivery: Linux x64 via `cargo-zigbuild`, beta channel, mirror source | Pending | |
| Devtools: network tools, process viewer, snippets, `.env` editor, scratchpad | Pending | |

## Phase 3 — Reach

| Item | State | Notes |
|---|---|---|
| Kubernetes | Pending | |
| gRPC, mock server, OpenAPI export | Pending | |
| SQL: ER diagram, DDL, schema/data compare, ClickHouse/DuckDB | Pending | |
| NoSQL: Cassandra/ScyllaDB | Pending | |
| Forge integration (GitHub/GitLab/Forgejo) | Pending | |
| Plugin API (WASM) | Pending | |
| Detachable tabs, audit log, presentation mode | Pending | |
| macOS runner and bundle, aarch64 builds | Pending | |
