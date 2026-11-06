# UI and UX rules

One UI language across every tool. If you are inventing a control, stop and
look for it in `crates/hs-ui/src/` first.

## Shared components (use these, don't re-roll them)

`hs_ui::form`, `hs_ui::dashboard`, `hs_ui::toolbar`, `hs_ui::table::DataTable`,
`hs_ui::tabs`, `hs_ui::tree`, `hs_ui::palette`, `hs_ui::dialog`,
`hs_ui::toast`, `hs_ui::kv_editor`, `hs_ui::env_banner`, `hs_ui::badge`,
`hs_ui::tip`, `hs_ui::varfield`, `hs_ui::ai_assistant`, `hs_ui::theme`,
`hs_ui::icons`.

- Never a bare `TableBuilder` for a wide table — use `DataTable`.
- The AI assistant control is always the hard-right toolbar item, through
  `hs_ui::toolbar::hard_right`. Never a bare right-to-left `with_layout` in a
  toolbar.
- Dashboards and lists share section headings and top spacing. Centre list-card
  icons and labels vertically. Row actions go in a fixed, aligned column.
  An action that cannot apply to a row (Move Up on the first) is drawn
  disabled, never left out, so the column stays aligned (CR-0061).
- **An empty table says so:** an editable or read-only table with no rows
  shows its header and a one-line sentence-case hint under it ("This
  table has no indexes yet."), never a bare header (CR-0061).
- **History and recent lists can be cleared:** every History, Recent or
  similar list in any activity has a right-click menu on its heading and
  rows with Clear History (and Remove for one row), behind a
  confirmation; it clears what is stored, not only what is shown
  (CR-0067).
- **Saved items can be deleted from their list:** a saved query, request,
  command, source or any saved thing has Delete in its right-click menu,
  behind a confirmation, in every activity (CR-0067).
- **A label is shown whole:** a button or card that shrinks to fit a row
  never goes narrower than its longest label; past that the row wraps.
  An ellipsis is only for user data that can be any length, never for
  the app's own names while there is room (owner, 2026-11-06: the Home
  rail cut "Containers" and "Automation").
- **Many of one kind, told apart by colour:** where a list holds many
  saved items of the same kind with the same glyph (Logs saved sources,
  and any list like it), each item wears a colour from a rotating palette
  of eight, assigned by its position so it is stable across restarts and
  two neighbours never share one. The same colour follows the item
  everywhere it is drawn - its sidebar row, its open tab's icon, its
  dashboard card (CR-0059).
- **A pane is never narrower than its tabs:** a resizable pane or sidebar
  that holds a row of tabs (segmented Project / Source Control, the
  Browser's Favorites / Logins, …) cannot be dragged narrower than the
  width that shows every tab's icon and label whole; nothing of one tab
  is ever drawn over another. Measure it with
  `hs_ui::tabs::segmented_min_width` and use it as the pane's minimum
  (CR-0069).
- **A side pane in a work pane can be hidden:** like the Editor's
  project column, a side pane that is not the work itself has a
  toolbar toggle that hides it, where the activity has room for one
  (CR-0069).
- **Menu items carry a glyph:** every item of a right-click menu we build
  has a glyph before its label (Remove and Delete a trash can, Clear a
  broom, Open an arrow), the same glyph for the same verb in every
  activity; an item that cannot act (Clear History on an empty list) is
  disabled, not hidden (owner, 2026-11-06).
- **A selected row stays readable:** a selected table or list row keeps
  the normal text colour on the selection fill; never accent text on an
  accent fill (owner, 2026-11-06).
- **A tab's title never runs under its close button:** the title
  truncates before the close button's slot, with a gap (owner,
  2026-11-06).
- **A panel's content scrolls rather than clips:** a list inside a
  fixed-height panel (the SSH Kept Forwards) scrolls when it is longer
  than the panel; a row is never cut off at the panel's edge (owner,
  2026-11-06).
- **A section is set apart from its rows:** a heading inside a panel (Kept
  Forwards, Headers, Indexes) has a gap above it and a gap before its
  first row (`form::ROW_GAP`), and each row of a field or label with its
  options (port, Auto, Start, delete) is a `centred_row` on one middle
  line (owner, 2026-11-06) (CR-0071).
