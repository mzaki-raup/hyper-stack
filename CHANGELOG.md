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

- Specified the **Container activity** - Docker (single container,
  Compose, Swarm), Podman, Kubernetes and Windows containers - as
  `docs/tools/containers.md` §10 on top of that document's existing
  resource requirements, with ADR-026, and planned it as eight
  releasable phases in the roadmap and the checklist. **Nothing is
  built yet.** The decisions worth arguing about: a container host is a
  `ConnectionProfile` of a container kind rather than a new store, so
  the vault, per-environment hosts, read-only handling and ordering are
  inherited rather than rewritten; **kubeconfig is referenced by path
  and context rather than copied into the vault**, because a copy goes
  stale on re-provisioning and breaks the `exec` credential plugins
  that resolve relative to the real file; logs and shells are handed to
  `tool-logs` and `hs-term` rather than rebuilt; and
  `hs_ui::table::DataTable` has to stop clipping and start scrolling
  first, which fixes S3 and the SFTP pane on the way.
- **Find Everywhere** (§15.3), and the cross-tool searcher registry it
  needed. A trace, request or correlation id in a record - taken from a
  field named like one, or from a UUID, long hex string or ULID in the
  line - is searched in parallel across every other open Logs tab, the
  SQL query history and the API client history, with the results grouped
  by tool under the record and each row opening the right tab.

  The registry is `hs_core::search` on `Services`: each tool implements
  a `Searcher` over its own store and the binary registers them, so the
  asking tool never depends on the answering one (ADR-004). An error is
  kept distinct from an empty result, so "could not look" and "found
  nothing" read differently; a searcher that panics becomes a failed
  answer rather than taking the search down; paths resolve at search
  time, so switching workspace cannot leave a searcher reading the old
  one; and a Logs tab excludes itself without excluding the other Logs
  tabs. Redis is the one source in the spec not yet wired up - it needs
  a live connection and `tool-redis`'s driver - and is now a `Searcher`
  impl away.
- **The time strip** above the list (§10, §15.6): records per bucket over
  the *filtered* window, so it answers "when did this happen" rather than
  "when was anything logged". The bucket follows the span in round units
  a reader recognises, a bar takes the colour of its worst level, and a
  gap stays a gap. It is a navigator: clicking a bar jumps the list to
  the first record in it, and dragging across it selects a range that
  becomes the after/before chips.
- **Compare Windows** (§15.8): drag two ranges, set them as A and B, and
  see what a deploy changed - shapes only in B, shapes only in A, and
  shapes whose count moved - counted by the same normalised shape key the
  repeat collapse uses, biggest move first. A row filters the list to
  that shape in that window.
- **Investigations** (§15.7): a whole Logs tab saved under a name - the
  source, the filter chips, the highlight rules, the pinned columns, the
  bookmarks *with their notes*, the time zone and Follow - in
  `logs/views/<slug>.toml`, with its own sidebar section. Reopening one
  restores the tab as it was, deliberately without Follow: an
  investigation is a frozen picture, and following would lose the place.
- **Errors reach the Problems dock tab** (§15.2). A highlight rule gains
  a **Report** switch and a per-rule cooldown; a match posts to a new
  `Services`-level inbox (`hs_core::problems`) any tool can write to,
  deduplicated by shape with a count so a storm is one row that climbs
  rather than a thousand rows. Clicking a row focuses the exact tab it
  came from and scrolls to that record. The dock's Problems tab had been
  a placeholder.
- **Highlight rules have an editor** at all now (toolbar, or
  `Logs: Highlight Rules…`): pattern, colour, bold, enabled, Report and
  its cooldown, with an invalid regular expression called out rather
  than silently ignored.

### Fixed

- `shape::key` now normalises a measurement with its unit attached, so
  `took 31ms` and `took 904ms` are one shape. There is no word boundary
  between a digit and a letter, so the bare-number rule never matched the
  `31` in `31ms` - which made every such row its own shape and defeated
  both the repeat collapse and the window diff.

## 0.82.1 — 2026-10-23

### Fixed

