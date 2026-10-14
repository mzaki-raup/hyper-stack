# HyperStack

A portable Rust + egui desktop workspace for developers, DevOps, DBAs, testers
and integration engineers: code editor, SQL and NoSQL database clients, Redis
viewer, API client, SSH/SFTP, S3 browser, Git, terminal, log tail viewer and
a devtools drawer in one multi-row, multi-tab window that shares connections,
environments and secrets across every tool.

**Status: Phase 0 (foundation), M1 (terminal with search and links,
editor with Markdown + Mermaid preview, in-app updater) and M2 (SSH/SFTP
with tunnels, S3) implemented; M3 under way: the SQL database tool
(PostgreSQL, MySQL/MariaDB, SQLite) is done, NoSQL and Redis next. On
2026-10-15 the shell gained a pinned **Home** tab, theme modes, an
Accessibility page (Read Aloud), settings import/export, Snip and Print…,
and four tools landed: the **API client** (`tool-api`, Postman-compatible,
JavaScript `pm` scripts on Boa), **Devtools** (`tool-devtools`: Base64,
hashes, JWT, Firebase tokens, Jasypt, PKI messages), **Graphics**
(`tool-graphics`) and the **Browser** (`tool-browser` + WebView2 host in
the binary, Windows only). The editor gives every project folder its own
window tab with a multi-row strip of file tabs inside, shows the file's
path under them, runs files and projects (F5), shows Git status, diffs and
history (`hs-git`, read-only over the git CLI), and carries an optional AI
assistant (ADR-019: 1min.AI or any OpenAI-compatible service such as LM
Studio, `hs-core::ai`); Terminal, SSH, SQL, S3, API client and Graphics
have a review-only assistant whose code blocks can be inserted but never
run.** The Cargo workspace (`crates/hs-core`, `hs-ui`, `hs-term`,
`hs-editor`, `hs-ssh`, `hs-git`, `tools/tool-terminal`, `tools/tool-editor`,
`tools/tool-ssh`, `tools/tool-s3`, `tools/tool-db`, `tools/tool-api`,
`tools/tool-devtools`, `tools/tool-graphics`, `tools/tool-browser`,
`hyperstack`) builds to a portable `HyperStack.exe` and passes the full gate
(fmt, clippy `-D warnings`, tests, `--self-test`). `CHECKLIST.md` tracks what
is done; releases are published automatically from `main`.

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
| Why each major choice was made (ADR-001 … ADR-019) | `docs/08-decisions.md` |
| CI, automatic releases, asset contract, in-app updater | `docs/09-ci-cd-and-updates.md`; live workflows in `.forgejo/workflows/`, scripts in `scripts/`, signing key notes in `keys/README.md` |
| One design document per tool | `docs/tools/*.md` |
| README screenshots and walkthrough GIF, and how to retake them | `docs/screenshots/` (`README.md` there) |
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
- **Public GitHub mirror** (`docs/09-ci-cd-and-updates.md` §10): the same
  history is also pushed to a GitHub repository under a different account
  name. `origin` carries both forges as *push* URLs, so `git push origin
  main` (and `--tags`) lands on both; a separate `github` remote is the fetch
  side. Forgejo is the version authority - its release job bumps and tags,
  GitHub only builds the tag. Changes made on GitHub come back with
  `pwsh -File scripts/sync-from-github.ps1` (fast-forward only); never set up
  a Forgejo push mirror while GitHub is writable, it force-pushes and would
  discard them. The commit author stays `owner <owner@zacx.net>` on both.
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
- **The GitHub mirror releases the same tag.** `.github/workflows/release.yml`
  runs on a `v*` tag, takes the version from the tag name, refuses if the
  `Cargo.toml` at that commit disagrees, reads the notes back out of
  `CHANGELOG.md` (`scripts/changelog-section.ps1`) and builds with
  `HS_RELEASES_URL`/`HS_FORGE_URL` pointing at GitHub, so that copy updates
  from GitHub. Both workflows call the same scripts; the GitHub one must pass
  `-RepoUrl` to `build-and-package.ps1`. Every GitHub job is guarded with
  `if: github.server_url == 'https://github.com'` because Forgejo Actions
  also reads `.github/workflows/` and cannot fetch published actions.
