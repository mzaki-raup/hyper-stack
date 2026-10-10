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

- Terminal tool: local shells (PowerShell 7/5, cmd, Git Bash, WSL) in tabs and in the dock, profiles, restart/clear, "open terminal here".
- Settings → Updates: version display, check now, release notes, verified download, restart-and-install, revert to the previous version.
- Editor tool: project tree with a file watcher, open/save with encodings and line endings, syntax highlighting, find/replace, go to line, multi-cursor, external-change detection, Markdown preview, quick-open of project files (Ctrl+P).
- Right-click copies the selection or pastes the clipboard in the terminal and the editor (PuTTY convention), with settings to turn it off.
- `HyperStack <file-or-folder>…` opens the arguments in the editor.

### Fixed

- Release planning found no release once a `v*` tag existed (a one-element tag list was read as the string "v").

## 0.1.0 — 2026-10-10

### Added

- Phase 0 foundation ΓÇö workspace, core services, shell and two built-in tools (6aa27efc)

### Changed

- First commit (89f50b7b)
