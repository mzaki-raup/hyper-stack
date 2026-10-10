# 06 — Security

**Status:** draft · **Last updated:** 2026-10-10

HyperStack holds the keys to databases, servers, buckets and APIs, often
production ones. Security is a product feature, not a checklist.

## 1. Threat model

| Threat | In scope? | Mitigation |
|---|---|---|
| Secrets leaking through files on disk (USB sticks, sync folders, repos, backups) | Yes | Secrets only in the encrypted vault (or an opt-in OS keyring); workspace files contain `secret:<id>` references only; export never includes secrets; a test greps `data/` after an e2e run. A portable `data/` folder is expected to travel, so the vault is designed to be safe when copied |
| Lost or stolen portable drive | Yes | Vault is useless without the master password (Argon2id, memory-hard); history and caches can be encrypted too (setting "encrypt local history", default on when portable) |
| Secrets leaking through logs, history, crash reports | Yes | Redaction layer in `tracing`; history stores resolved URLs/SQL with secret values replaced by `***`; no crash reporting upload |
| Shoulder surfing / screen sharing | Yes | Masked by default everywhere; reveal is per-click and auto-hides after 10 s; clipboard copy of a secret clears after 30 s (configurable) |
| Running a destructive action against the wrong environment | Yes | Environment banner, protected environments, destructive-statement guards, read-only connections |
| Man-in-the-middle on SSH / TLS | Yes | Host key verification with known_hosts and a first-connect fingerprint dialog; TLS via rustls with system roots; `verify=false` is a per-connection, visibly flagged setting |
| Malicious workspace folder (cloned repo with `.hyperstack/`) | Yes | Opening a workspace never runs anything; scripts (Rhai) run only when the user runs a request, in a sandbox with no filesystem/network access beyond the request itself; connections from a new workspace are shown in a review list before first use |
| Malicious server responses (terminal escape sequences, huge payloads) | Yes | Terminal parser is alacritty's (battle-tested); response bodies capped (default 50 MB) and rendered as text, never executed; JSON/HTML preview is a sanitised renderer, not a browser |
| Supply chain (crates) | Partly | `cargo deny` for licences and advisories in CI; `cargo vet` or audited lockfile review for anything touching crypto/network; release builds reproducible from tagged source |
| Compromised machine | No | Out of scope: if the OS user is compromised, the keyring is too |

## 2. Secret storage

`hs-core::secrets::SecretStore` trait with two implementations:

1. **Encrypted vault file** (default, portable): `data/vault.hsv`. Format:
   header (magic, version, Argon2id parameters, salt), then entries encrypted
   as one XChaCha20-Poly1305 blob keyed by `<workspace-id>/<secret-id>`; the
   key is derived from a master password with Argon2id (64 MiB, 3 iterations,
   tuned at first run to ~500 ms). Writes are atomic (temp + rename) and keep
   one `.bak`. Unlocked once per session; relock on idle timeout (default
   15 min) or on demand (Ctrl+Shift+L). Optional convenience on Windows:
   remember the master password for this machine with DPAPI (`CryptProtectData`,
   current-user scope) so the prompt is skipped on the user's own PC; the
   DPAPI blob is useless on another machine, which is exactly the point.
   Master password change re-encrypts in place; forgetting it loses the
   secrets, and the UI says so plainly when it is set.
2. **OS keyring** (opt-in for installed, single-machine use): Windows
   Credential Manager via the `windows` crate, macOS Keychain and Linux Secret
   Service via `keyring`. Entry name `hyperstack/<workspace-id>/<secret-id>`.
   Switching stores migrates entries and wipes the old location.

Rules:

- Secret values live in memory as `secrecy::SecretString` and are zeroised on
  drop.
- Tools receive a `SecretRef` and ask the store at the moment of use; the UI
  never holds the plaintext except inside the reveal widget.
- Changing a secret updates every connection that references it (they only hold
  the ref).
- Deleting a workspace offers to delete its secrets.

## 3. Environment protection

An environment can be `protected = true` (defaults: anything named `prod*`,
`production`, `live` gets a one-time suggestion). Effects:

| Tool | Effect in protected environment |
|---|---|
| Database | connection opens read-only by default; toggling write asks for confirmation with the environment name; UPDATE/DELETE without WHERE, DROP, TRUNCATE, ALTER are blocked unless the user types the table name |
| API client | non-GET requests show a confirm dialog naming environment and URL; collection runner refuses unless `--allow-protected` toggle set for the run |
| SSH / SFTP | delete and overwrite confirmations name the host; `rm -rf` typed in the shell is not intercepted (shell is the shell) but a red banner stays visible |
| S3 | delete of more than 10 objects or any prefix requires typing the bucket name; versioning status shown before delete |
| Git | `push --force`, `branch -D`, `reset --hard` disabled on protected remotes (remote pattern configurable) |
| Containers | stop/remove/prune require confirmation naming the context |

The whole main area gets a 2 px frame in the environment colour, and the title
bar shows a lock icon.

## 4. Destructive SQL classifier

A small parser (not a full SQL grammar) in `tool-db` classifies each statement:
`Read`, `Write`, `WriteUnbounded` (UPDATE/DELETE with no WHERE), `Ddl`,
`DdlDestructive` (DROP, TRUNCATE), `Admin` (GRANT, ALTER SYSTEM). Uses
`sqlparser` where the dialect is supported, with a regex fallback. Unit tests
cover comments, CTEs, strings containing keywords and multi-statement scripts.

## 5. Network

- HTTP: `reqwest` with `rustls`, system root store (`rustls-platform-verifier`),
  optional client certificates, proxy from settings or environment variables.
  Per-connection "skip TLS verification" prints a red badge on every response.
- SSH: `russh`. Algorithms: modern defaults; legacy
  algorithms opt-in per host. Agent support (Pageant/OpenSSH agent on Windows,
  `SSH_AUTH_SOCK` elsewhere). Known hosts in OpenSSH format so it interoperates.
- DB TLS: per-driver, surfaced with the same `ssl` modes vocabulary.
- No outbound connection without user action except the opt-in update check,
  which fetches a signed manifest.

## 6. Scripts (API tests, pre/post request)

Embedded **Rhai**: no file I/O, no network, no process spawn; access only to
the request, response, environment variables (set/get) and a small std. CPU and
memory limits per run. Scripts are visible in the request file, never hidden.

## 7. Clipboard and screen

- Copying a secret or a token from the JWT tool clears the clipboard after a
  timeout (setting).
- "Presentation mode" (command palette) masks all hostnames, usernames and IPs
  in titles, trees and status bar, for screen sharing.

## 8. Updates and distribution

- Release binaries are built in CI from tags, signed (Authenticode on Windows,
  notarised on macOS), with SHA-256 sums published. Windows ships as a zip
  containing `HyperStack.exe` (and nothing else required); no installer.
- The in-app updater (opt-in) downloads only from the release host, verifies
  signature and checksum, and asks before replacing the executable. It never
  touches `data/`. On Windows it writes `HyperStack.new.exe`, then a tiny
  helper swaps files after exit and removes itself.

## 9. Local data hygiene

- Response bodies and query result caches are stored under `data/cache/` and
  deleted according to the retention policy; a "Clear all local data" action
  exists in Settings → Data, and deleting the `data/` folder is a complete,
  documented factory reset.
- Portable defaults: history and cache encryption on, idle relock on, clipboard
  clearing on. Settings → Security shows a one-line "this folder may travel"
  summary of what is and is not protected.
- Logs rotate daily, keep 7 days by default, never contain secrets (redaction
  layer), and the Output panel shows what would be written.
