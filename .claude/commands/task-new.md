---
description: File a new FEAT, BUG or CR item in TASKS.md with the next free id
argument-hint: "<feat|bug|cr> <title> [--version <task version>] [--phase H]"
---

# /task-new

Request: $ARGUMENTS

Load the `hs-tasks` skill. You are writing a ledger entry, not code — don't
start implementing.

## 1. Pick the prefix

`FEAT` new visible capability · `BUG` behaviour that contradicts its spec or
crashes · `CR` a deliberate change to existing behaviour (changed default,
renamed label, tightened guard, dropped option). If the request is really two
items, file two and link them with **Depends on**.

## 2. Allocate the id

Highest existing number for that prefix, plus one, four digits. Never reuse,
never renumber.

```bash
grep -oE '^### (FEAT|BUG|CR)-[0-9]{4}' TASKS.md | sort
```

## 3. Work out what the item actually needs

Read the tool's spec in `docs/tools/` and, for a `BUG`, grep
`docs/GOTCHAS.md` for the symptom first — a known trap changes the steps. If
the request is ambiguous or admits two readings, **ask me** instead of writing
a vague item; a vague item is worse than no item.

## 4. Write the block

Under the `--version` heading, or `## Backlog` if none was given. `--version`
is a **task version** — my batch label, not the app's release number; if the
heading doesn't exist yet, create it as `## <label> — Planned` in the file's
existing style, and never derive the label from `Cargo.toml`. Keep the file's
shape exactly:

```markdown
### FEAT-0012 · <title: what the user can then do>
- **State:** Pending · **Spec:** docs/tools/<tool>.md · **Phase:** <letter, if any>
- **Depends on:** <ids, if any>
- [ ] <concrete step>
- [ ] <concrete step>
- [ ] Test: <what it asserts>
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — <what to click>
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** <who asked, what it replaces, what is out of scope>
```

The steps must be things someone can do and check, not restatements of the
title. The **App** box says what to click. For a `BUG`, the first step is the
reproducing test. For a `CR`, the first step is the spec edit.

## 5. Report

The id, where you filed it, and one line on anything you had to assume. Don't
touch `CHANGELOG.md` or `CHECKLIST.md` — those move when the work lands.
