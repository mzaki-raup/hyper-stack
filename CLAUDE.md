# HyperStack

A portable Rust + egui desktop workspace for developers, DevOps, DBAs, testers
and integration engineers: code editor, SQL and NoSQL database clients, Redis
viewer, API client, SSH/SFTP, S3 browser, Git, terminal, log tail viewer and
a devtools drawer in one multi-row, multi-tab window that shares connections,
environments and secrets across every tool.

**Status: Phase 0 (foundation), M1 (terminal with search and links,
editor with Markdown + Mermaid preview, in-app updater) and M2 (SSH/SFTP
with tunnels, S3) implemented; M3 under way: the SQL database tool
(PostgreSQL, MySQL/MariaDB, SQLite), the **Redis** tool (`tool-redis`:
SCAN key tree, type-aware viewers and editors, console with guards, Info)
and the **NoSQL** tool (`tool-nosql`: MongoDB and CouchDB through one
`DocStore`, container tree, find, document editing with guards) are done,
which completes M3. On
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
assistant (ADR-019: Anthropic, Google Gemini, Azure OpenAI, AWS Bedrock,
1min.AI or any OpenAI-compatible service such as LM Studio, added from a
gallery in Settings grouped by capability, `hs-core::ai`; image providers -
1min.AI or a self-hosted AUTOMATIC1111/SD.Next/Forge, ComfyUI, SwarmUI,
InvokeAI or Fooocus server with per-provider workflow settings,
`hs-core::ai_local` - feed the Graphics activity); the **Logs** activity
(`tool-logs`, 2026-10-22: local files followed like `tail -F`, SSH files
through `hs_ssh::SshClient::exec`, command output, pasted text; parsers
for plain, JSON lines, logfmt, syslog, Nginx, PostgreSQL and - since
2026-10-24 - **JVM thread dumps** (one record per thread with its stack
folded in and its state, locks and top frame as filterable fields,
`docs/tools/logs.md` §17, opened with **Open Thread Dump…** or pasted);
filter chips,
highlights, find, jump to time, bookmarks, saved sources and presets);
the bottom dock's Logs tab shows the app's own log at the level set in
Settings → General → Behaviour; since 2026-10-23 the **updater downloads
the release zip** and takes the executable out of it (the bare exe is
over the forge edge's 100 MB), and the **activity rail's order,
visibility and every sidebar width are per workspace** (`[rail]` in
`workspace.toml`, edited for any workspace from Settings → Appearance,
with a global Coloured Activity Icons switch that is off by default);
Settings → General → Startup ends with **Window Size** - the window's live
width and height plus a Full Screen or Custom picker
(`window_state::SizePreset`), over a **strict 1720 × 860 minimum** the
viewport, the fields and `WindowState::sanitised` all enforce;
Terminal, SSH, SQL, Redis, NoSQL, S3, API client and Graphics
have a review-only assistant whose code blocks can be inserted but never
run; the API client's assistant can also **fill the client** through
`hs-api` action blocks the user applies, and the **Scratchpad** is a
per-workspace collection of Markdown notes (sidebar list, pinning, one tab
per note, source + preview, assistant); every assistant has a microphone
toggle (`hs_core::stt`, offline Windows Speech, typed into the input),
per-tool quick actions and **Insert and Run** for the tools that execute
(ADR-019 as amended); the
editor is an **LSP client** (`hs-lsp`, ADR-023): completion, hover,
diagnostics, definition, references, rename and formatting through
language servers the user installs (Settings → Editor → Language
Servers), and a **DAP client** (`hs-dap`, ADR-024): breakpoints,
stepping, variables, call stack and console through installed debug
adapters (CodeLLDB, lldb-dap, GDB, debugpy, delve, netcoredbg), with
configurations from `.hyperstack/debug.json`, `.vscode/launch.json` or
generated for Cargo, Python and Go projects.**

**In progress (2026-10-20):** the Graphics activity grows into four tools —
Image, Icon, Diagram Editor and Diagram Generator — with a dashboard home tab,
an activity sidebar and a Recent section. Phases 1-6 landed: the sidebar and
dashboard, separate Image/Icon tabs, Convert to Icon, icon colour-key
transparency (`GraphicsHub`, `Tool::wants_dashboard_home`,
`Op::TransparentColor`), the `hs-diagram` core (model, strict Archify-adapted
IR, validation, flow/grid layout, scene, SVG/PNG/PDF export), the Diagram
Editor (canvas, selection, drag, add/connect, undo, properties, templates,
save/export), the Diagram Generator (prompt/examples, async AI into the strict
IR, validation, offline templates, preview, Open in Editor, export) and the
review-only AI assistant in all four tools. The generator now has its one repair
round. A diagram tab writes a debounced and close-time draft into Recent, and
Save to Project writes into an open Editor project's `docs/diagrams/` folder
(Editor roots are published through `OpenTabs`), with resize grips,
multi-select align/distribute and edge label/waypoint editing. Import now reads Archify
JSON (strict IR) and uncompressed draw.io XML. Still to come: ERD, sequence and
compressed/multi-page draw.io. The
About page now carries the compiled-in `THIRD_PARTY_NOTICES.md`.
The requirements are in `docs/tools/diagrams.md`, the
pinned Archify schema adaptation in `THIRD_PARTY_NOTICES.md` and ADR-025. The
diagram library is `hs-diagram`; the UI stays in `tool-graphics`.
The Cargo workspace (`crates/hs-core`, `hs-ui`, `hs-term`,
`hs-editor`, `hs-ssh`, `hs-git`, `hs-lsp`, `hs-dap`, `hs-diagram`, `tools/tool-terminal`, `tools/tool-editor`,
`tools/tool-ssh`, `tools/tool-s3`, `tools/tool-db`, `tools/tool-api`,
`tools/tool-devtools`, `tools/tool-graphics`, `tools/tool-browser`,
`tools/tool-redis`, `tools/tool-nosql`, `tools/tool-logs`, `hyperstack`) builds to a portable `HyperStack.exe` and passes the full gate
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
| Why each major choice was made (ADR-001 … ADR-025) | `docs/08-decisions.md` |
| CI, automatic releases, asset contract, in-app updater | `docs/09-ci-cd-and-updates.md`; live workflows in `.forgejo/workflows/`, scripts in `scripts/`, signing key notes in `keys/README.md` |
| One design document per tool | `docs/tools/*.md` |
| Container activity (Docker, Podman, Kubernetes, Windows containers) | `docs/tools/containers.md`, ADR-026 |
| Graphics activity, diagram editor/generator, Archify-adapted IR | `docs/tools/diagrams.md`, `THIRD_PARTY_NOTICES.md` |
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
  "Has any tool seen this?" goes through `hs_core::search`: a tool
  implements a `Searcher` over its **own** store and the binary
  registers it, so neither side learns the other's schema
  (`docs/tools/logs.md` §19.5).
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
  **A parenthetical attached to a label is its own unit and starts with a
  capital**: "None (Direct connection)", "Inspect (Developer tools, opens a
  window)", "(Not set)". A parenthetical *inside* a running sentence is part
  of that sentence and stays lower case ("Opens .hyperstack/debug.json in the
  project (created with a template when missing)."), which is what sentence
  case means. Owner's rule, 2026-10-17.
- **One UI/UX language across every activity and tool.** Dashboards use the
  same section headings and top spacing; list cards centre their icon and
  label vertically; row actions sit in a fixed, aligned column rather than
  floating at the far edge; the **AI Assistant** control is always the hard
  right item of a tool toolbar; empty, error and hint text follow the shared
  form/dashboard helpers. When a new or recent surface differs, audit and fix
  it in the same change. Owner's rule, 2026-10-20.
- **Every action, option, form and label looks professional, tidy and
  appropriate.** Reuse the existing `hs_ui::form`, `hs_ui::dashboard` and
  toolbar patterns before inventing a one-off control; keep Title Case for
  labels and sentence case for hints, and leave consistent padding and
  separators. Owner's rule, 2026-10-20.
- **Right-click copy/paste in text surfaces (PuTTY convention).** In the
  terminal and in every editor/text field we draw ourselves: right-click with
  a selection copies it to the clipboard and clears the selection; right-click
  with no selection pastes the clipboard at the cursor. A per-tool setting may
  turn it off; the default is on. Owner's rule, 2026-10-11.
- **The app never opens an OS terminal.** Every command HyperStack runs
  for itself goes through `hs_core::paths::background_command`, which
  sets `CREATE_NO_WINDOW` on Windows; a bare `std::process::Command::new`
  for an internal command is a bug, because it flashes a console on
  screen (the microphone, Read Aloud and the updater's `tasklist` probe
  all did until 2026-10-24). A command whose *output the user is meant to
  see* belongs in a surface the app owns - the Terminal activity, the
  editor's terminal panel, a Logs tab, the DAP console - never a console
  of its own. The only commands that may open something outside the app
  are the deliberate hand-offs to the OS: `explorer`/`open`/`xdg-open`
  for Reveal in Explorer and the updater relaunching the app itself.
  Owner's rule, 2026-10-24.
- **Cross-platform core.** Windows ships first, but `hs-core`, `hs-ui`,
  `hs-editor`, `hs-term` and every tool must compile on Linux and macOS.
  Platform code lives only in `hs-core::secrets`, `hs-core::storage::paths`,
  `hs-core::clipboard_files`, `hs-core::env_path` and `hs-term::pty`.

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
HS_REDIS=127.0.0.1:6379 cargo test -p tool-redis --test live   # Redis driver against a real server
HS_MONGO=127.0.0.1:27017 HS_COUCH=http://admin:pw@127.0.0.1:5984 cargo test -p tool-nosql --test live   # MongoDB and CouchDB drivers
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
- `POST /releases` right after the tag push can fail with an empty error
  body while the forge derives its own row for the tag (release run 66,
  v0.20.0): the tag and the bump commit were on `main`, no release existed,
  and a re-run would have planned nothing. The publish step now retries
  fetch-or-create, and the plan step passes an unpublished newest tag to
  `next-release.ps1 -Republish` so the next push to `main` publishes it.
- **Do not build locally while a job is running: the runner is this
  machine.** This caught me a third time on 2026-10-27 (run 410): the
  push, the monitor and then a local build of the *next* change, all in
  the same minute. Starting the CI watch is not the end of the push -
  the machine is busy until that watch reports. A push starts CI here, and a `cargo clippy`/`cargo test`
  started in the same minutes competes for the same cores, RAM and
  build lock - run 378 and run 381 both died this way, minutes into a
  job whose commit had passed the identical gate locally. The tell is
  a job that fails after a few minutes with nothing useful in the
  forge UI while the same commit is green on the desk. **Check for a
  running job before any local `cargo` command after a push**
  (`/actions/tasks?limit=4`), and if one is running, do reading or
  writing work until it finishes. Re-running the job on a quiet
  machine is the fix; "it failed, so something must be wrong with the
  code" is the wrong conclusion to reach first.
- The release job can die in `cargo test` with `rustc-LLVM ERROR: out of
  memory` / `STATUS_ILLEGAL_INSTRUCTION` (run 79, 2026-10-15) when the
  development machine is busy: the runner is this machine, and the test
  build compiles every crate's test binary in parallel. Nothing is bumped,
  tagged or published before that step, so the fix is to re-run the job
  from the Actions page (or push a `docs:` commit, which re-plans the same
  version); check nothing heavy is running locally first. A failing CI on
  the same commit minutes earlier would mean a real error instead.
- An asset upload can die mid-stream ("Unable to write data to the transport
  connection: An existing connection was forcibly closed by the remote host",
  release run 92, v0.29.0). The step had no retry, so it ended with the tag
  and the bump commit on `main` and a **draft** release carrying one asset.
  The upload loop now retries each file five times with a growing pause, and
  deletes a half-written duplicate first. Recovery when a publish dies this
  way: the plan step reads a draft release as unpublished and passes the tag
  to `next-release.ps1 -Republish`, so the **next push to `main`** rebuilds
  the same version and publishes into the existing row (the bump and tag
  steps are skipped on a republish). This forge has no rerun API -
  `POST /actions/runs/<id>/rerun` is 404 - so it is either that push or the
  Re-run button in the Actions page.
- The release checkout can die before planning with `RPC failed; curl 56
  Recv failure: Connection was reset`, `early EOF` and `unable to read tree`:
  a partial pack from the full-history fetch cannot be resumed, so nothing is
  tagged or published. `.forgejo/workflows/release.yml` now throws the `.git`
  directory away and retries the fetch five times with a growing pause; a
  failure after that is a real outage, not a build problem.
- `git.zacx.net` is served through Cloudflare, which gives the origin about
  **100 seconds** to answer and then cuts the request with a **524**. A
  release asset is too big for that: the 34 MB archive needed six minutes
  of retries to land and twice left the release a draft with only the
  changelog attached (runs 92 and 95), and the 83 MB exe is also near the
  free plan's 100 MB body cap. The forge answers the same API directly on
  the local network, and the same upload takes **0.24 s** there. The
  publish step reads that address from the `HS_FORGE_UPLOAD_URL`
  repository secret and uses it for assets only; unset (as on GitHub) it
  falls back to the public URL. A `Host` header carrying the public
  hostname goes with it - repository routes 404 without it. The address
  itself lives only in that secret: never write it into the tree, the
  history is mirrored publicly.
  The runner's own control channel goes through Cloudflare too and logs
  the same resets all day (`failed to fetch task`, `UpdateLog`,
  `UpdateTask` in `C:\Apps\gitea-runner\service-err.log`) - it retries,
  so they are noise rather than failures, but that log is the place to
  look when a job dies without a useful error, because this Forgejo has
  no API for job logs (every `/actions/.../logs` route is a 404) and no
  rerun endpoint either.
- A PowerShell step with a non-terminating error carries on and the job goes
  green having uploaded nothing. Every step starts with
  `$ErrorActionPreference = "Stop"` and the packaging step asserts every
  contracted asset exists on disk before upload.
- The verification step can fail on infrastructure, not on the release:
  on github.com the anonymous API allows 60 calls an hour **per IP**, and a
  shared Actions runner exhausts that before the step runs ("API rate limit
  exceeded for 40.65.61.144"), while git.zacx.net's Cloudflare can reset the
  97 MB `HyperStack.exe` download mid-stream. Neither means the assets are
  wrong: check the release (`gh release view`, or the local
  `verify-release.ps1`) before rebuilding. The script now retries downloads
  and takes `-Token` for the metadata read only; asset downloads stay
  anonymous, exactly as the updater makes them.
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
- `cargo deny check` reads the live advisory database, so a gate that was
  green can go red without a code change. Adding `resvg`/`svg2pdf` as direct
  dependencies surfaced `RUSTSEC-2026-0206` (`rustybuzz` unmaintained) even
  though it was already transitive through `egui_extras`; the ignore in
  `deny.toml` records why (local SVG rendering only, no fixed version).
  It happened again on 2026-10-24: `RUSTSEC-2026-0387` against
  `crypto-bigint 0.7.5` (constant-time operations taking
  secret-dependent branches), reached through
  russh → rsa/ssh-key/p256 → crypto-bigint, on a branch whose code had
  not touched any of that. **A red `cargo deny` is the first thing to
  check when CI fails on a branch that passed locally an hour earlier**,
  and when the advisory names a fixed version the answer is
  `cargo update -p <crate>` and a `security:` commit, not an ignore -
  ignores are for advisories with no fix.

### Git and signing
- A fixture repository in a test inherits the owner's global
  `commit.gpgsign = true` / `tag.gpgSign = true`, so a `git commit` there
  can hang on pinentry or fail with "gpg failed to sign the data" the
  moment the agent's cache goes cold - a test that passed a minute earlier
  then fails for no code reason. Every fixture repo that commits or tags
  sets `git config commit.gpgsign false` and `git config tag.gpgSign false`
  right after `init` (`hs-git::write` tests).
- Every commit here is GPG-signed with the owner's key, and
  `gpg: signing failed: Timeout` does **not** mean signing is broken — it
  means the agent had nothing cached and the pinentry prompt never got
  answered. Retrying from here does not help: the prompt appears fine when
  the owner signs from their *own* PowerShell (verified 2026-10-17 — a
  `gpg --clearsign` there prompted and succeeded on an uncached key), so a
  Timeout in an agent-driven shell means the dialog is not reaching them,
  not that they let it lapse. `gpg --pinentry-mode error --clearsign`
  reports "No pinentry" when the passphrase is not cached, which tells a
  locked agent from a real failure without burning a two-minute timeout.
  **Ask the owner to warm the cache in their own terminal**
  (`"warm" | gpg --local-user B89A6E03D2371647 --clearsign | Out-Null`),
  then commit; hold the work staged meanwhile (a resolved merge keeps its
  `MERGE_HEAD`, so the resolution is not lost) and never reach for
  `--no-gpg-sign`. `%APPDATA%\gnupg\gpg-agent.conf` now sets
  `default-cache-ttl 14400`, so one unlock covers a long session; the
  agent only re-reads it after `gpgconf --reload gpg-agent`, and
  `gpgconf --kill gpg-agent` drops the cache outright.
- A `chore: merge vX.Y.Z into the … branch` merge can land with the
  `CHANGELOG.md` conflict half-resolved: v0.60.3 through v0.65.0 all
  carried `=======` / `>>>>>>> main` lines and a duplicated, truncated
  history, and every later release bump prepended on top of it. Nothing
  in the gate reads the changelog, so it stayed that way for five
  releases. `grep -n '^<<<<<<<\|^=======\|^>>>>>>>' CHANGELOG.md` before
  committing any merge; `ci.yml` now fails on markers in tracked text.
  Repair is the last clean tag's copy plus the sections released since.
- **Writing `Unreleased` notes before the release lands guarantees a
  CHANGELOG conflict**, and resolving it by keeping "both" duplicates
  every line into the released section - which is how v0.60.3..v0.65.0
  got their mangled history. When the release job moves `Unreleased`
  into `## X.Y.Z`, the branch still has those lines under `Unreleased`,
  so `git stash pop` (or the merge) collides on exactly that hunk. The
  resolution is never "both": the **released section belongs to the
  workflow** - keep its generated one-line-per-commit form verbatim -
  and `Unreleased` keeps **only what is not yet released**, so every
  item the release just took is deleted from it rather than kept. Then
  `grep -c '^<<<<<<<\|^=======\|^>>>>>>>' CHANGELOG.md` must print 0
  before committing.
- Adding a **new workspace member** on a branch and then merging a release
  bump from `main` leaves that member at its old version in `Cargo.lock`
  (`update-version.ps1` could not see a crate that only existed on the
  branch). Every `--locked` command then dies with "cannot update the lock
  file"; run a plain `cargo check` after such a merge, commit the one-line
  lock fix, and remember the release job may already have tagged `main`
  before the merge.

### Shutting down
- **Dropping the tokio `Runtime` waits for every blocking task**, and
  `main` returning is what drops it. A `spawn_blocking` follower parked
  in a blocking `read` never returns, so the window closed and the
  process stayed alive for ever - Task Manager was the only way out, and
  the updater's "Restart and Install" looked broken because the new
  version waits 30 s for the old pid and then opens beside it (owner's
  report, 2026-10-25). `main` ends with `std::process::exit(0)`: by then
  the layout is saved and `on_exit` has dropped the tools. Never let
  process exit depend on a blocking task finishing, and never add a
  `Drop` that waits unbounded on a child.
- **A stop flag does not reach a thread parked in `read`.** Anything
  that follows a child must be able to *end* that child from outside
  the reading thread - the flag is only read between reads, and for a
  quiet source there is no next read. On Windows killing a `cmd /C …`
  is not enough either: its grandchild (`wsl.exe`, `docker`) inherited
  the pipe and keeps it open, so the kill has to take the tree
  (`taskkill /T /F /PID`).
- The shell arms a **watchdog** on the first close request that exits
  the process a few seconds later whatever is still running. It is the
  floor, not a licence: a hang it covers is still a bug. It fires after
  any update install, so an install that ever became slow (a cross-
  volume copy rather than a rename) would need the grace period
  revisited.

- **"Open this activity" and "give me a new tab" are different
  requests**, and `open_tool_in`'s dashboard shortcut has to tell them
  apart: an activity with a dashboard home answers a request with no
  item by focusing that dashboard, which for New Note in the Scratchpad
  sidebar - and for the strip's `+` - was the tab already in front, so
  the button read as dead (owner's report, 2026-10-25). The shortcut is
  gated on `reuse`, and the `+` asks for a new tab of the activity
  rather than for the activity. Any new "New …" button must go through
  `open_new_tab`, never `open`.

- **A nested layout does not reset the direction.** Inside a
  `right_to_left` group, `ui.horizontal` keeps going right-to-left - so
  a label added before its control lands on the *wrong side* - and
  `ui.with_layout(left_to_right, ..)` takes **all** the width that is
  left, so two of them overlap each other. To put "Label [control]"
  pairs in a right-to-left group, add each control first and then its
  label, and add the pairs in reverse order (2026-10-26, the title bar's
  Workspace and Environment pickers).
- **A Nerd Font glyph is drawn smaller than a Phosphor one at the same
  font size.** Phosphor's icons fill the em (mean height 0.722 em);
  a Nerd Font's brand marks are drawn to fit one cell of a monospace
  grid (0.559 em), so the Docker whale beside a Phosphor plug read as
  a smaller icon. The fix is one number rather than a scale threaded
  through every draw site: `fonts::install` registers the bundled Nerd
  Font **twice** - plain, for the terminal and the editor families, and
  again as `hs-icon-fallback` with `FontTweak::scale = 1.29` for the
  interface's last glyph fallback. Measure before changing it
  (`scratchpad/metrics.py`), and remember the scaled copy must stay
  **last** in `Proportional`: ahead of a real text font it would draw
  the letters too (2026-10-26).
- **egui's own `emoji-icon-font` steals Private Use Area codepoints
  too, and it is not Phosphor.** It is one of epaint's default
  Proportional fonts, a *legacy icon* set rather than an emoji one, and
  it claims 58 PUA codepoints - among them `U+E627` (Go), `U+E61D` (C++)
  and `U+E62D` (Elixir). The Go gopher therefore rendered as the
  **SoundCloud logo**, and the font file itself was innocent: rendering
  the codepoint with PIL showed a perfectly good gopher. `fonts::install`
  now inserts the icon fallback **before `emoji-icon-font`** (still after
  `NotoEmoji-Regular`, so real emoji are unaffected), and a test pins
  that order. The checker to run is `scratchpad/shadow.py`, which walks
  the whole fallback chain rather than Phosphor alone; `scratchpad/
  sheet.py` renders candidates to a contact sheet so the picture can be
  looked at without building the app (2026-10-26).
- **Phosphor is installed before the Nerd Font and wins where they
  overlap.** Both live in the Private Use Area, so a codepoint copied
  from a Nerd Fonts cheat sheet can quietly draw something else: `U+E76E`
  is PostgreSQL there and a **safe** in Phosphor, `U+E7B0` is Docker and
  a thin unrelated mark, `U+EBCA` is a bash prompt and a cube. All three
  were shipped-and-caught on screen before the cause was understood.
  `scratchpad/collide.py` compares the two cmaps and says which
  codepoints are unusable; run it before choosing a glyph, then still
  look at the result (2026-10-26).
- **Check a Nerd Font glyph by looking at it.** The codepoint being in
  the font says nothing about *which* picture it is: `U+EBCA`, listed as
  the codicon terminal-bash, draws a cube in the bundled JetBrains Mono,
  and shipped as Git Bash's icon it would have been simply wrong. The
  distro logos (`U+F300..`) and the two PowerShell marks were each
  confirmed on screen before being kept.

### Windows
- Renaming a running executable on the same volume is allowed; overwriting it
  is not. The updater relies on this (rename old → move new → relaunch), so
  keep `HyperStackData/update/` on the same volume as the exe.
- `std::fs::canonicalize` prefixes Windows paths with `\\?\`; strip that
  prefix before storing or comparing editor project roots, or saved paths and
  test/request URIs no longer match.
- `target/debug` on this machine reached **280 GB**, 150 GB of it
  `debug/incremental`, and filled D: completely: the symptom is
  `failed to build archive ...: There is not enough space on the disk.
  (os error 112)` in the middle of a `cargo test --workspace`, and a
  `Get-PSDrive` a minute later can already show space again because the
  failed writes are rolled back. Delete `target/debug/incremental` — it is
  cache, the artifacts in `debug/deps` stay, and the next build is not a
  full rebuild. Check it after a long session of repeated builds.
- A debug link can fail with `LINK : fatal error LNK1140: limit exceeded
  for program database` once `target/debug` has grown large enough: the
  PDB has outgrown its format limit, and no code change causes or fixes
  it. Do **not** reach for `CARGO_PROFILE_DEV_DEBUG=0` on the command
  line as a workaround - it changes every crate's fingerprint, triggers a
  full rebuild of the whole workspace beside the artifacts that are
  already there, and filled D: within minutes (os error 112 / "no space
  on device" mid-build). Delete `target/debug/incremental` first, and if
  it recurs set the profile's debug level in `Cargo.toml` so the
  fingerprint is stable rather than passing it per invocation.
- `cargo test --workspace` on this machine can die with "the paging file
  is too small" (os error 1455) and then "found invalid metadata files for
  crate …" on every later run: the parallel test build exhausts the
  Windows *commit limit* (RAM plus a small page file), not physical RAM,
  and a rustc killed mid-write leaves corrupt `.rmeta`/`.rlib` files
  behind. Delete `target/debug` and run with `-j 4`; the CI runner's
  release job OOM (run 79) was the same thing with the machine busy.
- The runner service's `%TEMP%` is `C:\WINDOWS\SystemTemp` (upper case,
  LocalSystem), while `canonicalize` returns `C:\Windows\…`; a test that
  compares an editor-reported path with one built from `temp_dir()` passes
  locally and fails on CI. Build expected paths from the canonical folder
  (`crates/hyperstack/src/editor_tests.rs`), or compare case-insensitively.
- Resolving a command on `PATH` must go through `PATHEXT` **only** on
  Windows, never falling back to the bare name: a file with no extension
  cannot be started (`CreateProcess` says "%1 is not a valid Win32
  application"). jdtls unpacks `bin/jdtls` (a Python script) next to
  `bin/jdtls.bat`, and `hs_lsp::registry::candidates` used to list the
  bare path first, so Java reported **Found** and then failed to start.
  `hs_dap::adapters` had the same copy of the function.
- A process keeps the `PATH` it was given at start, but on Windows the real
  value can be read again from the registry (`HKCU\Environment` and the
  machine's `Session Manager\Environment`, expanding `REG_EXPAND_SZ`):
  `hs_core::env_path::refresh` does that and Settings → Editor's **Refresh
  Status** calls it, so an installed server is found without a restart.
  Do not `std::env::set_var("PATH", ...)` to achieve this - it is unsound
  while other threads read the environment; keep the value beside it and
  have the lookups ask for it. A language server or
  adapter installed while HyperStack is open is invisible until it is
  restarted - and so is a tool installed while the Claude Code shell has
  been open, which is why `jdtls` was missing from this session's `PATH`
  but present for a fresh `pwsh`. Both settings tables now say so.
- BOM-less UTF-16 BE is ambiguous with other byte sequences; do not guess it
  during automatic decode. Offer an explicit encoding choice to reopen the
  original bytes instead.
- The DSH Windows sandbox can fail before a command starts with
  `SetNamedSecurityInfoW … grantWrite` or `sandbox-local windows-acl temp
  grant materialization failed`. On 2026-10-19 the workspace root and
  `D:\Temp` both lacked effective `WRITE_OWNER` for the signed-in user; the
  `diagnose-windows-sandbox-acl` skill (one unconfined run per path) repaired
  those two objects and verified the rights. Do not widen the workspace or
  edit ACLs by hand; if it recurs, run the skill on the failing path and on
  `%TEMP%`.

### egui / eframe 0.35
- `egui::Id::new(x)` needs `x: Hash + Debug`; a helper taking
  `id_salt: impl Hash` must add `+ std::fmt::Debug` (or use
  `Id::new(base).with(salt)`), or the build fails on that bound.
- Never nest two context locks: `ui.data_mut(|d| … ui.input(|i| …) …)`
  takes the `ContextImpl` write lock and then asks for it again, and in a
  debug build epaint panics after ten seconds ("Failed to acquire RwLock
  write"). Read `ui.input` into a local first, then call `data_mut`.
- `Response::drag_started()` fires only once the pointer has moved past
  egui's drag threshold, and `interact_pointer_pos()` is then already a
  few pixels from where the button went down. A hit test for grips or
  handles must use `ui.input(|i| i.pointer.press_origin())`, or an edge
  drag reads as "move" (`tool-graphics::crop_overlay`).
- egui lays a frame out twice when a panel's size changes
  (`request_discard`), and `Response::clicked()` is pointer *state*, not
  an event: the second pass sees the same click. A button whose handler
  re-fills the input it then sends (the assistant's quick actions) fires
  twice unless it is deduped by `ui.input(|i| i.time)`, which is the same
  value in both passes. Key events are not repeated; clicks are.
- `Color32` keeps its channels premultiplied by the alpha, so compositing
  is `base * (1 - a) + over`, not `base * (1 - a) + over * a`. Doing the
  latter (`hs_term::widget::blend` before 2026-10-16) dims an overlay
  twice and leaves a selection or a search hit barely visible, and
  `Color32::linear_multiply` dims the colour *and* the alpha, so it is
  the wrong way to make a translucent highlight: set the alpha with
  `from_rgba_unmultiplied` instead.
- A terminal selection is a range of grid points, so it has to be cleared
  on resize: after a reflow those points mean other cells and the
  highlight lands on unrelated text.
- A `Label::truncate()` inside an `egui::Grid` shrinks to whatever width
  the column had last frame (self-fulfilling), so wrap it in a scope with
  `set_min_width`/`set_max_width`. A `TextEdit::desired_width` in a grid
  cell is clamped the same way and never grows, which is why the settings
  tables widen the *cell* (`ui.scope` + `set_min_width`) and not only the
  widget. Pick that width from `ui.available_width()`, never a constant:
  a fixed 340 px made the whole window grow by 168 px per visit and egui
  saved the runaway width with the layout. And `Area` responses report
  `clicked_elsewhere()` for clicks on the area's own buttons; test the
  click against `response.rect` yourself (`hs_ui::varpeek`).
- A GUI test with the PowerShell harness: a crash is invisible unless the
  exe is started from Bash with `2> err.txt` (`RUST_BACKTRACE=1`); the log
  file only carries tracing output, not panics.
- `ui.add_space` inside an `egui::Grid` panics ("add_space makes no sense
  in a grid layout"), in the running app only: allocate an empty row with
  `allocate_exact_size(vec2(1.0, h), Sense::hover())` instead
  (`tool-graphics::Rows::space`).
- A slider's inactive rail uses `visuals.widgets.inactive.bg_fill`, which
  HyperStack's dark theme sets to the panel colour, so a slider at its
  minimum looks like a lone box. Set that fill (and `hovered.bg_fill`) on
  the slider's `Ui` and use `trailing_fill(true)`.
- A canvas click inside a `ScrollArea` is swallowed by the scroll area's
  drag sense: `ui.interact(img_rect, …)` never sees it. Read
  `i.pointer.primary_clicked()` and `interact_pos()` and test the position
  against the rectangle (the Graphics eyedropper does this).
- An `egui::Grid` keeps the **tallest height each row ever had** under
  its id (and the widest column), and eframe persists that memory across
  runs. A wrapped label in a grid cell is laid out at an unknown width on
  its first frame, wraps into five lines, and the row stays five lines
  tall for ever, even after the label is fixed (the Logs detail pane).
  Use `.truncate()` for grid cells, or key the grid by the record it
  shows so a new selection starts fresh.
- `ScrollArea::show_rows` places rows at `row_height + item_spacing.y`;
  a `vertical_scroll_offset` or an at-bottom test computed from the bare
  row height lands rows short of the target (the Logs tail jump).
- A non-wrapping `ui.horizontal` row inside a resizable side panel (seven
  checkboxes, four drag values) is the runaway-width case again: give the
  controls cell `set_max_width` and `horizontal_wrapped`.
- The harness's screenshot can show a *stale* frame: egui repaints on
  input, and a click's consequences (a tab switch, an inserted block) may
  not be painted until the next event. `shot` now nudges the pointer by a
  pixel first; read the state from files or logs when it matters.
- The GUI harness clicks in **window** coordinates (`GetWindowRect`), but
  egui lays out in **client** coordinates: on this window they differ by
  about (8, 31) — the border and the native title bar. A click is
  forgiving about that, a drag is not, and a 31 px error lands on the
  neighbouring row, which looks exactly like "drag-and-drop is broken" and
  then like "the order changes at random". Read the real rectangle out of
  the app (a temporary `tracing::info!(?rect)`) before concluding anything
  about a drag; do not infer coordinates from a cropped screenshot.
- egui's `Ui::dnd_drag_source` covers its content with
  `interact(rect, id, Sense::drag())`. Anything the content drew stops
  receiving clicks and hover, because that interaction is on top: a list
  whose rows are drag sources becomes unclickable. Drag from a grip that
  owns its own small rectangle instead (`hs_ui::reorder`).
- `Stop-Process -Name HyperStack` kills the owner's own installed copy
  too (`C:\Apps\hyper-stack`); stop the dev instance by pid
  (`hs-gui.pid`) and pick it with `Where-Object Path -like "D:\WORKSPACES\*"`.
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
- `ColorImage::region` converts a rectangle of points to pixels with its
  own rounding and then indexes **without checking**, so a rectangle that
  reaches the edge of the capture panics ("range end index ... out of
  range for slice of length ..."). The first thumbnail ever taken hit it.
  Convert and clamp yourself before reading (`app::crop_pixels`); Snip
  shared the same hazard and the same fix.
- The tab strip draws **pinned tabs in a fixed leading zone of row 0**,
  not in the order of the tab vector, so reordering `TabSpace` alone moves
  nothing on screen: a freshly pinned tab still appeared in front of the
  activity's permanent tab. Whatever must lead the strip has to lead that
  zone (`TabLayout::lead` takes the permanent tab first, then the pinned
  ones) - and the model is kept in the same order anyway, because Close to
  the Left/Right and drag-reorder read the vector.
- A tool's `ui` runs **only for a tab that is drawn**, so anything a tool
  must know about its own tab - that it is the activity's home tab, say -
  cannot arrive through `ToolContext`: a tab the user never opens would
  keep a working tab's title for ever (the Devtools home tab read
  "Base64"). `Tool::set_home_tab` is told when the tab is created instead.
  The same applies to `toolbar`, whose `ToolContext` had `is_home: false`
  hard-coded.
- Which tab is an activity's **home tab must be persisted**, not derived
  from the order. Deriving it turned the first saved connection, note or
  shell of each activity into a dashboard on the next start, and that
  tab's pane was simply gone. `LayoutTab::home` carries it; a layout
  written before the flag existed gains a *new* home tab
  (`TabSpace::ensure_home_tabs`) rather than taking one over.
- A process inherits `PATH` from whatever launched it, and on Windows
  that is usually an Explorer started at login, so a tool installed since
  then is invisible to a freshly started app too - "restart it" does not
  help, which is what made jdtls read **Not found** on every start until
  Settings -> Editor's Refresh Status was pressed. `bootstrap` calls
  `hs_core::env_path::refresh()` at startup now. Compare the registry
  value with the process value
  (`[Environment]::GetEnvironmentVariable('Path','User')` vs `$env:Path`)
  before suspecting the lookup itself.
- `ui.menu_button(..)` and `response.context_menu(..)` on the **same
  response share one popup id** (`Popup::default_response_id` is
  `response.id.with("popup")`), so the context menu's closed state shuts
  the menu in the frame it opens: the favourites bar's folder menu
  flashed for one frame (`any_popup_open` true in pass state, false in
  memory) and read as a dead click. Give the second popup its own id
  (`Popup::context_menu(&r).id(r.id.with(..)).show(..)`).
- A `TextEdit` inside a **nested** `menu_button` popup is unusable: the
  first click into the field closes the popup, so the typed text lands
  nowhere (or in whatever field holds focus next, which looked like the
  commit box swallowing a tag message). A tall nested popup was also drawn
  *under* its parent menu. Put fields that need typing in an inline panel
  beside the menu (rename, interactive rebase, New Tag do), or make the
  menu top-level; keep nested menus to buttons.
- The GUI harness's instant down/up click opens no `menu_button` at
  all: a real press spans frames. `Dev-Click` sleeps 80 ms between the
  two `mouse_event`s now; right-click menus never needed it.
- `hs_ui::reorder::drop_onto` **takes the drag payload** to report the
  source, so it must refuse a payload that cannot belong to it *before*
  taking it: a collections row (no scope) released over a collection
  header was swallowed and that list's own reorder was lost.
- **Split panes are gone (2026-10-23) and are not to be reintroduced
  casually.** The owner removed them after they were found broken in a way
  that proved nobody used them: `TabSpace::open` shows a new tab in the
  active pane, replacing the leaf `split_with` was about to divide, so every
  split after the first swapped a pane's tab instead of splitting it. The
  deeper problem is that a split tree spans activities while the strip,
  sidebar and toolbar are per activity. Bringing side-by-side back is a
  design task (which tab fills a pane, dragging a tab into one, a toolbar
  per pane), not a revert. Reasons are recorded in `docs/04-ui-design.md`
  §2.4.
- A tool's `current_uri` is what the shell matches an `OpenRequest::File`
  against, so a **dashboard home tab that reports a uri steals every
  request for it**: the Devtools home tab reported `devtools://base64`
  (its default utility), and clicking Base64 in the sidebar focused the
  dashboard and read as a dead click. A dashboard must report no uri and
  no `uri_affinity`, and `app::open_tool_in` skips dashboards when it
  matches one.
- A nested `egui::Panel::bottom` inside a tool's column sizes itself from
  the *previous* frame, so when its content changes height (a wrapped
  row of quick-action buttons) the central area is painted underneath it
  for a frame that can persist. A bottom-up child `Ui` is no better: a
  `horizontal_wrapped` row allocates one line and then wraps *into* the
  widget below it, and Send ended up clipped (second screenshot, same
  day). What works is to **measure the block first** in a child built
  with `UiBuilder::new().sizing_pass().invisible()` at the column's
  width, give the conversation `full.height - measured`, then draw the
  block for real below it (`hs_ui::ai_assistant::input_block`). Skip
  focus requests and state changes when `ui.is_sizing_pass()`.
- A `"` inside a plain `r"…"` raw string ends it; a regex that needs one
  wants `r#"…"#` (the Logs tool's `path:line` pattern).
- `tracing_subscriber::reload` is not a Cargo feature (it comes with
  `std`); adding `features = ["reload"]` fails the resolve.
- A widget that draws on `available_rect_before_wrap()` without
  **allocating** it reports no size to its container. Inside a central
  panel nothing shows, but a resizable `egui::Panel` stores the size its
  *content* reported and reads it back the next frame, so the panel
  collapses to its minimum and stays there - the terminal did that to the
  editor's new terminal panel and to the bottom dock
  (`hs_term::widget::TerminalView::show` now calls `allocate_rect`).
  Suspect this whenever a resizable panel ignores `default_size`.
- `hs_ui::table::DataTable` has the horizontal-scroll fix now
  (2026-10-24): it computes its own content width and bounds the inner
  `Ui` to it, so S3, SFTP and anything else using it scrolls rather than
  clipping. A new wide table should use `DataTable` rather than a bare
  `TableBuilder`, which still has the fault below.
- `egui_extras::TableBuilder` scrolls **vertically only** — there is a
  `vscroll` but no horizontal equivalent — and its sizing only ever
  divides up the width it is handed, so a table too wide for its pane
  silently squeezes its columns and then clips them, with nothing to
  scroll and no way to reach what is off the edge. Put it in an
  `egui::ScrollArea::horizontal` whose content width you compute yourself
  from the columns' own widths, and set **both** `set_min_width` and
  `set_max_width` on the inner `Ui`: the table reads
  `available_rect_before_wrap`, which is unbounded inside a horizontal
  scroll area, and a trailing `remainder()` column would become
  infinitely wide. `TableBody::widths()` reads the real widths back, which
  is the only way to notice a column the user has dragged wider
  (`tool-db::grid`).
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
- A `Tool` is `Send`, so a per-frame cache on a tool's state must be an
  `Arc`, not an `Rc` (the devtools diff/regex caches, 2026-10-22). And it
  is `ctx.egui_wants_keyboard_input()`, not `wants_keyboard_input()`.
- `gen` is a reserved keyword in edition 2024: a field or variable called
  `gen` fails to parse with "expected identifier, found reserved keyword"
  at every use (18 errors for one field). Name it `generate`.
- Appending functions to the end of a file puts them *after* its
  `#[cfg(test)] mod tests`, which clippy refuses ("items after a test
  module"). Insert new code before the test module; `cat >>` is only safe
  for files without one.
- **`hs_ui::toolbar::hard_right` only works inside a tool toolbar.** It
  draws into a corner the shell reserves and leaves in `ui.data`, so
  calling it anywhere else - a settings form right-aligning its Remove
  button - makes it draw into the corner of whatever toolbar was drawn
  last, and the page it was really in lays itself out around a widget
  that is not there. The symptom was a **card rendered as an empty
  rectangle**: the Project List's frame at full height with no rows in
  it, which read as "the list disappears when I edit a project"
  (owner's report, 2026-10-26). The shell now clears the marker after
  each toolbar (`toolbar::clear_outer`) so a stray call falls back to an
  inline layout, and a form that wants a right-aligned button uses a
  plain `with_layout(right_to_left)`.
- The shell draws tool toolbars with `horizontal_wrapped`; a tool's
  hard-right group must go through `hs_ui::toolbar::hard_right`, never a
  bare `with_layout(right_to_left)`: that scope only gets the width left on
  the current row, so the assistant control was the first thing to vanish
  in a narrow window. The same applies to any `Panel::right` next to tool
  content - tie `size_range`'s upper bound to `available_width()` and give
  way below a floor, or three stacked minimums (props + source + assistant
  = 800 px in the diagram editor) leave the canvas at zero.
- An immediate-mode `ui()` that recomputes derived data every frame hides
  until the data grows: layout_approx on a 500-node diagram, a Myers diff
  of two pasted configs, a regex with a 4 MiB DFA budget. Cache on a hash
  of the inputs (`diagram::fingerprint`, `input_key`) and let idle frames
  reuse it; `cargo test` will not tell you, only a big document will.
- `git diff <base>..HEAD` for "the last N days" needs the right base:
  `8c739c4` was a docs commit three days in, and the first review pass
  reported empty diffs for six crates until the reviewer widened it.

- `horizontal_wrapped` wraps on the size a widget reports *before* it is
  drawn, which an `egui::Frame` cannot do: a row of card frames runs past
  the panel instead of wrapping, and the width of a `Ui` nested in a
  `horizontal` layout is unbounded, so the cards never see a limit either.
  Give the column a real width (`set_width`) and compute the columns
  yourself (`floor((w + spacing) / (card + spacing))`, Settings → AI
  Assistant does). `ui.horizontal` also centres its items vertically, so
  cards of unequal height stagger; use `horizontal_top`.
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
- A **running `devsshd` locks `target/debug/examples/devsshd.exe`**, and
  `cargo test --workspace` builds that example: the whole run dies with
  `LNK1104: cannot open file …devsshd.exe`, which reads like a broken
  toolchain rather than "stop the server you started". Stop it before the
  gate.
- Live SSH test: `hs-ssh` ships an in-process test server behind the
  `testserver` feature and a `devsshd` example (`cargo run -p hs-ssh --features
  testserver --example devsshd -- 2222`, user `dev`/`dev`). Point the SSH tool
  at `127.0.0.1:2222`. WSL's own sshd is NOT reachable from Windows under the
  default NAT networking, so use devsshd for manual UI testing.

### Redis (tool-redis)
- Ubuntu in WSL already has `redis-server`; start it with
  `wsl -d Ubuntu-24.04 -- bash -lc "redis-server --port 6379 --bind 0.0.0.0 --daemonize yes --save '' --appendonly no"`
  and Windows reaches it on `127.0.0.1:6379` through WSL2's localhost
  forwarding (unlike sshd, which the note above says is unreachable). **The
  distro shuts down when the last `wsl.exe` command exits and takes the
  server with it**: the symptom is a connect that fails in a second with
  "timed out" (really ECONNREFUSED underneath). Hold it open with a
  long-running `wsl.exe … sleep` in the background.
- `redis::ConnectionInfo` has private fields: build one with
  `ConnectionAddr::into_connection_info()` plus `set_redis_settings`, and
  read it back with `addr()` / `redis_settings()`. `redis::Value` is
  `#[non_exhaustive]`, so a match on it needs a wildcard arm.
- A `parking_lot` guard is not `Send`: a runner task that holds one across
  an `.await` fails to compile with "future cannot be sent between
  threads". Scope the guard in its own block before the await.

### NoSQL (tool-nosql)
- Docker in WSL serves both stores for the live tests:
  `docker run -d --name hs-mongo -p 27017:27017 mongo:7` and
  `docker run -d --name hs-couch -p 5984:5984 -e COUCHDB_USER=admin -e COUCHDB_PASSWORD=… couchdb:3`.
  Windows reaches them on `127.0.0.1`, and the same WSL caveat as Redis
  applies: the distro must be held open or the containers go with it.
- The extended-JSON conversions (`Bson::try_from(serde_json::Value)`,
  `into_relaxed_extjson`) are behind bson's `serde_json-1` feature, which
  the driver does not enable. Depend on `bson` directly with that feature;
  feature unification then gives the driver's copy the same.
- `serde_json::Value` is a sorted map, so a document's field order is lost
  before any view sees it. Nothing in the tool can restore it without
  `preserve_order`, which would change every other tool's JSON too.

### Databases (tool-db)
- The BEGIN/COMMIT text classifier (`driver::tx_effect`) cannot see a
  block opened by a function, closed by a failed statement or left by
  `ROLLBACK TO`; Postgres and MySQL now ask the server after each
  statement (`probe_in_tx`: `now() <> statement_timestamp()` / 25P02 on
  Postgres, `@@in_transaction` or the performance-schema row on MySQL)
  and keep the classifier only as the fallback.
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
- **ComfyUI's `/object_info` describes a combo input three ways** across
  versions: `[[names], {}]` (classic), `["COMBO", {"options": [names]}]`
  (the V3 schema a 2026 ComfyUI answers with) and a bare
  `{"options": [names]}`. Reading only the first yields an empty list, and
  the caller then reports "ComfyUI has no upscale models" against a server
  that has them - which is what happened to the owner's Z-Image upscale
  graph. `ai_local::combo_options` reads all three; the mock test answers
  in the V3 shape so the old reader cannot come back.
- Its docs host (`docs.1min.ai`) and `chatplayground.ai` do not resolve from
  the cloud sandbox; test against a local mock server that speaks both SSE
  formats (the OpenAI one ends with `data: [DONE]`).
- **Never send a picture to a model that cannot read one, and never send
  a big one.** The Graphics assistant attached a 1024 px PNG at quality
  100, which for a photograph is ~2 MB - 2.8 MB of base64 in the request
  body (measured, `tool-graphics/tests/attachment.rs`) - and
  `Provider::accepts_images` was simply `chats()`, so every chat provider
  got it. A service whose model has no vision adapter may tokenise that
  blob as **text**: ~700 k tokens, which reads as the app hanging
  (owner's report, 2026-10-24, LM Studio running `qwen3-coder`). The
  OpenAI-compatible API has no capability field and no reliable error for
  this, so the model's *name* is the only signal: `ai::VisionSupport`
  (Auto/Yes/No, per provider) with `model_reads_images` as the Auto rule,
  which answers **no** for anything it does not recognise - a missing
  description is a worse answer, a picture to a text-only model was a
  hang. Size follows measurement rather than a guess: PNG while it is
  under 400 KB (a screenshot or diagram, where JPEG would smear the small
  text the model is being asked to read), JPEG above it (a photograph,
  where JPEG is invisible), PNG always when there is transparency.
- Diagnosing "the app hangs" on an assistant: the request is spawned on
  the runtime, so the UI thread is not blocked and the pane is not
  frozen - what is missing is any sign of time passing, because
  `http_client`'s `read_timeout` only fires on a **silent** connection
  and a service slowly grinding through a huge prompt never trips it. Ask
  which providers fail before suspecting the code: two providers sharing
  one request path (DeepSeek and LM Studio are both `openai_body`) where
  only one hangs rules out the body shape and points at the payload.

### Shell (2026-10-15)
- **The first tab of an activity becomes its home tab** (`TabSpace::open`
  asks before pushing), and a dashboard home draws the dashboard
  whatever it was handed - so an `OpenTool` request into an activity the
  user has **never opened** was swallowed in silence: View Logs on a
  container switched to Logs and showed the Logs dashboard, with the
  source gone. `open_tool_in` now lets that first tab be the dashboard
  it wants to be and opens a working tab beside it. Any new cross-tool
  handoff hits this the moment it is tested against a fresh workspace
  rather than one where the target activity happens to be open.
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

- **Upscaling and enhancing are different jobs.** Upscale adds pixels
  through a super-resolution model and leaves the content alone; Enhance
  improves the picture at its own size (face restoration, denoise, a
  low-denoise refiner pass). Every backend here ran an upscaler under the
  name Enhance until 2026-10-23, which is why `ImageJob` now has both and
  each carries its own settings and ComfyUI workflow. Servers also differ
  on how much bigger is asked for: a ratio (Fooocus, the Extras route) or
  a target size (ComfyUI's `ImageScale`), so `UpscaleMode` carries both
  and `ImageWorkflow::upscale_target`/`upscale_ratio` answer either.
- A container-level `#[serde(default)]` fills a missing field from the
  **struct's** `Default`, which is what a settings struct wants. Adding
  `#[serde(default)]` to an individual field silently overrides that with
  the **field type's** default, so a new `f32` read back as `0.0` and a
  new `bool` as `false`: every provider saved before the Enhance settings
  existed came back with a refiner strength of 0, which is a no-op. Put
  the attribute on the container only, and cover a migration with a test
  that parses an old TOML (`an_older_provider_gains_the_new_image_settings`).
- An `f32` dropped into `serde_json::json!` becomes the nearest `f64`
  (`0.7` → `0.699999988079071`), which a strict server may reject and a
  test comparing against `0.7` certainly does. Round at the boundary
  (`ai_local::tidy`, four decimals) for every f32 setting that goes on
  the wire.
- The workspace `reqwest` has no `query` feature either: `.query(&[..])`
  does not exist on its `RequestBuilder`. Build the query string by hand
  with a small percent-encoder (`hs_core::ai_local::urlencode`).
- A Fluent message that must show literal braces (`{{prompt}}`, a JSON
  example) wraps them in a string literal placeable: `{"{{prompt}}"}`,
  `{"{\"a\": 1}"}`; a bare `{` starts a placeable and the message fails to
  parse, which only shows at runtime as the raw key.

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
- `wry::WebView::open_devtools` exists only under `debug_assertions` or
  the `devtools` cargo feature; a debug build, clippy and the tests all
  pass without it and only the CI *release* build fails (run 61). The
  binary enables `features = ["devtools"]`; a local
  `cargo build --release --locked -p hyperstack` is the check that
  catches this class of error before pushing.

### Clipboard files (CF_HDROP)
- A Windows file list on the clipboard is `CF_HDROP`: a 20-byte `DROPFILES`
  header (`pFiles` offset, a POINT, `fNC`, `fWide`) followed by UTF-16 paths,
  each NUL-terminated with another NUL ending the list. It carries *paths*,
  not bytes, so "copy a remote file" means downloading it somewhere real
  first — `hs_core::clipboard_files` + the SFTP pane's Copy do that under
  `HyperStackData/tmp/`, which is cleared at start.
- egui-winit answers Ctrl+V by reading the clipboard as **text** and emits
  `Event::Paste` only if there is some; a clipboard holding files and no
  text produces no event at all. A file paste therefore cannot be bound to
  Ctrl+V — read `CF_HDROP` from a button instead.
- `Set-Clipboard -Path` from a short-lived PowerShell leaves nothing on the
  clipboard (the process owns the data and dies). Set it from a persistent
  session with `[System.Windows.Forms.Clipboard]::SetFileDropList`, and read
  it back with `GetFileDropList` in an `-STA` process.
- A harness click on the point the cursor is **already** on does nothing:
  `SetCursorPos` to the same pixel sends no move, so egui never updates its
  hover state and the press lands on nothing. Clicking the same button
  twice in a row (Open Image… for a second file) silently did nothing for
  three attempts. Click somewhere harmless first, then the target.
- The GUI harness's data folder is `$env:TEMP\hs-gui-data`, and `$env:TEMP`
  for the shell that launches it is **`D:\Temp`**, not
  `C:\Users\...\AppData\Local\Temp`. Both folders exist from different
  sessions, so reading or seeding fixtures in the wrong one looks exactly
  like the app ignoring them (API collections that never appeared). Ask
  the running instance which one it has:
  `(Get-CimInstance Win32_Process -Filter "ProcessId=$pid").CommandLine`.
- Two HyperStack windows at the same screen position (the dev build and the
  owner's installed copy) make the GUI harness capture and click the wrong
  one: `SetForegroundWindow` from a non-foreground process can fail
  silently. Check `GetForegroundWindow`, and `WindowFromPoint` +
  `GetWindowThreadProcessId` against the dev pid, before any click
  (`scratchpad/devclick.ps1`, `devshot.ps1`).
- `SetForegroundWindow` from a non-foreground process fails silently even
  when the HWND is right, and `WScript.Shell.AppActivate` can return `true`
  while another window stays in front: keys and screenshots then go to
  whatever is actually on top (a stray `mailto` dialog opened in an
  unrelated app on 2026-10-19). Force it with `AttachThreadInput` to the
  foreground thread, `ShowWindow(SW_RESTORE)`, `SetWindowPos(HWND_TOPMOST)`,
  `SetForegroundWindow`, then clear topmost. `D:\Temp\focus-hs.ps1` was the
  working pattern; do not send the vault password until a screenshot proves
  the HyperStack window is in front.

- A connection profile's file name is derived from the profile's *name*
  (`ConnectionStore::file_for` slugifies the profile's *name*), so a
  hand-written fixture saved under any other file name is **duplicated**
  the first time anything writes it: the store then holds two profiles
  with the same id and every list shows both. Write fixtures through the
  app, or name the file exactly `slug(name).toml`. Two confusing hours of
  "the reorder wrote the wrong numbers" were this and nothing else.

### SFTP and the browser
- russh-sftp's `write()` opens with `OpenFlags::WRITE` alone, so it fails
  with "no such file" on a name that does not exist yet. `create()` is the
  one that creates (`CREATE | TRUNCATE | WRITE`).
- `Response::request_focus()` called every frame re-grants focus, and the
  field then never reports the `lost_focus()` that an Enter press causes.
  Request it once, when nothing else has focus.
- A WebView2 child holds **native keyboard focus** even when egui believes
  one of its own fields has it. `SetFocus` on the parent and wry's
  `focus_parent()` both hand it over and the child takes it straight back
  on its next message; hiding the view lost a tab. What holds is
  `EnableWindow(child, FALSE)`: a disabled window cannot be focused at
  all. The host does that while the address bar has egui focus and
  re-enables on blur (`Cmd::AddressFocus`, sent every frame because one
  frame's gap is enough for the view to take the keyboard back).

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
- `New-Object Some.Type()` with *empty* parentheses is a PowerShell parse
  error ("An expression was expected after '('"); build the constructor
  without the parentheses when there are no arguments (`hs_core::stt`
  learned this from a stderr line that only said `At line:1 char:163`).
- Synchronous `SpeechRecognitionEngine.Recognize()` cannot be interrupted:
  a kill between "stop speaking" and the result loses the phrase. Use
  `RecognizeAsync(Multiple)` with `Register-ObjectEvent` + `Wait-Event`,
  a `ReadAsync` on stdin as the stop signal, and `RecognizeAsyncStop()`
  before exiting, so the last phrase is printed first (`hs_core::stt`).
- `egui::SelectableLabel` is gone in 0.35; `egui::Button::selectable(on,
  text)` is the replacement, and it takes `add_enabled` like any button.
- A recogniser error leaves its message on stderr in PowerShell's
  multi-line format; keep the first non-empty line for the UI and log the
  rest.
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

### Checking a command's result
- `cargo test ... | grep -E "FAILED" | head -5; echo "(clean)"` prints
  "(clean)" whether or not the grep matched, because the `echo` is a
  separate command with its own exit status. A real failure
  (`launcher::tests::clicking_a_chip_opens_its_connection`, 2026-10-17)
  scrolled past under a reassuring line and was committed on a red
  suite. Let the grep's own output be the answer, or branch on it
  (`if grep -q ...; then`), and never narrate a result the command did
  not actually give. The same thing happened again on 2026-10-18 with
  `clippy ... | grep -E "^error" | head; cargo deny ... && git commit`:
  the lints were printed and the commit went ahead on them. A gate that
  decides a commit must be a command whose **exit code** is checked
  (`cargo clippy ... >/dev/null 2>&1 && git commit`), not one whose
  output is eyeballed.

### Patch scripts
- A Rust string holding a unicode escape (`\u{b7}`, `\u{2192}`) inside a
  Python patch script is a **Python** escape first: `\u` must be followed by
  four hex digits, so the script dies with "truncated \uXXXX escape" before
  it does anything - including a script written to *record* this lesson,
  which failed exactly that way. Put the character itself in the
  replacement text, or build it with `chr()`. Doubling the backslash
  writes a literal `\\u` into the Rust source, which compiles to the
  wrong thing.

- A text-replace script re-run after a partial failure inserted the
  editor's debug toolbar block twice (the first run had applied the early
  steps). The symptom was indirect: the configuration picker closed
  itself, because the second toolbar instance saw the opening click as a
  click outside *its* popup. A "skip when the new text is present" guard
  is not enough when the new text starts with the old one; check the
  inserted marker alone, and `grep -c` the markers after any re-run.

- A **Windows path** is the same trap as `\u{b7}`, and bit three times
  in a row on 2026-10-24: `"C:\\Users\\..."` in a plain Python string
  is read as a truncated `\U` escape and the script dies before it
  runs. Use a raw string (`r"C:\Users\..."`) for every path. The same
  path in **TOML** needs a *literal* string with single quotes
  (`kubeconfig_path = 'C:\Users\...'`): a basic double-quoted TOML
  string processes `\U` too, and an invalid escape fails the whole
  file - which showed up only as `skipping connection ...: TOML parse
  error` in the log and a profile that silently never appeared.

### Patch scripts (continued)
- Re-running a half-applied patch script duplicates whatever its earlier
  steps already did (two `LspStatus` definitions, two toolbar blocks).
  Make each step idempotent by testing for a marker that exists *only*
  after that step, never the first line of the replacement (which usually
  appears in the anchor too), and write each step to disk as it succeeds
  rather than at the end.
- A shell heredoc is the wrong carrier for a script containing quotes and
  backslashes: write the script to a file with the Write tool and run it.
  Two lost patches this session were quoting accidents, not logic errors.
- An **unquoted** heredoc also expands `$name` inside Fluent text:
  `settings-terminal-remove-cli = Remove { $name }?` reached `en.ftl` as
  `Remove {  }?`, which is a parse error that drops the whole message,
  and the only sign was `locale en has 1 syntax errors` in the log, for
  days. `hs_core::i18n::tests::the_bundled_locale_parses_without_junk`
  now fails the gate on any junk entry; write locale edits with the Edit
  tool, never through a heredoc.

### GUI screenshot harness (2026-10-22)
- **`click x y` takes client coordinates; a screenshot is the whole window.**
  `origin` is `ClientToScreen(0,0)` while `shot`/`rawshot` capture
  `GetWindowRect`, so a control seen at (sx, sy) in a screenshot is clicked
  with `click (sx-8) (sy-31)`. Passing the screenshot's own numbers lands a
  row or two lower, which reads as "the dropdown will not open" (it opened a
  drag value instead), and anything below client height (960-39 = 921) throws
  "is not over the dev window".
- The window's minimum is a strict **1720 x 860** since 2026-10-23. Windows
  holds a *user's* drag to it but lets a `SetWindowPos` from another process
  through, so `HyperStackApp::enforce_min_size` reads the size every frame
  and pushes back. A harness that asks for 1400x960 therefore gets the small
  window for a frame and 1720x860 after: every coordinate taken from an older
  screenshot is wrong, and a capture taken in that first frame lies. Shoot
  fresh, after a pause, before clicking anything.
- **`focus-hs` forces the window back to its own size**, and every command but
  `rawshot` calls it. Testing a resize therefore needs the open-and-click in
  **one** harness invocation, and the result read with `GetWindowRect` from a
  separate process - a following `shot` would have undone it before capturing.
- The rail's vertical position depends on the active activity (one or two
  tab rows, a toolbar or none), so a fixed rail y lands on the wrong entry
  after switching tabs; the harness's `rail <k>` finds the Home icon by
  colour in a fresh capture and counts from it. The Settings and Sidebar
  entries sit at the bottom and are clicked directly (`click 38 889`).
- `SendKeys` right after a click arrives before egui has moved focus into
  the field: `click; wait 300; type …` typed into nothing (or into the
  previous field) three times. Use a 500 ms wait after the click.
- The harness splits its command string on `;` and trims each command, so
  a typed SQL script lost its semicolons and typed YAML lost its indent;
  `<sc>`, `<sp>` and `<dl>` (for `$`, which both shells expand) are the
  escapes. A `^{SPACE}` SendKeys token does not exist: Ctrl+Space is `^ `.
- `mouse_event` takes an unsigned delta: a negative wheel must be wrapped
  (`(4294967296 + d) % 4294967296`), **and the unit is `WHEEL_DELTA` =
  120 per notch**. Passing `-10` scrolls by about a pixel, which looks
  exactly like a page pinned by a scroll target and sent me chasing a
  `scroll_to_rect` that was innocent. `hs.ps1 wheel` multiplies by 120
  now, so its argument is notches.
- A WebView2 page holding native focus swallows the first click on the
  rail; click a different rail entry first, or use `rail <k>` twice.
- **A screenshot must verify the foreground window, not only the
  click.** `click` has checked `WindowFromPoint` against the dev pid
  since the two-HyperStack mix-up, but `shot` captured a screen
  *rectangle* with no such check: with the editor window in front, the
  capture was of the editor, and the next three clicks were aimed from
  a picture of the wrong application (2026-10-24). A capture can also
  be of the **right** window in a stale state. Both end the same way -
  a click that lands on a real button nobody meant to press, which
  against a cluster pane means a real pod. `shot` now refuses to
  capture unless the dev window is in front; after any action that can
  change which window is in front, `focus` then `shot`, and re-read the
  fresh capture before aiming anything.
- Verbs in a pane under test are **real writes against real
  infrastructure**. Before clicking at coordinates in a pane that can
  scale, restart or delete, take a fresh capture in the same
  invocation, and prefer the harmless control (a list row) to a verb
  when confirming layout.
- moto keeps its buckets in memory, so a restarted `moto_server` has no
  buckets and the S3 tool shows "NoSuchBucket"; re-run
  `D:\Temp\opencode\seed_s3.py`. The fixture projects under `D:\Temp`
  (`ir-conflict`, `lsp-demo`, `debug-demo`) had been cleaned away too;
  `make_fixtures.py` in the session scratchpad rebuilds them.

### Containers (tool-containers)
- **`wants_dashboard_home` alone creates no home tab.** The shell needs
  `wants_home_tab() == true` *and* `home_is_dashboard() == true` as
  well; with only the third, the activity opened a working tab with no
  item in it and drew its "this has been deleted" empty state on the
  first run. All three, or none of them.
- Draw the dashboard whenever the tab has **no item**, not only when
  `is_home` is set: the flag arrives from the shell a beat later, and
  keying on it alone shows an empty state in a tab that has simply not
  been given anything yet.
- **`title()` is asked every frame and is handed no `Services`**, so an
  `Action::SetTitle` is overwritten the moment it lands. A tab whose
  label comes from a stored profile has to keep the *name* in its own
  saved state, not look it up.
- **A detail block that appears on a click answers that same click.**
  egui lays a frame out twice when a panel's size changes
  (`request_discard`) and `clicked()` is pointer *state*, so the second
  pass sees the click still held - and the detail block, which did not
  exist in the first pass, now has buttons under that pointer. Selecting
  a Compose project ran `config`; the identical path on the container
  list would have reached **Remove**. Both passes share
  `ui.input(|i| i.time)`, so storing it when the selection changes and
  refusing verb clicks while it matches is the exact guard. Any pane
  where picking a row reveals buttons needs it.
- **Output from a verb belongs to what it was run against.** The
  Compose output block survived a selection change, so one project's
  `config` appeared under the next project someone clicked and read as
  that project's own. Clear it when the selection changes.
- A connection profile lives in `connections/<kind>/<slug>.toml`, in a
  **sub-directory per kind** — a fixture written straight into
  `connections/` is silently never loaded, which looks exactly like the
  tool failing to list anything.
- **`docker logs` writes the container's stderr to its own stderr**, and
  a Logs command source reads stdout. Most images log to stderr -
  portainer, nginx, and most Go and Java applications - so a tab that
  follows a live, chatty container shows **0 lines** and looks like a
  broken follower. `follow_logs_line` ends with `2>&1`; the same applies
  to anything else here that pipes a container's output.
- **A container may have no shell at all.** `/bin/sh` is a reasonable
  default and still wrong for a distroless image: portainer answers
  `OCI runtime exec failed: exec: "/bin/sh": stat /bin/sh: no such file
  or directory`. The shell is **Settings → Containers → Shell for Exec**
  rather than a constant, and because the line is typed into a Terminal
  tab the user owns, a wrong answer is visible and editable instead of
  silent.
- A port published on `0.0.0.0` or `::` is reachable on **localhost from
  Windows even when the engine is in WSL** (WSL forwards it), which is
  why `Reachable` is derived from the transport rather than guessed -
  and why a *remote* engine must never be given `localhost`, the one
  wrong answer that may well respond with someone else's service.

### Containers (hs-container)
- **`docker ps --format '{{json .}}'` is line-delimited JSON whose
  `Labels`, `Ports` and `Mounts` are *human strings*, not arrays**:
  `"a=1,b=2"` and `"8000/tcp, 0.0.0.0:9443->9443/tcp"`. `podman ps
  --format json` is a JSON *array* with `Id` not `ID`, `Names` as an
  array, `Labels` as an object and `Ports` as objects. A parser written
  against one engine's output silently returns nothing for the other;
  accept both shapes for each field.
- A label **value** routinely contains `=` and commas of its own (a URL,
  a JSON blob), so split each pair at the **first** `=` only.
- An IPv6 port mapping is `[::]:9443->9443/tcp`: the colons are inside
  the brackets, so the host address cannot be split off at the last
  colon. Respect the bracket or every IPv6 mapping is mangled.
- The engine CLI accepts `-H` and the TLS flags **only before the
  subcommand**, and the WSL prefix goes before the program with a `--`
  after the distro, or `wsl.exe` reads the engine's flags as its own.
  Both mistakes fail in ways that read as something else entirely, which
  is why the argv building is a pure, tested function.
- `--tls` and `--tlsverify` are not the same: without `--tlsverify` the
  certificates are ignored, which looks exactly like TLS working.
- Treat "could not reach a daemon" and "understood and refused" as
  different errors. Returning the empty stdout of a failed command is
  how a tool reports "no containers" for "no permission on the socket".
- `DOCKER_API_VERSION` is an environment variable, not a flag; it cannot
  be smuggled into the argv.
- **Redacting a Kubernetes secret's `data` is not enough.** `kubectl
  apply` leaves `kubectl.kubernetes.io/last-applied-configuration` on
  the object: the whole applied document, values in the clear. The YAML
  view showed `"DB_PASSWORD":"hunter2"` in that annotation while `data`
  above it read `<redacted>` (bench k3s, 2026-10-24). `k8s::render`
  drops the annotation for **every** kind rather than for secrets
  alone - a kind-specific rule only waits for the next kind that
  carries something. Assume any field whose job is to hold a copy of
  the object is a second place the same secret lives.
- **`PatchParams::force()` is only valid with `Patch::Apply`.** Pairing
  it with `Patch::Merge` - which is what `kubectl scale` sends - fails
  *before the request leaves the process*: "Failed to validate request:
  PatchParams::force only works with Patch::Apply". Scale therefore
  uses `PatchParams::default()` with a merge patch on the `scale`
  subresource, which is also the right ownership answer (server-side
  apply would make this app the owner of a field the deployment's own
  manifest sets).
- A `kube::Error` that never reached the network must not be reported
  as "could not reach the cluster": `BuildRequest` and `SerdeError` are
  local faults with local remedies, and `classify` names them as such.
  The default arm of an error mapper is where lies like this hide.
- **`kube`'s `AttachedProcess` owns the connection: keep it alive.**
  Taking `stdin`/`stdout`/`terminal_size`/`take_status` out of it and
  then letting the value drop at the end of the function ends the
  session immediately - the streams answer EOF, the pump stops, and
  the terminal reads "the shell exited with code 0" with nothing in
  it. Move it into the task that pumps it and `join()` it at the end.
  The same shape of bug hides in any handle whose `Drop` cancels work
  you have already taken the pieces of.
- **A status of `None` is not an exit.** `take_status()` on a TTY exec
  can resolve with no `Status` at all; treating that as "finished"
  closed the shell the instant it opened. Only a real `Status` carries
  an exit code; otherwise end-of-output is end-of-session.
- **An empty string can be a real choice, and a flag is the only way to
  say so.** The cluster pane seeds its namespace picker from the
  profile "when the namespace is empty", which made **All namespaces**
  unreachable: choosing it emptied the field, and the re-read that
  followed read empty as "not set yet" and put the profile's namespace
  straight back. `State::namespace_set` now separates "chosen: all"
  from "not chosen yet". Any field whose default is applied on a
  falsy-empty test has this bug waiting in it.
- The `Tunnels` service is **SSH-shaped** - `ForwardSpec` carries an
  `ssh_id` and the implementation is `hs_ssh::tunnels::TunnelManager` -
  so a Kubernetes port-forward does not go through it; routing one
  through would mean inventing an SSH connection. It lives in
  `k8s::forward` instead, and the spec records why.
- rustls refuses to pick a provider when more than one is compiled in,
  and the **first TLS handshake panics** rather than failing: `k8s`
  connects over TLS, so `k8s::connect` installs ring through a `Once`
  exactly as the MySQL driver does. A new crate that opens its own TLS
  connection needs the same call before the first handshake, not after.

### Cross-tool search (hs-core::search)
- **A searcher registered for the life of the app must not capture a
  workspace path.** `<workspace>/api/history.jsonl` is resolved at
  *search* time; capturing it at registration leaves the searcher
  reading the previous workspace's data for ever after a switch, which
  would look like a stale cache and be nearly impossible to spot.
- **"Could not look" is not "found nothing".** `Searcher::search`
  returns a `Result` so the panel can show the difference; an
  implementation that swallows its error into an empty list makes a
  broken store indistinguishable from a clean miss.
- Excluding a **source** and skipping a **searcher** are different
  things: `ask_all(skip)` drops a whole searcher, `Query::exclude` drops
  one of its sources. The asking Logs tab needs the second - the first
  would stop every other Logs tab answering too.
- A process-wide registry makes its own tests order-dependent: they pass
  under `--test-threads=1` and fail in parallel, because one test's
  registration is visible to another's assertions. Serialise them with a
  test-only mutex guard held for the body of each test, rather than
  relying on how the runner is invoked (`tool_logs::searcher`).
- Hold a tab's records **weakly** in any long-lived registry
  (`Weak<Mutex<Shared>>`): a strong handle would keep a closed tab's
  whole record window alive for the rest of the session.

### Logs (tool-logs)
- **A histogram bucket must follow the span, in round units.** A fixed
  per-minute bucket is right for an incident and useless for a week:
  10 080 bars in a 300 px strip is one bar per thirty-three pixels of
  nothing. `chart::Bucket::pick` returns the smallest round unit that
  keeps the count under `TARGET_BARS`; a computed bucket like "37
  seconds" would make the axis unreadable.
- `Response::drag_started()` fires only past egui's drag threshold, so a
  range dragged on a strip must be measured from
  `i.pointer.press_origin()` - otherwise the selection is clipped by
  however far the pointer moved before egui called it a drag. Same
  lesson as the crop overlay in tool-graphics.
- **Filtering to a time range must replace the after/before chips, not
  add to them.** Two overlapping time windows combine with AND and
  filter to nothing, which reads as a broken feature rather than as two
  filters.
- A record is reached again by **line number**, never by filtered
  position: the window drops its oldest records, so a position means a
  different record ten minutes later. `GOTO_MIME` scrolls an open tab to
  a line; an `OpenRequest::File` would reopen the source and lose the
  place.
- `shape::key` had no word boundary to work with between a digit and a
  letter, so `\d+` never matched the `31` in `31ms` and every
  duration was its own shape - which quietly defeated both the repeat
  collapse and the window diff. A measurement with its unit attached is
  normalised now; keep that unit list short, since a long one starts
  eating words.
- **A quoted name in a thread dump is not always a thread header.** The
  deadlock section is full of references - `"worker-8":`, `which is held
  by "worker-7"` - and treating any line starting with `"` as a header
  invented threads that do not exist and tore the deadlock section into
  pieces. `parse::is_thread_header` requires one of the JVM's own
  attributes (`tid=`, `nid=`, `prio=`, `daemon`, `#<id>`).
- A thread dump cannot be detected by proportion the way the other
  formats are: almost every line is a stack frame, so no 60% ratio test
  passes. Detect on **presence** of the VM line, a `tid=`/`nid=` header
  or a `java.lang.Thread.State:` line anywhere in the sample.
- A dump prints a thread's state and its monitors on the lines *below*
  the header, so those fields cannot come from `parse` (it sees one line).
  `parse::absorb_continuation` reads them as the engine folds each
  continuation line in; any future format whose fields arrive after its
  header needs the same hook.
- **A coloured line is an unparsed line.** Terminal escapes do not just
  look wrong in a log viewer: they sit between the timestamp and the
  level, which is where every parser looks, so portainer's lines showed
  `[32mINF` *and* left the Level column empty. `ansi::strip` runs in
  `Shared::push_line` (the one choke point every source passes through)
  and over the sample `pick_format` detects from - a JSON line behind a
  colour code does not start with `{`, so a coloured sample detects as
  plain and every field is lost. Strip, never render, and never guess:
  text containing `[90m` with no escape byte is data.
- Assert "nothing was lost" by **reassembling**: joining every record's
  `raw` with newlines must equal the input file. Counting
  `raw.lines()` instead silently miscounts, because `"a\n".lines()` is 1
  and trailing blank lines vanish - which looked like dropped lines when
  the parser was in fact correct.

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
