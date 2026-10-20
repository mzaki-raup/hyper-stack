# Tool spec — Graphics diagrams (editor and generator)

**Crate:** `tool-graphics` (UI) + `hs-diagram` (model, layout, scene, export) ·
**Priority:** P1 (owner's request, 2026-10-19) ·
**Status:** in progress — Phase 6 (AI assistants) implemented 2026-10-20

This document is the requirements and design for the Graphics activity as it
grows from a picture editor into four tools: **Image**, **Icon**, **Diagram
Editor** and **Diagram Generator**. It is written before the code, as the
owner's process rules require.

| Phase | State |
|---|---|
| Graphics activity shell: sidebar, dashboard, Recent, Image/Icon tabs, Convert to Icon | Implemented 2026-10-19 |
| Icon colour-key transparency | Implemented 2026-10-19 |
| `hs-diagram` model, validation, flow/architecture layout, scene, SVG/PNG/PDF export | Implemented 2026-10-19 |
| Diagram Editor UI (flowchart, architecture; selection, drag, add, connect, undo, templates, save/export) | Implemented 2026-10-19 |
| Diagram Generator UI (prompt, examples, AI, offline templates, preview, Open in Editor, export) | Implemented 2026-10-20 |
| Review-only AI assistant in all four tools | Implemented 2026-10-20 |
| ERD, sequence, Archify JSON and draw.io XML import (Release 2) | Partial 2026-10-20 |

Related documents: [graphics.md](graphics.md) (the existing image/ICO tool),
[../03-architecture.md](../03-architecture.md),
[../04-ui-design.md](../04-ui-design.md),
[../05-data-model-and-storage.md](../05-data-model-and-storage.md),
[../08-decisions.md](../08-decisions.md) (ADR-025).

---

## 1. Purpose

The Graphics activity becomes one place for the pictures and diagrams that sit
next to a project:

- open, adjust and export a picture or an icon;
- draw a flowchart or an architecture diagram by hand and keep it editable;
- generate a first draft of one from a sentence, without leaving the app.

The diagram document is a normal file in the project (`<project>/docs/diagrams/
<name>.hdiag` by default), so it is committed, diffed and reviewed with the
code. An unfinished diagram is autosaved as a draft and survives a restart.

**Release 1** ships the Graphics activity, the Image/Icon split, and the
flowchart and architecture diagram types (editor + generator + AI assistants).
**Release 2** adds ERD and sequence diagrams, draw.io XML import, and Archify
JSON import.

### 1.1 Owner's requests (2026-10-19)

The request, verbatim in substance, with the answer that fixed each design
point:

1. A flow/diagram generator inside the app; the result can be saved into a
   project folder in the Editor activity. Archify was named as a reference.
2. A diagram editor that can be saved and reopened for editing, and exported as
   an image — "like draw.io".
3. In the Graphics activity:
   - the sidebar lists every function: Image, Icon, Diagram Editor, Diagram
     Generator;
   - the permanent first tab lists the functions and shows a "What To Do"
     dashboard with a grid of screenshots of the open tabs;
   - Image and Icon manipulation are separate;
   - right-clicking Image opens an image in a new tab, with the existing
     settings on the left and the preview on the right; an image can be
     converted to an icon and opened in a new Icon tab;
   - right-clicking Icon opens an icon in a new tab with the icon settings on
     the left and the preview on the right;
   - right-clicking Diagram Editor creates a diagram in a new tab; an unsaved
     diagram is temporarily saved and persists across a restart; export as
     image or PDF;
   - right-clicking Diagram Generator opens a generator tab with suggested
     prompts, an example and a preview; export as image or PDF;
   - the sidebar has a Recent section below the functions, scrollable, about
     half the column, indented, and clicking an entry focuses its open tab or
     opens it.

Answers that fixed the scope:

| Question | Answer |
|---|---|
| Permanent tab | The activity's existing permanent home tab (first in the strip); the sidebar lists all four tools. |
| Draw.io scope | The v1 list (shapes, connectors, labels, align, grid, undo, save, export) plus draw.io XML **import** in Release 2. |
| Diagram types | Release 1: flowchart and architecture. Release 2: ERD and sequence. Mermaid is optional for creating a diagram. |
| File location | `<project>/docs/diagrams/<name>.hdiag` by default; `.diagram.json` may be added later as an alias; export is image (PNG/SVG) and PDF. |
| Drafts | A draft stays in Recent when the tab is closed. |
| AI | The existing AI providers; the generator also works offline from templates. |
| Icon transparency | A colour picker/eyedropper makes a chosen colour transparent, like IconConverter; existing operations and ICO sizes stay. |
| Archify | Reimplement the ideas; adapt its JSON schema and validation rules with a `THIRD_PARTY_NOTICES.md` and both copyright lines; keep the renderer native. |
| AI assistant | Every one of the four tools has an assistant with several suggestion buttons. |

---

## 2. Scope

### 2.1 Release 1

| Area | Included |
|---|---|
| Activity shell | Permanent home dashboard; sidebar function list + Recent; right-click menus; commands; recents/drafts |
| Image | Existing operations, save/export, one tab per image, convert to icon |
| Icon | Open icon; existing operations; ICO frames/sizes; colour-key transparency; save ICO/PNG |
| Diagram Editor | Flowchart and architecture; shapes, connectors, labels, align, grid/snap, undo/redo, zoom/pan; save/open; drafts; PNG/SVG/PDF export; templates; Mermaid **flowchart** import |
| Diagram Generator | Flowchart and architecture; prompt + examples; AI providers; offline templates; preview; open in editor; export |
| AI assistant | Image, Icon, Diagram Editor, Diagram Generator; four quick actions each + "More Suggestions"; review-only |
| Format | `.hdiag` (editor-native JSON) and the Archify-adapted semantic IR used by the generator |

### 2.2 Release 2

| Area | Included |
|---|---|
| Diagram types | ERD (entities, columns, types, keys, cardinality) and Sequence (participants, messages, returns, notes, activations, `alt`/`loop`/`opt`) |
| Import | Archify JSON (`workflow`, `architecture`, `erd`, `sequence`) and draw.io XML (plain and compressed, first page) |
| Format | `.diagram.json` as an alias of `.hdiag`; schema migration |
| Mermaid | Sequence import (the existing parser already reads Mermaid sequence) |

### 2.3 Out of scope

Full draw.io parity (thousands of stencils, pages/layers, plugins), draw.io XML
export, real-time collaboration, cloud hosting, AI vision for Image/Icon,
AI-applied edits, branded/stencil icon libraries, deployment-cloud semantics
for architecture (until a later release).

---

## 3. Architecture

### 3.1 Crates

```text
crates/hs-diagram/                  library, no egui dependency (implemented)
  lib.rs                            re-exports + layout_approx
  model.rs                          DiagramDoc, Node, Edge, Group, Style, Page
  schema/archify/                   adapted JSON Schema files (see §9)
  schema/mod.rs                     strict serde types + IR validation
  validate.rs                       rule codes + limits
  text.rs                           measurement callback, approximation, wrapping
  layout/mod.rs                     scene builder, node sizing, page fit
  layout/flow.rs                    ranks, barycentre ordering, placement
  layout/grid.rs                    architecture grid and boundaries
  route.rs                          ports, straight/orthogonal/curved routes
  scene.rs                          Prim/Scene, resolved geometry, text lines
  svg.rs                            Scene -> SVG
  export.rs                         SVG -> PNG (resvg), SVG -> PDF (svg2pdf)
  convert.rs                        IR -> DiagramDoc
  template.rs                       offline flowchart and architecture templates
  io.rs                             `.hdiag` read/write with limits and atomic replace

crates/tools/tool-graphics/         UI
  lib.rs                            factory, tab dispatch, title/dirty/open
  activity.rs                       GraphicsHub, sidebar, home, recents
  image.rs                          existing picture pipeline (moved)
  icon.rs                           icon document, ICO, transparency
  diagram/editor.rs                 canvas and tab
  diagram/canvas.rs                 transform, hit-testing, drag
  diagram/props.rs                  style/geometry panel
  diagram/commands.rs               palette and keyboard actions
  diagram/templates.rs              blank, flow, architecture
  diagram/mermaid_import.rs         Mermaid flowchart -> DiagramDoc
  generator.rs                      prompt, AI, validation/repair, preview
  assistant.rs                      per-tool context and quick actions
```

The tool keeps the "one crate per tool" rule. `hs-diagram` is a helper library
like `hs-editor`/`hs-git`, not a tool.

### 3.2 Rendering pipeline

```text
Doc -> validate -> layout(measure) -> Scene ──► egui painter
                                            ├──► SVG ──► resvg ──► PNG
                                            └──► SVG ──► svg2pdf ──► PDF
```

- One scene, three consumers. The screen and the exports never have separate
  renderers.
- Text is wrapped once during layout; the scene stores the final lines, and
  both the painter and the SVG writer draw those lines.
- `layout` takes a `measure` callback, like `hs_editor::mermaid::layout`, so
  the core is testable without a window.
- `svg2pdf` 0.13 uses `usvg ^0.45`, `resvg ^0.45` and `tiny-skia ^0.11.4`, the
  versions already in `Cargo.lock` through `egui_extras`' SVG loader. The new
  dependency is MIT/Apache-2.0 and passes `deny.toml`.

**Implemented 2026-10-19.** `hs-diagram` is a workspace member with
`model`, `schema`, `validate`, `text`, `layout` (`flow`, `grid`), `route`,
`scene`, `svg`, `export`, `convert`, `template` and `io`. The tests cover the
flow and architecture templates, deterministic layout, validation codes,
strict IR parsing, SVG escaping, `.hdiag` round-trip, PNG signature and PDF
header (16 tests at this phase). The example
`cargo run -p hs-diagram --example export -- <dir>` writes both templates as
SVG, PNG and PDF; the PNGs were checked visually (flowchart flow and the
architecture boundary grid). The `resvg`/`svg2pdf` graph added the unmaintained
`rustybuzz` advisory, ignored in `deny.toml` with its reason.

**Editor implemented 2026-10-19.** A Diagram tab opens from the activity
sidebar. The canvas pans (Space+drag or middle drag), zooms (Ctrl+wheel) and
shows the grid; clicking selects, dragging moves a node, the shape buttons
place a process/decision/note, Connect links two nodes, and the toolbar has
undo/redo, Fit and Grid. The properties panel edits the title and the selected
node's label and shape; the Outline selects nodes; the validation state is
shown. Templates and Save/Open `.hdiag` and Export PNG/SVG/PDF are wired.
Still to do for the release: resize handles, alignment/distribution, edge
labels and waypoints in the UI, the project-save dialog, Mermaid import. Close-time drafts are implemented. The
editor screenshots are in `docs/screenshots/graphics-diagram-editor.png` and
`graphics-diagram-architecture-editor.png`.

**Generator implemented 2026-10-20.** A Diagram Generator tab has the
flowchart/architecture picker, a prompt, three examples per type, Use Template,
Generate/Stop, and a preview of the same scene. Generate asks the configured
provider for the strict IR asynchronously (`hs_core::ai::chat`), strips fences,
parses and validates it, then converts to a `DiagramDoc`; invalid output keeps
the last good preview and shows the validation codes. With no provider (or an
empty prompt) the offline template is used and the message says so. Open in
Editor writes a generated `.hdiag` under
`HyperStackData/graphics/generated/` and opens it as a normal Diagram tab;
Export offers PNG and PDF. Invalid IR is now rejected, sent back once with the validation reasons, and
the corrected reply is used; a second failure keeps the last good preview and
shows the reason. The real-provider path is not exercised here because no
provider is configured in the test data folder; the offline path is verified
in the app. The
screenshot is `docs/screenshots/graphics-diagram-generator.png`.

### 3.3 Shell changes

| Change | Why |
|---|---|
| `GraphicsHub` (`Arc<Mutex<...>>`) passed to `GraphicsFactory` | Recents, drafts, last save folder and the image→icon handoff are shared by every Graphics tab, like `SharedHome`/`SharedBrowser`. |
| `OpenRequest::NewDocument { kind, options }` | A sidebar action must request "new diagram/generation"; `File`/`Text` should not be overloaded. |
| `TabInfo.uri` | A Recent row finds the exact open tab and focuses it instead of matching by title. |
| `OpenProjects` registry in `hs_core` | The save dialog lists the Editor's open project roots. Published by the editor like `OpenTabs`. |
| Dashboard-home migration in `ensure_home_tabs` | An upgraded layout keeps the old image tab marked `home`; the dashboard must replace it, not disappear. |

---

## 4. The Graphics activity

### 4.1 Sidebar

```text
Graphics
──────────────────────────────
Image             Open an image…
Icon              Open an icon…
Diagram Editor    New diagram…
Diagram Generator Generate a diagram…
──────────────────────────────
Recent                         (≈50%, scrollable)
  Checkout flow    Draft
  login.sequence   saved
  logo.ico         saved
```

- The four function rows are clickable and keyboard-navigable. Left-click opens
  the most recent document of that kind, or the file picker when there is none.
- Right-click opens the per-function menu:
  - Image: Open Image…, Open Recent Image, Convert Open Image to Icon.
  - Icon: Open Icon…, New Icon…, From Open Image.
  - Diagram Editor: New Blank Diagram, New from Template, Open Diagram…,
    Import Mermaid….
  - Diagram Generator: New Generation, From Example…, Open Recent Generation.
- The Recent section has its own `ScrollArea` with a stable `id_salt`, a left
  inner margin and a fixed maximum height of about half the sidebar. Rows show
  an icon, the title and a kind/draft badge.
- Clicking a Recent row: if a tab's `current_uri` matches, focus it
  (`Action::FocusTab`); otherwise open the saved file or the draft.

### 4.2 Permanent home tab

The first Graphics tab is the activity's permanent tab and is a dashboard:

- **What To Do**: Open Image, Open Icon, New Diagram, Generate Diagram, plus a
  Recent shortcut.
- **Functions**: the same four entries with a one-line description, so the page
  works when the sidebar is hidden.
- **Open tabs**: `hs_ui::dashboard::open_tabs` filtered to Graphics, with the
  last screenshot of each pane and click-to-focus.

`home_is_dashboard()` is true. A dashboard reports no `current_uri` and no
`uri_affinity`. The old layout migration is in §8.3.

### 4.3 Image and Icon

Both tabs use an in-pane layout, not the activity sidebar:

```text
toolbar
[ settings panel 280–320 px | preview canvas ]
```

- The existing image operations and tests are preserved.
- The Icon tab adds:
  - ICO frame selection and multi-size assembly (existing);
  - a transparent-background checkerboard and pixel grid at high zoom;
  - **colour-key transparency**: an eyedropper picks a colour from the icon, a
    tolerance slider (0–100) selects the matching pixels, and one operation
    makes them transparent. It is non-destructive, lives in the operation
    list, and previews live. Undo removes it.
- "Convert to Icon" renders the current image (committed operations plus the
  live preview) and opens a new Icon tab. The handoff is passed through
  `GraphicsHub`, so an unsaved image needs no temporary file.

---

## 5. Diagram Editor (Release 1: flowchart and architecture)

### 5.1 Canvas

- World/screen transform with pan (Space+drag or middle mouse) and zoom
  (Ctrl+wheel, +/−, 0 = fit).
- Grid, snap-to-grid, alignment guides.
- Selection: click, Shift+click, marquee, move, eight resize handles, arrow-key
  nudge (Shift = 10 px), Delete.
- Direct manipulation only in R1: no rotation, no groups moving children, no
  pages/layers.

### 5.2 Shapes

Rectangle, rounded rectangle, ellipse, diamond (decision), stadium
(start/end), parallelogram (input/output), cylinder (database), note, text,
container/frame. A small palette, not a stencil library.

### 5.3 Connectors

- Drag from a node's four ports; arrowheads, dashed and emphasis variants.
- Routes: straight, orthogonal (elbow), curved; automatic port selection by
  relative position; manual waypoints.
- Labels with a background patch; double-click to edit.
- No obstacle-avoiding routing in R1.

### 5.4 Text and style

- Double-click a node or edge label to edit in place; Enter commits, Esc
  cancels.
- Properties: fill, stroke, text colour, font size, bold/italic, alignment,
  dash, arrowheads, corner radius, z-order, lock.
- Alignment: left/centre/right/top/middle/bottom, distribute horizontally/
  vertically.

### 5.5 Undo, clipboard, keyboard

- Undo/redo with document snapshots; one snapshot per drag, not per pixel.
- Copy/paste/duplicate/delete, select all.
- Ctrl+Z/Y, Ctrl+C/V/X/D/A, arrows, Delete, Space+drag, +/−/0.
- Every action also has a palette command and a Fluent title.

### 5.6 Outline and accessibility

The properties panel includes an **Outline** list of nodes and edges. It is
keyboard-navigable, selects the matching canvas item, and is the screen-reader
path to the canvas content. Custom canvas controls carry `widget_info`.

### 5.7 Templates

Blank, Flowchart (start → step → decision → end), Architecture (client →
service → store, with a boundary). Templates are ordinary `DiagramDoc`s, not a
separate format.

### 5.8 Mermaid import (R1: flowchart)

Paste Mermaid text, parse with `hs_editor::mermaid`, convert the flowchart AST
to a `DiagramDoc`, lay it out, and report dropped constructs. Mermaid is not a
save format. Sequence import arrives with the sequence type in R2.

---

## 6. Diagram Generator

### 6.1 Layout

```text
toolbar: type picker, examples, Generate/Stop, Open in Editor, Export, AI
[ prompt panel 360 px | preview ]
```

- Left: type (Flowchart/Architecture), prompt, three clickable example prompts,
  a template button, Generate/Stop, and the validation state.
- Right: the preview, zoom/fit, "Open in Editor" and "Export".
- No AI provider configured: templates and examples still work; a hint links to
  Settings → AI Assistant.

### 6.2 Semantic IR, not pixel coordinates

The generator asks for the Archify-adapted **semantic IR** (lanes/columns/rows,
no absolute coordinates); `hs-diagram` lays it out deterministically. Asking a
model for `x/y` produces overlaps and cannot be repaired reliably.

Example prompts offered in the UI:

- “Draw a checkout flow: browser → CDN → API → Postgres, with a Redis cache on
  the API path.”
- “Architecture for a web app: browser, API, worker, Redis and PostgreSQL;
  group the API, worker and databases behind a private network boundary.”
- “Turn this into a flowchart: receive order, validate stock, charge card,
  reserve stock, send confirmation; on payment failure, release the stock.”

### 6.3 AI call and repair

- Reuse `hs_core::ai::chat`; stream on the runtime; cancel by dropping the task
  or the panel's stop flag.
- System prompt: the compact adapted schema, one example per type, and the
  rules (unique ids, every edge endpoint exists, no empty labels, limits).
- Parse strictly (strip Markdown fences), validate, and if invalid send one
  repair round containing the rule codes and the exact subjects.
- Keep the last good preview while a new generation is running or invalid.
- Cap output: 200 nodes/components, 500 edges/connections, 4 KB per label.
- Show provider/model and errors; never send project files unless the user adds
  context.

### 6.4 Offline templates

Flowchart and architecture templates are built into `hs-diagram`. They are the
fallback, not an error state, and produce the same `DiagramDoc` as AI output.

---

## 7. AI Assistant in all four tools

The shared `hs_ui::ai_assistant::AssistantPanel` remains the UI. Each tool
provides a context builder and quick actions:

| Tool | Context | Quick actions |
|---|---|---|
| Image | path, dimensions, format, operation list | Explain Image, Suggest Edits, Best Export, Check Accessibility |
| Icon | the same text context and actions as Image | Explain Image, Suggest Edits, Best Export, Check Accessibility |
| Diagram Editor | outline of nodes/edges/groups, no pixels | Review Layout, Suggest Missing Nodes, Name Diagram, Accessibility Check |
| Diagram Generator | type, prompt, last result summary | Refine Prompt, Critique Result, Alternative Layout, Explain Diagram |

- Four fixed quick actions plus **More Suggestions**, which asks the AI for
  three tailored actions and shows them as buttons; clicking one fills the
  prompt and sends.
- **Review-only.** The assistant never applies an edit; the user applies it by
  hand. Image/Icon context is text-only in R1 (no pixels).
- No provider configured: the buttons are disabled with a Settings link.
- **Implemented 2026-10-20.** All four tools show the shared panel; the
  Diagram Editor and Generator add an Assistant button to their toolbar and
  pass a text outline/prompt summary as context, with the four actions above.
  Verified in the app with the generator panel open (no provider, so the
  Settings hint is what the screenshot shows).

---

## 7.1 Outstanding R1.1 work (planned 2026-10-20)

The R1 core is merged. This is the remaining work, in order, with the branch
`feat/diagram-project-save`:

| Phase | Item | Approach | State |
|---|---|---|---|
| 8a | Save to project | `Tool::project_roots` is published by the shell through `OpenTabs`; the Save to Project modal has Project, Subfolder (default `docs/diagrams`), File name and overwrite confirmation; `hs_diagram::io::save` writes atomically and the tab takes the saved path. | Completed 2026-10-20 |
| 8b | Draft autosave | Debounced (1 s) while a diagram is dirty, writing the same stable draft file as close; close still writes synchronously. | Completed 2026-10-20 |
| 8c | Resize and align | Bottom-right resize grip on the selected node; Shift-click multi-select (outline clears it); Align left/centre/right/top/middle/bottom and Distribute horizontally/vertically over the selection. | Completed 2026-10-20 |
| 8d | Edge labels and waypoints | Click an edge near its route to select it, edit its label, Add Waypoint / Clear Waypoints, and drag the circular waypoint handles. Delete removes the selected edge. | Completed 2026-10-20 |
| 9 | Release 2 imports | ERD and sequence diagram types, Archify JSON import and draw.io XML import, each with validation and a round-trip test. | Imports done 2026-10-20; ERD, basic sequence templates and Mermaid sequence import done 2026-10-20; full participants/messages/fragments remain |

The project list is deliberately owned by the Editor: Graphics must not guess
from recent files, and a tool may not depend on `tool-editor`. The shell owns
`Services`, so the Editor publishes roots there and Graphics only reads them.

---
## 8. Files, drafts and recents

### 8.1 `.hdiag` (editor-native)

A single pretty-printed JSON file. The same bytes may also be named
`<name>.diagram.json`; open, affinity and recent handling treat both
extensions alike, and Save always writes the canonical `.hdiag`.


```json
{
  "schema": 1,
  "kind": "flow",
  "title": "Checkout",
  "subtitle": "",
  "page": { "size": "fit", "margin": 24, "background": "#ffffff" },
  "nodes": [
    { "id": "browser", "label": "Browser", "kind": "process",
      "x": 0, "y": 0, "w": 140, "h": 56, "style": {} }
  ],
  "edges": [
    { "id": "e1", "from": "browser", "to": "api", "label": "HTTPS",
      "route": "orthogonal", "arrow": "end", "via": [] }
  ],
  "groups": [],
  "meta": { "created": "...", "modified": "...", "app": "..." }
}
```

- Absolute geometry is the editor's; the IR is not stored in the file.
- Unknown fields are preserved or warned about (lenient evolution).
- File size, node count, edge count and label length are capped on open.

### 8.2 Drafts and recents

```text
HyperStackData/graphics/drafts/<sanitised-title>-draft-<unix>.hdiag
HyperStackData/graphics/recents.json
```

- Never inside the workspace: a workspace can be an external `.hyperstack/`
  folder in a git repo, and drafts must not pollute it.
- The tab's `save_state` carries `{ kind, doc, ... }`, so a running tab
  survives a restart with its document intact.
- Closing a diagram tab writes the draft (close-time autosave) and adds a
  `(draft)` row to Recent. A debounced autosave while typing is still to do.
- A successful save writes the final `.hdiag`; closing a clean saved tab
  writes no draft.
- Closing a diagram tab keeps the draft and its Recent entry. There is no
  “lose your work” prompt for diagrams.
- Recent rows: kind, title, path or draft id, last touched. Cap 50, dedupe by
  path/draft, prune older than 30 days. A missing file is greyed with Remove.
- If `restore_tabs` is off, orphan drafts still appear in Recent.

### 8.2a Import (Release 2, partial)

The Diagram Editor's **Import** menu reads:

- **Archify JSON** — the same strict workflow/architecture IR and validator as
  the generator; an invalid document is refused with the rule codes and no tab
  changes.
- **Mermaid sequence** — a `sequenceDiagram` block, participants, solid and
  dashed arrows and message labels; notes and fragments are ignored for now.
- **draw.io XML** — the plain or compressed `mxGraphModel` form. Vertices become
  nodes at their own geometry (rounded, rhombus, ellipse start/end, cylinder,
  note and text styles are mapped), edges with a source and target become
  connections, and the document is fixed-layout (`auto_layout = false`).
  Compressed pages are inflated transparently. Multiple pages still import the
  first one.

Import replaces the current diagram and marks the tab dirty; undo returns to
the previous document.
### 8.3 Save to a project

A "Save Diagram" modal:

- **Project** dropdown from `OpenProjects` (plus Workspace and Choose Folder…).
- **Subfolder**, default `docs/diagrams`.
- **File name**, derived from the title and sanitized.
- **Format** `.hdiag`.
- Remember the last project/subfolder; confirm overwrite; write atomically.
- Default with no project open: `<workspace>/diagrams/`.
- After saving, the editor's watcher refreshes its tree and the file is
  git-diffable.

### 8.4 Home-tab migration

Changing Graphics from a working first tab to a dashboard is a saved-layout
change. `ensure_home_tabs` only inserts a home when none exists, so an upgraded
layout would keep the old image tab as `home`. The migration clears the old
Graphics home flag and inserts a dashboard; a test uses a saved layout with an
image home tab.

---

## 9. Archify adaptation (schema and validation only)

Pinned to **v3.2.0**, commit `208acba8d412335185b2056fedf54b3405df43fb`.
The full MIT text and both copyright lines are in
[THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md).

### 9.1 Adapted files

| Upstream | Adapted |
|---|---|
| `common.schema.json` | `id`, `point`, `side`, `componentType`, `variant`, `relationshipWidth` |
| `workflow.schema.json` | lanes, nodes, edges, groups; `meta.title/subtitle` |
| `architecture.schema.json` | layout, components, boundaries, connections; `meta.title/subtitle` |
| `erd.schema.json` (R2) | entities, attributes, keys, relationships |
| `sequence.schema.json` (R2) | participants, messages, fragments |
| Two example JSONs | used as fixtures when the importer lands (R2) |

Dropped: `meta.output`, `views`, `sources`, `repository`, `translations`,
`brand`, `legend`, `cards`, animation, visual preset, quality profile,
engineering profile, HTML output-path rules, brand marks, fonts, renderer,
viewer and generated validators. The renderer is entirely native.

### 9.2 Validation rules and codes

Schema: required fields, enums, `additionalProperties: false`, id pattern
`^[a-zA-Z][a-zA-Z0-9_-]*$`, min items, unique ids, edge endpoints, lane
references, boundary `wraps` references, column/row ranges.

Codes (internal, Fluent messages): `diagram/required`,
`diagram/unknown-field`, `diagram/invalid-id`, `diagram/duplicate-id`,
`diagram/unknown-node`, `diagram/unknown-lane`,
`diagram/unknown-boundary-target`, `diagram/out-of-range`,
`diagram/too-many-elements`, `diagram/empty-label`,
`diagram/unsupported-type`.

Strict for the IR; lenient for `.hdiag`.

### 9.3 IR shapes (R1)

```json
{
  "schema_version": 1,
  "diagram_type": "workflow",
  "meta": { "title": "Checkout", "subtitle": "" },
  "lanes": [{ "id": "client", "label": "Client" }],
  "nodes": [{ "id": "browser", "lane": "client", "col": 0,
              "type": "frontend", "label": "Browser" }],
  "edges": [{ "from": "browser", "to": "api", "label": "HTTPS",
              "variant": "default", "role": "main", "route": "auto" }],
  "groups": []
}
```

```json
{
  "schema_version": 1,
  "diagram_type": "architecture",
  "meta": { "title": "Production" },
  "layout": { "mode": "grid", "cols": 4 },
  "components": [{ "id": "api", "type": "backend", "label": "API",
                   "row": 0, "col": 1 }],
  "boundaries": [{ "kind": "security-group", "label": "VPC",
                   "wraps": ["api"] }],
  "connections": [{ "from": "web", "to": "api", "label": "HTTPS",
                    "route": "orthogonal-h" }]
}
```

Import mode accepts Archify's extra fields (`meta.output`, `pos`, `size`,
`via`, `viewBox`) and drops them with a warning list.

