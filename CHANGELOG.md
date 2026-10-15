# Changelog

All notable changes to HyperStack are recorded here. The release workflow
prepends a section for every version it publishes, generated from the
Conventional Commit subjects since the previous tag (see
[docs/09-ci-cd-and-updates.md](docs/09-ci-cd-and-updates.md) §6). Do not edit
released sections by hand.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) · Versioning:
[Semantic Versioning](https://semver.org/).

## Unreleased

- Home: a pinned dashboard tab first in every workspace (Get Started cards, workspaces, tools, recently closed tabs, pinned connections, keys); replaces the Welcome tab and Settings → Welcome.
- Appearance: System / Dark / Light mode with a dark and a light scheme; the choice is pinned into both egui theme slots so it survives a restart on Windows 10; clicking a palette swatch selects the scheme for its mode; activity rail order and optional labels; the sidebar toggle and Settings sit at the bottom of the rail.
- Accessibility (new settings page): Read Aloud through the operating system's voice with a title-bar button that lists what the active tab can read (selection, file, terminal screen, table cell, query, the assistant's last answer); speech rate; reduce motion.
- General: export and import settings and shortcuts as one TOML bundle after confirming the master password (providers merged by id, no secrets).
- Workspace settings: compare environments with another workspace and copy the missing ones either way.
- AI assistant: fenced blocks in answers get Copy and Insert; Insert types into the terminal or SSH shell, or into the SQL editor and the API client's body or script, without running anything. The API client and the graphics tool have the assistant too.
- Editor: full path of the active file under the file tabs (click copies); **Run** (F5) opens a terminal and runs the file or the project (Cargo, npm, Go, Gradle, Maven, .NET, Make, or the interpreter for the file type); pictures in the project tree open in the Graphics tool.
- Database: the cell viewer shows pictures as pictures and text as text, names documents (PDF, ZIP/Office, …) and can save the bytes; the hex dump stays available.
- Snip (Ctrl+Alt+S): the tab or the window as a PNG, copied and saved. Print… (Ctrl+Alt+P): HTML export of code, Markdown preview (Mermaid), query and result table, console screen, S3 listing, request and response, picture; opens in the browser with a Light/Dark switch for paper or PDF.
- Devtools (new tool): Base64 with auto-detect and picture/text/document preview, data URI and Basic auth header; hashes/HMAC table with compare; JWT decode, verify and sign (HS*, RS*, ES256); Firebase custom, access and ID tokens from a service account; Jasypt (`PBEWithMD5AndDES`, `PBEWITHHMACSHA512ANDAES_256`, `ENC(…)`) and the legacy DES/AES/MD5 DataEncryptor; the gateway's PKI message flow (SHA256withRSA + AES-256-GCM).
- API client (new tool): requests with params, headers, bodies (JSON, text, XML, form, multipart, binary), auth (inherit, basic, bearer, API key); collections and folders as JSON in the workspace; `{{variables}}` from collection, environment (vault secrets at send time) and globals; pre-request and post-response **JavaScript** with the `pm` API (environment/globals/collectionVariables/variables, request, response, test, expect, console) on Boa; collection libraries and snippets; Postman v2.1 collection and environment import, Postman export; curl import and Copy as curl; history; protected-environment confirmation; response body pretty/raw/preview, headers, cookies, tests, console.
- Graphics (new tool): open pictures from the editor or a dialog; resize, scale, extend canvas, crop, rotate, flip, brightness, contrast, hue, saturation, grayscale, invert, blur, sharpen as a removable operation list; convert between PNG, JPEG, WebP, BMP, GIF and ICO; icon frame extraction and multi-size ICO export.
- Browser (new tool, Windows): WebView2 pages in multi-row tabs with an address bar, back/forward/reload/home, per-page ad and tracker blocking (bundled blocklist plus Settings → Browser, cosmetic hiding), JavaScript switch, forced dark rendering, zoom, Inspect, Print, Clear Browsing Data; pages restored per workspace; Shift+click on a terminal link opens it here.
- Shell: the status bar counts work tabs only; the terminal can be opened with a command to type.
- Terminal: Nushell is detected (PATH, the MSI/winget install, `cargo install`, scoop) and listed between Command Prompt and Git Bash in the shell list and the dropdown.
- Devtools: one tab per utility (the sidebar opens or focuses it), inputs come back after a restart, and a typed key, password or service account can be moved into the vault with **Save to Environment…**, which leaves a `{{name}}` reference behind; every field expands `{{variables}}` (vault secrets included) when it runs.
- Variable peek: any field that holds a `{{variable}}` (devtools, the API client URL) gets an eye button that lists each reference's value with Copy; vault secrets stay masked until Reveal.
- Shell: tool state that changes without a tab-strip change (a devtools field, an API draft) is written to `layout.json` within a few seconds, so a crash or a forced stop loses little.
- Tests: the editor project-tab test builds its expected paths from the canonical temp folder, so it passes on the CI runner whose `%TEMP%` is spelled `C:\WINDOWS\SystemTemp`.

