---
description: Bring the ledger and docs up to date with the current change — TASKS, spec, CHANGELOG, CHECKLIST, README, GOTCHAS
argument-hint: "[audit|apply] [task version or id] (default: audit the current change)"
---

# /docsync

Mode: $ARGUMENTS (default `audit` over the current change).

## audit

Spawn `spec-guard` **and** `task-auditor` in parallel on the current change
(`git status` plus `git diff main...HEAD`), then report one table: each
document as `updated`, `needs update` or `not affected`, followed by the exact
edits still owed and every ledger inconsistency found.

## apply

Make those edits, and only those:

1. **`TASKS.md`** — tick the boxes whose checks actually ran and passed; set
   each item's **State** (`Completed` only with every box ticked); put a reason
   in **Notes** for anything left unticked or `Blocked`. Never tick a box to
   make the file look finished, and never invent an id for work that has none —
   file it with `/task-new`.
2. The affected `docs/tools/*.md` spec and any of `docs/01`–`docs/10` the
   change contradicts — a design change belongs in the same commit as the code.
3. `CHANGELOG.md`: one line under **Unreleased** per add, change, delete or
   fix, in the voice of the lines already there, referencing the id where it
   helps.
4. `CHECKLIST.md`: the affected row only. States are exactly `Completed`,
   `In-Progress`, `Pending`. `Completed` only where the gate passed **and** the
   feature was exercised in the running app; a check that could not be made on
   this machine is written down as not made. Update the `Last updated:` line.
5. `README.md`: only if features, requirements, build commands or restrictions
   changed; a screenshot for a UI change (`docs/screenshots/`).
6. `docs/GOTCHAS.md`: an entry under the matching heading for each mistake this
   change ran into or fixed.

Don't touch `[workspace.package] version`. Don't create a summary file. Don't
rewrite prose the change didn't affect.

## With a task version or an id

Audit or apply for just that scope: the items under that heading in
`TASKS.md`, and the documents those items name.
