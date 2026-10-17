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

- The **AI assistant reaches Redis and the NoSQL tool**, the two that did
  not have it. Both are review-only: Redis can put a command in the
  console's input and the document store can fill the filter or pipeline
  field, and in both the user is the one who runs it. Each is told what is
  on screen — the open key and the last console command; the open
  container and the query — and nothing else.

- A database connection's tab holds **as many query tabs as you open**, in
  a strip above the editor like the editor's own files. Each keeps its own
  text, caret and unsaved mark; a tab is named by the saved query it holds
  or Untitled; opening a saved query opens another query tab on the same
  connection rather than another window tab. A window tab is still exactly
  one connection.

- The API client's sidebar can be **dragged into order** at every level:
  requests among their siblings, folders among theirs, collections among
  the collections. A collection remembers its place in an `order` field,
  since collections are files in a directory and a directory has no order
  of its own.

- **Settings → Workspace lists every workspace**, not only the one that is
  open: each with its logo, name and folder, the open one marked, and a
  click switches to it. New Workspace… is there too, so the page is a
  place to see what there is rather than only to rename what you are in.

- Browser page tabs show the site's **favicon**, and so does the browser's
  own tool tab. There is no favicon API in wry on Windows, so it is
  fetched from the origin's `/favicon.ico` and normalised to one small PNG
  — once per site, not once per tab, and the globe stays for a page that
  is loading or has no origin to ask.

### Changed

- The database tool's **schema is a column beside the editor** — version,
  filter, object tree, then Saved Queries and History under a separator —
  rather than a block nested in the connections list. Hiding the sidebar
  is about choosing a server; it no longer takes the schema of the one you
  are in with it.

- Settings → Security drops the **Connection String** column, which was
  empty for every kind but one (a document store's URI is now its
  Host/URL). Every column sizes itself to its content except Username and
  Password, which share one width so the two a reader compares line up.
  The Reveal Password dialog's buttons are right-aligned with the action
  outermost, like every other dialog in the app.

### Fixed

- The API client's folder and request menus go through Fluent. They had
  been written in English in the code, which nothing else in the app does.

- Forced **dark mode covers the whole page**. The filter was on `html`,
  which inverted its own dark background into a light one, so everything
  the page itself did not cover came out light — a short page looked half
  dark. It is on `body` now, with `html` left dark behind it.

- The browser's **address bar can be typed into**. A WebView2 child takes
  keyboard focus back the moment it is allowed to, so the field was
  unusable; the web view is now disabled outright while the address bar
  holds the keyboard, which is the only thing that holds.

- S3's **AI Assistant** button sits hard right like every other tool's; it
  was missed when the rest were moved.

## 0.31.0 — 2026-10-16

### Added

- A credentials table, SFTP commands in the shell, one place for the assistant (595cd5f5)
- Test Connection proves a profile before it is saved (44658fdb)
- Explain a checked cron expression, a result card per generator, one Graphics tab per image (e42e06d3)
- A cron builder that writes all three dialects at once (22ac02ea)
- Generators, and TOML and XML checking with formatting (236b53d0)
- Pin connections for real, and give the sections their own weight (022fa95b)

### Fixed

- The AI assistant is a column, not something drawn over the tool (f9da7f21)
- Pinned tabs keep their name, one tab per connection, New File creates files (e7c9aadd)

## 0.30.0 — 2026-10-16

### Added

- Download Zipped and Material file icons finish the SFTP pane (693dfa24)
- New File in the SFTP toolbar, and a permissions dialog (dea55ce9)
- Drag to reorder Settings -> Connections and the shell profiles (66e07413)
- Drag to reorder the database, Redis, NoSQL and S3 lists too (b121dcab)
- Drag a row to reorder a list, starting with the SSH hosts (9ffe2138)
- Tools can be hidden from the activity rail, and come back when they are used (47bd723e)
- Refresh PATH and extension status without restarting, and ask before running unsaved files (92591b03)

### Fixed

- Drag rows by a grip, so the rows themselves stay clickable (b5930200)

## 0.29.1 — 2026-10-16

### Fixed

- Find a language server's Windows shim, not the extensionless file beside it (0f7a4562)

## 0.29.0 — 2026-10-16

### Added

- Copy files and folders between the SFTP pane and the system clipboard (cf8cd881)
- Dock Inspect under the page, and tidy the Redis list and the sidebar headers (7cd43fe4)
- Document store client for MongoDB and CouchDB, finishing M3 (79ace060)
- A connections column in the sidebar, so one click moves the tab to another server (6ea51c6c)

## 0.28.0 — 2026-10-16

### Added

- Redis tool: SCAN key tree, type-aware viewers and editors, console with guards (ab533f4e)

## 0.27.0 — 2026-10-16

### Added

- Providers for Anthropic, Gemini, Azure OpenAI and AWS Bedrock, added from a service gallery (6c0c68b7)

### Fixed

- Give each YAML tab its own text (795bc086)
- Line up the workspace cards and set New Workspace apart (58fc2abc)

## 0.26.0 — 2026-10-15

### Added

- Assistant quick actions and Insert and Run, URL and YAML devtools, readable terminal selection (6f54e785)

## 0.25.0 — 2026-10-15

### Added

- Debugger through debug adapters: configurations, breakpoints, stepping, variables, call stack, console (0ea88a26)

## 0.24.0 — 2026-10-15

### Added

- Language server client: completion, hover, diagnostics, definition, references, rename, format (c0c38761)

## 0.23.0 — 2026-10-15

### Added

- Delete any workspace but the last, default included; workspace logos wherever the name shows (b72f5ca1)

## 0.22.0 — 2026-10-15

### Added

- Dictation keeps the phrase in flight and times out, Home workspace logos, DownloadΓÇª everywhere, rail labels on by default (63cb1427)

## 0.21.0 — 2026-10-15

### Added

- Microphone toggle in every assistant with offline speech to text (6eb6ba94)

## 0.20.1 — 2026-10-15

### Fixed

- Crop placeholder resizes from every edge, live Adjust sliders, Grayscale and Invert toggles (cf084c6b)

## 0.20.0 — 2026-10-15

### Added

- Scratchpad notes collection with AI, API client assistant that fills the client, graphics crop placeholder and tidy sidebar, title-bar icon rhythm (60a2a3f4)

## 0.19.0 — 2026-10-15

### Added

- Per-utility tabs, persisted inputs, secrets to the environment vault, variable peek; Nushell in the terminal (8f4a3b51)
- Run (F5) for files and projects; API client AI assistant; docs, screenshots and harness for the 2026-10-15 work (41731182)
- Embedded browser tool with WebView2 pages, ad and script blocking, forced dark mode, devtools and print (2f5c3b83)
- Devtools (JWT, Jasypt, PKI messages, hashes, Base64, Firebase tokens), API client with Postman import/export and JavaScript scripts, Graphics tool, Snip and Print (0423b966)
- Home tab, theme mode, rail order and labels, Read Aloud, settings import/export, AI Insert, DB binary preview (9824cee4)
- Workspace logo in the switcher, pinned Settings tab with Welcome page, notification history, Quick Open fix, editor encodings, review-only AI assistant in Terminal, SSH, SQL and S3 (b5073a50)

### Fixed

- Enable wry's devtools feature so Inspect compiles in release builds (90f079fc)
- Build the editor project-tab test's expected paths from the canonical temp folder (3c8bf729)

## 0.18.0 — 2026-10-12

### Added

- Git view in the editor: tree status, Source Control, diffs, history (8746f145)
- One window tab per project with a multi-row strip of file tabs (ca9199a7)

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