- **The tag reaches GitHub from the Forgejo release job**: its last step,
  `scripts/push-to-mirror.ps1`, fast-forwards GitHub's branch to the version
  commit (never forced), pushes the tag and dispatches GitHub's
  `release.yml` for it, using the forge secrets `HS_GITHUB_MIRROR_URL` and
  `HS_GITHUB_MIRROR_TOKEN` (GitHub token with Contents + Workflows + Actions
  write). Without them the step is skipped and GitHub builds nothing.

## Build & run (once the workspace exists)

```bash
cargo build --release -p hyperstack        # target/release/HyperStack.exe
cargo run -p hyperstack -- --data-dir ./tmp-data   # run against a throwaway data folder
cargo run -p hyperstack -- --self-test     # what the CI smoke step runs
cargo test -p tool-s3                      # one tool crate at a time
HS_S3_ENDPOINT=http://127.0.0.1:5055 cargo test -p hs-core --test s3_live   # S3 client against moto_server / MinIO
HS_PG=127.0.0.1:5432:postgres:pg:postgres HS_MYSQL=127.0.0.1:3306:dev:dev:shop cargo test -p tool-db --test drivers   # SQL drivers (SQLite always)
pwsh -File scripts/next-release.ps1        # what the next push to main would release
pwsh -c 'Invoke-Pester scripts/tests'      # release-planner tests (Pester 5)
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
- Forgejo Actions reads `.github/workflows/` as well as `.forgejo/workflows/`.
  A GitHub-only workflow dropped into the tree runs on the Forgejo runner and
  dies on its first `uses:`; guard every such job with
  `if: github.server_url == 'https://github.com'`.
- GitHub does not start push-triggered workflows for a commit whose
  message contains `[skip ci]`, **including a tag push**: the tag of
  `chore(release): vX.Y.Z [skip ci]` reached GitHub and no release ran
  (0.12.1). `push-to-mirror.ps1` dispatches `release.yml` with the tag
  instead; by hand, Actions → Release → Run workflow.
- A Forgejo *push mirror* force-pushes and a *pull mirror* makes the repo
  read-only, so neither is usable for a two-way GitHub mirror. Two push URLs
  on `origin` plus a fast-forward-only pull script is the arrangement that
  rejects divergence instead of destroying it.

### Windows
- Renaming a running executable on the same volume is allowed; overwriting it
  is not. The updater relies on this (rename old → move new → relaunch), so
  keep `HyperStackData/update/` on the same volume as the exe.
- `std::fs::canonicalize` prefixes Windows paths with `\\?\`; strip that
  prefix before storing or comparing editor project roots, or saved paths and
  test/request URIs no longer match.
- BOM-less UTF-16 BE is ambiguous with other byte sequences; do not guess it
  during automatic decode. Offer an explicit encoding choice to reopen the
  original bytes instead.

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
- A widget in a side panel that asks for even a few pixels more than
  `available_width()` (a `TextEdit` with `desired_width(avail - n)` beside
  another, a button next to a full-width row) makes the resizable panel
  grow every frame, and the runaway width is then saved with the layout.
  Use `desired_width(f32::INFINITY)`, leave real slack for ComboBox
  padding, and overlay row buttons inside the row's rect (the SSH tunnel
  rows do).
- egui's default proportional font has no arrows (→ ↑ ↓) or box drawing;
  `hs_ui::fonts::install` appends the bundled Nerd Font as the last
  fallback so they render instead of tofu boxes.
- An `egui::Label` is text-selectable by default and takes the click, so a
  table row whose cell text is clicked neither selects nor opens. Cells in
  `hs_ui::table` use `.selectable(false)`; do the same for any label inside
  a clickable row.
- A single-line `TextEdit` gives up focus on Escape *before* your code sees
  the key, so `r.has_focus() && key_pressed(Escape)` never fires; test
  `(r.has_focus() || r.lost_focus())` (the terminal find bar does).
- `egui_extras::TableBuilder::sense(click)` registers a row-wide click area
  *after* the cells, so a cell's own `interact` never sees clicks (no cell
  selection, no double-click). Leave the builder's sense off and put
  `ui.interact(ui.max_rect(), id, Sense::click())` first in each cell,
  before the hover-only label (`tool-db::grid`).
- `EditorView` keeps per-line highlight spans and used to drop them only
  for edits it made itself, so a buffer changed from outside (the AI
  assistant applying an edit) drew each changed line cut at its old length.
  The view now invalidates the cache whenever the buffer version moved
  behind its back; edit the buffer from anywhere, the view catches up.
- egui drops keyboard focus on Escape before the tool runs, even when the
  tool then consumes the key (closing a completion list). Request focus for
  the editor again after handling such an Escape.
- Fluent plural selectors (`[one]`) never match string arguments, and
  `t_args` takes strings: `I18n::t_args` passes canonical integers as
  numbers so "1 row" / "1 tab" work. A new `[one]` message needs nothing
  more; non-canonical text ("007", "0.12.1") stays text.
- `egui_kittest`'s synthetic clicks (and raw pointer events) never form a
  double-click; cover double-click paths in the running app.
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

### Headless Linux (screenshots, GUI smoke tests)
- Under Xvfb the app panics at start without `libxkbcommon-x11-0`; install it
  (plus `libgl1-mesa-dri`) and it renders with llvmpipe. With no window
  manager, `xdotool` input is ignored until `xdotool windowfocus <id>`;
  `xdotool type` needs `--` before text starting with `-`.
- SSH **Follow Shell Folder** reads the xterm title (`user@host: dir`); a
  remote prompt that does not set it leaves the file pane where it is.
- `rfd` folder/file dialogs do not open headless; seed `layout.json` (editor
  `root`/`path`) instead of clicking Open Folder. S3/SFTP Upload and
  Download use them too, so test those paths through the client tests.
- S3 without MinIO: `pip install "moto[server]"` in a venv, run
  `moto_server -H 127.0.0.1 -p 5055`, keys `testing`/`testing`, then
  `HS_S3_ENDPOINT=http://127.0.0.1:5055 cargo test -p hs-core --test s3_live`.
  moto verifies the request shape, not the signature; the SigV4 unit tests
  against AWS's examples cover that.
