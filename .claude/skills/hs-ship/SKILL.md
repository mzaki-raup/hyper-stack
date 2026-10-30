---
name: hs-ship
description: Take finished HyperStack work from a branch to a release — conventional commit, docs and CHANGELOG upkeep, push, wait for ci.yml, merge into main, and let the release publish itself. Use when asked to commit, push, open or merge a branch, check CI, cut or verify a release, or "ship it".
---

# Shipping

The owner asked for this flow every time: **branch → push → CI green → merge
into `main` → push.** Never merge red. Read
[.claude/rules/git.md](../../rules/git.md) and
[docs/09-ci-cd-and-updates.md](../../../docs/09-ci-cd-and-updates.md).

## 1. Before the commit

- The gate is green (`hs-gate`) and the feature was exercised in the running
  app.
- `TASKS.md`: every item in this branch has its boxes ticked for checks that
  actually ran, and its **State** set — `Completed` only when all of them are.
  Anything left open is `Blocked` or `In-Progress` with a reason in **Notes**,
  and gets named in the report rather than quietly shipped.
- Docs updated in the same branch: the affected `docs/*.md` spec, `README.md`,
  one `CHANGELOG.md` line under **Unreleased** per add/change/delete/fix,
  `CHECKLIST.md` (`Completed` / `In-Progress` / `Pending`), and new
  `docs/GOTCHAS.md` entries.
- For a merge commit, this must print nothing:

```bash
grep -nE '^(<<<<<<<|=======|>>>>>>>)' CHANGELOG.md
```

## 2. Commit

Conventional Commits: `feat:`, `fix:`, `perf:`, `security:`, `docs:`, `chore:`,
`refactor:`, `test:`, `ci:`, `style:`, `build:`; `!` or a `BREAKING CHANGE:`
footer for a major bump. One commit per ledger item, with `Refs: <id>` in the
footer. Commit as the repository owner — don't touch the local
git identity. Never `--no-verify`, never `--no-gpg-sign`. Chain on the exit
code of the gate command rather than reading its output.

## 3. Push and wait

```bash
git push -u origin HEAD
```

`ci.yml` runs on branches, not on `main`. The runner is this machine, so **run
no local cargo commands while a job is running** — check the forge's
`/actions/tasks?limit=4` first, and again before merging.

## 4. Merge

Only once CI is green:

```bash
git switch main && git merge --no-ff - && git push origin main
```

(Adjust to the branch's own name; re-check the conflict-marker grep if the
merge touched `CHANGELOG.md`.)

## 5. The release

The task version in `TASKS.md` is the owner's batch label, **not** this
number. Never rename a heading to match a release. Set a heading to
`— Done <YYYY-MM-DD>` when its items are all `Completed`, and — afterwards, if
it is worth recording — add `> **Shipped in:** app v<number>, <date>` under it.

Releases publish automatically from `main` — **don't tag or publish by hand**,
and don't edit the version: it lives once in `[workspace.package] version` in
the root `Cargo.toml` and only `scripts/update-version.ps1` moves it. A
documentation-only push to `main` triggers no release.

Dry runs and verification:

```bash
pwsh -File scripts/next-release.ps1
```

```bash
pwsh -File scripts/build-and-package.ps1 -Version 0.0.0-local -SkipSign
```

```bash
pwsh -File scripts/verify-release.ps1
```

`origin` (Forgejo) holds version authority and its push also updates the public
GitHub mirror; `scripts/push-to-mirror.ps1` and `scripts/sync-from-github.ps1`
handle the mirror — don't improvise around them.

## 6. Report

What changed, why, side effects, follow-ups, and anything you could not verify
on this machine.