- Four Logs reports, providers in a card, and image properties (717279e6)

## 0.82.0 — 2026-10-23

### Added

- Summarise a thread dump by state, and follow its deadlock cycle (e44487c9)
- Read JVM thread dumps, and stop the assistant hanging on a picture (c580ec5e)

## 0.81.0 — 2026-10-23

### Added

- Configurable AI Assistant prompt buttons (a2e9423d)

### Fixed

- Find Docker and systemd inside WSL (a05c0f4c)
- Logs service detection, tab names, and a scan per tab (bdd9d736)

### Security

- Take the tokio 1.54.1 fix for the AsyncFd unsoundness (6b508066)

## 0.80.1 — 2026-10-23

### Fixed

- Colour the level word, fill the service form, fit assistant tables (d9d1fc3d)

## 0.80.0 — 2026-10-23

### Added

- Logs sidebar sections, Running Service, horizontal scrolling (3685aadd)
- An assistant, a dashboard and three Logs fixes (09a67cd3)
- Collapse repeats in the Logs tool (b9c9e753)

### Fixed

- Read every level alias, in any case (f51cbb3a)
- Restore the database screenshots deleted by a stray glob (cef5e3b7)

### Security

- Take the ctutils fix for RUSTSEC-2026-0385 (31e4c2ef)

## 0.79.0 — 2026-10-23

### Added

- Redaction rules for the Logs tool (d7123b4d)

### Fixed

- Read every shape ComfyUI describes a combo input in (453d8e81)

## 0.78.0 — 2026-10-22

### Added

- Remove split panes (0d59d4e0)

## 0.77.0 — 2026-10-22

### Added

- A strict 1720 x 860 minimum window, Full Screen or Custom only (4b43ca79)

## 0.76.0 — 2026-10-22

### Added

- Resize the window from Settings, General, Startup (55343765)

## 0.75.0 — 2026-10-22

### Added

- The Graphics sidebar uses the shared tool-list row (bd807d17)
- Upscale as its own job, enhance stops resizing, bigger workspace logos (34c198ac)

## 0.74.0 — 2026-10-22

### Added

- Update from the release zip, per-workspace rail and sidebar, plain activity icons (b190e9ef)

## 0.73.0 — 2026-10-22

### Added

- Logs activity, Logs dock tab, readable ComfyUI errors, assistant layout (626e8eff)

### Fixed

- Compact the rail, measure the assistant composer, open logs at the tail; retake every screenshot (3f6ca719)
- Update the tool-logs lock entry to 0.72.0 (95086d14)

## 0.72.0 — 2026-10-22

### Added

- OAuth 2.0 and AWS SigV4 authentication in the API client (095366f9)

### Fixed

- Ask the server for transaction state, import and export off the UI thread, mock the image servers (e6bc9430)

## 0.71.1 — 2026-10-22

### Fixed

- Fluent for every diagram label, protected-host guards in SSH (364c3272)

## 0.71.0 — 2026-10-22

### Added

- Tabbed Manage Collection, live variable edits, Graphics function icons and Image menu (f38e8e63)

## 0.70.0 — 2026-10-22

### Added

- Self-hosted image providers, capability-grouped AI settings, rendered answers (d67acd71)

## 0.69.0 — 2026-10-22

### Added

- Edit and enhance the current picture through 1min.AI (3751fc5b)

## 0.68.0 — 2026-10-21

### Added

- DeepSeek in the AI gallery, SDXL 1.0 as the default image model (427a0a9a)
- Package libraries, Manage Collection, AI pictures, per-activity sidebars (9f4cbe71)

## 0.67.0 — 2026-10-21

### Added

- Frame-to-frame edges and icon-only ERD/sequence toolbar buttons (876576f0)

## 0.66.0 — 2026-10-21

### Added

- Sequence note strips and draw.io image cells (7aebfff7)

### Fixed

- Review pass over the week's code, narrow-window toolbars and panels (4ae67227)

## 0.65.0 — 2026-10-21

### Added

