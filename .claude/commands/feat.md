---
description: Implement a feature — ledger item, spec first, branch, reuse, guards, gate, app check, docs
argument-hint: "<FEAT-0012 | a feature description | a phase letter>"
---

# /feat — implement a feature

Request: $ARGUMENTS

## 0. The ledger item

- An id (`FEAT-0012`) → load `hs-tasks`, read that block in
  [TASKS.md](../../TASKS.md), and work **its** checklist. The ledger is the
  requirement, not this conversation's paraphrase of it.
- A description → file it first with `/task-new feat <title>`, then work it.
  Nothing gets implemented without an id to tick.
- A lettered phase (A–O) of
  [docs/10-depth-programme.md](../../docs/10-depth-programme.md) → use the
  `hs-depth-phase` skill instead; it is the fuller procedure, and it files its
  items as it goes.

Set the item's **State** to `In-Progress` before you start.

## 1. Read, then restate

The tool's spec in `docs/tools/`, the feature's row in `docs/02-features.md`,
the crate and its tests, the callers, and the matching `docs/GOTCHAS.md`
section. Then restate in one or two sentences, list the files to change and the
existing functions and `hs_ui` pieces to reuse, and name what else could break.

**Stop and ask** if the requirement is unclear or admits two readings. State
assumptions instead of inventing requirements. If a simpler approach exists,
say so.

## 2. Spec before code where design moves

The spec is the requirement. If this feature changes a design, change
`docs/tools/<tool>.md` (or the relevant `docs/0x`) in the same commit; use
[templates/spec-tool.md](../templates/spec-tool.md) for a new tool and add an
ADR to `docs/08-decisions.md` for a new dependency or protocol.

## 3. Branch

The item's **Branch** field, or `feat/<area>-<slug>` — and write it back into
the item. Never implement on `main`.

## 4. Build it

- Reuse before writing: `hs_ui::{form, dashboard, toolbar, table::DataTable,
  tabs, tree, dialog, toast}` and the `hs_core` services. One crate per tool,
  no tool-to-tool dependency — cross-tool work through `Services`,
  `OpenRequest` and events.
- All I/O on the tokio runtime; results through bounded channels drained at
  frame start; anything over ~200 ms a Job with progress and cancel; never
  `block_on` on the UI thread.
- Secrets only as `secret:<ulid>`; destructive verbs guarded; protected
  environments read-only.
- Every visible string in `locales/en.ftl` via the **Edit tool**, Title Case
  for labels and sentence case for hints; a palette command and a rebindable
  shortcut per action; empty, error and loading states.
- Unit tests for the feature, an integration test for cross-module behaviour.

Tick each of the item's steps as it genuinely passes.

## 5. Close the four boxes, in order

1. **Test** — what the item says it asserts.
2. **Gate** — the `hs-gate` skill, in full.
3. **App** — `cargo run -p hyperstack -- --data-dir ./tmp-data`, doing what the
   **App** box says.
4. **Docs** — spec, `README.md` (screenshot for UI), one `CHANGELOG.md` line
   per add/change/delete/fix, the `CHECKLIST.md` row, and `docs/GOTCHAS.md` for
   anything you tripped over.

Never tick a box for a check that didn't run. A check impossible on this
machine stays unticked, with the reason in the item's **Notes**.

## 6. Ship

Commit with the id in the footer (`Refs: FEAT-0012`), Conventional Commits.
Then `hs-ship`: push, wait for `ci.yml` green, merge into `main`, push. Never
merge red. Set the item's **State** to `Completed` only when every box is
ticked.

## 7. Report

What changed, why, which boxes are green, which are not and why, side effects
and follow-ups.
