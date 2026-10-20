<p align="center"><img src="assets/icon/hyperstack-128.png" width="128" height="128" alt="HyperStack icon: three stacked layers in purple, blue and gold"></p>

# HyperStack

**One workspace for everyone who ships software.**

<p align="center">
  <a href="https://github.com/mzaki-raup/hyper-stack/releases"><img src="https://img.shields.io/github/downloads/mzaki-raup/hyper-stack/total?style=for-the-badge&color=5F4B87&labelColor=14161A" alt="Downloads" /></a>
  <a href="https://github.com/mzaki-raup/hyper-stack/releases/latest"><img src="https://img.shields.io/github/v/release/mzaki-raup/hyper-stack?style=for-the-badge&color=5F4B87&labelColor=14161A" alt="Latest release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/mzaki-raup/hyper-stack?style=for-the-badge&color=5F4B87&labelColor=14161A" alt="License" /></a>
  <a href="https://github.com/mzaki-raup/hyper-stack/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/mzaki-raup/hyper-stack/ci.yml?branch=main&label=build&style=for-the-badge&color=5F4B87&labelColor=14161A" alt="Build (CI on main)" /></a>
  <a href="https://github.com/mzaki-raup/hyper-stack/stargazers"><img src="https://img.shields.io/github/stars/mzaki-raup/hyper-stack?style=for-the-badge&color=5F4B87&labelColor=14161A" alt="Stars" /></a>
</p>

<p align="center"><img src="docs/screenshots/hyperstack-demo.gif" alt="HyperStack walkthrough: unlock the vault, open a terminal from the launcher, search the output and Ctrl+click a compiler location to open it in the editor, preview Markdown with a Mermaid diagram, connect over SSH and start a port-forward tunnel, browse an S3 bucket and create a presigned URL, then switch to the protected Production environment" width="960"></p>

HyperStack is a desktop application for full-stack developers, DevOps engineers,
database administrators, testers and integration engineers. It brings the tools
those people switch between all day — code editor, SQL database client, NoSQL
document-store client, Redis cache viewer, API client, SSH/SFTP, S3 browser,
Git, terminal, log tail viewer and a drawer of developer utilities — into a
single window organised as a **multi-row, multi-tab workspace**, where every tool
shares the same connection profiles, environments, secrets store, command palette
and theme.

> **Status:** Phase 0 (foundation), M1 and M2 implemented, M3 under way and
> the first parts of M4 and M5 landed (2026-10-15): the workspace shell with
> workspaces as projects (create with a logo, clone, rename, switch, delete),
> a pinned **Home** dashboard and a pinned Settings tab in every workspace,
> portable data folder, vault, environments, theme modes (System / Dark /
> Light with a scheme for each), accessibility (Read Aloud), settings
> import/export, a terminal with scrollback search and Ctrl+click links, a
> code editor holding several projects (each with its own row of file tabs,
> the file's full path under them, Run with F5, Git status, diffs and
> history) and Markdown preview with native Mermaid diagrams, an SSH + SFTP
> tool with a synced dual pane, port forwarding and a file pane that copies
> files and folders to and from the system clipboard, an S3 browser for AWS
> and S3-compatible stores, a SQL client for PostgreSQL, MySQL/MariaDB and
> SQLite with a cell viewer that previews pictures and documents, a **Redis**
> key browser with type-aware viewers, a console and guards, a **NoSQL**
> client for MongoDB and CouchDB, an **API
> client** with collections, JavaScript scripts (`pm` API) and Postman
> import/export, **Devtools** (Base64, URL encoding, hashes, JWT, Firebase
> tokens, Jasypt, PKI messages; one tab per utility, inputs persisted, secrets
> saved to the environment vault and used as `{{variables}}`), a **Graphics** tool (convert, resize, extend,
> adjust, icons), an embedded **Browser** (WebView2, ad and script> blocking, forced dark, inspect, print), **Snip** and **Print…** for every
> tab, an optional AI assistant in the editor (Anthropic, Gemini, Azure
> OpenAI, AWS Bedrock, 1min.AI, LM Studio, Ollama or any OpenAI-compatible
> service) and a review-only assistant in
> Terminal, SSH, SQL, Redis, NoSQL, S3, API client and Graphics whose code blocks can be
> inserted but never run, and the in-app updater with signed releases.
> The Git write side and logs follow in the next milestones
> per [docs/07-roadmap.md](docs/07-roadmap.md). Sibling project:
> `../eden-explorer` (Rust + egui file manager) serves as a **reference
> only**: HyperStack studies how it solved the multi-row tab strip,
> terminal, SSH/SFTP, S3 and Git status, then writes its own
> implementation. No code is copied or shared.

