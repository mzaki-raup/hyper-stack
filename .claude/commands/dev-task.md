---
description: Run every TASKS.md item under a task version, or one item by id, end to end
argument-hint: "<task version | id | ids | next> [--plan-only]"
---

# /dev-task

Target: $ARGUMENTS

Load the `hs-tasks` skill. [TASKS.md](../../TASKS.md) is the source of truth
for what the work is — don't take it from memory or from this conversation.

## 1. Resolve the target

Two forms, and they are not the same job:

| I typed | You do |
|---|---|
| **a task version** — `v0.7.1`, `v3`, `sprint-11` | every `### (FEAT\|BUG\|CR)-dddd` block under that `## ` heading, as one batch |
| **an id** — `BUG-0003`, `FEAT-0012` | that item alone. Not its neighbours, not the rest of its heading, and no closing of the batch around it |

Also accepted: several ids in one go (`FEAT-0062 BUG-0012`) — then only those
run, in the order I listed them unless **Depends on** says otherwise. And
`next`, meaning the first `## ` task version heading in file order whose status
is not `Done`.

The heading label is **my batch label, not the app's release number** — don't
reconcile it with the version in `Cargo.toml`, and don't rename it.

`## Format` and `## Backlog` are not task versions; `Backlog` is a target only
if I name it. No match, or a heading with no items → say so and stop. Don't
guess a neighbouring version, and don't widen a single id into its batch.

```bash
grep -nE '^(## |### (FEAT|BUG|CR)-)' TASKS.md
```

```bash
awk '/^## v0\.7\.1/{f=1;next} /^## /{f=0} f' TASKS.md
```

Only headings at column 0 are real — the file's Format section indents its
examples precisely so they aren't picked up. If a grep returns an id from
inside a fenced example, your pattern is wrong, not the file.

## 2. Print the plan, before any edit

For a single id, this is three lines — the item, its unticked boxes, and the
branch — then get on with it. For a batch:

A table: id · type · title · state · spec · depends on · the unticked boxes.
Then:

- **Order**: file order, except an item whose **Depends on** names another item
  in the batch runs after it.
- **Blocked before we start**: an item whose dependency is outside the batch
  and not `Completed`, or whose spec doesn't exist yet. Name them; they are not
  attempted.
- **Branch**: `batch/<task version>` for a multi-item batch, the depth phase's
  own branch if every item belongs to one phase, or `fix/<slug>` /
  `feat/<slug>` for a single id. Create it from an up-to-date base; never work
  on `main`.
- Anything in the batch that needs a decision from me — an ambiguous
  requirement, two plausible readings, a missing spec section. Ask now, not
  after the code.

With `--plan-only`, stop here.

## 3. Work the items, one at a time

For each, in the resolved order:

1. Mark its **State** `In-Progress` in `TASKS.md`.
2. Dispatch on the prefix: `BUG` → the `/bug` procedure (prior art in
   `docs/GOTCHAS.md` first, reproducing test before the fix); `CR` → spec edit
   in the same commit, then the change, then a test that asserts the new
   behaviour and the absence of the old; `FEAT` → the `/feat` procedure.
3. Tick each item-specific box as it genuinely passes.
4. Then the four closing boxes in order: **Test**, **Gate** (`hs-gate`, in
   full), **App** (`cargo run -p hyperstack -- --data-dir ./tmp-data`, doing
   what the box says), **Docs**.
5. Commit with the id in the footer (`Refs: FEAT-0012`), Conventional Commits.
6. Set **State** `Completed` only when every box on that item is ticked.

Rules that don't bend: never tick a box for a check that didn't run; never
weaken, skip or delete a test to close an item; never restructure code just to
pass one. A check that can't be made on this machine stays unticked with the
reason in **Notes**.

## 4. When an item fails

Mark it `Blocked`, put the reason in **Notes**, and **continue with the items
that don't depend on it**. One failure doesn't abandon the batch, and it never
justifies closing the batch early.

## 5. Close

- Run the gate once more over everything the run touched.
- `CHANGELOG.md` lines under **Unreleased**, `CHECKLIST.md` rows, specs.
- `hs-ship`: push, wait for `ci.yml` green (check `/actions/tasks?limit=4` —
  the runner is this machine), merge into `main`, push. The app release then
  publishes itself from `main`, numbered by the pipeline — that number has
  nothing to do with the task version, so don't rename the heading to it and
  never edit `[workspace.package] version` by hand.
- For a **batch**: once every item is `Completed`, set the heading to
  `— Done <YYYY-MM-DD>` (the date the work finished). If it is worth recording
  which release carried it, add `> **Shipped in:** app v<number>, <date>`
  underneath, afterwards. A batch left part-done keeps its status and the
  report says what is open.
- For a **single id**: tick and set that item's **State**. Leave the heading's
  status alone — one item finishing does not close its batch.

## 6. Report

Per item: id, what changed, which boxes are green, which are not and why. Then
what is still open under that task version, and anything I need to verify
myself. Say "the batch is done" only when every item is `Completed`; if the app
released during the run, say which release number carried it rather than
implying the task version is one.