- **A label beside a button sits on its middle line too:** in a detail
  view (a container's Ports beside Open in Browser, any value with its
  action) the row is a `centred_row`, label and button on one middle
  line (owner, 2026-11-06) (CR-0075).
- **An item-kind column is easy to read:** a column that lists the kinds
  of thing a pane shows (Containers, Projects, Images, Volumes, Networks,
  Swarm …) has larger icons and labels than an ordinary list (20 px
  icon, body text, a 30 px row) and each kind wears its own colour and
  fitting picture (Material when on) (owner, 2026-11-06) (CR-0074).
- **Status figures are laid out, not strung together:** monitoring values
  (CPU, memory, disks, load) are a table or a grid of labelled figures,
  aligned in columns, never one long line of text (owner, 2026-11-06)
  (CR-0071).
- **A host's mark follows the host:** wherever a host or a host's
  children are grouped (the SSH Tunnels list, the shell's Tunnels view),
  the group header wears that host's coloured mark (Ubuntu, macOS, …), and
  an item kind with its own picture (a tunnel) wears it in colour too
  (owner, 2026-11-06) (CR-0071).
- **Each tool has its own coloured picture, the same everywhere:** in an
  activity that offers several tools (Devtools' Base64, JWT, Cron …;
  Graphics' Image, Icon, Diagram Editor, Diagram Generator), each tool
  wears a multi-colour picture that shows what it does, and the same
  picture is used in the sidebar, the dashboard card and the tool's tab
  (owner, 2026-11-06) (CR-0076).
- **One set of log-level colours:** every view that shows log lines - the
  Logs activity, Automation run logs, container and pod logs, the Editor's
  debug console and build output, the SSH monitor, the Redis/API consoles -
  colours ERROR, WARN, INFO, DEBUG, TRACE and their aliases (ERR, FATAL,
  CRIT, WARNING, NOTICE, VERBOSE …) with the Logs activity's colours,
  through one shared function, never its own palette (owner, 2026-11-06)
  (CR-0077). Only the level word itself is coloured (`ERROR`, `WARN`,
  `level=error` …), exactly as the Logs activity paints it; the rest of
  the line keeps the normal text colour (owner, 2026-11-07).
- **Options on one row are spaced apart:** checkboxes or radio buttons
  sharing a row (Amend · Push After Commit; If Not Exists · Drop First;
  regex flags) have `form::OPTION_GAP` (18 px) between them, so each reads
  as its own choice; options in a form grid stay one per row (owner,
  2026-11-06) (CR-0078).
- **A product wears its real mark, everywhere:** a saved item that is a
  known product - a shell (PowerShell, Command Prompt, Git Bash, a WSL
  distro, zsh, fish, Nushell …), a database engine, a broker, a cloud, a
  forge - wears that product's real multi-colour logo, the same in its
  list row, its tab, the dashboard, Home's Recent, any history and its
  Settings page; a plain glyph only for a product with no logo, then in
  the product's colour (owner, 2026-11-06) (CR-0079).
- **Settings forms are two left-aligned columns:** in every Settings page
  (and any form built like one) each label is left-aligned in the first
  column; a sub-setting's label (one that only applies when its parent is
  on) is indented one step (`form::SUB_INDENT`) under its parent, still
  left-aligned; every control - checkbox, radio buttons, text field,
  dropdown, slider - starts at the same x in the second column, whatever
  the label's length. Built with `form::field_grid` / `form::label` /
  `form::sub_label`, never a hand-made row (owner, 2026-11-06) (CR-0080).
  Every row of a Settings page - in the grid or outside it (a stacked
  checkbox, a hint, a button row, a list) - has the same vertical air
  (`form::ROW_GAP` between rows); no two rows touch (owner, 2026-11-06).
  Text blocks count as rows: a description, a guide, a paragraph, a hint
  or a standalone label has the same gap above and below as the controls
  around it - it never sits flush against a field or a heading (owner,
  2026-11-06).
  Buttons in a Settings page are one size: every button - primary or
  not - is a field's height with the same padding (`form::button` /
  `form::primary_button`, never a bare `ui.button` or `small_button`),
  and buttons side by side have `form::BUTTON_GAP` between them (owner,
  2026-11-06).
  A sub-section heading inside a Settings card (a group of rows under the
  card's own title) is a step larger than body text and strong
  (`form::subsection`), smaller than the card's title, so the hierarchy
  card title > sub-section > row label reads at a glance (owner,
  2026-11-06).
  A list of checkboxes has `ROW_GAP` between items, and a checkbox's
  description sits under its label (indented to the label's text) with
  the gap after the description, not between the box and its text.
  Reorder controls (move up / move down, drag handles) are standard-size
  buttons in an aligned column, and the rows they reorder have `ROW_GAP`
  between them (owner, 2026-11-06).
  Suggestion lists (CLI suggestions, presets to add, discovered servers):
  each row is a `centred_row` - icon, name, detail and its Add button on
  one middle line - the button standard size in an aligned right column,
  and `ROW_GAP` between rows (owner, 2026-11-06).
- **The work-pane header wears the tab's mark:** the header band at the
  top of a work pane (`hs_core::tool::ToolHeader`) leads with the same
  picture and colour as that tab's icon and its sidebar row - a
  product's logo, a tool's picture, a Material item icon, a rotating
  source colour - never a generic or uncoloured glyph, in every activity
  (owner, 2026-11-06).
- **Dropdown items wear their kind's mark:** every dropdown (ComboBox,
  `pick_field` popup, menu) whose items show an icon colours it by the
  item's kind exactly as that kind is drawn elsewhere - a connection's
  product logo or brand colour, a shell's logo, a workspace's mark, an
  environment's colour, a file or item kind's Material picture when
  Material icons are on, an activity's rail colour - and the selected
  value in the closed box wears the same mark (owner, 2026-11-07)
  (CR-0081).
- **An environment wears its tier's mark:** every environment shows the
  icon and colour of its tier - set by hand, else matched from its name
  (LOCAL/LOCALHOST, DEV/LOWER, SIT, TEST/QA, TRAINING, UAT, STAGING,
  PROD-SUPPORT, PROD/PRODUCTION) - in the title-bar dropdown, the banner,
  Settings → Environments and every tab tied to it; a protected
  environment always shows the red shield, whatever its tier. Production
  and Production Support default to protected (owner, 2026-11-07)
  (FEAT-0106).
- **A workspace without a logo is a lettered tile:** its one or two
  initials on a rounded square in a colour picked from its name (the
  shared palette of eight), the same in every place a workspace is drawn;
  a set logo always wins (owner, 2026-11-07) (CR-0082).
- **A list or tree row is one middle line:** caret, picture, name and
  badge share the row's middle line. The row's height is set to its
  tallest item (the name button) *before* the row opens - a 16 px picture
  added first to a row that grows later is centred on the shorter height
  and sits high (owner, 2026-11-07: the Redis key tree).
- **A status line is a row too:** a count, a "scanning …" or a result
  summary under a filter field, toolbar or table has `form::ROW_GAP` above
  it, in every pane, never flush against the field (owner, 2026-11-07).
- **A group's grip is on its own line:** the drag grip of a row that
  opens below itself (a collection, a folder) sits on the header's middle
  line, beside its name, never halfway down its open children - draw it
  with `reorder::Reorder::group_row` and the header's height, not `row`
  (owner, 2026-11-07).
- **A table stays inside its card:** a table's header band ends at the
  card or pane it sits in, never at the window edge; and a table with no
  columns is not drawn at all - the empty state says why, with no bare
  band under it (owner, 2026-11-07).
- **Devtools tabs show the whole tool name:** a Devtools tab's title is
  a fixed name (Unit Conversion, Firebase Tokens), so its strip makes
  every tab wide enough for the longest one and wraps rows rather than
  eliding it (`Tool::tab_title_whole`). Other activities' tabs keep the
  shared width and elide long user data as before - owner's choice,
  2026-11-07: "Only Devtools, because the label name is fixed."
- **An Open Here card wears its tab's mark:** every card in a dashboard's
  Open Here section leads its title and subtitle with the same coloured
  icon its tab wears in the strip (a logo, a tool's picture, a Material
  item icon, a coloured glyph), on the middle line of the two lines - in
  every activity, through the shared `dashboard` card (owner,
  2026-11-07).
- **A page title wears its tab's mark, and says what the page is for:**
  a tool or page title inside a work pane (each Devtools tool, API
  Client's Manage Collection) leads with the same coloured picture its
  tab wears, on the title's middle line (`form::page_title_marked`), and
  has a one-line sentence-case description under it - no tool goes
  without one. Hints and notes under it wrap at the cards' width
  (`form::CARD_MAX_WIDTH`), never at the window edge; an input with
  nothing in it yet shows a muted empty hint, never a red error (owner,
  2026-11-07).
- **What opens a tab shows it:** any action that opens, adds or fills a
  tab - New Query, Open Data, a saved item, a history entry, a template -
  brings that tab to the front, whatever kind of tab (a designer, a
  preview) was in front before, in every activity (BUG-0047).
- **The header row is one accent band:** every table's header has the
  accent fill (`table::header_fill`) across the full row - every column,
  the gaps and the slack to the right edge - with an accent rule under it,
  so it reads apart from the data cells. Painted once per table with
  `table::begin_header_band` / `end_header_band`, never per cell; every
  table, every activity (owner, 2026-11-06).
- **Header titles are indented:** every table header title starts
  `table::HEADER_INDENT` (8 px) in from its cell's left edge, and the
  column's left-aligned cell text starts at the same x - every table, every
  activity, the Database results grid included (owner, 2026-11-06).
- **A table fits its pane:** column floors add up to no more than a usual
  work pane (about 1050 px); a header must never run past the pane's
  right edge at the default window size (CR-0061).
- **Every list row has a glyph** when any row in that list has one -
  palette commands, menus, sidebars - so the names line up (CR-0066).
- **Scope of these rules:** every rule here applies to every pane,
  component, row, table and activity, not only the one it was found in.
  When the owner states a UI rule for one place that applies broadly, it
  is written here the same turn and applied everywhere.

## Text

- Every visible string goes through Fluent, `locales/en.ftl` first. **Edit
  locale files with the Edit tool, never a heredoc** — an unquoted heredoc eats
  variable placeholders and silently drops the message, and
  `the_bundled_locale_parses_without_junk` then fails the gate.
- Casing:
  - **Title Case** for button labels, menu and list items, setting names, tab
    and category labels. Minor words (a, the, of, to, from, at, in, on, and)
    stay lowercase.
  - **Sentence case** for tooltips, hints, placeholders, dialog bodies, toasts
    and empty states.
  - A parenthetical attached to a label starts with a capital — "None (Direct
    connection)". A parenthetical inside a sentence stays lowercase.

## Interaction

- Every action gets a palette command **and** a rebindable shortcut. Every list
  and tree is keyboard-navigable.
- Never bind Ctrl+V, Ctrl+Shift+V, Shift+Insert, Ctrl+C or Ctrl+X:
  egui-winit turns them into paste/copy/cut events first. Global shortcuts win
  over tool-local keys, so a tool must not handle the same combo.
- Right-click copy/paste (PuTTY style) in the terminal and every editor or text
  field we draw: with a selection, copy and clear it; without one, paste at the
  cursor. On by default; a per-tool setting may turn it off.
- Destructive actions are **guarded, not forbidden** (a confirmation; a typed
  confirmation for purge- and delete-style verbs). Protected environments
  (production) default to read-only in every tool.
- Every async action needs an **empty**, an **error** and a **loading** state,
  through the shared helpers.

## Fields, lists and cards (CR-0038, 2026-11-05)

- **A field the server can fill is `hs_ui::form::pick_field`** - typed or
  chosen, the server's list behind a caret - never a bare text field: refs,
  assignees, priorities, types. With an empty list (server unreachable) it
  is a plain text field, so the form still works. Dates are
  `hs_ui::datefield::date_field`, `YYYY-MM-DD` text with a calendar; it
  writes the format the server takes.
  The same holds for any list the app already knows: a referenced
  table's columns from the catalog, a schema's tables, saved profiles,
  branches - a field that can be filled from one is a `pick_field`, for
  a list of values one per pick (owner, 2026-11-06: the designer's
  Foreign Keys columns) (CR-0072).
- **Folders first, then items, each by name**, in every list that mixes
  them (the Redis key tree is the model).
- **Anything a user makes several of takes an optional name**, shown first
  in its list (SSH forwards).
- **A sub-tab keeps its own state**: a Database query tab shows the results
  it ran, not the last run anywhere.
- **A server's log is laid out in steps** before it is drawn
  (`tool_automation::logfmt`): banners, groups, commands, indented body;
  lay it out once per change, draw with `show_rows`.
- **Settings cards:** `form::section` for the usual width;
  `form::section_max(…, f32::INFINITY, …)` only for a card that holds a wide
  table. Never draw a column no row fills. A connection form states an
  external requirement (a vendor client, a CLI) as a field hint every time
  that engine is chosen, with the "not installed" warning beside it.
- **Home's toolbar icons:** icons only by default; with Settings →
  Appearance → Activity Rail → *Show Tool Names Under Home's Toolbar Icons*
  on, one fixed width per icon (`LABELLED_WIDTH`) with the name under it.

## Layout and controls standard (CR-0061, 2026-11-06)

The numbers and helpers every pane uses. If a screen needs a value that is
not here, add it here and to `hs_ui` first; never a magic number in a tool.
Each rule names the item that decided it, so the history is in `TASKS.md`.

### Pane margins and gaps

- **Pane margin:** a work pane's content sits `form::PANE_MARGIN` (12 px)
  in from the pane's left and right edges and from the toolbar or tab
  strip above it. Draw a pane's body inside `form::pane(ui, |ui| ..)`,
  which applies it. Nothing touches a border (CR-0031, CR-0032).
- **Between parts of a pane:** `form::PANE_GAP` (8 px) under a toolbar or
  a header rule, between an action row and a filter row, between a filter
  row and its table, and between a table and the status line under it
  (CR-0031, CR-0033, CR-0039). A section that ends is ruled off with a
  separator that has `PANE_GAP` above and below it.
- **Between form rows:** `form::ROW_GAP` (10 px), the vertical spacing of
  `form::field_grid`. Rows stacked by hand get `ui.add_space(form::ROW_GAP)`
  between them; rows inside an editable table get it from the row height
  (CR-0061).

### Inputs

- **One height for every input in a row:** a text field, a dropdown, a
  pick field, a checkbox and a button beside them are all
  `form::field_height(ui)` tall (about 32 px at the default font) (CR-0026).
  - Text fields come from `form::text_field`, `form::text_field_width`,
    `form::pick_field` or, when the call site needs its own width, hint or
    font, `form::line_edit(&mut value)`. **Never** a bare
    `egui::TextEdit::singleline` - egui's default margin draws it about ten
    pixels shorter (CR-0061).
  - Dropdowns, checkboxes and buttons get the height from their container:
    `form::field_grid` (forms), `toolbar::strip_style` (toolbars),
    `table::edit_table` (editable tables). Anywhere else a row mixes a
    label, a field, a dropdown or buttons, open it with
    `form::centred_row(ui, |ui| …)`, or `form::centred_row_wrapped` when
    it may wrap (CR-0026, CR-0027, CR-0061).
  - **Labels sit on the field's middle line.** egui fixes a horizontal
    row's height when the row opens, so the field height must be set
    *before* `ui.horizontal` / `ui.horizontal_wrapped`, never inside the
    closure: `match_field_height` inside the closure makes the field tall
    but leaves the label centred on the old, shorter row - it sits high
    (owner's report, 2026-11-06: "main." beside the table name, "Script"
    beside Apply). `centred_row` does it in the right order. A bare
    `ui.horizontal` holding a label and a `line_edit` is the same bug.
  - Exceptions, kept compact on purpose: the command palette's search, the
    title bar and the bottom dock.
- **Width shows the value:** a field is wide enough for the values it
  holds. In a form a field is `FIELD_MIN_WIDTH` (260) to `FIELD_MAX_WIDTH`
  (560) wide. In a table, a column's width is its floor and spare width is
  shared out (`table::edit_table` does this); floors are a name 200, a
  type 220, a default or an expression 180, a list of columns 200, a
  reference (`schema.table`) 220, a rule (`CASCADE`, `SET NULL`) 140, a
  checkbox 60 (CR-0061).
- **Vertical middle:** every row that holds more than one input lines its
  inputs up on one middle line. `ui.horizontal` does this once the heights
  match; `ui.horizontal_top` is only for page layouts (a list beside a
  form), never for an input row. A label beside a field in a grid is
  `form::label` (CR-0026, CR-0061).
- **Description under a field** goes on its own row
  (`form::field_hint_row`), never in the field's cell, or the row grows and
  its label drops to the middle of the pair (CR-0026).

### Tables

- **Read-only data:** `hs_ui::table::DataTable` - never a bare
  `TableBuilder` for a wide table (docs/04 §6).
- **Rows you edit** (a table's columns, a list of rules):
  `hs_ui::table::edit_table` - never an `egui::Grid` with a hand-made
  header row (CR-0061).
- **Item and value facts** (Type / Bug): `hs_ui::form::detail_table`, a
  table with no header (CR-0028).
- **Every table** has the header band (`table::header_band`, the surface
  fill and a rule under it), bold header text at `table::HEADER_TEXT`
  (14.5) on a `table::HEADER_HEIGHT` (34) row, data rows
  `table::ROW_HEIGHT` (26) tall (editable rows `table::edit_row_height`),
  striped rows, and every cell laid out by `table::cell_layout()` - from
  the left, **vertically centred** in its row (CR-0028, CR-0046, CR-0061).
- A table that scrolls on its own sits in `form::well`; a block of output
  in `form::scroll_well` with a fixed height (CR-0028).

### Buttons and toolbars

- **Two toolbar sizes:** the activity's main toolbar (the first row under
  the pane title) has the larger buttons; a sub-toolbar inside the pane
  (a sub-tab's own row, a panel's header) has smaller buttons with a
  little air above and below, never as large as the main ones
  (`toolbar::MAIN_BUTTON_HEIGHT` > `toolbar::SUB_BUTTON_HEIGHT`) (owner,
  2026-11-06) (CR-0073).
- **Show/hide toggles come first:** every toggle that shows or hides a
  column of the work pane - the left browse column (Folder, Schema, Tree)
  and the side panels (Graphics' Properties for the diagram, image and
  icon editors, the generator's Prompt) - is the left-most item of the
  first row of the activity's toolbar, before any action, in every
  activity. This places existing toggles; it does not add one to every
  column (owner, 2026-11-06: "placement only") (CR-0073). A toggle for a
  view of the session itself - SSH's Tunnels and Monitor - is not a
  column: it stays with that session's actions, just before Broadcast
  (owner, 2026-11-06).
- **Every button has an icon and a label** - `format!("{icon} {label}")`.
  The only exception is a row's own small actions (move, remove) in a
  fixed, aligned column: an icon with a tooltip (CR-0027, CR-0030,
  CR-0061).
- **A row of buttons wraps** instead of running past the pane: build it
  with `ui.horizontal_wrapped`, never `ui.horizontal`, when it holds more
  than two buttons or sits beside a field. Only the AI Assistant corner
  uses `toolbar::hard_right` (CR-0027, CR-0061).
- Hover highlights a control; it never grows or moves (`Theme::apply`
  zeroes widget expansion) (CR-0026, CR-0050). This holds for every
  hand-painted row, card, tree node, link and button too: hover changes
  the fill or draws a border, it never paints a larger rect, a bigger font
  or a scaled icon, and no scope may reset the style to egui's default
  (which brings expansion back). Owner, 2026-11-07, seen again in
  Containers, Redis and NoSQL.
  Selectable items go through `form::selectable_label` /
  `selectable_value` / `toggle_value` / `add_selectable`, never egui's own
  (see GOTCHAS: they grow 1 px a side on hover).
- Toolbar buttons and dropdowns are the field height with
  `toolbar::BUTTON_PADDING` (10 x 6); the AI Assistant control is the
  hard-right item through `toolbar::hard_right`; a toolbar that wraps
  puts its second row under a full-width rule (CR-0027).
- A sidebar starts with `form::sidebar_head` (title, then the labelled
  Add button with air above and below and a rule under it) (CR-0030).

### The right component for the value

| The value is | Use |
|---|---|
| on / off | a checkbox (`ui.checkbox`), or `form::toggle_button` in a toolbar |
| one of a fixed set | a dropdown (`egui::ComboBox`, raised to field height) |
| one the server knows, or typed | `form::pick_field` (typed or chosen) |
| a date | `hs_ui::datefield::date_field` |
| a secret (password, token, key) | `form::secret_field`, stored as `secret:<ulid>` |
| a number in a range | `form::slider`, or a `DragValue` beside its unit |
| a name, a path, a short text | `form::text_field` / `form::line_edit` |
| several lines, code, JSON | `form::code_field` or the editor engine, in a `form::well` |
| a list of name = value pairs | `hs_ui::kv_editor::KvEditor` |
| rows of several inputs | `table::edit_table` |
| facts to read (Type / Bug) | `form::detail_table` |
| many rows to read, sort, filter | `table::DataTable` |

A value is never squeezed into a weaker component (a yes/no as a text
field, a fixed set as free text) and never into a stronger one that the
user has to fight (a free name as a dropdown).

### Checking a screen

Run the app on a scratch data folder and look, at the default size and
at a narrow window. Every item below must hold before the screen is
reported; write what you checked in the item's App box.

1. **Fields fit their values:** every field and every column shows a
   typical value whole (a table name, a type such as
   `enum('low','normal','high')`, a URL, a path). Nothing is a sliver, and
   long values truncate with an ellipsis, not mid-glyph.
2. **One height, one middle line:** every input in a row is the field
   height, and the row's inputs, labels and buttons sit on one middle line.
   Table cells are vertically centred.
3. **Sections are separated:** each section of a work pane (a toolbar, a
   form, a table, a script or output area, a status line) is set apart by
   a separator or a card, with `PANE_GAP` above and below it; form rows
   are `ROW_GAP` apart.
4. **Margins:** nothing touches a pane edge; content is `PANE_MARGIN` in.
5. **Right components:** each value uses the component the table above
   names for it.
6. **Tables:** the header band, bold header text, centred cells, the
   standard row heights.
7. **Text:** Title Case labels and buttons, sentence-case hints and
   toasts; every string from Fluent.
8. **States:** the empty, loading and error states each say something.
9. **Buttons:** every button has an icon and a label (row actions: icon
   and tooltip), and every row of buttons wraps at a narrow window.

## Icons

- **Colour everywhere an item is drawn** (CR-0065): a dashboard `Entry`
  sets `image` or `tint`, never neither, and an inner tab strip's
  `TabItem` returns `icon_image` from `hs_ui::fileicons` when
  `material_icons(<tool>)` is on, the same picture its list uses. The
  palette's rows too: `PaletteItem::image` / `colour` (CR-0066).
  Every sidebar list and tree row of an item kind too - saved queries,
  history entries, snippets, sources - with the same picture its tab
  wears; a plain glyph only when Material icons are off (CR-0068).

Verify a Nerd Font / Phosphor glyph by looking at it on screen. A codepoint
being present does not mean it draws the right picture.

## Verifying UI work

Tests are not enough. Run the app against a scratch data folder and look:

```bash
cargo run -p hyperstack -- --data-dir ./tmp-data
```

`egui_kittest` covers widget snapshots, but synthetic clicks never form
double-clicks — exercise double-click paths in the running app. Check a narrow
window for the toolbar hard-right layout. The "Headless and GUI Testing"
section of `docs/GOTCHAS.md` has the click-coordinate offsets and the minimum
window size.
