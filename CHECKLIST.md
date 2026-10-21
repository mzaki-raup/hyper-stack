# HyperStack — implementation checklist

Mirrors the phases and milestones of [docs/07-roadmap.md](docs/07-roadmap.md).
States: `Completed` · `In-Progress` · `Pending`. Update a line only after the
work is verified (checks pass and the feature was exercised in the running
app), then commit and push.

Last updated: 2026-10-20 (Graphics diagrams: R1.1 (project save, autosave, resize/align, edge labels/waypoints) merged; Release 2 (ERD, sequence, Archify/draw.io import) remains)

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
| Collections as files, variable scopes, importers (Postman, Insomnia, Bruno, OpenAPI) | Completed | collections as files and Postman done 2026-10-15; Insomnia v4, Bruno folders and OpenAPI 3 (JSON/YAML) importers 2026-10-18 (`tool-api::importers`, 5 tests) |
| Git read side in the editor: `hs-git` (status, diffs, log, branches over the git CLI), branch + change letters in the tree, Source Control list, diff viewer (inline / side by side), history with commit details, gutter change markers | Completed | 2026-10-12, ahead of M4 at the owner's request: parsers and a real fixture repository in `hs-git` tests; exercised in the app on a repo with staged/unstaged/deleted/untracked files, a tag and a second branch (tree, list, both diff modes, arrow stepping, history and Down, Esc, live refresh on an outside edit, live gutter while typing) |
| `tool-git`: stage/unstage (file + hunk), commit, branches, stash, fetch/pull/push via CLI, protected-remote guard | Completed | Source Control supports stage/unstage/discard, commit/amend/sign-off with a Conventional Commit prefix picker, stash, branch create/switch/delete/rename, set upstream, remotes add/edit/remove, lightweight and annotated tag create/delete/push, fetch/pull/push, hunk actions, protected-push confirmation, merge/rebase/cherry-pick with Continue/Skip/Abort state controls, interactive rebase (pick/squash/fixup/drop/reorder), whole-file conflict resolution (Keep Ours / Keep Theirs / Mark as Resolved) and a block-by-block conflict editor with Ours / Theirs / Both / a typed result per block. Signed tags are deliberately left to the terminal so no pinentry prompt can block the app. Engine tests cover cherry-pick, rebase operations, interactive rebase, annotated tags and conflict parsing/resolution (all with fixture repositories that disable signing). |

