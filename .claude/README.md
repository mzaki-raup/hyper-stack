# `.claude/` — agent scaffolding for HyperStack

Everything here is instructions for Claude Code, not application code. The root
[CLAUDE.md](../CLAUDE.md) stays the law; these files break it into pieces that
load only when they are needed, so a session doesn't carry the whole rulebook
for a one-line fix.

The work itself lives in [TASKS.md](../TASKS.md) at the root: `FEAT-`, `BUG-`
and `CR-` items, each under one **task version** heading, each with its own
checklist. `/dev-task v0.7.1` runs everything under that heading;
`/dev-task FEAT-0062` runs one item. A task version is the owner's batch label,
not the app's release number.

## Commands (type these)

| Command | Does |
|---|---|
| [`/dev-task`](commands/dev-task.md) | Run every `TASKS.md` item under a task version, or one item by id |
| [`/task-new`](commands/task-new.md) | File a new `FEAT` / `BUG` / `CR` item with the next free id |
| [`/feat`](commands/feat.md) | Implement a feature item: spec first, branch, reuse, guards, gate, app check, docs |
| [`/bug`](commands/bug.md) | Fix a bug item: prior art first, reproducing test, smallest fix |
| [`/cr`](commands/cr.md) | Work a change request: spec edit first, then the change, then a test that the old behaviour is gone |
| [`/review`](commands/review.md) | Review the current change — three reviewers in parallel; `--file` files findings as items |
| [`/gate`](commands/gate.md) | Run the full gate and report, without fixing anything |
| [`/phase`](commands/phase.md) | Start, continue or finish a lettered depth-programme phase (A–O) |
| [`/docsync`](commands/docsync.md) | Audit, or apply, the ledger and document updates a change owes |
| [`/gotcha`](commands/gotcha.md) | Record a mistake in `docs/GOTCHAS.md` under the right heading |

`CR` means **change request** throughout — the code review is `/review`.

## Skills (Claude loads these itself when the task matches)

| Skill | Covers |
|---|---|
| [`hs-tasks`](skills/hs-tasks/SKILL.md) | The ledger: ids, task versions, states, dependencies, how a batch runs and closes |
| [`hs-gate`](skills/hs-gate/SKILL.md) | The gate on this machine: CI runner clash, locked exe, `-j 4`, corrupted `.rmeta`, self-test, the running-app check |
| [`hs-depth-phase`](skills/hs-depth-phase/SKILL.md) | One phase of `docs/10-depth-programme.md`, branch to release |
| [`hs-new-activity`](skills/hs-new-activity/SKILL.md) | A new tool crate: `Tool`/`ToolFactory`, rail position, dashboard, Settings, connections, strings, shortcuts |
| [`hs-db-driver`](skills/hs-db-driver/SKILL.md) | A `DbSession` or `DocStore` driver: engine enum, type mapping, cancel, introspection, `table_schema`, live tests |
| [`hs-ui-work`](skills/hs-ui-work/SKILL.md) | egui work: `hs_ui` reuse, Fluent, casing, shortcuts, states, per-frame caching |
| [`hs-ship`](skills/hs-ship/SKILL.md) | Commit, push, wait for `ci.yml`, merge into `main`; the release publishes itself |

## Agents (subagents, spawned by the commands or by name)

| Agent | Does | Writes code? |
|---|---|---|
| [`rust-reviewer`](agents/rust-reviewer.md) | Reviews a diff against the project's rules, most severe first | no |
| [`ui-auditor`](agents/ui-auditor.md) | Audits UI code for one-off controls, Fluent, casing, guards, states | no |
| [`spec-guard`](agents/spec-guard.md) | Spec agreement plus `TASKS`/`CHANGELOG`/`CHECKLIST`/`README`/`GOTCHAS`/ADR upkeep | no |
| [`task-auditor`](agents/task-auditor.md) | Hunts unsupported ticks, id drift and version placement in `TASKS.md` | no |
| [`gate-runner`](agents/gate-runner.md) | Runs the gate and reports the failure with its output | no |
| [`gotcha-scout`](agents/gotcha-scout.md) | Finds prior art for a symptom before the fix is written | no |

## Rules (read the one for the area you are touching)

[`rules/00-index.md`](rules/00-index.md) maps them:
[rust](rules/rust.md) · [ui](rules/ui.md) ·
[async-and-jobs](rules/async-and-jobs.md) · [security](rules/security.md) ·
[testing](rules/testing.md) · [docs-and-release](rules/docs-and-release.md) ·
[git](rules/git.md).

## Templates (fill-in forms)

[tool spec](templates/spec-tool.md) · [ADR](templates/adr.md) ·
[bug report](templates/bug-report.md) ·
[feature request](templates/feature-request.md) ·
[phase plan](templates/phase-plan.md).

## The short version

- **The ledger is what is to be done.** `TASKS.md` by id and task version;
  `CHANGELOG.md` is what was done; `CHECKLIST.md` is how far the roadmap got.
  Ids are never reused; commits carry `Refs: <id>`; a box is ticked only after
  that check ran and passed.
- **Specs are requirements.** `docs/tools/*.md` and `docs/01`–`docs/10` are the
  contract. Implementation that forces a design change changes the document in
  the same commit.
- **The gate is the definition of done**: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked -j 4`, `--self-test` on the built exe, plus
  the feature exercised in the running app with a scratch `--data-dir`.
- **Read `docs/GOTCHAS.md`** for your area before fixing a bug, and add an
  entry for every new mistake you find.
- **Branch → push → CI green → merge to `main` → push.** Never merge red; the
  runner is this machine, so don't build locally while a job runs.
- One crate per tool, one `Tool` trait, no tool-to-tool dependency; cross-tool
  work through `Services` / `OpenRequest` / events.
- All I/O on the tokio runtime, bounded channels drained at frame start,
  anything over ~200 ms a Job with progress and cancel, never `block_on` on the
  UI thread.
- Every visible string through Fluent (`locales/en.ftl`), edited with the
  **Edit tool**, never a heredoc.
- All app data under `HyperStackData/` beside the exe; secrets only as
  `secret:<ulid>` in the vault.

## Not included on purpose

No `settings.json` and no hooks: permissions and automation are the owner's
choice, and a committed allowlist would apply to everyone who clones the repo.
