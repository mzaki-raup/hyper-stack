# HyperStack — implementation checklist

Mirrors the phases and milestones of [docs/07-roadmap.md](docs/07-roadmap.md).
States: `Completed` · `In-Progress` · `Pending`. Update a line only after the
work is verified (checks pass and the feature was exercised in the running
app), then commit and push.

Last updated: 2026-10-15 (Home tab, theme modes, accessibility, devtools, API client, graphics, browser, snip/print verified in the running app)

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
| Pester tests for `scripts/next-release.ps1` | Completed | `scripts/tests/next-release.Tests.ps1`: 33 cases on fixture repos; run in `ci.yml` |
| App icon master and derived sizes in `assets/icon/` (`hyperstack.ico`, PNG 16–512), `scripts/make-icons.ps1` | Completed | 2026-10-10 |
| Binary flags `--version`, `--self-test`, `--data-dir`, `--finish-update`; `build.rs` embeds `hyperstack.ico` + version resources; window icon via `with_icon` | Completed | `--finish-update` only waits for the old PID; the hand-over itself is M1 |
| `hs-core`: `Tool`/`ToolFactory`, `Services`, `ToolContext`, actions queue | Completed | |
| `hs-core`: tokio runtime, `UiWaker`, `Jobs`, `Events` | Completed | |
| `hs-core`: portable paths (`HyperStackData/`, writability probe, fallback), atomic TOML/JSON io, schema versioning | Completed | SQLite history db deferred to M3 (first consumer) |
| `hs-core`: `Connections`, `Environments`, variable substitution, `Secrets` (vault; keyring opt-in) | Completed | DPAPI remember-me deferred to M1 with the Updates/Security pages |
| `hs-core`: settings, shortcuts, i18n (Fluent), `tracing` with redaction | Completed | |
| `hs-ui`: theme, multi-row `TabStrip`, `SplitLayout`, `Palette`, `Tree`, `DataTable` v1, toasts, dialogs, `EnvBanner`, `KeyValueEditor` | Completed | |
| `hyperstack` bin: window, title bar, activity rail, sidebar, dock, status bar, launcher, settings, workspaces, layout persistence | Completed | native window decorations in Phase 0; custom title bar later; workspaces as projects (create/clone/switch/delete, copy-from picker, per-workspace logo) 2026-10-11; launcher pinned-connection chips open or focus their SSH/S3/DB tab 2026-10-12 |
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
| `tool-s3`: profiles, AWS import, SigV4 client, browsing, multipart upload, download, guarded delete, presign, `s3://` VFS | Completed | 2026-10-11: SigV4 verified against AWS examples; client tested end to end against moto (`HS_S3_ENDPOINT`); browsing, import, editor round trip, folder create and guarded delete exercised in the app. Upload/Download use native file dialogs, which cannot open headless, so they were exercised through the client test only. `s3://` VFS deferred (temp-file round trip like SFTP) |

### M3 · SQL database + NoSQL + Redis
| Item | State | Notes |
|---|---|---|
| `tool-db`: `DbSession`; SQLite, PostgreSQL, MySQL/MariaDB | Completed | 2026-10-12: `tests/drivers.rs` passes on SQLite, PostgreSQL 16 (incl. TLS `require`/`verify-full`) and MariaDB 10.11; streaming, row limit with server-side stop, cancel, introspection, read-only sessions |
| Schema browser, SQL editor with completion, run statement/selection/script, cancel | Completed | Exercised in the app on all three engines; error positions underlined; Escape cancels. `:name` parameter prompts and SQL formatting deferred |
| Result grid streaming, cell viewer, copy/export | Completed | Sort, filter, NULL styling, cell viewer, Fetch More, Copy As (6 formats), Export of all rows as a Job on a second connection (native save dialog, so not exercised headless; covered by the `FileSink` test). Column reorder/hide, transpose, Excel deferred |
| History, saved queries, transactions, read-only, destructive classifier, SSH tunnel | Completed | Guards exercised on a protected environment (refusal, Allow Writes, typed name); tunnel exercised through OpenSSH; the vault keeps the password (grep found no plaintext) |
| `tool-nosql`: `DocStore`; MongoDB (tree, find, aggregation, views, Extended JSON editing, guarded writes) | Pending | |
| `tool-nosql`: CouchDB (databases, `_rev`, Mango, attachments) | Pending | |
| `tool-redis`: connection, SCAN tree, type viewers/editors, TTL, console, guards | Pending | |

