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
