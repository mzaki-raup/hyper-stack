---
description: Fix a bug the project's way — prior art, reproducing test, fix, gate, app check, GOTCHAS entry
argument-hint: "<BUG-0003 | a symptom | a path to a bug report>"
---

# /bug — fix a bug

Symptom: $ARGUMENTS

Work in this order and don't skip a step.

## 0. The ledger item

An id (`BUG-0003`) → load `hs-tasks` and work that block's checklist in
[TASKS.md](../../TASKS.md). A raw symptom → file it with
`/task-new bug <symptom>` first, so the fix has an id to tick and the commit
has something to reference. Set **State** to `In-Progress`.

## 1. Prior art (before any edit)

Spawn `gotcha-scout` with the symptom. While it runs, read the owning module,
its tests and its callers. Nothing gets fixed before you know whether
`docs/GOTCHAS.md` already records this trap — repeating a recorded mistake is
the one failure mode this project most wants to avoid.

## 2. Restate

One or two sentences: what is actually wrong, which module owns it, which files
you will change, which existing functions you will reuse, and what else could
break. If two readings of the symptom lead to different fixes, say so and ask
rather than picking silently.

## 3. Reproducing test first

Write the test that fails for this bug, in the crate that owns the behaviour
(an integration test if it crosses modules). Show it failing. Never restructure
code to make a test pass, and never weaken or delete a test to get green — if a
test looks wrong, explain why and ask.

## 4. Fix

The smallest change that makes the test pass. Surgical: no adjacent cleanup, no
refactor of working code. Remove only what your change orphaned.

## 5. Gate

The `hs-gate` skill in full — fmt, clippy `-D warnings`, `cargo test
--workspace --locked -j 4`, `--self-test` — plus any live test whose server is
up, plus the other crates that call the changed code.

## 6. Verify in the app

```bash
cargo run -p hyperstack -- --data-dir ./tmp-data
```

Reproduce the original symptom and confirm it's gone. This is the item's **App**
box; a passing test does not tick it.

## 7. Paperwork

- `docs/GOTCHAS.md`: an entry under the matching heading — the mistake, the
  symptom it showed as, and the fix.
- `CHANGELOG.md`: a `fix:` line under Unreleased.
- The spec, `README.md` and `CHECKLIST.md` if the behaviour they describe
  changed.
- Tick the item's boxes; `Completed` only when all of them are.

## 8. Ship and report

Commit with `Refs: BUG-0003` in the footer, then `hs-ship`. Report what was
wrong, why it happened, what the test now asserts, side effects, and anything
you could not verify here.
