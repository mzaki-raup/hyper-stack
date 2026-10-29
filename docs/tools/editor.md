# Tool spec — Code editor

**Crates:** `hs-editor` (engine + widget) and `tool-editor` (tab, project tree, VFS) · **Priority:** P0 · **Status:** v1 implemented in M1 (local files, Markdown preview with native Mermaid flowcharts and sequence diagrams; no VFS/diff/minimap yet)

## 0a. Download (2026-10-15)

The project tree's context menu offers **Download…** for a file (a copy
to a place the user picks) and **Download as ZIP…** for a folder: the
archive is written on a worker thread (`zip_dir`, deflate, `.git`
skipped) and a toast reports the file count and path when it is done.

## 1. Purpose

A lightweight editor for the files developers and operators actually touch
from this app: configs (YAML/TOML/JSON/INI/.env), scripts (shell/PowerShell/
Python), SQL, Dockerfiles, Compose, Terraform/HCL, Markdown, and quick fixes to
source files — locally, over SFTP, or in S3. It also powers every other text
surface in HyperStack (SQL editor, request bodies, JSON viewer, diff, log
record view, console input).

Not a goal: replacing VS Code or an IDE. Language features come from an
installed language server (§11), never from a bundled one; there is no
debugger yet (§12 is the plan).

## 2. User stories

- Open `docker-compose.yml` on `web-01` over SFTP, fix an env var, Ctrl+S, it
  saves back and the SSH tab next to it is ready for `docker compose up -d`.
- Open a 200 MB CSV export and scroll, search, delete a column with
  multi-cursor, save — without the app stalling.
- Open the project folder, Ctrl+P to a file, Ctrl+Shift+F to search a string
  across the project, click a result.
- Compare two environment files side by side (diff mode).

## 3. Layout

```
┌ sidebar: project ─────────┬ main ─────────────────────────────────────────────────────┐
│ 📁 shop-api              │ compose.yml ● │ main.rs │ .env.staging                      │ (internal buffers in a tab, optional)
│ ▾ src                    │  1 │ services:                              ┌ minimap ┐    │
│   ▸ handlers             │  2 │   api:                                 │         │    │
│   ● main.rs              │  3 │     image: shop/api:{{tag}}            │         │    │
│ ▾ deploy                 │  4 │     environment:                       │         │    │
│   ● compose.yml  M       │  5 │       DB_URL: postgres://…             │         │    │
│   ● .env.staging         │  6 │                                        └─────────┘    │
│ ▸ docs                   │ ──────────────────────────────────────────────────────── │
│ 🔍 search in project     │ Ln 3, Col 12 · Spaces: 2 · UTF-8 · LF · YAML · sftp://web-01 │
└──────────────────────────┴───────────────────────────────────────────────────────────┘
```

**One window tab per project, its files inside** (owner's request,
2026-10-12, replacing the "Separate Tabs / Project Rows" choice of the
same day). Two multi-row tab strips:

- **The window's tab strip holds projects.** Every folder opened becomes
  its own editor tab, titled with the folder's name; opening a folder that
  already has a tab focuses it. The strip wraps onto more rows like every
  window tab strip (docs/04-ui-design.md §2.2).
- **Inside a project tab, a strip of file tabs** (the same
  `hs_ui::tabs::TabStrip`, `compact`: lower rows) holds that project's open
  files and **wraps onto more rows** when they do not fit, so no file tab
  is hidden. The sidebar tree shows that project only.

```
│ webshop ×  calc ×  api-gateway ×  billing-service ×  …  data-pipeline × │  window tabs
│ admin-portal ×  +                                                       │  (projects, wrapped)
├─────────────────────────────────────────────────────────────────────────┤
│ README.md ×  app.ts ×  compose.yml ×  customers.ts ×  … orders.ts ×     │  file tabs of
│ payments.ts ×  products.ts ×  refunds.ts ×  reviews.ts ●  +             │  "webshop" (wrapped)
```

- Clicking a file in the tree opens it as a file tab of this project (or
  switches to it). Files outside every project (a terminal Ctrl+click,
  Open File elsewhere) go to an **Other Files** tab that collects them; a
  file of another open project goes to that project's tab.
- A file tab: click to switch, middle-click or × to close, drag to
  reorder, right-click for Close / Close Others / Close to the Left / Close
  to the Right / Copy Path; "+" (Ctrl+N) adds an untitled file. Closing
  files with unsaved changes asks first ("Close Without Saving" / Cancel);
  closing the last file leaves an empty untitled one.
