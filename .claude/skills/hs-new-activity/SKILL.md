---
name: hs-new-activity
description: Add a new HyperStack activity (a new tool crate such as tool-queue, tool-automation, tool-tracker) — the crate, the Tool and ToolFactory impl, rail position, dashboard, Settings page, connections and vault credentials, Fluent strings, palette commands and shortcuts, tests and docs. Use when asked to add a new tool, activity or rail entry, or to scaffold a tool crate.
---

# Adding an activity

An activity is one crate under `crates/tools/`, one `Tool` impl, one rail
entry. Read [docs/03-architecture.md](../../../docs/03-architecture.md) and
[docs/04-ui-design.md](../../../docs/04-ui-design.md) first, then write the
spec in `docs/tools/<name>.md` from
[templates/spec-tool.md](../../templates/spec-tool.md) **before** the code —
the spec is the requirement.

Copy the shape of the closest existing tool (`tool-devtools` for a utility
hub, `tool-redis` or `tool-db` for a connection-per-tab tool,
`tool-containers` for a resource browser). Don't invent a new shape.

## 1. The crate

- `crates/tools/tool-<name>/` with `Cargo.toml` using
  `[workspace.dependencies]` entries only — no version numbers of its own.
- Add it to `members` **and** `[workspace.dependencies]` in the root
  `Cargo.toml`.
- Dependencies: `hs-core`, `hs-ui`, and `hs-editor` / `hs-term` only where the
  spec says so. **Never another tool crate** — cross-tool work goes through
  `Services`, `OpenRequest` and events.
- A new wire protocol or heavy dependency needs an ADR in
  `docs/08-decisions.md` and a `cargo deny` pass (`deny.toml`).

## 2. `Tool` and `ToolFactory`

In `crates/hs-core/src/tool.rs`:

- `ToolDescriptor` — a stable `id` (used in layout files and the palette), a
  `title_key` (Fluent), a Phosphor `icon`, a `ToolCategory`, and the
  `accepts` list for drag-and-drop / `OpenRequest`.
- `ToolFactory::create` must restore from the `Option<serde_json::Value>` the
  shell saved, and name the tab before the first frame so a restored tab strip
  reads right.

Register it in `crates/hyperstack/src/tools/mod.rs::factories()` at the rail
position the spec or `docs/10-depth-programme.md` gives. That order is only the
default — the rail is per workspace and user-ordered (ADR-026).

## 3. The UI (see [.claude/rules/ui.md](../../rules/ui.md))

- A **dashboard** home: summaries, Start Here, saved connections, Open Here —
  through `hs_ui::dashboard`, with the same headings and top spacing as the
  others.
- Tabs through `hs_ui::tabs`, tables through `hs_ui::table::DataTable`, forms
  through `hs_ui::form`, confirmations through `hs_ui::dialog`, toasts through
  `hs_ui::toast`.
- Toolbar through `hs_ui::toolbar`, with the AI assistant control as the
  hard-right item via `hs_ui::toolbar::hard_right`.
- `hs_ui::env_banner` for a protected environment, which defaults the whole
  activity to read-only.
- Empty, error and loading states for every async action.

## 4. Connections, credentials, Settings

- Connections through `hs_core::connections`; environments and variable
  substitution through `hs_core::environments`.
- Credentials accept `password`, `token` and `api-key` kinds; the form shows
  only that kind's fields; the secret is always `secret:<ulid>` in the vault.
  Reuse the shape of `hs_core::git_hosts::GitCredential`.
- A Settings page (list left, form right) matching the existing pages.

## 5. Strings, commands, shortcuts

- Every visible string in `locales/en.ftl`, edited with the **Edit tool**,
  never a heredoc. Title Case for labels, sentence case for hints and toasts.
- Every action gets a palette command (`CommandDescriptor`) **and** a
  rebindable shortcut through `hs_core::shortcuts`. Don't bind Ctrl+C/V/X,
  Ctrl+Shift+V or Shift+Insert.
- Every list and tree keyboard-navigable.

## 6. Work and guards

All I/O on the tokio runtime, results through bounded channels drained at frame
start, anything over ~200 ms a Job with progress and cancel, never `block_on`
on the UI thread. Every destructive verb asks first and honours the
protected-environment rule.

## 7. Tests and docs

- Unit tests in the crate, cross-module behaviour as an integration test,
  `egui_kittest` for widget snapshots. Any test writing a secret greps the data
  folder afterwards.
- Run the gate (`hs-gate`), then exercise the activity in the running app with
  `--data-dir ./tmp-data`.
- `docs/tools/<name>.md`, `README.md` (with a screenshot), `CHANGELOG.md`,
  `CHECKLIST.md`, and new `docs/GOTCHAS.md` entries.