### M5 · Logs + Devtools core
| Item | State | Notes |
|---|---|---|
| `tool-logs`: local file engine, SSH source, parsers, filters, highlights, structured view, find, bookmarks, saved sources | Pending | |
| `tool-devtools`: JSON/YAML/TOML/XML, JWT, Base64/URL/hex, regex, cron, UUID/ULID/hash, timestamp, diff | Completed | JSON/YAML/TOML/XML, JWT, Base64/URL, hashes, generators (UUID/ULID), cron and the timestamp converter landed 2026-10-15..17; Regex Tester and Text Diff 2026-10-18 (`regex_tool`, `diff_tool`, 5 tests) |

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
| Devtools: generators, TOML and XML check and format, cron builder | Completed | `tool-devtools::generate` (12 tests) and `::markup` (9 tests) and `::cron` (12 tests, including that every built expression passes its own check), `fmt::pretty_xml_with` for the indent; generators and cron exercised live |
| Devtools → Cron: a checked expression is explained in a Cron Type / Match / Details grid | Completed | `cron::explain`; verified live — `0 30 9 ? * MON,WED,FRI` reads as "At 09:30, on day of the week MON,WED,FRI." for Spring and Quartz, rejected for Unix |
| Devtools → Generators: each generator keeps its own result card | Completed | `generate::State::output`/`set_output`, card titled by generator; verified live — a password generated between two visits left the UUID list intact |
| Database: one connection per window tab, many query tabs inside it | Completed | `QueryDoc` + `doc()`/`doc_mut()`, `query_tabs` strip, `State::queries`/`active_query`; verified live — two Untitled tabs kept separate text, the dirty dot followed the right one, and Ctrl+Enter ran the tab in front |
| Home: the sidebar carries the workspace, Pinned and Recent | Completed | `launcher::side`, drawn under the page when no tabs are open; verified live |
| Home: Get Started in the owner's order | Completed | workspace, environments, connection, project, terminal, quick open; verified live |
| API client: the method picker is as tall as the URL field | Completed | `form::field_row` sizes a control to `form::field_height`; verified live at 4x — both frames share a top and bottom edge |
| Dialog buttons right-aligned everywhere | Completed | audited all 28 modals; the SQL guard, Save Query and S3 New Folder were left-aligned and now match |
| API client: the two script boxes are the same height | Completed | each gets half the pane in its own scroll area |
| Shell: Split Right/Down removed from the tab menu and default keys | Completed | menu entries and `tab.split_*` bindings dropped, palette commands kept; verified live — a Database tab's menu ends at Duplicate |
| Shell: Duplicate refused where a second tab is meaningless | Completed | `Tool::current_connection` + `tabs_per_connection` (SSH 3) and `can_duplicate_tab` (Graphics); verified live — Duplicate greyed on a Database tab |
| Connections: Pin to Home in every connection sidebar | Completed | `form::connection_row` carries the menu for SSH, Database, Redis, NoSQL and S3; verified live — pinned a SQLite profile from the Database sidebar |
| Home: pinned connections grouped by type, in rail order | Completed | `pinned_by_kind` ranks groups by `app::rail_order`; verified live — a DB heading with its colour dot above the pill |
| Home: the workspace logo sits beside the sidebar title | Completed | `form::sidebar_header_icon`, falling back to the workspace glyph; verified live |
| Home: Recent rows align left | Completed | `Atom::grow()` holds the row at full width with the label at the left edge; verified live |
| Database: the row number is frozen beside the grid | Completed | `grid::paint_gutter` numbers the rows the table drew, outside the scroll area; verified live — the numbers held while the data scrolled to `PASSPORT_ISSUE_DATE` |
| Database: the result footer stays reachable when narrow | Completed | laid out right to left, the filter box shrinks first; verified live — Copy As and Export both whole with the assistant open |
| Database: generated statements are terminated | Completed | Open Data and Count Rows emit `SELECT …;`; verified live against SQLite |
| Database: the result grid scrolls horizontally | Completed | `grid::show` wraps the table in a horizontal `ScrollArea` sized from the columns' own widths; verified live — a 12-column result reached `PASSPORT_EXTEND_DATE` and beyond with the assistant open, and a 3-column result still fills the pane with no scrollbar |
| Database: Open Data runs in its own query tab | Completed | `run_in_new_tab` fills an empty Untitled tab or adds one, then runs; `reusable_tab` covered by three unit tests. Verified live against a SQLite fixture — Open Data on `cand` filled the blank tab with `SELECT * FROM cand` and 40 rows, then Count Rows opened a second tab with `SELECT COUNT(*) FROM cand` = 40 and left the first untouched |
| Database: Prettify lays the open query out again | Completed | `sqlformat` 0.5 (MIT OR Apache-2.0, `cargo deny` clean); verified live — a one-line join became 13 indented lines |
| API client: the method is coloured, in the tab too | Completed | `ui::method_rgb` fixed colours, `TabItem::title_prefix` colours only the method; verified live — GET, POST, PUT and DELETE tabs each in their own colour |
| API client: one window tab per request | Completed | `uri_affinity` claims an `api://` URI only while the tab is empty, the exact URI focuses the tab that has it; verified live — Ping filled the empty tab, Health opened a second, Ping again focused the first |
| API client: drag a request between folders | Completed | `Reorder::scoped` names each level in the drag payload, `Move::From` carries it; verified live — four drags in and out of a folder, each correct on disk |
| API client: a copy or a clashing drop is numbered " 001" | Completed | `model::next_copy_name` and `free_name` (3 tests); verified live |
| API collections: drag to reorder at every level | Completed | one `hs_ui::reorder` per level keyed by collection and folder, `Collection::order` for the collections themselves; verified live — a request, a request inside a folder and a whole collection each moved and the move was on disk |
| Settings → Workspace lists every workspace | Completed | a Workspaces section from `Registry::load`, `SettingsEffect::RunAction` carries `ws:<name>` and `ws.new` to the shell; verified live |
| Browser: forced dark mode covers the whole page | Completed | the filter moved from `html` (which inverted its own background) to `body`, `html` left dark; verified live — example.com stays dark to the end of the page |
| The browser address bar can be typed into | Completed | `Cmd::AddressFocus` + `EnableWindow(child, FALSE)` in `browser_host`; verified live — typed example.com into the bar and the page navigated |
| Browser page tabs show the site's favicon | Completed | `hs_core::favicon` (4 tests) fetches `/favicon.ico` per origin and normalises it to a 32px PNG; `TabItem::icon_uri` draws it; verified live — the DuckDuckGo tab shows its duck |
| Database: the schema is a column beside the editor, not a sidebar block | Completed | `schema_column` in a resizable `Panel::left`; verified live — hiding the sidebar leaves the version, filter and tree in place |
| AI assistant in Redis and NoSQL | Completed | `ai_assistant::split_for_assistant` plus a `body`/`assistant_column` split so the column still draws on the tool's early-return paths; `redis.ai` and `nosql.ai` in the palette, Read Aloud reads the last answer; verified live — both columns open, including while the connection is down |
| Settings → Security: credentials table, masked passwords, per-row reveal that lapses | Completed | `crate::credentials` (4 tests), `SettingsEffect::CopySecret` and the clipboard timer in `app.rs`; verified live — the grid lists all eight columns, a wrong master password is refused, the right one revealed one row and it re-masked after 30 s |
| SFTP actions echo their command into the shell pane | Completed | `tool_ssh::echo_sftp` writes them as comments (mkdir/touch/mv/rm/chmod); verified live against devsshd, which now swallows comment lines like a real shell |
| The AI Assistant button is hard right on every toolbar | Completed | `right_to_left` group in editor, terminal, SSH, SQL, S3, API client, Graphics and Scratchpad; verified live in the editor, SQL and SSH |
| UI casing: a parenthetical on a label starts with a capital | Completed | 13 strings in `locales/en.ftl` plus `redis::profile::mode_label`; rule recorded in CLAUDE.md; verified live — "None (Direct connection)", "Standalone" |
| Settings → Connections: Test Connection between Save and Delete | Completed | `conn_test` (2 tests): SSH, S3, SQL, Redis, MongoDB and CouchDB through their own drivers, on the runtime; verified live — SQLite answered "Connected. SQLite 3.50.2" and a Redis server that was not running answered "timed out" |
| Graphics: one tab per image, reopening focuses the tab that has it | Completed | `uri_affinity` answers only while the tab is empty, both open paths go through `Services::open`; verified live — red then blue made two tabs, reopening red focused its own |
| Documentation, screenshots, GIF | Completed | docs updated 2026-10-15; superseded by the 2026-10-19 documentation and screenshot pass below |