- Routing (`Tool::uri_affinity`, lower wins): a tab that has the file open
  or is that folder's project (0); the project holding the file (1 + its
  depth below the root, so of nested projects the innermost wins); the
  Other Files tab or an empty editor tab (200). Another folder never
  joins an existing project tab: it opens a new tab
  (`Action::OpenToolNewTab`). A file opened into a fresh tab replaces its
  empty untitled file.
- A layout saved while one tab could hold several projects keeps the
  active project in that tab and reopens the others as tabs of their own
  on the first frame.
- The window tab's title is the project name (the file name in the Other
  Files tab); the tab is dirty when any of its files is.
- Saved layout: `{ projects: [{ root?, expanded, files: [{path, cursor,
  preview}], active }], active }`; the one-file layout of earlier versions
  (`path`, `root`, `cursor`, `expanded`) is read once and migrated.
- One `notify` watcher per tab: the project root recursively, the folders
  of open files outside it non-recursively. Quick open (Ctrl+P) lists the
  project's files.
- Project roots are unique across the window: opening the same folder again
  (including a normalized alternate path) focuses its existing editor tab.
- The minimum width of file tabs inside a project is configurable under
  Settings → Appearance → Tabs (default 160 px, range 80–320 px).
- The status bar's encoding menu controls the encoding used when saving.
  UTF-8, UTF-8 BOM, UTF-16 LE/BE with or without BOM, and Windows-1252 are
  supported. **Reopen From Disk With Encoding** re-decodes a clean saved file;
  it is disabled while edits are unsaved. BOMs and BOM-less UTF-16 LE are
  detected automatically; BOM-less UTF-16 BE must be selected manually.
- The AI assistant panel is one per editor tab and works on the active file.
- **Git** (docs/tools/git.md §3a): branch and change letters in the tree, a
  Source Control list in the sidebar (switch at its top), diffs and history
  in the main area, change markers in the gutter.

## 4. Engine (`hs-editor`)

### 4.1 Buffer

- `ropey::Rope` text; `Vec<u64>` line-start cache maintained by rope's own
  line API; 100 MB opens in < 2 s; files over a threshold (default 512 MB) open
  read-only in a streaming "large file" mode shared with the log viewer.
- Encoding sniffing (UTF-8/BOM, UTF-16 LE/BE, Windows-1252 fallback via
  `encoding_rs`), line ending detection and conversion, trailing newline
  policy, tab/space detection.
- Undo tree with grouping by time/edit kind; redo; "undo all since save".
- Change tracking: dirty flag, external modification detection (watcher) with
  reload/keep prompt, lossless save (same encoding/EOL unless changed).

### 4.2 Cursors and selections

- Multiple cursors with independent selections; add cursor (Ctrl+Alt+↑/↓,
  Alt+click), select next occurrence (Ctrl+D), select all occurrences
  (Ctrl+Shift+L), column/box selection (Shift+Alt+drag), expand selection to
  word/line/bracket/indent block.
- Standard navigation: word, line, page, document, matching bracket (Ctrl+Shift+\),
  go to line (Ctrl+G), smart Home.

### 4.3 Editing

Auto-indent (keep indent; language rules for `{`, `:`, `then`), bracket and
quote auto-close with overtype, comment toggle line/block per language, move
line up/down, duplicate line, delete line, join lines, sort lines, trim
trailing whitespace, upper/lower/title case, indent/outdent, transpose, insert
UUID/timestamp (from devtools), snippets with tab stops (P1).

### 4.4 Highlighting

- `syntect` with the default syntax set plus added syntaxes (TOML, HCL,
  Dockerfile, PowerShell, Nginx, logfmt), themes mapped from the app theme
  (`syntax.*` tokens) with a few bundled dark/light syntect themes.
- Incremental: parse state cached per line; an edit re-parses from the edited
  line until the state converges. Only visible lines are laid out.
- Tree-sitter in Phase 2 for folding, structural selection and better
  accuracy; the highlighter is a trait so both can coexist.

### 4.5 Rendering

Virtualised: only visible lines create galleys; galleys cached until the line
changes or the font/theme changes. Gutter with line numbers, folding markers
(P1), git signs (added/modified/deleted via `tool-git` events), diagnostics
(API test failures, SQL errors, JSON validation). Current line highlight,
rulers, whitespace rendering, word wrap (soft, with indent-aware wrap),
minimap (P1), scrollbar annotations (search matches, diagnostics).

### 4.6 Find and replace

In-file: incremental, regex (`regex` crate), case/whole-word, in-selection,
replace one/all with capture groups, match count and overlay highlights.
In-project (Ctrl+Shift+F): background Job walking the tree with `ignore`
crate (respects `.gitignore`), results grouped by file with context, click to
open at line, replace across files with preview (P1).

