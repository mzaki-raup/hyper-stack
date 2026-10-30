---
name: hs-ui-work
description: Build or change HyperStack UI with egui — a toolbar, dashboard, form, table, tab strip, tree, dialog, toast or panel — plus its Fluent strings, casing, palette command, shortcut, empty/error/loading states and per-frame caching. Use for any visible change, a new label or tooltip, a layout fix, an icon, a shortcut binding, or an "it looks wrong / recomputes every frame" report.
---

# UI work

Read [.claude/rules/ui.md](../../rules/ui.md) and the "Shell and UI Layout
(egui)" section of [docs/GOTCHAS.md](../../../docs/GOTCHAS.md) before drawing
anything. [docs/04-ui-design.md](../../../docs/04-ui-design.md) is the design
contract.

## Before you write a widget

Look for it in `crates/hs-ui/src/` — `form`, `dashboard`, `toolbar`, `table`
(`DataTable`), `tabs`, `tree`, `palette`, `dialog`, `toast`, `kv_editor`,
`env_banner`, `badge`, `tip`, `varfield`, `ai_assistant`, `theme`, `icons`,
`fileicons`, `header`, `reorder`. One-off controls are the thing this project
most wants to avoid. Never a bare `TableBuilder` for a wide table.

## While you write it

- **Strings**: Fluent only, `locales/en.ftl` first, edited with the **Edit
  tool**. Title Case for buttons, menu and list items, setting names, tabs and
  categories (minor words lowercase). Sentence case for tooltips, hints,
  placeholders, dialog bodies, toasts and empty states. An attached
  parenthetical starts with a capital.
- **Toolbars**: the AI assistant control is the hard-right item through
  `hs_ui::toolbar::hard_right` — never a bare right-to-left layout. Check it
  in a narrow window.
- **Actions**: a palette command and a rebindable shortcut for each. Don't bind
  Ctrl+C/V/X, Ctrl+Shift+V or Shift+Insert — egui-winit turns them into
  clipboard events first, and global shortcuts win over tool-local keys.
- **States**: empty, error and loading for every async action, via the shared
  helpers.
- **Destructive**: a confirmation (typed, for purge/delete), and read-only on
  protected environments.
- **Right-click copy/paste** (PuTTY style) in the terminal and every editor or
  text field we draw: with a selection, copy and clear it; without one, paste at
  the cursor.
- **Per-frame cost**: an immediate-mode `ui()` runs every frame. Cache derived
  data (diffs, layouts, regexes, parses, galley sizes) on a hash of its inputs,
  in an `Arc` — a `Tool` is `Send`, so never `Rc`. Test with a large document.

## Verify by looking

```bash
cargo run -p hyperstack -- --data-dir ./tmp-data
```

- `egui_kittest` snapshots are for widgets; image snapshots are skipped in CI
  (the runner has no GPU).
- Synthetic clicks never form double-clicks — double-click paths must be tried
  in the app.
- A Nerd Font / Phosphor glyph is only verified once you have seen it on
  screen.
- For headless screenshots and clicks, read the "Headless and GUI Testing"
  gotchas first: client-vs-window coordinates (`sx-8, sy-31`), the enforced
  1720 × 860 minimum, and the pause needed before clicking.

## Finish

Gate (`hs-gate`), then `docs/04-ui-design.md` or the tool's spec,
`README.md` (screenshots live in `docs/screenshots/`), `CHANGELOG.md`, and a
`docs/GOTCHAS.md` entry for any layout trap you hit.