- Under Xvfb a modal dialog can leave the window without keyboard focus
  (typed keys go nowhere afterwards); run `xdotool windowfocus <id>`
  again before typing.
- A shell heredoc that writes code must be quoted (`<<'EOF'`): with
  `<<EOF` the shell runs every `` `backtick` `` span in the text, which
  silently blanked the backticked names in doc comments being inserted.
- `pkill -f /opt/HyperStack/HyperStack` run from a Bash tool call also
  matches that shell's own command line and kills it (exit 144); stop the
  app with `pkill -x HyperStack`. `xdotool type` cannot type multi-byte
  characters such as "—".
- Live DB servers for the driver tests: PostgreSQL from the distro's
  `/usr/lib/postgresql/16/bin` (`initdb`/`pg_ctl` as the `postgres` user,
  with the data directory under `/var/lib/postgresql/` — the scratchpad's
  parent folders lose their permissions and the server then dies), and
  `apt-get install mariadb-server` then `mysqld_safe --user=mysql`.
- Repeated `cargo test` runs grow `target/debug` past 20 GB and the disk
  fills ("No space left on device", even `pwd` fails); delete
  `target/debug` and rebuild.
- `cargo clippy --workspace` on Linux needs `libwayland-dev`,
  `libxkbcommon-dev` and `pkg-config` (the `egui_kittest` dev-dependency
  builds `wayland-sys`). Windows-only helpers (`hs-term::shells::on_path`)
  must be `#[cfg(windows)]`, or Linux clippy fails on dead code that the
  Windows CI never sees.

### SSH (russh)
- russh 0.64 with `default-features = false, features = ["ring", "rsa"]` so it
  builds on Windows without aws-lc-rs (which needs NASM/CMake, absent here).
  The server `check_server_key` takes `&PublicKeyOrCertificate` (call
  `.public_key()`); agent auth iterates `request_identities()` and uses
  `authenticate_publickey_with`. Bound only the TCP connect by a timeout, not
  the whole handshake (a host-key prompt may wait for the user).
