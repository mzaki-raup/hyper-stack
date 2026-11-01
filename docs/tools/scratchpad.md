# Tool spec — Scratchpad (notes)

**Crate:** `hyperstack::tools::scratchpad` (built into the shell) · **Priority:** P1 · **Status:** implemented 2026-10-15

## 1. Purpose

A per-workspace collection of Markdown notes: meeting notes, runbooks,
checklists, diagrams, snippets to keep next to the connections they
belong to. Notes are plain `.md` files, so they survive the app and can be
read with anything.

## 2. Storage

```
<workspace>/notes/
├── 01M4…D03.md        # one note = one file, named by id; title = first heading or line
├── Work/              # a folder is a real directory, at any depth
│   ├── 01M4…K5Q.md
│   ├── Clients/
│   │   └── order.json # each level keeps its own order
│   └── order.json
├── pins.json          # ids pinned to the top of the list, in pin order
├── collapsed.json     # folders the user collapsed
└── order.json         # the top level's order: note ids, folders as "/Name"
```

**Folders** (depth programme phase O, 2026-10-31). A folder is a
directory under `notes/`, so the tree survives the app and reads with any
file manager. A note keeps its id wherever it sits, so `note://<id>` still
names it after a move: `find_note` looks at the top level first and then
through the folders. A save looks the note up again rather than trusting a
remembered path, because another tab may have moved it in the meantime and
writing to the old place would leave a stale copy behind.

Each level's `order.json` lists its rows - notes by id, folders as `/` and
their name. A slash can appear in neither, so the two never collide, and an
`order.json` written before folders existed (ids only) still reads. Rows the
order does not mention come first: folders by name, then notes newest first,
which is where a new note has always appeared.

A folder name clash - on New Folder, Rename, or a move into a folder that
already has one of that name - takes the usual ` 001` suffix
(`hs_core::storage::unique_name`). The comparison ignores case, because
`Work` and `work` are one directory on Windows and macOS. Notes are named by
id and so can never clash. A name is made safe for every platform first:
separators and reserved characters become `_`, and `.`, `..` and a trailing
dot or space are refused, since Windows strips those and `Work.` would land
on top of `Work`.

A note is written **1.5 s after typing stops** and on Save (Ctrl+S, the
Save button, the palette command); the tab shows the dirty dot in
between. A brand-new empty note is not written until it has content. The
layout only remembers which note a tab shows (`note://<id>`); the file is
the source of truth and is re-read on restart. A note that was never
saved travels in the layout as a draft.

## 3. Layout

- **Sidebar** (between the activity rail and the tab): pinned notes at the
  top (pin icon) whatever folder they are in, then the tree - folders as
  collapsing headers, each level's folders and notes in its saved order, a
  search box, **New Note**, and **New Folder** (the folder-plus icon). Drag a
  row by its grip to reorder it within its level, drop it between two rows of
  another level, or drop it onto a folder's header to move it in - a
  collapsed or empty folder takes a drop too. A folder dropped onto itself
  or into one of its own subfolders goes nowhere. Pinned notes cannot be
  dragged: unpin one to move it. A folder collapses with a click on its
  header and stays so (`notes/collapsed.json`), across a rename or a move. A
  right-click on a note offers **Pin to Top** / **Unpin** and **Delete**; on
  a folder, **New Folder** (inside it), **Rename**, and **Delete**, which
  says how many notes go with it and is confirmed inline. A new folder opens
  with its name ready to type. While the search box has text the tree gives
  way to a flat list of matches - by title or by folder - each showing the
  folder it sits in. Clicking a note focuses its tab or opens one: **one
  tab per note**, and an empty new tab takes the first note opened into
  it.
- **Quick open** (Ctrl+P) lists every note, with *Scratchpad / Work /
  Clients* - the folder it sits in - as its subtitle. The palette delivers a
  choice to the tab that listed it, so a tab holding a different note passes
  the request on to the shell, which focuses the note's own tab or opens
  one; a note never replaces another tab's note, unsaved edits included.
- **Toolbar**: Source only / Source and preview / Preview only, Save, the
  pin toggle for the open note, **Download…** (the note as a `.md` file
  named after its title), **AI**, and the save state.
- **Tab**: source on the left (monospace editor), the rendered preview on
  the right (`hs_editor::markdown::render`: headings, lists, `- [ ]`
  checklists, tables, code, Mermaid diagrams), split by a draggable
  divider.
- **Variables and links in the preview** (2026-10-29). The preview
  resolves `{{name}}` through the active environment, its parents and
  the application-wide globals before rendering
  (`Environments::substitute_lenient`): an unknown name is left as
  written, not refused, because a note is prose. A link in the preview -
  `[text](https://…)` or a bare `https://…` in the text - opens as a new
  page in the Browser activity (`OpenRequest::Url`), the cursor turning
  to a hand over it. The source pane is for editing and has no clickable
  links. `markdown::render` keeps each link's character range beside
  the `LayoutJob` and hit-tests clicks against the galley it drew.
- **Dashboard** (the activity's home tab, reworked 2026-10-27): a
  bordered band carrying the icon, the name and the subtitle; a rule;
  the **summaries** - Folders, Notes and Open Tabs (Folders added
  2026-10-31, FEAT-0076; it counts every folder, nested ones included) - each in a rounded box with
  the figure above an icon-and-label line, every box one width taken
  from the widest of them; a rule; **Start Here** with the New Note
  button; a rule; **Saved Notes**, every note a clickable button in a
  grid sized to the pane; a rule; and **Open Here**, the shared grid of
  tabs. The furniture is `hs_ui::dashboard` (`head`, `rule`, `section`,
  `Stat`/`stats`, `open_tabs_section`) so the other activities can take
  the same shape without it drifting apart.

- **Folder rows** draw their folder glyph at 22 px, a little bigger than
  a note row's 20 px glyph, so a folder reads as the container at a
  glance (FEAT-0076).

## 4. AI assistant

The shared review-only `AssistantPanel` opens on the right with the note
as context. Its system prompt asks for note content in a fenced
`markdown` block, diagrams in a `mermaid` block and commands in a `bash`
block, each with **Insert**: Markdown goes in as it is, any other language
is inserted as a fenced block of that language, at the editor's cursor
(or at the end as its own paragraph). Nothing is written to the file by
the assistant; the autosave picks the edit up like any other.

## 5. Other surfaces

- **Read Aloud** offers the note and the assistant's last answer.
- **Print…** exports the note as HTML (`markdown::to_html`); **Snip**
  captures the tab.
- `OpenRequest::Text` starts a new note with the text; a dropped or opened
  `.md` file starts a new note with its content (the file is not touched).
- Palette: *Scratchpad: New Note*, *New Folder*, *Save Note*, *Clear
  Note*, *Toggle AI Assistant*.

## 6. Tests

`title_of` (first heading or line, bare `#` lines skipped), `list_notes`
(newest first, only `.md`), `order_notes` (pinned first in pin order).
Folders: the tree is read whole and a dot-directory skipped
(`list_notes`, `list_folders`); a note is found after a move and keeps its
id and content; a level holds only its own folders and unpinned notes,
unlisted rows first; a saved order is kept, and one from before folders
still reads; a clash takes ` 001`, case ignored; a case-only rename is not
a clash; a folder moves with its notes and never into itself; collapsed
folders follow a rename, a sibling with a similar name untouched; folder
names are refused or cleaned for every platform.
The dual pane, autosave, pinning and Insert were exercised in the running
app with the PowerShell harness (`docs/screenshots/scratchpad.png`,
`scratchpad-ai.png`).
