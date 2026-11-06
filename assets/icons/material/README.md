# Material file and folder icons

SVG icons from the VS Code **Material Icon Theme**
(<https://github.com/material-extensions/vscode-material-icon-theme>, MIT,
licence text in `LICENSE.txt`), used for the project tree in the editor and
every other file listing (`crates/hs-ui/src/fileicons.rs`). Only the icons
HyperStack maps are vendored; the mapping from folder and file names to icon
names follows the theme's own conventions (`folder-src`, `rust`, `readme`, …)
with `folder-base` / `document` as fallbacks.

The theme generates its "open folder" variants at runtime, so the closed
folder icon is used for both states; the tree's caret shows the state.

To add an icon: drop `<name>.svg` from the theme's `icons/` folder here, run
`scratchpad/gen_fileicons.py` (or add the `include_image!` line by hand) and
map the name in `fileicons.rs`.

## Item icons drawn for HyperStack (not from the theme)

The Material Icon Theme is file-type oriented and has nothing for the things
a work pane lists — a volume, a topic, a stash, a table column. These 26 are
**drawn for HyperStack** in the theme's style (one flat filled path, no
strokes, a 24×24 viewBox) and are ours, not MIT-licensed upstream art:

`branch` `bucket` `collection` `column` `container` `field` `function`
`history` `metadata` `network` `node` `note` `object` `project` `queue`
`remote` `saved-query` `service` `source-control` `stack` `stash` `tag`
`topic` `view` `volume` `workspace`

They are mapped by item kind in `fileicons::for_kind`, which the activities
listed in `settings::MATERIAL_ICON_TOOLS` use for their lists (FEAT-0084).
Keep one hue per domain, as the existing ones do: warm for version control,
blue for infrastructure, green for relational objects, teal for documents,
amber for object storage, purple for messaging. A new kind with no sensible
picture gets **no** icon rather than a misleading one — `for_kind` returns
`None` and the pane keeps its glyph.

## Recoloured copies

`console-windows.svg` is the theme's `console.svg` with its fill changed to
Windows blue `#0078D4`: Command Prompt's mark, which has no logo of its own
(`hs_ui::icons::PRODUCT_LOGOS`, CR-0079).

`folder-shared.svg` (purple, #AB47BC) was added 2026-11-06 as the WebDAV
connection's mark - a network share - in every list, tab and card (CR-0079).
