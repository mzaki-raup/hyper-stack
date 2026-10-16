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

1. **Title bar**: app logo and name, workspace switcher (including the
   selected workspace logo), environment switcher, Quick Open, notification
   bell, Snip/Print menu (camera), Read Aloud (speaker, when enabled in
   Settings → Accessibility), vault control, Settings, and window controls.
   The notification bell opens retained toast history; it does not open the
   bottom dock.
2. **Tab space** — one or more rows of tabs. Every tab is a tool instance.
3. **Environment banner + tool toolbar** — a thin coloured stripe (theme colour
   per environment; red default for production) and the active tool's own
   toolbar row.
4. **Activity rail** (left, 56 px, or 78 px with labels) — **Home** first,
   then one icon per tool in the order from Settings → Appearance →
   Activity Rail (optionally with the tool's name under the icon), and the
   sidebar toggle and Settings pinned at the bottom; the rail is the
   component switcher: the tab space shows only the tabs of the selected
   component (editor tabs, or terminal tabs, or settings, …), the icon's
   tooltip carries that component's tab count, and clicking an icon shows its
   most recent tab or opens a fresh one when it has none. "+" in the strip adds
   another tab of the current component; opening something from another tool
   (quick-open, "open terminal here") switches to that component. Ctrl+PageUp/
   PageDown cycle within the component. Owner's request, 2026-10-11.
5. **Sidebar** — content supplied by the active tab's tool (`Tool::sidebar`).
   File and folder listings use the VS Code Material Icon Theme SVGs
   (`assets/icons/material/`, MIT) through `hs_ui::fileicons`, so a project
   tree reads like VS Code's. Owner's request, 2026-10-11.
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

## 3. Home tab and launcher

**Home** is a mandatory pinned tab, first in every workspace (owner's
request, 2026-10-15): the app name and build, Get Started cards (Open Folder
as Project, New Connection, New Terminal, New Workspace, Environments, Quick
Open), the workspaces as chips, then the launcher content below (tools by
category, recently closed tabs, pinned connections) and the keys worth
knowing. It replaced the Welcome tab and Settings → Welcome. Like Settings,
it cannot be closed or unpinned and is not part of the saved layout.

The launcher content is also what an empty tab space shows:

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

Each pinned-connection chip opens its profile in the tool that owns the
connection kind (`ssh` → SSH, `s3` → S3, `db` → SQL database) by sending
`OpenRequest::Connection { id }` to that tool, the same path the tools' own
sidebars and the quick-open palette use. If a tab of that tool is already
bound to the profile (`Tool::current_connection`), the shell focuses it
instead of opening another. Chips for kinds no tool handles yet (Redis,
MongoDB, …) are drawn muted, are not clickable, and say so in their tooltip.

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

The workspace switcher shows each workspace with its own logo (the
folder glyph when none is set) and a trash icon on every row while more
than one workspace exists; the same logo appears on the Home cards and
beside the Compare With list in Settings → Workspace.

Every control on the right of the title bar is the same 38 px square
(`hs_ui::form::icon_button`; the two menus use `icon_menu`, which opens an
`egui::Popup::menu` on that same square), so the row keeps one rhythm
whatever is enabled.

## 7b. Snip, Print and Read Aloud

Every tab can be **snipped** (Ctrl+Alt+S, or the camera menu): egui takes a
screenshot, the shell crops it to the tab's rectangle (or keeps the window),
copies the picture to the clipboard and offers to save a PNG. **Print…**
(Ctrl+Alt+P) asks the tool for an HTML export (`Tool::printable`: code with
its highlighting, the Markdown preview with Mermaid, a query with its
result table, a console screen, an S3 listing, a request with its
response, a picture), wraps it in a page with a Light/Dark switch and a
Print button, writes it under `HyperStackData/tmp/` and opens the browser.
**Speech to text** is the other direction: every AI assistant has a
microphone toggle beside Send (`hs_ui::mic::mic_button`). While it is on,
the operating system's dictation engine runs in a child process
(`hs_core::stt`; Windows Speech with its dictation grammar, one recognised
phrase per stdout line) and each phrase is appended to the input box, so
the user can read and edit before pressing Send. Nothing is sent to a
model by the microphone. The engine listens continuously; the button
shows a countdown and the session ends on its own after **Stop After**
(Settings → Accessibility → Speech to Text, default 60 s). Stopping is
graceful: the host writes `stop` to the engine's stdin, the engine
finishes the phrase in flight, prints it and an end mark, and the panel
keeps draining until the child has exited (a "Finishing…" state), so a
phrase spoken just before Stop still lands. Settings → Accessibility →
Speech to Text hides the toggle or sets the recogniser language; a
platform without an engine never shows it. An engine error (no microphone, no recogniser for the
language) is shown under the input for a few seconds.

**Read Aloud** (Settings → Accessibility) lists the texts the active tab
offers (`Tool::speakable`: selection, file, screen, cell, query, the
assistant's last answer) and reads one through the operating system's
voice.

## 8. Theming

Settings → Appearance has a **mode** (System, Dark, Light) and one scheme
for each of dark and light. The shell pins egui's theme preference to the
mode and fills both of egui's visuals slots with the chosen palette, so the
choice survives a restart regardless of what the OS reports; System mode
hands egui both palettes and follows the OS switch live (ADR-022).

- Theme system: dark/light base, prebuilt themes, custom palette editor with
  live preview, accent/secondary derivation, font picker (same feature set
  users know from Eden Explorer, written for `hs-ui`).
- Additional semantic colours: `env.dev`, `env.test`, `env.staging`, `env.prod`,
  `diff.added`, `diff.removed`, `diff.changed`, `terminal.ansi[16]`,
  `syntax.*` (mapped to a syntect theme), `method.get/post/put/patch/delete`,
  `status.2xx/3xx/4xx/5xx`.
- Terminal and editor can use a different font from the UI; a bundled Nerd
  Font ensures prompt glyphs render.
- **Brand palette** comes from the app icon (`assets/icon/hyperstack.png`, three
  stacked layers): purple `#7200CA`, blue `#0075D3`, gold `#A78700`, with the
  darker edge tones `#0055D4` and `#AB6C00`. The default dark and light themes
  use purple as the accent and blue as the secondary; gold is reserved for
  warnings and the beta-channel badge so it never competes with the
  environment colours. The icon itself is shown at 24 px in the title bar
  (replacing the ⚡ placeholder in the mock-ups), at 64 px on the launcher and
  at 128 px in Settings → About.

## 9. Settings categories

One Settings tab is always open and pinned in every workspace (right after
Home). It is an application-level surface, not part of a workspace's saved
tab layout, and cannot be closed or unpinned. The category list is its
sidebar and the page is the tab body. Opening Settings again focuses the
existing tab. Owner's request, 2026-10-14; Welcome moved to the Home tab on
2026-10-15.

Order: General (startup workspace, restore tabs, language, settings
import/export) · Appearance (theme mode and the dark and light schemes,
clickable swatches, interface font, tabs, activity rail order and labels)
· Accessibility (Read Aloud through the OS voice, speech rate, reduce
motion) · Workspace (name, logo, folder, environment compare and sync with
another workspace) · Environments · Security · Connections · Terminal ·
Editor · Browser (home page, blocking and script defaults, extra blocked
hosts) · AI Assistant (providers, models, Cascade rounds; ADR-019) ·
Shortcuts · About, which also holds **Updates** (installed version with sha/date/channel, channel, automatic
check, check now, release notes, download/install, revert; see
[09](09-ci-cd-and-updates.md) §5.1). The About → Installed card also shows the
data folder with **Open Data Folder**, which opens it in Eden Explorer when
that is running or installed, or in the system file manager otherwise; there
is no separate Data page any more (owner's request, 2026-10-11). Appearance
includes clickable color-scheme swatches, interface font and size, window-tab
width, editor file-tab width, and tab gap. The terminal and editor pages carry
their own font and size; three bundled Nerd Font Mono families are listed in
`assets/fonts/README.md`, along with fonts installed on the machine.

### 9.1 Visual rules

Settings pages are built from `hs_ui::form`: a page title with a one-line
description, then cards (`section`) that hold a two-column label/field grid.
Text fields are tall (7 px vertical padding) and grow to the available width
between 260 and 560 px; hints sit under the control in muted text; notes and
instructions use a framed panel with an icon. Tooltips open above the hovered
widget (`TipExt::tip`) so the pointer never covers them. The window geometry,
sidebar width and dock height are restored on the next start
(`HyperStackData/window.toml`, Appearance settings). Owner's requests,
2026-10-11.

## 10. Accessibility and ergonomics

- Full keyboard traversal of lists and trees (a known gap in Eden Explorer's
  Settings sidebar; every list here must be arrow-key navigable).
- Minimum 12 px UI font, user-scalable UI zoom (Ctrl+= / Ctrl+-).
- High-contrast prebuilt theme. Colour is never the only signal for environment
  (name is always shown; protected environments also show a lock icon).
- Reduced-motion setting disables toast slide and spinner animation.

## Addendum (2026-10-16): the activity rail scrolls

The rail holds Home, every tool and then the sidebar toggle and Settings. A
window too short for that list used to draw the last entries on top of each
other. Home is now pinned at the top and the sidebar toggle and Settings at
the bottom; only the tools between them scroll. The three fixed entries are
always reachable, which is what makes the rail usable on a short window.

## Addendum (2026-10-16): the activity rail scrolls

The rail holds Home, every tool and then the sidebar toggle and Settings. A
window too short for that list used to draw the last entries on top of each
other. Home is now pinned at the top and the sidebar toggle and Settings at
the bottom; only the tools between them scroll. The three fixed entries are
always reachable, which is what makes the rail usable on a short window.

## Addendum (2026-10-17): hiding a rail entry

Settings → Appearance → Activity Rail lists every movable tool with its
order arrows and a **Show** checkbox. Unchecking one takes it out of the
rail (`appearance.rail_hidden`); Home, the sidebar toggle and Settings are
fixed and not listed.

**Hiding is tidying, not disabling.** A tool whose entry is hidden still
opens, and when it does its entry comes back:

- `App::reveal_rail` runs at `open_tool_in`, the one place every
  programmatic open goes through — Ctrl+clicking a link into the browser,
  opening a remote file from SFTP in the editor, opening a connection, or
  picking the tool on Home.
- Saving a connection in Settings → Connections reveals the tool that kind
  belongs to (`launcher::tool_for_kind`), because a connection with nowhere
  to open is worse than a slightly fuller rail.

Both write the setting and persist it at once, so the rail is right on the
next frame and after a restart.

## Addendum (2026-10-17): dragging a row to reorder a list

Every sidebar list is the user's own collection, and the order is part of
how someone works, so one helper does it everywhere: `hs_ui::reorder`.

- Each row gets a small **drag grip** on its left (six dots, 14x22 px).
  Only the grip starts a drag. Making the whole row the drag source is
  what `Ui::dnd_drag_source` does, and it lays a `Sense::drag()`
  interaction over everything the row drew — which swallows the row's own
  clicks and hover, so the list looks right and does nothing. The grip
  also tells the user the list can be reordered, which nothing else did.
- While a row is held, a 2 px accent line with a nub at its left end shows
  the **boundary** it would land on — between two rows, not on one,
  because "below the third" and "on the third" are different intentions.
- `Reorder::finish` reports `(from, to)` where `to` is that boundary in
  `0..=len`; the list is never touched by the helper. `move_item` applies
  it, and the caller saves wherever it keeps the order.
- Dropping a row immediately above or below itself is a no-op, so a small
  accidental drag does nothing.
- The payload is a private type, so two lists on screen cannot accept each
  other's rows.

Connection profiles keep their place in an `order` field
(`docs/05-data-model-and-storage.md`). `ConnectionStore::reorder` renumbers
only the profiles in the view being dragged, over the order values they
already occupy, so putting the SSH hosts in a new order never moves the
Redis connections.

## Addendum (2026-10-17): Test Connection

Settings → Connections has a **Test Connection** button between Save and
Delete. It opens the connection for real with the driver the tool would
use, reports what the server said, and drops it again — nothing is saved,
so a profile can be proved before it is kept. Owner's requirement,
2026-10-17.

- The form is tested, not the stored profile: what you typed is what is
  tried, which is the only reading that makes "test, then decide whether
  to save" mean anything.
- The answer names the server where it can — "Connected. PostgreSQL 16.4",
  "Connected. redis 7.2.4", the bucket a scoped S3 profile reached — because
  a bare "ok" does not tell you that you reached the server you meant.
- The connect runs on the runtime like every other one (`conn_test::start`),
  so the window keeps painting; the button says Testing… while it is out.
- An answer is tied to the connection it was started for, so a slow server
  cannot report onto whichever profile is selected by the time it replies.
- **SSH host keys are not accepted from here.** The settings page is no
  place to decide to trust a key, so an unknown one is reported with its
  fingerprint and the user accepts it by opening the connection once. A
  host already in `known_hosts` authenticates fully.
- A kind with no driver (an API collection) leaves the button disabled and
  says why, rather than passing a test it never ran.

## Addendum (2026-10-17): the assistant sits hard right

Every tool's **AI Assistant** button is the last thing on its toolbar, in a
`right_to_left` group, so it is in the same place in the editor, the
terminal, SSH, SQL, S3, the API client, Graphics and the Scratchpad. The
buttons that act on what is selected keep the left; the assistant is a mode
for the whole tab, not an action on a row, and it reads that way when it is
not sitting among them. Owner's requirement, 2026-10-17.
