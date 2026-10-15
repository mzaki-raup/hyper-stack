# 02 — Feature catalogue

**Status:** draft · **Last updated:** 2026-10-10

Priorities: **P0** = must ship in v1.0 · **P1** = first releases after 1.0 ·
**P2** = planned, not scheduled · **Idea** = worth keeping, not committed.
Effort: **S** = a day or two · **M** = about a week · **L** = several weeks.
"(requested)" marks features from the original brief; everything else is a
suggestion.

## 1. Workspace shell

| Feature | Priority | Effort | Notes |
|---|---|---|---|
| Multi-tab, **multi-row** tab space that wraps instead of scrolling (requested) | P0 | M | Same behaviour as Eden Explorer's strip (reference: its `compute_tab_layout` / `tab_row_count`), written fresh; min tab width and tab gap configurable |
| Tab = one tool instance; mixed tools in one strip | P0 | M | See `Tool` trait in architecture |
| **Launcher** page on new tab: tool grid, recent items, pinned connections | P0 | S | Where the user picks what to do |
| Split panes (vertical/horizontal), drag tab into a split | P0 | M | Tab rows above, split layout below |
| Pin tabs, close others / left / right, duplicate, reorder by drag | P0 | S | Same semantics as Eden Explorer |
| **Tab groups** (saved named sets) and **workspaces** (whole layout + environment) | P0 | M | Workspace restores on launch |
| Colour-coded **environment banner** per tab and per workspace | P0 | S | Red border for production, read-only badge |
| Command palette (Ctrl+Shift+P) with tool-provided commands | P0 | M | Provider-based; includes "connect to…", "open recent…", "switch environment…" |
| Global quick-open (Ctrl+P): files, connections, saved queries, requests | P0 | M | Fuzzy ranking shared with palette |
| Bottom dock: Output, Jobs, Problems, Terminal | P0 | M | Collapsible, per-workspace |
| Left activity rail: tool icons + per-tool sidebar (project tree, schema, collections, hosts, buckets) | P0 | M | |
| Jobs panel: every background task with progress, cancel, log | P0 | M | Transfers, long queries, pushes, uploads |
| Notifications / toasts with history | P0 | S | Done: the title-bar bell opens the retained history |
| **Home tab**: pinned dashboard first in every workspace (get-started cards, workspaces, tools, recent tabs, pinned connections, keys) | P0 | S | Done 2026-10-15; replaced the Welcome tab |
| Theme **mode** System / Dark / Light with a dark and a light scheme, persisted across restarts (Windows 10 included) | P0 | S | Done 2026-10-15 (ADR-022) |
| Activity rail: tool order and optional labels under the icons; sidebar toggle and Settings at the bottom | P1 | S | Done 2026-10-15 |
| **Accessibility**: Read Aloud through the OS voice (title-bar button lists what the active tab can read), reduce motion | P1 | S | Done 2026-10-15 |
| **Snip** (tab or window to PNG and clipboard) and **Print…** (HTML export of code, Markdown, query and result, console, listing, response; opens in the browser for paper/PDF) | P1 | S | Done 2026-10-15 |
| Settings and shortcuts **export/import** as one bundle, master password required, providers merged by id | P1 | S | Done 2026-10-15 |
| Workspace settings page: rename, logo, compare environments with another workspace and copy the missing ones | P1 | S | Done 2026-10-15 |
| Settings window with categories, search, import/export | P0 | M | |
| Themes: dark/light, prebuilt, custom palette editor, live preview | P0 | M | |
| Rebindable shortcuts with a shortcuts page | P0 | S | TOML keymap |
| i18n with Fluent, OS-locale detection | P0 | S | Start with `en` |
| **Portable by default** (requested): all settings and data in `HyperStackData/` beside the executable, no registry, no `%APPDATA%`; read-only location detection with fallback; "Reset all data" = delete folder | P0 | S | See ADR-017 |
| **In-app updates** (requested): Settings → Updates shows the installed version (SemVer + git sha + build date + channel), checks the forge for a newer release (daily, switchable), shows release notes, downloads with progress, verifies size + sha256 + Ed25519-signed manifest, installs by rename-and-relaunch leaving `HyperStackData/` untouched, keeps the previous executable for a one-click revert, "install from file" for offline machines | P0 | M | See [09](09-ci-cd-and-updates.md) |
| Stable and beta update channels; "security" releases notify with a toast | P1 | S | |
| `--version`, `--self-test`, `--data-dir`, `--finish-update` command-line flags | P0 | S | CI smoke test and updater depend on them |
| Drag and drop between tools (file → editor, file → SFTP, result → request) | P1 | M | |
| Detach tab to its own OS window | P2 | L | egui viewports |
| Session audit log of commands run (local only) | P2 | M | Useful for DBAs and SREs |

