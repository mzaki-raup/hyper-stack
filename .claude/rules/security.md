# Security rules

Read [docs/06-security.md](../../docs/06-security.md) before changing anything
here.

## Portability

All app data goes under `HyperStackData/` beside the exe. Never the registry,
never `%APPDATA%`, never `%TEMP%`. Never commit `HyperStackData/`.

## Secrets

- Never plaintext. A secret lives in the encrypted vault and is referenced as
  `secret:<ulid>`.
- Redact secrets in logs and in history.
- A vault-locked save must return an error, not silently drop the value.
- Any test that writes a secret must grep the data folder afterwards to prove
  nothing leaked.

## Credentials for the newer activities

Automation, Queue, ELK and Tracker accept three credential kinds beside
username/password: `password`, `token` (bearer / personal access token) and
`api-key` (with the header or query-parameter name the service wants, for
example `X-Api-Key`, `PRIVATE-TOKEN`, `X-Redmine-API-Key`). The secret itself is
always a `secret:<ulid>`. The form shows only the fields that kind needs. Reuse
the shape of `hs_core::git_hosts::GitCredential`.

## Protected environments

Production-flagged environments default to read-only in every tool. When a tool
gains a write or destructive verb, check that guard explicitly — a new action is
not covered by an old one.