- A shell channel is exposed to `hs-term` through the `Transport` trait
  (`hs-ssh::ShellTransport`) plus a `ChannelReader` fed by the channel's data.
- russh still calls the server `Handler::data` for a channel even after you
  take it via `into_stream`, so a test/real server that also runs an SFTP
  subsystem on another channel must skip `data` for the SFTP channel id, or it
  echoes the client's SFTP bytes back and the client sees a duplicate INIT
  ("sender dropped"). The HyperStack SFTP *client* is the stock
  `SftpSession::new(channel.into_stream())` pattern and is fine against real
  servers; the bug was only in the in-repo test server.
- Live SSH test: `hs-ssh` ships an in-process test server behind the
  `testserver` feature and a `devsshd` example (`cargo run -p hs-ssh --features
  testserver --example devsshd -- 2222`, user `dev`/`dev`). Point the SSH tool
  at `127.0.0.1:2222`. WSL's own sshd is NOT reachable from Windows under the
  default NAT networking, so use devsshd for manual UI testing.

### Databases (tool-db)
- `mysql_async` builds its rustls config with `ClientConfig::builder()`,
  which panics when more than one crypto provider is compiled in (reqwest
  brings aws-lc-rs, we use ring): install ring as the process default
  once before connecting (`driver::mysql::install_crypto_provider`).
- A Postgres cancel request issued when a row limit is reached must be
  *awaited* before the next statement starts, or it can cancel that one;
  and inside a transaction block it aborts the transaction, so there the
  rest of the result is read and dropped instead. MySQL's `KILL QUERY`
  likewise goes out (and is awaited) before draining the result.
- Postgres via `simple_query` returns every value as server text (the
  psql form), which is what the grid wants; column types come from a
  separate `prepare` of the same statement and are only used for alignment
  and unquoted numbers.

### AI assistant (hs-core::ai)
- 1min.AI is not OpenAI-compatible: `POST https://api.1min.ai/api/chat-with-ai`
  (`?isStreaming=true` for SSE) with an `API-KEY` header (no `Bearer`) and
  `{"type":"UNIFY_CHAT_WITH_AI","model":…,"promptObject":{"prompt":…}}`; the
  older `CHAT_WITH_AI` type on `/api/features` is gone. It takes one prompt,
  so the conversation is folded into a transcript; there is no model-list
  endpoint and no tool calling, which is why edits use SEARCH/REPLACE blocks
  rather than tools. The SSE stream sends `content` pieces, then a `result`
  record repeating the whole answer (ignore it unless nothing streamed).
- Its docs host (`docs.1min.ai`) and `chatplayground.ai` do not resolve from
  the cloud sandbox; test against a local mock server that speaks both SSE
  formats (the OpenAI one ends with `data: [DONE]`).

### Shell (2026-10-15)
- `Theme::apply` used to call `ctx.set_visuals`, which writes only the
  visuals slot of egui's *current* theme; with `ThemePreference::System`
  Windows 10 reported its theme after the first frame and egui switched to
  its default light visuals, so a saved dark scheme looked lost. Pin the
  preference (`ctx.set_theme`) and fill both slots (`set_visuals_of`) —
  `hs_ui::theme::install` does; never call `set_visuals` directly.
- Home and Settings are mandatory pinned tabs (`app::MANDATORY`). They are
  stripped from saved layouts and re-created by `ensure_mandatory_tabs`;
  anything that counts, closes or unpins tabs must skip them.
- The GUI smoke harness (`scratchpad/gui.ps1` pattern: `Start-Process` with
  `--data-dir`, `SetProcessDPIAware`, `MoveWindow 0,0,1400,900`, SendKeys
  and `mouse_event`): the first click after a native file dialog closes
  only re-activates the window, so click twice or screenshot after each
  click; `GetWindowRect` includes the invisible 8 px border, so a stray
  glyph at the very edge of a screenshot is the desktop behind the window.
- Two `cargo` invocations on one target folder serialise on the build
  lock; a background `cargo test` started before a new crate is registered
  fails on that crate's missing module, so re-run after registering.