### 4.7 Modes

- `Edit` — normal.
- `ReadOnly` — viewers (JSON response, Redis value, log record).
- `Diff` — two buffers, side-by-side or inline, synced scroll, change
  navigation, hunk actions (copy left/right), word-level diff via `similar`.
- `Merge` — three-way with base/ours/theirs and a result pane (P1, used by Git).
- `Single line` — console/palette inputs share the key handling.

## 5. Tool (`tool-editor`)

- **Project tree** on `hs-ui::Tree` with one root per project (local, or a
  remote `sftp://` / `s3://` folder); lazy load, `notify` watcher (local only;
  remote refresh on focus; a refresh keeps expanded subfolders open, and the
  expanded set is saved with the tab and carried to new tabs on the same
  root, so only the user collapses a folder), create/rename/delete/move with confirmation, drag
  into terminal/SFTP/S3, context menu "Open in terminal here", "Git: show
  diff", "Copy path / relative path".
- **VFS**: `hs-core::vfs` `read/write/stat/list` by scheme; saves are atomic
  for `file://` (temp + rename), direct `PUT` for `s3://` and
  write-to-temp-then-rename for `sftp://` when the server allows.
- **Quick-open provider**: fuzzy over project files (ignore-aware index built
  in the background, cached).
- **Status bar**: Ln/Col, selection count, indentation, encoding (clickable to
  convert), EOL, language (clickable to override), URI.
- **Minified JSON** (Settings → Editor → Lay Out Minified JSON When
  Opening, on by default): a JSON file that arrives on one line or with a
  line over 400 characters (a source map, a minified payload) is indented
  as it opens, as one undoable edit. `.map` counts as JSON for
  highlighting. The file on disk is untouched until the user saves, and
  the tab shows the unsaved dot to say the buffer differs from it; Ctrl+Z
  restores the original line. A file that is already laid out is left
  exactly as it is.
- **Markdown preview** (Ctrl+Shift+M, because egui turns Ctrl+Shift+V into
  a paste): `pulldown-cmark` rendered to egui with a native Mermaid subset
  (flowchart, sequence), side-by-side with scroll sync (P1).
  *Implemented (`hs-editor::mermaid`):* `flowchart`/`graph` in all four
  directions with rect, round, stadium, diamond, circle, cylinder and
  hexagon nodes, `-->`, `---`, `-.->`, `==>`, `--x`/`--o`, labels as
  `-->|text|` or `-- text -->`, chains, `&`, `;`, quoted labels, `<br/>`,
  nested `subgraph … end` drawn as frames and edges to a subgraph; ranks by
  longest path, order by neighbour barycentre, top-down ranks wider than
  760 px wrap onto another row. `sequenceDiagram` with `participant`/`actor
  … as …`, the eight arrow kinds, self messages, `Note left of/right of/
  over A[,B]`, `loop`/`alt`/`opt`/`par`/`critical`/`break` with
  `else`/`and`; long labels wrap. A diagram wider than the preview shrinks
  (down to ~55 %) and then scrolls sideways. `classDef`/`style`/`click`,
  `activate` and `autonumber` are accepted and ignored; other diagram kinds
  (gantt, class, state, …) show their source under a "not supported" note.
  Edges are straight lines, so a long edge can cross a node.
- **External formatter** (P1): run a configured command (`prettier`,
  `rustfmt`, `sqlfluff`, `black`) on save or on demand; show its errors in
  Problems.

## 6. Languages (v1 highlighting + comment/indent rules)

Rust, TypeScript/JavaScript (+JSX/TSX), JSON/JSONC, YAML, TOML, INI/.env,
Markdown, HTML, CSS/SCSS, SQL (generic, PostgreSQL, MySQL, T-SQL), Shell
(bash/zsh), PowerShell, Batch, Python, Go, Java, Kotlin, C#, C/C++, PHP, Ruby,
Dockerfile, Compose, HCL/Terraform, Nginx, Apache, XML, GraphQL, Protobuf,
Makefile, Diff, Log/logfmt, CSV (coloured columns).

## 7. Keyboard (editor scope)