## 2. Code editor (requested) — [spec](tools/editor.md)

| Feature | Priority | Effort |
|---|---|---|
| Rope-based buffer, multi-cursor, undo/redo, large-file safe | P0 | L |
| Syntax highlighting (syntect) for ~40 languages incl. SQL, YAML, TOML, Dockerfile, HCL, shell, Rust, TS/JS, Python, Go, Java, C# | P0 | M |
| Find / replace with regex, in file and in project | P0 | M |
| Project tree (local folder) with file watch | P0 | M |
| Open/save over SFTP and S3 transparently | P0 | M |
| Auto-indent, bracket matching, comment toggle, line ops, column selection | P0 | M |
| Encoding and line-ending detection and conversion | P0 | S |
| Minimap, word wrap, rulers, whitespace rendering | P1 | S |
| Markdown preview with Mermaid flowcharts and sequence diagrams rendered natively | P1 | M |
| Tree-sitter for folding, structural selection, better highlighting | P1 | L |
| Snippets, format-on-save via external formatter | P1 | M |
| Diff editor (two-way, and three-way for merges) | P1 | L |
| LSP client: completion, hover, diagnostics with a Problems panel, go to definition, references, rename, formatting; servers installed by the user, any stdio server configurable per language (done 2026-10-15, `hs-lsp`) | P1 | L |
| Hex editor mode | P2 | M |

## 3. Database client (requested) — [spec](tools/database.md)

| Feature | Priority | Effort |
|---|---|---|
| Drivers: SQLite, PostgreSQL, MySQL/MariaDB | P0 | L |
| Driver: SQL Server (tiberius) | P1 | M |
| Drivers: ClickHouse, DuckDB, Oracle (via ODBC) | P2 | M–L |
| Redis and document/NoSQL stores: see their own tools below (sections 3b, 3d) | — | — |
| Schema browser: databases, schemas, tables, views, columns, indexes, foreign keys, routines | P0 | M |
| SQL editor with keyword/table/column completion, multi-statement, run selection | P0 | M |
| Result grid: virtualised, sortable, filterable, copy as CSV/JSON/Markdown/INSERT, cell viewer for JSON/blob/long text | P0 | M |
| Inline editing with generated statements and preview before commit | P1 | M |
| Explain / explain analyze with visual plan tree | P1 | M |
| Export (CSV, JSON, SQL, Excel) and import (CSV) with column mapping | P1 | M |
| Query history and saved queries per connection, parameterised with `{{var}}` | P0 | S |
| Transactions toggle (auto-commit off), read-only connections | P0 | S |
| Destructive statement guard (UPDATE/DELETE without WHERE, DROP, TRUNCATE) naming the environment | P0 | S |
| SSH tunnel for any connection (reuses SSH tool) | P0 | M |
| ER diagram, DDL generation, schema compare, data compare | P2 | L |

## 3b. Redis cache viewer (requested) — [spec](tools/redis.md)

| Feature | Priority | Effort |
|---|---|---|
| Connections: standalone, TLS, ACL users, Sentinel, Cluster; SSH tunnel; import from `redis://` URI | P0 | M |
| Key browser: `SCAN`-based lazy tree grouped by delimiter (`user:123:session` → `user` › `123`), per-db selector, pattern filter, key count estimate | P0 | M |
| Type-aware value viewers and editors: string (JSON/text/hex/MessagePack decode), hash, list, set, sorted set, stream, HyperLogLog, bitmap | P0 | L |
| TTL display with countdown, set/persist TTL, rename, delete, copy key, duplicate | P0 | S |
| Memory usage per key (`MEMORY USAGE`), encoding, idle time, big-key finder (sampled) | P1 | M |
| Command console with completion and history (RESP2/RESP3), raw command tab | P0 | M |
| Pub/Sub subscriber and publisher tab | P1 | S |
| Stream viewer with consumer groups, pending entries, ack | P1 | M |
| Server info dashboard: memory, clients, hit rate, ops/sec, keyspace, replication, slowlog, `CLIENT LIST` | P1 | M |
| Real-time `MONITOR` tail with filter (guarded: warns about production impact) | P1 | S |
| Bulk operations: delete by pattern (SCAN + UNLINK batches, dry run first), export/import keys as JSON or RDB-like dump | P1 | M |
| Lua script runner, `EVAL` with keys/args | P2 | S |
| Guards: `FLUSHALL`/`FLUSHDB`/`KEYS *` blocked on protected environments; `SCAN` always preferred over `KEYS` | P0 | S |

