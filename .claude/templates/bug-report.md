# Template — bug report

Hand this to `/bug`, or to `/task-new bug` to file it for a later version
(save it anywhere outside the repo, or paste it). Either way it becomes a
`BUG-dddd` item in [TASKS.md](../../TASKS.md) with its own checklist. The
fields that matter most are the first three: without them the fix is guesswork.

```markdown
## Symptom

What you saw, in the words the app used. Paste the error text, the toast, the
log line (`HyperStackData/logs/`) or the screenshot.

## Steps

1. Started with `cargo run -p hyperstack -- --data-dir ./tmp-data` (or the
   packaged exe, version `--version`).
2. …
3. …

## Expected / actual

Expected: …
Actual: …

## Where

Activity / tool: …
Connection kind and engine, if any: …
Environment protected? yes / no
Workspace: fresh / existing

## Environment

- HyperStack version (`--version` or the title bar):
- OS:
- Server version, for a driver or client bug (Postgres 17, Redis 7.4, …):
- Reproducible from a clean `--data-dir`? yes / no / didn't try

## Anything already known

Does `docs/GOTCHAS.md` mention it? Did it work in an earlier version — which?
```

## What the fix will do with this

`/bug` turns it into a `BUG` item, then a failing test, then the smallest fix,
then the gate and the running-app check, then a `docs/GOTCHAS.md` entry and a
`CHANGELOG.md` `fix:` line, ticking the item's boxes as each actually passes. A
report that cannot be reproduced gets said so rather than guessed at.
