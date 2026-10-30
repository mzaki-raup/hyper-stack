# Documentation and release rules

Specs are requirements. If the implementation forces a design change, change
the document **in the same commit**.

## After every change, update what it touches

1. `TASKS.md` — the item's boxes, ticked only for checks that actually ran, and
   its **State** (`Completed` only when every box is ticked). Work with no item
   gets one filed; see [the ledger](#the-ledger).
2. `CHANGELOG.md` — one line under **Unreleased** for every add, change, delete
   or fix, in code or deployment.
3. `README.md` — when features, requirements, build commands or restrictions
   change.
4. The affected `docs/*.md` and `docs/tools/*.md` spec.
5. `CHECKLIST.md` — only after the work is verified. States are exactly
   `Completed`, `In-Progress`, `Pending`.
6. `docs/GOTCHAS.md` — an entry under the matching heading for every new
   mistake you found and fixed.

No summary file exists. Don't create one unless asked.

## The ledger

[`TASKS.md`](../../TASKS.md) at the root holds the work itself: `FEAT-`,
`BUG-` and `CR-` items, each under one task version heading, each with its own
checklist. It is **what is to be done**; `CHANGELOG.md` is what was done and
`CHECKLIST.md` is how far the roadmap got. Keep all three in step and don't
restate one inside another.

- Ids are allocated once and never reused or renumbered — commits, branches and
  changelog lines point at them. Commit footer: `Refs: FEAT-0012`.
- A box is ticked only after that check ran and passed. One that cannot be made
  on this machine stays unticked with the reason in **Notes**.
- `TASKS.md`'s own Format section is authoritative; the `hs-tasks` skill is the
  working procedure.

## Where things are

| Topic | File |
|---|---|
| Work items by id and task version, with checklists | `TASKS.md` |
| Scope, personas, non-goals | `docs/01-vision-and-scope.md` |
| Features with priority and effort | `docs/02-features.md` |
| Crate layout, `Tool` trait, `Services`, runtime, Eden reference table | `docs/03-architecture.md` |
| Window layout, tabs, launcher, palette, shortcuts, theming | `docs/04-ui-design.md` |
| Data folder, workspace files, vault, history db, importers | `docs/05-data-model-and-storage.md` |
| Threat model, vault format, protected environments | `docs/06-security.md` |
| Phases, milestones, risks | `docs/07-roadmap.md` (progress in `CHECKLIST.md`) |
| Decisions ADR-001 … ADR-026 | `docs/08-decisions.md` |
| CI, releases, asset contract, updater | `docs/09-ci-cd-and-updates.md` |
| The depth programme, phases A–O | `docs/10-depth-programme.md` |
| One spec per tool | `docs/tools/*.md` |
| App icon (regenerate with `scripts/make-icons.ps1`, never by hand) | `assets/icon/` |
| README screenshots | `docs/screenshots/README.md` |

## Release mechanics

- The version lives once, in `[workspace.package] version` in the root
  `Cargo.toml`. Never edit it by hand; `scripts/update-version.ps1` moves it.
- Releases publish automatically from `main`. Don't tag or publish by hand.
- `ci.yml` does not run on `main`. Documentation-only pushes to `main` don't
  trigger a release.
- Dry runs:

```bash
pwsh -File scripts/next-release.ps1
```

```bash
pwsh -File scripts/build-and-package.ps1 -Version 0.0.0-local -SkipSign
```
