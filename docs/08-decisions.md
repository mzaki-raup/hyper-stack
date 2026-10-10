# 08 — Architecture decision records

**Status:** draft · **Last updated:** 2026-10-10

Each record: context, options, decision, consequences. Status is *Proposed*
until the first code lands against it.

---

## ADR-001 · Language: Rust

**Context.** The brief asks for Rust "or anything suitable". The author has
shipped Eden Explorer in Rust, and a portable single-executable distribution
is a hard requirement.

**Options.** Rust · TypeScript/Electron · C# / .NET (Avalonia) · Go (Wails).

**Decision.** Rust, edition 2024, stable toolchain.

**Consequences.** Single static binary (ideal for portable use), predictable
memory, excellent async and driver ecosystem (`tokio-postgres`, `mysql_async`,
`redis`, `russh`, `reqwest`). Slower UI iteration than web stacks; mitigated by
egui's immediate mode. Compile times managed by a workspace with many small
crates.

---

## ADR-002 · GUI toolkit: egui/eframe (not Tauri, Iced, Slint, GPUI)

**Context.** The app is widget-heavy: tables, trees, editors, terminals,
split layouts, many tabs.

| Option | For | Against |
|---|---|---|
| **egui/eframe** | Eden Explorer proves it handles a multi-row tab strip, a terminal panel, a theme system and a palette well, so the design risk is known; fast custom widgets; immediate mode makes "many tabs with live data" simple; `egui_kittest` for tests; no runtime dependency, which suits a portable exe | Text editing widget is basic (we build our own in `hs-editor`); accessibility weaker than native |
| Tauri (Rust backend + web UI) | Monaco editor and xterm.js for free; huge web ecosystem | Two languages, IPC boundary for every table row, WebView memory and startup, depends on the system WebView2 (not self-contained for portable use), inconsistent WebView versions on Windows/Linux |
| Iced | Elm-style, retained, nice | Smaller ecosystem for tables/editors; no proven reference for this UI shape |
| Slint | Declarative, fast | Commercial licence considerations for some uses; less flexible for dynamic widget-heavy UIs |
| GPUI (Zed) | Excellent editor rendering | Not yet a stable standalone toolkit; macOS-first history |

**Decision.** egui/eframe 0.35 with the `wgpu` backend (glow fallback).

**Consequences.** We own the editor widget (ADR-005) and write every shell
widget ourselves (ADR-014). We accept egui's accessibility limits and
compensate with full keyboard navigation (AccessKit is enabled in eframe and
gives screen readers a baseline).

---

## ADR-003 · Cross-platform from day one; Windows ships first

**Context.** Eden Explorer is Windows-only and uses many Win32 APIs. DevOps
and DBA users are frequently on Linux/macOS.

**Decision.** `hs-core`, `hs-ui`, `hs-editor`, `hs-term` and all tools compile
on Windows, Linux, macOS from the first commit; CI builds all three. Platform
code is confined to three modules (secrets, paths, PTY). Windows gets the first
polished release.

**Consequences.** Terminal uses `portable-pty` rather than calling ConPTY
directly; secrets use a trait with the encrypted vault as the portable default
and native keyrings as opt-in. Windows-only conveniences Eden Explorer has
(shell context menu, Everything search) are deliberately not in scope.

---

## ADR-004 · One crate per tool, a single `Tool` trait, monolithic binary

**Context.** We want tools to be independent, testable and, later, pluggable.

**Options.** One big crate · one crate per tool linked statically · dynamic
libraries · WASM plugins from the start.

**Decision.** One crate per tool implementing `Tool`/`ToolFactory` from
`hs-core`; tools never depend on each other; the binary registers factories.
Dynamic/WASM plugins are Phase 3, and only for devtools and importers at first.

**Consequences.** Parallel compilation, clear ownership, `cargo test -p
tool-db` works alone. Cross-tool features must go through `Services`,
`OpenRequest` and events, which keeps coupling explicit.

---