## 3c. Log tail viewer (requested) — [spec](tools/logs.md)

| Feature | Priority | Effort |
|---|---|---|
| Sources: local file, remote file over SSH (`tail -F` or SFTP range reads), Docker container logs, Kubernetes pod logs, `journalctl`, Windows Event Log, command stdout | P0 (local, SSH) / P1 (others) | L |
| Live tail with follow, pause, scroll-lock indicator, "new lines" badge, auto-reconnect after rotation (`tail -F` semantics, inode change detection) | P0 | M |
| Large-file engine: open multi-GB files by index, backward paging, no full load | P0 | L |
| Filters: include/exclude text, regex, level, time range; multiple filter chips combined; highlight rules with colours | P0 | M |
| Format detection: plain, JSON lines, logfmt, syslog, Apache/Nginx combined, Java/.NET stack traces (multi-line grouping) | P0 | M |
| Structured view for JSON/logfmt: columns from fields, pin fields, expand record, copy as JSON | P0 | M |
| Level colouring (TRACE…FATAL), timestamp normalisation to local or UTC, relative time | P0 | S |
| Multi-source merge: tail several hosts/containers into one timeline, source colour tag (e.g. web-01 + web-02) | P1 | M |
| Bookmarks, "jump to time", find (Ctrl+F) with match count, context lines | P0 | S |
| Statistics: lines per level per minute sparkline, top messages, error rate | P1 | M |
| Save filtered output, copy selection, open line's file:line in editor if path detected | P1 | S |
| Alerts: pattern seen → toast / sound / badge on tab (e.g. "ERROR" appears) | P1 | S |
| Saved log sources and filter presets per environment | P0 | S |

## 3d. NoSQL / document database client (requested) — [spec](tools/nosql.md)

| Feature | Priority | Effort |
|---|---|---|
| MongoDB: connection (URI, SRV, replica set, TLS, auth sources, Atlas), SSH tunnel | P0 | M |
| MongoDB: databases › collections tree with counts, indexes, validators, stats | P0 | M |
| Document browser: paged results as tree / table (auto-flattened) / raw JSON, filter + projection + sort form, aggregation pipeline editor with stage-by-stage preview | P0 | L |
| Document editing: edit as JSON (Extended JSON v2 with type preservation), insert, duplicate, delete, bulk update via query (dry-run count), field-level edit in tree | P0 | M |
| Index management (create/drop, TTL, unique, text, compound), explain plan for find/aggregate | P1 | M |
| Schema inference: sample documents → field names, types, frequency; export as JSON Schema | P1 | M |
| Import/export: JSON, JSON lines, CSV (mongoimport-compatible), copy collection between connections/environments (streamed) | P1 | M |
| CouchDB / PouchDB-compatible: databases, documents with `_rev` conflict handling, Mango queries, views (design docs, map/reduce), attachments, replication status | P0 (browse/edit) / P1 (views, replication) | L |
| Elasticsearch / OpenSearch: indices, mappings, aliases, Query DSL editor with completion, hits as table/tree, `_cat` dashboards, index lifecycle | P1 | L |
| DynamoDB (AWS and DynamoDB Local): tables, scan/query builder with key conditions, item editor, GSI/LSI listing, PartiQL console | P1 | L |
| Cassandra / ScyllaDB: keyspaces, tables, CQL console with completion, paged results | P2 | L |
| Firestore (service account), Cosmos DB (Mongo/SQL APIs), Neo4j (Cypher) | Idea | L |
| Common: saved queries per connection, history, result export, "send to API client / editor", protected-environment guards (no `drop`, `deleteMany({})`, `DELETE /_all`) | P0 | S–M |

## 4. API client (requested) — [spec](tools/api-client.md)

