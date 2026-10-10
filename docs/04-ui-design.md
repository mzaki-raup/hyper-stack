# 04 — UI design

**Status:** draft · **Last updated:** 2026-10-10

## 1. Window anatomy

```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ ⚡ HyperStack   [Workspace: acme-shop ▾]   [Env: ● staging ▾]        🔔  ⚙   — ▢ ✕   │ title bar
├──────────────────────────────────────────────────────────────────────────────────────┤
│ 📌 main.rs │ 🗄 orders — staging │ 🌐 POST /checkout │ 🖥 web-01 │ ☁ assets-bucket │ ⌥ git │ + │ tab row 1
│ 📝 docker-compose.yml │ 🗄 users — prod 🔒 │ 💻 pwsh │ 🧰 JWT │                           │ tab row 2 (wraps)
├────┬─────────────────────────────────────────────────────────────────────────────────┤
│ ▤  │ ▌ staging ▐  toolbar of the active tool (Run ▶  Explain  Commit  Format …)        │ env banner + toolbar
│ 📝 │─────────────────────────────┬───────────────────────────────────────────────────│
│ 🗄 │                             │                                                   │
│ 🌐 │   sidebar of active tool    │   main area of active tab (may be split)          │
│ 🖥 │   (schema tree, project     │                                                   │
│ ☁  │    tree, collections,       ├───────────────────────────────────────────────────┤
│ ⌥  │    hosts, buckets)          │   second pane of the split                        │
│ 💻 │                             │                                                   │
│ 🧰 ├─────────────────────────────┴───────────────────────────────────────────────────│
│    │ ▸ Output  Jobs (2)  Problems  Terminal                                      ⌃  │ bottom dock (collapsible)
├────┴─────────────────────────────────────────────────────────────────────────────────┤
│ ● connected postgres 16 · 1,204 rows · 38 ms      UTF-8  LF  SQL      ⇅ 2 jobs   en  │ status bar
└──────────────────────────────────────────────────────────────────────────────────────┘
```

Regions, top to bottom:

1. **Title bar** (custom-drawn): app menu, workspace switcher,
   environment switcher, notification bell, settings, window controls.
2. **Tab space** — one or more rows of tabs. Every tab is a tool instance.
3. **Environment banner + tool toolbar** — a thin coloured stripe (theme colour
   per environment; red default for production) and the active tool's own
   toolbar row.
4. **Activity rail** (left, 44 px) — one icon per tool category; clicking opens
   that tool's sidebar or, if the active tab is a different tool, offers "new
   tab" for it. Collapsible.
5. **Sidebar** — content supplied by the active tab's tool (`Tool::sidebar`).
   Width persisted per tool.
6. **Main area** — the active tab's `Tool::ui`, or a split tree of several tabs.
7. **Bottom dock** — Output (logs), Jobs, Problems (editor diagnostics, failed
   assertions, SQL errors), Terminal (quick local shell). Height persisted.
8. **Status bar** — tool-supplied left segments, global right segments.

## 2. The tab space

### 2.1 Model

```rust
struct TabSpace {
    tabs: Vec<TabModel>,        // display order
    active: TabId,
    layout: LayoutNode,         // Leaf(TabId) | Split { dir, ratio, a, b }
    rows_cache: Vec<Row>,       // recomputed when width/count/settings change
}
struct TabModel {
    id: TabId,
    tool: Box<dyn Tool>,
    pinned: bool,
    colour: Option<Color32>,    // user override; else environment colour
    group: Option<GroupId>,     // tab group membership
    created: Instant,
}
```

### 2.2 Multi-row wrapping (behaviour modelled on Eden Explorer's strip, implemented fresh)

Behaviour, in order:

1. Pinned tabs render first, at icon-only width, never wrap away.
2. Remaining width is divided among tabs down to **minimum tab width**
   (Settings → Appearance → Layout, default 140 px) with **tab gap** (default
   4 px) between them.
3. When `n_tabs * (min_width + gap)` exceeds the strip width, tabs **wrap onto
   additional rows**; `tab_row_count()` gives the row count and the title-bar
   region grows by `row_height` per row. Rows fill left to right in display
   order.
