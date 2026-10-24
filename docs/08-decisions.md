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

**Amended 2026-10-12.** The first Git feature to ship is the read-only Git
view in the editor, and it reads through the `git` CLI as well (`hs-git`:
porcelain v2 status, diffs, log, branches, `--no-optional-locks`), not
`gix`: one dependency instead of a large one, identical answers to the
user's terminal (config, attributes, worktrees), and the parsers are small
and tested against a real repository. `gix` comes back if status or log on
large repositories proves too slow through the CLI; `hs-git`'s API does not
change when it does. Consequence: without `git` installed the Git view
shows an install hint instead of read-only data.

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

**Amended in M3 (2026-10-12).** The abstraction landed as one `DbSession`
trait object per tab with an `Engine` enum, not a separate `DbDriver` plus
`Dialect`. Statement splitting and the classifier use the tool's own
dialect-aware tokenizer instead of `sqlparser`: the classifier needs the
verb, CTE bodies and a missing WHERE, and `sqlparser` rejects much valid
dialect SQL (procedures, `DELIMITER` scripts, extensions) that still has to
be split and guarded. PostgreSQL runs over the simple-query protocol so
values keep the server's text form (docs/tools/database.md §15).

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

## ADR-019 · Optional AI assistant in the editor (bring your own service)

**Context.** Q6 in [01](01-vision-and-scope.md) deferred an AI assistant
past v1. On 2026-10-11 the owner asked for one in the editor: chat about the
open file or a selected fragment, review it, and let the model change the
code directly, iterating like an agent ("cascade"). The owner uses 1min.AI,
and wants self-hosted LM Studio with several models as well.

**Decision.**
- `hs-core::ai` holds a small streaming client with two provider kinds:
  **1min.AI** (its own API: `POST /api/chat-with-ai?isStreaming=true`, type
  `UNIFY_CHAT_WITH_AI`, `API-KEY` header, SSE events `content`/`result`/
  `done`/`error`; it takes one prompt, so the conversation is folded into a
  transcript) and **OpenAI-compatible** (`POST {base}/chat/completions`,
  bearer token optional, `GET {base}/models` for the model list). The second
  covers OpenAI, LM Studio, Ollama, OpenRouter, vLLM and similar servers.
  ChatPlayground AI publishes no developer API, so it cannot be a provider.
- Several named providers live in `settings.toml` (`[ai]`), each with a
  default model; keys are vault references (`secret:<ulid>`). A chat picks
  any provider and model, so one LM Studio server with JIT loading serves
  every downloaded model.
- Models change code only through **SEARCH/REPLACE edit blocks** that the
  editor parses, shows as diffs and applies on request (or automatically
  in Cascade), each as one undo step. No tool calls are needed, so the same
  flow works with 1min.AI, which has none. Cascade sends the updated file
  back after applying and stops when the model answers DONE, when no edit
  applies, or after `max_rounds`.
- Nothing is sent until the user asks; the request runs on the runtime.

