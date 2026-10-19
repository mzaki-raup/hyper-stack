# Screenshots

Images used by the top-level [README](../../README.md). They show the build from this
repository, not mock-ups; the set was last refreshed on Windows 11 for 0.45.x.

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
`magick -delay 150 -loop 0 <frames> -resize 1200x -colors 256 -layers Optimize hyperstack-demo.gif`,
one screen every 1.5 s.

The assistant is covered by `settings-ai.png`, `editor-ai-review.png`,
`editor-ai-cascade.png`, `editor-ai-models.png`, `api-client-ai.png`,
`scratchpad-ai.png` and `ai-dictation.png`. The per-tool review-only panels
(terminal, SSH, SQL, S3, Redis, NoSQL) use the same layout; no separate
captures are kept for each.

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
(`magick -delay 150 -loop 0 <frames> -resize 1200x hyperstack-demo.gif`),
one screen every 1.5 s, replacing the 0.17 screen recording.

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
(`magick -delay 150 -loop 0 <frames> -resize 1200x -colors 256`), one
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
