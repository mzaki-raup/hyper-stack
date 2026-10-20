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
gallery in Settings, `hs-core::ai`); Terminal, SSH, SQL, Redis, NoSQL, S3, API client and Graphics
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
`tools/tool-redis`, `tools/tool-nosql`, `hyperstack`) builds to a portable `HyperStack.exe` and passes the full gate
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
- **Right-click copy/paste in text surfaces (PuTTY convention).** In the
  terminal and in every editor/text field we draw ourselves: right-click with
  a selection copies it to the clipboard and clears the selection; right-click
  with no selection pastes the clipboard at the cursor. A per-tool setting may
  turn it off; the default is on. Owner's rule, 2026-10-11.
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
- Adding a **new workspace member** on a branch and then merging a release
  bump from `main` leaves that member at its old version in `Cargo.lock`
  (`update-version.ps1` could not see a crate that only existed on the
  branch). Every `--locked` command then dies with "cannot update the lock
  file"; run a plain `cargo check` after such a merge, commit the one-line
  lock fix, and remember the release job may already have tagged `main`
  before the merge.

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
- A tool's `current_uri` is what the shell matches an `OpenRequest::File`
  against, so a **dashboard home tab that reports a uri steals every
  request for it**: the Devtools home tab reported `devtools://base64`
  (its default utility), and clicking Base64 in the sidebar focused the
  dashboard and read as a dead click. A dashboard must report no uri and
  no `uri_affinity`, and `app::open_tool_in` skips dashboards when it
  matches one.
- A widget that draws on `available_rect_before_wrap()` without
  **allocating** it reports no size to its container. Inside a central
  panel nothing shows, but a resizable `egui::Panel` stores the size its
  *content* reported and reads it back the next frame, so the panel
  collapses to its minimum and stays there - the terminal did that to the
  editor's new terminal panel and to the bottom dock
  (`hs_term::widget::TerminalView::show` now calls `allocate_rect`).
  Suspect this whenever a resizable panel ignores `default_size`.
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
