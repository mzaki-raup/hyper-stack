# 06 — Security

**Status:** draft · **Last updated:** 2026-10-10

HyperStack holds the keys to databases, servers, buckets and APIs, often
production ones. Security is a product feature, not a checklist.

## 1. Threat model

| Threat | In scope? | Mitigation |
|---|---|---|
| Secrets leaking through files on disk (USB sticks, sync folders, repos, backups) | Yes | Secrets only in the encrypted vault (or an opt-in OS keyring); workspace files contain `secret:<id>` references only; export never includes secrets; a test greps `HyperStackData/` after an e2e run. A portable `HyperStackData/` folder is expected to travel, so the vault is designed to be safe when copied |
| Lost or stolen portable drive | Yes | Vault is useless without the master password (Argon2id, memory-hard); history and caches can be encrypted too (setting "encrypt local history", default on when portable) |
| Secrets leaking through logs, history, crash reports | Yes | Redaction layer in `tracing`; history stores resolved URLs/SQL with secret values replaced by `***`; no crash reporting upload |
| Shoulder surfing / screen sharing | Yes | Masked by default everywhere; reveal is per-click and auto-hides after 10 s; clipboard copy of a secret clears after 30 s (configurable) |
| Running a destructive action against the wrong environment | Yes | Environment banner, protected environments, destructive-statement guards, read-only connections |
| Man-in-the-middle on SSH / TLS | Yes | Host key verification with known_hosts and a first-connect fingerprint dialog; TLS via rustls with system roots; `verify=false` is a per-connection, visibly flagged setting |
| Malicious workspace folder (cloned repo with `.hyperstack/`) | Yes | Opening a workspace never runs anything; scripts (Rhai) run only when the user runs a request, in a sandbox with no filesystem/network access beyond the request itself; connections from a new workspace are shown in a review list before first use |
| Malicious server responses (terminal escape sequences, huge payloads) | Yes | Terminal parser is alacritty's (battle-tested); response bodies capped (default 50 MB) and rendered as text, never executed; JSON/HTML preview is a sanitised renderer, not a browser |
| Supply chain (crates) | Partly | `cargo deny` for licences and advisories in CI; `cargo vet` or audited lockfile review for anything touching crypto/network; release builds reproducible from tagged source |
| Compromised machine | No | Out of scope: a keylogger captures the master password; an unlocked vault is readable from process memory |

## 2. Secret storage

`hs-core::secrets::SecretStore` trait with two implementations:

1. **Encrypted vault file** (default, portable): `HyperStackData/vault.hsv`. Format:
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
- A password typed into a connection form is moved into the vault when the
  profile is saved. If that cannot happen (vault locked, write failed) the
  profile is **not saved**: the form keeps the text and asks for the vault
  to be unlocked, so a typed secret never reaches the profile's TOML file.
  `{{variable}}` references are kept as typed.
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

A small classifier (not a full SQL grammar) in `tool-db::sql` classifies each
statement: `Read`, `Session` (BEGIN/COMMIT/SET/USE…, allowed on read-only
connections), `Write`, `WriteUnbounded` (UPDATE/DELETE with no WHERE or LIMIT
at its own level), `Ddl`, `DdlDestructive` (DROP, TRUNCATE, ALTER … DROP),
`Admin` (GRANT/REVOKE, ALTER SYSTEM/USER/ROLE, CREATE USER/ROLE, KILL,
SET GLOBAL, `pg_terminate_backend()`…). It runs on the tool's own
dialect-aware tokenizer rather than `sqlparser`: it only has to be right
about the verb, CTE bodies (`WITH d AS (DELETE …)`, `EXPLAIN ANALYZE
DELETE …`) and a missing WHERE, and anything it does not know is a `Write`.
It also names the target (`DROP TABLE x`, `DELETE FROM x`) for the "type the
name" guard. Unit tests cover comments, strings and quoted identifiers
containing keywords, CTEs, subqueries with their own WHERE, `ON DELETE
CASCADE`, `FOR UPDATE` and multi-statement scripts.

