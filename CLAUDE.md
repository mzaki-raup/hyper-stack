# CLAUDE.md

## Project Basics
- Project: HyperStack, a portable Rust + egui desktop workspace (Cargo workspace; see README.md for the crate layout).
- Build: `cargo build`. Gate (must pass before every commit):
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - `cargo test --workspace --locked`
  - `--self-test` on the built exe
- Use `cargo test -j 4` on the Windows dev machine. A full parallel build can exhaust the page file and corrupt `.rmeta`/`.rlib` files; if that happens, delete `target/debug` and rebuild.
- Watch disk space. Delete `target/debug/incremental` after long sessions of repeated builds. Do not set `CARGO_PROFILE_DEV_DEBUG=0` on the command line.
- Keep `.cargo/config.toml` (`+crt-static`) so the MSVC build stays portable.

## Build & Run
Available once the Cargo workspace exists (Phase 0). Until then, CI detects the missing root `Cargo.toml` and skips building.
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
**Running the app while developing:**
- A running `HyperStack.exe` locks its own file, so a release build fails with "Access is denied" while an old instance is open. Stop it first: `Stop-Process -Name HyperStack -Force`.
- Always pass `--data-dir` with a scratch folder when developing, so your real `HyperStackData/` beside the exe is not touched.
- Live-server tests need the matching environment variables above. Skip any test whose server you aren't running; don't edit the test to skip it.

## Core Rules
1. **Understand before changing.** Read the module, its tests, its callers, and the relevant spec in `docs/` first.
2. **Smallest change that works.** No speculative features, no "flexibility" nobody asked for, no refactors of working code, no dependency upgrades unless required.
3. **Surgical edits.** Don't touch adjacent code, comments, or formatting. Match existing style. Remove only the imports or functions your change made unused. Mention unrelated dead code; don't delete it.
4. **Never restructure code just to pass a test.** Fix the logic. If a test seems wrong, explain why and ask before changing it.
5. **Never weaken or delete tests** to get green. Report anything you couldn't fix.
6. **Check side effects.** Run the full gate, not just the affected test. Check other crates and tools that call the changed code.
7. **Ask before implementing** when requirements are unclear. State assumptions, and present alternatives instead of picking one silently.
8. **Check Gotchas below before fixing a bug**, so you don't repeat a known mistake. Record every new mistake you find and fix there under the matching heading.

## Workflow
**Before coding:** restate the task in one or two sentences, list the files to change and the existing functions to reuse, and note what else could break.

**After coding:**
1. Update the docs the change touches: `README.md` (features, requirements, build commands, restrictions), the affected `docs/*.md` (the specs), `CHECKLIST.md`, and the Unreleased section of `CHANGELOG.md`. Add a line for every add, change, delete, or fix.
2. Verify the feature in the running app, not only in tests, before marking a phase Completed.
3. Work on a branch. Push it, wait for CI to go green, then merge into `main` and push. Never merge while CI is red.
4. Phases follow the order in `docs/07-roadmap.md`. Track each item as Completed, In-Progress, or Pending.
5. Summarize what changed, why, and any side effects or follow-ups.

**Git:**
- Commit as the repository owner. Do not change the local git identity.
- Use Conventional Commits (`feat:`, `fix:`, `perf:`, `security:`, `docs:`, `chore:`, `refactor:`, `test:`, `ci:`, `style:`, `build:`). Add `!` or a `BREAKING CHANGE:` footer for major bumps.
- Don't use `--no-verify` or `--no-gpg-sign`.
- Gate commands must check their exit code, e.g. `cargo clippy ... >/dev/null 2>&1 && git commit`. Don't eyeball output.
- Before committing a merge, run `grep -n '^<<<<<<<\|^=======\|^>>>>>>>' CHANGELOG.md`. It must print nothing.