---

## 10. Export

| Format | Route | Options |
|---|---|---|
| PNG | Scene → SVG → `resvg` | 1x/2x/3x; white or transparent background |
| SVG | Scene → SVG | embedded font; vector |
| PDF | Scene → SVG → `svg2pdf` | one page; Fit diagram / A4 / Letter |

- Export runs as a `Job`; write atomically; never overwrite without a
  confirmation.
- Default file name = diagram title; default folder = the diagram's folder, or
  the last export folder.
- Fonts are bundled; the same bytes are used on screen and in the exports.
  Characters outside the bundled font are a documented limitation in R1.
- PDF is vector with a subset font; text remains selectable.

---

## 11. Tests

- `hs-diagram` unit tests: schema round-trip, validation codes, layout
  determinism, routing, hit-testing, text wrapping, SVG escaping, export
  dimensions.
- Golden fixtures: one flowchart and one architecture doc → SVG/PNG/PDF.
- Compatibility fixtures: Archify examples convert; invalid fixtures produce
  the expected codes; `.hdiag` never contains `meta.output`.
- `egui_kittest`: sidebar rows and context menus, dashboard cards, save dialog,
  prompt panel, assistant quick actions.
- GUI smoke (manual harness): right-click Image, new diagram, drag/resize,
  export, restart and reopen the draft.