What the tool does with the class (docs/tools/database.md §9, §15):
read-only profiles and protected environments (until **Allow Writes** for
the tab) refuse anything but `Read`/`Session`, and their server session is
read-only too; `WriteUnbounded`, `DdlDestructive` and `Admin` (and `Ddl` on
a protected environment) open a dialog naming the connection and the
environment, and on a protected environment the target's name has to be
typed.

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
- The AI assistant (ADR-019) sends only what the user chose (the selection,
  the open file, or nothing) and only when the user asks or Cascade continues
  a task the user started. Provider keys live in the vault; the settings
  page warns that code leaves the machine and points to local servers (LM
  Studio, Ollama). Edits the model proposes change the buffer, never the
  file on disk, and a read-only or protected buffer refuses them.

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

Full design in [09-ci-cd-and-updates.md](09-ci-cd-and-updates.md). Security
properties:

- Release binaries are built by the Forgejo release workflow from a tagged
  commit on `main`; the job re-runs all checks, publishes a `manifest.json`
  signed with an Ed25519 (minisign) key held only as a forge secret, and then
  downloads its own release anonymously to verify it before it may go green.
- The app embeds the minisign public key(s). It accepts a release only if the
  manifest signature verifies and the downloaded file's size and sha256 match
  the manifest. TLS alone is not trusted: a compromised or mirrored host cannot
  inject a binary without the signing key. Authenticode signing is added when
  a certificate is available (`WinVerifyTrust` checked when present).
- No downloaded file is executed before verification; after verification it
  is run once with `--self-test` in a child process before being installed.
- Install is rename-and-relaunch inside the executable's folder; it never
  touches `HyperStackData/` except `HyperStackData/update/`. The previous
  executable is kept for a one-click revert, and a crash before the first
  successful frame of the new version offers the revert automatically.
- Update checks are a single anonymous GET to the project's own forge, on by
  default, switchable, with no identifier beyond the version string. Mirrors
  and `enabled = false` can be set in `settings.toml` for managed machines.
- Windows ships as a zip containing `HyperStack.exe`; no installer, nothing
  elevated, nothing in the registry.

## 9. Local data hygiene

- Response bodies and query result caches are stored under `HyperStackData/cache/` and
  deleted according to the retention policy; a "Clear all local data" action
  exists in Settings → Data, and deleting the `HyperStackData/` folder is a complete,
  documented factory reset.
- Portable defaults: history and cache encryption on, idle relock on, clipboard
  clearing on. Settings → Security shows a one-line "this folder may travel"
  summary of what is and is not protected.
- Logs rotate daily, keep 7 days by default, never contain secrets (redaction
  layer), and the Output panel shows what would be written.

## Addendum (2026-10-15): scripts, browser, devtools, exports

- **API client scripts** run in Boa with no host I/O (no `fetch`, timers
  or files) and a loop/recursion limit. They see variable *references*,
  never vault values: `secret:` strings resolve only in the send pipeline.
  A non-GET request to a protected environment asks first.
- **Browser**: navigation to a blocklisted host is refused by the view's
  navigation handler; ad containers are hidden by style. WebView2 keeps
  its own cookies and cache outside `HyperStackData/`; **Clear Browsing
  Data** empties them. Pages are restored from `layout.json` (URLs and
  titles only).
- **Devtools**: inputs are saved with the tab, except keys, passwords,
  tokens and service accounts, which are written only as `{{references}}`
  to environment variables (a typed secret is blanked by `for_save()`).
  **Save to Environment…** is the sanctioned way to keep one: it goes into
  the vault and the field keeps the reference. The peek popup masks vault
  values until Reveal and never writes them anywhere. The Firebase utility
  talks to Google only when its two network buttons are pressed.
- **Settings bundles** carry no secrets (vault references only) and need
  the master password to export or import.
- **Read Aloud** sends text to a local child process (Windows Speech,
  `say`, espeak-ng); nothing leaves the machine.
- **Speech to text** (the assistants' microphone) likewise runs the
  operating system's dictation engine as a local child process; audio is
  never recorded to disk or sent anywhere, and the recognised text only
  lands in the input box the user then edits and sends, or not. The
  language setting is validated to a culture name before it reaches the
  PowerShell command line.

