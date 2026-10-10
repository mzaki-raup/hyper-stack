# Changelog

All notable changes to HyperStack are recorded here. The release workflow
prepends a section for every version it publishes, generated from the
Conventional Commit subjects since the previous tag (see
[docs/09-ci-cd-and-updates.md](docs/09-ci-cd-and-updates.md) §6). Do not edit
released sections by hand.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) · Versioning:
[Semantic Versioning](https://semver.org/).

## Unreleased

- Phase 0 foundation: Cargo workspace (`hs-core`, `hs-ui`, `hs-term`, `hyperstack`),
  portable `HyperStackData/` folder resolution with per-user fallback, settings,
  workspaces with layout persistence, connection profiles, environments with
  `{{variable}}` substitution and `.local.toml` overrides, encrypted secret vault
  (Argon2id + XChaCha20-Poly1305) with master password, rebindable shortcuts,
  Fluent i18n, background jobs with cancel, event bus, tracing with secret
  redaction, release-manifest parsing and minisign verification.
- Application shell: multi-row wrapping tab strip with pin/close/reorder/context
  menu, split panes, launcher, command palette and quick-open, environment
  banner and protected-environment frame, activity rail, sidebar, bottom dock
  (Output, Jobs), status bar, toasts, Settings window (General, Updates,
  Appearance, Shortcuts, Environments, Connections, Security, Data, About),
  vault create/unlock/change-password dialogs, idle relock.
- Built-in Welcome and Scratchpad tools; `--version`, `--self-test`,
  `--data-dir`, `--finish-update` flags; Windows icon and version resources.