## ADR-005 · Editor engine: own widget on `ropey` + `syntect`, tree-sitter later, LSP maybe

**Context.** egui's `TextEdit` is not a code editor (no virtualisation, no
multi-cursor, slow on large text). The editor is also needed by the DB tool
(SQL), API tool (bodies), devtools (JSON), Git (diff) and logs (read-only).

**Options.** egui `TextEdit` · `egui_code_editor` crate · own engine · embed
Monaco via WebView (contradicts ADR-002).

**Decision.** Build `hs-editor`: `ropey` rope buffer, line-indexed, cursor set
with multi-cursor, undo tree, incremental `syntect` highlighting cached per
line and invalidated on edit, virtualised rendering of visible lines only,
pluggable gutters (line numbers, git signs, diagnostics), read-only and diff
modes. Tree-sitter (`tree-sitter` + `tree-sitter-highlight`) in Phase 2 for
folding and structural selection. LSP client is P2 behind a feature flag.

**Consequences.** M1 is the longest milestone. In return every text surface in
the app shares one engine, one theme mapping and one keymap.

---

## ADR-006 · Git: hybrid `gix` for reads, `git` CLI for writes and network

**Context.** `gix` (gitoxide) is fast and pure Rust for status, log, diff,
blame; its push, merge and rebase support is incomplete or young. `git2`
(libgit2) covers more but brings a C dependency, its own credential handling
and no system credential helper integration by default.

**Decision.** `gix` for repository discovery, status, log graph, diff, blame,
refs. Shell out to the user's installed `git` for commit (to honour hooks and
signing), fetch/pull/push (to reuse their credential helpers and SSH config),
merge, rebase, cherry-pick, stash. Parse `--porcelain` output; stream progress
into Jobs. Detect missing `git` and show a clear install hint.

**Consequences.** Behaviour identical to the user's terminal (same hooks,
config, credentials, signing). A `git` binary is required for write
operations; read-only browsing works without it. Can revisit when `gix` ships
stable push/merge.

---

## ADR-007 · Database drivers: native per-engine crates behind a `DbDriver` trait (not sqlx)

**Context.** We need runtime-dynamic queries, streaming results, cancel,
schema introspection and multiple engines.

**Options.** `sqlx` (compile-time checked, `Any` driver) · per-engine crates ·
ODBC for everything.

**Decision.** `DbDriver` trait in `tool-db` with implementations on
`rusqlite` (SQLite, blocking → `spawn_blocking`), `tokio-postgres`,
`mysql_async`, `tiberius` (SQL Server), and `odbc-api` as an escape hatch for
Oracle and others. Document stores are not `DbDriver`s (see ADR-008b). `sqlparser` for statement splitting and the
destructive classifier.

**Consequences.** Per-engine code for introspection queries (information_schema
differs), but full control over streaming, cancellation (`pg_cancel_backend`,
`KILL QUERY`), type decoding and explain plans. `sqlx`'s compile-time checking
offers nothing for ad-hoc queries.

---

## ADR-008 · Redis is its own tool, not a `DbDriver`

**Context.** Redis is key-value with rich types, TTLs, pub/sub and streams.
Forcing it into a SQL-shaped result grid would hide most of its value.

**Decision.** `tool-redis` on the `redis` crate (tokio, RESP3, cluster and
sentinel features), with a key tree, type-specific viewers, a console and later
a server dashboard. Shares connections, tunnels, environments and guards with
everything else.

**Consequences.** Some duplicated plumbing (connection state, console) with
`tool-db`; shared pieces (console widget, history) live in `hs-ui`/`hs-core`.

---

## ADR-008b · NoSQL document stores are their own tool with a `DocStore` trait

**Context.** MongoDB, CouchDB, Elasticsearch, DynamoDB and Cassandra share a
shape — containers of JSON-like documents, queried by a store-specific
language, often schemaless — that neither a SQL result grid nor a Redis key
tree serves well. Users asked for viewing and manipulating these stores.

