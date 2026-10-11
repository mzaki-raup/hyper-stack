# Changelog

All notable changes to HyperStack are recorded here. The release workflow
prepends a section for every version it publishes, generated from the
Conventional Commit subjects since the previous tag (see
[docs/09-ci-cd-and-updates.md](docs/09-ci-cd-and-updates.md) §6). Do not edit
released sections by hand.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) · Versioning:
[Semantic Versioning](https://semver.org/).

## Unreleased

### Added

- Workspaces as projects: create a new workspace, switch between them, and
  delete one from the workspace menu.
- New Workspace can copy environments and connections from any existing
  workspace, chosen from a "Copy From" dropdown.
- A workspace can have a logo (PNG, ICO, JPG, BMP, GIF, WEBP or SVG), picked
  when it is created and stored in the workspace folder so it travels with the
  project. The logo shows in the workspace menu and its switcher.
- Typing `{{` in an environment-aware field (the SSH connection's host, user,
  remote directory and startup command) opens a fuzzy autocomplete of the
  active environment's variables; a key can be picked with the mouse or
  keyboard, or a brand-new variable defined inline and saved to the
  environment.

### Changed

- Connection kinds now display with proper casing (DB, SSH, S3, Redis,
  MongoDB, …) everywhere they are shown, while still stored as lowercase ids.

### Fixed

- SSH dual pane now splits exactly 50/50 and tracks the window width; the
  shell no longer shrinks as the mouse moves over the panes.
- SSH file list is browsable again (double-click a folder to open it, a file
  to edit it) and the row cursor shows it is clickable.
- Disconnecting an SSH session no longer closes the app.

## 0.8.0 — 2026-10-11

### Added

- SSH + SFTP tool with a synced dual pane (8d6e0854)

## 0.7.0 — 2026-10-10

### Added

- Settings menu order, About with updates, icon buttons, status bar (e1ada2a7)

## 0.6.0 — 2026-10-10

### Added

- One tab per file; fix collapsed variable fields (31348d65)

## 0.5.0 — 2026-10-10

### Added

- Settings redesign, colour schemes, window state, instant terminal (8a2b6e24)

### Fixed

- Read panel sizes from egui's PanelState (outer_rect) (b945f352)

## 0.4.0 — 2026-10-10

### Added

- Material file and folder icons in the project tree (d6795a2c)
- One component per tab strip, headless install/revert, larger rail (83a45621)

## 0.3.0 — 2026-10-10

### Added

- Bundled Nerd Font Mono families and a system-font picker (6bb618b2)
- Settings open as a tab; font sizes say what they affect (56aba2b3)

## 0.2.0 — 2026-10-10

### Added

- Code editor tool with project tree, highlighting, find and preview (54514f67)
- Right-click copies the selection or pastes the clipboard (9e9d352f)
- Local terminal tool on alacritty_terminal with shell profiles (f76872ee)
- Settings > Updates with check, verified download, install and revert (15a38e06)

- Settings open as a regular tab (categories in the sidebar) instead of a floating window.
- Settings pages redesigned: cards, label/field grids, comfortable full-width text fields, icons in the category list; the Settings tab cannot be duplicated.
- Colour schemes: HyperStack Dark/Light, Dracula, Nord, One Dark, Monokai, Gruvbox Dark, Catppuccin Mocha, Solarized Dark/Light, GitHub Light; the terminal and editor follow the scheme.
- Window size, position and maximised state, sidebar width and dock height are restored on the next start.
- Terminal opens instantly: shell detection (which runs `wsl -l`) is cached and prefetched at startup.
- Tooltips appear above the pointer; toolbars and sidebar headers got vertical padding; the terminal's shell list has larger icons with hover and selection highlights and a framed hint panel.
- Hidden `--install-update` and `--revert-update` flags run the updater's install and revert headlessly (scripted end-to-end tests).
- Larger activity rail (56 px) and icons.
- The editor's project tree uses VS Code Material Icon Theme icons for folders and files (132 bundled SVGs, MIT).
- The tab strip shows one component at a time: the activity rail switches between editor tabs, terminal tabs, settings and so on instead of mixing every tool's tabs in one strip.
- Appearance → Interface Font Size now exists and applies; the terminal and editor font sizes say what they affect.
- Font families: JetBrainsMono, Fira Code and BitstromWera Nerd Font Mono are bundled (JetBrainsMono is the default for the terminal and the editor) and any font installed on the machine can be picked for the interface, the terminal or the editor.

### Fixed

- Plain absolute paths from the command line; narrower context menu (6e7a43e0)
- Keep typing in one undo group; add kittest view test (198ea9dd)

## 0.1.0 — 2026-10-10

### Added

- Phase 0 foundation ΓÇö workspace, core services, shell and two built-in tools (6aa27efc)

### Changed

- First commit (89f50b7b)