| Feature | Priority | Effort |
|---|---|---|
| REST: methods, params, headers, body (JSON/form/multipart/raw/binary), auth (basic, bearer, API key, OAuth2 flows, AWS SigV4, digest) | P0 | L |
| Collections and folders saved as files; environments and variables with `{{var}}` | P0 | M |
| Response viewer: pretty/raw/preview, headers, cookies, timing waterfall, size | P0 | M |
| History, duplicate, "copy as curl", import from curl | P0 | S |
| Import Postman v2.1, Insomnia, Bruno, OpenAPI 3; export OpenAPI/Postman | P0 | M | Postman v2.1 collections and environments import, Postman export: done 2026-10-15 |
| JavaScript pre-request and test scripts with the `pm` API, collection libraries, snippets | P1 | L | Done 2026-10-15 (ADR-020) |
| Tests/assertions per request (status, JSON path, header, time) and pre-request scripts in an embedded scripting language (Rhai) | P1 | L |
| Collection runner with data files and a report | P1 | M |
| WebSocket and Server-Sent Events client | P1 | M |
| GraphQL with schema introspection and query completion | P1 | M |
| gRPC with reflection or .proto import | P2 | L |
| Mock server from a collection or OpenAPI spec | P2 | M |
| Cookie jar, proxy settings, client certificates, TLS inspection | P1 | M |

## 5. SSH + SFTP (requested) — [spec](tools/ssh-sftp.md)

| Feature | Priority | Effort |
|---|---|---|
| Host profiles: password, key, agent, jump hosts, import `~/.ssh/config` | P0 | M |
| Shell tab with full terminal emulation (reuse `hs-term`) | P0 | M |
| SFTP pane: dual-pane local/remote, drag transfer, queue with progress, resume | P0 | L |
| Edit remote file in editor, save back; permissions dialog (chmod/chown) | P0 | M |
| Host key verification and known hosts (OpenSSH format, stored in `HyperStackData/`) | P0 | S |
| Port forwarding (local, remote, dynamic/SOCKS) managed in Jobs | P1 | M |
| Multiple shells per host, broadcast input to several hosts | P1 | M |
| Server monitor widget: CPU, memory, disk, load, top processes via SSH | P1 | M |
| Snippets / saved commands, sudo password prompt handling | P1 | S |
| Mosh / serial / telnet | Idea | M |

## 6. S3 browser (requested) — [spec](tools/s3.md)

| Feature | Priority | Effort |
|---|---|---|
| Profiles for AWS, MinIO, Cloudflare R2, Backblaze B2, DigitalOcean Spaces, Wasabi (any SigV4 endpoint); import AWS credentials file | P0 | S |
| Bucket list, prefix navigation, virtualised listing with paging, search by prefix | P0 | M |
| Upload/download (multipart, resume), delete, copy/move, rename, folder create | P0 | M |
| Object details: metadata, tags, storage class, ETag, versions | P1 | M |
| Presigned URL generator, public URL, copy ARN/URI | P0 | S |
| Edit small objects in the editor and save back | P0 | S |
| Sync folder ↔ prefix (dry run first), bucket policy / CORS viewer | P1 | M |
| Lifecycle rules viewer, bucket size estimate | P2 | M |
| Azure Blob and Google Cloud Storage drivers behind the same object-store trait | P2 | L |

## 7. Git (requested) — [spec](tools/git.md)

| Feature | Priority | Effort |
|---|---|---|
| Status, stage/unstage (file and hunk), commit with message templates and sign-off | P0 | M |
| Diff viewer (side by side / inline) using the editor engine | P0 | M |
| Branches: create, checkout, rename, delete; stash list | P0 | M |
| Fetch, pull, push with upstream tracking and credential helper integration | P0 | M |
| Merge with conflict list; conflict resolution in the three-way editor | P1 | L |
| Log graph with search, blame, file history | P1 | M |
| Rebase (interactive list), cherry-pick, revert, tags | P1 | M |
| Submodules, worktrees, LFS awareness | P2 | M |
| GitHub / GitLab / Forgejo integration: PRs, issues, CI status (token in the vault) | P2 | L |

## 8. Terminal (suggested) — [spec](tools/terminal.md)

| Feature | Priority | Effort |
|---|---|---|
| Local shells: PowerShell, cmd, Git Bash, WSL, bash/zsh/fish; profiles | P0 | M |
| Terminal grid shared with SSH (alacritty_terminal), true colour, scrollback, search, selection, clickable links | P0 | M |
| Tabs and splits, "open terminal here" from project tree / SFTP / Git | P0 | S |
| Task runner: detect `package.json` scripts, `Cargo.toml`, `Makefile`, `justfile`, `docker-compose.yml` and run with one click | P1 | M |
| Command history search across sessions | P2 | S |