**Options.** Fold MongoDB into `tool-db` as a driver with a JSON column ·
one tool per store · one `tool-nosql` with a `DocStore` trait and per-store
query editors.

**Decision.** `tool-nosql` with a `DocStore` trait (list containers, count,
find/scan with paging, get/put/delete documents, run native query, list/manage
indexes, infer schema, import/export) and per-store **query front-ends**
(Mongo filter/aggregation editor, Mango/CouchDB, Elasticsearch Query DSL,
DynamoDB key-condition builder + PartiQL, CQL console). Document rendering
(tree, flattened table, raw JSON with type badges) and editing (JSON editor
with store-specific type preservation such as Extended JSON for Mongo) are
shared. Drivers: `mongodb`; CouchDB via `reqwest` against its HTTP API;
Elasticsearch/OpenSearch via `reqwest` (official clients are thin wrappers
anyway); DynamoDB via SigV4 + `reqwest` (reusing the S3 signer) rather than
the full AWS SDK; Cassandra via `scylla`.

**Consequences.** One coherent document UI across stores, shared guards and
history, small binary. Store-specific features (CouchDB views, Elasticsearch
`_cat`, DynamoDB GSIs) are exposed through optional trait extensions so the
common UI stays simple.

---

## ADR-009 · Log viewer: indexed file engine, sources behind a `LogSource` trait

**Context.** Logs are huge, append-only and come from many places (files, SSH,
containers, journald). Loading into memory does not scale; `tail -f` semantics
and rotation must work.

**Decision.** `tool-logs` builds a line-offset index (memory-mapped for local
files, incrementally for streams), keeps a bounded in-memory window of parsed
records, pages backward on demand, and follows via `notify` (local), `tail -F`
over an SSH exec channel (remote, SFTP range read as fallback), the Docker/k8s
log APIs (Phase 2). Parsers are pluggable; filters run on a background task
and publish matching line indices to the UI.

**Consequences.** Multi-GB files open quickly; filtering is asynchronous and
cancellable; the same UI serves every source.

---

## ADR-010 · HTTP: `reqwest` + `rustls`, own OAuth2 flows, Rhai for scripts

**Decision.** `reqwest` (rustls, platform verifier, HTTP/2, gzip/brotli/zstd),
`cookie_store` for the jar, `tokio-tungstenite` for WebSocket, `tonic` later
for gRPC, `oauth2` crate for flows with a local redirect listener. Scripts use
`rhai` with sandbox limits. Postman's JavaScript scripts are translated to
Rhai on import where possible, with the original kept as a comment.

**Consequences.** No JavaScript engine (no `deno_core`), smaller binary, safer
sandbox; some Postman scripts will need manual porting.

---

## ADR-011 · Object storage: our own small SigV4 client, not the AWS SDK

**Context.** `aws-sdk-s3` is large, pulls many crates and is AWS-shaped;
users also target MinIO, R2, B2, Spaces, Wasabi. Eden Explorer showed that a
hand-written SigV4 client of a few hundred lines is enough for browsing.

**Decision.** Write `hs-core::objstore` on async `reqwest` with our own SigV4
signer (`hmac` + `sha2`, tested against the AWS signature test vectors): list
v2 with pagination, multipart upload, range download, copy, delete batch,
presign, object metadata/tags, versions. The same signer serves DynamoDB and
the API client's AWS auth. An `ObjectStore` trait allows Azure Blob and GCS
later.

**Consequences.** A small, auditable client we fully understand; we implement
new S3 features ourselves as needed.

---

## ADR-012 · Storage: files for shareable state, SQLite for history, encrypted vault for secrets

**Decision.** As described in [05](05-data-model-and-storage.md). TOML for
hand-editable objects, JSON for the layout blob, `.sql` with a header for
queries, one object per file, ULID ids, `.local.toml` overrides.

**Consequences.** Workspaces diff and merge well in git and are readable
without the app; a team can share connections and collections. History stays
local and fast in SQLite.