- A running `HyperStack.exe` keeps `target/debug/HyperStack.exe` locked;
  `Stop-Process -Name HyperStack` before `cargo build`.

### API client (tool-api)
- Boa's native closures must be `Copy`; host state goes through a
  `thread_local!` record (`script::HOST`) written before `run` and read
  after. `JsValue::to_json` needs a `&mut Context`; strings come out as
  `to_std_string_escaped()`.
- `serde_json::Value::Object` is a BTreeMap: re-serialising a JWT header
  or payload reorders keys and changes the signature input. Compact the
  user's text instead (`jwt::compact_json`) when it already validates.
- The workspace `reqwest` has no `form` or `multipart` feature:
  `send::form_body` and `send::multipart` build those bodies by hand.

### Browser (tool-browser + browser_host)
- A WebView2 child window paints above everything egui draws. The host
  must hide the view whenever a modal, the palette or a popup is open
  (`overlay` in `app.rs`) and whenever the tab is not visible; the tool
  reports its rectangle every frame and the host clears it after use.
- `wry` is a `[target.'cfg(windows)'.dependencies]` entry of the binary
  only; the tool crate must stay free of platform code so Linux clippy
  passes.
- JavaScript on/off is fixed at view creation: flipping it rebuilds the
  view (`Cmd` handling in `BrowserHost::sync`).

### Connection forms
- `store_typed_secrets` used to warn and carry on when the vault was
  locked, so the profile was saved with the typed password in its TOML. It
  now returns false and the form does not save; any new form must check
  that result before `upsert`.

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
- A beta's bump was measured only from the previous beta, so `fix:` after a
  `feat:` beta planned `0.1.1-beta.1` after `0.2.0-beta.1`. Bump over the
  range since the last *stable* tag; limit only the notes to the channel
  range (`scripts/tests/next-release.Tests.ps1` covers it).
- The PowerShell Gallery is unreachable from the cloud sandbox; Pester comes
  from nuget.org's `pester` package (chocolatey layout: copy its `tools/`
  folder to `~/.local/share/powershell/Modules/Pester/<version>`).
- `Start-Process -Wait` puts the started process in a job and waits for the
  whole tree; when the wait ends it kills what is still running in that job.
  An updater end-to-end test run that way sees the relaunched new version die
  a frame after it started (state says `ok = true`, no process left). Run
  `HyperStack.exe --install-update` / `--revert-update` from Bash (or without
  `-Wait`) when the relaunched process must survive.
- `[ref]$name` on a variable that was never assigned throws "[ref] cannot be
  applied to a variable that does not exist". The Actions `pwsh` shell runs
  with `$ErrorActionPreference = 'stop'`, so this failed `ci.yml` in three
  seconds before any Rust step. Always `$x = $null` before `[ref]$x`, and
  reproduce workflow snippets locally under `Stop` before pushing.

### Rust / build
- Merging a release bump (`chore(release): vX.Y.Z`) into a branch that
  adds a workspace crate leaves that crate's `Cargo.lock` entry at the old
  version (the release script only bumps the members it knows), and every
  `--locked` command then fails in CI. After such a merge run any `cargo`
  command once without `--locked` (it rewrites the entry) and commit
  `Cargo.lock`.
- `.cargo/config.toml` sets `-C target-feature=+crt-static` so the MSVC build
  is a fully static, portable exe; keep it.
- `cargo clippy --fix --allow-dirty` clears the mechanical lints
  (collapsible ifs, `sort_by_key`) in one go; with edition 2024, nested
  `if let` blocks collapse into let-chains (`if let Some(x) = a && cond`).
- Cargo does not track `option_env!` for rebuilds, and a workflow variable
  that exists but is blank arrives as `Some("")`. `hs-core/build.rs` declares
  `rerun-if-env-changed` for every baked-in forge variable (the CI runner
  shares one `CARGO_TARGET_DIR`), and the `match` arms treat empty as unset;
  `update::tests::release_source_follows_the_build_environment` fails on a
  stale object file. `match` with guards and `&[a, b]` promotion both work
  in a `const` initializer.
