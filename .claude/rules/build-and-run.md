# Build and run

HyperStack is a portable Rust + egui desktop workspace — a Cargo workspace of
small crates, one per tool. `README.md` has the crate layout;
`docs/03-architecture.md` has the shape.

## Build and run

```bash
cargo build --release -p hyperstack
```

Produces `target/release/HyperStack.exe`.

```bash
cargo run -p hyperstack -- --data-dir ./tmp-data
```

Run against a throwaway data folder. **Always pass `--data-dir` with a scratch
folder while developing**, so the real `HyperStackData/` beside the exe is not
touched.

```bash
cargo run -p hyperstack -- --self-test
```

What the CI smoke step runs.

```bash
cargo test -p tool-s3
```

One tool crate at a time.

## Running the app while developing

- A running `HyperStack.exe` locks its own file, so a release build fails with
  "Access is denied" while an old instance is open. Stop it first:

```bash
Stop-Process -Name HyperStack -Force
```

- Live-server tests need their environment variables (see
  [testing.md](testing.md)). Skip any test whose server you aren't running;
  **don't edit the test to skip it**.

## Live and container-backed tests

```bash
HS_S3_ENDPOINT=http://127.0.0.1:5055 cargo test -p hs-core --test s3_live
```

```bash
HS_PG=127.0.0.1:5432:postgres:pg:postgres HS_MYSQL=127.0.0.1:3306:dev:dev:shop cargo test -p tool-db --test drivers
```

```bash
HS_REDIS=127.0.0.1:6379 cargo test -p tool-redis --test live
```

```bash
HS_MONGO=127.0.0.1:27017 HS_COUCH=http://admin:pw@127.0.0.1:5984 cargo test -p tool-nosql --test live
```

SQLite always runs. Container-backed driver tests run only when
`HS_INTEGRATION` is set.

## Release and packaging scripts

```bash
pwsh -File scripts/next-release.ps1
```

What the next push to `main` would release.

```bash
pwsh -c 'Invoke-Pester scripts/tests'
```

The release-planner tests (Pester 5).

```bash
pwsh -File scripts/build-and-package.ps1 -Version 0.0.0-local -SkipSign
```

A local packaging dry run.

## This machine

- Use `cargo test -j 4`. A full parallel build can exhaust the page file and
  corrupt `.rmeta`/`.rlib` files; if that happens, delete `target/debug` and
  rebuild.
- Watch disk space. Delete `target/debug/incremental` after long sessions of
  repeated builds. Do **not** set `CARGO_PROFILE_DEV_DEBUG=0` on the command
  line.
- Keep `.cargo/config.toml` (`+crt-static`) so the MSVC build stays portable.
- The CI runner is this machine: don't run local cargo commands while a CI job
  is running — check the forge's `/actions/tasks?limit=4` first.

## Assets

The app icon lives in `assets/icon/`. Regenerate it with
`scripts/make-icons.ps1`, never by hand. README screenshots:
`docs/screenshots/README.md`.