| Shortcut | Action |
|---|---|
| Ctrl+S / Ctrl+Shift+S | save / save as (local or remote) |
| Ctrl+F / Ctrl+H / Ctrl+Shift+F | find / replace / find in project |
| Ctrl+D / Ctrl+Shift+L | select next occurrence / all occurrences |
| Ctrl+Alt+↑/↓ | add cursor above/below |
| Alt+↑/↓ · Shift+Alt+↓ | move line · duplicate line |
| Ctrl+/ · Ctrl+Shift+/ | toggle line / block comment |
| Ctrl+G | go to line |
| Ctrl+Shift+\ | jump to matching bracket |
| Ctrl+K Ctrl+F | format (external) |
| Ctrl+Shift+M | Markdown preview (Ctrl+Shift+V is paste on Windows) |
| Ctrl+F4 | close the active file (Ctrl+W closes the whole editor tab) |
| Ctrl+Alt+PgDn / Ctrl+Alt+PgUp | next / previous file tab |
| Ctrl+Shift+G / Ctrl+Shift+H | Source Control in the sidebar / repository history (docs/tools/git.md §3a) |
| Ctrl+Space · Ctrl+Shift+Space | completions · hover for the symbol at the cursor (§11) |
| F12 · Shift+F12 · Ctrl+click | go to definition · references · go to definition |
| F2 · Shift+Alt+F · Ctrl+Shift+J | rename symbol · format document · Problems panel |
| Ctrl+F5 · F9 · Ctrl+Shift+D | start or continue debugging · toggle breakpoint · debug panel (§12) |
| F5 (paused) · F10 · F11 · Shift+F11 · Shift+F5 | continue · step over · step into · step out · stop |
| Alt+Z | toggle word wrap |
| Ctrl+` | the editor's terminal (§13) |
| Ctrl+Shift+B | build the project (§14) |

## 8. Settings

Font and size (separate from UI), tab size and insert-spaces per language,
auto-save (off / after delay / on focus loss), trim whitespace on save, final
newline, word wrap default, rulers, minimap, bracket colouring, auto-close,
format on save, file exclusions for search, large-file threshold.

## 9. Implementation notes and tests

- Property tests on the buffer (random edits vs. a `String` oracle), undo/redo
  round-trips, encoding round-trips, line-ending conversion.
- Snapshot tests of rendering for highlighting and selection with
  `egui_kittest`.
- Benchmarks: open 100 MB, type at end of 100 MB, find across 100 MB.
- Reference points when designing: Lapce (`floem` editor core), Zed, Helix
  (selection model), CodeMirror 6 (view virtualisation).

## 10. AI assistant (ADR-019)

- **Panel**: toolbar **AI Assistant** or Ctrl+Shift+I opens a resizable
  panel on the right. A chat picks a provider and, from a **dropdown**, a
  model: the list the provider offers (`GET {base}/models` for
  OpenAI-compatible servers, asked when the provider is first picked and
  shared by every editor tab for the session; ⟳ asks again; Anthropic and
  Gemini list theirs through their own endpoints; 1min.AI cannot list its
  chat models, so it offers the 95 ids its chat endpoint accepts,
  `hs_core::ai::ONEMIN_MODEL_GROUPS`, under vendor headings with the
  display name as a tooltip; the separate "1min.AI (OpenAI-Compatible)"
  preset points at its `/openai/v1` adapter, whose `/models` is read
  live), plus an **Other model id** field at the bottom for an id the list
  lacks.
- **Providers** (Settings → AI Assistant, ADR-019 amended 2026-10-16 and
  2026-10-22): the page is arranged by **capability** - Chat Providers
  and Image Providers (`hs_core::ai::Capability`) - in the gallery, the
  provider list and the kind picker, so a chat service and an image
  server never read as interchangeable. **Add Provider** opens the
  gallery: under Chat, Hosted Services (OpenAI, Anthropic, Google Gemini,
  Azure OpenAI, AWS Bedrock, Mistral, DeepSeek in both API shapes,
  Moonshot/Kimi, Tencent Hunyuan, 1min.AI), Gateways (OpenRouter,
  1min.AI's OpenAI-compatible adapter) and On This Machine (LM Studio,
  Ollama, vLLM, any other OpenAI-compatible server); under Image, Hosted
  (1min.AI) and Self-Hosted Servers (AUTOMATIC1111, SD.Next, Forge,
  ComfyUI, SwarmUI and its ComfyUI backend, InvokeAI, Fooocus-API, the
  Stability Matrix packages, any other WebUI API). Picking a card fills
  the kind, base URL and default model, and the form shows only the fields
  that kind needs: a Resource URL, API Version and Deployment for Azure; a
  Region, Access Key ID and Secret Access Key for Bedrock; an API Key for
  the other chat kinds; a Server URL and an optional Credential
  (`user:password` for a WebUI's `--api-auth`, SwarmUI's account token, a
  proxy's bearer token) for an image server, followed by its **Image
  Workflow** section (checkpoint with Fetch Checkpoints, negative prompt,
  steps, CFG, sampler, scheduler, seed, upscaler and factor, the three
  ComfyUI workflows with Customise / Reset to Built-in, and an extra JSON
  object merged into every request). **Fetch Models** asks the service
  for its list (Bedrock cannot, so it offers **Known Models** instead) and
  **Test Connection** sends a one-line question to a chat provider or
  pings an image server for its version. Bedrock answers in one piece
  rather than word by word, which the page says. Make Default applies to
  chat providers; `Ai::chosen()` and `chat_providers()` skip image kinds,
  which only the Graphics activity lists (`image_providers()`).
- **Answers are rendered, not shown raw** (owner's request, 2026-10-22):
  `hs_ui::ai_assistant::markdown_text` reads the prose into blocks -
  headings, paragraphs with their lines joined, nested bullet and numbered
  lists, task items, block quotes, rules and **pipe tables drawn as a
  striped grid** (header row bold, horizontal scroll when wide) - with
  bold, italic, code, strikethrough and links inline. Every assistant
  (shared panel, editor panel, user turns) goes through it; fenced code
  stays a code block with its own actions.
- **Context**: Auto (the selection when there is one, else the whole file),
  Whole File, Selection Only or No Code. Only the newest request carries the
  code; earlier ones are reduced to their text. Files over 120 000
  characters are cut with a note.
- **Quick actions**: Explain, Review, Improve, Add Comments (also palette
  commands `AI: Explain/Review/Improve Selection or File`).
- **Edits**: the system prompt asks for SEARCH/REPLACE blocks. Each block
  is drawn as a diff (red removed, green added) with Apply / Reject and
  Apply All. The block is located exactly (once) or line by line ignoring
  indentation and trailing spaces; a block that matches nowhere or twice is
  marked Not Applied. An applied block is one undo step. Code fences in
  answers get Copy and Insert at Cursor.
- **Cascade** (toggle): blocks are applied automatically and the updated
  file is sent back ("continue or answer DONE") until DONE, no block
  applies, or Settings → AI Assistant → Maximum Rounds (default 4).
- The request streams on the runtime; Stop aborts it. Read-only buffers
  refuse edits. Keys come from the vault (unlock first).

## 11. Language servers (LSP, 2026-10-15)

Zed-inspired language features without a bundled toolchain: the editor is
an LSP client (`hs-lsp`, ADR-023) and the user installs the servers.

- **Which server**: `hs_lsp::registry::DEFAULTS` maps an editor language
  to the usual server command (`rust-analyzer`, `typescript-language-server
  --stdio`, `pyright-langserver --stdio`, `gopls`, `clangd`, `jdtls`,
  `vscode-json-language-server --stdio`, `yaml-language-server --stdio`,
  `taplo lsp stdio`, `bash-language-server start`, `intelephense`,
  `solargraph stdio`, `lua-language-server`, `marksman server`, …) and
  how to install it. Settings → Editor → Language Servers lists every
  language with its command, arguments, an on/off switch and whether the
  command is on `PATH`; an empty command keeps the built-in one, and any
  server speaking LSP over stdio can be entered, which is how a language
  "extension" is added. Nothing is downloaded.
- **Finding the command on `PATH`** (`hs_lsp::registry::on_path`, and the
  same rule for adapters in `hs_dap::adapters`): a name with no directory
  in it is looked up in each `PATH` entry. On Windows it is looked up
  **through `PATHEXT` only** - `jdtls.bat`, `jdtls.cmd`, `jdtls.exe` -
  never as a bare file, because a file with no extension cannot be
  started there: the Eclipse JDT LS tarball unpacks `bin/jdtls` (a Python
  script) next to `bin/jdtls.bat`, and resolving to the former gets
  "%1 is not a valid Win32 application" from `CreateProcess`. Elsewhere
  the bare name is the program. A command the user types with a path or
  an extension is taken as it is.
  `PATH` itself is whatever the process inherited at start, but it can be
  read again: **Refresh Status** in both settings tables calls
  `hs_core::env_path::refresh`, which on Windows re-reads the machine and
  user `Path` values from the registry (expanding a `REG_EXPAND_SZ`),
  appends anything the process was given that the registry does not know
  about, and forgets what the table had decided about each command. So a
  server installed from a terminal is found without a restart, and the
  button says whether `PATH` had actually moved. Elsewhere there is no
  such store and the inherited value stands.
- **jdtls on Windows** needs more than its own folder on `PATH`:
  `bin/jdtls.bat` runs `python bin/jdtls`, so Python has to be on `PATH`
  too, and the server itself needs Java 21 or newer. It keeps its
  workspace under the system temp folder, keyed on the project folder's
  name - that is the server's own choice, not something HyperStack
  configures.
- **Lifecycle**: one server process per server id and project root,
  started when the first file of that language opens in the tab, stopped
  with `shutdown`/`exit` when the tab closes or the settings table changes
  (then every open file is re-registered). `initialize` advertises UTF-8
  and UTF-16 position encodings and the server's answer decides how
  columns are converted (`hs_lsp::text`). Documents are synced as full
  text 150 ms after the last keystroke, and always right before a request
  that depends on the text.
- **Completion**: as you type an identifier or a server trigger character
  (`.`, `::`), or on Ctrl+Space. The list opens at the caret: kind icon,
  label, detail; the selected item's documentation beside it. Up/Down,
  Page Up/Down move, Enter or Tab accept, Escape closes, typing filters
  (prefix, then substring, then subsequence; `sortText` orders). Accepting
  applies the item's `textEdit` or inserts its text, plus
  `additionalTextEdits` (auto-imports), as one undo step; snippets are
  reduced to plain text with the cursor at `$0`. While the list is open the
  view hands those keys to it (`EditorView::popup_open`).
- **Hover**: rest the mouse on a symbol for 550 ms, or Ctrl+Shift+Space
  at the cursor (stays until Escape, a key or a click). Markdown is
  rendered with the Markdown preview's renderer, fenced code highlighted.
- **Diagnostics**: `publishDiagnostics` draws wavy underlines by severity
  (error, warning, info, hint); the message shows as a tooltip under the
  mouse. The status bar counts errors and warnings and opens the
  **Problems** panel (Ctrl+Shift+J), a bottom panel listing every
  diagnostic of the tab's open files with file, line and column; a click
  jumps there. The same panel shows **References** (Shift+F12).
- **Navigation**: F12 or Ctrl+click goes to the definition (the first
  location; another file opens through the shell's routing, so a file of
  another project goes to that project's tab). F2 renames the symbol
  across the project: open files are edited in place, other files are
  rewritten on disk. Shift+Alt+F formats the document through the server.
- **Installing a server**: when the language has a known server that is
  not on `PATH`, the status bar and Settings → Editor → Language Servers
  show an **Install** button. It opens a terminal tab and runs the
  server's install command (`rustup component add rust-analyzer`, `npm
  install -g …`, `pip install pyright`, …), or opens the download page
  when installation is platform-specific (clangd, jdtls, lua-language-
  server, …). The same button sits beside a missing debug adapter.
- **Status bar**: the server for the active file with its state
  (starting, a `$/progress` message such as rust-analyzer's "Indexing
  37%", ready, or stopped with the last stderr line), or, when the
  language has a known server that is not installed, a warning whose
  tooltip carries the install command.
- **Out of scope for now**: incremental sync, signature help, code
  actions, semantic tokens, inlay hints, workspace symbols, and servers
  over TCP. Files over the read-only threshold are never sent to a server.

## 12. Debugger (DAP, 2026-10-16)

The second half of the Zed-inspired request: a Debug Adapter Protocol
client (`hs-dap`, ADR-024). Adapters are installed by the user, like
language servers.

- **Adapters** (`hs_dap::adapters::ADAPTERS`): CodeLLDB (`codelldb
  --port {port}`, TCP) and lldb-dap or GDB 14 (`gdb -i dap`) for Rust, C
  and C++; debugpy (`python -m debugpy.adapter`) for Python; delve (`dlv
  dap --listen 127.0.0.1:{port}`) for Go; netcoredbg for C#; **js-debug**
  (VS Code's own JavaScript debugger, `node dapDebugServer.js {port}` over
  TCP) for JavaScript and TypeScript, which is also what a `launch.json`
  `"type": "node"` or `"pwa-node"` maps to. Java is not here: its debugger
  is a plugin loaded by jdtls rather than a standalone adapter, so it needs
  the LSP and DAP clients to talk to each other, which they do not yet.
  Settings →
  Editor → Debug Adapters shows each adapter's command, arguments and
  whether it is on `PATH`, with the install line when it is not; the
  command can be replaced (a virtual environment's `python`, a CodeLLDB
  unpacked from the VS Code extension).
- **Configurations** (the toolbar's **Debug** button, Ctrl+F5): the
  picker lists `.hyperstack/debug.json` (Zed's shape: `label`, `adapter`,
  `request`, `program`, `cwd`, `args`, `env`, a `build` step with
  `command`/`args`; `${workspaceFolder}`, `${file}` and `$ZED_WORKTREE_ROOT`
  are substituted), `.vscode/launch.json` (`name`, `type` mapped to an
  adapter: `lldb`, `python`, `go`, `coreclr`, …; comments and trailing
  commas allowed) and generated ones: a Cargo package (`cargo build`,
  then the binary under `target/debug` with the first installed adapter),
  the open Python file, a Go module. **Edit debug.json…** creates the file
  from the generated entries and opens it.
- **Session**: a build step runs first, its output in the console; the
  adapter is started on a thread (stdio, or a TCP port polled until it
  listens), `initialize` → `launch`/`attach` → on `initialized` every
  breakpoint, the adapter's default exception filters and
  `configurationDone`. `runInTerminal` is refused: generated
  configurations keep the program's console inside the editor
  (`console: internalConsole`, CodeLLDB `terminal: console`).
- **Breakpoints**: click left of a line number or press F9 (the cursor's
  line); red dots, hollow until the adapter verifies them. They are sent
  to a running session at once and saved with the tab's layout.
- **Stepping**: Continue (F5 while paused, or Ctrl+F5), Pause, Step Over
  (F10), Step Into (F11), Step Out (Shift+F11), Restart, Stop (Shift+F5)
  in the toolbar while a session runs. A stop highlights the frame's line
  with an arrow in the gutter and opens its file.
- **Panel** (Ctrl+Shift+D, a bottom panel): **Console** (program output,
  adapter messages, and an expression line evaluated in the selected
  frame), **Variables** (scopes with their variables; values with
  children expand on click and are fetched then), **Call Stack** (frames;
  clicking one selects it and jumps to its line), **Breakpoints** (every
  breakpoint with a remove button).
- **Out of scope for now**: conditional and logpoint breakpoints,
  attach pickers, hover evaluation, watch expressions, several sessions
  at once, and adapters that need a terminal.

## 13. Terminal (2026-10-17)

A shell inside the editor, so building and running a project does not mean
leaving it. Owner's requirement, 2026-10-17.

- **Where.** A resizable panel at the foot of the editor pane, above the
  status row, opened from the **Terminal** button in the editor toolbar,
  the palette (*Editor: Toggle Terminal*) or **Ctrl+`** - the shortcut
  the bottom dock's terminal had until that was removed on 2026-10-18.
