# Tool spec — Terminal

**Crates:** `hs-term` (grid, PTY, transports) and `tool-terminal` (local shells, task runner) · **Priority:** P0 · **Status:** draft

## 1. Purpose

A real terminal inside the workspace: local shells with the user's own
profiles and prompt, tabs and splits, and the same emulator the SSH tool uses.
Also hosts "exec" sessions from containers and Kubernetes, and a quick
terminal in the bottom dock. Not a goal: competing with Windows Terminal or
WezTerm on features; the point is proximity to the other tools.

## 2. User stories

- Ctrl+` opens PowerShell 7 in the editor at the project folder, Oh My Posh
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
  present), **Nushell** (PATH, `%LOCALAPPDATA%\Programs\nu`, `~/.cargo/bin`,
  scoop shims, WinGet links; listed between Command Prompt and Git Bash),
  Git Bash, WSL distributions, MSYS2; on Unix `$SHELL`, bash, zsh,
  fish, nushell; profile fields: executable, args, env, cwd policy (project /
  last / fixed), icon, colour scheme, font override, startup command; loads
  the user's profile so prompts (Oh My Posh, Starship) work.

## 4. `tool-terminal`

- **Terminal tab**: one PTY session; tab title from OSC 0 or the shell name;
  cwd shown as subtitle via OSC 7 when available; exit shows a "process
  exited (code)" overlay with Restart.
- **Splits** inside a terminal tab (own small split tree) in addition to
  workspace-level splits, because terminal users expect in-tab panes.
- **Dock terminal**: removed on 2026-10-18 at the owner's request. Shells
  live in this activity and in the editor's own panel
  (`docs/tools/editor.md` §13), which starts in the project's folder; a
  third place to look was one too many. What follows described it:
  persistent per workspace, one or more sessions
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

## 5. Settings and CLI profiles (implemented 2026-10-19)

The Terminal page sits **between Security and Connections**, beside the other
connection-shaped pages. It has three cards.

**Session defaults** - default shell profile, font (Nerd Font bundled), size,
line height, cursor, scrollback, colour scheme (16 ANSI from the theme or
custom), bell, copy on select, paste warning for multi-line, right-click
behaviour, confirm close with a running process, word separators, link
detection on/off, OSC 52 allow.

**Terminal List** - the shells and CLI tools the Terminal activity shows in
its sidebar, in the order the sidebar uses. The **arrow buttons** move a row
up or down (the activity's own shell list keeps its drag grip); **Add CLI**,
**Edit** and **Remove** sit beside the list, and Remove asks first. A CLI row
can be disabled without being deleted: it stays in this list and leaves the
activity sidebar. The shells keep their existing order (`shell_order`).

**Suggestions** - commands found through PATH/PATHEXT that are not in the
list yet, each with an Add button: `mysql`, `psql`, `sqlite3`, `mongosh`,
`redis-cli`, `aws`, `kubectl`, `docker`, `claude`, `opencode`, `gemini`,
`aider`, `ollama`. Adding one creates a **disabled** profile with a sensible
name and icon; nothing is run or executed by detection. The list is scanned
when the page is first opened in a session (so a restart picks up a new
install); **Refresh** re-reads PATH from the system and scans again, which
also finds a CLI installed while the app is open.

A **CLI profile** has these fields:

| Field | Meaning |
|---|---|
| Name | the sidebar label |
| Command | the executable, resolved through PATH/PATHEXT at launch |
| Arguments | passed to the shell as a command line, so quoting stays the user's |
| Start folder | empty = the activity's default folder; `{{workspace}}` = the workspace folder; anything else is that path |
| Icon | `terminal`, `database`, `cloud` or `ai`, drawn beside the row |
| Enabled | shown in the activity sidebar |

The ordered list is stored in `settings.toml` as `[terminal] cli` (an array
of tables; docs/05), so the settings page's order is the sidebar's order.
Profiles never store a secret: `{{variables}}` are references reserved for
the next phase.

The Terminal activity sidebar shows the two groups in one scroll area under
the fixed header (docs/04 §1.5):

```
Shells                    CLI
  PowerShell 7              mysql-cli
  Command Prompt            psql-cli
  Nushell                   claude
  Git Bash                  opencode
  WSL - Ubuntu-24.04
```

Clicking a CLI opens a terminal tab in the profile's folder and, once the
shell is ready, types its command line and Return - the same path as the
assistant's **Insert and Run**, so the command stays visible and editable. A
profile whose command is not found on PATH shows "Not found on PATH" and does
not open; Edit it in Settings.

The editor's terminal panel reads the same list but offers the enabled
**shells** only, so a database CLI does not appear at the foot of every
file. A future `tool-cli` host with its own tabs is out of scope: opening
`mysql`, `psql`, `claude` and the like in a terminal is enough for this
phase.

**P1, not in this first pass:** per-profile environment variables (injecting
the active or a named environment into the session) and an icon accent
colour. The profile stores neither yet.

## 6. Keyboard (terminal scope)

| Shortcut | Action |
|---|---|
| Ctrl+Shift+` | new terminal tab |
| Ctrl+Shift+C / Ctrl+Shift+V | copy / paste (Ctrl+C/V pass through; configurable) |
| Ctrl+Shift+F | search |
| Alt+Shift+D / Alt+Shift+E | split down / right (in-tab) |
| Alt+arrows | move between in-tab panes |
| Ctrl+Shift+W | close pane |
| Ctrl+Shift+↑/↓ | previous / next prompt (shell integration, P1) |
| Ctrl+= / Ctrl+- | font zoom |

## 7. Tests

## 7a. Selecting text (2026-10-16)

A drag anchors the selection at the pixel the mouse went **down** on
(`pointer.press_origin()`), not where egui first reports a drag: the
latter is already past the drag threshold, which used to drop the first
character. The anchor cell is always inside the selection — its side is
taken from the drag direction — while the moving end keeps the half-cell
rule, so a character is picked up once the pointer is past its middle.
The selection is cleared on resize, because its grid points would
otherwise land on unrelated cells after a reflow.

## 8. AI assistant (implemented 2026-10-14)

The toolbar's **AI Assistant** button opens the shared assistant. It can
discuss the visible terminal screen (up to 80 rows), but it cannot read
scrollback that is not currently visible. Context is shown in the panel and
sent only after the user submits a question. The panel uses the providers
configured in Settings → AI Assistant.

Under the chat sit the quick actions **Explain Output**, **Explain the
Error**, **Suggest a Command** and **Summarise**: each fills the input with
its prompt and sends it. A fenced block in an answer offers **Copy**,
**Insert** (typed into the shell, the user presses Return) and **Insert and
Run** (typed with the Return added), as agreed in ADR-019's 2026-10-16
amendment.

alacritty's parser is tested upstream; we add snapshot tests of grid
rendering (colours, wide chars, cursor), key-encoding tables, link detection
regexes, PTY spawn on each OS in CI (echo round-trip, resize), profile
detection with fake filesystems, task detection fixtures.

## Dashboard (2026-10-29)

The activity's home tab wears the shape every activity now shares
(`hs_ui::dashboard::saved_dashboard`): a band with the activity's
mark, name and subtitle; the figures in equal rounded boxes;
**Start Here** with its first moves; **Saved Shells** - the shells detected on this machine, each over its executable;
and the shared **Open Here** grid, each section ruled apart.
