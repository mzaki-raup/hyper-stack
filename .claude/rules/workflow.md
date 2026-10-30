# Working on this project

## Before coding

1. **Understand before changing.** Read the module, its tests, its callers, and
   the relevant spec in `docs/` first.
2. **Check [docs/GOTCHAS.md](../../docs/GOTCHAS.md)** for your area before
   fixing a bug, so you don't repeat a recorded mistake.
3. **Restate** the task in one or two sentences, list the files to change and
   the existing functions to reuse, and note what else could break.
4. **Ask before implementing** when requirements are unclear. State your
   assumptions, and present alternatives instead of picking one silently.

## While coding

- **Smallest change that works.** No speculative features, no "flexibility"
  nobody asked for, no refactors of working code, no dependency upgrades unless
  required.
- **Surgical edits.** Don't touch adjacent code, comments or formatting. Match
  existing style. Remove only the imports or functions your change made
  unused. Mention unrelated dead code; don't delete it.
- **Never restructure code just to pass a test.** Fix the logic. If a test
  seems wrong, explain why and ask before changing it.
- **Never weaken or delete tests** to get green. Report anything you couldn't
  fix.
- **Check side effects.** Run the full gate, not just the affected test. Check
  the other crates and tools that call the changed code.

## After coding

1. Update the docs the change touches: [`TASKS.md`](../../TASKS.md) (the item's
   boxes and state), `README.md`, the affected `docs/*.md` specs,
   `CHECKLIST.md`, and the Unreleased section of `CHANGELOG.md`. A line for
   every add, change, delete or fix — see
   [docs-and-release.md](docs-and-release.md).
2. Verify the feature **in the running app**, not only in tests, before marking
   anything Completed.
3. Work on a branch. Push it, wait for CI to go green, then merge into `main`
   and push. Never merge while CI is red — see [git.md](git.md).
4. Record every new mistake you found and fixed in `docs/GOTCHAS.md`, under the
   matching heading.
5. Summarise what changed, why, and any side effects or follow-ups.

## Phases

- Phases follow the order in [docs/07-roadmap.md](../../docs/07-roadmap.md) and
  [docs/10-depth-programme.md](../../docs/10-depth-programme.md). Follow the
  latest developer notes.
- Track each item as `Completed`, `In-Progress` or `Pending` — those three
  states only.
- Before marking a phase done: all gate checks pass **and** the feature has
  been exercised in the running app (with `--data-dir` on a scratch folder).
  Update `CHECKLIST.md` right after that verification.

## Specs are requirements

- Read the relevant spec in `docs/` before implementing a tool.
- If implementation forces a design change, update that document **in the same
  commit**.

## Editing files safely

These have cost real work before now (`docs/GOTCHAS.md` → "Patch Scripts and
Text Editing"):

- **Locale files go through the Edit tool, never a heredoc.** An unquoted
  heredoc expands backticks and `$name`, which silently mangles Fluent
  messages — the only symptom is "locale en has 1 syntax errors" and a failing
  `the_bundled_locale_parses_without_junk`.
- Prefer the Edit tool generally; if you must script an edit, write the script
  to a file and run it rather than piping a heredoc. Quote heredocs (`<<'EOF'`)
  when you do use them.
- Make each patch step idempotent, and test for a marker that exists only
  *after* that step — re-running a half-applied patch has duplicated whole
  functions.
- **Don't write Rust or Windows paths with escapes inside Python or TOML
  strings.** Use raw strings (`r"C:\Users\..."`) and TOML literal strings
  (single quotes). `"C:\Users"` fails on `\U`, and an invalid TOML escape fails
  the whole file — the symptom was only a "TOML parse error" and a connection
  that never appeared.
- Insert new Rust functions **before** the `#[cfg(test)] mod tests` block;
  never append with `cat >>`.
