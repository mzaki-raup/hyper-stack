# Tool spec — Code editor

**Crates:** `hs-editor` (engine + widget) and `tool-editor` (tab, project tree, VFS) · **Priority:** P0 · **Status:** v1 implemented in M1 (local files, Markdown preview with native Mermaid flowcharts and sequence diagrams; no VFS/diff/minimap yet)

## 1. Purpose

A lightweight editor for the files developers and operators actually touch
from this app: configs (YAML/TOML/JSON/INI/.env), scripts (shell/PowerShell/
Python), SQL, Dockerfiles, Compose, Terraform/HCL, Markdown, and quick fixes to
source files — locally, over SFTP, or in S3. It also powers every other text
surface in HyperStack (SQL editor, request bodies, JSON viewer, diff, log
record view, console input).

Not a goal: replacing VS Code or an IDE. No debugger, no build integration, no
LSP in v1.

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

Each **editor tab in the strip holds one file** by default (consistent with
the rest of the app). Optional "buffer tabs inside one editor tab" is a
setting for people who want an IDE-like group.

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

- **Project tree** on `hs-ui::Tree` rooted at a chosen folder (local, or a
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
