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
├── 01M4…K5Q.md
'├── pins.json          # ids pinned to the top of the list, in pin order
└── order.json         # saved order of the unpinned notes'
```

A note is written **1.5 s after typing stops** and on Save (Ctrl+S, the
Save button, the palette command); the tab shows the dirty dot in
between. A brand-new empty note is not written until it has content. The
layout only remembers which note a tab shows (`note://<id>`); the file is
the source of truth and is re-read on restart. A note that was never
saved travels in the layout as a draft.

## 3. Layout

- **Sidebar** (between the activity rail and the tab): the note list,
  newest first with pinned notes above (pin icon), drag unpinned notes by the
  grip to change their order (`notes/order.json`), a search box, **New
  Note**. A right-click offers **Pin to Top** / **Unpin** and **Delete**
  (confirmed inline). Clicking a note focuses its tab or opens one: **one
  tab per note**, and an empty new tab takes the first note opened into
  it.
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
  the **summaries** - Notes and Open Tabs - each in a rounded box with
  the figure above an icon-and-label line, every box one width taken
  from the widest of them; a rule; **Start Here** with the New Note
  button; a rule; **Saved Notes**, every note a clickable button in a
  grid sized to the pane; a rule; and **Open Here**, the shared grid of
  tabs. The furniture is `hs_ui::dashboard` (`head`, `rule`, `section`,
  `Stat`/`stats`, `open_tabs_section`) so the other activities can take
  the same shape without it drifting apart.

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
- Palette: *Scratchpad: New Note*, *Save Note*, *Clear Note*, *Toggle AI
  Assistant*.

## 6. Tests

`title_of` (first heading or line, bare `#` lines skipped), `list_notes`
(newest first, only `.md`), `order_notes` (pinned first in pin order).
The dual pane, autosave, pinning and Insert were exercised in the running
app with the PowerShell harness (`docs/screenshots/scratchpad.png`,
`scratchpad-ai.png`).
