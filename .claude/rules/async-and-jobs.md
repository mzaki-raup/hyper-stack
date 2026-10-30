# Async, jobs and subprocess rules

## The UI thread never blocks

- All I/O runs on the tokio runtime (`hs_core::runtime`).
- Results reach the UI through **bounded** channels drained at frame start.
- Anything over about 200 ms is a **Job** (`hs_core::jobs`) with progress and
  cancel.
- Never call `block_on` on the UI thread.

## Processes

- Internal commands go through `hs_core::paths::background_command`, which sets
  `CREATE_NO_WINDOW`. Never a bare `std::process::Command::new` for an internal
  command.
- Resolve commands through `PATHEXT` on Windows only. Never set the `PATH`
  environment variable from inside the process.
- Subprocess output goes to the logs with redaction applied; see the "Logs and
  subprocesses" section of `docs/GOTCHAS.md`.

## Cancellation and errors

- A Job that talks to a server must be cancellable, and cancel must actually
  stop the work rather than only hide it.
- Surface failures as the tool's error state plus a toast where the user started
  the action; don't swallow them into a log line.
