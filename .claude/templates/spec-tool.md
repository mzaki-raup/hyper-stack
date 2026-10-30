# Template — tool spec (`docs/tools/<name>.md`)

Copy this into `docs/tools/<name>.md`, fill it in, and commit it **before** the
code. The spec is the requirement: if the implementation later forces a design
change, change this file in the same commit.

Keep the numbered-section shape the existing specs use (`redis.md`,
`devtools.md`, `database.md`) so they stay comparable. Add a dated addendum
section for later depth work rather than rewriting history.

---

```markdown
# Tool spec — <Display Name>

**Status:** draft · **Last updated:** <YYYY-MM-DD>

## 1. Purpose

What this tool is for, in two or three sentences, and what it is explicitly not
for (point at `docs/01-vision-and-scope.md` non-goals where relevant).

## 2. User stories

- As a <persona from docs/01>, I want <…> so that <…>.

## 3. Layout

The dashboard home (summaries, Start Here, saved connections, Open Here), the
tab layout, which panes, which `hs_ui` components. Name them:
`hs_ui::dashboard`, `hs_ui::table::DataTable`, `hs_ui::form`, `hs_ui::tree`,
`hs_ui::toolbar` (AI assistant hard-right), `hs_ui::env_banner`.

## 4. <Domain sections>

One per area of the tool — the tree, the editor, the viewers, the console. What
each shows, what each can do, and what it refuses to do.

## 5. Connections and credentials

Connection kinds and their form fields. Credential kinds: `password`, `token`,
`api-key` (with the header or query-parameter name) — the form shows only that
kind's fields, the secret is always `secret:<ulid>` in the vault.

## 6. Guards

Every destructive verb and how it is guarded (confirmation, typed
confirmation). What becomes read-only on a protected environment.

## 7. Work and progress

Which operations are Jobs (anything over ~200 ms) with progress and cancel.
What is streamed, what is paged, what the bounds are.

## 8. Implementation notes

Crates and modules it reuses (`hs_core::…`, `hs_ui::…`), the files it owns,
cross-tool integration through `Services` / `OpenRequest` / events, and the
dependencies it adds (with the ADR number, if a decision was needed).

## 9. Keyboard

Palette command ids and the default shortcut for each action. Nothing on
Ctrl+C/V/X, Ctrl+Shift+V or Shift+Insert.

## 10. Open questions

Numbered, so they can be answered in place later.

## What is implemented (<YYYY-MM-DD>)

The honest state: what works, what is stubbed, what was verified in the running
app and what could not be verified on this machine.
```