4. Tabs are never scrolled off-screen and never shrink below the minimum.
5. The active tab's row is visually joined to the toolbar below (no bottom
   border) so the user can see which row is "front".

Tab anatomy: `[tool icon] [title, middle-elided] [dirty dot | env dot] [✕]`.
Hover shows a tooltip with the full title, the connection, the environment and
the file path or URL.

### 2.3 Tab interactions

| Action | Mouse | Keyboard |
|---|---|---|
| Switch | click | Ctrl+Tab / Ctrl+Shift+Tab (MRU), Ctrl+1…9, Ctrl+PgUp/PgDn (by position) |
| New (launcher) | `+` or double-click empty strip | Ctrl+T |
| Close | ✕, middle-click | Ctrl+W (asks if dirty) |
| Reopen closed | — | Ctrl+Shift+T |
| Reorder | drag within the strip (cross-row allowed) | Ctrl+Shift+PgUp/PgDn |
| Pin | context menu | Ctrl+Shift+P → "Pin tab" |
| Split | drag tab onto the main area's edge (drop zones: left/right/top/bottom) or context menu "Split right/down" | Ctrl+\ (split right), Ctrl+Shift+\ (split down) |
| Move to split | drag onto an existing pane | Ctrl+Alt+→ / ← |
| Duplicate | context menu | — |
| Close others / left / right | context menu | — |
| Add to tab group / save all as group | context menu | — |
| Colour tab | context menu | — |

Context menu also offers tool-specific entries (Reconnect, Copy path, Copy URL,
Open in terminal).

### 2.4 Splits

The main area is a binary tree of splits. A split contains tabs that are also
in the strip; the strip highlights every visible tab (one with full accent, the
others with a thin underline). Closing a tab in a split collapses the split.
Ratios persist. Maximum nesting depth 3 to keep the UI sane.

### 2.5 Tab groups and workspaces

- **Tab group**: a named, ordered list of `(factory_id, saved_state)`. Open as
  "add" or "replace". Saved in the workspace folder as `tab-groups/<name>.toml`.
- **Workspace**: folder + environments + connections + collections + the last
  layout (`layout.json`: tabs, splits, sidebar/dock sizes, active env). The app
  opens the last workspace; `File → Open Workspace…` switches. Multiple
  windows are not required for v1 (one workspace per process; second instance
  opens a second process).

## 3. Launcher (new tab page)

Shown by `+`, Ctrl+T, and when the last tab is closed.

```
┌─ What do you want to do? ──────────────────────────────────── 🔍 type to search ─┐
│  Code              Data               Network             Infra        Utilities   │
│  📝 Editor         🗄 SQL database     🌐 API request      🖥 SSH shell  🧰 JSON     │
│  ⌥ Git             🍃 NoSQL / Mongo    🔌 WebSocket        📁 SFTP       🔑 JWT      │
│  💻 Terminal       📊 Redis            ◼ GraphQL          ☁ S3          ⏱ Cron     │
│                    📜 Log tail                            🐳 Docker     … more     │
├─ Recent ────────────────────────────────────────────────────────────────────────────┤
│  🗄 orders — staging · 2 min ago     🌐 POST /checkout · 10 min ago   📝 main.rs    │
├─ Pinned connections ────────────────────────────────────────────────────────────────┤
│  ● prod-db (read-only)   ● staging-db   🖥 web-01   🖥 web-02   ☁ assets-bucket       │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

Typing filters tools, recent items and connections together using the shared
fuzzy ranker. Enter opens the first match.

## 4. Command palette and quick-open

- **Ctrl+Shift+P** — commands. Sources: shell commands (tabs, layout, settings,
  environment switch, workspace switch, theme), active tool commands
  (`Tool::commands`), global tool commands (every factory offers "New …").
  Shows the shortcut on the right; recently used first.
- **Ctrl+P** — quick-open items: project files, connections, saved queries,
  requests, hosts, buckets, snippets, recent files. Prefix filters: `>`
  commands, `@` connections, `#` saved queries, `:` go to line (editor).
- Both share one fuzzy ranker (`hs-core::fuzzy`: subsequence match with
  bonuses for word starts, camel humps and recency) and one renderer, with a
  provider trait so tools register their own item sources.

## 5. Environments in the UI