---

## ADR-013 · Async: one tokio runtime, bounded channels, Jobs for anything slow

**Decision.** One multi-thread tokio runtime started before the window; tools
spawn through `Jobs` or `Services::rt`; results arrive via bounded `mpsc` into
per-tool receivers drained at frame start; `UiWaker` triggers repaint. No
`block_on` on the UI thread, ever (a clippy lint config + a debug assertion
enforce it).

---

## ADR-014 · Eden Explorer is a reference, never a code source

**Context.** The sibling project solved several of the same problems in the
same toolkit. Copying would be faster in the short term.

**Options.** Copy modules · extract shared crates both apps depend on · read
for reference and write fresh.

**Decision.** Read for reference, write fresh. No file, function or snippet is
copied from Eden Explorer into this repository's **product code**, and
HyperStack never depends on an Eden crate. Contributors may read Eden's source
to learn behaviour, pitfalls and crate choices, and the architecture doc keeps
a table of which Eden files are worth reading for which problem. The single
exception is delivery tooling (`.forgejo/workflows/`, `scripts/*.ps1`), which
targets the same forge and runner and was adapted from Eden's scripts
(ADR-018); it is not part of the application.

**Consequences.** Phase 0 and Phase 1 take longer than a port would. In
exchange: every module is designed for HyperStack's own `Tool`/`Services`
model, portable data folder and cross-platform core from the first line; no
Windows-only assumptions or file-manager-shaped abstractions leak in; the two
projects can evolve and be licensed independently; there is no shared-crate
coordination cost. The architecture doc's reference table replaces the
"shared crates later" plan entirely.

---

## ADR-017 · Portable by default: data lives beside the executable

**Context.** The app must run from a USB stick or a tools folder on Windows
with its settings next to the executable, with no installer and nothing left
behind on the machine.

**Options.** Per-user data folder (`%APPDATA%`) with an optional portable flag
· portable by default with a fallback · portable only.

**Decision.** Portable by default on every platform, with a safe fallback:

1. At startup `hs-core::storage::paths` resolves the data directory in this
   order: `--data-dir <path>` flag → `HYPERSTACK_DATA` environment variable →
   `HyperStackData/` beside the executable **if the executable's folder is writable**
   (probe by creating and deleting a temp file) → per-user fallback
   (`%LOCALAPPDATA%\HyperStack`, `$XDG_DATA_HOME/hyperstack`,
   `~/Library/Application Support/HyperStack`) with a one-time notice in the
   UI saying where data went and why.
2. Everything the app writes goes under that single directory: settings,
   shortcuts, themes, vault, history, cache, logs, known hosts, the built-in
   `default` workspace, update staging. Nothing is written to the registry,
   `%APPDATA%`, `%TEMP%` (temp files go to `HyperStackData/tmp/`), or the user's home,
   except files the user explicitly exports.
3. All paths stored in settings are **relative to the data directory** when
   they point inside it, so the folder can be moved or renamed. External
   paths (a project folder, a key file) are stored as given; a "portable
   path" helper also accepts `${EXE_DIR}` and `${DATA_DIR}` tokens so users
   can keep SSH keys on the same stick.
4. Secrets use the encrypted vault file (ADR in [06-security.md](06-security.md))
   so they travel with the folder; OS keyrings are opt-in.
5. The updater replaces only the executable. Deleting `HyperStackData/` is the
   documented factory reset. A `HyperStack.exe --data-dir X` shortcut supports
   several isolated profiles side by side.
6. SQLite databases run in WAL mode with `synchronous=NORMAL` and checkpoint on
   exit so removable media stays consistent; the app detects well-known
   cloud-sync folders (OneDrive, Dropbox, Google Drive paths) and warns once
   about possible sync conflicts.
7. Single-instance lock is a file in `HyperStackData/` (`instance.lock`) with the PID,
   never a named mutex or registry key, so a second copy on another drive
   works independently.

