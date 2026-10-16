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
| `tool-nosql`: `DocStore`; MongoDB (tree, find, aggregation, views, Extended JSON editing, guarded writes) | Completed | 2026-10-16: 27 unit tests plus `tests/live.rs` against MongoDB 7 in a container; exercised in the app (tree, find, table view, document editor). Schema inference and import/export pending (`docs/tools/nosql.md` §12) |
| `tool-nosql`: CouchDB (databases, `_rev`, Mango, attachments) | Completed | 2026-10-16 for databases, `_all_docs`, Mango `_find`, `_rev` conflict checks and bulk delete, against CouchDB 3 in a container; attachments and views pending |
| `tool-redis`: connection, SCAN tree, type viewers/editors, TTL, console, guards | Completed | 2026-10-16: 27 unit tests plus `tests/live.rs` against a real server (`HS_REDIS=…`); exercised in the app against Redis 8.6 — key tree with type badges, JSON/gzip/binary string viewers, list table, console rendering and the FLUSHALL guard, Info sections. Command completion, Pub/Sub, the live dashboard and Sentinel/Cluster sessions are still open (`docs/tools/redis.md` §11) |

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
| Editor: run/debug/language features (Zed-inspired) | Completed | Run (F5); LSP client (`hs-lsp`, 2026-10-15): completion, hover, diagnostics/Problems, definition, references, rename, format; DAP client (`hs-dap`, 2026-10-16): configurations, breakpoints, stepping, variables, call stack, console; both with an adapter/server table in Settings. Conditional breakpoints, watches and code actions are later requests |
| Database cell viewer previews pictures and documents | Completed | `hs_core::sniff` |
| Devtools: Jasypt, JWT, Firebase tokens, PKI messages, hashes, Base64 | Completed | `tool-devtools`, 19 unit tests with known answers |
| API client (Postman-like, import/export, JavaScript scripts, variables, auth, snippets, libraries) | Completed (P0 + scripts) | `tool-api` on Boa; runner, OAuth 2, WebSocket/GraphQL pending |
| Graphics tool with AI assistant | Completed | `tool-graphics`; AI enhancement through a provider pending |
| Browser with ad/script blocking, inspect, forced dark, per-workspace tabs, Shift+click links | Completed (Windows) | `tool-browser` + `browser_host` (WebView2); other platforms pending |
| Print/snip of web, Markdown, console, code, query and result, S3 listing | Completed | Snip (PNG) and Print… (HTML) for every tool; Redis has no tool yet |
| Terminal: Nushell between Command Prompt and Git Bash (auto-detected) | Completed | `hs_term::shells` (2026-10-15, request 3) |
| Devtools: secrets into the environment/vault, variables usable everywhere, one tab per utility, inputs persisted | Completed | `hs_ui::varpeek`, `Services::expand`, `for_save()` keeps only `{{references}}` |
| `{{variable}}` fields get a view/copy of the value | Completed | eye button → peek popup (devtools, API URL) |
| Title-bar icon rhythm (Read Aloud beside Snip/Print) | Completed | `icon_menu` on `icon_button` (2026-10-15, request 4) |
| Graphics sidebar tidy: aligned rows, full-width sliders, crop placeholder on the canvas | Completed | `Rows` grid helper, `slider_block`, `crop_overlay` |
| Scratchpad: notes collection with sidebar list, pinning, one tab per note, dual pane, AI assistant | Completed | `docs/tools/scratchpad.md` |
| API client assistant that fills the client (collections, requests, fields, scripts, docs) | Completed | `tool-api::assist`, `hs-api` blocks with Apply; `api_tests.rs` |
| Performance pass over the new code | Completed | per-frame digests/verification/parsing cached; assistant context rebuilt on change |
| Graphics: crop placeholder resizes from every edge/corner; live Adjust sliders; Grayscale/Invert toggles | Completed | press-origin grip test, preview layer, `fix/graphics-crop-live-adjust` |
| Speech to text toggle in every AI assistant (offline Windows Speech) | Completed | `hs_core::stt`, `hs_ui::mic`; macOS/Linux engines pending |
| Dictation keeps the phrase in flight on stop; listening time cap | Completed | continuous `RecognizeAsync`, `stop` on stdin, `stt_max_secs` |
| Home workspace cards larger with the workspace logo | Completed | `home::workspace_card` |
| Download for editor files/folders (ZIP), notes, API bodies, devtools outputs | Completed | `hs_core::download`, `tool-editor::zip_dir` |
| Rail labels on by default | Completed | `appearance.rail_labels = true` |
| Delete any workspace (default included) with its data, one must remain; logos wherever a workspace is named | Completed | `Registry::remove`/`startup_name`, switcher, Settings → Workspace |
| AI assistant quick actions in every tool; Insert and Run through each tool's own guarded Run | Completed | `hs_ui::ai_assistant::QuickAction`, ADR-019 amended 2026-10-16 |
| Assistant header and toolbar wording the same in every tool | Completed | Start a New Chat / Close Assistant; "AI Assistant" |
| Terminal and SSH selection readable (premultiplied blend, contrast, cleared on resize) | Completed | `hs_term::widget::blend`/`readable`, `Session::resize` |
| API client and devtools lay out JSON and XML without reordering keys | Completed | `hs_core::fmt`, `ResponseView`, `BodyView::set_text_pretty` |
| Devtools URL encoder/decoder with scopes and a parts table | Completed | `tool-devtools::url_tool`, 5 tests |
| Install button for a missing language server or debug adapter | Completed | registry `install_command`/`install_url`, status bar and Settings |
| Workspace logo in the Startup Workspace dropdown and the environment compare headers | Completed | `settings_ui::ws_icon` |
| Devtools YAML checker and YAML ↔ properties converter | Completed | `tool-devtools::yaml`, 6 tests |
| Editor lays out minified JSON and source maps on open | Completed | `Doc::lay_out_minified`, setting, `.map` = JSON |
| Terminal and SSH drag selects from the pressed character | Completed | `press_origin` anchor, direction-aware side |
| AI providers that are not OpenAI-compatible: Anthropic, Gemini, Azure OpenAI, AWS Bedrock | Completed | `hs_core::ai` six kinds, `Services::ai_credentials`, SigV4 for Bedrock, 17 tests; exercised in the app against the mock server and the forms checked for each kind. Not yet tried against the live services |
| Add a provider from a gallery of services with per-kind fields | Completed | `settings_ai::CATALOGUE`, 15 services in 3 groups, computed grid |
| Home page workspace cards aligned, with a gap before New Workspace | Completed | `tools::home::workspace_card` |
| Devtools YAML tabs keep their own text | Completed | `yaml::State::buffer`, test `each_tab_keeps_its_own_text` |
| Activity rail scrolls, with Home, the sidebar toggle and Settings pinned | Completed | `app::rail`; a short window used to draw the last entries over each other |
| Inspect docked under the page in the browser | Completed | `browser_host::set_devtools_panel`, CDP front end on a local port, off by default (docs/06-security.md) |
| One sidebar header everywhere: title and buttons on one line | Completed | `hs_ui::form::sidebar_header`, SSH, S3, database, Redis and NoSQL |
| Redis key list aligned and indented, "Redis 8.6.3" in the toolbar | Completed | selectable labels instead of stretched buttons |
| SFTP pane copies files and folders to and from the system clipboard | Completed | `hs_core::clipboard_files` (CF_HDROP, 5 tests), `upload_tree`/`download_tree`, palette commands; Copy and paste exercised live against `devsshd` 2026-10-16 |
| A Windows shim (jdtls.bat) wins over an extensionless file of the same name | Completed | `registry::candidates` goes through `PATHEXT` only; same in `hs_dap::adapters`; jdtls exercised live, completion answered |
| SFTP pane: right-click menu, New File, Permissions, Download Zipped, Material file icons, double-click to parent | Completed | `SshTool::entry_menu`/`perms_dialog`/`download_zipped`, `tool-ssh::perms` (5 tests, every mode round-trips), `hs_core::archive` (4 tests, zip/tar/tar.gz), `DataTable::row_icons`. Menu, dialog and icons exercised live against devsshd |
| Drag a row to reorder a list, with a drop indicator | In-Progress | `hs_ui::reorder` (4 tests) + `ConnectionStore::order`/`reorder` (3 tests) + `terminal.shell_order` (1 test); done for SSH hosts, database, Redis, NoSQL, S3, Settings → Connections and the shell profiles — SSH exercised live, UI and files agree. The API collections (nested) still to follow |
| Activity rail entries can be hidden, and come back when a tool is used | Completed | `appearance.rail_hidden`, `App::reveal_rail` at the `open_tool_in` choke point and on connection save; Show checkbox beside the order arrows. Exercised live: hid Redis, saved its connection, icon returned; hid Browser, opened it from Home, icon returned |
| Refresh Status re-reads PATH and rechecks every server and adapter | Completed | `hs_core::env_path` (registry on Windows, 3 tests); exercised live: Java went from Not found to Found with no restart |
| Command and Arguments fields in the extension tables are as wide as the page | Completed | `extension_field_widths`; a fixed width made the window grow every frame |
| Debug adapter for JavaScript and TypeScript | Completed | `js-debug`, TCP; `launch.json` "type": "node"/"pwa-node" maps to it |
| Run and Debug ask about unsaved files (Save All / Ignore / Cancel) | Completed | `hs_ui::dialog::three_way`, `save_all_dirty` |
| Home: pinned connections are really pinned, above Recent, bigger headings and pills by kind | Completed | `ConnectionProfile::pinned` + `set_pinned`, `form::pin_menu`, `launcher::kind_colour`/`pill`/`section_heading`; exercised live — pinning a host wrote the flag and the pill appeared |
| One tab per connection, three for an SSH host | Completed | `app::tabs_per_connection`, `unbound_tab`; every tool routes its sidebar clicks through the shell |
| Pinned tabs keep their name, with a colour marker | Completed | `TabItem::icon_only_when_pinned`, `PINNED_TEXT_WIDTH` (1 test); browser stays icon-only |
| The AI assistant is a column in every tool, closed only by its toggle | Completed | `ai_assistant::split_for_assistant`; database, terminal, SSH and S3 converted (API client and Graphics already split). Verified live: the terminal reflowed 131x33 to 80x33, so the area really shrank |
| Devtools: generators (UUID, number, password, passphrase), TOML and XML check and format | Completed | `tool-devtools::generate` (11 tests) and `::markup` (9 tests), `fmt::pretty_xml_with` for the indent; generators exercised live |
| Documentation, screenshots, GIF | In-Progress | docs updated 2026-10-15; new screenshots captured with the PowerShell harness; the walkthrough GIF still shows the 0.17 flows |
