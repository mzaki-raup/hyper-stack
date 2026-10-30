# Template — feature request

Hand this to `/feat` to build it now, or to `/task-new feat --version v0.115.0`
to schedule it; either way it becomes a `FEAT-dddd` item in
[TASKS.md](../../TASKS.md). Unclear requirements get questions back before
code, not after, so the more of this that is filled in, the less there is to
ask. The **Behaviour** and **Done when** sections below become the item's
checklist, so write them as things someone can check.

```markdown
## What

One or two sentences: the capability, in terms of what the user can then do.

## Where

Activity / tool (or "new activity"): …
Which screen: dashboard · a tab · a pane · Settings · the palette only

## Why

The task it unblocks. If this came from the depth programme, the phase letter
from `docs/10-depth-programme.md`.

## Behaviour

- Normal case: …
- Empty case: what the screen says with nothing to show.
- Error case: what the user sees when the server says no.
- Long case: is this over ~200 ms? Then it is a Job — what does progress count,
  and what does cancel stop?

## Data and credentials

New connection kind? Fields? Credential kinds (`password` / `token` /
`api-key`, with the header name)? Anything stored — and it goes under
`HyperStackData/`, secrets as `secret:<ulid>`.

## Destructive verbs

Anything that writes, deletes, purges, kills or triggers. Each needs a guard,
and read-only on a protected environment.

## Out of scope

What this explicitly does not include, so it doesn't grow.

## Done when

The checks that would convince you: a test asserting …, and this seen working
in the running app against <which server>.
```

## What `/feat` adds on its own

The ledger item and its four closing boxes, a spec update in the same commit,
reuse of `hs_ui` and `hs_core` rather than new controls, Fluent strings with
the right casing, a palette command and a rebindable shortcut per action, the
gate, the running-app check, and the `CHANGELOG` / `CHECKLIST` / `README`
lines.
