# UI and UX rules

One UI language across every tool. If you are inventing a control, stop and
look for it in `crates/hs-ui/src/` first.

## Shared components (use these, don't re-roll them)

`hs_ui::form`, `hs_ui::dashboard`, `hs_ui::toolbar`, `hs_ui::table::DataTable`,
`hs_ui::tabs`, `hs_ui::tree`, `hs_ui::palette`, `hs_ui::dialog`,
`hs_ui::toast`, `hs_ui::kv_editor`, `hs_ui::env_banner`, `hs_ui::badge`,
`hs_ui::tip`, `hs_ui::varfield`, `hs_ui::ai_assistant`, `hs_ui::theme`,
`hs_ui::icons`.

- Never a bare `TableBuilder` for a wide table — use `DataTable`.
- The AI assistant control is always the hard-right toolbar item, through
  `hs_ui::toolbar::hard_right`. Never a bare right-to-left `with_layout` in a
  toolbar.
- Dashboards and lists share section headings and top spacing. Centre list-card
  icons and labels vertically. Row actions go in a fixed, aligned column.

## Text

- Every visible string goes through Fluent, `locales/en.ftl` first. **Edit
  locale files with the Edit tool, never a heredoc** — an unquoted heredoc eats
  variable placeholders and silently drops the message, and
  `the_bundled_locale_parses_without_junk` then fails the gate.
- Casing:
  - **Title Case** for button labels, menu and list items, setting names, tab
    and category labels. Minor words (a, the, of, to, from, at, in, on, and)
    stay lowercase.
  - **Sentence case** for tooltips, hints, placeholders, dialog bodies, toasts
    and empty states.
  - A parenthetical attached to a label starts with a capital — "None (Direct
    connection)". A parenthetical inside a sentence stays lowercase.

## Interaction

- Every action gets a palette command **and** a rebindable shortcut. Every list
  and tree is keyboard-navigable.
- Never bind Ctrl+V, Ctrl+Shift+V, Shift+Insert, Ctrl+C or Ctrl+X:
  egui-winit turns them into paste/copy/cut events first. Global shortcuts win
  over tool-local keys, so a tool must not handle the same combo.
- Right-click copy/paste (PuTTY style) in the terminal and every editor or text
  field we draw: with a selection, copy and clear it; without one, paste at the
  cursor. On by default; a per-tool setting may turn it off.
- Destructive actions are **guarded, not forbidden** (a confirmation; a typed
  confirmation for purge- and delete-style verbs). Protected environments
  (production) default to read-only in every tool.
- Every async action needs an **empty**, an **error** and a **loading** state,
  through the shared helpers.

## Icons

Verify a Nerd Font / Phosphor glyph by looking at it on screen. A codepoint
being present does not mean it draws the right picture.

## Verifying UI work

Tests are not enough. Run the app against a scratch data folder and look:

```bash
cargo run -p hyperstack -- --data-dir ./tmp-data
```

`egui_kittest` covers widget snapshots, but synthetic clicks never form
double-clicks — exercise double-click paths in the running app. Check a narrow
window for the toolbar hard-right layout. The "Headless and GUI Testing"
section of `docs/GOTCHAS.md` has the click-coordinate offsets and the minimum
window size.
