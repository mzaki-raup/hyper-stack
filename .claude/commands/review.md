---
description: Review the current change against the project's rules, specs and UI language
argument-hint: "[path, crate, version or 'branch'] [--file] (default: uncommitted + branch diff)"
---

# /review — change review

Target: $ARGUMENTS (empty means the uncommitted changes plus
`git diff main...HEAD`). A version (`v0.115.0`) means: review what the items
under that heading in [TASKS.md](../../TASKS.md) produced, and check the diff
against those items' checklists.

Run the three reviews **in parallel**, as one message with three Agent calls:

1. `rust-reviewer` — correctness, scope, architecture, UI-thread blocking,
   secrets, guards, performance.
2. `ui-auditor` — only if the diff touches `crates/hs-ui/`, a tool's `ui()`,
   `locales/en.ftl`, shortcuts or icons.
3. `spec-guard` — spec agreement and the paperwork (`TASKS.md`, `CHANGELOG`,
   `CHECKLIST`, `README`, the spec, ADRs, `GOTCHAS`).

Give each agent the target, the files in the diff, and the specs and ledger
items that apply. Don't hand them your own conclusions.

Then, in one report:

- **Must fix** — defects, rule breaches, missing guards, missing tests. Each
  with file, line and the concrete failure.
- **Should fix** — reuse, simplification, per-frame cost, consistency.
- **Ledger** — boxes ticked for checks that didn't run, items marked
  `Completed` with unticked boxes, work in the diff with no id.
- **Paperwork** — the exact document edits still owed.
- **Not verified here** — what only the running app can confirm (glyphs,
  double-click paths, narrow-window layout), and any live-server test whose
  server isn't up on this machine.

Report only. Don't apply fixes unless I ask — then re-run the gate afterwards.

## With `--file`

File the findings as ledger items so they can be scheduled: a defect becomes a
`BUG`, a deliberate change to documented behaviour becomes a `CR`, each through
`/task-new` with the finding's file and line in **Notes** and `**From:** review
of <target>`. A pure cleanup with no visible effect gets no id — mention it in
the report and move on. Say which findings you filed and which you didn't.
