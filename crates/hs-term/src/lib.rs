//! Terminal support shared by the terminal tool, SSH shells and the dock
//! terminal (docs/tools/terminal.md).
//!
//! - [`session`] — a `Term` from `alacritty_terminal` fed by a transport
//!   (local PTY now; SSH, Docker and kubectl later) on a reader thread.
//! - [`shells`] — shell profile detection (PowerShell 7/5, cmd, Git Bash, WSL,
//!   `$SHELL`).
//! - [`keys`] — egui key events to terminal byte sequences.
//! - [`widget`] — the egui view: rendering, scrolling, selection, input.
//! - [`pty`] — the portable-pty spawn helper (also used by `--self-test`).

pub mod keys;
pub mod pty;
pub mod session;
pub mod shells;
pub mod widget;

pub use alacritty_terminal;
