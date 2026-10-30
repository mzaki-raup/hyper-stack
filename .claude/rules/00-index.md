# Rules index

HyperStack is a portable Rust + egui desktop workspace: a Cargo workspace of
small crates, one per tool, shipping as a single static exe that keeps all its
data in `HyperStackData/` beside itself. `README.md` has the crate layout,
`docs/03-architecture.md` the shape.

The root [CLAUDE.md](../../CLAUDE.md) holds the behavioural guidelines that
apply to any project. **These files hold HyperStack's own rules** — read the one
for the area you are about to change.

| File | Read it before you… |
|---|---|
| [workflow.md](workflow.md) | start anything: understand-first, ask-first, the after-coding steps, phases, safe file edits |
| [build-and-run.md](build-and-run.md) | build, run, or test — every command, and this machine's quirks |
| [rust.md](rust.md) | write or change any Rust in the workspace |
| [ui.md](ui.md) | draw anything, add a string, a toolbar, a table, a shortcut |
| [async-and-jobs.md](async-and-jobs.md) | do I/O, spawn a process, add a long action |
| [security.md](security.md) | touch credentials, the vault, logs, environments |
| [testing.md](testing.md) | add or change tests, or claim something works |
| [docs-and-release.md](docs-and-release.md) | finish a change (the ledger, docs, CHANGELOG, CHECKLIST) |
| [git.md](git.md) | commit, branch, merge, or release |

Always also read the matching section of [docs/GOTCHAS.md](../../docs/GOTCHAS.md)
and the tool's spec in [docs/tools/](../../docs/tools/).

## The non-negotiables, in one place

- **Portable:** all app data under `HyperStackData/` beside the exe. Never the
  registry, `%APPDATA%` or `%TEMP%`. Never commit `HyperStackData/`.
- **Secrets:** never plaintext. Encrypted vault, referenced as `secret:<ulid>`.
  Redacted in logs and history. A test that writes a secret greps the data
  folder afterwards.
- **Architecture:** one crate per tool, one `Tool` trait. Tools depend on
  `hs-core`/`hs-ui` (and `hs-editor`/`hs-term` where needed), never on each
  other. Cross-tool features go through `Services`, `OpenRequest` and events.
- **The UI thread never blocks.** All I/O on the tokio runtime; results through
  bounded channels drained at frame start; anything over ~200 ms is a Job with
  progress and cancel. Never `block_on` on the UI thread.
- **Commands and processes:** internal commands go through
  `hs_core::paths::background_command` (sets `CREATE_NO_WINDOW`). Never a bare
  `std::process::Command::new` for an internal command. Resolve through
  `PATHEXT` on Windows only; never set `PATH` from inside the process.
- **Cross-platform:** `hs-core`, `hs-ui`, `hs-editor`, `hs-term` and every tool
  must compile on Linux and macOS. Platform code lives only in the modules the
  spec lists.
- **Specs are requirements.** A design change updates its document in the same
  commit.
- **Eden Explorer** (`../eden-explorer`) is a reference only. Don't copy code
  and don't add it as a dependency.
- **Destructive actions are guarded**, not forbidden. Protected environments
  (production) default to read-only in every tool.