- Choose a draw.io page on import (cdffdb8f)
- Finish sequence fragment ranges and ERD DDL import (80b52fc9)

### Fixed

- Diagram editor polish, markdown assistant, API rename dedup (37caa302)

## 0.64.0 — 2026-10-21

### Added

- Add structured ERD columns and sequence fragments (4d07674d)

## 0.63.0 — 2026-10-21

### Added

- Continue graphics R2 with schema migration and diagram types (8b544666)

## 0.62.2 — 2026-10-21

### Fixed

- Place the edge label indicator on the painted label (e1811f85)
- Tidy frame title controls and label indicators (5fadaeee)

## 0.62.1 — 2026-10-21

### Fixed

- Render the missing frame title controls and Add Frame button (49beacd8)

## 0.62.0 — 2026-10-21

### Added

- Frame label placement, inner frames and API folder tools (5c386b7d)

## 0.61.2 — 2026-10-21

### Fixed

- Draw coloured material icons in the API tree (11e2861a)

## 0.61.1 — 2026-10-21

### Fixed

- Keep new scratchpad notes above the saved order (96d7102f)

## 0.61.0 — 2026-10-21

### Added

- Material API tree icons and scratchpad note order (4d7aff43)

## 0.60.3 — 2026-10-21

### Fixed

- Show the outline separator for frame selection (abd5334b)
- Stop the generator assistant overlay overlapping quick actions (9514151d)


## 0.60.2 — 2026-10-21

### Changed

- Reapply "fix: select frames on click and keep the outline separator" (d6b7ba06)

## 0.60.1 — 2026-10-21

### Fixed

- Select frames on click and keep the outline separator (2cbc2e78)

## 0.60.0 — 2026-10-21

### Added

- Resize diagram frames and refresh screenshots (ccb7b3aa)

## 0.59.0 — 2026-10-20

### Added

- Markdown assistant with inline diagram actions (7edb6857)

### Fixed

- Clear the new webpki security advisories (560aeef6)
## 0.58.0 — 2026-10-20

### Added

- Improve diagram selection, frames and assistant layout (bf9bdf02)
## 0.57.0 — 2026-10-20

### Added

- Select and highlight diagram frames (5013b559)

### Fixed

- Keep every tab close cross visible (97143ac8)
## 0.56.0 — 2026-10-20

### Added

- Add diagram colours, frames and generator templates (a8c37a2f)
## 0.55.0 — 2026-10-20

### Added

- Align Graphics UI and add offline diagram drafts (a65be63e)

## 0.54.0 — 2026-10-20

### Added

- Import Mermaid sequence diagrams (6a5adbda)
- Add ERD and basic sequence diagram types (7ba38aa5)

## 0.53.0 — 2026-10-20

### Added

- Accept the .diagram.json alias (60062238)

## 0.52.0 — 2026-10-20

### Added

- Import compressed draw.io pages (59b2b526)
- Import Archify JSON and draw.io XML (4186cd63)

### Fixed

- Decode compressed draw.io percent escapes as bytes (2b2d496c)

## 0.51.0 — 2026-10-20

### Added

- Finish the diagram editor project, draft and edge work (90e74ddd)

## 0.50.0 — 2026-10-20

### Added

- Keep closed diagram tabs as Recent drafts (3a221236)
- Repair invalid generated diagram IR once (1dfe6bf7)
- Show third-party notices on About (dae76a33)
- Add Graphics AI assistants to diagrams and the generator (d8e6b6bd)

## 0.49.0 — 2026-10-20

### Added

- Add the diagram generator (39693ee2)

## 0.48.0 — 2026-10-20

### Added

- Add the diagram core and editor (32cc1ccc)

### Fixed

- Update the hs-diagram lock entry to 0.47.0 (37f80cdd)

## 0.47.0 — 2026-10-19

### Added

- Add icon colour-key transparency (2480bb24)

## 0.46.0 — 2026-10-19

### Added

- Add the Graphics activity shell (519fdba0)

## 0.45.1 — 2026-10-19

### Fixed

- Open the New Collection dialog on the dashboard (034accb0)