## 9. Containers (suggested) — [spec](tools/containers.md)

| Feature | Priority | Effort |
|---|---|---|
| Docker/Podman: containers, images, volumes, networks; start/stop/logs/exec/inspect | P1 | L |
| Compose projects: up/down/logs per service | P1 | M |
| Kubernetes: contexts, namespaces, pods, logs, exec, port-forward, describe, apply YAML | P2 | L |
| Remote Docker over SSH | P1 | S |

## 9b. Browser (requested) — [spec](tools/browser.md)

| Feature | Priority | Effort |
|---|---|---|
| Multi-row page tabs, address bar with search fallback, back/forward/reload/home, zoom | P1 | M |
| Ad and tracker blocking (navigation blocklist + cosmetic hiding), per-page JavaScript switch, forced dark rendering | P1 | M |
| Inspect (developer tools), Print, Clear Browsing Data | P1 | S |
| Pages restored per workspace; Shift+click on a terminal link opens it here | P1 | S |
| Platforms other than Windows (webkit2gtk / WKWebView through `wry`) | P2 | M |

## 9c. Graphics (requested) — [spec](tools/graphics.md)

| Feature | Priority | Effort |
|---|---|---|
| Open pictures from the editor tree or a dialog; view with zoom on a checkerboard | P1 | S |
| Resize, scale, extend canvas, crop, rotate, flip; brightness, contrast, hue, saturation, grayscale, invert, blur, sharpen — as a removable operation list | P1 | M |
| Convert between PNG, JPEG, WebP, BMP, GIF, ICO; multi-size ICO export; icon frame extraction | P1 | S |
| Review-only AI assistant on the picture's metadata and operations | P1 | S |
| AI enhancement/outpainting through a provider | P2 | M |

## 10. Devtools drawer (suggested) — [spec](tools/devtools.md)

| Utility | Priority | Effort |
|---|---|---|
| JSON / YAML / TOML / XML format, validate, convert, query (JSONPath / jq-like) | P0 | M |
| JWT decode and verify, Base64 / URL / HTML encode, hex | P0 | S |
| Regex tester with highlighting and explanation | P0 | S |
| Cron expression builder and next-runs preview | P0 | S |
| UUID / ULID / random / password generator, hash (MD5/SHA/CRC/BLAKE3) | P0 | S |
| Timestamp / timezone converter, duration calculator | P0 | S |
| Text diff (reuses editor diff), Markdown preview, number base converter | P1 | S |
| Colour picker/converter, fake data generator, SQL formatter, case converter | P1 | S |
| Network tools: DNS lookup, TCP/HTTP ping, port check, whois, TLS certificate viewer | P1 | M |
| Local process and port viewer (what is listening on :8080) | P1 | M |
| Snippet manager (code, SQL, shell) with tags | P1 | M |
| `.env` file editor with secret masking and environment linking | P1 | S |
| Scratchpad notes (Markdown) per workspace — done 2026-10-15: notes collection, pinning, dual pane, AI assistant ([tools/scratchpad.md](tools/scratchpad.md)) | P1 | S |

## 11. Shared services (enablers)

| Feature | Priority | Effort |
|---|---|---|
| Connection profiles with kinds (db, ssh, s3, api base, docker, k8s), tags, colour, folders | P0 | M |
| Environments: named sets of variables + default connection per kind; `{{var}}` substitution everywhere | P0 | M |
| Secrets in an encrypted vault file beside the executable (master password), referenced by id from profiles; OS keyring as an opt-in alternative for installed, non-portable use | P0 | M |
| Import/export of everything except secrets as a folder of files; put `.hyperstack/` in a repo to share with a team | P0 | S |
| Global search across saved queries, requests, hosts, buckets, snippets | P1 | M |
| Plugin API (WASM, limited to devtools and importers first) | P2 | L |
| Optional AI assistant (bring your own service: 1min.AI, OpenAI-compatible, local LM Studio/Ollama): editor chat, review, edit blocks applied as diffs, Cascade rounds (ADR-019); review-only assistant in terminal, SSH, SQL and S3 uses explicitly shared context and never executes actions | P1 | M |