### M4 · API client + Git
| Item | State | Notes |
|---|---|---|
| `tool-api`: request editor, auth (incl. OAuth2, SigV4), send, response viewer, history, curl import/export | Pending | |
| Collections as files, variable scopes, importers (Postman, Insomnia, Bruno, OpenAPI) | Pending | |
| Git read side in the editor: `hs-git` (status, diffs, log, branches over the git CLI), branch + change letters in the tree, Source Control list, diff viewer (inline / side by side), history with commit details, gutter change markers | Completed | 2026-10-12, ahead of M4 at the owner's request: parsers and a real fixture repository in `hs-git` tests; exercised in the app on a repo with staged/unstaged/deleted/untracked files, a tag and a second branch (tree, list, both diff modes, arrow stepping, history and Down, Esc, live refresh on an outside edit, live gutter while typing) |
| `tool-git`: stage/unstage (file + hunk), commit, branches, stash, fetch/pull/push via CLI, protected-remote guard | Pending | |

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
| Editor: AI assistant (1min.AI / OpenAI-compatible, edit blocks, Cascade; ADR-019) | Completed | 2026-10-11, owner's request ahead of the roadmap: chat with file/selection context, diffs with Apply/Reject (one undo step each), Cascade rounds, Settings → AI Assistant with several providers, Fetch Models and Test Connection; exercised in the app against a mock server speaking both the 1min.AI and the OpenAI SSE formats; API key stored in the vault and the data folder grepped clean. Not yet tried against the live 1min.AI service or a real LM Studio |
| Editor: several project roots, each with its own editor tabs | Completed | 2026-10-12: one editor tab, a row of file tabs per project, Other Files row, close guards, Ctrl+F4 / Ctrl+Alt+PgUp/PgDn, routing from other tools via `uri_affinity`, one-file layouts migrated; tool-level test with real services, and exercised in the app (command-line folder, tree, terminal Ctrl+click, unsaved-close dialog, restart restore) |
| Editor: one window tab per project with a multi-row strip of file tabs inside (replaces "Open Files In") | Completed | 2026-10-12, owner's request: exercised in the app (a two-project tab split into two project tabs, 13 files wrapping in the file strip, 10 project tabs wrapping in the window strip, close and the file-tab menu), editor tool test, tab strip kittest |
| Editor: "Open Files In" setting (Separate Tabs default, Project Rows with wrapping rows) | Completed | 2026-10-12, after the owner found files "overwriting" the tab: exercised both modes in the app (new tab per tree click, focus of an open file, projects carried to new tabs, live switch, a 14-file row wrapping) and in the tool-level test |
| AI: model dropdown from the provider's model list | Completed | 2026-10-12: LM Studio-style `/v1/models` list, 1min.AI built-in list, custom id, refresh, error shown in the list; exercised against the mock server |
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

## UI follow-up (2026-10-14)

| Item | State | Notes |
|---|---|---|
| Workspace selected logo, post-create workspace name/logo editing, fixed Workspace settings route | Completed | 2026-10-15: created a workspace with a logo in the running app; the logo shows on the switcher button and in the list; Settings → Workspace renames and replaces the logo |
| Persistent pinned Settings tab, Welcome page migration, Quick Open popup, notification history, app logo | Completed | 2026-10-15: Welcome now lives on the Home tab; Settings and Home are pinned and present in every workspace |
| Clickable appearance palettes and editor file-tab width setting | Completed | 2026-10-15: swatches pick the scheme for their mode; theme mode persists across restart (checked twice with the harness) |
| Project-root tab deduplication and encoding selection/reopen | Completed | 2026-10-15 |
| Reusable AI assistant across terminal, SSH, SQL, S3 and API tools | Completed | 2026-10-15: shared panel in Terminal, SSH, SQL, S3, API client and Graphics; fenced blocks get Copy/Insert |

## Owner's requests (2026-10-15)

| Item | State | Notes |
|---|---|---|
| Theme mode System/Dark/Light persisted on Windows 10 | Completed | ADR-022; both egui visuals slots filled |
| Home tab first in the rail; sidebar toggle and Settings at the bottom; rail labels and order setting | Completed | Settings → Appearance → Activity Rail |
| Accessibility settings page with Read Aloud (TTS) and reduce motion | Completed | `hs_core::tts`, `Tool::speakable` |
| Settings import/export behind the master password | Completed | `hs_core::portable` bundle |
| Workspace settings between Appearance and Environments: rename, logo, environment compare/sync | Completed | Accessibility sits between Appearance and Workspace |
| Editor: full file path row under the file tabs | Completed | click copies |
| Editor: run/debug/language features (Zed-inspired) | In-Progress | Run (F5) for files and projects done; LSP completion/diagnostics and a debugger are not started |
| Database cell viewer previews pictures and documents | Completed | `hs_core::sniff` |
| Devtools: Jasypt, JWT, Firebase tokens, PKI messages, hashes, Base64 | Completed | `tool-devtools`, 19 unit tests with known answers |
| API client (Postman-like, import/export, JavaScript scripts, variables, auth, snippets, libraries) | Completed (P0 + scripts) | `tool-api` on Boa; runner, OAuth 2, WebSocket/GraphQL pending |
| Graphics tool with AI assistant | Completed | `tool-graphics`; AI enhancement through a provider pending |
| Browser with ad/script blocking, inspect, forced dark, per-workspace tabs, Shift+click links | Completed (Windows) | `tool-browser` + `browser_host` (WebView2); other platforms pending |
| Print/snip of web, Markdown, console, code, query and result, S3 listing | Completed | Snip (PNG) and Print… (HTML) for every tool; Redis has no tool yet |
| Terminal: Nushell between Command Prompt and Git Bash (auto-detected) | Completed | `hs_term::shells` (2026-10-15, request 3) |
| Devtools: secrets into the environment/vault, variables usable everywhere, one tab per utility, inputs persisted | Completed | `hs_ui::varpeek`, `Services::expand`, `for_save()` keeps only `{{references}}` |
| `{{variable}}` fields get a view/copy of the value | Completed | eye button → peek popup (devtools, API URL) |
| Documentation, screenshots, GIF | In-Progress | docs updated 2026-10-15; new screenshots captured with the PowerShell harness; the walkthrough GIF still shows the 0.17 flows |
