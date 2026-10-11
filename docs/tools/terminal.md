# Tool spec — Terminal

**Crates:** `hs-term` (grid, PTY, transports) and `tool-terminal` (local shells, task runner) · **Priority:** P0 · **Status:** draft

## 1. Purpose

A real terminal inside the workspace: local shells with the user's own
profiles and prompt, tabs and splits, and the same emulator the SSH tool uses.
Also hosts "exec" sessions from containers and Kubernetes, and a quick
terminal in the bottom dock. Not a goal: competing with Windows Terminal or
WezTerm on features; the point is proximity to the other tools.

## 2. User stories

- Ctrl+` opens PowerShell 7 in the dock at the project folder, Oh My Posh
  prompt intact; `cargo run` output scrolls; a `path:line` in a panic message
  is clickable and opens the editor.
- Right-click a folder in the project tree or SFTP pane → "Open terminal here"
  (local or remote).
- Select `package.json` scripts or `justfile` recipes from the task runner and
  run them in a dedicated terminal with a restart button.
- Split a terminal to watch logs on the right while typing on the left.

## 3. `hs-term`

Written for HyperStack; Eden Explorer's `terminal.rs`, `terminal_shells.rs`
and `terminal_panel.rs` are worth reading beforehand for the egui-specific
pitfalls (key encoding, repaint wake-ups, PowerShell profile loading, Nerd
Font fallback) but no code is taken from them.

- **Grid**: `alacritty_terminal` `Term` with a `Listener` → UI wake-up; cell
  rendering with cached glyphs, true colour, bold/italic/underline/strike/
  dim/inverse, wide characters, combining marks, Nerd Font fallback chain,
  cursor styles and blink, scrollback (default 10 k, configurable), reflow on
  resize, mouse reporting modes, bracketed paste, OSC 0/2 title, OSC 7 cwd,
  OSC 8 hyperlinks, OSC 52 clipboard (off by default), OSC 133 shell
  integration marks for prompt navigation (P1), bell (visual/toast/sound).
- **Transports**: `Pty` (`portable-pty`: ConPTY on Windows, forkpty
  elsewhere), `Ssh` (russh channel with PTY request and window-change),
  `DockerExec` (`bollard` exec with TTY), `KubectlExec` (via `kube` attach),
  `Process` (no PTY, for task output). All implement `read → bytes`,
  `write(bytes)`, `resize(cols, rows)`, `kill`.
- **Input**: key → bytes encoding table (xterm sequences), including application cursor mode,
  modifiers, kitty keyboard protocol (P2); IME composition.
- **Selection**: character/word/line/block, copy (plain or with ANSI), copy on
  select option, **right-click copies the selection or pastes when nothing is
  selected (PuTTY convention, default on)**, URL/path detection with
  regexes for `file:line:col`, Windows paths, URLs, git SHAs (→ Git tool).
  *Implemented (`hs-term::links`):* URLs (`http`, `https`, `ftp`, `file`)
  and paths with `:line[:col]` or MSBuild's `(line,col)` suffix; a bare
  path needs an anchor (`/`, `./`, `../`, `~/`, `C:\`), an extension or a
  position, so prose such as "and/or" or "3:4" never matches. Holding Ctrl
  underlines the link under the pointer; Ctrl+click opens a URL in the
  browser, a file in the editor at that line (the column is not passed on
  yet), or a folder in a new terminal. Relative paths resolve against the
  folder in a `user@host: dir` window title (the default bash/zsh prompt
  title), then the tab's start folder, then the workspace folder; a path
  that resolves nowhere shows a toast. Git SHAs wait for the Git tool.
  Detection runs per grid row, so a link split by a soft wrap is not found.
- **Search**: Ctrl+Shift+F in scrollback with highlights, regex, case.
  *Implemented (`hs-term::search`):* a find bar in the terminal toolbar
  (also the **Find** button and the `terminal.find` palette command, seeded
  with a one-line selection); Aa and .* toggles; every hit highlighted, the
  current one stronger, with an "n of m" counter (capped at 1000+); Enter
  steps to the older hit, Shift+Enter to the newer, Escape closes and
  returns to the bottom. Like links, matching is per grid row.
- **Shell profiles**: detection of PowerShell 7/5, cmd (with Clink if
  present), Git Bash, WSL distributions, MSYS2; on Unix `$SHELL`, bash, zsh,
  fish, nushell; profile fields: executable, args, env, cwd policy (project /
  last / fixed), icon, colour scheme, font override, startup command; loads
  the user's profile so prompts (Oh My Posh, Starship) work.

## 4. `tool-terminal`

- **Terminal tab**: one PTY session; tab title from OSC 0 or the shell name;
  cwd shown as subtitle via OSC 7 when available; exit shows a "process
  exited (code)" overlay with Restart.
- **Splits** inside a terminal tab (own small split tree) in addition to
  workspace-level splits, because terminal users expect in-tab panes.
- **Dock terminal** (Ctrl+`): persistent per workspace, one or more sessions
  with its own mini tab bar; "move to tab" and "move to dock" actions.
- **Open here**: `OpenRequest::Terminal{cwd, host}` from project tree, SFTP,
  S3 (local temp dir), Git, containers (exec), k8s (exec).
- **Broadcast** input across selected local/SSH terminals (shared with the
  SSH tool).
- **Task runner (P1)**: detects runnable things in the project —
  `package.json` scripts (npm/pnpm/yarn/bun by lockfile), `Cargo.toml`
  (build/run/test/clippy + binaries/examples), `Makefile` targets, `justfile`
  recipes, `taskfile.yml`, `docker-compose.yml` services (up/down/logs),
  `pyproject`/`poetry`, `go.mod` (build/test), `.NET` projects, `gradle`/
  `maven` — listed in the sidebar; click runs in a dedicated terminal with
  Restart/Stop; pin favourites; environment variables from the active
  environment can be injected (opt-in, shows which).
- **Command history across sessions (P2)**: Ctrl+R-like palette over the
  SQLite command history (captured via shell integration marks).

## 5. Settings

Default profile, per-profile overrides, font (Nerd Font bundled), size, line
height, cursor, scrollback, colour scheme (16 ANSI from theme or custom),
bell, copy on select, paste warning for multi-line, right-click behaviour,
confirm close with running process, word separators, link detection on/off,
OSC 52 allow.

## 6. Keyboard (terminal scope)

| Shortcut | Action |
|---|---|
| Ctrl+` | toggle dock terminal |
| Ctrl+Shift+` | new terminal tab |
| Ctrl+Shift+C / Ctrl+Shift+V | copy / paste (Ctrl+C/V pass through; configurable) |
| Ctrl+Shift+F | search |
| Alt+Shift+D / Alt+Shift+E | split down / right (in-tab) |
| Alt+arrows | move between in-tab panes |
| Ctrl+Shift+W | close pane |
| Ctrl+Shift+↑/↓ | previous / next prompt (shell integration, P1) |
| Ctrl+= / Ctrl+- | font zoom |

## 7. Tests

alacritty's parser is tested upstream; we add snapshot tests of grid
rendering (colours, wide chars, cursor), key-encoding tables, link detection
regexes, PTY spawn on each OS in CI (echo round-trip, resize), profile
detection with fake filesystems, task detection fixtures.