The Graphics activity is being expanded (owner's request, 2026-10-19) into four
tools — Image, Icon, Diagram Editor and Diagram Generator — in two releases.
The first Release 1 phase landed on 2026-10-19: the activity sidebar, the
dashboard with Recent and screenshots, the separate Image and Icon tabs, and
Convert to Icon. The flowchart/architecture editor and the prompt generator
follow in the same release, with an AI assistant in every tool. Release 2 adds
ERD and sequence diagrams plus Archify JSON and draw.io XML import. The
requirements and design are in [docs/tools/diagrams.md](docs/tools/diagrams.md);
the pinned Archify schema adaptation is recorded in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

---

## Screenshots

Every feature built today, captured from a real `HyperStack` build on
Windows 11 (HyperStack Dark, activity rail labels on). See
[docs/screenshots/](docs/screenshots/README.md) for how they were made and
the fixtures behind them.

| Category | What the shots cover |
|---|---|
| [Shell and workspaces](#shell-and-workspaces) | Home, launcher, palette, tabs, split, dock, workspaces, environments |
| [Vault and connections](#vault-and-connections) | First-run vault, unlock, profiles and secrets |
| [Editor](#editor) | Projects, files, Markdown and Mermaid, LSP, debugger, AI assistant |
| [Git](#git) | Tree, Source Control, history, tags, prefixes, conflict editor, interactive rebase |
| [Terminal](#terminal) | Shells, search, links |
| [SSH and SFTP](#ssh-and-sftp) | Host keys, dual pane, tunnels, remote editing |
| [S3](#s3-and-s3-compatible-storage) | Buckets, profiles, presigned URLs, guards |
| [SQL databases](#sql-databases-postgresql-mysql--mariadb-sqlite) | Browser, completion, results, transactions, guards |
| [Redis](#redis) | Key tree, type viewers, console, info |
| [NoSQL](#nosql-mongodb-and-couchdb) | Containers, find, aggregate, indexes, documents |
| [API client](#api-client) | Requests, scripts, assistant |
| [Devtools](#devtools) | JWT, hashes, Jasypt, PKI, regex, diff, Base64, URL, YAML, generators, cron, units |
| [Graphics](#graphics) | Activity dashboard, image and icon tabs, operations, crop |
| [Scratchpad and assistant](#scratchpad-and-assistant) | Notes, pinning, dictation |
| [Browser](#browser) | WebView2 pages, favourites, passwords |
| [Settings](#settings) | Every page |

### Shell and workspaces

| | |
|---|---|
| ![Home tab](docs/screenshots/home.png) | ![Launcher](docs/screenshots/launcher.png) |
| **Home** — the pinned first tab of every workspace: Get Started cards, the workspace switcher and the What do you want to do? tool grid; recent and pinned items live in the sidebar | **Launcher** (Ctrl+T) — tools by category plus pinned connections, filtered as you type |
| ![Command palette](docs/screenshots/command-palette.png) | ![Multi-row tab strip](docs/screenshots/tabs-multi-row.png) |
| **Command palette** (Ctrl+Shift+P) — every action, fuzzy-matched, with its shortcut | **Multi-row tab strip** — tabs wrap onto more rows instead of scrolling |
| ![Split view](docs/screenshots/split-scratchpad.png) | ![Bottom dock](docs/screenshots/dock.png) |
| **Split panes** (Ctrl+\) — two Scratchpad notes side by side, each with source and preview | **Bottom dock** (Ctrl+J) — Output, Jobs and Problems |
| ![Workspace menu](docs/screenshots/workspace-menu.png) | ![New workspace](docs/screenshots/workspace-new.png) |
| **Workspace switcher** in the title bar, with the selected workspace logo | **New workspace** — name, logo, and copy environments/connections from another |
| ![Environment switcher](docs/screenshots/environment-switcher.png) | ![Production environment](docs/screenshots/environment-production.png) |
| **Environment switcher** — colour-coded, protected ones marked | **Protected Production** — red banner and frame on every tab |

Every activity opens on a **permanent tab of its own** - named after the
activity, with its icon, and it cannot be closed, pinned or duplicated.
For Terminal, SSH, Database, NoSQL, Redis, S3, Devtools and the Scratchpad
that tab is a dashboard: a short summary of the activity and a grid of
what it has open, each card carrying the last picture taken of that pane,
and clicking one goes to the tab. Opening a connection, a note or a shell
always makes a tab beside it. The API client's permanent tab is the
workspace's instead, with its logo and name. Pinning a tab puts it
directly after the permanent one.

### Vault and connections

| | |
|---|---|
| ![Create vault](docs/screenshots/vault-create.png) | ![Unlock vault](docs/screenshots/vault-unlock.png) |
| **Create the vault** on first run (Argon2id + XChaCha20-Poly1305) | **Unlock** on start; the title-bar padlock locks it again |
| ![SSH connection form](docs/screenshots/connection-ssh-form.png) | ![Saved connection](docs/screenshots/connection-saved.png) |
| **Connection profile** (kind SSH) — host, user, start folder, auth | **Saved** — every tool sees the profile at once; typed secrets go to the vault and the profile keeps a `secret:` reference |

### Editor

| | |
|---|---|
| ![Editor](docs/screenshots/editor.png) | ![Markdown preview](docs/screenshots/editor-markdown-preview.png) |
| **Editor** — project tree with file icons, syntax highlighting, status bar (indent, encoding, EOL, language) | **Markdown preview** side by side (Ctrl+Shift+M) |
| ![Find and replace](docs/screenshots/editor-find-replace.png) | ![Mermaid flowchart](docs/screenshots/editor-mermaid-flowchart.png) |
| **Find and replace** — case, whole word and regex, match count | **Mermaid flowcharts** drawn natively in the preview — subgraphs, shapes, wrapped wide ranks |
| ![Several projects](docs/screenshots/editor-projects.png) | ![AI model picker](docs/screenshots/editor-ai-models.png) |
| **Projects as tabs** — each project root has one window tab; opening the same root again focuses it. Inside it, files are a second strip of tabs; both strips wrap onto more rows instead of hiding tabs. Files from outside the projects collect in an **Other Files** tab | **Model picker** — the models the provider lists (LM Studio: every downloaded model; 1min.AI: a built-in list), or type any other id |
| ![Mermaid sequence diagram](docs/screenshots/editor-mermaid-sequence.png) | ![AI review](docs/screenshots/editor-ai-review.png) |
| **Mermaid sequence diagrams** — participants, messages, notes, loop/alt blocks | **AI Assistant** (Ctrl+Shift+I) — chat about the file or the selection; Explain, Review, Improve, Add Comments; answers are review-only, with Insert for a code block and nothing run automatically |
| ![Completion from rust-analyzer](docs/screenshots/editor-lsp-completion.png) | ![Problems panel](docs/screenshots/editor-lsp-problems.png) |
| **Language server features** — completion as you type with documentation, hover, go to definition, references, rename and format through any installed LSP server (Settings → Editor → Language Servers shows which are found) | **Problems panel** (Ctrl+Shift+J) — diagnostics as underlines in the text, listed with file and line; the status bar counts errors and warnings |
| ![Debugger paused at a breakpoint](docs/screenshots/editor-debug.png) | ![Call stack](docs/screenshots/editor-debug-stack.png) |
| **Debugger** — breakpoints in the gutter (F9), the stopped line with an arrow, Continue/Step controls (F5, F10, F11), and the Variables tab of the debug panel; adapters (CodeLLDB, lldb-dap, debugpy, delve, netcoredbg) are installed by you, configurations come from `.hyperstack/debug.json`, `.vscode/launch.json` or are generated | **Call Stack** — frames of the paused thread; clicking one shows its variables and line |
| ![AI Cascade](docs/screenshots/editor-ai-cascade.png) | |
| **Cascade** — toggle it beside the scope to keep the assistant working on the updated file, one reviewable step at a time | **Terminal in the editor** (Ctrl+`) — a shell at the foot of the pane, started in the project's folder, several side by side; Run (F5) and Build go there, and a web app opens in the Browser at the address it prints |

### Git

| | |
|---|---|
| ![Git in the project tree](docs/screenshots/editor-git-tree.png) | ![Git diff](docs/screenshots/editor-git-diff.png) |
| **Git in the editor** — the branch on the project, change letters and colours in the tree, change markers in the gutter as you type | **Source Control** (Ctrl+Shift+G) — staged, changed and untracked files; diffs inline or side by side; Up/Down steps through them |
| ![Git history](docs/screenshots/editor-git-history.png) | |
| **History** (Ctrl+Shift+H) — commits with branch and tag chips; each commit's message, files and diffs | **Commit, push, pull, stash, branches, tags** — stage and unstage from the list, discard with a question, commit (and push) with a Conventional Commit prefix, fetch/pull/push as jobs, stashes, branches, remotes and lightweight or annotated tags; a protected remote asks before a push and never gets a force |
| ![Conflict editor](docs/screenshots/editor-git-conflict.png) | ![Interactive rebase](docs/screenshots/editor-git-rebase.png) |
| **Conflict editor** — an unmerged file opens block by block: Ours and Theirs under their marker labels, then Ours / Theirs / Both / a typed result per block; **Save and Mark Resolved** writes the file without markers and stages it | **Interactive Rebase** — commits oldest first with Pick / Squash / Fixup / Drop and move arrows; merge, rebase and cherry-pick offer Continue / Skip / Abort when Git pauses |
| ![Tags](docs/screenshots/editor-git-tags.png) | ![Commit prefix](docs/screenshots/editor-git-prefix.png) |
| **Tags** - lightweight or annotated, created from their own menu and pushed or deleted per tag | **Commit prefixes** - pick a Conventional Commit type; it is inserted once with its meaning, and Clear Prefix removes it |

### Terminal

| | |
|---|---|
| ![Terminal](docs/screenshots/terminal.png) | ![Terminal search](docs/screenshots/terminal-search.png) |
| **Terminal** — shell profiles in the sidebar, colours, restart/clear, opened here from the editor with "Open Terminal Here" | **Search** (Ctrl+Shift+F) — plain or regex, match case, every hit highlighted, "n of m" |
| ![Shell list with Nushell](docs/screenshots/terminal-shells.png) | ![Ctrl+click a file location](docs/screenshots/terminal-links.png) |
| **Shell list** - PowerShell 7, Command Prompt, Nushell and Git Bash, ordered by preference | **Links** - hold Ctrl to underline a URL or `path:line:col`; Ctrl+click opens it in the browser or the editor |
| ![CLI tools](docs/screenshots/terminal-cli.png) | |
| **CLI tools** (Settings → Terminal) - a profile such as `cmd /k echo Hello from CLI`, `mysql`, `psql`, `claude` or `opencode` opens a terminal tab and types its command line once the shell is ready | |

### SSH and SFTP

| | |
|---|---|
| ![Host key prompt](docs/screenshots/ssh-host-key.png) | ![SSH with SFTP pane](docs/screenshots/ssh-sftp.png) |
| **Host key check** — new keys are shown by fingerprint before trusting | **Dual pane** — remote files (SFTP) on the left, the shell on the right |
| ![File pane follows the shell](docs/screenshots/ssh-follow-shell-folder.png) | ![Remote file in the editor](docs/screenshots/ssh-remote-edit.png) |
| **Follow Shell Folder** — `cd` in the shell moves the file pane | **Edit remote files** — double-click downloads into the editor; saving uploads |
| ![SSH tunnels](docs/screenshots/ssh-tunnels.png) | |
| **Tunnels** — local port forwards (`ssh -L`) with live traffic; New Tunnel form in the sidebar | **Clipboard** — Copy puts a remote entry (a folder with everything in it) on the system clipboard; paste uploads what the clipboard holds (Windows) |

### S3 and S3-compatible storage

| | |
|---|---|
| ![S3 browser](docs/screenshots/s3-browser.png) | ![S3 profile](docs/screenshots/connection-s3-form.png) |
| **Browser** — profiles and buckets in the sidebar, breadcrumb, size / modified / class / ETag, Load More | **Profile** — provider presets (AWS, MinIO, R2, B2, Spaces, Wasabi), keys stored in the vault |
| ![Import AWS profiles](docs/screenshots/s3-import-aws.png) | ![Presigned URL](docs/screenshots/s3-presign.png) |
| **Import** profiles from `~/.aws/credentials` and `~/.aws/config` | **Presigned URLs** — GET or PUT, 15 minutes to 7 days |
| ![Guarded delete](docs/screenshots/s3-delete-guard.png) | ![Edit an object](docs/screenshots/s3-edit.png) |
| **Guarded delete** — prefixes and protected environments need the bucket name typed; shows versioning | **Edit objects** — double-click opens it in the editor; saving puts it back |

### SQL databases (PostgreSQL, MySQL / MariaDB, SQLite)

<p align="center"><img src="docs/screenshots/database-walkthrough.gif" alt="Database tool walkthrough: expand the schema tree and open a table's rows, write a join with completion, run it, run a script whose last statement fails with the error underlined in the editor, then on the protected Production connection see a DELETE refused, allow writes, and get asked to type the table name" width="960"></p>

| | |
|---|---|
| ![Schema browser and rows](docs/screenshots/db-browse.png) | ![Completion](docs/screenshots/db-completion.png) |
| **Schema browser** — schemas, tables, views and columns with types, indexes (functions and PK/FK/NOT NULL badges where the engine reports them); double-click a table for its rows | **Completion** from the schema cache — tables after FROM/JOIN, columns after an alias |
| ![Query and result grid](docs/screenshots/db-query.png) | ![Script with an error](docs/screenshots/db-script-error.png) |
| **Result grid** — streamed rows, NULL shown as NULL, numbers right-aligned, sort, filter, Copy As, Export | **Scripts** — one result tab per statement; the failing one is selected and the server's error position underlined |
| ![Refused on a protected environment](docs/screenshots/db-refused.png) | ![Guarded statement](docs/screenshots/db-guard.png) |
| **Protected environments** open read-only — writes are refused until **Allow Writes** | **Guards** — UPDATE/DELETE without WHERE, DROP, TRUNCATE ask first; on production the table name must be typed |
| ![Manual commit](docs/screenshots/db-manual-tx.png) | ![Cell viewer](docs/screenshots/db-cell-viewer.png) |
| **Transactions** — Auto-Commit or Manual Commit with Commit / Rollback and an open-transaction badge | **Cell viewer** — JSON pretty-printed, binary as a hex dump |
| ![DB connection form](docs/screenshots/connection-db-form.png) | |
| **Connection** — paste a `postgres://` / `mysql://` / `sqlite:` URL; the password goes into the vault; TLS modes and an SSH tunnel through any SSH profile | |

### Redis

| | |
|---|---|
| ![Redis key tree and hash viewer](docs/screenshots/redis-browser.png) | |
| **Keys** — SCAN-backed tree grouped by prefix with each key's type, a filter, and a type-aware viewer: hashes, strings, lists, sets, sorted sets and streams, with TTL and inline editing behind a confirmation. **Console** runs guarded commands; **Info** shows the server. Protected environments default to read-only | |
| ![Console](docs/screenshots/redis-console.png) | ![Info](docs/screenshots/redis-info.png) |
| **Console** - raw commands with history and completion, numbered replies and guards on a protected environment | **Info** - server version, build, process, listeners and keyspace |

### NoSQL (MongoDB and CouchDB)

| | |
|---|---|
| ![MongoDB find view](docs/screenshots/nosql-browser.png) | |
| **Containers** — databases and collections (MongoDB) or databases and document counts (CouchDB); a collection opens in **Find** with filter, projection and sort, the documents as a table and a JSON editor with delete/update guards; **Aggregate** and **Indexes** are beside it | |
| ![Aggregate](docs/screenshots/nosql-aggregate.png) | |
| **Aggregate** - a JSON pipeline with a page limit, previewed as a table or JSON; **Indexes** lists each collection's indexes | |

### API client

| | |
|---|---|
| ![API client](docs/screenshots/api-client.png) | ![API client scripts](docs/screenshots/api-client-scripts.png) |
| **Requests** — method, URL with `{{variables}}`, params, headers, body (JSON, text, XML, form, multipart, binary), auth (inherit, basic, bearer, API key); response as pretty, raw or preview with headers, cookies, tests and console; Copy as curl, Import curl; history | **Scripts** — pre-request and post-response JavaScript with the `pm` API (`pm.environment`, `pm.globals`, `pm.collectionVariables`, `pm.request`, `pm.response`, `pm.test`, `pm.expect`, `console`), snippets, collection libraries; Postman v2.1 collections and environments import and export |
| ![API client AI assistant](docs/screenshots/api-client-ai.png) | |
| **Assistant that fills the client** — ask for a collection, a request, headers, a body, auth, scripts or docs; the answer's `hs-api` block gets **Apply**, which edits the open request and the collections exactly as the sidebar would. Nothing is sent until you press Send | |

Collections are JSON files in the workspace; variables resolve through the
collection, the active environment (vault secrets only when sending) and
globals. A non-GET request to a protected environment asks first.

### Devtools

| | |
|---|---|
| ![JWT](docs/screenshots/devtools-jwt.png) | ![Hashes](docs/screenshots/devtools-hashes.png) |
| **JWT** — decode with claim explanations and expiry, verify with a secret or public key, sign for testing (HS256/384/512, RS256/384/512, ES256); the secret can be saved to the environment (vault) and the `{{reference}}` peeked | **Hashes** — MD5, SHA-1/2/3, BLAKE3, CRC32 of text or a file, HMAC, hex or Base64, compare field |
| ![Jasypt](docs/screenshots/devtools-jasypt.png) | ![PKI messages](docs/screenshots/devtools-pki.png) |
| **Jasypt** — `PBEWithMD5AndDES` and `PBEWITHHMACSHA512ANDAES_256` property encryption with jasypt's salt/IV layout and `ENC(…)`, plus the legacy DES/AES/MD5 DataEncryptor | **PKI messages** — sign with RSA and encrypt with AES-256-GCM under a shared key derived from a device id, then decrypt and verify, for both request and response flows |
| ![Regex Tester](docs/screenshots/devtools-regex.png) | ![Text Diff](docs/screenshots/devtools-diff.png) |
| **Regex Tester** — a pattern with flags, every match with its groups and positions, and the text with matches marked; the same engine as the app's filters | **Text Diff** — two texts side by side, changed lines marked, whitespace-insensitive mode, copy as a unified diff |
| ![Base64](docs/screenshots/devtools-base64.png) | ![URL](docs/screenshots/devtools-url.png) |
| **Base64** - auto-detected text, pictures and documents, data URIs and Basic auth headers; the decoded text with a hex dump | **URL** - Component, Full URL or Form encoding, a parts table and strict mode |
| ![YAML](docs/screenshots/devtools-yaml.png) | ![Generators](docs/screenshots/devtools-generators.png) |
| **YAML** - check with line-numbered problems and convert to and from Java `.properties` | **Generators** - UUID v4/v7/Nil, numbers, passwords and passphrases with a seed and entropy |
| ![Cron](docs/screenshots/devtools-cron.png) | ![Unit conversion](docs/screenshots/devtools-units.png) |
| **Cron** - build a schedule and read it in Unix, Spring and Quartz dialects, with an expression checker | **Units** - time, epochs, number bases, data sizes, durations and bandwidth, with every unit listed |

Also **TOML** and **XML** (check, format, convert) and **Firebase tokens**;
keys typed into a utility are never saved.

### Graphics

| | |
|---|---|
| ![Graphics activity](docs/screenshots/graphics-dashboard.png) | ![Image tab](docs/screenshots/graphics-image-tab.png) |
| **Graphics activity** — the sidebar lists Image, Icon, Diagram Editor and Diagram Generator plus a scrollable Recent section; the permanent first tab is a dashboard with “What To Do”, the function list and a screenshot grid of the open Graphics tabs | **Image tab** — the picture keeps the tab, the operations and settings sit on the left and the preview on the right; **Convert to Icon** opens the result in a new Icon tab |
| ![Icon tab](docs/screenshots/graphics-icon-tab.png) | ![Crop placeholder](docs/screenshots/graphics-crop.png) |
| **Icon tab** — the same operations plus ICO frame selection, sizes and colour-key transparency; here the blue background was picked on the canvas and cleared, leaving the label | **Crop placeholder** — Select on Canvas shows a rectangle with grips; move it, resize it by an edge or corner, or draw a new one, and the X/Y/W/H boxes follow |
| ![Diagram Editor](docs/screenshots/graphics-diagram-editor.png) | ![Architecture in the editor](docs/screenshots/graphics-diagram-architecture-editor.png) |
| **Diagram Editor** — the flowchart template on the canvas with the Diagram panel, Outline, shape picker and undo/redo; nodes can be selected, moved and connected, and the same scene exports as SVG, PNG or PDF | **Architecture template** — the grid with a VPC boundary, database cylinder and labelled async links; the properties panel shows the selected component |
| ![Diagram Generator](docs/screenshots/graphics-diagram-generator.png) | ![Rendered flowchart](docs/screenshots/graphics-diagram-flow.png) |
| **Diagram Generator** — type picker, prompt, examples, Use Template and Generate; with no AI provider the offline template is a real result and the message says so | **Same scene, exported** — the generator’s preview is the editor’s scene and the SVG/PNG/PDF writer, so the export matches the screen |
| ![Save to Project](docs/screenshots/graphics-save-project.png) | ![Edge properties](docs/screenshots/graphics-diagram-edge.png) |
| **Save to Project** — pick an open Editor project, keep `docs/diagrams`, name the file and confirm an overwrite | **Edge editing** — select an edge from the Outline to edit its label, add or clear waypoints and delete it |

**Implemented (2026-10-20):** the activity shell, the dashboard and Recent
list, the separate Image and Icon tabs, Convert to Icon and icon colour-key
transparency. The `hs-diagram` core is in place: the `.hdiag` model and strict
Archify-adapted IR, validation, deterministic flow/architecture layout,
routing, a shared scene, and SVG/PNG/PDF export. The **Diagram Editor** renders
that scene on a pannable/zoomable canvas, with selection, node dragging, adding
shapes, connecting, undo/redo, the properties panel, templates and save/export.
The editor saves into an open project's `docs/diagrams/` folder through a
Save to Project dialog and autosaves a draft while typing. The canvas has
resize grips, Shift-click multi-select align/distribute, edge label and
waypoint editing, and Import for Archify JSON and plain or compressed draw.io XML. The **Diagram
Generator** turns a prompt into the strict IR through the
configured AI provider, falls back to the offline template, validates the
result, previews it and opens it in the editor. The review-only **AI
Assistant** is available in all four tools with four quick actions each,
More Suggestions and text-only context for the diagram and generator. The
repair round and drafts follow in the same release; ERD, sequence, Archify JSON
and draw.io XML import follow in Release 2. About → Third-Party Notices
lists the bundled third-party work, including the Archify schema adaptation.
See [docs/tools/diagrams.md](docs/tools/diagrams.md).

The picture operations are resize, scale, extend canvas, crop, rotate, flip,
brightness, contrast, hue, saturation, grayscale, invert, blur and sharpen,
each removable; the AI assistant is review-only.

### Scratchpad and assistant

| | |
|---|---|
| ![Scratchpad](docs/screenshots/scratchpad.png) | ![Scratchpad AI assistant](docs/screenshots/scratchpad-ai.png) |
| **Notes** — a collection of Markdown notes per workspace: the sidebar lists them newest first with pinned notes on top, one tab per note, source and preview side by side, saved 1.5 s after typing stops | **Assistant** — drafts checklists, tables, Mermaid diagrams and command fences; each block inserts at the cursor |
| ![Pinned note](docs/screenshots/scratchpad-pin.png) | |
| **Pinned notes** - right-click a note to pin it above the rest; source and preview share the pane and save a moment after typing stops | |
| ![Dictation into an assistant](docs/screenshots/ai-dictation.png) | |
| **Dictate to any assistant** — the microphone beside Send runs the operating system's own speech recognition (offline); phrases are typed into the box, a stop keeps the phrase in flight, the session ends on its own after a set time, and nothing is sent until you press Send | |

### Browser

| | |
|---|---|
| ![Browser](docs/screenshots/browser.png) | |
| **Browser** — WebView2 pages in multi-row tabs, with favourites and saved logins in the sidebar: address bar with search fallback, back/forward/reload/home, per-page ad and tracker blocking, JavaScript switch, forced dark rendering, zoom, Inspect, Print; pages restored per workspace; Shift+click a terminal link to open it here (Windows) | |
| ![Favourites](docs/screenshots/browser-favourites.png) | ![Passwords](docs/screenshots/browser-passwords.png) |
| **Favourites** - a per-workspace tree with folders and a bar under the address bar, Chrome's click rules; the star adds the open page | **Passwords** - per-workspace logins; username, password, passkey and API key live in the vault, and Fill in Password is in the toolbar |

### Settings

| | |
|---|---|
| ![General](docs/screenshots/settings-general.png) | ![Appearance](docs/screenshots/settings-appearance.png) |
| **General** — startup workspace, restore tabs, language; export and import settings and shortcuts behind the master password | **Appearance** — System / Dark / Light mode with a scheme for each (clickable swatches), interface font and size, UI zoom, tab widths, activity rail labels and order |
| ![Accessibility](docs/screenshots/settings-accessibility.png) | ![Workspace](docs/screenshots/settings-workspace.png) |
| **Accessibility** — Read Aloud through the OS voice, speech rate, reduce motion | **Workspace** — rename, logo, folder, compare environments with another workspace and copy the missing ones |
| ![Environments](docs/screenshots/settings-environments.png) | ![Security](docs/screenshots/settings-security.png) |
| **Environments** — colour, inheritance, protected flag; variables edited in place; **Import…** merges Postman/Insomnia/Bruno JSON and **Export…** writes Postman, Insomnia, Bruno or Flat JSON (secrets omitted) | **Security** — unlocked vault, relock and clipboard timers, and the credentials table: masked passwords, **Reveal Secret** and **Edit Secret** in the Action column |
| ![Edit Secret](docs/screenshots/settings-credential-edit.png) | ![Connections](docs/screenshots/settings-connections.png) |
| **Edit Secret** — after the master password, the password and key are editable; typed values move into the vault, an empty field clears the secret | **Connections** — every profile by kind, folder and tags, drag to reorder, Edit and Test Connection |
| ![Terminal settings](docs/screenshots/settings-terminal.png) | ![Editor settings](docs/screenshots/settings-editor.png) |
| **Terminal** — default shell, font, right-click copy/paste, link detection | **Editor** — font, right-click, trim and final newline on save, auto reload, language servers and debug adapters |
| ![Terminal List](docs/screenshots/settings-terminal-list.png) | |
| **Terminal List** - shells and CLI profiles in order, with Add CLI, Edit, Remove and PATH suggestions (sqlite3, claude and opencode here); a disabled CLI stays in the list and leaves the sidebar | |
| ![Browser settings](docs/screenshots/settings-browser.png) | ![Shortcuts](docs/screenshots/settings-shortcuts.png) |
| **Browser** — home page, blocking and script defaults, extra blocked hosts | **Shortcuts** — every action rebindable; conflicts listed |
| ![AI settings](docs/screenshots/settings-ai.png) | ![About](docs/screenshots/settings-about.png) |
| **AI providers** — Anthropic, Google Gemini, Azure OpenAI, AWS Bedrock, 1min.AI or any OpenAI-compatible server; add one from a gallery of fifteen services, keys in the vault | **About** — installed version, update channel, Check Now, release notes, data folder |

Every tab can be **snipped** to a PNG (Ctrl+Alt+S, camera menu) or
**printed** (Ctrl+Alt+P) as an HTML page with a Light/Dark switch that
opens in your browser: code with its highlighting, the Markdown preview,
a query with its result table, a console screen, an S3 listing, a request
with its response, a picture.

## Why

A typical day for a backend developer touches VS Code, DBeaver, Postman, WinSCP,
PuTTY, Cyberduck, a Git GUI, Docker Desktop and three browser tabs of JWT /
JSON / cron utilities. Each tool keeps its own connection list, its own secrets,
its own idea of "staging vs production", its own theme and its own update
nagging. HyperStack's bet: **the connections, environments and secrets are the
product**, and the tools are views over them.

## Stack (decided)

| Layer | Choice | Why |
|---|---|---|
| Language | **Rust** (edition 2024) | Native speed, single portable binary, same ecosystem the author already knows from Eden Explorer |
| GUI | **egui / eframe 0.35** | Immediate-mode, proven by Eden Explorer for this kind of UI, fast tables and custom widgets, no web runtime |
| Async | **tokio** runtime on background threads, channels to the UI thread | Network tools (HTTP, SSH, DB drivers) are async-first |
| Distribution | **Portable by default on Windows**: one `HyperStack.exe`, all settings and data in a `HyperStackData/` folder beside it; no installer, no registry, no `%APPDATA%` | Run from a USB stick or a tools folder, move or back up by copying the folder |
| Storage | Human-readable TOML/JSON files for shareable things + SQLite for history/cache | Collections and profiles can live in git; history stays local |
| Secrets | Encrypted vault file beside the executable (master password, Argon2id + XChaCha20-Poly1305); OS keyring optional | Portable and never a plaintext password on disk |
| Platforms | Windows first-class, Linux and macOS supported from day one | DevOps and DBAs are often not on Windows |
| Delivery | Forgejo Actions on `git.zacx.net` plus a public GitHub mirror that shares the same history and version numbers: CI on every branch, automatic SemVer release on `main`, signed `manifest.json`; each copy updates itself from the forge it was downloaded from (Settings → Updates) and can revert once | Portable copies stay current without an installer, from either source |

Full reasoning in [docs/08-decisions.md](docs/08-decisions.md).

## Documents

| # | Document | What it covers |
|---|---|---|
| 01 | [Vision and scope](docs/01-vision-and-scope.md) | Problem, personas, principles, non-goals, success criteria |
| 02 | [Feature catalogue](docs/02-features.md) | Every feature, requested or suggested, with priority and effort |
| 03 | [Architecture](docs/03-architecture.md) | Crate layout, runtime model, the `Tool` trait, services, events, persistence |
| 04 | [UI design](docs/04-ui-design.md) | Window layout, multi-row tab space, splits, launcher, command palette, keyboard, theming |
| 05 | [Data model and storage](docs/05-data-model-and-storage.md) | Workspaces, connection profiles, environments and variables, file formats, history DB |
| 06 | [Security](docs/06-security.md) | Threat model, secrets, host keys, TLS, destructive-action guards |
| 07 | [Roadmap](docs/07-roadmap.md) | Phases, milestones, checklists, effort estimates |
| 08 | [Decisions (ADRs)](docs/08-decisions.md) | Why egui and not Tauri, why hybrid Git, why not sqlx, portable-first, and so on |
| 09 | [CI/CD, releases and in-app updates](docs/09-ci-cd-and-updates.md) | Forgejo workflows, automatic versioning, signed release manifest, Settings → Updates (check, download, verify, install, revert), the public GitHub mirror (§10) |
| — | [Workflows](.forgejo/workflows/) and [release scripts](scripts/) | Active Forgejo Actions (`ci.yml`, `release.yml`), their GitHub counterparts in [.github/workflows/](.github/workflows/) and the PowerShell scripts both run; [keys/](keys/) holds the release-signing public key |
| — | [Tool specs](docs/tools/) | One design document per tool (editor, SQL database, NoSQL, Redis, logs, API client, SSH/SFTP, S3, Git, terminal, containers, devtools) |

## Tool specs

| Tool | Spec | MVP? |
|---|---|---|
| Code editor | [tools/editor.md](docs/tools/editor.md) | Yes |
| Database client | [tools/database.md](docs/tools/database.md) | Yes |
| Redis cache viewer | [tools/redis.md](docs/tools/redis.md) | Yes |
| NoSQL / document DB client (MongoDB, CouchDB, Elasticsearch, DynamoDB, …) | [tools/nosql.md](docs/tools/nosql.md) | Yes (MongoDB, CouchDB) |
| Log tail viewer | [tools/logs.md](docs/tools/logs.md) | Yes (local + SSH sources) |
| API client (REST / WebSocket / GraphQL / gRPC) | [tools/api-client.md](docs/tools/api-client.md) | Yes (REST) |
| SSH + SFTP | [tools/ssh-sftp.md](docs/tools/ssh-sftp.md) | Yes |
| S3 browser | [tools/s3.md](docs/tools/s3.md) | Yes |
| Git | [tools/git.md](docs/tools/git.md) | Yes |
| Terminal | [tools/terminal.md](docs/tools/terminal.md) | Yes |
| Containers (Docker / Podman / Kubernetes) | [tools/containers.md](docs/tools/containers.md) | Phase 2 |
| Devtools drawer (JSON, JWT, regex, cron, diff, …) | [tools/devtools.md](docs/tools/devtools.md) | Completed (core set; JSON query and the P1 extras remain) |
| Graphics | [tools/graphics.md](docs/tools/graphics.md) | Yes |
| Graphics diagrams (editor and generator) | [tools/diagrams.md](docs/tools/diagrams.md) | Planned (R1: flowchart, architecture) |
| Scratchpad | [tools/scratchpad.md](docs/tools/scratchpad.md) | Yes |
| Browser | [tools/browser.md](docs/tools/browser.md) | Windows |

## Build and run

```bash
cargo build --release -p hyperstack          # target/release/HyperStack.exe (static MSVC build)
./target/release/HyperStack.exe --version
./target/release/HyperStack.exe --self-test --data-dir ./tmp-data
./target/release/HyperStack.exe --data-dir ./tmp-data   # run against a scratch data folder
cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked
HS_S3_ENDPOINT=http://127.0.0.1:5055 cargo test -p hs-core --test s3_live   # S3 client against moto_server or MinIO
# SQL drivers: SQLite always; PostgreSQL / MySQL when given host:port:user:password:database
HS_PG=127.0.0.1:5432:postgres:pg:postgres HS_MYSQL=127.0.0.1:3306:dev:dev:shop cargo test -p tool-db --test drivers
```

On Linux, `cargo clippy --workspace` also needs `libwayland-dev`,
`libxkbcommon-dev` and `pkg-config` (the UI test harness builds the Wayland
backend).

## Repository layout

```
hyper-stack/
├── Cargo.toml                 # workspace
├── crates/
│   ├── hyperstack/            # the binary: eframe shell, tab space, launcher, settings
│   ├── hs-core/               # services: workspace model, jobs, events, settings, secrets, env vars
│   ├── hs-ui/                 # shared widgets: tab strip, data table, tree, split panes, palette, theme
│   ├── hs-editor/             # text buffer + highlighting + editor widget (used by several tools)
│   ├── hs-term/               # terminal grid, PTY, SSH shell transport
│   └── tools/
│       ├── tool-editor/
│       ├── tool-db/           # SQL: driver/ (postgres, mysql, sqlite; mssql, … later), splitter + classifier, grid
│       ├── tool-nosql/        # documents: + drivers/ (mongodb, couchdb, elasticsearch, dynamodb, …)
│       ├── tool-redis/
│       ├── tool-logs/
│       ├── tool-api/
│       ├── tool-ssh/
│       ├── tool-s3/
│       ├── tool-git/
│       ├── tool-terminal/
│       ├── tool-containers/
│       └── tool-devtools/
├── assets/
│   └── icon/                  # hyperstack.png (512 master), hyperstack.ico, PNG sizes — regenerate with scripts/make-icons.ps1
├── locales/                   # Fluent .ftl files
└── docs/
```

## Portable layout on disk (Windows)

```
HyperStack\
├── HyperStack.exe
├── HyperStackData\            # everything the app writes; delete it for a factory reset
│   ├── settings.toml
│   ├── vault.hsv              # encrypted secrets
│   ├── history.sqlite
│   ├── workspaces\default\    # built-in workspace (connections, environments, collections …)
│   └── themes\  shortcuts.toml  known_hosts  cache\  logs\
└── (optional) README.txt
```

See [docs/05-data-model-and-storage.md](docs/05-data-model-and-storage.md) for
the full layout and the rules for non-writable locations.

## License

To be decided — MIT suggested.