- AI: shared review-only assistant in Terminal, SSH, SQL and S3, with per-tool context previews and explicit send; no commands or data-changing actions are executed. API client is not implemented yet.
- Screenshots: marked the current screenshot set as predating the new AI panels; recapture requires the desktop/Xvfb fixture environment.
- Git view in the editor (read-only, via the installed `git`): the project root shows the branch with ahead/behind, changed files carry coloured letters and folders with changes a dot; a **Source Control** switch in the sidebar (Ctrl+Shift+G) lists conflicts, staged, changed and untracked files; clicking one shows its diff inline or side by side (Up/Down to step, Esc to go back); **History** (Ctrl+Shift+H) lists commits with branch and tag chips and shows each commit's message, files and diffs; the gutter marks added, modified and removed lines while you type. Status refreshes on file and `.git` changes. New crate `hs-git`.
- Editor: one window tab per project. Each opened folder is its own tab titled with the folder name, and inside it the project's open files are a second, multi-row strip of file tabs (close, reorder, Close Others / to the Left / to the Right, Copy Path, + for a new file). Files from outside the projects collect in an Other Files tab. This replaces Settings → Editor → Open Files In (Separate Tabs / Project Rows); a tab saved with several projects reopens them as separate tabs.
- Workspace: selected logo appears beside the workspace name; the Workspace settings page can rename workspaces and update/remove their logos after creation. Renaming also repairs the registry's last-opened marker.
- Settings: one pinned Settings tab is kept open in each workspace without stealing focus from a restored work tab; the old Welcome tab is now Settings → Welcome. Added clickable colour-scheme swatches and a configurable minimum width for editor file tabs.
- Editor: opening an existing project root focuses its existing tab, including normalized Windows paths. Encoding choices now distinguish UTF-16 LE/BE with and without BOM; clean files can be re-opened with an explicitly selected encoding.
- Shell: Quick Open no longer closes on its opening click. The notification bell opens retained notification history, dismisses on outside click, and has Clear All instead of opening the bottom dock; the app logo is shown beside HyperStack.

## 0.17.0 — 2026-10-12

### Added

- Choose where files open, separate tabs by default (27cf9151)
- Hs-git read engine for the editor's Git view (d90018da)
- List the models 1min.AI's chat API accepts (7cab12ef)

## 0.16.0 — 2026-10-12

### Added

- Open pinned connections from their chips (d7860d26)

## 0.15.0 — 2026-10-12

### Added

- Pick the model from the provider's model list (35ae7707)
- Several projects in one editor tab, each with its own row of file tabs (72e8b72b)

## 0.14.0 — 2026-10-12

### Added

- AI assistant with 1min.AI and OpenAI-compatible providers (205ac253)
- Open the data folder from About in Eden Explorer; keep editor folders expanded (17677f4f)

## 0.13.0 — 2026-10-11

### Added

- SQL database tool for PostgreSQL, MySQL/MariaDB and SQLite (59ca64d4)

### Security

- Never save a typed secret the vault could not take (b9397bb5)

## 0.12.1 — 2026-10-11

### Fixed

- Bump betas from the last stable tag; add Pester tests (80b68dcf)

## 0.12.0 — 2026-10-11

### Added

- S3 browser with SigV4 client, AWS profile import and guarded delete (562d5804)
- Tunnels service for local port forwards (c1804a8b)
- Draw Mermaid flowcharts and sequence diagrams in the Markdown preview (2146cd54)
- Scrollback search and Ctrl+click links (34ef1ead)

## 0.11.0 — 2026-10-11

### Added

- Add MIT License (6dd4cec8)

## 0.10.0 — 2026-10-11

### Added

- Public GitHub mirror with per-forge update source (a2aac6c6)

## 0.9.0 — 2026-10-11

### Added

- Copy variables between environments across workspaces (f37645bb)
- {{ }} variable autocomplete with inline add (472583b9)
- Per-workspace logo and copy-from picker (0eaa35d1)
- Workspaces as projects; fix SSH dual-pane layout (fe430b94)

### Fixed

- Show connection kinds with proper casing (255a5f94)

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
