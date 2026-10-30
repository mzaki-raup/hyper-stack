---
description: Run the full HyperStack gate (fmt, clippy -D warnings, tests -j 4, self-test) and report
argument-hint: "[crate | id | task version] (default: the whole workspace)"
---

# /gate

Scope: $ARGUMENTS (empty means the whole workspace — which is the gate; a crate
name only narrows the test step, and the clippy and fmt steps still run across
the workspace). An id or a task version means: run the gate, then say which items'
**Gate** boxes in `TASKS.md` this run earns — but tick them only if I ask,
and only for items whose other work is actually done.

Use the `hs-gate` skill. Check the forge's `/actions/tasks?limit=4` first — the
CI runner is this machine — and stop a running `HyperStack.exe` before any
release build.

Run the long steps in the background rather than letting them time out.

Report per step: pass/fail, and for a failure the command, the exit code and
the output that proves it. Don't fix anything, don't weaken a check, and don't
edit a test to make it pass. End by stating that the running-app check is not
part of this and still has to be done:

```bash
cargo run -p hyperstack -- --data-dir ./tmp-data
```
