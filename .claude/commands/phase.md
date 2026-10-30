---
description: Start, continue or finish a depth-programme phase (A–O) end to end
argument-hint: "<letter> [start|continue|finish]"
---

# /phase

Phase: $ARGUMENTS

Load the `hs-depth-phase` skill and work its steps. Before anything else,
print a short plan from the phase's own row in
[docs/10-depth-programme.md](../../docs/10-depth-programme.md):

1. The phase's scope, verbatim from the table.
2. Its **Builds on** column — the existing clients, runners and `hs_ui` pieces
   to reuse rather than rewrite.
3. The files you expect to change, and the spec in `docs/tools/` that governs
   them.
4. What else could break.
5. What is already done: the ledger items for this phase
   (`grep -n 'Phase:\*\* <letter>' TASKS.md`), the branch
   (`git log --oneline main..HEAD`), the row's state in `CHECKLIST.md`, and
   anything uncommitted.

## The ledger

A phase is several items, not one. Split its scope into `FEAT` (and `CR` where
it changes documented behaviour) items with `/task-new`, each with a real
checklist and an **App** box saying what to click, all under one task version
heading in [TASKS.md](../../TASKS.md) with `**Phase:** <letter>`. Then the
phase is just `/dev-task <that task version>`, and its progress is readable
without asking anyone. The heading is a batch label, not the app's release
number.

## Modes

- `start` — create the branch `feat/depth-<letter>-<slug>`, write or update the
  spec first, file the items, and begin.
- `continue` — pick up from the plan above; re-read the spec before coding.
- `finish` — every item `Completed`, gate green, exercised in the running app
  against a real service where one is on this machine, then spec / README /
  CHANGELOG / CHECKLIST / GOTCHAS, then `hs-ship`.

Phase order is the table's order, not alphabetical. Don't begin a later phase
while an earlier one is open unless I say so. A check that cannot be made on
this machine goes into the item's **Notes** and into `CHECKLIST.md` as not
made — never claimed.