- The title-bar **environment switcher** shows the active environment with its
  colour. Switching emits an event; tools whose current tab is bound to a
  different environment show a "This tab is pinned to *prod*" badge rather than
  switching silently.
- The **environment banner** under the tab strip shows the environment name of
  the active tab. Production (or any environment flagged `protected`) also gets
  a 2 px coloured frame around the whole main area.
- Protected environments: database connections default to read-only, API
  requests with non-GET methods ask for confirmation, SFTP/S3 deletes ask with
  the environment named, `git push --force` is disabled.

## 6. Shared widgets (`hs-ui`)

| Widget | Used by | Notes |
|---|---|---|
| `TabStrip` | shell | multi-row, drag, pin, context menu |
| `DataTable` | DB results, S3 listing, SFTP listing, API headers/cookies, container lists, process viewer | virtualised rows, column resize/reorder/hide, multi-sort (Shift+click adds, Ctrl+click removes), inline filter, cell copy, cell viewer popup, selection (row/range/cell), export menu |
| `Tree` | project tree, schema, collections, hosts, buckets, k8s | lazy children, inline rename, drag source/target, badges, context menu |
| `SplitLayout` | shell main area, SFTP dual pane, editor diff | drag handles, min sizes, ratio persistence |
| `Palette` | shell | commands + quick-open |
| `EnvBanner`, `Badge`, `Toast`, `Dialog`, `ProgressBar`, `KeyValueEditor` (headers, params, variables with enable checkboxes), `CodeView` (read-only highlighted text with copy), `Breadcrumb` (paths, prefixes, URLs) | everywhere | |

## 7. Keyboard

Global defaults (all rebindable via `HyperStackData/shortcuts.toml`):

| Shortcut | Action |
|---|---|
| Ctrl+T / Ctrl+W / Ctrl+Shift+T | new tab / close / reopen |
| Ctrl+Tab, Ctrl+1…9 | switch tabs |
| Ctrl+\ , Ctrl+Shift+\ | split right / down |
| Ctrl+Shift+P, Ctrl+P | palette, quick-open |
| Ctrl+B | toggle sidebar |
| Ctrl+J | toggle bottom dock |
| Ctrl+` | quick terminal in dock |
| Ctrl+Shift+E | switch environment |
| Ctrl+, | settings |
| F1 | shortcuts page |
| Ctrl+S | save (editor, request, query) |
| Ctrl+Enter / F5 | run (query, request) |
| Ctrl+Shift+Enter | run with explain / run collection |
| Esc | close popup / cancel running job in focused tool |

Tool-specific maps live in each tool spec. Conflicts are shown in the
shortcuts page.

## 8. Theming

- Theme system: dark/light base, prebuilt themes, custom palette editor with
  live preview, accent/secondary derivation, font picker (same feature set
  users know from Eden Explorer, written for `hs-ui`).
- Additional semantic colours: `env.dev`, `env.test`, `env.staging`, `env.prod`,
  `diff.added`, `diff.removed`, `diff.changed`, `terminal.ansi[16]`,
  `syntax.*` (mapped to a syntect theme), `method.get/post/put/patch/delete`,
  `status.2xx/3xx/4xx/5xx`.
- Terminal and editor can use a different font from the UI; a bundled Nerd
  Font ensures prompt glyphs render.

## 9. Settings categories

General (startup workspace, restore tabs, language, update check) · Appearance
(theme, fonts, tab min width, tab gap, density) · Shortcuts · Environments ·
Connections · Editor · Terminal · Database · API client · SSH/SFTP · S3 · Git ·
Security (secret store: vault or OS keyring, master password, remember on this
PC, lock after idle, clipboard clearing, encrypt history) · Data (export /
import / reset per section, data folder location and "open data folder") ·
About.

## 10. Accessibility and ergonomics

- Full keyboard traversal of lists and trees (a known gap in Eden Explorer's
  Settings sidebar; every list here must be arrow-key navigable).
- Minimum 12 px UI font, user-scalable UI zoom (Ctrl+= / Ctrl+-).
- High-contrast prebuilt theme. Colour is never the only signal for environment
  (name is always shown; protected environments also show a lock icon).
- Reduced-motion setting disables toast slide and spinner animation.