**Consequences.** Zero-footprint on the host machine, trivial backup and
migration, several isolated installs possible. Users in `Program Files`-style
read-only locations get the per-user fallback automatically. The vault's
master-password prompt is the price of portability; the Windows DPAPI
"remember on this PC" option softens it without weakening the travelling copy.

---

## ADR-018 · Delivery: Forgejo Actions, commit-driven SemVer, signed manifest, rename-and-relaunch updater

**Context.** The code lives on a self-hosted Forgejo (`git.zacx.net`) with one
Windows runner that cannot use published actions. Users run a portable
executable and must be able to update it from inside the app without an
installer. Eden Explorer's pipeline on the same forge showed two failure modes:
releases published with missing assets, and release metadata clobbered by the
forge's asynchronous tag-to-release derivation.

**Options considered.**

| Topic | Options | Chosen |
|---|---|---|
| CI system | Forgejo Actions on the existing runner · GitHub mirror + Actions · none | **Forgejo Actions**, PowerShell steps, no `uses:` |
| Versioning | manual bump in `Cargo.toml` · `cargo-release` · derive from Conventional Commits | **derive from commits** via `scripts/next-release.ps1` (Conventional types first, plain verbs as fallback); manual override by `release:` trailer |
| Release scripting | PowerShell scripts · Rust `xtask` crate | **PowerShell scripts in `scripts/`**, rewritten from Eden Explorer's release tooling (the one deliberate exception to ADR-014: delivery tooling for the same forge and runner, not product code); a Rust `xtask` can replace them later if Linux/macOS runners make PowerShell awkward |
| Release trust | TLS only · `SHA256SUMS.txt` only · Authenticode only · signed manifest | **Ed25519-signed `manifest.json`** (minisign) as the mandatory check, sha256 inside it, Authenticode added when a certificate exists, `SHA256SUMS.txt` kept for humans |
| Update source | forge releases API · static manifest URL · both | **forge releases API** for discovery, manifest asset for truth; `source` override for mirrors |
| Install mechanism | installer (MSI/Inno) · helper updater exe · rename the running exe and relaunch | **rename-and-relaunch**: Windows allows renaming a running executable, so no helper, no elevation, works on a USB stick |
| Rollback | none · keep previous exe · keep N versions | **keep one previous** with a Revert button and automatic offer after a crash before first frame |
| Update check default | off · on | **on**, daily, single anonymous GET, switchable; install always user-initiated unless "install on exit" chosen |

**Decision.** As in the table; full mechanics in
[09-ci-cd-and-updates.md](09-ci-cd-and-updates.md). The asset names and the
manifest schema are a versioned contract (`schema`, `min_supported_version`).
The release job must verify its own published release as an anonymous client
before it may succeed.

**Consequences.** Releases are frequent, small and reproducible from tags.
Users of portable copies update in two clicks and can revert in one. The
signing key is a single point of trust and is protected as a forge secret with
a documented rotation path (the app embeds a list of accepted keys). The forge
is a hard dependency for updates; a mirror option exists but still requires the
signed manifest. Linux and macOS delivery wait for cross-compilation (Phase 2)
and a Mac runner (Phase 3).

---

## ADR-015 · Terminal emulation: `alacritty_terminal` grid + `portable-pty`

**Decision.** Build `hs-term` on `alacritty_terminal`'s `Term` for parsing
and the grid (the same choice Eden Explorer validated), with our own listener,
rendering and key-encoding code; `portable-pty` for cross-platform PTYs; SSH
shells feed the same grid through a `Transport` enum (`Pty`, `Ssh`,
`DockerExec`, `KubectlExec`).

---

## ADR-016 · Icons and fonts

**Decision.** Phosphor icons via `egui-phosphor`, a bundled Nerd Font variant
for terminal glyphs, system font enumeration for UI/editor font pickers via
`fontdb`. Fonts are embedded in the executable (`rust-embed`/`include_bytes!`)
so the portable exe has no loose asset files.
