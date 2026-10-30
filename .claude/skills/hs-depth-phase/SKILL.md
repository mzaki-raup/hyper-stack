---
name: hs-depth-phase
description: Execute one phase of HyperStack's depth programme (phases A–O in docs/10-depth-programme.md — Redis depth, SQL depth, Logs, SSH, S3, Devtools, Cassandra, Forge, Automation, Queue, ELK, Tracker, Scratchpad folders) end to end: branch, spec, implement, gate, verify in the app, document, CI, merge. Use when asked to start, continue or finish a lettered phase, or when the task is one of the items in that programme.
---

# Executing a depth-programme phase

The plan and the record are [docs/10-depth-programme.md](../../../docs/10-depth-programme.md).
Each phase is one branch, gated, exercised in the running app, documented and
released from `main` before the next begins. `CHECKLIST.md` carries the state.

## Order

Phases are ordered by value against risk, not alphabetically. The first
tranche — A, B, D, E, F, G, J, M, N, O — needs no new wire protocol. C, H, I, K
and L follow, each new driver behind a `cargo deny` check and a
container-backed test. Don't start a later phase while an earlier one is open
unless the owner says so.

## Steps

1. **Read before coding.** The phase's row in `docs/10-depth-programme.md`, the
   tool spec in `docs/tools/`, the crate and its tests, the callers, and the
   matching section of `docs/GOTCHAS.md`. Then restate the task in one or two
   sentences, list the files you will change and the existing functions you
   will reuse, and name what else could break.
2. **Branch and ledger.** `feat/depth-<letter>-<slug>`, matching the existing
   names (`feat/depth-h-sql-2`). Split the phase's scope into `FEAT` (and `CR`)
   items in [TASKS.md](../../../TASKS.md) with `/task-new`, each with
   `**Phase:** <letter>` and a real checklist, all under one version heading —
   a phase is several items, not one. From then on the phase is
   `/dev-task <that version>`, and its progress is readable without asking.
3. **Spec first where the phase changes design.** A phase that forces a design
   change updates its spec in the same commit. New activities (K, L, M, N) need
   a new `docs/tools/<name>.md` — use
   [templates/spec-tool.md](../../templates/spec-tool.md) — and an ADR in
   `docs/08-decisions.md` when a dependency or a protocol is chosen.
4. **Implement in the phase's "Builds on" column.** That column names the
   existing clients, runners and `hs_ui` pieces to reuse. A phase that adds a
   new activity also needs: a `Tool` impl, a dashboard, Settings page,
   connections with vault credentials, palette commands and shortcuts, Fluent
   strings, and the activity rail position the table gives.
5. **Guards.** Every destructive verb (bulk delete, kill client, delete
   version, purge, cancel build, trigger on production) asks first and honours
   the protected-environment read-only rule. Nothing over ~200 ms on the UI
   thread — make it a Job with progress and cancel.
6. **Credentials.** New activities accept `password`, `token` and `api-key`
   kinds; the form shows only that kind's fields; the secret is always
   `secret:<ulid>`. See [.claude/rules/security.md](../../rules/security.md).
7. **Gate.** Run `/gate` (the `hs-gate` skill). All of it.
8. **Verify in the running app** against a real service where one is on this
   machine (Redis and the SQL engines in WSL/Docker, the SSH test server, moto
   for S3, the fixture repositories), against a mock or fixture where it is
   not. A phase whose service cannot be reached here **says so in
   `CHECKLIST.md`** rather than claiming a check it did not make.
9. **Document.** The items' boxes and **State** in `TASKS.md`, the spec,
   `README.md` entry with screenshots, `CHANGELOG.md` lines under Unreleased,
   the phase's row in `CHECKLIST.md` (`Completed` / `In-Progress` / `Pending`),
   and new `docs/GOTCHAS.md` entries for every mistake found on the way.
10. **Ship.** Push the branch, wait for `ci.yml` green (check
    `/actions/tasks?limit=4`), merge into `main`, push. Never merge red. The
    release publishes itself from `main`.
11. **Summarise** what changed, why, and any side effects or follow-ups.

## Phase done means

- Gate green, feature exercised in the running app.
- Every destructive verb guarded and protected-environment aware.
- Nothing blocking the UI thread.
- Spec, README, CHANGELOG and CHECKLIST updated in the same branch.