**Consequences.** Code leaves the machine when a remote provider is used
(the settings page says so and suggests a local server). Edits are applied
to the buffer, never saved without the user. Other tools (SQL "explain this
query") can reuse `hs-core::ai` later.

**Amended 2026-10-14.** Terminal, SSH, SQL and S3 use a shared
`hs-ui::ai_assistant::AssistantPanel` for review-only help. Context is
prepared locally and shown in a collapsed preview; nothing is sent until the
user submits a question. The assistant cannot edit files, execute SQL, run
shell commands, or invoke storage actions. API assistance will be added when
the API client exists; that tool is not implemented yet.

**Amended 2026-10-15.** Every assistant answer renders fenced code blocks
with **Copy** and, where the tool has a place for it, **Insert**: the
terminal and SSH shell receive the block as typed input (the user presses
Enter), the SQL editor and the API client's body or script take it at the
cursor. Insert never runs anything. The API client and the graphics tool
use the same panel (graphics with Insert disabled).

**Amended 2026-10-16** (owner's request). Two changes to the above:

- Every assistant offers the tool's own **quick actions** under the chat,
  the way the editor offers Explain / Review / Improve: the terminal and
  SSH explain output or an error and suggest a command, SQL writes a
  select, a count or an ordered select and reviews the open query, the API
  client writes pre-request and post-response scripts (including one that
  stores response values as environment, collection or global variables),
  tests and docs, S3 explains a selection and writes a CLI command or a
  policy, graphics suggests edits. A quick action fills the input and
  sends it; nothing is sent before that click.
- A block can also be **Insert and Run** where the tool has a Run of its
  own: the terminal and SSH add the Return the user would press, and the
  SQL editor sends exactly the inserted text through the same
  classify-and-confirm path as the Run button, so a protected
  environment, a dangerous statement or a read-only profile still stops
  it. Insert on its own still runs nothing, and the API client keeps
  Apply for its `hs-api` action blocks. The owner asked for this
  explicitly; the panel is no longer review-only for tools that execute.

**Amended 2026-10-16** (owner's request, services that are not
OpenAI-compatible). `hs-core::ai` now carries **six** provider kinds
instead of two, chosen by `AiProvider::kind`:

| Kind | Endpoint and shape |
|---|---|
| **OpenAI-Compatible** | `POST {base}/chat/completions`, bearer key, `GET {base}/models`. Covers OpenAI, Mistral, Moonshot (Kimi), Tencent Hunyuan, OpenRouter, 1min.AI's adapter, LM Studio, Ollama and vLLM, which all copy that API |
| **1min.AI** | its own `chat-with-ai` API, unchanged |
| **Anthropic** | `POST {base}/v1/messages`, `x-api-key` and `anthropic-version` headers, the system prompt in its own field, turns that must alternate and start with the user, SSE `content_block_delta` text deltas ending at `message_stop` |
| **Google Gemini** | `POST {base}/v1beta/models/{model}:streamGenerateContent?alt=sse`, `x-goog-api-key`, `contents`/`parts`, and no end marker: the stream simply stops |
| **Azure OpenAI** | `POST {resource}/openai/deployments/{deployment}/chat/completions?api-version=…`, `api-key` header; the model field names the *deployment*, and an API version is part of the contract |
| **AWS Bedrock** | `POST /model/{id}/converse` on `bedrock-runtime.{region}.amazonaws.com`, signed with SigV4 (`hs_core::objstore::sigv4`, service `bedrock`) from an access key *and* a secret key |

- A provider therefore has three more fields (`secret_key`, `region`,
  `api_version`); both key fields are vault references, and
  `Services::ai_credentials` is the one place that resolves them, so a
  locked vault is an error rather than an empty key.
- **Bedrock answers in one piece.** Its streaming form is an
  application-level binary event stream (`vnd.amazon.eventstream`), which
  this client does not speak, so the Converse call is made without
  streaming and the whole answer arrives at once; the settings page says
  so. Bedrock also has no model list without a second permission, so a
  **Known Models** button offers the well-known ids instead of Fetch
  Models.
- Settings → AI Assistant adds a provider from a **gallery** of fifteen
  services in three groups (Hosted Services, Gateways, On This Machine):
  picking one fills in its kind, base URL and default model, and the form
  then shows only the fields that kind needs. The catalogue is a
  convenience, not a constraint: every field stays editable and any other
  server that copies OpenAI's API works under OpenAI-Compatible.

**Consequences.** Two services the owner named are reached through the
OpenAI-compatible kind rather than a kind of their own (Mistral, Moonshot,
Hunyuan); if one of them diverges, it needs its own arm. Bedrock's
non-streaming answer feels slower than the others even when it is not.

## ADR-020 · API client scripts run JavaScript on Boa with a `pm` shim

**Context.** [tools/api-client.md](tools/api-client.md) planned Rhai for
pre-request and test scripts. On 2026-10-15 the owner asked for a client
"almost exactly like Postman" with **custom JavaScript** to store variables
in the environment, pre/post request scripts and pasted library code, and
for Postman import/export, which carries `pm.*` scripts verbatim.

**Decision.**
- Scripts are JavaScript, executed by the pure-Rust **Boa** engine
  (`boa_engine`, default features off) inside the send pipeline on the
  runtime's blocking pool, with a loop-iteration and recursion limit and
  no host I/O: there is no `fetch`, no timers, no file access.
- The `pm` object is written in JavaScript (`tool_api::script::PRELUDE`)
  over four host functions (`__hs_get/__hs_set/__hs_unset/__hs_test` plus
  `__hs_log`) that read and write the variable scopes through a
  thread-local host record. It covers what day-to-day scripts use:
  `pm.environment / globals / collectionVariables / variables`,
  `pm.request` (editable before sending), `pm.response` with
  `json()/text()/to.have.status()`, `pm.test`, `pm.expect` with a small
  chai subset, `console.log`, and the legacy `postman.*` setters.
- Scripts never see secrets: a value that is a `secret:` reference stays a
  reference in the scopes and is resolved by the vault only when the
  request is built. Variable changes are written where they belong
  (globals file, collection file, the active HyperStack environment).
- Collection **libraries** (pasted JavaScript) are evaluated before every
  script of the collection, which is how `require`-less helper code and
  pure-JS libraries are shared.

**Consequences.** Boa adds a few minutes to a clean build and about 10 MB
to the executable; in exchange Postman collections run unchanged for the
common patterns. `pm.sendRequest` and the collection runner are not there
yet (they need the runtime inside scripts and a Job) and throw a clear
error. Rhai is not used.

## ADR-021 · The browser tool uses a native child web view owned by the shell

**Context.** egui cannot render HTML. The owner asked (2026-10-15) for a
multi-tab browser with ad blocking, a JavaScript switch, inspect element and
forced dark mode, with tabs restored per workspace and links from other
tools opening in it.

**Decision.**
- `tool-browser` is a normal tool that draws only the chrome and persists
  the pages; the page itself is a **WebView2 child window** created by
  `wry` from the window handle `eframe::Frame` exposes, owned by
  `hyperstack::browser_host`. The tool and the host talk through a shared
  hub (desired rectangle, pages and commands one way; titles, URLs,
  loading, blocked navigations and new-window requests the other way).
- The host positions the view over the tool's page area every frame and
  hides it whenever the tab is not visible or a modal is open, since a
  native window always paints above egui.
- Blocking is a navigation blocklist (bundled list plus settings) and
  cosmetic hiding by an initialization script; forced dark is a CSS
  inversion injected the same way; JavaScript off rebuilds the view.
- `wry` is a Windows-only dependency of the binary for now; the tool
  crate has no platform code and compiles everywhere, showing a notice
  where no native view exists.

**Consequences.** The page cannot be split or detached like egui content,
drop-down menus over the page area are covered by it, and subresource ads
are hidden rather than blocked. The rest of the application stays pure
egui, and removing the feature is deleting one module and one dependency.

## ADR-022 · Home tab, theme modes, Read Aloud and Snip/Print live in the shell

**Context.** Owner's requests of 2026-10-15: a pinned welcome/dashboard
surface in the activity rail, a dark/light/system mode that survives a
restart on Windows 10, text-to-speech, picture or print export of every
tool's content, settings import/export behind the master password, and a
configurable activity rail.

**Decision.**
- **Home** (`hyperstack::tools::home`) is a mandatory pinned tab first in
  every workspace, like Settings; it talks to the shell through a hub
  because it needs what only the shell knows (recently closed tabs, the
  registry). Settings → Welcome is gone.
- **Theme mode**: `appearance.mode` (`system`/`dark`/`light`) with a dark
  and a light scheme. The shell pins egui's theme preference and fills
  *both* of egui's visuals slots with the chosen palette, so a late or
  contradicting system-theme report (Windows 10) can no longer swap our
  palette for egui's default one; System mode gives egui both palettes.
- **Read Aloud** uses the operating system's voice through a child
  process (`hs_core::tts`); tools offer texts through `Tool::speakable`.
- **Snip** uses egui's own screenshot (cropped to the tab's rectangle),
  **Print…** writes an HTML page (`Tool::printable`) under `tmp/` and opens
  the browser, whose print dialog does paper and PDF.
- Settings and shortcuts export as one TOML bundle with no secrets;
  import merges providers by id and sets shortcuts one by one; both
  re-ask the master password.

**Consequences.** Two mandatory tabs per workspace; the status bar counts
only work tabs. Printing depends on an installed browser.

## ADR-023 · Language features through installed language servers (LSP client in `hs-lsp`)

**Status.** Accepted, 2026-10-15.

**Context.** The owner asked for Zed-like coding features (intellisense,
navigation) in the editor. Zed bundles and downloads language servers per
language and wraps each in an extension; its code is GPL-licensed and is a
reference only here (project rule). HyperStack is a portable single exe
that must not write outside its folder, and bundling one server per
language (rust-analyzer alone is 40 MB) would not fit the app.

**Decision.**
- `hs-lsp` is a small client written here: JSON-RPC 2.0 over the server's
  stdio with `Content-Length` framing, one reader thread per server,
  callbacks for replies and a channel of events the editor drains once per
  frame. Only the slice of the protocol the editor uses is typed; the rest
  stays `serde_json::Value`. No `lsp-types`/`url` dependency: the types the
  editor needs fit in one file, and the client controls its own
  compatibility.
- Servers are **not bundled or downloaded**. A registry maps each editor
  language to the conventional command and its install line; the command
  is looked up on `PATH` (with `PATHEXT`, so npm `.cmd` shims count). The
  user can replace any language's command in Settings, which is the
  "extension" mechanism: any server that speaks LSP over stdio.
- Documents are synced as full text, debounced, because every server
  supports it and the editor's buffers are small enough; the client asks
  for UTF-8 positions and converts UTF-16 (the default) itself.
- Replies never block the UI thread: requests carry a sequence number and
  stale answers are dropped.

**Consequences.** Features depend on what the user installed, and the
status bar says what is missing and how to get it. One server process per
project root and language per editor tab (tabs are projects), stopped when
the tab closes. Incremental sync, code actions and semantic tokens are
later additions behind the same client.

## ADR-024 · Debugging through installed debug adapters (DAP client in `hs-dap`)

**Status.** Accepted, 2026-10-16.

**Context.** The same owner request as ADR-023 asked for debugging. Zed
implements the Debug Adapter Protocol and ships or downloads adapters per
language; HyperStack must stay a portable exe and cannot carry CodeLLDB
(tens of megabytes) or a Python.

**Decision.**
- `hs-dap` mirrors `hs-lsp`: `Content-Length` framing, a reader thread,
  callbacks for responses and a channel of events drained per frame, only
  the protocol slice the panel shows typed. Adapters that listen on TCP
  (CodeLLDB, delve) are started with a free port and the client polls the
  port until it answers, all on a thread.
- Adapters are installed by the user; a registry maps each to its command
  and install line, Settings can replace the command, and `launch.json`
  `type` names map onto the same registry so VS Code configurations work
  unchanged. `.hyperstack/debug.json` uses Zed's shape so examples from
  Zed's documentation apply.
- The client does not implement `runInTerminal`: generated configurations
  keep the debuggee's console inside the editor, which every supported
  adapter offers, and a configuration that insists on a terminal gets a
  clear message.
- Breakpoints are plain line breakpoints saved with the tab's layout;
  conditions, logpoints and watches wait for a request.

**Consequences.** Debugging works where an adapter is installed and says
what is missing otherwise. One session per editor tab. The build step is
a child process with its output in the console; nothing runs in the
shell's terminal tool.

## ADR-025 · Native diagram document, scene/export pipeline and an Archify-adapted IR

**Status.** Accepted, 2026-10-19.

**Context.** The owner asked for a diagram generator and a draw.io-like
editor inside the Graphics activity, with diagrams saved into a project
folder, unsaved work surviving a restart, and PNG/PDF export. Archify was
named as a reference. Archify is an MIT-licensed Node.js agent skill that
turns a typed JSON IR into standalone HTML; it has no WYSIWYG editor and no
general auto-layout, and its renderer is browser-oriented. HyperStack is a
portable Rust/egui binary whose CI runner has no Node.

**Decision.**
- Build a native diagram document and editor. `hs-diagram` owns the
  editor-native `.hdiag` format, a separate semantic IR, deterministic
  layout, a resolved `Scene`, and SVG → PNG (`resvg`) and SVG → PDF
  (`svg2pdf`) export. The renderer is one pipeline for screen and files; no
  Archify HTML/CSS/SVG/JS is copied.
- Adapt Archify's JSON Schema and validation rules only, pinned to v3.2.0
  (commit `208acba8d412335185b2056fedf54b3405df43fb`), as a semantic IR for
  the generator and (in Release 2) Archify JSON import. The adapted files
  carry the MIT notice and both copyright lines in
  `THIRD_PARTY_NOTICES.md`, on the About page and in the release zip. Brand
  marks, fonts, the viewer and the generated validators are not copied.
- Keep `.hdiag` editor-native (absolute geometry, styles, page) and the IR
  semantic (lanes/columns/rows, no coordinates). Storing the IR in the file
  would drag renderer-oriented fields into every save and complicate
  editing; importing Archify JSON maps the IR to the document instead.
- Release 1 ships flowchart and architecture, the Graphics activity shell,
  the Image/Icon split, the generator and the four AI assistants. Release 2
  adds ERD, sequence, draw.io XML import and Archify JSON import.
- Drafts and recents live under `HyperStackData/graphics/`, not inside the
  workspace, because a workspace can be an external `.hyperstack/` folder in
  a git repository.

**Consequences.** The editor is the expensive part, not the file format or
the export; the IR and schema adaptation are bounded. PDF export is vector
with a subset font and the same scene as the screen. A future Archify upgrade
requires a deliberate re-pin and a notice update. Full draw.io parity is
explicitly out of scope.

## ADR-026 · A Container activity built on the connection store, with CLI-where-honest drivers

**Date** 2026-10-24 · **Status** accepted (specified; see
`docs/tools/containers.md`) · **Owner's request**, pulled forward from
Phase 2 ahead of M6.

**Context.** Docker (single container, Compose, Swarm), Podman,
Kubernetes and Windows containers belong in the workspace that already
holds the connections, environments and secrets they need. The owner
asked for a Portainer-shaped Docker pane and a Lens-shaped Kubernetes
pane, and for every credential to live in the vault.

**Decision.**

- **A container host is a `ConnectionProfile`**, not a new store.
  `kind` already reserved `docker` and `k8s` and nothing consumed them;
  `podman` and `wincontainer` join them. This inherits the vault,
  environments, read-only/protected handling, folders, tags, pinning
  and ordering, and it means Settings → Containers → Container List is a
  kind-filtered view of one store rather than a second list that can
  disagree with the first.
- **kubeconfig is referenced by path and context, not copied into the
  vault**, with a Paste option for a static or portable case. A copy
  would go stale on re-provisioning, duplicate a secret that already
  exists on disk, and break the `exec` credential plugins that resolve
  relative to the real file. Tokens, passwords and TLS keys are
  vault-bound unconditionally.
- **The Docker/Podman driver is the CLI** (`docs/tools/containers.md`
  §10.3a, revised 2026-10-24 while phase 2 was built). `bollard` reaches
  a local socket and a TCP host; it cannot reach an engine inside a WSL
  distribution - where it usually is on Windows - nor one over SSH,
  without first exposing or forwarding a socket. The CLI reaches all
  four transports by prefixing, and `docker` and `podman` accept the
  same `--format '{{json .}}'`, so one parser serves both. `bollard` is
  **deferred, not rejected**: streaming `/events` and `/stats` is the
  one thing the CLI cannot do, so it arrives in phase 4 for the Local
  and Tcp transports, behind the same `Engine` interface. Kubernetes
  stays `kube` + `k8s-openapi`. Typed watches are what make a live pane possible, and
  needing no external binary matches the portable promise. A profile
  may be marked **Use kubectl** as an explicit escape hatch where a
  cluster's auth cannot be satisfied in-process. **Compose and Swarm
  are CLI-only surfaces** and are shelled out to for fidelity, through
  `hs_core::paths::background_command` so no console appears. Docker
  inside WSL reuses the `wsl.exe -d <d> --` prefixing proven in
  `tool_logs::services`.
- **Logs and shells are not rebuilt.** Viewing a container's or pod's
  log opens a **Logs tab** through `OpenRequest` with the follow
  command pre-filled; Exec opens a shell through `hs-term`'s
  `Transport`, as `hs-ssh` does. Containers owns the inventory and the
  verbs, not a second log viewer to fix twice.
- **Applying YAML dry-runs and diffs first**
  (`kubectl apply --dry-run=server`), and is refused on a protected
  environment unless the profile is explicitly not read-only.
- **`hs_ui::table::DataTable` is fixed to scroll before the panes are
  built.** It divides the width it is given and then clips, so a narrow
  pane hides columns unreachably; `tool-db::grid` already has the
  working pattern. S3 and the SFTP pane get the fix too.
- **Unhealthy objects post to `hs_core::problems`**, so a crash loop is
  visible in the dock without watching the pane.

**Alternatives rejected.** A separate container store (a second
credential path and a second ordering that would drift from the first).
A log viewer inside the activity (duplicates `tool-logs`, including its
filters, levels, time strip and assistant). Copying kubeconfig into the
vault by default (stale on re-provisioning, and breaks the `exec`
plugins that resolve relative to the real file). Driving Kubernetes
through `kubectl` as the primary path was considered and kept only as
the escape hatch: it would trade live watches and the portable promise
for auth convenience the `kube` crate largely already provides.

**Consequences.** Kubernetes works without any external binary;
`kubectl`, `helm` and `docker compose` are detected and reported like a
language server when they are wanted, never bundled. Windows container
support is whatever the Windows engine reports; no Hyper-V
management. New assistant prompt buttons need no
migration, because `prompts::resolve` already appends shipped defaults
that a saved list does not mention. Eight phases, each releasable, with
the `DataTable` fix first because nothing is readable without it.