## Owner's requests, 2026-10-17

Raised directly by the repository owner on 2026-10-17, tracked here so
the queue does not live only in a conversation. Items marked *do not
start* are held or deferred by the owner's instruction.

| Item | State | Notes |
|---|---|---|
| API client: outer-tab Duplicate copies the request, numbered, in its folder | Completed | `Tool::duplicate_state` + `ApiTool::duplicate_state`; verified live — one click produced `New Request 001` on disk and one new tab |
| API client: folder New Request numbers against the folder | Completed | `free_name` over `sibling_names`; verified live — three clicks gave New Request, 001, 002 |
| API client: the sidebar's New Request button creates a request | Completed | `home_for_new_request`; offers a collection when there are none; verified live |
| API client: the list is the source of truth for the workspace | Completed | every creation path (button, folder menu, tab Duplicate) now writes into a collection on disk; verified live against the collection file |
| API client: request-name row above the action bar, renaming in place | Completed | `name_row` writes through on Enter or blur; verified live — renaming to Orders updated the list row and the tab |
| API client: drag a request between folders, sibling, parent and sub | Completed | `reorder::drop_onto` makes a folder a target in its own right; verified live — into a collapsed empty folder and back out again |
| Browser: one outer tab per workspace, carrying its icon and name | Completed | `Tool::single_tab` + `tab_icon`/`tab_icon_uri`; verified live — the tab reads `default` with the workspace glyph |
| Browser: Favorites tab - tree with folders, per-workspace store | Completed | `store::Favorites` + `side.rs`; verified live — a page added from the toolbar appeared with its real favicon, Open/Edit/Delete on right-click, and the detail pane narrowed the page rather than covering it |
| Browser: Passwords tab - per-workspace store | Completed | `store::Passwords`; the three secret fields hold vault references only; verified live — a new login showed `Nothing stored.` and the locked-vault warning |
| Browser: Add to Favorites in the toolbar | Completed | root, an existing folder or a new one; verified live against `favorites.json` |
| Browser: Fill in Password in the toolbar | Completed (partly) | matches by host and fills the page's own username/password/API-key fields through `Cmd::Script`; verified live — the dialog matched `duckduckgo.com` and refused while locked. **HTTP basic auth and passkeys cannot be filled**: one is a native dialog outside the page, the other a WebAuthn exchange |
| Editor: Java debug adapter | Completed | `java-debug` in `hs_dap::adapters`, answering `"type": "java"`; two tests, one of which refuses any argument holding a placeholder nothing substitutes |
| Editor: Run/Debug asks about unsaved tabs | Completed | the three-way prompt already existed for both Run and Debug; it skipped documents with no path, so a brand-new file was never mentioned or written. `save` already falls back to Save As, so the filter was the only thing in the way |
| Devtools: Unit Conversion tool | Completed | `convert.rs`: Time, Epoch, Numbers, Data Size and Duration panes, each with its own inputs and result; 10 unit tests; verified live — 1 hour = 3600 s with every unit listed, and 255 = 0b11111111 = 0o377 = 0xFF = `ÿ` |
| Database: word-wrap toggle in the query field | Pending | held by the owner, 2026-10-17 - do not start |
| Shell: middle-click slowness | Pending | deferred by the owner, not reproducible - do not start |