- **Where it starts.** In the project's folder - the root of the editor
  tab that opened it - so `cargo build`, `mvn test` or `npm run dev` need
  no `cd`. With no project open it starts in the workspace folder. The
  folder is shown at the right of the panel's strip.
- **Which shell.** PowerShell 7 where it is installed, otherwise the
  Terminal activity's own default, otherwise the first shell detected.
  Settings → Editor → Terminal picks another; a terminal that is already
  running keeps the shell it started with.
- **Several.** The strip lists the panel's terminals; **+** adds one in
  the same folder, the bin closes the active one, and ✕ hides the panel
  without killing anything. A shell that exits says so and offers a
  restart.
- **Links.** Ctrl+click opens a `path:line` in this editor tab and a URL
  in the system browser (Shift for HyperStack's own), the same rule the
  dock terminal follows.
- It is the terminal tool's engine (`hs-term`: session, grid, keys,
  scrollback search, links) in a panel, not a second implementation, so
  the terminal settings - scrollback, copy on select, right-click
  copy/paste, link detection - apply here too. The font size is the
  editor's.

## 14. Projects: build and run (2026-10-17)

What the project **is** decides what Run and Build do, so that a Java
project is not "a folder with .java files in it".

| Found at the root | Project | Run | Build |
|---|---|---|---|
| `pom.xml` | Maven | `mvn -q compile exec:java` | `mvn package -DskipTests` |
| `pom.xml` naming Spring Boot | Maven + Spring Boot | `mvn spring-boot:run` | as above |
| `build.gradle[.kts]`, `settings.gradle` | Gradle | `gradle run` | `gradle build -x test` |
| the same naming `org.springframework.boot` | Gradle + Spring Boot | `gradle bootRun` | as above |
| `Cargo.toml` | Cargo | `cargo run` | `cargo build` |
| `package.json` | npm | `npm run dev` for a dev server, else `npm start` | `npm run build` |
| `go.mod` | Go | `go run .` | `go build ./...` |
| a `.csproj` | .NET | `dotnet run` | `dotnet build` |
| `pyproject.toml`, `requirements.txt` | Python | `python -m main` | `python -m compileall .` |
| `CMakeLists.txt` / `Makefile` | CMake / Make | `cmake --build build --target run` / `make run` | `cmake --build build` / `make` |