- Migration test: an old layout with an image `home` tab gains a dashboard.
- Notice test: the notice exists, has both copyright lines and the MIT text.

---

## 12. Constraints and risks

- **UI thread never blocks**: drafts, saves, exports and AI calls run as Jobs.
- **Portable data**: drafts/recents under `HyperStackData`; exported images
  where the user chooses.
- **Every string in Fluent**; Title Case for labels, sentence case for hints.
- **Accessibility**: Outline list, keyboard operations, `widget_info`.
- **Scope**: full draw.io parity is out of scope; R1 is the common 20%.
- **Routing**: simple orthogonal routing in R1; no obstacle avoidance.
- **Performance**: cap documents and cull; test a 500-node document.
- **Fonts**: bundled Latin coverage; CJK/system-font export is a later feature.
- **Security**: untrusted `.hdiag` is capped, validated and escaped; no external
  paths or scripts.
- **Archify**: schema/validation only, pinned commit, notice shipped in the
  repo, About page and release zip; no renderer or brand assets.

---

## 13. Release 2 plan

1. ERD: adapted schema, table layout, editor, generator, fixtures.
2. Sequence: adapted schema, lifeline layout, editor, generator, Mermaid import.
3. Archify JSON import: paste/open, strict validation, conversion, warnings.
4. draw.io XML import: plain + compressed, first page, `mxCell`
   vertices/edges, style/label mapping, small images, warnings.
5. `.diagram.json` alias, schema migration, polish, docs.

The detailed R2 depth (ERD DB-schema import, sequence fragments, draw.io pages/
images) is decided at the start of R2.