## Owner's requests, 2026-10-17 (second batch)

A second set raised by the repository owner on 2026-10-17. Most of
these share one mechanism - a permanent first tab per activity, with a
dashboard of what that activity has open - so the shared parts are
listed first.

| Item | State | Notes |
|---|---|---|
| Shell: a tool's first window tab is permanent | Completed | `Tool::wants_home_tab` + `TabEntry::home` + `TabItem::permanent`; verified live — the Database home tab offers only Close Others and Close to the Right and wears no close cross |
| Shell: remove the add-new-tab button from the window strip | Completed | `TabStrip::show_new_tab`; the inner strips (editor files, database queries, browser pages) keep theirs |
| API client: a permanent home tab with the workspace logo and name | Completed | `ui::home`; the tab carries the workspace's logo and name with no method prefix, the pane shows the summary when there are collections and the what-to-do options either way; verified live on an empty workspace |
| API client: duplicate a collection or a folder with everything in it | Completed | `model::deep_copy` gives new ids throughout; one test proves a duplicated folder shares no ids with the original |
| API client: the list refreshes wherever a request is created | Completed | an `Event::Custom` on `api.collections` after every write; the writing tab skips its own |
| Terminal: permanent home tab with a dashboard of open terminals | Completed | the shell now starts on the first frame that draws a terminal, not when the tool is made, so the home tab has none; verified live — the dashboard listed 6 shells and 0 open tabs, and opening one gave a live PowerShell beside it |
| SSH: permanent home tab with a dashboard of open connections | Completed | same grid; the card carries the host |
| Database: permanent home tab with a dashboard of open connections | Completed | `hs_ui::dashboard` + `OpenTabs`; verified live — the dashboard showed 1 connection, 1 open tab and a captured thumbnail of the query pane, and clicking the card focused that tab |
| NoSQL: permanent home tab with a dashboard of open connections | Completed | same `hs_ui::dashboard` grid; the card carries the store kind and connection |
| Redis: permanent home tab with a dashboard of open connections | Completed | verified live — the permanent Redis tab shows the summary and the empty state |
| S3: permanent home tab with a dashboard of open connections | Completed | the card carries the open bucket |
| Browser: permanent first tab, no tab context menu | Completed | `has_menu` is false for home and browser tabs; the window strip's + is gone |
| Home: permanent first tab, no tab context menu | Completed | as above |
| Graphics: permanent first tab | Completed | `wants_home_tab`; the sidebar and preview pane are untouched |
| Devtools: permanent home tab with a dashboard of open tools | Completed | counts the utilities the drawer offers |
| Scratchpad: permanent home tab with a dashboard of open notes | Completed | counts the workspace's notes |
| Shell: a permanent tab is named after its activity | Completed | `Tool::set_home_tab`, told before the first frame so a tab the user never opened is named too; verified live — "Devtools" (not "Base64"), "Scratchpad" (not "New Note") and "Terminal" (not the saved shell), each with the activity's icon. Owner's report, 2026-10-17 |
| Shell: pinned tabs start to the right of the permanent tab | Completed | the strip's leading zone is the permanent tab then the pinned ones (`TabLayout::lead`), and `TabSpace::regroup` keeps the saved order to match; verified live — pinning a note moved it next to the Scratchpad tab instead of in front of it. Owner's request, 2026-10-17 |
| Shell: a dashboard home tab draws no toolbar | Completed | Save, Run and Download would aim at nothing |
| Shell: a restored layout keeps its tabs | Completed | the home flag is saved with the layout; a layout without one gains a home tab rather than turning the first saved connection or note into a dashboard (`tabspace_tests`, 1 test); verified live — the Database connection, the note and the shells all came back with their panes |
| Devtools: a utility clicked in the sidebar opens a tab of its own | Completed | the home tab reported the default utility as the item it was showing, so the click focused the dashboard; a dashboard answers no request for an item now. Owner's report, 2026-10-18; verified live - Base64 opened beside the permanent tab |
| Shell: remove the bottom dock's terminal | Completed | the Terminal activity and the editor's panel are the two places for a shell; Ctrl+` is the editor's now. Owner's request, 2026-10-18 |
| Editor: an installed language server is found without Refresh Status | Completed | PATH is read from the system at startup (`hs_core::env_path::refresh`), since the inherited value comes from an Explorer started at login; verified live - Java/jdtls read Found on a fresh start, and `C:\Libraries\jdtls\bin` is indeed only in the registry value. Owner's report, 2026-10-18 |
| Editor: Run uses the editor's terminal, not the Terminal activity | Completed | Build beside it, and "Open Terminal Here" too. Owner's report, 2026-10-18 |
| API client: drag a request or folder anywhere, including another collection | Completed | the move carries both ends and saves both files; a collection header is a drop target too, so a collapsed or empty one can be reached; the open tab follows a request that changed collection (1 test); verified live - into another collection's folder, into another collection's root, into a sibling folder, into a sibling sub-folder, a whole folder across collections, and a folder dropped into its own child left the tree untouched. Owner's report, 2026-10-18 |
| Browser: a favourites bar under the address bar, with a Settings -> Browser toggle | Completed | `tool-browser::favbar`: Chrome's click rules (current page / middle or Ctrl+click for a new page behind), folders as menus with sub-menus, cut names, overflow behind a » (1 test); verified live - the bar drew under the address row with the seeded favourites and folder, and Settings -> Browser shows the toggle. Owner's request, 2026-10-18 |
| Editor: a terminal inside the editor | Completed | `tool-editor::terminal` - a resizable panel at the foot of the pane (Ctrl+`, toolbar, palette), started in the project's folder, several terminals per panel, PowerShell 7 by default and any shell from Settings -> Editor -> Terminal (2 tests); verified live - the panel came up on `D:\Temp\javademo` with a live pwsh, a second terminal opened beside it and the dock terminal still worked (the dock's was removed on 2026-10-18 at the owner's request) |
| Editor: detect a project's language and offer its tooling | Completed | `tool-editor::project` - Maven, Gradle, Cargo, npm, Go, .NET, Python, Make, CMake, with Spring Boot read from the pom or the Gradle build file; the project's wrapper preferred over the Settings -> Editor -> Build Tools paths; Build (Ctrl+Shift+B) and Run both go to the editor terminal; a web app opens in the Browser at the address it prints (10 tests); verified live - Run on a Node project typed `npm run dev` in the editor terminal and the Browser opened http://localhost:5173 |
| Shell: a dashboard home tab is never taken over by a connection | Completed | `Tool::home_is_dashboard` keeps `unbound_tab` and the URI router off it; verified live — clicking a Database connection opened a second tab beside the permanent one instead of consuming it |
| Devtools: bandwidth and speed conversion | Completed | a Bandwidth pane with both families and a transfer-time calculator; two tests |