## 0.45.0 — 2026-10-19

### Added

- A reorderable CLI list in Terminal settings (f4d132e7)

### Fixed

- Re-read PATH when refreshing the CLI suggestions (2ff826b1)

## 0.44.2 - 2026-10-18

### Fixed

- Scroll every sidebar list under its fixed header (0c254b29)

## 0.44.1 - 2026-10-18

### Fixed

- Re-check the paused operation after a status refresh (f5f593ff)

## 0.44.0 — 2026-10-18

### Added

- A block-by-block conflict editor (fd6e08e1)

## 0.43.0 — 2026-10-18

### Added

- Edit credential secrets and export environments (2756c9e2)

## 0.42.0 — 2026-10-18

### Added

- Interactive rebase and annotated tags in Source Control (f4c593c8)

## 0.41.0 — 2026-10-18

### Added

- Branches, remotes, tags and merge/rebase in Source Control (03472454)

## 0.40.0 — 2026-10-18

### Added

- Import environments from settings (695513a9)

## 0.39.0 — 2026-10-18

### Added

- A regex tester and a text diff (d8760636)

## 0.38.0 — 2026-10-17

### Added

- Import Insomnia, Bruno and OpenAPI collections (f85354d3)
- Stage, unstage or discard one hunk from the diff view (f0f1a959)
- Commit, push, pull, stash and branches from Source Control (9bb7e30f)
- A favourites bar under the address bar (f8e6cc3e)

### Fixed

- Favicons and room between the favourites bar's chips (cd3b5042)

## 0.37.0 — 2026-10-17

### Added

- Build and run a project from the editor, in its own terminal (08f89192)
- A terminal inside the editor (c46314ad)

### Fixed

- A request or folder can be dragged anywhere in the sidebar (61bdd1c6)

## 0.36.0 — 2026-10-17

### Added

- A home tab for the workspace's API client (034f41ed)
- Dashboards for Terminal, SSH, Devtools and the Scratchpad (1a0a901b)
- Dashboards for Redis, NoSQL and S3 home tabs (9a74c403)
- Dashboards for an activity's home tab, starting with Database (392adcba)
- A permanent home tab per activity, and no add-tab button (56bfbed4)

### Fixed

- A permanent tab is named after its activity, and pinning follows it (27cd3e45)
- A dashboard home tab is never taken over by a connection (eb76d322)

## 0.35.0 — 2026-10-17

### Added

- Unit conversion, a Java debug adapter, and unsaved new files (0d981465)
- Favourites and passwords per workspace, one tab per project (5c326be9)
- Rename in place, drop onto folders, and one action per click (25e92848)
- New Request lands in the list, and Duplicate copies the request (37ba8525)
- Pin from any connection list, and group the pins by type (a8f035f2)
- No splitting from the tab menu, and Duplicate knows when to refuse (0903720f)

## 0.34.0 — 2026-10-17

### Added

- Open Data runs in a query tab of its own (9a272300)

### Fixed

- Freeze the row number, keep the footer reachable, terminate statements (56297046)
- The result grid scrolls horizontally (e4638f7d)
- A refilled query tab shows its whole statement (cc0b81d2)

## 0.33.0 — 2026-10-17

### Added

- Prettify the open query, and give each HTTP method its colour (18d5470d)
- A tab per request, drags across folders, and numbered copies (d9dcfd53)
- The sidebar says where you were, and Get Started is in setup order (29eee499)

### Fixed

- The method picker is as tall as the URL field beside it (2c9633de)

## 0.32.0 — 2026-10-17

### Added

- Drag the collections sidebar into order, at every level (5d44984f)
- The Workspace page lists every workspace (ab958b5c)
- Query tabs per connection, and the address bar accepts typing (4db2a53a)
- Favicons on browser tabs, the database schema as its own column, tidier credentials (b88ca847)
- The AI assistant reaches Redis and the NoSQL tool (48d48a2f)

### Fixed

- Restored query text goes back to the tab it came from (d40e42cc)
- Forced dark mode covers the whole page (5f9bf5be)

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
