# Screenshots

Images used by the top-level [README](../../README.md). They show HyperStack
0.11.0 as built from this repository, not mock-ups.

## How they were made

- Built with `cargo build --release -p hyperstack` on Linux x64 and run under
  Xvfb at 1440×900 (Mesa software rendering), driven with `xdotool`.
- Fresh portable data folder (`HyperStackData/` beside the executable); the
  vault, the SSH connection profile and the folder sync were created and
  exercised through the UI. The three environments (Development, Staging,
  Production) and a second, illustrative SSH profile (`db-01 (production)`)
  were written as workspace files first.
- The SSH/SFTP shots use a local OpenSSH server (`127.0.0.1:2200`, user
  `deploy`) so the shell and SFTP pane are real; the shell prompt sets the
  xterm title, which is what **Follow Shell Folder** reads.
- The S3 shots use moto's S3 server (`moto_server -p 5055`, keys
  `testing`/`testing`) seeded with a few buckets; the profiles were imported
  through the tool's "Import AWS Profiles" from a `~/.aws` pair pointing at it.
  The tunnel shot forwards a local port through the same OpenSSH server to a
  small HTTP service.
- The database shots and `database-walkthrough.gif` (12 fps x11grab → 10 fps,
  1200 px GIF) use a local PostgreSQL 16 and MariaDB 10.11 seeded with a small
  shop schema (customers, products, 600 orders, order items, an `audit`
  schema), and a SQLite copy of it. The PostgreSQL and MariaDB profiles were
  created through Settings → Connections by pasting a URL (the password went
  into the vault); "orders (production)" points at the same database under the
  protected Production environment, and "orders via web-01" tunnels through
  the local OpenSSH server.
- `hyperstack-demo.gif` is an `ffmpeg x11grab` recording of one scripted run
  (vault unlock → launcher → terminal with search and a Ctrl+clicked
  `path:line` → editor at that line → Markdown preview with a Mermaid diagram
  → SSH/SFTP and a new tunnel → S3 bucket browsing and a presigned URL →
  Production environment), converted at 10 fps, 1200 px wide with a generated
  palette.
- PNGs are quantised to 256 colours to keep the repository small.

## AI assistant imagery

The AI assistant now appears in the terminal, SSH, SQL and S3 toolbars. The
existing screenshots and GIFs have **not** yet been retaken for this UI: doing
so requires an interactive desktop/Xvfb capture session with the relevant
connection and test-provider setup. When recaptured, add dedicated shots for
terminal, SSH, SQL and S3 assistant panels, plus an updated walkthrough GIF;
retain current filenames and README links for existing shots.

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
beside it, and the Problems panel under the diagnostics' underlines) and
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
