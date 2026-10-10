# HyperStack

A portable Rust + egui desktop workspace for developers, DevOps, DBAs, testers
and integration engineers: code editor, SQL and NoSQL database clients, Redis
viewer, API client, SSH/SFTP, S3 browser, Git, terminal, log tail viewer and
a devtools drawer in one multi-row, multi-tab window that shares connections,
environments and secrets across every tool.

**Status: Phase 0 (foundation) implemented; Phase 1 tools not started.** The
Cargo workspace exists (`crates/hs-core`, `hs-ui`, `hs-term`, `hyperstack`),
builds to a portable `HyperStack.exe`, and passes the full gate (fmt, clippy
`-D warnings`, tests, `--self-test`). `CHECKLIST.md` tracks what is done.

## Where things are

| Need | Read |
|---|---|
| Product scope, personas, principles, non-goals | `docs/01-vision-and-scope.md` |
| Every feature with priority (P0 = v1.0) and effort | `docs/02-features.md` |
| Crate layout, `Tool` trait, `Services`, runtime model, Eden Explorer reference table | `docs/03-architecture.md` |
| Window layout, multi-row tab space, launcher, palette, shortcuts, theming | `docs/04-ui-design.md` |
| Portable data folder, workspace files, environments, vault, history db, importers | `docs/05-data-model-and-storage.md` |
| Threat model, vault format, protected environments, destructive-action guards | `docs/06-security.md` |
| Phases, milestones, checklists, risks | `docs/07-roadmap.md` (progress is tracked in `CHECKLIST.md`) |
| Why each major choice was made (ADR-001 … ADR-018) | `docs/08-decisions.md` |
| CI, automatic releases, asset contract, in-app updater | `docs/09-ci-cd-and-updates.md`; live workflows in `.forgejo/workflows/`, scripts in `scripts/`, signing key notes in `keys/README.md` |
| One design document per tool | `docs/tools/*.md` |
| App icon and every derived size | `assets/icon/` (master `hyperstack.png`; regenerate with `scripts/make-icons.ps1`, never by hand; usage in `docs/03-architecture.md` §13) |

Read the relevant spec before implementing a tool; the specs are the
requirements. When implementation forces a design change, change the document
in the same commit.

## Project rules (non-negotiable)

- **Eden Explorer (`../eden-explorer`) is a reference, never a code source.**
  Read its files to learn behaviour, pitfalls and crate choices (the table in
  `docs/03-architecture.md` §12 says which file answers which question), then
  write the code fresh here. Do not copy functions, snippets or files, and never
  add an Eden crate as a dependency (ADR-014).
- **Portable by default.** Everything the app writes goes under
  `HyperStackData/` beside the executable (fallback to a per-user folder only
  when that location is read-only). No registry, no `%APPDATA%`, no `%TEMP%`.
  Paths inside the folder are stored relative to it (ADR-017,
  `docs/05-data-model-and-storage.md` §1). `/HyperStackData/` is gitignored;
  never commit it.
- **Secrets never touch disk in plaintext.** They live in the encrypted vault
  (`HyperStackData/vault.hsv`) and are referenced as `secret:<ulid>`; logs and
  history are redacted. Any test that writes a secret must grep the data folder
  afterwards (`docs/06-security.md`).
- **One crate per tool, one `Tool` trait.** Tools depend on `hs-core`/`hs-ui`
  (and `hs-editor`/`hs-term` where needed), never on each other. Cross-tool
  features go through `Services`, `OpenRequest` and events (ADR-004).
- **The UI thread never blocks.** All I/O runs on the tokio runtime; results
  arrive through bounded channels drained at frame start; anything over
  ~200 ms is a `Job` with progress and cancel. No `block_on` on the UI thread
  (ADR-013).
- **Destructive actions are guarded, not forbidden**, and protected
  environments (production) default to read-only in every tool.
- **Every visible string goes through Fluent** (`locales/en.ftl` first);
  every action has a palette command and a rebindable shortcut; every list and
  tree is keyboard-navigable.
