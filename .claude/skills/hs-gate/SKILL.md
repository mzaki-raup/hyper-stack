---
name: hs-gate
description: Run HyperStack's commit gate correctly on this Windows dev machine — fmt, clippy with -D warnings, the workspace tests at -j 4, the exe self-test, and the running-app check. Use before any commit, when asked to "run the gate", "check it builds", "make sure it's green", or when a build fails with a locked exe, a corrupted .rmeta/.rlib, or a page-file / disk-space error.
---

# Running the gate

The gate is the definition of done. Run **all** of it, not only the crate you
touched: other crates and tools call the changed code.

## 0. Is CI running?

The CI runner is this machine. Check the forge's `/actions/tasks?limit=4`
first; if a job is running, wait. Two cargo builds against one page file is how
`.rmeta`/`.rlib` files get corrupted.

## 1. Is the app running?

A live `HyperStack.exe` locks its own file and a release build fails with
"Access is denied".

```bash
Stop-Process -Name HyperStack -Force
```

## 2. The three checks, exit codes checked

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

```bash
cargo test --workspace --locked -j 4
```

`-j 4` is not optional on this machine. Don't eyeball output — chain on the
exit code when the next step depends on it, e.g.

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings >/dev/null 2>&1 && git commit
```

## 3. Self-test on the built exe

```bash
cargo run -p hyperstack -- --self-test
```

This is what the CI smoke step runs.

## 4. Exercise it in the running app

Tests are not the gate's last step. Launch against a throwaway data folder so
the real `HyperStackData/` beside the exe is untouched:

```bash
cargo run -p hyperstack -- --data-dir ./tmp-data
```

Look at the thing you changed. Double-click paths, narrow-window toolbar
layouts and Nerd Font glyphs can only be checked here — synthetic clicks never
form double-clicks and a present codepoint can still draw the wrong picture.

## Optional, when the change touches them

```bash
pwsh -c 'Invoke-Pester scripts/tests'
```

```bash
pwsh -File scripts/build-and-package.ps1 -Version 0.0.0-local -SkipSign
```

Live-server tests only when that server is up — the variables are in
[.claude/rules/testing.md](../../rules/testing.md). Never edit a test so it
skips.

## When it goes wrong

| Symptom | Fix |
|---|---|
| "Access is denied" linking the exe | an instance is running; stop it |
| corrupt `.rmeta` / `.rlib`, odd metadata errors | delete `target/debug`, rebuild (page file exhausted by a parallel build) |
| disk filling up | delete `target/debug/incremental` |
| "locale en has 1 syntax errors", `the_bundled_locale_parses_without_junk` fails | a Fluent entry was mangled; fix `locales/en.ftl` with the Edit tool |
| clippy fails only on Linux | platform code leaked outside the five allowed modules |

Never set `CARGO_PROFILE_DEV_DEBUG=0` on the command line. Report anything you
could not fix rather than weakening a check.
