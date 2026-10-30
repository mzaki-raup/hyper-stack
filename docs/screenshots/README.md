# Screenshots

Images used by the top-level [README](../../README.md). They show the build from this
repository, not mock-ups; the whole set was last refreshed on Windows 11 on
2026-10-22 (v0.72.0 plus the `tool-logs` branch), at 1400x960.

**The window no longer goes that small.** From 2026-10-23 the minimum is a
strict 1720 × 860 (`WindowState::MIN_SIZE`, owner's rule). Windows holds a
user's drag to it, and `HyperStackApp::enforce_min_size` pushes back against
anything else, a harness's `SetWindowPos`/`MoveWindow` included: the
undersized frame is accepted and then corrected, so a capture taken
immediately after the resize can still show the small window. A harness
asking for 1400x960 settles at 1720x860 within a frame or two, which means
the next refresh of these images will be at that size and the coordinates in
the recipes below no longer line up. Wait a beat after sizing the window,
and retake a whole page rather than patching one shot into the old set.

## How they were made

Captured from the debug build with `scripts/gui-smoke.ps1` (`start`, `click`, `dclick`, `rclick`, `mclick`,
`drag`, `wheel`, `focus`, `keys`, `shot`, `stop`). The app runs against a scratch
`--data-dir`; the window sits at 0,0 and is sized 1400x900, and the capture is
`GetWindowRect`, so the invisible 8 px border is part of each PNG. HyperStack
Dark, activity rail labels on. Clicks and keys go through `mouse_event` and
`SendKeys` at window-relative coordinates.

PNGs are quantised to 256 colours (`magick <in> -colors 256 -depth 8 <out>`)
to keep the repository small. `hyperstack-demo.gif` is a slideshow assembled
from the current set with
`magick <frames> -set delay 300 -loop 0 -resize 1200x -colors 256 -layers Optimize hyperstack-demo.gif`,
one screen every 3 s.

The assistant is covered by `settings-ai.png`, `editor-ai-review.png`,
`editor-ai-cascade.png`, `editor-ai-models.png`, `api-client-ai.png`,
`scratchpad-ai.png` and `ai-dictation.png`. The per-tool review-only panels
(terminal, SSH, SQL, S3, Redis, NoSQL) use the same layout; no separate
captures are kept for each.

## 2026-10-30 set: the Git activity, dashboards, variables, the Devtools toolbar

Captured at 1760x900 from the v0.102/v0.103 debug build with the session
harness (`hs.ps1`: `start`, `click`, `rclick`, `keys`, `wheel`, `shot`,
client coordinates = screenshot − (8, 31)), quantised with
`magick <in> -colors 256 -depth 8 <out>`. The fixture repository
`D:\Temp\hs-git-fixture` has a merge in progress (README.md conflicted
between `main` and `fix`), an unstaged change to `main.rs`, a stash, a
tag, and the `feature` and `topic` branches; `hs-bare.git` is its bare
clone for the Localhost source.

- `git-dashboard.png`, `git-repository.png` (File Status with a hunk's
  verbs under the paused-merge bar), `git-history.png`,
  `git-conflict.png`, `git-blame.png`, `git-flow.png`,
  `git-branch-menu.png`, `git-assistant.png`, `settings-git.png`,
  `settings-git-credential.png`.
- `git-walkthrough.gif`: those ten in that order,
  `magick <frames> -resize 1200x -colors 256 -layers Optimize -set delay 300 -loop 0 git-walkthrough.gif`
  (3 s a frame, 0.7 MB).
- `terminal-dashboard.png`, `db-dashboard.png`: the shared dashboard
  layout every activity's permanent tab uses since 2026-10-27.
- `settings-variables.png` (the Collection Variables and Global Variables
  cards beside the environments), `settings-globals.png` (the Globals
  editor).
- `devtools-toolbar.png`: a utility tab with its toolbar row.
- `settings-git.png` and `settings-editor.png` retaken after the
  two-column refactor of 2026-10-30 (list left, form right); both GIFs
  rebuilt with the new Settings → Git frame.
- `hyperstack-demo.gif`: the 112 frames of the 2026-10-22 set plus
  twelve new ones (the ten Git frames, the Terminal dashboard and the
  Globals editor, each `-resize 1200x -gravity center -background
  '#14161A' -extent 1200x823`), re-optimised with
  `magick <old coalesced> <new> -colors 256 -layers Optimize -set delay 300 -loop 0`:
  124 frames, 3 s each, 8.7 MB.

## 2026-10-15 set: Home, settings pages, Devtools, API client, Graphics, Browser

`home.png`, `settings-general.png`, `settings-appearance.png`,
`settings-accessibility.png`, `settings-workspace.png`,
`devtools-hashes.png`, `devtools-jwt.png` (with the variable peek popup
revealed), `devtools-jasypt.png`, `terminal-shells.png` (Nushell between
Command Prompt and Git Bash),
`devtools-pki.png`, `api-client.png`, `api-client-scripts.png`,
`api-client-ai.png` (an `hs-api` action block with Apply, answered by a
local mock model), `graphics.png`, `graphics-ops.png`, `graphics-crop.png`
(the crop placeholder), `scratchpad.png`, `scratchpad-ai.png`,
`ai-dictation.png` (the microphone toggle listening in the API client's
assistant; the phrase was played through the speakers with Read Aloud,
hence the garbled words), `editor-lsp-completion.png` and
`editor-lsp-problems.png` (rust-analyzer serving a small Cargo project:
the completion list filtered by typing with the item's documentation
beside it, and the Problems panel under the diagnostics' underlines),
`editor-debug.png` and `editor-debug-stack.png` (debugpy paused at a
breakpoint in a small Python file: the stopped line, the Variables tab
with the frame's locals, and the Call Stack tab) and
`browser.png` were captured on
Windows 11 from the debug build with the harness in
`scripts/gui-smoke.ps1` (`start`, `keys`, `click`, `shot`): `Start-Process
HyperStack.exe --data-dir <scratch>`, `SetProcessDPIAware`, the window
moved to 0,0 and sized 1400×900, keystrokes through `SendKeys`, clicks
through `mouse_event` at window-relative coordinates, and
`Graphics.CopyFromScreen` of `GetWindowRect` (so the 8 px invisible
border is part of each PNG). The theme is HyperStack Dark; the API client
shot shows a real `GET https://httpbin.org/json`, the browser shot the
DuckDuckGo home page in a WebView2 child view, the JWT shot the jwt.io
sample token. The walkthrough GIF (`hyperstack-demo.gif`) is now a
slideshow assembled with ImageMagick from the current screenshots
(`magick <frames> -set delay 300 -loop 0 -resize 1200x hyperstack-demo.gif`),
one screen every 3 s, replacing the 0.17 screen recording.

## 2026-10-19 set: Git, credentials, environments, Redis, NoSQL, Devtools

Captured on Windows 11 from the debug build with
`scripts/gui-smoke.ps1` (now started with `-WindowStyle Hidden` so the
debug console does not sit over the window), HyperStack Dark, the
activity rail labels on, and the window at 0,0 sized 1400x900.

New shots:

- `editor-git-conflict.png` and `editor-git-rebase.png` — a fixture
  repository with a real `UU shared.txt` merge and three commits loaded
  into the Interactive Rebase panel (`HEAD~3`).
- `settings-security.png` (vault unlocked, `hunter2` revealed in the
  shop-postgres row, Action column) and `settings-credential-edit.png`
  (Edit Secret for the same row).
- `settings-environments.png` (the dev environment with its variables and
  the Export… menu open) and `settings-connections.png`.
- `redis-browser.png` — a local Redis 8.6 under WSL with string, hash,
  list, set, sorted set and stream keys; `user:1001` selected.
- `nosql-browser.png` — MongoDB 7 in the `hs-mongo` Docker container, the
  `shop.orders` collection in the Find view (filter, projection, sort).
  `hs-couch` runs beside it for CouchDB.
- `devtools-regex.png` and `devtools-diff.png` — a pattern with named
  groups over four lines, and a two-text diff with +3/−2 lines.

Retaken for the current UI: `settings-general.png`,
`settings-appearance.png`, `settings-accessibility.png`,
`settings-workspace.png`, `settings-terminal.png`, `settings-editor.png`,
`settings-browser.png`, `settings-shortcuts.png` and
`settings-about.png`. The obsolete `welcome.png` (the page moved to Home)
was removed.

`hyperstack-demo.gif` is regenerated from this set with ImageMagick
(`magick <frames> -set delay 300 -loop 0 -resize 1200x -colors 256`), one
screen every 1.5 s.

Fixtures: the vault's master password is `TestMaster1` (scratch only);
Redis is started in WSL and the distro held open with a sleeping `wsl`
process (it stops when the last command exits); `docker start hs-mongo
hs-couch` after that hold, or the containers stop with the distro. SSH uses
the in-repo test server (`cargo run -p hs-ssh --features testserver
--example devsshd -- 2222`, user `dev`/`dev`, files under `/home/dev`), not
a system sshd. The database shots use a SQLite file
(`D:\Temp\opencode\shop.db`) with customers, products and orders, opened
through a seeded `shop-sqlite` profile; no server is required.

## 2026-10-19 set: Graphics activity (Phase 1)

Three shots of the Graphics activity shell, captured from the debug build
with the same harness, HyperStack Dark, the rail labels on and the window at
0,0 sized 1400x900:

- `graphics-dashboard.png` — the permanent Graphics dashboard: the sidebar
  with Image / Icon / Diagram Editor / Diagram Generator and the Recent row,
  the What To Do cards, the function list and the Open Here grid with the
  captured thumbnail of the image tab.
- `graphics-image-tab.png` — an image tab with the settings panel on the left
  and the preview on the right, and Convert to Icon in the toolbar.
- `graphics-icon-tab.png` — the Icon tab created from that picture; the
  Transparency panel picked `#285AA0` from the canvas, Tolerance 24, and
  Make Transparent cleared the blue background to the checkerboard while the
  white label remained.

The layout was seeded with one `graphics` tab
(`{"kind":"image","path":"D:\\Temp\\hs-gfx-test.png"}`) so the dashboard and
the image tab could be opened deterministically; the app added the dashboard
and the icon tab itself. The source picture is a 320x200 PNG generated with
`System.Drawing` at `D:\Temp\hs-gfx-test.png`. The Diagram Editor and
Diagram Generator rows are visibly disabled because those panes are the next
phase; the shots will be retaken when they work.

The harness needed an explicit foreground push: `SetForegroundWindow` and
`WScript.Shell.AppActivate` both failed silently while another app stayed on
top, so `D:\Temp\focus-hs.ps1` attaches to the foreground thread, restores and
raises the window before the keys and the capture. See the gotcha in
`CLAUDE.md`.

Fixtures: vault `TestMaster1`; the seeded Graphics tab above; no server.

Added with the Terminal CLI list (2026-10-19): `settings-terminal-list.png`
(the Terminal page's Shells and CLI groups with Add CLI and PATH suggestions
- `sqlite3`, `claude` and `opencode` were found on the capture machine) and
`terminal-cli.png` (the Terminal activity sidebar with a "Test Shell" CLI
profile, and the tab it opened running `cmd /k echo Hello from CLI`). The
profile was added through the Settings page; the suggestion "Claude Code" was
added disabled and moved above it to check that the TOML order follows the
page.

Second gap-filling pass, 2026-10-19: `browser-favourites.png` (DuckDuckGo
added from the toolbar star, shown in the sidebar and the bar under the
address) and `browser-passwords.png` (a "Shop Staging" login with its
username and vault-stored password); `devtools-base64.png`,
`devtools-url.png`, `devtools-yaml.png`, `devtools-generators.png`,
`devtools-cron.png` and `devtools-units.png`; `redis-console.png` and
`redis-info.png`; `nosql-aggregate.png` (the pipeline editor on shop.orders);
`scratchpad-pin.png` (a "Release checklist" note pinned to the top); and
`editor-git-tags.png` / `editor-git-prefix.png` (a v1.0-demo annotated tag and
the Conventional Commit prefix menu). The walkthrough GIF is rebuilt from
this set.

Third pass, 2026-10-19: `ssh-host-key.png`, `ssh-sftp.png` and
`ssh-follow-shell-folder.png` against `devsshd` (host key prompt for
127.0.0.1, dual pane at /home/dev, and `cd projects` followed by the file
pane), plus `db-browse.png`, `db-query.png` and `db-cell-viewer.png` against
the SQLite `shop.db` (schema tree with columns and indexes, a JOIN with
results, and a pretty-printed JSON cell). The SQL shots no longer depend on
a PostgreSQL server. `scripts/gui-smoke.ps1` gained `dclick` and `drag` for
these captures (a table's rows, the editor/result splitter). The API client
shots were reworked in the same pass: `api-client.png` shows a saved
`Shop API` collection and a live `GET https://httpbin.org/json` with the 200
response, and `api-client-scripts.png` a completed `pm.test` in the
post-response editor. Capturing them uncovered the dashboard's **New
Collection** button doing nothing (its dialog was only drawn on a request
tab); that is fixed in the same set. `browser.png` also shows the current
Browser sidebar (Favorites and Passwords) with DuckDuckGo open, and
`settings-ai.png` the Add a Provider gallery (OpenAI, Anthropic, Gemini,
Azure, Bedrock, Mistral, Moonshot, Hunyuan, 1min.AI and the rest).

Fourth pass, 2026-10-19: the SQL error, guard, refused and manual-transaction
shots and the database connection form (SQLite, dev and Production
environments); `devtools-jwt.png`, `devtools-hashes.png`,
`devtools-jasypt.png` and `devtools-pki.png`; `terminal-shells.png`,
`terminal-search.png` and `terminal-links.png`; `ssh-tunnels.png` and
`ssh-remote-edit.png` (`remote-note.txt` was uploaded over SFTP because the
test shell understands only `echo`/`cd`/`pwd`, not redirection);
`graphics.png`, `graphics-ops.png` (Brightness +35 and Grayscale in the
Operations list) and `graphics-crop.png` with the app icon; `scratchpad.png`
with a new note; `editor-projects.png`, `editor-find-replace.png` and
`editor-markdown-preview.png` (the architecture flowchart rendered beside its
source); `launcher.png`, `command-palette.png`, `dock.png`,
`workspace-menu.png`, `workspace-new.png`, `environment-switcher.png`,
`tabs-multi-row.png` (from the 32-tab session before duplicates were closed),
`vault-unlock.png`, `vault-create.png` (a second instance on a fresh data
folder) and the SSH and S3 connection forms. `editor-git-tree.png`,
`editor-git-diff.png` and `editor-git-history.png` were retaken from the
ir-conflict fixture: a modified `note1.txt` in Source Control, its inline
diff, and the commit list with the `v1.0-demo` tag.

S3 pass, 2026-10-19: `moto_server` 5.2.4 on `127.0.0.1:5055`
(`testing`/`testing`) seeded three buckets - `assets-bucket` with
`config/`, `data/`, `docs/` and `images/`, `shop-backups` and
`release-artifacts` - and a `mock-s3` profile with path-style addressing.
That produced `s3-browser.png` (bucket list and a prefix), `s3-edit.png`
(`config/app.toml` opened in the editor from its temp copy),
`s3-presign.png` (a signed GET URL with its expiry), `s3-delete-guard.png`
(a single-object confirmation that notes versioning is off) and
`s3-import-aws.png` (the import dialog's empty state, because this machine
has no AWS CLI profiles).

AI pass, 2026-10-19: a local mock OpenAI-compatible server
(`D:\Temp\opencode\mock_ai.py`) on `127.0.0.1:11434` served
`mock-small`/`mock-large` and streamed canned answers; it was added as
**Mock OpenAI** in Settings. That produced `editor-ai-review.png` (Review
with a code block and Insert), `editor-ai-models.png` (the model picker),
`editor-ai-cascade.png` (Cascade armed), `api-client-ai.png` (an `hs-api`
block with Copy and Apply), `scratchpad-ai.png` (Markdown and Mermaid blocks
with Insert) and `ai-dictation.png` (the microphone listening, with the
countdown). No real provider was contacted.

Editor pass, 2026-10-19: `lsp-demo` (a Point struct) produced
`editor-lsp-completion.png` through the palette's **Editor: Show
Completions** action, and a `let n: i32 = "not a number";` line produced
`editor-lsp-problems.png` with the editor's own Problems panel (NOT the
bottom dock, which lists another source). `debug-demo/app.py` ran under
debugpy and paused on a breakpoint at line 12: `editor-debug.png` shows the
Variables tab with `orders`, `editor-debug-stack.png` the Call Stack
(`main` app.py:12, `<module>` app.py:16). `editor-mermaid-flowchart.png` and
`editor-mermaid-sequence.png` come from `docs/03-architecture.md` with the
preview widened and its scrollbar dragged; the second shows the runtime
sequence diagram.

## Final SQL and split pass (2026-10-19)

- PostgreSQL 16 in Docker (WSL) with the `shop` database (customers, products,
  orders) and `shop-postgres` pointing at 127.0.0.1; `db-completion.png` shows
  table completion after `FROM or` with the schema cache loaded.
- `split-scratchpad.png` is the Scratchpad split with `Release checklist` on
  the left and `Build ideas` on the right.

## Diagram core exports (2026-10-19)

These two are produced by `hs-diagram`, not by the editor UI:

- `graphics-diagram-flow.png` — the offline `Checkout flow` template: start,
  add to cart, stock decision, payment, cancelled branch and order placed.
- `graphics-diagram-architecture.png` — the `Service architecture` template
  with the VPC boundary, client, API, database and events.

Both were written at 2× with a white background by
`cargo run -p hs-diagram --example export -- D:\Temp\hs-diagram-out`; the same
scene also produced the SVG and PDF files.

The editor shots are from the running app (data folder
`%TEMP%\hs-gui-data`, vault `TestMaster1`):

- `graphics-diagram-editor.png` — a Diagram tab with the `Checkout flow`
  template loaded from the Diagram panel, the canvas fitted, the Start node
  selected and the properties/outline column on the left.
- `graphics-diagram-architecture-editor.png` — the `Architecture` template in
  the same tab, showing the grid, the VPC boundary and labelled connectors.
- `graphics-diagram-generator.png` — the Generator tab with the offline
  template preview and the no-provider message; the same dialog can show the
  AI result once a provider is configured.
- `graphics-erd.png` — the ERD template with entities, columns, keys and a
  `1 .. *` relationship.
- `graphics-sequence.png` — the basic sequence template with Client, API and
  Database participants and labelled messages.
- `graphics-save-project.png` — the Save to Project modal with the project,
  `docs/diagrams` subfolder, file name and overwrite checkbox.
- `graphics-diagram-edge.png` — an edge selected from the Outline, with its
  label field, Add/Clear Waypoints and Delete Edge.
- `graphics-assistant.png` — the review-only assistant open over the generator
  with no provider configured; with a provider the same panel shows the
  four quick actions and More Suggestions.

## 2026-10-20 set: UI consistency, live source and shapes

Recaptured from the debug build at 1400x900, HyperStack Dark, rail labels
on, after the UI consistency pass:

- `graphics-dashboard.png` — the dashboard with the "What To Do" spacing,
  centred cards and the aligned function action column.
- `graphics-source-preview.png` — the Diagram Editor's Split view: live
  JSON source to the right of the canvas.
- `graphics-node-shapes.png` — the architecture template with the database
  drawn as a real cylinder, shape icons in the Outline and separated
  properties sections.
- `graphics-edge-properties.png` — an edge selected, with label, Arrow,
  Route, Line, Add Waypoint and Delete Edge in the properties pane.
- `settings-general.png` — the new Behaviour card with the visible Undo
  Limit slider (1–100).

`hyperstack-demo.gif` was rebuilt from the full activity set with
`magick <frames> -set delay 300 -loop 0 -resize 1200x -colors 256 -layers Optimize`.
## 2026-10-21: frame resize, title frame and assistant column

- `graphics-ai-column.png` — the Diagram Editor with the AI Assistant as its
  own right-hand column and the title frame around the diagram.
- `graphics-dashboard.png` — enabled New Diagram / New Generation cards.
- `graphics-node-shapes.png` — updated to the current editor.
- `hyperstack-demo.gif` rebuilt from the current dashboard, assistant column,
  source split, editor and settings screenshots.
## 2026-10-21: full walkthrough GIF

`hyperstack-demo.gif` now covers every activity and every Settings page,
in this order:

Home, launcher, workspace menu/new, command palette, multi-row tabs;
Editor (file, projects, find, Markdown, Mermaid, LSP, debug, Git tree/diff/
history/conflict); Terminal (shell, search, links, CLI); SSH (SFTP, tunnels);
S3 (browser, edit); Database (browse, query, completion, guard); Redis
(browser, console, info); NoSQL (browser, aggregate); API client (request,
scripts, assistant); Devtools (hashes, JWT, YAML, regex, diff, units);
Browser (page, favourites, passwords); Graphics (dashboard, image, icon,
diagram editor, generator, source, assistant, ERD, sequence); Scratchpad
(note, assistant); Settings (General, Appearance, Accessibility, Workspace,
Environments, Security, Connections, Terminal, Editor, Browser, AI Assistant,
Shortcuts, About).

```powershell
magick <frames> -resize 1200x -gravity center -background '#14161A' `
  -extent 1200x771 -colors 256 -layers Optimize -set delay 300 -loop 0 `
  hyperstack-demo.gif
```

Every frame is held for 3 seconds. The GIF has 70 frames.
## 2026-10-22 set: every screenshot retaken for the current UI

Every PNG the README references was recaptured on this date from the
debug build of the `tool-logs` branch (v0.72.0 plus that branch): the rail
with one colour per activity and the new **Logs** entry, the Graphics
dashboard with its icon buttons, the tabbed Manage Collection page, the
AI settings grouped by Chat and Image providers, the Logs dock tab, and
the Markdown-rendered assistant answers. The harness is
`shots.ps1` in the session scratchpad (the `gui-smoke.ps1` pattern with
client-coordinate clicks checked against the dev pid and an
`AttachThreadInput` foreground push); the window is now **1400x960** so
the whole rail - thirteen activities, the sidebar toggle and Settings -
is in frame, which 900 px no longer fit. HyperStack Dark, rail labels on,
scratch data folder `D:\Temp\hs-gui-data` with the vault `TestMaster1`.

Fixtures, as before: `devsshd` on 2222 (`dev`/`dev`), moto on 5055
(`mock-s3`), the mock OpenAI server on 11434, Redis in WSL on 6379, the
`hs-mongo`, `hs-couch` and `hs-postgres` containers, the SQLite `shop.db`,
the `ir-conflict`, `lsp-demo` and `debug-demo` projects, and for the Logs
activity a generated `D:\Temp\hs-logs\app.log` (JSON lines with a stack
trace) followed live.

Logs activity shots: `logs.png` (the file followed to its tail, the
source list and preset hints on the left), `logs-detail.png` (an ERROR
record selected, its fields with filter/exclude/pin actions and the raw
line) and `logs-filter.png` (a `"timeout"` text chip, 130 of 490 shown,
with Clear and Save Preset…).

New in this set: `api-manage-variables.png`, `api-manage-auth.png` and
`api-manage-docs.png` (the tabbed Manage Collection page of a seeded
**Shop API** collection: Orders and Products folders, three variables,
collection-level OAuth 2.0 client credentials, a SigV4 request and a
Markdown description; `api-client.png` sends its `List orders` request to
httpbin with two passing `pm.test`s). `settings-ai.png` shows two seeded
providers (Mock OpenAI and a ComfyUI workstation) with the gallery open.

Retaken on the same date, all at 1400x960: every Settings page, Home,
launcher, palette, workspace and environment dialogs, the dock, every
Devtools utility (sample inputs typed through the harness), the Scratchpad
(notes, assistant answer, split), Browser, Redis (the console shows the
FLUSHALL guard), NoSQL (find with `$in`, an `$unwind`/`$group` pipeline),
SQL (Postgres browse, JOIN, completion, script error, DELETE guard, Manual
Commit badge, Production refusal, SQLite cell viewer), the connection
forms, S3 (moto re-seeded with `D:\Temp\opencode\seed_s3.py`), SSH (host
key prompt after forgetting the key in `known_hosts`, SFTP, follow folder,
remote edit, a live tunnel to 5432), Terminal (shell, search hits,
Ctrl-hovered link, Test Shell CLI, Terminal List), the Editor (projects,
find/replace, Markdown, both Mermaid diagrams, Git tree/diff/history/
conflict/rebase/tags/prefix on a rebuilt `ir-conflict`, rust-analyzer
completion and Problems on `lsp-demo`, debugpy paused on `debug-demo`, the
assistant's Review, model picker, Cascade and dictation) and Graphics
(every diagram tab, image, icon, operations, crop, dashboard last so its
thumbnails are filled). `vault-create.png` comes from a second instance on
an empty data folder.

Harness notes for next time (`shots.ps1`): `rail <k>` clicks the k-th
activity by finding the Home icon's colour in a fresh capture, because
the rail's vertical position depends on how many tab rows the active
activity has; `type` takes `<sp>` for a leading space, `<dl>` for `$` and
`<sc>` for `;` (commands are trimmed and split on semicolons, and the
shells expand `$`); `ctrlhover x y` holds Ctrl over a point and `rawshot`
captures without moving the pointer, for the terminal link shot; `shot`
nudges the pointer inside the client area first so the frame after a
click is painted; a `type` straight after a `click` needs a 500 ms `wait`
or the text lands before the field has focus; `HS_DATA`/`HS_PIDFILE`
drive a second instance. The fixture projects are rebuilt by
`make_fixtures.py` (a merge left in conflict, a Cargo crate with one type
error, a Python script with a `.hyperstack/debug.json`) and the layout's
editor tabs by `seed_editor.py`; `seed_api.py` writes the Shop API
collection and `seed_cli.py` the two CLI profiles.

`hyperstack-demo.gif` is rebuilt from the new set with
`magick <frames> -resize 1200x -gravity center -background '#14161A' -extent 1200x823 -colors 256 -layers Optimize -set delay 300 -loop 0 hyperstack-demo.gif`:
112 frames in activity order (shell, vault, Editor, Terminal, SSH, S3,
Database, Redis, Logs, NoSQL, API client, Devtools, Browser, Graphics,
Scratchpad, Settings), 3 s each, 9.6 MB.

Depth programme, phase A (2026-10-30), against Redis 8.6.3 in WSL with
the `seed.sh` keys (40 `session:*` strings, a 60 KB `cache:big:1`, the
`demo:events` stream with two groups): `redis-info.png` retaken as the
Info dashboard (cards, three sparklines after a burst of GETs, the raw
sections folded), `redis-tools.png` (Tools → Big Keys after a run) and
`redis-streams.png` (Streams on `demo:events`: 25 entries, the workers
group with 5 pending, the newest entries).

Depth programme, phase B (2026-10-30), on the SQLite fixture
`D:\Temp\hs-depth-b\shop.db` (customers, orders, imports): `db-explain.png`
(Explain of a join as the plan tree), `db-edit.png` (two pending edits with
the Apply Changes review open) and `db-import.png` (the Import CSV dialog on
`imports` with the mapping guessed from `stock.csv`).

Depth programme, phase D (2026-10-30): `logs-merge.png` (the API and Worker
saved sources merged, written by `D:\Temp\hs-depth-d\writer.py`, with the
Statistics panel open) and `logs-eventlog.png` (the Windows Event Log
Application log followed on this machine).

Depth programme, phase E (2026-10-30): `ssh-monitor.png` (the WSL Ubuntu
host over key auth with the Monitor strip above the shell: CPU, load,
memory, disk, uptime and the top processes).

Depth programme, phase F (2026-10-30), against moto at 127.0.0.1:5055 with
the `hs-depth-f` bucket (versioning on, a tagged and thrice-written
`reports/quarter.json`, a bucket policy and CORS rules) and the local folder
`D:\Temp\hs-depth-f\local`: `s3-details.png` (the details pane with metadata,
custom headers, tags and three versions), `s3-sync.png` (the dry run: two new
files and one changed) and `s3-policy.png` (the read-only policy and CORS
viewer).

Depth programme, phase H (2026-10-31), on two SQLite fixtures built by a
script at `D:\Temp\hs-depth-h\{source,target}.db` — `customers`, `orders`
and `order_lines` with a composite key; the target lacks `orders.note` and
carries a stale `legacy_audit` — opened through two seeded profiles,
**Depth H Source** and **Depth H Target**:

- `db-erd.png` — the ER diagram in the Diagram Editor: entities with their
  columns badged PK / FK / UQ, the `1 .. *` edge from `orders` to
  `customers`, and the structured Columns panel beside the canvas.
- `db-ddl.png` — Generated DDL with the dialect switched to **PostgreSQL**
  against a SQLite source, so it shows `GENERATED BY DEFAULT AS IDENTITY`,
  the table-level key, and the caution that column types are kept as the
  source server spelled them.
- `db-compare.png` — Compare Schema between the two: `legacy_audit`
  Missing, `orders.note` New, and the generated migration with the
  `DROP TABLE` commented out and the `ADD COLUMN` live.

Captured at 1736x900 (the window's own minimum) with
`scripts/gui-smoke.ps1` and quantised with `magick <in> -colors 256
-depth 8 <out>`. **`gui-smoke.ps1` takes window-rect coordinates — the
same numbers the screenshot shows.** The `screenshot − (8, 31)` offset
noted elsewhere belongs to the later `hs.ps1` harness, which works in
client coordinates; using it here puts every click 31 px high, which
silently hits nothing. The vault prompt is dismissed with **Later**: a
SQLite profile stores no secret, so none of this needs the vault. Clicks
land only after `D:\Temp\focus-hs.ps1` has pushed the window foreground.

The ER diagram's **Export** was exercised in the same pass: Ctrl+Alt+Shift+D
opened the native Save As with `main.svg` pre-filled, and the written file
was a 2.6 KB SVG naming both entities, `customer_id` and the `1 .. *`
label. A native dialog leaves focus on whichever tab opened it, so the next
keystroke can reach the wrong tab — re-focus and click the intended tab
before sending more keys.

No shot of **Compare Table Data**: its dialog was not opened on screen. Driving it needs a statement naming a table in the editor, and the harness kept leaving focus on the schema tree, where the typed SQL goes nowhere and the shortcut does nothing. Its behaviour end to end - picker, target, result and the INSERT/UPDATE it generates - is covered by `hyperstack/src/db_tests.rs::a_data_compare_reports_the_rows_that_differ`; what has not been seen is how it looks.