- **UI text casing.** Title Case for button labels, menu and list item names,
  setting names, tab and category labels ("Close Others", "Check Now",
  "Minimum Tab Width"; minor words such as a/the/of/to/from/at/in/on/and stay
  lowercase). Sentence case for information and instructions: tooltips,
  hints and placeholders, dialog bodies, toasts, empty-state messages ("No
  background jobs.", "Type to search…"). Owner's rule, 2026-10-10.
- **Right-click copy/paste in text surfaces (PuTTY convention).** In the
  terminal and in every editor/text field we draw ourselves: right-click with
  a selection copies it to the clipboard and clears the selection; right-click
  with no selection pastes the clipboard at the cursor. A per-tool setting may
  turn it off; the default is on. Owner's rule, 2026-10-11.
- **Cross-platform core.** Windows ships first, but `hs-core`, `hs-ui`,
  `hs-editor`, `hs-term` and every tool must compile on Linux and macOS.
  Platform code lives only in `hs-core::secrets`, `hs-core::storage::paths`
  and `hs-term::pty`.

## Git identity and remote

- Remote: `https://git.zacx.net/owner/hyper-stack.git` (Forgejo). Commit and
  push as the repository owner: this clone has `user.name = owner` and
  `user.email = owner@zacx.net` set locally; keep it that way.
- Commit subjects decide releases (see **Releases**). Use Conventional Commits:
  `feat:`, `fix:`, `perf:`, `security:`, `docs:`, `chore:`, `refactor:`,
  `test:`, `ci:`, `style:`, `build:`. A `!` after the type or a
  `BREAKING CHANGE:` footer marks a major bump.
- End commit messages with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## Build & release workflow

CI/CD runs on the **Forgejo** instance that hosts this repo (`git.zacx.net`),
not GitHub. Workflows live in `.forgejo/workflows/` and are active; both detect
a missing root `Cargo.toml` and finish green without building or releasing, so
documentation pushes are safe until Phase 0 creates the workspace. The release
logic lives in `scripts/*.ps1` (adapted from Eden Explorer's release tooling,
the one sanctioned exception to the no-reuse rule, because it is delivery
tooling for the same forge and runner rather than product code). Add new
release steps to a script first so they can be run locally.

Nothing in a workflow may use a published action (`uses:`): the runner cannot
fetch actions from `code.forgejo.org` (forgejo#2809) and has no Node. Checkout
is a few lines of `git` with the job token passed as an `http.extraheader` on
the command line, never written to `.git/config`.

### The runner

One runner serves every `owner/*` repository on the forge, including
`hyper-stack`. It is a Windows service on the development machine itself
(`gitea-runner`, NSSM-wrapped, `C:\Apps\gitea-runner`, running as
LocalSystem), registered at account scope rather than per repository - there
were briefly two repo-scoped runners, which let two projects build
concurrently and defeated the single-runner serialization the workflows
assume. `C:\Apps\gitea-runner\make-global.ps1` is what collapsed them.

- **A runner service needs `cargo`/`rustup` put on its PATH explicitly.** They
  live in `C:\Rust\.cargo\bin`, which is on the *user* PATH only, and a
  service running as LocalSystem sees just the machine PATH - which does have
  `git` and `pwsh`, so the checkout and the version plan both succeed and the
  job then dies at `rustup update stable` about eight seconds in. The working
  service supplies a full PATH through NSSM's `AppEnvironmentExtra`; a runner
  installed with a plain `nssm install` has none and fails exactly that way.
  If a new runner is ever set up, copy that value from the existing service
  rather than retyping it.
- `runs-on: windows-latest` matches because the runner registers the label
  `windows-latest:host` - `host` meaning jobs run directly on the machine, not
  in Docker, which is what a Rust build here needs.
- There is no `unregister` command. A replaced registration leaves a stale
  runner entry that reads "offline" for ever in the forge UI; delete those by
  hand.
- Re-registering deletes `.runner`, and a bad token then leaves the machine
  with no runner at all and every project unable to build - back that file up
  before touching it (`make-global.ps1` does, and restores it on failure).
- The runner's working copy is emptied at the start of every job, but
  `C:\hs-ci\cargo` (`CARGO_HOME`) and `C:\hs-ci\target` (`CARGO_TARGET_DIR`)
  live outside it on purpose so builds stay incremental between jobs.

### Workflow

- Work on a branch. After a change is tested locally (`cargo fmt --all
  --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked`) **and** the documentation is updated to
  match (`README.md`, the affected `docs/*.md`, the `Unreleased` section of
  `CHANGELOG.md`, `CHECKLIST.md`, and this file), push the branch, wait for
  `ci.yml` to go green, then merge into `main` and push `main`. **The
  repository owner asked for this to be done every time without asking again.**
- **Never merge while CI is red.** A branch is the only place a change gets
  checked before it lands: `ci.yml` deliberately does **not** run on `main` -
  the release workflow runs there, and with a single runner whose working copy
  every job empties, two workflows on one commit would delete each other's
  files mid-build.
- Documentation-only pushes to `main` are safe: the release workflow sees no
  release-worthy commit and stops without publishing.

### Checks (all are hard gates from the first commit of code)

- `cargo fmt --all --check` - the codebase is rustfmt-formatted from day one,
  so this fails the job on any drift. Run `cargo fmt --all` before committing.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` - no warning
  backlog is allowed to form. Use a scoped `#[allow(...)]` with a comment when
  a lint is genuinely wrong for a line, never a crate-wide allow.
- `cargo test --workspace --locked` - unit tests, property tests on the editor
  buffer, `egui_kittest` snapshot tests of widgets. Container-backed driver
  tests run only when the `HS_INTEGRATION` secret is set.
- `cargo deny check` - licences, advisories, duplicate crates (`deny.toml` in
  the root).
- `cargo build --release --locked -p hyperstack`, then the smoke step runs
  `HyperStack.exe --version` and `--self-test --data-dir <temp>` and fails if
  anything appeared under `%APPDATA%\HyperStack` (the portable guarantee).
- CI uses the latest stable Rust, which can have newer clippy lints than a
  local toolchain; `rustup update stable` locally before blaming CI.

### Releases

- `.forgejo/workflows/release.yml` publishes a release on **every push to
  `main`** that contains a release-worthy commit - there is no manual version
  bump and no tag to push by hand. `scripts/next-release.ps1` works out the
  version from the commits since the last `v*` tag, the job re-runs the
  checks, `scripts/update-version.ps1` bumps the root `Cargo.toml`
  (`[workspace.package] version`; every crate inherits
  `version.workspace = true`), every member entry in `Cargo.lock` and the
  `CHANGELOG.md` section, the bump is committed and tagged *before* the build,
  `scripts/build-and-package.ps1` builds, packages and signs, and nothing is
  pushed or published until that succeeds. `scripts/verify-release.ps1` then
  checks the published release anonymously and fails the job if anything is
  wrong.
- **Commit subjects decide the version.** Conventional types are primary:
  `feat:` is a **minor** bump, `fix:` / `perf:` / `security:` a **patch**,
  `feat!:` or a `BREAKING CHANGE:` footer a **major**; `docs:` / `chore:` /
  `refactor:` / `test:` / `ci:` / `style:` / `build:` and merge commits cut
  **no release**. Plain-verb subjects are accepted as a fallback with the same
  meaning (`Add …`/`Introduce …`/`Implement …`/`Support …` = minor, `Fix …` =
  patch, `Document …` = none). A `release: X.Y.Z` trailer overrides the
  computed version (used once to force `1.0.0`). Run
  `pwsh -File scripts/next-release.ps1` to see what the next push to `main`
  would publish; it only writes `release/notes.md`.
- **Never hand-edit `version` in `Cargo.toml`.** The workflow owns it, and the
  tag and the `Cargo.toml` at that tag agree by construction. They have to: the
  app compares a release's tag against the version it was built with, and a
  mismatch would offer the same update for ever.
- **The published asset names are a contract with `hs-core::update`**, not a
  convention (`docs/09-ci-cd-and-updates.md` §4.3): `HyperStack.exe`
  (downloaded in place - a versioned filename would break the updater),
  `HyperStack-<version>-windows-x64.zip`, `manifest.json`,
  `manifest.json.minisig`, `SHA256SUMS.txt`, `CHANGELOG-<version>.md`.
  Renaming any of them is a breaking change for every installed copy.
- **The signed manifest is the trust anchor.** `manifest.json` carries each
  asset's size and sha256 and is signed (rsign2, minisign format) with the key
  held only in the `HS_MINISIGN_KEY` forge secret; the public key is committed
  as `keys/minisign.pub` and embedded in the app, which refuses a release whose
  signature or hashes do not verify. `SHA256SUMS.txt` is for humans and
  scripts. The release job refuses to run without the secret, and its last
  step downloads the release anonymously and verifies it. Key generation and
  rotation: `keys/README.md`. Active key id: `99C2A397C6491EAB` (generated
  2026-10-10 with `--unencrypted`). `.gitignore` blocks `*.key`/`*.sec` and everything in `keys/`
  except the README and the public key; never weaken that.
- Tags are `vMAJOR.MINOR.PATCH` on `main` (stable channel). Pushes to a `beta`
  branch publish `vMAJOR.MINOR.PATCH-beta.N` with the forge's `prerelease`
  flag; stable users never see those, beta users do.
- Release notes are generated from the commit subjects (grouped Added / Fixed /
  Changed / Security) into `CHANGELOG-<version>.md` and prepended to
  `CHANGELOG.md`. Hand-edit only the `Unreleased` section of `CHANGELOG.md`;
  released sections belong to the workflow.

## Build & run (once the workspace exists)

```bash
cargo build --release -p hyperstack        # target/release/HyperStack.exe
cargo run -p hyperstack -- --data-dir ./tmp-data   # run against a throwaway data folder
cargo run -p hyperstack -- --self-test     # what the CI smoke step runs
cargo test -p tool-db                      # one tool crate at a time
pwsh -File scripts/next-release.ps1        # what the next push to main would release
pwsh -File scripts/build-and-package.ps1 -Version 0.0.0-local -SkipSign   # local packaging dry run
```

A running `HyperStack.exe` locks its own file, so a release build fails with
"Access is denied" while an old instance is open - stop it first
(`Stop-Process -Name HyperStack -Force`). When developing, point the app at a
scratch data folder with `--data-dir` so your real `HyperStackData/` beside
the exe is not touched.

## Operational Instructions

You must strictly adhere to these guidelines during every task execution.
Prioritize system safety, meticulous tracking, and verification.

### Workflow & Execution Rules
- **Phase Execution:** Implement the latest plan and design phase-by-phase, in
  the order of `docs/07-roadmap.md`. Follow the latest version of developer
  notes if they exist.
- **Pre-Verification:** You must always verify and test that the current phase
  is fully working (checks above pass, the feature is exercised in the running
  app) before updating any checklist or moving forward.
- **Git Automation:** If a git repository exists, you must always commit and
  push the code/documentation to the server after completing a phase, following
  the branch → green CI → merge to `main` flow above.
- **Mistake:** If any mistake is encountered during development and fixed,
  always record it in the **Gotchas** section of this file, under the relevant
  heading, so that the same mistake is not made twice.

### File Tracking & Documentation Rules
- **Checklist File (`CHECKLIST.md`):**
  - Lives in the root folder; create it if it does not exist (it mirrors the
    phases and milestones of `docs/07-roadmap.md`).
  - Detail every phase and sub-phase.
  - Track states strictly as: `Completed`, `In-Progress`, or `Pending`.
  - Update this file immediately after verifying a phase's success.
- **Change Log (`CHANGELOG.md`):** Add a line under `Unreleased` every time
  you add, delete, change, or fix code, implementation, or deployment. The
  release workflow moves those lines into the versioned section.
- **README.md:** Update the Readme whenever changes are made to features,
  requirements, cross-platform build commands, or restrictions.
- **Design documents (`docs/`):** Update the affected document whenever an
  implementation decision differs from what it says; the documents are the
  specification and must stay true.
- **Summary File:** Update the Summary file (if it exists) whenever any
  project asset or file is modified. None exists yet; do not create one
  unless asked.

## Gotchas

Record confirmed, non-obvious lessons here as they are learned (the rule
above). Keep each entry short: what went wrong, why, what to do instead.

### Forge and runner
- The forge derives a release row from a pushed tag **asynchronously**; a PATCH
  of name/body that lands before that derivation is overwritten. Upload assets
  first, PATCH metadata last, then GET in a loop until the forge reports the
  body and every asset (`.forgejo/workflows/release.yml` does this).
- A PowerShell step with a non-terminating error carries on and the job goes
  green having uploaded nothing. Every step starts with
  `$ErrorActionPreference = "Stop"` and the packaging step asserts every
  contracted asset exists on disk before upload.
- `rsign generate -W` ("passwordless") keys are still in the encrypted format
  and `rsign sign` prompts for the password on the console (not stdin, so
  piping does not help); on the runner that is a hang. Only
  `rsign generate --unencrypted` keys sign non-interactively. The first
  release attempt (run 8) also showed the forge secret had been pasted as the
  base64 line alone ("Missing encoded key in secret key"); the script now
  normalises that and times out the signing step with a clear message.

### Windows
- Renaming a running executable on the same volume is allowed; overwriting it
  is not. The updater relies on this (rename old → move new → relaunch), so
  keep `HyperStackData/update/` on the same volume as the exe.

### egui / eframe 0.35
- `eframe::App` has `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`,
  not `update(ctx, frame)`; get the context with `ui.ctx()`. Panels are
  `egui::Panel::top/bottom/left/right(id)` with `.exact_size`, `.default_size`,
  `.size_range`, `.resizable`, shown with `.show(ui, …)` on the root `Ui`;
  `CentralPanel::default().show(ui, …)` likewise. `TopBottomPanel`/`SidePanel`
  no longer exist.
- `ctx.content_rect()` is the screen rect (`screen_rect()` is gone);
  `TextEdit::frame` takes an `egui::Frame` (`Frame::NONE`), not a bool;
  `ui.close()` closes menus; `rect_stroke` needs a `StrokeKind`.
- Painter-drawn controls (`ui.interact` + `painter.text`) have no AccessKit
  node: screen readers and `egui_kittest` queries cannot find them. Give
  every such control `response.widget_info(|| WidgetInfo::labeled(..))`, or
  use a real widget.
- `rust-embed`'s `#[include]`/`#[exclude]` attributes need the
  `include-exclude` feature; without it the derive silently fails and every
  `Locales::get` call reports "no associated function".

- egui-winit turns Ctrl+V, Ctrl+Shift+V and Shift+Insert into `Event::Paste`
  (and Ctrl+C/X into Copy/Cut) before any key handling, so never bind those
  as shortcuts; the Markdown preview is Ctrl+Shift+M for that reason.
- Global shortcuts win over tool-local keys: `run_action` forwards unknown
  action ids (`file.save`, `file.open`, …) to the active tool's `run_command`,
  so a tool must not also handle the same combo itself, and a tool-local
  combo that collides with a global default (Ctrl+Shift+L = `vault.lock`)
  has to pick another one (the editor uses Alt+F3 for "select all occurrences").
- Driving the release exe for a GUI smoke test from PowerShell: `Start-Process`
  with `--data-dir` in `%TEMP%`, `SetForegroundWindow`, then
  `[System.Windows.Forms.SendKeys]::SendWait(...)` works for keyboard paths
  (Ctrl+P, Ctrl+T, Escape). Synthetic `mouse_event` clicks only land where
  the screenshot says once the PowerShell process has called
  `SetProcessDPIAware()` (otherwise `GetWindowRect`, `CopyFromScreen` and
  `SetCursorPos` all see DPI-virtualised coordinates and clicks drift by a
  row or more). Mouse paths are also covered headlessly with `egui_kittest`
  (`crates/hs-editor/tests/view.rs`).

### Terminal / ConPTY
- A ConPTY child does not run until the "terminal" answers the Device Status
  Report query `ESC[6n` it sends at startup with a cursor position
  (`ESC[1;1R`). Anything that spawns through `portable-pty` and reads output
  must take the writer and answer that query (see `hs-term::pty`), or the
  child hangs with 4 bytes of output. `alacritty_terminal` answers it for the
  real terminal in M1.

### PowerShell scripts
- `"... $LASTEXITCODE: $x"` does not parse: a colon right after a variable name
  is read as a scope/drive prefix (`$env:`). Write `${LASTEXITCODE}:` inside
  strings. `ci.yml` parse-checks every script so this is caught before a
  release job needs it.
- `[regex]::Replace` with a string replacement expands `$1`-style tokens, which
  mangles release notes containing `$`; use the scriptblock overload
  (`{ param($m) $text }`) for literal insertion.
- `HyperStack.exe` is a GUI-subsystem binary (release builds set
  `windows_subsystem = "windows"`). PowerShell's `& $exe …` returns
  immediately for such a process and leaves `$LASTEXITCODE` empty, so
  `if ($LASTEXITCODE -ne 0)` throws even when `--self-test` printed
  "self-test ok" (CI run 6). Always run the exe with
  `Start-Process -FilePath $exe -ArgumentList @(…) -Wait -PassThru -NoNewWindow`
  and read `.ExitCode`; redirect stdout to a file when the text is needed.
  Bash and Rust's `Command::status()` wait correctly, which is why local
  checks passed.
- PowerShell unrolls a one-element array returned from a function or an
  `if` expression into a scalar string on assignment, so `$tags.Count` is 1
  and `$tags[0]` is the first *character* ("v"). `next-release.ps1` planned
  no release on the first push after `v0.1.0` existed (release run 19) for
  that reason. Wrap such assignments in `@(...)` and test scripts with
  exactly one tag/item, not only zero or many.
- `[ref]$name` on a variable that was never assigned throws "[ref] cannot be
  applied to a variable that does not exist". The Actions `pwsh` shell runs
  with `$ErrorActionPreference = 'stop'`, so this failed `ci.yml` in three
  seconds before any Rust step. Always `$x = $null` before `[ref]$x`, and
  reproduce workflow snippets locally under `Stop` before pushing.

### Rust / build
- `.cargo/config.toml` sets `-C target-feature=+crt-static` so the MSVC build
  is a fully static, portable exe; keep it.
- `cargo clippy --fix --allow-dirty` clears the mechanical lints
  (collapsible ifs, `sort_by_key`) in one go; with edition 2024, nested
  `if let` blocks collapse into let-chains (`if let Some(x) = a && cond`).