## Owner's requests, 2026-10-19

Raised during the Git and settings sessions that ran from 2026-10-18 late
evening into 2026-10-19 (owner's time zone, Asia/Kuala_Lumpur). The two
process rules are standing instructions, mirrored in CLAUDE.md; every other
item below was documented in the affected docs before the code was written.

| Item | State | Notes |
|---|---|---|
| Process: plan first and document a request in every affected document before coding | In effect | each request's design was written first in `docs/tools/git.md`, `docs/04-ui-design.md`, `docs/05`, `docs/06`, `docs/tools/api-client.md`, `README.md`, this file and `CHANGELOG.md` |
| Process: create a proper feature branch before editing code | In effect | `feat/m4-git-branch-management`, `feat/m4-git-interactive-rebase`, `feat/credentials-edit-and-env-export`, `feat/git-conflict-editor`; each passed CI before merging |
| Git: rename a branch and set its upstream | Completed | `write::rename_branch`/`set_upstream`, branch menu; fixture test checks the tracking branch |
| Git: remotes add/edit/remove; tags create/delete/push | Completed | `write::{add_remote,set_remote_url,remove_remote,tags,create_tag,delete_tag,push_tag}`; a protected remote or tag asks first |
| Git: merge, rebase and cherry-pick with Continue / Skip / Abort | Completed | `OperationState` detection plus the three commands; the menu follows Git's paused state |
| Git: Conventional Commit prefix picker in the commit box | Completed | `COMMIT_PREFIXES`; picking a type prepends it once, Clear removes it |
| Git: whole-file conflict resolution | Completed | Keep Ours / Keep Theirs / Mark as Resolved on a conflicted row; `resolve_conflict` rewrites and stages the file |
| Git: interactive rebase | Completed | `write::rebase_interactive` writes the todo into the git dir and runs `git rebase -i` with a sequence editor, so no editor opens; pick/squash/fixup/drop/reorder; exercised in the app (a commit dropped) |
| Git: annotated tags | Completed | a message makes `git tag -a`; `tag.gpgSign=false` so no pinentry can block the app; engine test |
| Git: line-level conflict editor | Completed | `hs-git::conflict` parses two-way and diff3 blocks (CRLF and malformed blocks kept); the main area shows Context, Ours and Theirs per block with Ours / Theirs / Both / Custom, All Ours / All Theirs, and Save and Mark Resolved; exercised in the app (Both and Custom) |
| Credentials: Action column with Reveal Secret / Edit Secret, password and key editable behind the master password | Completed | `credentials::Row` carries a password and a key secret (SSH key passphrase); reveal covers both; edit asks the master password, moves typed values into the vault and never writes plaintext. Exercised in the app: reveal showed `hunter2`, edit saved `n3w-secret` as `secret:` and the vault re-read it; the profile TOML had no plaintext |
| Environments: edit variables in place and export to Postman / Insomnia / Bruno / flat JSON, secrets omitted | Completed | `tool_api::env_export` (2 tests incl. a round-trip through the importer); Export menu on the Variables card; secret rows write an empty value and Postman keeps `type: secret`. Exercised in the app: edited and saved `api_base`, exported a Postman file with the real value |
| Scrollable sidebar lists: Home, Terminal, SSH hosts, database, NoSQL, Redis, S3, API client, Browser, Graphics, Devtools and Scratchpad | Completed | Every tool sidebar list is a ScrollArea under a fixed header; DB, Redis, NoSQL, API, editor, Home, Scratchpad, Browser and Graphics already were, and Devtools, SSH, S3, Terminal and the Settings page list now match. The Devtools list hid its last group at 900 px. See `docs/04` §1.5 |
| Terminal: a Settings page between Security and Connections with a reorderable Terminal List of CLI profiles (mysql, psql, mongosh, redis-cli, claude, opencode, custom) and PATH suggestions; each shows in the Terminal activity sidebar | Completed | `CliProfile` + `Terminal.cli` in `settings.toml [[terminal.cli]]`; arrows reorder, Add/Edit/Remove, enabled toggle, Suggestions from PATH, icons, `{{workspace}}` start folder; clicking a CLI opens a tab and types its command line. Verified live: added Test Shell (`cmd /k echo Hello from CLI`) and it ran in a new tab; added Claude Code from Suggestions (disabled) and moved it above Test Shell, the TOML order changed. P1: per-profile environment injection and icon colour |
| Documentation, screenshots and GIF for every feature | Completed | README screenshots restructured into 16 categories with a category index; new shots for the Git conflict editor and interactive rebase, credentials (table and Edit Secret), environment export, Connections, Redis, NoSQL, Regex Tester and Text Diff; Settings shots retaken on 0.44.0; Home, Editor, Terminal and Production retaken; obsolete Welcome shot removed; walkthrough GIF rebuilt from the current set; `docs/screenshots/README.md` records the fixtures and harness |

## Owner's requests, 2026-10-20 (UI/UX consistency)

| Item | State | Notes |
|---|---|---|
| Document the UI/UX consistency rules | Completed | `CLAUDE.md` Project rules and `docs/04-ui-design.md` §1.6; branch `fix/ui-ux-consistency` |
| Graphics dashboard: "What To Do" top spacing, centred cards, aligned function actions | Completed | 10 px heading offset, one-line 180×44 cards, three-column function grid with a 140 px action column |
| AI Assistant hard right in the Diagram Editor and Generator | Completed | `assistant_right` wraps the shared button in a `right_to_left` layout |
| Diagram Outline icons and section separators | Completed | Shape icons per node, arrow icon per edge, separators before Properties, Outline and Status |
| Diagram Preview / Source / Split, source on the right and live | Completed | Toolbar modes; JSON is regenerated from the document every frame in a read-only code editor |
| Generator prompt clears the old diagram and names the rejected field | Completed | `start` clears `doc`/view; repair text now carries the validator message |
| Assistant answer → Update/Generate Diagram | Completed | `can_insert` plus a labelled button; parses Archify IR or `.hdiag` JSON |
| Tab strip right margin / last close button | Completed | `STRIP_RIGHT_PAD` 14 px in `hs_ui::tabs` |
| Image and Icon Undo/Redo, Ctrl+Z / Ctrl+Y, global limit | Completed | snapshots in `GraphicsTool`, Settings → General → Behaviour → Undo Limit 1–100 |
| Diagram database cylinder and edge arrow/route/line attributes | Completed | Cylinder draws the body, full top ellipse and only the front half of the bottom ellipse; edge grid has Arrow/Route/Line |
| Offline diagram generation without an AI provider | Completed | **Offline Draft** splits the prompt into steps and chains them; tested in the app and unit-tested |
| Generator provider/model picker and Set AI Provider | Completed | Per-run Provider and Model; **Set AI Provider** opens Settings → AI Assistant (`settings:ai`) |
| More templates | Completed | CI Pipeline, Support Triage and Event Architecture added to the template gallery |
| Diagram title/frame toggle | Completed | `DiagramDoc.show_title`; layout emits a dashed title frame, properties has the checkbox |
| Frame drag, resize and selection | Completed | Outline selection and accent frame; border drag moves nodes, corner grip scales them |
| Edge label drag and line styles | Completed | `label_at`, draggable label, Solid/Dashed/Dotted painting and SVG |
| Assistant as a column | Completed | `assistant_column` uses a right `Panel`; Copy and Update/Generate at the top |
| Dashboard New Diagram/Generation enabled | Completed | Cards and function rows enable both actions |
| Assistant Markdown and inline actions | Completed | Shared panel renders headings/lists/inline code; Copy, Replace Diagram and Append JSON per answer; JSON example hint |
| Dashboard card centring | Completed | Custom painted card with a centred icon and label |
| Tab rows fit | Completed | Layout uses the narrowest row budget; `every_row_fits_inside_the_strip` test |
| Node, edge and frame colours | Completed | Outline/Fill/Text on nodes, Outline/Label on edges, Text/Outline/Fill/Text on frames; frames use their style in the canvas and SVG |
| Frame title position/alignment | Completed | Outer title and each inner frame: Top/Bottom, Left/Middle/Right |
| Add and populate inner frames | Completed | Add Frame wraps selection/all; auto-add to selected frame; dragging a node into a frame adds it |
| Edge label drag reliability | Completed | Same midpoint as painter and an accent drag indicator |
| API Add Folder toolbar | Completed | New Collection, New Folder, New Request, Import, History; folder disabled without a collection |
| Unique API names | Completed | ` 001`, ` 002` suffix for duplicate collections and folders |
| Tab text/close margin | Completed | 8 px gap before the cross |
| API sidebar Material icons | Completed |' `folder-api` for collections and `fileicons::for_folder` for folders |
| Scratchpad note reordering | Completed | Grip on unpinned rows, persisted in `notes/order.json`, pinned notes fixed on top |
| Empty separator hidden | Completed |' Separators only render when a node or edge is selected |
| Refresh every screenshot and the walkthrough GIF, README licence MIT | In-Progress | README licence MIT; five key screenshots and `hyperstack-demo.gif` refreshed, the remaining 100+ shots are still the 0.45–0.54 set |
## Owner's requests, 2026-10-19 (Graphics diagrams)

The Graphics activity becomes four tools — Image, Icon, Diagram Editor and
Diagram Generator — in two releases. Requirements and design:
[docs/tools/diagrams.md](docs/tools/diagrams.md). The Archify schema adaptation
is pinned to v3.2.0 and recorded in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
and ADR-025.

| Item | State | Notes |
|---|---|---|
| Process: plan and document this request before coding | Completed | `docs/tools/diagrams.md`, `docs/02`, `docs/03`, `docs/04`, `docs/07`, `docs/08` (ADR-025), `README.md`, this file and `CHANGELOG.md` updated 2026-10-19; branches `feat/graphics-diagram-suite` (docs) and `feat/graphics-activity-shell` (Phase 1) |
| Release 1: Graphics activity sidebar, dashboard home and Recent section | Completed | Sidebar lists Image/Icon/Diagram Editor/Diagram Generator with right-click menus; the permanent tab is a dashboard with What To Do, the function list and the screenshot grid; Recent persists and focuses a tab; verified in the running app — dashboard, two tabs and a focused card; `feat/graphics-activity-shell` |
| Release 1: separate Image and Icon tabs, settings left, preview right, Convert to Icon | Completed | Image and Icon are separate tabs; the activity sidebar keeps the functions, the picture settings moved into an in-pane panel; Convert to Icon opened an Icon tab with its own settings; verified in the running app |
| Release 1: icon colour-key transparency with eyedropper and tolerance | Completed | `Op::TransparentColor` (per-channel tolerance 0–100), the Transparency section at the top of Icon Settings, Pick on Canvas, live checkerboard preview and a removable operation; 6 Graphics tests; verified live — picked `#285AA0` on the canvas, cleared the background, the white label remained; `feat/graphics-icon-transparency` |
| Release 1: `hs-diagram` model, validation, layout, scene and PNG/SVG/PDF export | Completed | `crates/hs-diagram` with model, strict Archify-adapted IR, validation codes/limits, flow ranks and architecture grid, routing, scene, SVG, PNG (`resvg`) and PDF (`svg2pdf`); 16 tests; both templates exported and the PNGs checked visually; branch `feat/hs-diagram-core` |
| Release 1: Diagram Editor, flowchart and architecture | Completed | Implemented: Diagram tab from the sidebar, canvas with pan/zoom/grid, selection and node drag, add process/decision/note, connect mode, undo/redo, properties/outline, flow and architecture templates, save/open `.hdiag`, SVG/PNG/PDF export. Drafts autosave and close into `graphics/drafts/`; Save to Project writes `docs/diagrams/`; resize grips, Shift-click multi-select align/distribute and edge label/waypoint editing are in. Mermaid flowchart import remains for R2; verified in the running app (project save, templates, node drag, add, undo) |
| Release 1: Diagram Generator | Completed | Flowchart/architecture picker, prompt, three examples, Use Template, Generate/Stop, async `hs_core::ai` call, strict IR parse/validation, one repair round that sends the validation reasons back to the model, offline template fallback, preview, Open in Editor (writes a generated `.hdiag`) and PNG/PDF export. Verified in the app with the offline path; the real-provider path is not exercised because the test data folder has no provider |
| Release 1: AI assistant in all four tools | Completed | Image, Icon, Diagram Editor and Diagram Generator all open the shared review-only assistant; four quick actions each plus More Suggestions (the shared panel); text-only context (picture metadata/operations, diagram/outline ids and labels); verified in the app with the generator panel open |
| Release 1: Archify schema adaptation and notices | Completed | Pinned v3.2.0; `THIRD_PARTY_NOTICES.md`, the adapted schemas and the strict Rust validator are committed; About now shows the compiled-in Third-Party Notices in a collapsible section, with a test that the pinned commit, Archify and MIT text are present |
| Release 1: tests, documentation, screenshots and GIF | In-Progress | 22 shell tests, 14 Graphics tests, 16 `hs-diagram` tests and the migration test pass; the dashboard, image-tab, icon-tab, editor, generator and assistant screenshots are added; the walkthrough GIF waits for the outstanding editor work |
| R1.1: Save Diagram to a project | Completed | `Tool::project_roots` + `OpenTabs::set_projects` publish Editor roots; the Save to Project modal has Project / Subfolder (`docs/diagrams`) / File name, overwrite confirmation and the saved path; verified in the app by saving `Checkout-flow.hdiag` into a seeded project |
| R1.1: Draft autosave while typing | Completed | Debounced (1 s) hash-checked autosave into the same stable draft file the close path uses; `dirty` stays set; unit test covers the debounce |
| R1.1: Resize handles, multi-select align/distribute | Completed | Bottom-right grip; Shift-click selection; six align actions and two distribute actions; unit test for align-left |
| R1.1: Edge labels and waypoints | Completed | Click an edge to select, label field, Add/Clear Waypoints and draggable handles; Delete removes the edge; unit test |
| Release 2: ERD structured columns and sequence fragments | Completed | Entity columns named/typed with PK/FK/UQ, Add/Remove Column, edge cardinality picker, sequence fragments (`alt`/`loop`/`opt`) added/edited/removed in Properties, activation bars; tested |
'| Release 2: Fragment ranges, column order and DDL import | Completed | Editable fragment from/to, entity column move up/down, SQL `CREATE TABLE`/FK import; parser and layout tests |
| Release 2: ERD and sequence diagrams | In-Progress |' 2026-10-21: `NodeKind::Entity` table nodes, structured columns, sequence lifeline layout with participants/messages/fragments; DB-schema import remains |
| Release 2: Archify JSON import and draw.io XML import | Completed | Archify JSON through the strict IR; plain and compressed draw.io XML with vertices, edges and shape mapping; tests cover both formats and the first page. Multiple-page selection is not exposed |
| Release 2: `.diagram.json` alias and schema migration | Completed | `.diagram.json` opens/saves as an alias; schema v2 migrates v1 documents in `from_json` and writes the new marker on save, with tests |