## Where Things Are
- Scope, personas, non-goals: docs/01-vision-and-scope.md
- Features with priority and effort: docs/02-features.md
- Crate layout, Tool trait, Services, runtime, Eden reference table: docs/03-architecture.md
- Window layout, tabs, launcher, palette, shortcuts, theming: docs/04-ui-design.md
- Data folder, workspace files, vault, history db, importers: docs/05-data-model-and-storage.md
- Threat model, vault format, protected environments: docs/06-security.md
- Phases, milestones, risks: docs/07-roadmap.md (progress in CHECKLIST.md)
- Decisions ADR-001 to ADR-026: docs/08-decisions.md
- CI, releases, asset contract, updater: docs/09-ci-cd-and-updates.md; workflows in .forgejo/workflows/, scripts in scripts/, signing notes in keys/README.md
- One spec per tool: docs/tools/*.md
- App icon: assets/icon/. Regenerate with scripts/make-icons.ps1, never by hand.
- README screenshots: docs/screenshots/README.md

## Specs Are Requirements
- Read the relevant spec in docs/ before implementing a tool.
- If implementation forces a design change, update that document in the same commit.

## Phase Execution
- Implement phases in the order of docs/07-roadmap.md. Follow the latest developer notes.
- Before marking a phase done, all gate checks pass and the feature has been exercised in the running app (use --data-dir with a scratch folder).
- Update CHECKLIST.md right after verification. States are only Completed, In-Progress, or Pending.

## Documentation Upkeep
- CHANGELOG.md: add one line under Unreleased for every add, change, delete, or fix, in code or deployment.
- README.md: update for changes to features, requirements, cross-platform build commands, or restrictions.
- Summary file: none exists. Don't create one unless asked. If one exists, update it when any asset changes.

## Git and CI
- Work on a branch, push it, wait for ci.yml to go green, then merge into main and push. The owner asked for this every time without being asked again.
- Never merge while CI is red.
- ci.yml does not run on main. Releases publish automatically from main; don't tag or publish by hand.
- Documentation-only pushes to main don't trigger a release.
- Don't run local cargo commands while a CI job is running. The runner is this machine. Check /actions/tasks?limit=4 first.

## Project Rules (Non-Negotiable)
- **Portable:** all app data goes under `HyperStackData/` beside the exe. Don't use the registry, `%APPDATA%`, or `%TEMP%`. Never commit `HyperStackData/`.
- **Secrets:** never store them in plaintext. Use the encrypted vault and reference them as `secret:<ulid>`. Redact secrets in logs and history. Any test that writes a secret must grep the data folder afterwards.
- **Architecture:** one crate per tool, one `Tool` trait. Tools depend on `hs-core`/`hs-ui` (and `hs-editor`/`hs-term` where needed), never on each other. Cross-tool features go through `Services`, `OpenRequest`, and events.
- **UI thread never blocks.** All I/O runs on the tokio runtime. Results arrive through bounded channels drained at frame start. Anything over about 200 ms is a Job with progress and cancel. Never call `block_on` on the UI thread.
- **Commands and processes:** internal commands go through `hs_core::paths::background_command` (sets `CREATE_NO_WINDOW`). Never use a bare `std::process::Command::new` for an internal command.
- **Cross-platform:** `hs-core`, `hs-ui`, `hs-editor`, `hs-term`, and every tool must compile on Linux and macOS. Platform code lives only in the modules the spec lists.
- **Eden Explorer** (`../eden-explorer`) is a reference only. Don't copy code or add it as a dependency.
- **Subprocess PATH:** resolve commands through PATHEXT on Windows only. Never call `std::env::set_var("PATH", ...)`.

## UI/UX Consistency
- **One UI language across all tools.** Reuse `hs_ui::form`, `hs_ui::dashboard`, `hs_ui::toolbar`, and `hs_ui::table::DataTable`. Don't invent one-off controls. Don't use bare `TableBuilder` for wide tables.
- **Toolbars:** the AI assistant control is always the hard-right item, via `hs_ui::toolbar::hard_right`. Never use a bare `with_layout(right_to_left)` in a toolbar.
- **Dashboards and lists:** same section headings and top spacing. Centre list-card icons and labels vertically. Put row actions in a fixed, aligned column.
- **Casing:**
  - Title Case for button labels, menu and list items, setting names, and tab or category labels. Minor words (a, the, of, to, from, at, in, on, and) stay lowercase.
  - Sentence case for tooltips, hints, placeholders, dialog bodies, toasts, and empty states.
  - A parenthetical attached to a label starts with a capital ("None (Direct connection)"). A parenthetical inside a sentence stays lowercase.
- **Text:** every visible string goes through Fluent (`locales/en.ftl` first). Edit locale files with the Edit tool, never through a heredoc.
- **Actions:** every action has a palette command and a rebindable shortcut. Every list and tree is keyboard-navigable.
- **Destructive actions** are guarded (confirmation), not forbidden. Protected environments (production) default to read-only in every tool.
- **Right-click copy/paste** (PuTTY style) in the terminal and every editor/text field we draw: with a selection, copy and clear it; without one, paste at the cursor. Default on; a per-tool setting may turn it off.
- **Shortcuts:** don't bind Ctrl+V, Ctrl+Shift+V, Shift+Insert, Ctrl+C, Ctrl+X. egui-winit turns them into paste/copy/cut events first. Global shortcuts win over tool-local keys, so a tool must not handle the same combo.
- **Empty, error, and loading states:** every async action needs all three, using the shared helpers.
- **Icons:** verify a Nerd Font glyph by looking at it on screen. The codepoint being present doesn't mean it shows the right picture.

## Performance
- Cache derived data on a hash of its inputs. Don't recompute diffs, layouts, regexes, or parses every frame in an immediate-mode `ui()`. Test with a large document.
- Don't allocate in hot loops. Pre-size with `Vec::with_capacity` and reuse buffers.
- Prefer iterators over intermediate `Vec`s.
- Share large data with `Arc`, references, or channels, not clones. A `Tool` is `Send`, so per-frame caches must use `Arc`, not `Rc`.
- Measure before optimizing (`criterion`, `cargo flamegraph`).

## Rust Practices
- Return `Result` and propagate with `?`. Avoid `unwrap()`/`expect()` outside tests; if unavoidable, use `expect("invariant")`.
- Avoid `unsafe`, or add a `// SAFETY:` comment.
- Don't ignore warnings. Use a scoped `#[allow(...)]` with a comment, never a crate-wide allow. Don't let lints pile up.
- Edition 2024: `gen` is a reserved keyword; name the identifier `generate`.
- Insert new functions before `#[cfg(test)] mod tests`, not after it. Don't append with `cat >>`.
- Don't write Rust or Windows paths with escapes inside Python or TOML strings. Use raw strings (`r"C:\Users\..."`) and TOML literal strings (single quotes).

## Code Reuse
- Search for existing helpers first (`rg "fn_name"`, shared crates, `hs_ui`, `hs_core`) before writing new code.
- If logic is needed in two or more places, extract a shared function. Don't copy-paste.
- Don't change public signatures without updating all call sites.

## Testing
- Every bug fix starts with a test that reproduces it, then the fix that makes it pass.
- Every new feature gets unit tests. Cross-module behaviour goes in integration tests.
- Use `egui_kittest` for widget snapshots. Synthetic clicks never form double-clicks, so test double-click paths in the running app.
- Any test that writes a secret must grep the data folder afterwards.
- Container-backed driver tests run only when `HS_INTEGRATION` is set.

## Gotchas
Every known mistake and its fix is recorded in `docs/GOTCHAS.md`. Read the section for your area before starting, and add new entries under the matching heading.
