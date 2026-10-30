# CLAUDE.md

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.

---

## Project-specific instructions: HyperStack

The guidelines above are general. HyperStack's own rules live in `.claude/`,
split by area so a session loads only what it needs. **Read the ones your task
touches before you start** — they are requirements, not suggestions.

| Where | What |
|---|---|
| [.claude/README.md](.claude/README.md) | How the scaffolding fits together: commands, skills, subagents, rules, templates |
| [.claude/rules/00-index.md](.claude/rules/00-index.md) | The non-negotiables in one page, and which rules file covers which area |
| [TASKS.md](TASKS.md) | The work ledger: `FEAT-`/`BUG-`/`CR-` ids under task versions, each with its checklist |
| [docs/GOTCHAS.md](docs/GOTCHAS.md) | Every known mistake and its fix. Read your area's section **before** fixing a bug; add an entry for every new one |
| [docs/](docs/) | The specs. `docs/tools/*.md` per tool; `docs/01`–`docs/10` for scope, architecture, UI, storage, security, roadmap, decisions, CI, the depth programme |

The four that bite hardest if you skip them:

- **The gate is the definition of done**: `cargo fmt --all --check`, `cargo
  clippy --workspace --all-targets --locked -- -D warnings`, `cargo test
  --workspace --locked -j 4`, `--self-test` on the built exe, **and** the
  feature exercised in the running app with `--data-dir` on a scratch folder.
  See [.claude/rules/testing.md](.claude/rules/testing.md) and
  [.claude/rules/build-and-run.md](.claude/rules/build-and-run.md).
- **The UI thread never blocks**, and secrets never touch plaintext —
  [.claude/rules/async-and-jobs.md](.claude/rules/async-and-jobs.md),
  [.claude/rules/security.md](.claude/rules/security.md).
- **Branch → push → CI green → merge to `main` → push.** Never merge red; the
  CI runner is this machine — [.claude/rules/git.md](.claude/rules/git.md).
- **Specs are requirements**, and a design change updates its document in the
  same commit — [.claude/rules/docs-and-release.md](.claude/rules/docs-and-release.md).
