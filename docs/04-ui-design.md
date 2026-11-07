# 04 — UI design

**Status:** draft · **Last updated:** 2026-10-10

## 1. Window anatomy

```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ ⚡ HyperStack   [Workspace: acme-shop ▾]   [Env: ● staging ▾]        🔔  ⚙   — ▢ ✕   │ title bar
├──────────────────────────────────────────────────────────────────────────────────────┤
│ 📌 main.rs │ 🗄 orders — staging │ 🌐 POST /checkout │ 🖥 web-01 │ ☁ assets-bucket │ ⌥ git │ + │ tab row 1
│ 📝 docker-compose.yml │ 🗄 users — prod 🔒 │ 💻 pwsh │ 🧰 JWT │                           │ tab row 2 (wraps)
├──────────────────────────────────────────────────────────────────────────────────────┤
│ ▌ staging ▐                                                                          │ env banner (window)
├────┬─────────────────────────────┬───────────────────────────────────────────────────┤
│ ▤  │                             │ toolbar of the active tab (Run ▶  Explain  …)     │ toolbar (pane)
│ 📝 │                             │───────────────────────────────────────────────────│
│ 🗄 │                             │                                                   │
│ 🌐 │   sidebar of active tool    │   main area of the active tab                     │
│ 🖥 │   (schema tree, project     │                                                   │
│ ☁  │    tree, collections,       │                                                   │
│ ⌥  │    hosts, buckets)          │                                                   │
│ 💻 │                             │                                                   │
│ 🧰 ├─────────────────────────────┴───────────────────────────────────────────────────│
│    │ ▸ Output  Jobs (2)  Problems                                                ⌃  │ bottom dock (collapsible)
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
   **Work-pane item lists use the same language** (FEAT-0084): a branch, a
   volume, a table column, a topic and the rest resolve through
   `fileicons::for_kind`, whose icons were drawn for HyperStack because the
   upstream theme is file-type oriented and has none of them (provenance in
   `assets/icons/material/README.md`). The ten activities with such lists —
   `settings::MATERIAL_ICON_TOOLS`: Scratchpad, Editor, Git, SSH, Container,
   Database, NoSQL, Redis, S3 and Queue — each have a switch in
   Settings → Appearance → Material Icon, on by default; off, that activity
   draws the glyphs it drew before. A kind with no sensible picture keeps its
   glyph rather than taking a misleading one, so a Kubernetes pod, a foreign
   key and a loading row stay glyphs even with the switch on.
   Width persisted per tool.
   **Every sidebar list is a vertical scroll area with a fixed header**
   (`egui::ScrollArea::vertical().auto_shrink([false, false])` and a stable
   `id_salt`): a long connection list, the Devtools utilities, the Terminal
   shells and the Settings page list must never hide their last rows when
   the window is short, and a future tool or connection must not require a
   layout change. The DB, Redis, NoSQL, API client, editor tree, Home
   sidebar, Scratchpad and Graphics lists already worked this way;
   Devtools, SSH, S3, Terminal and the Settings page list follow now.
   Owner's request, 2026-10-19.
6. **Main area** - the active tab's `Tool::ui`. One tab, one pane.
7. **Bottom dock** — **Logs** (the app's own log: a level picker, search,
   Copy and Clear, lines coloured by level; the tab label counts errors
   that arrived while another tab was showing; the recorded level is
   Settings → General → Behaviour → Log Level and is applied to the
   `tracing` filter at run time; every error or warning toast a tool
   raises is also written to the log, so the reason outlives the toast -
   owner's request, 2026-10-22), Jobs, Problems (editor diagnostics,
   failed assertions, SQL errors). The quick shell was removed on
   2026-10-18. Height persisted.
8. **Status bar** — tool-supplied left segments, global right segments.

### 1.6 UI/UX consistency rules

Owner's rules, 2026-10-20. They apply to every activity dashboard, tool
toolbar, properties pane and dialog:

- **Dashboards share one rhythm.** A section heading starts with the same
  top spacing as the other activities (Graphics' "What To Do" follows
  Devtools' "Open Here"), actions use the same card padding, and the label
  and icon are vertically centred inside each card.
- **Row actions sit in a fixed column.** A list of functions puts its action
  buttons in one aligned column next to the description, not at the far right
  of a wide pane; the AI Assistant is the one control that belongs at the hard
  right of a toolbar.
- **One assistant position.** The AI Assistant button is the hard-right item
  of every tool toolbar, whether the tool draws a toolbar or a diagram canvas.
- **Source and preview are peers.** Where a document has a source form, the
  Preview / Source / Split control sits in the toolbar and Split puts the
  source to the right of the preview. The source follows edits immediately.
- **Undo is universal.** Every visual editor gets Undo and Redo, Ctrl+Z and
  Ctrl+Y, and the number of steps comes from Settings → General → Behaviour →
  Undo Limit (1–100), so changing the setting changes every tool that keeps
  its own history.
- **Shape-specific drawing.** Diagram nodes use the shape their type names
  (a cylinder for a database, a diamond for a decision, a folded note), and
  edge, node and canvas attributes live in the properties pane with the same
  label/field grid.
- **Nothing is clipped at the edge.** The tab strip reserves a right margin
  so the last tab's title and close button stay visible and the strip wraps
  before the window edge.

## 2. The tab space

### 2.1 Model

```rust
struct TabSpace {
    tabs: Vec<TabModel>,        // display order
    active: TabId,
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

1. The activity's **permanent tab** renders first, then the pinned tabs,
   all in fixed-width slots on row 0 that never wrap away. Pinned slots are
   icon-only where the tool says so (the browser's favicon); a permanent tab
   always keeps its name, because it is the activity's front page rather
   than a bookmark. The strip's own order decides nothing here, so pinning
   can never put a tab in front of the permanent one (owner's requirement,
   2026-10-17).
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

A permanent tab is named after its **activity** ("Devtools",
"Scratchpad", "Terminal"), not after whatever its tool happens to hold:
the shell tells a tool it is the home tab through `Tool::set_home_tab`
before the first frame, so the name is right even for a tab the user has
not opened yet. The API client is the exception the owner asked for - its
home tab carries the workspace's name and logo. A dashboard home tab also
draws no toolbar, since Save, Run and Download would aim at nothing
(owner's report, 2026-10-17).

Tab anatomy: `[tool icon] [title, middle-elided] [dirty dot | env dot] [✕]`.
Hover shows a tooltip with the full title, the connection, the environment and
the file path or URL.

**Whole titles (2026-11-07, CR-0086).** A tool whose tab titles are fixed
names rather than user data returns true from `Tool::tab_title_whole()`
(passed to the strip as `TabItem::whole_title()`); Devtools is the only one.
The strip then raises its minimum tab width to the widest such title,
above the usual cap, so "Unit Conversion" or "Firebase Tokens" shows whole
beside its icon, and wraps rows (rule 3) instead of eliding. Every other
activity keeps the shared width and the middle ellipsis - the owner's
choice: "Only Devtools, because the label name is fixed."

**Row height is a setting** (Settings → Appearance → Tabs, 2026-11-03):
*Minimum Main Tab Height* for the window strip (28–48, default 36) and
*Minimum Sub Tab Height* for an inner strip — the editor's files, the
browser's pages, a database tab's queries (22–44, default 30). The inner
one is held to the main one's height however the file is edited, so the
main tabs stay the ones that read as main. Everything inside a tab
scales with its row: the icon and the title at 0.44 of the height, the
close cross a little smaller, so a taller strip means bigger tabs rather
than a taller box around the same small ones (CR-0029).

**The close-the-others entries follow the tab's position.** An entry is
offered only when it would close something, and never when it would
close exactly what Close Others closes — two ways to do one thing read
as two different things. So: Close Others whenever another closable tab
exists; Close to the Left and Close to the Right only on a tab with
closable tabs on *both* sides. The activity's permanent tab therefore
offers Close Others alone, the first and last tabs offer Close Others
alone, and a lone tab beside the permanent one has no menu at all.
Pinned and permanent tabs survive all three actions, so they never count
as a neighbour worth closing.

**A pinned tab is the exception**, and for the same reason: Close Others
spares the other pinned tabs, so from a pinned tab it does not mean what
it says and is not offered. A pinned tab is drawn in the lead zone at
the strip's head, so every closable tab is to its *right* whatever its
place in the tab list and nothing is ever to its left: its menu is Close
and Close to the Right, and that entry closes every closable tab while
sparing the other pinned ones. The same holds for the permanent tab,
whose scope is read from where it is drawn rather than where it sits in
the list. **Duplicate** can
also be dropped rather than greyed out (`TabItem::shows_duplicate`): the
Scratchpad does, because a note tab *is* the note and a second view of
it would share one cursor (owner's rules, CR-0029).

### Fixed tabs: colour and logo (2026-11-03, CR-0043)

An activity's fixed tab — its permanent home, and Settings — draws its
glyph in the activity's rail colour (`app::activity_colour`) while
Appearance → Sidebar → *Coloured Activity Icons* is on, and in the
muted text colour otherwise. It is **never outlined**, whatever *Outline
Activity Icons* says: at a tab's size a ring reads as a smudge. The
Editor, the API Client and the Browser are left out, because their fixed
tab wears a picture rather than a glyph.

A fixed tab, and a pinned tab that shows its name, is **as wide as its
icon and label need** — measured in the font the strip draws, so a taller
tab height with its bigger text still fits — rather than sitting in a
fixed 132 px slot that truncated "Scratchpad" and "Containers" once the
text grew. A pinned note's slot stops at the regular maximum tab width,
since a note's title can be long; a fixed tab's name is shown whole.

The Editor's and the API Client's home tabs are the workspace's front
page, so they wear the workspace's logo (Settings → Workspace → Logo)
when it has one. A tool asks for it with `Tool::tab_wears_workspace_logo`
and the **shell** supplies it, through `hs_ui::icons::workspace_logo_uri`:
only the shell has the frame's context to load an image with, and a tab
nobody has drawn never runs its own `ui`. The Browser's window tab wears
it too. The file is read when the workspace changes (the cache is keyed
by its folder) and when Settings → Workspace sets or removes the logo
(`reload_workspace_logo`), never per frame.

### One pin for a Scratchpad note (2026-11-03, CR-0041)

A note is pinned in one place — the sidebar's list, `notes/pins.json` —
and everything else reads it. **The Scratchpad only**; every other
activity keeps the shell's own per-tab pin.

- `Tool::tab_pinned()` answers `Some` for a tab holding a saved note, so
  the strip draws it pinned when the sidebar says so. Pin or unpin in
  the sidebar and the tab follows.
- Pin or unpin in the tab's menu goes to `Tool::set_tab_pinned`, which
  writes that same list and broadcasts `Event::Custom` on
  `scratchpad.pins`; the other note tabs reload it and the sidebar
  follows. The shell does not touch its own flag for such a tab, so the
  two cannot disagree.
- `Tool::pin_rank()` is the note's place in that list, and the strip
  sorts its pinned slots by it — so the pinned tabs read in the
  sidebar's order.
- Pinning a note from the sidebar **opens** it (or focuses the tab
  already showing it), so the pin is on the strip at once; unpinning
  leaves the tab alone (BUG-0041).
- `Tool::tab_closable()` is false while a note is pinned: pinning a note
  is how you say it stays. The cross goes, Close leaves the menu, and
  the shell refuses a close however it was asked for (Ctrl+W, a middle
  click, a sweep). Unpinning brings it all back.
- `Tool::tab_icon_is_pin()` puts the pin on the tab in place of the
  note's own icon, drawn in `theme.pin` — the same colour the sidebar's
  pin wears, so the two lists agree at a glance. The colour is a setting
  (Appearance → Scratchpad → Pin Colour, one of eight in
  `hs_core::settings::PIN_COLOURS`, red by default) and choosing one is a
  theme change, so the shell's cached theme — which the strip reads — is
  rebuilt with it.
- An unpinned note's tab wears the Material note icon its sidebar row
  does. `Tool::tab_item_kind()` names the kind; the shell maps it through
  `fileicons::for_kind` when the activity has Material icons on and hands
  the strip the image itself (`TabItem::icon_image`), since an
  `include_image!` source has no loadable `bytes://` uri on its own.
  With Material icons off the tab keeps the tool's glyph.

The **Settings tab has no context menu and its strip no `+`**: it is one
tab, always open, and every entry would act on tabs it has no business
closing (CR-0029).

### 2.3 Tab interactions

| Action | Mouse | Keyboard |
|---|---|---|
| Switch | click | Ctrl+Tab / Ctrl+Shift+Tab (MRU), Ctrl+1…9, Ctrl+PgUp/PgDn (by position) |
| New (launcher) | `+` or double-click empty strip | Ctrl+T |
| Close | ✕, middle-click | Ctrl+W (asks if dirty) |
| Reopen closed | — | Ctrl+Shift+T |
| Reorder | drag within the strip (cross-row allowed) | Ctrl+Shift+PgUp/PgDn |
| Pin | context menu | Ctrl+Shift+P → "Pin tab" |
| Duplicate | context menu | — |
| Close others / left / right | context menu | — |
| Add to tab group / save all as group | context menu | — |
| Colour tab | context menu | — |

Context menu also offers tool-specific entries (Reconnect, Copy path, Copy URL,
Open in terminal).

### 2.4 No splits (removed 2026-10-23)

**The main area shows exactly one tab.** Split panes were specified here and
built as a binary tree (`hs_ui::split`, `Layout::split`), reached from the tab
context menu, then from the palette alone after 2026-10-17. They are gone, at
the owner's decision, with the evidence that argued for it:

- Every split after the first silently swapped a pane's tab instead of
  dividing it, and the bug went unnoticed for as long as the feature existed.
  Palette-only reach meant nobody was using it.
- A split tree spans activities, which the rest of the shell does not. The
  strip lists one activity's tabs, the sidebar and toolbar follow the active
  tab, and switching activity left the panes alone, so a pane could show a tab
  its own strip did not list.
- The new pane always got a fresh Scratchpad note. There was no way to put a
  chosen tab in it, so the thing splits exist for, comparing two documents,
  was never possible.

If side-by-side is wanted later it is a design task, not a restoration:
choosing which tab fills a pane, dragging a tab into one, a toolbar per pane,
and a rule for panes belonging to different activities. A `layout.json` saved
before the removal still loads, because `Layout` carries a container-level
`serde(default)` and does not reject unknown fields, so the old `split` key is
ignored.

### 2.5 Tab groups and workspaces

- **Tab group**: a named, ordered list of `(factory_id, saved_state)`. Open as
  "add" or "replace". Saved in the workspace folder as `tab-groups/<name>.toml`.
- **Workspace**: folder + environments + connections + collections + the last
  layout (`layout.json`: tabs, sidebar/dock sizes, active env). The app
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

Home is the one dashboard with a **toolbar**: a row of activity icons, the
quickest way from Home to anywhere (owner's requirement, 2026-10-25). It is
the same set the rail shows for this workspace, in the same order — Settings
→ Appearance → Activity Rail is per workspace, so an activity hidden there
is hidden here — minus Home itself and the sidebar toggle, which is a view
switch rather than an activity, plus **Settings** after a separator, as at
the foot of the rail. Icons only by default, with the activity's name on
hover: the names are already under the rail's icons, and a row of labels
would be a worse version of the launcher further down the page. Settings →
Appearance → Activity Rail can put the names under these icons too, at one
fixed width each (CR-0038, addendum below). Their colour follows
Settings → Appearance → Sidebar → Coloured Activity Icons, so the two
columns of icons never disagree. The icons are 22 px with 8 px between
them, and hovering one fills its plate and draws an accent edge — a row
of targets needs to say which one is under the pointer, and a colour
change alone cannot say it on an already-coloured glyph. A click opens the activity's **dashboard**
— the request carries no item, which is what makes `open_tool` focus the
dashboard home tab — and Settings opens on the page it was last left on,
General the first time.

The launcher content is also what an empty tab space shows:

Shown by `+`, Ctrl+T, and when the last tab is closed.

The strip's **`+` means "a new tab of this activity"**, not "open this
activity" — for an activity whose home is a dashboard those are different
things, and the second does nothing when the dashboard is what the `+` was
clicked from. It appears only where a tab is a **document somebody makes**
(the editor, the API client, Graphics, the Scratchpad); an activity whose
tab is a connection, a shell or a host — Terminal, SSH, the database
activities, S3, the browser, Logs, Devtools and **Containers** — hides it,
because those tabs are opened from the sidebar and an empty one would hold
nothing (`shows_new_tab`, owner's decision 2026-10-25). With no activity in
front, `+` opens the launcher instead.

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

### 1.1 Panel order (2026-10-25)

The shell declares its egui panels in this order, and the order *is* the
layout: top bar, tab strip, environment banner — each the width of the
window — then the activity rail, then the sidebar, and only then the
**toolbar**, the status bar and the dock, each of which therefore spans
what the two left panels leave: the work pane's own column.

The toolbar used to sit in the banner's panel, full width, which pushed
the rail and the sidebar a row down and put the buttons that act on the
pane above the sidebar as well. It is the *tab's* toolbar, so it belongs
over the tab's pane — where the status bar already was. The panel is
skipped entirely when the active tab has no toolbar (a dashboard home
tab), rather than drawn empty. Owner's requirement.

### 1.2 Outlined activity icons (2026-10-26)

**Settings → Appearance → Sidebar → Outline Activity Icons** draws a ring
around each coloured activity icon — light on a dark theme, dark on a
light one — so a glyph in its own hue separates from the panel behind it.
On by default, and offered only with Coloured Activity Icons on: a plain
icon is already the theme's text colour, and a contrasting ring around
that reads as a glow rather than an edge.

epaint has no stroke for text (`TextShape` carries an `underline` and
nothing else), so the ring is the glyph drawn eight times at small
offsets under the real one — `hs_ui::icons::outlined_text`, with the
diagonals pulled in by √½ so it is round. The width is
`app::ICON_OUTLINE_WIDTH`, 1.5 pt: enough to separate the glyph without
closing up its own detail at 18 px.

Nine text shapes per icon is nothing for a rail of sixteen and would be
worth thinking about for a tree of hundreds, which is why this is a
helper a caller opts into rather than something `painter.text` does
everywhere. Icons built into a button's label (`format!("{icon} {text}")`
handed to `Button`) cannot be outlined at all: egui lays the icon and the
text out as one run.

### 1.3 A tab wears its own icon (2026-10-26)

`Tool::tab_icon` lets a tab show the glyph **its own row wears in the
sidebar** rather than its activity's, so a strip of tabs from one
activity is readable at a glance:

| Activity | The tab's icon |
|---|---|
| Terminal | the shell profile's (PowerShell, cmd, Git Bash, WSL…), the CLI profile's when it was opened from one, the pod glyph for a shell inside a pod |
| Containers | the host's kind — Docker, Podman, Kubernetes, Windows containers |
| Graphics | the kind — Image, Icon, Diagram Editor, Diagram Generator |
| Devtools | the utility's, from the same table the sidebar lists from |

Two of these need the answer **kept in the tab's own saved state**, because
`tab_icon` is asked every frame and is handed no `Services`: the container
host's kind (`State::host_kind`, taken again on the first draw so a tab
saved before the field existed heals itself) and the terminal's CLI icon
key (`State::icon`, resolved once when the tab is opened by matching the
request's command against the CLI profiles — which is why no new field was
needed in `OpenRequest::Terminal`).

## 5. Environments in the UI

- The title-bar **environment switcher** shows the active environment with its
  colour. Switching emits an event.
- The **environment banner** under the tab strip shows **the open workspace's**
  active environment — the same for every tab. Production (or any environment
  flagged `protected`) also gets a 2 px coloured frame around the whole main
  area.
- **No tab is bound to an environment** (2026-10-25). A tab used to pin itself
  to whatever environment the connection it was opened from named: the banner
  then read "This tab is pinned to *prod*", and the tab's colour stripe came
  from that environment too. An environment is a property of the workspace, and
  two tabs in one window claiming different ones is a reading nobody asked for.
  `Tool::environment` is gone, so no tool can re-introduce the tie, and a tab's
  colour is now only the one the user gave it. The **connection profile** keeps
  its Environment field: that is what makes a production profile read-only
  everywhere, which is a guard, not a label on a tab. Owner's requirement.
- Protected environments: database connections default to read-only, API
  requests with non-GET methods ask for confirmation, SFTP/S3 deletes ask with
  the environment named, `git push --force` is disabled.
- **Environment marks** (FEAT-0106): every environment wears its tier's icon
  and colour (`hs_ui::envmark`) - in the title-bar switcher (closed box and
  rows), the banner, Settings → Environments (list rows and the Inherits
  From picker) and every connection's Environment picker. The tier is set
  by hand with the **Tier** field (Automatic (Matched: …) or a tier), else
  matched from the name (docs/06 §3):

  | Tier | Icon | Colour |
  |---|---|---|
  | Local | laptop | muted grey |
  | Development | code | blue |
  | Integration | plugs connected | teal |
  | Testing | flask | purple |
  | Training | graduation cap | green |
  | Acceptance | check square | amber |
  | Staging | stack | orange |
  | Production Support | lifebuoy | red-orange |
  | Production | shield warning | red |
  | no match | globe | muted |

  A **protected** environment always wears the red shield (warning shield in
  the theme's error colour), whatever its tier. The name keeps the
  environment's own colour. No tab carries a mark, since no tab is bound to
  an environment (above).

## 6. Shared widgets (`hs-ui`)

| Widget | Used by | Notes |
|---|---|---|
| `TabStrip` | shell | multi-row, drag, pin, context menu |
| `DataTable` | DB results, S3 listing, SFTP listing, API headers/cookies, container lists, process viewer | virtualised rows, column resize/reorder/hide, multi-sort (Shift+click adds, Ctrl+click removes), inline filter, cell copy, cell viewer popup, selection (row/range/cell), export menu |
| `Tree` | project tree, schema, collections, hosts, buckets, k8s | lazy children, inline rename, drag source/target, badges, context menu |
| `Palette` | shell | commands + quick-open |
| `EnvBanner`, `Badge`, `Toast`, `Dialog`, `ProgressBar`, `KeyValueEditor` (headers, params, variables with enable checkboxes), `CodeView` (read-only highlighted text with copy), `Breadcrumb` (paths, prefixes, URLs) | everywhere | |

### A table's own measurements (2026-11-03, CR-0028)

`hs_ui::table::ROW_HEIGHT` (26) and `HEADER_HEIGHT` (30) are the one pair
of numbers a table is built from — the `DataTable` call sites and the
Database result grid both take them, so a row is the same to read and
the same to hit wherever it is drawn. A header is a **band**, not a
first row in bold: `table::header_band` fills its cells with
`theme.surface` and rules them off, and the names are bold at 13.5. A
grid with a fixed gutter beside it (the result grid's row numbers)
paints the same band over the gutter's header, or the strip breaks where
the two meet.

Three more things follow from "a table says what it is":

- **A well for anything scrollable.** `form::well` and
  `form::scroll_well` draw a field-coloured, bordered panel — darker on
  a dark theme, lighter on a light one, both from `theme.field_bg()` —
  so a list or a block of output reads as something you scroll rather
  than as text that happens to stop. A pane that paints its own
  background (the Logs view, the editors) keeps it.
- **Item-and-value data is a table with no header.**
  `form::detail_table`: striped, two columns, rows a table's height, the
  name muted. The facts a detail pane lists are data, not a form. A list
  whose values carry their own formatting (monospace, a colour that
  means something) keeps the formatting and takes only the striping and
  the row height.
- **A text box in a settings card does not grow with its content.** A
  fixed height in a `ScrollArea` (`max_height` with
  `min_scrolled_height`), or a long list pushes the rest of the page out
  of reach.

Owner's report, 2026-11-03.

## 7. Keyboard

Global defaults (all rebindable via `HyperStackData/shortcuts.toml`):

| Shortcut | Action |
|---|---|
| Ctrl+T / Ctrl+W / Ctrl+Shift+T | new tab / close / reopen |
| Ctrl+Tab, Ctrl+1…9 | switch tabs |
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

Each entry is **one row, clickable along its whole length** — the logo,
the name and the space between them all switch workspace, rather than
only the few pixels of the name (owner's request, 2026-10-24). The Delete
button owns a fixed column at the right end and is interacted with after
the row, so it sits on top of it and the two never take each other's
clicks. The menu's width is **measured** from the longest workspace name
plus the logo, the padding each end and that Delete column
(`app::ws_menu_width`, capped at 420 px), so it is as wide as it needs to
be and no wider; egui's own sizing grew it from a row whose Delete button
claimed every pixel going and left most of the menu empty.

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
- Bundled schemes (`Theme::schemes()`, FEAT-0105), listed HyperStack first,
  then dark and light each A–Z, from each theme's published palette:
  - Dark: HyperStack Dark, Ayu Dark, Catppuccin Macchiato, Catppuccin Mocha,
    Dracula, Everforest Dark, GitHub Dark, Gruvbox Dark, Kanagawa, Material
    Palenight, Monokai, Night Owl, Nord, One Dark, Rosé Pine, Solarized Dark,
    Tokyo Night.
  - Light: HyperStack Light, Ayu Light, Catppuccin Latte, Everforest Light,
    GitHub Light, Gruvbox Light, Light Owl, One Light, Rosé Pine Dawn,
    Solarized Light, Tokyo Night Day, Tomorrow.
  - Every scheme keeps text at 4.5:1 on its window, panel and surface, and
    muted text and the accent at 3:1 on the window (tested). Where a
    published colour falls short it is darkened just enough, with a comment
    naming the original.
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
another workspace) · Environments · Security · Terminal · Connections ·
Editor · Browser (home page, blocking and script defaults, extra blocked
hosts) · AI Assistant (providers, models, Cascade rounds; ADR-019) ·
Shortcuts · About, which also holds **Updates** (installed version with sha/date/channel, channel, automatic
check, check now, release notes, download/install, revert; see
[09](09-ci-cd-and-updates.md) §5.1). The About page also shows a **Third-Party
Notices** section (compiled in from `THIRD_PARTY_NOTICES.md`, including the
Archify schema adaptation). The About → Installed card also shows the
data folder with **Open Data Folder**, which opens it in Eden Explorer when
that is running or installed, or in the system file manager otherwise; there
is no separate Data page any more (owner's request, 2026-10-11). Appearance
includes clickable color-scheme swatches, interface font and size, window-tab
width, editor file-tab width, and tab gap. The terminal and editor pages carry
their own font and size; three bundled Nerd Font Mono families are listed in
`assets/fonts/README.md`, along with fonts installed on the machine.

The **Security** page's credentials grid ends with an **Action** column,
right-most: **Reveal Secret** (eye) and **Edit Secret** (pencil), icons only,
with space between them. Reveal uncovers the row's password and key for 30
seconds after the master password. Edit asks for the master password, then
opens a dialog for the password and key; saving moves typed values into the
vault (owner's request, 2026-10-19). The **Environments** page edits an
environment's variables in place (Key, Value, secret toggle) and saves the
draft with **Save**; **Import** merges a Postman, Insomnia, Bruno or generic
JSON environment into that draft, and **Export** writes the selected
environment in one of those formats. Secrets are never exported in plaintext
(see [06](06-security.md) §addendum).

The **Terminal** page sits between Security and Connections: session defaults
above, then the **Terminal List** - every shell and CLI the Terminal
activity shows, in its order. A row is dragged to reorder or moved with
arrows; **Add CLI**, **Edit** and **Remove** (with a question) sit beside it,
and a **Suggestions** card offers commands found on PATH (mysql, psql,
mongosh, redis-cli, claude, opencode, ...) as one-click additions. A
disabled row stays in the list and leaves the activity sidebar. Owner's
request, 2026-10-19; details in [tools/terminal.md](tools/terminal.md) 5.

### 9.1 Visual rules

Settings → Git (2026-10-29, between Editor and Browser) is three cards:
Repositories (master list with drag reorder and an Add Repository /
detail form - source, name, icon, URL and credential for a host source,
path with Browse…, language; Save stays disabled until name, path and -
for a host - URL hold, with the hint naming what is missing), Git
Credentials (name, host kind with its default URL, host URL, username,
token through the vault; Remove asks with a second click), and General
(protected patterns, pull with rebase, sign-off, stash untracked - moved
here from the Editor page - plus default clone folder, terminal profile
and the Git Flow names). `docs/tools/git-activity.md` §3.

On every Settings page, every list that has a form (environments,
connections, containers, projects, repositories, credentials, CLI
profiles) is laid out as **two columns**: the list on the left at a fixed
width, the add/edit form on the right, and a hint on the right when
nothing is selected (owner's rule, 2026-10-30; the Environments page set
the shape).

Settings pages are built from `hs_ui::form`: a page title with a one-line
description, then cards (`section`) that hold a two-column label/field grid.
Text fields are tall (7 px vertical padding) and grow to the available width
between 260 and 560 px; hints sit under the control in muted text; notes and
instructions use a framed panel with an icon. Tooltips open above the hovered
widget (`TipExt::tip`) so the pointer never covers them. The window geometry,
sidebar width and dock height are restored on the next start
(`HyperStackData/window.toml`, Appearance settings). Owner's requests,
2026-10-11.

**Window Size (owner's request, 2026-10-23).** The Startup card ends with a
**Window Size** block: a **Size** picker and a live **Width and Height**.
The two numbers are the window's own inner size read from
`ViewportInfo::inner_rect`, so they follow the window as an edge is dragged
rather than being a setting of their own; typing or dragging either one
sends `ViewportCommand::InnerSize` and resizes the window there and then.
The picker (`window_state::SizePreset`) offers exactly two entries: **Full
Screen**, which maximises rather than sizing to the monitor so the taskbar
is left alone, and **Custom**, the window at the width and height beside it,
which also restores a maximised window to that size. Shares of the screen
(three quarters, two thirds, half, a default size) were offered on the day
this landed and taken out again at the owner's request the same day; do not
reintroduce them.

**The minimum is strict: 1720 × 860** (`WindowState::MIN_SIZE`, owner's
rule, 2026-10-23). It is the smallest layout every tool is designed for, so
it is enforced in four places and never merely clamped at one of them: the
viewport carries it as `with_min_inner_size`, so Windows refuses a smaller
drag; the Width and Height fields take it as their lower bound; a
`window.toml` written before the rule, or by hand, is replaced with the
default rather than kept (`sanitised`); and `HyperStackApp::enforce_min_size`
reads the live size every frame and pushes back, because Windows applies
`with_min_inner_size` only to user-driven sizing and lets a `SetWindowPos`
from another process straight through. A maximised, fullscreen or minimised
window is left alone there, and a pixel of tolerance stops it arguing with
the window manager. `DEFAULT_SIZE` is the same pair, so a first start opens
at the minimum. Nothing in this block is persisted
separately; the geometry still lands in `window.toml` the way a dragged
resize does.

### 9.2 Prompt Buttons (owner's request, 2026-10-23)

Every activity's assistant offers a row of one-click prompts. They were
written into each tool, so changing one meant editing the source.

The defaults still live in code, in `hs_core::prompts::DEFAULTS`, because
that is what makes them shared and translatable: a row names the activity,
a stable id, and the two Fluent keys. Settings → AI Assistant → **Prompt
Buttons** then lets the user pick an activity and rename a button, rewrite
what it asks, drag it into another order, turn it off, add one of their
own, or reset the activity to what it ships with. A tool calls
`QuickAction::for_activity(s, "logs")` and draws the result.

The settings are **global, not per workspace**: a prompt is how someone
likes to ask a question, which does not change because they opened a
different project.

Three rules keep an upgrade from losing anything:

- **Only differences are stored.** A button the user merely reordered or
  hid keeps empty `label` and `prompt`, so a later translation still
  reaches it.
- **A newly shipped button is appended, never dropped.** The saved order
  leads; anything in `DEFAULTS` that the saved list has not seen follows
  it, so upgrading brings new prompts with it.
- **A button the user added is marked as theirs** and is the only kind
  that can be deleted; a shipped one is hidden instead.

The editor and the Scratchpad are deliberately absent from the activity
list: their assistants have no prompt buttons, and listing an activity
whose edits would do nothing is worse than not listing it.

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

Labelled entries are 50 px tall. Since 2026-10-22 (thirteen activities)
the rail compacts them to 44 px with no gap *before* it scrolls, when the
full size would not fit: every activity stays visible on a 960 px window,
and only a shorter window scrolls the middle section.

## Addendum (2026-10-23): the rail and the sidebar widths belong to the workspace

The rail's **order and visibility** and every activity's **sidebar width**
are stored per workspace, in `workspace.toml` under `[rail]`
(`hs_core::workspace::RailPrefs`: `order`, `hidden`, `sidebar_widths`), not
in the global settings. A workspace without a `[rail]` table is seeded
from the global Appearance values the first time it is shown, so nothing
moves for an existing data folder; the global fields stay only as that
seed. Switching workspaces switches the rail and the sidebar widths with
it (the sidebar panel's egui id carries the workspace name, so egui's own
remembered size cannot leak across). Whether the rail shows labels, and
whether its icons are coloured, stay global.

Settings → Appearance → **Activity Rail** starts with a **Workspace**
picker (logo and name of every registered workspace, the open one
preselected); the Order and Show list under it edits the picked
workspace and writes its `workspace.toml` at once, whether or not that
workspace is open. Settings → Appearance gained a **Sidebar** card between
Interface Font and Tabs with one switch, **Coloured Activity Icons**: off
by default, the rail icons take the theme's text colour (muted when
inactive); on, each activity keeps its own hue (`app::activity_colour`).
The Workspaces list on Settings → Workspace draws a 26 px logo and a
15 px name per row.

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

## Addendum (2026-10-25): Edit This Connection, and Connect

Three changes that close the loop between the sidebar a connection is used
from and the Settings form it is kept in. Owner's requirements, 2026-10-25.

- **Every connection row's context menu opens with Edit This Connection…**,
  above Pin to Home. It takes the user to Settings → Connections with that
  profile already selected in the form; the Container activity's hosts say
  **Edit This Host…** and land on Settings → Containers the same way. The
  menu is built once in `hs_ui::form::row_menu`, so no list can be left
  without it — the reason Pin to Home lives there too.
- The shell reaches the form through the commands `settings:connection:<id>`
  and `settings:container:<id>`, which open the page and then select the
  profile (`app::select_connection_in_settings`). Selecting it is the point:
  opening the page alone leaves the user to find in a list the row they had
  just right-clicked.
- **Connect sits after Test Connection** in both forms. It saves the draft
  and then opens the profile in its own activity, focusing the tab that
  already shows it or opening a new one. Save and Connect share
  `settings_ui::save_draft`, so what opens is what reached disk — and a
  draft whose secrets could not be stored (a locked vault) is not opened,
  because it was not saved. A kind no activity handles leaves the button
  disabled and says so, as Test Connection does.
- The form is **one column**. Its sections — Details, then the kind's own —
  are stacked in a `vertical` of their own inside the page's horizontal
  split; drawn straight into that split they became two columns side by
  side, with the kind's fields beside Details instead of under it.

## Addendum (2026-10-17): the assistant sits hard right

Every tool's **AI Assistant** button is the last thing on its toolbar, in a
`right_to_left` group, so it is in the same place in the editor, the
terminal, SSH, SQL, S3, the API client, Graphics and the Scratchpad. The
buttons that act on what is selected keep the left; the assistant is a mode
for the whole tab, not an action on a row, and it reads that way when it is
not sitting among them. Owner's requirement, 2026-10-17.

## Addendum (2026-10-22): narrow windows

The shell lays every tool toolbar out in a **wrapping** row
(`horizontal_wrapped`), so a window too narrow for the editor's or the
diagram canvas's buttons moves the overflow onto a second row instead of
clipping it off the right edge. A tool ends its hard-right group with
`hs_ui::toolbar::hard_right`, which starts a fresh row when the current one
has too little room left for it; a bare `right_to_left` scope would get
only the width left on the row and the assistant control would vanish
first.

Panels give way before content does: the shared assistant column and the
Graphics assistant column shrink to keep 320 px for the tool and step aside
below 580 px of tool width; the sidebar's and the dock's upper bounds follow
the window (half the width, 60 % of the height) rather than fixed pixel
maximums, so a size saved on a wide monitor cannot keep most of a small one;
the split-view divider refuses to move once both halves are at the 120 px
minimum.

## Addendum (2026-11-04): Start Here's primary button on the light theme (CR-0045)

A dashboard's **Start Here** row fills its first button with the accent;
its icon and label are **white** on it in both themes, as
`form::primary_button`'s are. They used to take the theme's text colour,
which is dark on the light theme and sank into the purple. The Scratchpad's
Add Note and the Editor's Add Project Folder draw their own Start Here
button and follow the same rule.

## Addendum (2026-11-04): product logos for connection kinds (CR-0036)

A connection kind wears its **product's multi-coloured logo** everywhere it
is drawn: the activity sidebars, Settings' lists, the dashboards' saved
tiles, the tabs and the work-pane header band. One function decides it,
`hs_ui::icons::conn_icon(kind, engine)` (`profile_conn_icon` for a saved
profile), returning a `ConnIcon`: the logo's `bytes://product/…` URI, or
the glyph and how to colour it. The logos are Devicon v2.18.0 (MIT),
vendored in `assets/icons/devicon/` and registered once by
`install_image_loaders`; the folder's README lists which kind takes which.

- The same goes for an **SSH host's operating system** (`os_conn_icon`:
  Ubuntu, Debian, Fedora, Red Hat, CentOS, Arch, openSUSE, Linux, Windows,
  and macOS as the **rainbow-striped Apple**, not the black one), a
  **terminal shell** (PowerShell, Git Bash, bash, zsh, fish; a WSL shell its
  distribution), a **CLI** by the command it runs (`cli_conn_icon`: `psql`,
  `kubectl`, `az`, `gcloud`, `gh`…, falling back to the category the user
  picked), and an **Editor project's language** (`language_conn_icon`) in
  the project list, the tab, the header and the dashboard.
- A **black logo** (GitHub, Kafka, MySQL, MariaDB) would vanish on the dark
  theme, so those keep their glyph in the theme's **text** colour; Rust,
  Lua and Markdown use the file tree's coloured Material icons instead of
  Devicon's black or navy ones.
- A product with **no logo** in the set keeps its glyph in the product's
  colour: S3, Qdrant, MQTT (a lighter step of its purple), Redmine, Alpine,
  FreeBSD, Nushell.
- Valkey, WebDAV, Nexus, cmd and the API client keep their glyph as before:
  no product logo or colour to give them.
- The shared drawers take it: `form::connection_row` / `conn_list_row`,
  `dashboard::Entry` (`image`, `tint`), `ToolHeader` (`icon_image`,
  `icon_hex`, `icon_plain`) and the `Tool::tab_icon_uri` /
  `tab_icon_colour` hooks.
- The **Git** activity's work-pane header shows a repository's uploaded
  icon, as its list row and tab already did.

## Addendum (2026-11-04): Remove From List on a connection row (FEAT-0094)

Every connection row's context menu (`hs_ui::form::row_menu`, so all
nine sidebars — SSH, Containers, Database, NoSQL, Redis, S3, Queue,
Automation and Tracker — get it together) has **Remove From List**
directly under Edit This Connection / Edit This Host. The row only
reports the choice; the shell (`connection:remove:<id>`) asks first, in
a danger dialog whose body says the profile **and its stored
credentials** are deleted and its tabs close — unlike Remove From List on
a project or repository, which only forgets a folder. Yes deletes through
`Connections::remove_with_secrets` (docs/06-security.md §2), closes the
profile's tabs and clears it from Settings if it was open there; Cancel
changes nothing. A locked vault refuses with a toast that asks for it to
be unlocked. Owner's requirement, 2026-11-04.

## Addendum (2026-11-04): Add lands on a focused form, Save opens it (FEAT-0093)

Every Add in an activity's sidebar (and its palette `*.add` command) goes
straight to the Settings form that adds one, and the form takes the
keyboard. Owner's requirement, 2026-11-04.

- **SSH, Database, NoSQL, Redis and S3** send `settings:connection:new:<kind>`
  (`ssh`, `db`, `mongodb`, `redis`, `s3`): Settings → Connections with an
  empty form already set to that kind. The command picks the page from the
  kind (`settings_ui::page_for_kind`), so a container, queue, automation or
  tracker kind lands on its own page.
- **Containers, Queue, Automation and Tracker** have one form per kind:
  their Add (sidebar and dashboard) sends `settings:<page>:new`, which
  opens an empty form of the page's **first kind** (Docker, RabbitMQ,
  Jenkins, Jira) with the kind's defaults and Name focused. This replaced
  landing on the kind buttons with no form open (CR-0044).
- Those four pages have **one Add button** above their list — Add
  Container, Add Broker, Add Automation Server, Add Tracker — in place of
  a button per kind, as Settings -> Connections has New Connection. The
  form's **Kind** field lists only that page's kinds (`kind_family`), and
  switching it on a new profile takes the new kind's defaults, and its
  "New …" name if the old one was still there (`new_kind_draft`). An
  existing profile keeps its fields when its kind changes (CR-0044).
- **Manage** jumps land on a list, not a form: Manage Projects
  (`settings:editor:projects`), Manage Repositories (`settings:git:repos`)
  and Git Credentials (`settings:git:credentials`) scroll their section to
  the top of the view (`SettingsState::focus_section`). Add CLI on the
  Terminal dashboard opens the CLI form with Name focused (CR-0044).
- Each Add button names what it adds: Add SSH Host, Add NoSQL, Add Redis,
  Add S3 Profile, Add Automation Server, Add Note — the dashboards no
  longer share a generic "Add Connection" (CR-0044).
- **Editor** (Add Project Folder, and the strip's `+`) opens Settings →
  Editor with an empty project form; **Git** (Add Repository, and the
  strip's `+`) opens Settings → Git with an empty Add Repository form.
- The focus is one flag, `SettingsState::focus_form`, set by the jump and
  taken by the first form drawn: its Name field is scrolled into view and
  given the keyboard. A page whose form waits for a kind takes it on its
  list instead.
- **Save on a new profile closes the form** and opens the profile in its
  own activity, as Connect does; saving an edit keeps the form open, as
  before. The connection form gains **Close**, beside Save, which discards
  the draft. Projects and repositories already behaved this way; a new
  repository is also registered as an Editor project. Git credentials
  close on Save and open nothing, having no tab of their own.
- The other strips' `+` (API Client, Graphics, the Scratchpad) make a new
  document and are unchanged; the connection activities hide theirs.

## Addendum (2026-11-03): one sidebar header (CR-0030)

Every activity's sidebar opens with `hs_ui::form::sidebar_head`, in the
shape the Containers sidebar set: the title in its bar, then one row of
**labelled** actions with air above and below, then a rule before the
list. The first action is the primary one (accent-filled); a second —
S3's Import, Database's Refresh Schema, the Scratchpad's Add Folder —
sits beside it as a framed toolbar button. A sidebar never repeats its
title as a subtitle under the buttons.

`form::sidebar_action_row` is that band on its own, so a sidebar can put
something else in the same place: the Scratchpad's folder-name form
replaces the buttons while it is open and gives them back on Create or
Cancel. The folder is named before it exists, so Cancel leaves nothing
behind. Home keeps its own header, with the workspace's logo.

## Addendum (2026-11-03): one toolbar standard (CR-0027)

Everything a toolbar draws comes from `hs_ui::toolbar`, so the strip reads
the same in every activity:

- **Metrics.** `strip_style`, applied once where the shell hands a tool
  its row, sets `button_padding` to 10 × 6 and `interact_size.y` to
  `form::field_height`. A button, a dropdown and a status chip in a
  toolbar are therefore the same height as a form field, and the sidebar
  column gets the same padding so its buttons match.
- **Colour.** `item_text` builds an item's text in two colours — the icon
  in the accent, the label in the ordinary text colour — and `btn`,
  `btn_enabled`, `tab` and `toggle` draw with it. A disabled button mutes
  both: a `LayoutJob`'s own colours survive egui's dimming of the widget
  around them, so muting has to be explicit.
- **Every button says what it does.** An icon with no label is a
  guess-and-hover; the label is the control and the tooltip is the
  detail. The editor's find bar takes its words from the app through
  `hs_editor::view::FindLabels`, the way the tab strip takes
  `TabStripLabels`, because the editor is a library with no locale.
- **Status** is a chip (`toolbar::status`), not loose text: the mark in
  the state's colour, the text beside it, the strip's padding around
  both.
- **Rows.** `hard_right` pins the assistant to the **first** row — one
  row tall, whatever the strip grew to — and `row_break` ends a row with
  a rule across the toolbar's whole width. A sub-toolbar with a text
  field in it (the terminal's find bar) draws inside its own strip after
  the break: a `TextEdit` in a wrapping row takes the width that is left
  rather than asking for its own, and after a row break there is none.

Owner's report, 2026-11-03.

## Addendum (2026-10-17): Home's sidebar, and where the buttons go

**Home's sidebar is "where you were".** The page answers *what can I do* —
Get Started, the tool grid, the shortcuts — so the column beside it answers
*what was I doing*: the workspace's name, the connections you pinned, and
the tabs you closed, most recent first. Recent used to sit at the very
bottom of the page, under the tool grid, which is backwards: reopening
yesterday's work is the commonest reason to be on Home at all. With no
tabs open there is no sidebar, so that view draws the same column under
the page instead. Owner's requirement, 2026-10-17.

**Get Started is in setup order**: New Workspace, Environments, New
Connection, Open Folder as Project, New Terminal, Quick Open — a workspace
to put things in, the environments they vary by, the connections that use
them, then the day's work. Owner's order, 2026-10-17.

**Dialog buttons go on the right, action outermost.** Every modal in the
app does this; three had been left-aligned (the SQL guard, Save Query, and
S3's New Folder) and now match. A *form's* action row — Settings →
Connections' Save / Test Connection / Delete, an inline editor's
Save / Delete / Cancel — stays left-aligned: it reads as part of the
content flow rather than as the end of a conversation.

### A control beside a text field (2026-10-17)

A `ComboBox` or a button put next to a text field comes out **shorter**
than it: the field carries `form::FIELD_MARGIN` (7 above and below) and
the control carries egui's own `interact_size`. `ui.horizontal` centres
them, so neither edge lines up and the row reads as crooked — which is
what it was in the API client's method picker beside the URL.

`form::field_row` runs its contents with `interact_size.y` set to
`form::field_height`, so a control drawn inside it is exactly as tall as
the field beside it. Use it for any control sharing a row with a text
field. Owner's report, 2026-10-17.

### A form row: one control, label level with it (2026-11-03)

`form::field_grid` does for a whole form what `field_row` does for one
row: it runs its cells with `interact_size.y` at `form::field_height`, so
every dropdown, checkbox and button in a form matches the fields around
it without each call site asking.

A form row holds **one** control. egui centres a grid cell in its row
(`Align2::LEFT_CENTER`), so a cell that stacks a description under its
control makes the row tall and drags the label opposite it down to the
middle of the pair — which is how the Automation Run tab read. A
description goes on a row of its own, through `form::field_hint_row`. A
cell that genuinely holds several controls (Settings → Workspace's name
field with its Rename button) keeps centring on the stack, which is
right. Owner's report, 2026-11-03.


### The tab menu: no splitting, and Duplicate knows when to refuse (2026-10-17)

**Split Right / Split Down left a tab's context menu** on 2026-10-17, with
their default bindings (Ctrl+\ and Ctrl+Shift+\), because splitting
rearranges the whole tab space and that is not what someone right-clicking one
tab is reaching for. The palette commands stayed then; splitting left the app
altogether on 2026-10-23 (§2.4).

**Duplicate is offered but disabled** where a second tab would be a lie.
Two rules decide it, both on the `Tool` trait so a new activity type
inherits them rather than being added to a list:

- `Tool::current_connection` already named the connection profile a tab is
  bound to. A tab bound to one is a view of a live session, so duplicating
  it starts a second session against the same server. `tabs_per_connection`
  says how many are allowed — one by default, **three for SSH**, where
  separate shells on one host are how people work.
- `Tool::can_duplicate_tab` refuses outright for a tool whose tab *is* the
  thing it holds rather than a view onto it: **Graphics**, where a tab is
  one open image.

The entry is disabled rather than hidden, with the reason on hover. A menu
item that silently disappears reads as a bug; one that is present and
greyed reads as a rule.

## Addendum (2026-10-19): the Graphics activity

Graphics stops being a picture tab with a settings sidebar. It becomes an
activity with four tools and a permanent dashboard home tab, following the
Devtools pattern: every Graphics tab draws the same activity sidebar, so the
list does not change as the user moves between an image, an icon, a diagram and
a generator. The full requirements are in
[tools/diagrams.md](tools/diagrams.md).

**Sidebar.** `Image`, `Icon`, `Diagram Editor` and `Diagram Generator`, each a
clickable row with a right-click menu (Open, New, From Example, Import Mermaid,
Convert to Icon). Below a separator, **Recent** takes about half the remaining
column height, scrolls on its own (stable `id_salt`, `auto_shrink([false,
false])`), is left-indented, and shows an icon, a title and a Draft/saved badge.
Clicking a row focuses the tab whose `current_uri` matches, or opens the saved
file/draft. The outer sidebar is not one scroll area wrapped around another:
the function list takes its natural height and the Recent area gets a fixed
maximum.

**Permanent home tab.** A dashboard: “What To Do” cards (Open Image, Open Icon,
New Diagram, Generate Diagram), the four functions with one-line descriptions,
and the shared `hs_ui::dashboard::open_tabs` grid of Graphics tabs with their
last screenshot. It reports no `current_uri` and no `uri_affinity`, so opening
a picture never consumes it. `home_is_dashboard()` is true; an old saved
layout with an image as `home` is migrated by clearing that flag and inserting
a dashboard.

**Image and Icon tabs.** The picture stays the whole tab, but the settings move
into a resizable in-pane panel on the left (280–320 px), preview on the right,
so the activity sidebar can keep the function list. The Icon tab adds frames,
ICO sizes, a pixel grid at high zoom and colour-key transparency (eyedropper +
tolerance). The assistant remains the last toolbar button, hard right.

**Diagram Editor.** Toolbar: Select, Shape, Connector, Text, Pan, Zoom, Grid,
Align, Undo/Redo, Export, AI. Properties panel on the left (geometry, style,
text, z-order, align/distribute) with an **Outline** list of nodes and edges —
the keyboard and screen-reader path to the canvas. Canvas in the centre.
Double-click edits a label in place; drag handles resize; Space+drag or the
middle mouse pans; Ctrl+wheel zooms.

**Diagram Generator.** Type picker and prompt on the left (360 px) with three
clickable examples and a template button; preview on the right; “Open in
Editor” and “Export” above the preview. With no AI provider configured,
templates and examples still produce diagrams and a hint links to Settings → AI
Assistant.

**AI assistant.** Four quick actions per tool plus **More Suggestions**, which
asks for three tailored actions and shows them as buttons. Review-only: the
assistant never applies an edit. Image and Icon context is text-only in R1
(metadata and the operation list, never pixels).

**Implemented 2026-10-19.** The activity sidebar, the dashboard with Recent and
the screenshot grid, the separate Image and Icon tabs, the in-pane settings
panel and Convert to Icon are in the app and verified live. Colour-key
transparency is implemented in the Icon tab (eyedropper, tolerance, Make
Transparent). The Diagram Editor pane is implemented too: a properties/outline
column, a pan/zoom canvas over the shared scene, selection/drag, shape tools,
connect mode, undo/redo, templates and save/export. The Diagram Generator pane
is implemented as well: a prompt column with examples and the offline
template, an async AI call into the strict IR, and a preview with Open in
Editor and export.

## Addendum (2026-10-22): per-activity sidebars and a coloured rail

Each activity's sidebar (Settings included) keeps its own width:
`appearance.sidebar_widths` is keyed by activity id, the panel's egui id is
`("hs-sidebar", <activity>)` so egui's remembered size is per activity as
well, and an activity never dragged starts at the shared `sidebar_width`.

The rail icons carry one colour per activity (`app::activity_colour`),
muted while inactive and full while active or hovered, so the column reads
at a glance. Tab tooltips name the activity ("API Client"), not its id.

Splits were kept at this point, with a **Close Split** command so a split
could be ended without closing a tab. They were removed outright on
2026-10-23; §2.4 says why.

Every AI Assistant column goes through `hs_ui::ai_assistant::split_for_assistant`
now, the Scratchpad, the API client and the Graphics picture tabs included;
a hand-cut 390 px column read as a floating card and ignored the window.

## Workspace switching (2026-10-29)

Switching workspaces keeps the workspace being left **alive**: its
`TabSpace` - every tab and the tool behind it - is parked in the shell
and taken back out when the workspace is returned to, so a logged-in
browser page, an SSH shell or a database connection survives the round
trip. The target's tabs come from its parked space when it has one and
from its saved layout the first time it is opened in a session. Layouts
are still written to disk at every switch. Tab ids are process-wide, so
parked and live tabs never collide in the browser host, the open-tabs
registry or the thumbnails.

## Settings -> Environments: three cards (2026-10-29)

The page's left column stacks three list cards in one shape - a list of
rows, the selected one opening its detail in the right column - and the
right column shows exactly one of them at a time: **Environments** (the
workspace's named sets, with secrets), **Collection Variables** (the
API collections of this workspace, each editing its own variable list),
and **Global Variables** (the one application-wide set). Picking a row
in any card clears the other two selections. Collection and global
variables use the same `KvEditor` as an environment, without the secret
toggle, and Save writes straight to the collection file or
`globals.json`. A collection's detail also has **Compare With…**, the
environment card's copy dialog reshaped for collections: a source
workspace and collection (any workspace in the registry, read from
disk), each key tagged New / Same / Differs with both values shown,
Copy Selected for the ticked keys, and **Sync All**, which makes every
value the source has read the same in this collection while keeping the
keys only this one has - a sync that deleted would be a surprise. The
dialog edits the draft; Save writes it.

## Addendum (2026-11-05): Settings polish, and the standard written down (CR-0038)

Owner's rules from the v0.7.6 batch, so the next activity follows them
without being told (the agent's copy is `.claude/rules/ui.md`):

- **A Settings card is as wide as its content needs.** `form::section`
  caps a card at `CARD_MAX_WIDTH`; a card holding a wide table uses
  `form::section_max(…, f32::INFINITY, …)` so it takes the page rather than
  scrolling sideways. Security → Credentials is the one so far. A table
  column that no row fills is not drawn (its Key column, with no SSH key
  on file).
- **A requirement is stated where the choice is made.** The database form
  tells the user, as a field hint every time Oracle or Db2 is the engine,
  that the engine connects through ODBC and needs that vendor's client on
  this machine; the warning that none is installed stays beside it.
- **Home's toolbar follows the rail's label setting of its own.** Settings
  → Appearance → Activity Rail gains *Show Tool Names Under Home's Toolbar
  Icons*; on, every icon in Home's toolbar is one fixed width (76 px, a
  6 px margin either side of a one-line, truncated name under a 22 px
  glyph), so the row stays a grid whatever the names' lengths. Off (the
  default) it is the 34 px icons-only row of §3.
- **A field the server can fill is a `pick_field`.** Where the server
  knows a field's values - a repository's refs, a project's people, its
  priorities and types - the control is `hs_ui::form::pick_field`: a text
  field with the list behind a caret, typed or chosen, which is a plain
  text field when the list is empty. Dates are
  `hs_ui::datefield::date_field`, a `YYYY-MM-DD` field with a calendar.
- **Folders before files, each by name**, in every list that mixes them
  (the Redis key list, CR-0037).
- **Anything a user makes several of takes a name** (SSH forwards,
  FEAT-0098), optional, shown first in its list.
- **A sub-tab keeps its own state** (a Database query tab its results,
  FEAT-0096): what a tab shows is what that tab did, not what happened
  last.
- **A server's log is laid out in its steps** before it is shown
  (`tool_automation::logfmt`, FEAT-0095): banners, groups, commands, body.

## Addendum (2026-11-05): table cells, the calendar's width, S3 kind colours (CR-0046)

- **Tables.** `DataTable` and the database grid lay every cell out with
  `hs_ui::table::cell_layout()` - left to right, centred on the row - so
  text sits in the middle of its row; the header is `HEADER_HEIGHT` 34 px
  with `HEADER_TEXT` 14.5 px bold, air above and below the words.
- **The calendar** (`hs_ui::datefield`) is `CALENDAR_WIDTH` wide: a menu
  popup otherwise takes the justified width of the form it opens from.
- **S3 kinds.** Nexus and WebDAV have no vendored logo, so they wear a
  colour of their own (Sonatype green, folder blue) like S3's orange, and
  the S3 activity's connection tabs take the profile's icon and colour
  through `Tool::tab_icon*`, as the other connection activities do.
- **Home's toolbar** with names on: 2 px between buttons; each button's
  own margin is the air.

## Addendum (2026-11-05): Home's Recent rows (CR-0048)

Each row in Home → Sidebar → Recent draws its activity's glyph in the activity's colour (`app::activity_colour`), as the rail and Home's toolbar do, before the tab's title.

## Addendum (2026-11-05): dashboard marks (CR-0049)

- Home's tool list (What do you want to do) colours each activity's glyph with `app::activity_colour` while Settings → Appearance → Sidebar → Coloured Activity Icons is on; off, the accent as before.
- A saved-card (`hs_ui::dashboard::item_button_art`) centres its icon or picture on the middle of its two text lines; with one line the icon stays level with it.
- The API client's collection cards carry the sidebar's `folder-api` Material icon as their picture.

## Addendum (2026-11-05): no growth on hover (CR-0050)

`Theme::apply` sets `widgets.hovered.expansion`, `widgets.active.expansion` and `widgets.open.expansion` to 0: egui's default grows a hovered or pressed widget by a pixel on each side, which read as rows and buttons magnifying under the pointer (the Redis key column was where it was noticed). Hover is the highlight colour alone, in every activity at once (CR-0026's rule, now enforced in the one place every widget reads).

## Addendum (2026-11-06): one layout standard (CR-0061)

The rules for tables, inputs, widths, alignment and pane spacing are
collected in `.claude/rules/ui.md` → *Layout and controls standard*, with
the numbers and the `hs_ui` helpers that carry them. New in this item:
`hs_ui::table::edit_table` for rows of inputs (the table standard's header
band, bold header, centred cells, field-height rows, column widths that
show the value), `hs_ui::form::line_edit` for every single-line field,
`form::match_field_height` for a row that mixes fields and dropdowns,
`form::pick_field_width` for a pick field in a cell, and `form::pane`
with `PANE_MARGIN` (12) and `ROW_GAP` (10) beside `PANE_GAP` (8). The
whole app moved onto `line_edit` (124 fields) and its loose dropdowns
onto the field height in the same change.

## Addendum (2026-11-06): header band, centred rows, history menus, tab focus (CR-0061, CR-0067, CR-0068, BUG-0047)

Written in full in `.claude/rules/ui.md`; in short: every table header is one
accent band across the full row with titles indented by `HEADER_INDENT`, and
an empty table says so under it; a row mixing a label and a field opens with
`form::centred_row` so the label sits on the field's middle line; every
History / Recent list has a right-click Clear History and Remove, and every
saved item a Delete, each behind a confirmation and changing what is stored;
any action that opens or fills a tab brings it to the front; list rows wear
the Material picture their tab wears when Material icons are on.

## Addendum (2026-11-07): row middle lines, tables in their cards, Open Here marks (CR-0084, CR-0085, CR-0087)

Written in full in `.claude/rules/ui.md`; in short:

- **A list or tree row is one middle line** (CR-0084): the row's height is
  set to its tallest item before it opens, so caret, picture, name and
  badge line up - the Redis key tree, NoSQL collections, API Client
  collections and folders, Database saved queries and history, Scratchpad
  and Source Control folders, the Containers host title. A status line (a
  count, "Scanning…", a result summary) has `form::ROW_GAP` above it.
- **Tables stay in their cards** (CR-0085): `table::begin_header_band` is
  bounded by `ui.max_rect()` as well as the clip rect, so the band ends at
  its card; a `DataTable` with no columns draws nothing and leaves the
  empty state to say why.
- **A group's grip is on its header** (CR-0085): a row that opens below
  itself (a collection, a folder) draws its drag grip with
  `reorder::Reorder::group_row` and the header's height.
- **Open Here cards wear their tab's mark** (CR-0087): `hs_core::open_tabs::TabInfo`
  carries `icon`, `icon_uri` and `icon_colour`, filled by the shell from
  the same tab views the strip draws, and the shared `hs_ui::dashboard`
  Open Here card draws a 20 px mark on the middle line of its title and
  subtitle - a logo, a tool's picture, a Material icon or a coloured
  glyph, exactly as the tab shows it - in all 17 activities with an Open
  Here section.

- **A page title wears its tab's mark** (CR-0088): `form::page_title_marked`
  leads a work-pane title with the same coloured picture its tab wears,
  on the title's middle line, with a one-line description under it.
  Every Devtools tool and API Client's Manage Collection use it.

## Addendum (2026-11-07): browse columns, previews, log tables, shell focus, broker picks, variable tooltips (CR-0089, CR-0091 to CR-0094, CR-0090, FEAT-0107, FEAT-0108)

- **Open Here shows the tab's coloured prefix** (CR-0089):
  `hs_core::open_tabs::TabInfo::prefix` carries the coloured piece of a
  tab's title (an API request's method); the Open Here card draws it
  before the title in its colour, the title in the normal text colour.
- **A browse column can be hidden and resized** (CR-0091): a column that
  lists what a pane browses (S3 buckets, Redis keys, the NoSQL databases)
  is a resizable panel, with a show/hide toggle as the first toolbar item
  (CR-0073) and a palette command. Hidden, it takes no width.
- **An item's content sits beside its list** (FEAT-0108, CR-0093): a
  clicked item that can be shown (an S3 object's text, image or Markdown;
  a NoSQL document's JSON) opens in a resizable column to the right of the
  list, not under it. A preview reads a capped range, can be cancelled,
  is cached on what identifies the version (an ETag), and has empty,
  loading, error (with Retry) and "too large" states.
- **A log table without levels still wears the level colours**
  (CR-0092): a table of timed entries (Redis Slow Log) maps each entry to
  a level by a stated threshold and colours only that cell
  (`DataTable::show_coloured`) with the shared log-level colours; the
  clicked row's detail sits under the table.
- **A shell takes the keyboard when its tab comes to the front**
  (CR-0094): Terminal and SSH focus the shell on the frame its tab
  becomes active, once - never while a dialog or another field
  (`form::focus_is_elsewhere`) has the keyboard.
- **A field the broker can list is a pick field** (CR-0090): the rule of
  CR-0038 holds for brokers - a RabbitMQ exchange and routing key, a NATS
  subject, a Kafka partition are `form::pick_field`s filled from the
  broker, and plain text fields when it lists nothing.
- **A `{{variable}}` shows its value on hover** (FEAT-0107): every shared
  field (`varfield::var_field`, `KvEditor`, `form::text_field` /
  `code_field`, the editor engine with `EditorView::var_tips`) shows a
  tooltip with the reference and its value through `hs_ui::varpeek`,
  installed once by the shell. A secret is shown masked and never
  decrypted for a tooltip; a locked vault and an unset name say so.
