---
name: hs-tasks
description: Read and maintain HyperStack's work ledger TASKS.md — FEAT/BUG/CR ids, task version headings, per-item checklists, states, dependencies, and how a batch runs and closes. Use whenever an item id or a task version is named (FEAT-0012, BUG-0003, CR-0007, "v0.115.0", "what's in the next batch"), when filing a new item, or when ticking a checkbox.
---

# The work ledger

[TASKS.md](../../../TASKS.md) at the repo root says what is to be done.
`CHANGELOG.md` says what was done, `CHECKLIST.md` how far the roadmap phases
have got, `docs/07-roadmap.md` and `docs/10-depth-programme.md` why and in what
order. Don't duplicate one in another.

## A task version is not the app version

A `## ` heading here is the owner's **batch label** — work to be done together.
It is not the application's release number and need not match it. The app's
number comes from the release pipeline reading the Conventional Commit types on
`main`; nothing in `TASKS.md` influences it, and
`[workspace.package] version` is never edited by hand. Never rename a task
version to match a release, and never report a batch as "released" when what
you mean is that its items are done.

## Grammar (the file's own Format section is authoritative)

```
## v0.7.1 — Planned              heading: Planned | In-Progress | Done [<date>]
> **Shipped in:** app v0.108.0   optional, added after the fact
### FEAT-0012 · Title            ^### (FEAT|BUG|CR)-\d{4} · .+
- **State:** Pending …           Pending | In-Progress | Completed | Blocked
- **Depends on:** FEAT-0011
- [ ] item-specific steps
- [ ] Test: …                    the four closing boxes, on every item
- [ ] Gate: …
- [ ] App: …
- [ ] Docs: …
- **Notes:** …
```

The label is any single token the owner chose (`v0.7.1`, `v3`, `sprint-11`,
`2026-11`) — match the file's existing style, don't impose one.

Useful greps:

```bash
grep -nE '^(## |### (FEAT|BUG|CR)-)' TASKS.md
```

```bash
awk '/^## v0\.115\.0/{f=1;next} /^## /{f=0} f' TASKS.md
```

## Allocating an id

Highest existing number per prefix, plus one. Four digits. **Never reuse,
never renumber** — branches, commit footers and `CHANGELOG` lines point at
them.

```bash
grep -oE '^### (FEAT|BUG|CR)-[0-9]{4}' TASKS.md | sort
```

Which prefix: `FEAT` for new visible capability, `BUG` for behaviour that
contradicts its spec or crashes, `CR` for a deliberate change to existing
behaviour (a changed default, a renamed label, a tightened guard, a dropped
option). A review finding is filed as `BUG` if it is a defect and `CR` if it is
a deliberate change; a pure cleanup with no visible effect needs no id.

## States, and what may tick a box

- `Completed` only when **every** box on the item is ticked.
- A box is ticked only after that check ran and passed. Never tick ahead of the
  work, never tick the **App** box from a test run, never tick **Gate** from a
  partial run.
- A check that cannot be made on this machine (the server isn't here) stays
  unticked, with the reason in **Notes**. Say so in the report too.
- `Blocked` always carries its reason in **Notes**.
- Keep the item's `State` line and its boxes consistent — they are read
  together.

## Running one item

Dispatch on the prefix:

| Prefix | Procedure |
|---|---|
| `BUG` | `/bug` — prior art in `docs/GOTCHAS.md` first, reproducing test, smallest fix |
| `CR` | spec edit in the same commit (it changes documented behaviour), then the change, then a test asserting the new behaviour **and** that the old one is gone |
| `FEAT` | `/feat` — spec first where design moves, reuse `hs_ui`/`hs_core`, guards, Job for anything over ~200 ms |

Then the four closing boxes, in order: Test → Gate (`hs-gate`) → App
(`cargo run -p hyperstack -- --data-dir ./tmp-data`) → Docs.

Commit per item, with the id in the footer:

```
fix(tool-db): keep the schema cache across a reconnect

Refs: BUG-0003
```

## The two targets

| Target | Means |
|---|---|
| `/dev-task v0.7.1` | every item under that task version heading |
| `/dev-task FEAT-0062` | that one item, and nothing else |

A single id is not a smaller batch: do that item, don't touch its neighbours,
and don't close the batch around it. Several ids may be named at once, and then
only those run.

## Running a batch

`/dev-task <task version>` is the batch. The order is the file's order, except that
an item whose **Depends on** names another item in the same batch runs after
it. A dependency outside the batch that is not `Completed` blocks the item —
report it, don't silently reorder the release.

One branch for the batch (`batch/v0.7.1`, after the task version), unless
every item belongs to one depth phase, in which case use that phase's branch.
One commit per item, one CI run, one merge. A single-id run gets a branch for
that item (`fix/<slug>`, `feat/<slug>`), not a batch branch.

If an item fails, mark it `Blocked` with the reason and **carry on with the
items that don't depend on it**. Never weaken a test or a gate step to close a
batch; report what is left.

## Closing a batch

1. Every item `Completed`, or moved to another heading with a note saying why.
2. `CHANGELOG.md` has its line per item under **Unreleased**; `CHECKLIST.md`
   rows updated; the specs updated.
3. `hs-ship`: push, `ci.yml` green, merge into `main`, push. The app release
   then publishes itself from `main`, on its own numbering.
4. Set the heading to `## v0.7.1 — Done <YYYY-MM-DD>` once every item is
   `Completed`. That date is when the work finished, not when the app shipped.
5. Afterwards, if it is worth recording which release carried the batch, add
   `> **Shipped in:** app v<number>, <date>` under the heading. Find the number
   from what actually published:

```bash
pwsh -File scripts/next-release.ps1
```

A batch may span two app releases, and one release may carry three batches —
both are normal and neither needs the labels reconciled.
