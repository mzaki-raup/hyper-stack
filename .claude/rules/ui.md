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
    `table::edit_table` (editable tables). Anywhere else, call
    `form::match_field_height(ui)` at the top of the row's closure, or
    use `form::field_row` (CR-0026, CR-0027, CR-0061).
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

- **Every button has an icon and a label** - `format!("{icon} {label}")`.
  The only exception is a row's own small actions (move, remove) in a
  fixed, aligned column: an icon with a tooltip (CR-0027, CR-0030,
  CR-0061).
- **A row of buttons wraps** instead of running past the pane: build it
  with `ui.horizontal_wrapped`, never `ui.horizontal`, when it holds more
  than two buttons or sits beside a field. Only the AI Assistant corner
  uses `toolbar::hard_right` (CR-0027, CR-0061).
- Hover highlights a control; it never grows or moves (`Theme::apply`
  zeroes widget expansion) (CR-0026, CR-0050).
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
