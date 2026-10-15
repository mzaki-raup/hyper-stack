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
| Alt+Z | toggle word wrap |

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
  shared by every editor tab for the session; ⟳ asks again; 1min.AI has no
  cannot list its chat models, so it offers the 95 ids its chat
  endpoint accepts, `hs_core::ai::ONEMIN_MODEL_GROUPS`, under vendor
  headings with the display name as a tooltip; the separate "1min.AI
  (OpenAI-Compatible)" preset points at its `/openai/v1` adapter, whose
  `/models` is read live),
  plus an **Other model id** field at the bottom for an id the list lacks (Settings → AI
  Assistant lists providers: 1min.AI or OpenAI-compatible such as OpenAI,
  LM Studio, Ollama, OpenRouter; Fetch Models asks an OpenAI-compatible
  server for its list; Test Connection sends a one-line question).
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
- **Status bar**: the server for the active file with its state
  (starting, a `$/progress` message such as rust-analyzer's "Indexing
  37%", ready, or stopped with the last stderr line), or, when the
  language has a known server that is not installed, a warning whose
  tooltip carries the install command.
- **Out of scope for now**: incremental sync, signature help, code
  actions, semantic tokens, inlay hints, workspace symbols, and servers
  over TCP. Files over the read-only threshold are never sent to a server.

## 12. Debugger (planned)

The next step of the same request: a Debug Adapter Protocol client
(`hs-dap`) with launch configurations read from `.hyperstack/debug.json`
and `.vscode/launch.json`, generated ones for Cargo, Python and Go
projects, breakpoints in the gutter, continue/step controls, call stack,
variables and a debug console. Adapters (CodeLLDB, debugpy, delve) are
installed by the user, like language servers.
