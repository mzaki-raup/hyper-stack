# Template — phase plan

Write this out before starting a lettered phase of
[docs/10-depth-programme.md](../../docs/10-depth-programme.md), and keep it in
the session (not as a committed file — no summary file exists in this repo and
none should be created unless asked).

```markdown
# Phase <letter> — <name>

**Branch:** feat/depth-<letter>-<slug>
**Spec(s):** docs/tools/<…>.md
**Ledger:** TASKS.md items <ids>, under `## v<x.y.z>`
**Row in docs/10-depth-programme.md:** <paste the Scope cell verbatim>

## Reuse (the row's "Builds on")

- <module / client / hs_ui piece> → what it already does, what it needs added.

## Steps, each with its check

1. <step> → verify: <the test or the thing seen in the app>
2. <step> → verify: <…>
3. <step> → verify: <…>

## Design changes this forces

Which spec sections change, and whether an ADR is needed (new dependency, new
wire protocol, storage format).

## Guards to add

Every destructive verb in this phase and its confirmation; what becomes
read-only on a protected environment.

## Jobs

Which operations exceed ~200 ms, what progress counts, what cancel stops.

## What else could break

Other crates and tools that call the changed code, and the tests that cover
them.

## Verification plan

- Unit / integration tests: …
- Live server on this machine: <which, and the HS_* variable> — or "not
  available here", which goes into CHECKLIST.md as a check not made.
- Running app: <what to click, in `--data-dir ./tmp-data`>

## Paperwork at the end

TASKS items ticked and Completed · spec · README (+ screenshot) ·
CHANGELOG (Unreleased) · CHECKLIST row · GOTCHAS entries
```