- **Where it runs.** Both go to the editor's own terminal (§13), in the
  project's folder — not to a tab of the Terminal activity, which is what
  they did until the owner asked for this on 2026-10-18.
- **Maven and Gradle.** A project that ships `mvnw`/`gradlew` is run
  through it, because the wrapper pins the version the project expects.
  Settings → Editor → Build Tools sets the command for each (a full path
  works, and one with spaces is quoted) and can turn the wrapper
  preference off.
- **Spring Boot** is recognised from the `pom.xml` or the Gradle build
  file, which is what picks `spring-boot:run` / `bootRun` over
  `exec:java` / `run`, and makes the project a web application.
- **A web application opens in the Browser.** The address is not guessed:
  the terminal's output is read until the program says where it listens -
  a dev server's `Local:  http://localhost:5173/` or Spring Boot's
  "Tomcat started on port(s): 8080" - and that address opens as a page in
  the Browser activity, once per run.
- **Run still runs a single file** when the open file is not the
  project's own: a Python helper inside a Maven project runs as a script.
  The project's build file (pom.xml, build.gradle, package.json, …)
  counts as the project, so Run is offered while it is open.

## 15. The project list (2026-10-26, language picker 2026-10-27)

The activity is a list of **projects**, not of folders that happen to be
open: the sidebar shows every saved project, the work pane gives each one
a single tab, and the list itself is owned by `hs_core::projects`
(`HyperStackData/projects.toml`, one `ProjectEntry` per row: id, name,
root, language, order and the repository identity).

