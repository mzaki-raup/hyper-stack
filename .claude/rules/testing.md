# Testing rules

## Definition of done (the gate)

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

```bash
cargo test --workspace --locked -j 4
```

```bash
cargo deny check
```

Then `--self-test` on the built exe, then the feature exercised in the running
app with a scratch `--data-dir`. All of it, not just the affected test — check
the other crates and tools that call the changed code.

`cargo deny check` is in the gate because CI runs it and its result does not
depend on the branch: advisories come from the RustSec database, so a branch
that was green yesterday can fail today with no change of its own. Run 485
failed that way on `RUSTSEC-2026-0433` (`pulldown-cmark` 0.13.5) after a
clean local gate. The fix is usually `cargo update -p <crate>` for a patch
bump — but check whether the code actually reaches the vulnerable path and
say so either way, rather than implying a vulnerability shipped.

## What to write

- Every bug fix **starts** with a test that reproduces it, then the fix that
  makes it pass.
- Every new feature gets unit tests; cross-module behaviour gets an integration
  test.
- `egui_kittest` for widget snapshots. Synthetic clicks never form
  double-clicks, so test double-click paths in the running app.
- Any test that writes a secret must grep the data folder afterwards.

## What never to do

- Never restructure code just to pass a test. Fix the logic. If a test looks
  wrong, explain why and ask before changing it.
- Never weaken, skip or delete a test to get green. Report what you could not
  fix.
- Never edit a live-server test so it skips — just don't run it when its server
  isn't up.

## Live and container-backed tests

Each needs its environment variable; without it the test skips itself.

```bash
HS_S3_ENDPOINT=http://127.0.0.1:5055 cargo test -p hs-core --test s3_live
```

```bash
HS_PG=127.0.0.1:5432:dev:dev:postgres HS_MYSQL=127.0.0.1:3306:dev:dev:shop cargo test -p tool-db --test drivers
```

```bash
HS_REDIS=127.0.0.1:6379 cargo test -p tool-redis --test live
```

```bash
HS_MONGO=127.0.0.1:27017 HS_COUCH=http://admin:pw@127.0.0.1:5984 cargo test -p tool-nosql --test live
```

Container-backed driver tests run only when `HS_INTEGRATION` is set.

Release-planner tests are Pester 5:

```bash
pwsh -c 'Invoke-Pester scripts/tests'
```

## Don't fight the runner

The CI runner is this machine. Don't run local cargo commands while a CI job is
running — check the forge's `/actions/tasks?limit=4` first.