- **One tab per project.** Clicking a project that is already open
  focuses its tab rather than opening a second one. The activity also
  keeps a permanent workspace tab and a dashboard.
- **The sidebar** lists the projects in their saved order, draggable by
  the grip (never by the row — `hs_ui::reorder`, and see the Gotcha about
  `dnd_drag_source` swallowing clicks). `+` opens the add-project form in
  Settings with the folder chooser; Home → Get Started → Open Folder As
  Project goes to the same place.
- **Right-click** a row for *Edit This Project Settings*, which jumps
  into Settings → Editor the way a connection's menu does, and *Remove
  From List*, which closes every tab under that root and **never touches
  the folder on disk**. The confirmation says so in those words, because
  "Remove" beside a path reads as something much worse than it is.
- **Settings → Editor → Project List** is the same list as a
  master/detail card, Connections-shaped: Add Project Folder leads the
  card and opens an empty form in the detail column, and Save returns to
  the list.
- **Per-project settings** are Name, Project Folder, Language, and six
  repository-local Git keys (name, email, signing key, a three-state Sign
  Commits, default branch and remote URL) written to the repository's own
  `.git/config`. An empty field *unsets* the key rather than writing a
  blank one, so the global identity is inherited again.
- **Language.** `projects::detect_language` reads the folder's manifest
  (`Cargo.toml` → Rust, `go.mod` → Go, a `package.json` beside a
  `tsconfig.json` → TypeScript, …), which is right for most folders and
  wrong for one that carries none or carries two. The Language field
  lists the 26 ids `hs_ui::icons::language_icon` draws, pre-selected from
  detection and overridable; **Detect from the folder** is the empty
  value and puts it back. Whatever it says reaches the tab icon, the
  sidebar row and the header band's `Version : Rust 1.89.0` fact
  (`tool-editor::langver`, which asks the toolchain once per session on
  the blocking pool).
- **Run settings** (2026-10-29): a project's **Run** card in Settings
  carries Spring profiles (`SPRING_PROFILES_ACTIVE`), Maven/Gradle `-P`
  profiles, a goal or task that replaces `spring-boot:run` / `exec:java`
  / `bootRun` / `run`, extra arguments, JVM arguments (`JAVA_TOOL_OPTIONS`)
  and `KEY=VALUE` environment lines. The flags go on the command line
  (`project::run_command_with`); the environment is set on the shell
  Run starts, so it reaches the application whatever the shell is. Run
  is offered for a project folder whether or not a runnable file is in
  front.
- **The dashboard** is laid out as the Scratchpad's is (2026-10-27): a
  band with the activity's mark, name and subtitle; the **Projects** and
  **Open Tabs** figures in equal rounded boxes; **Start Here** with Add
  Project Folder and Manage Projects; **Saved Projects**, every project
  a cell in a grid carrying its language icon and name over the folder's
  path; and the shared **Open Here** grid - each section ruled apart.
  It is the home tab and it stays one: it reports no uri, no
  `uri_affinity` and no project roots, and draws no toolbar, so a file
  opened from anywhere gets a tab beside it rather than replacing the
  dashboard. Reporting a uri was what made the Editor look as though it
  had no dashboard at all - one file in, and it was gone for the life of
  the workspace (owner's report, 2026-10-27; the same trap the Devtools
  home tab fell into, recorded in CLAUDE.md).
