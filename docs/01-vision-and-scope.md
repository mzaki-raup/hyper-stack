# 01 — Vision and scope

**Status:** draft · **Last updated:** 2026-10-10

## 1. One-line positioning

HyperStack is a native, portable desktop workspace that puts the day-to-day
tools of software delivery — editing, databases, APIs, servers, object storage,
Git, terminals and utilities — in one window, sharing one set of connections,
environments and secrets.

## 2. Problem

People who build and run software spend their day in six to ten separate tools:

| Need | What people use today | Pain |
|---|---|---|
| Edit a config or script | VS Code, Notepad++, vim over SSH | Heavy to launch for a two-line edit; remote edits need a separate SFTP tool |
| Look at a database | DBeaver, pgAdmin, SSMS, TablePlus | Java/Electron memory use, one connection list per tool, no link to the API or code that uses the table |
| Call an API | Postman, Insomnia, Bruno, curl | Account nagging, cloud sync of secrets, environments duplicated from the DB tool |
| Reach a server | PuTTY + WinSCP, Termius, MobaXterm | Shell and file transfer are separate apps; credentials entered twice |
| Browse S3 / MinIO | Cyberduck, S3 Browser, AWS console | Another credential store, another UI |
| Commit and push | Git CLI, GitKraken, Fork, Sourcetree | Fine alone, but disconnected from the editor and terminal in use |
| Decode a JWT, format JSON, test a regex, build a cron | Random websites | Pasting production tokens into a stranger's web page |

The real costs are **context switching**, **duplicated connection and secret
management**, and **no shared idea of "which environment am I pointed at"**.
Pointing a query at production because the DB tool's environment selector did
not match the API tool's is a real class of incident.

## 3. Vision

- **Connections, environments and secrets are first-class and shared.** A
  "staging" environment carries the DB connection, the API base URL and
  variables, the SSH hosts, the S3 bucket and the Kubernetes context. Switching
  environment switches every tool at once, with a visible, colour-coded banner.
- **Every tool is a tab, every tab is cheap.** Open a query, an SSH shell, a
  request and a diff side by side; wrap tabs onto extra rows rather than hiding
  them; save the whole arrangement as a workspace.
- **Tools talk to each other.** Query result → JSON → request body. SFTP file →
  editor → save back over SFTP. Git diff → editor. Log line → highlighted in the
  terminal. Request response → saved as a fixture file.
- **Native, fast, portable, private.** One executable, sub-second startup, no
  account, no telemetry, no cloud sync unless the user puts the workspace folder
  in a git repo or a synced drive.

## 4. Target users

| Persona | Typical day | What HyperStack must nail for them |
|---|---|---|
| **Full-stack developer** | Edit code, run the app, hit its API, check the table it writes, commit | Editor + API client + DB client + Git in one window; environment switching |
| **DevOps / SRE** | SSH into boxes, tail logs, poke containers, move artifacts to S3, edit YAML | SSH + SFTP + terminal + S3 + containers; multi-host sessions; key management |
| **Database administrator** | Run queries, inspect schema, export data, compare environments | Database tool depth: schema browser, result grid, explain plan, export, safety guards |
| **Tester / QA** | Replay API collections, inspect DB state after a test, read server logs | API collections with assertions and runners; read-only DB mode; log viewer |
| **Integration engineer** | Map payloads between systems, test webhooks, decode tokens | API client + devtools (JSON/XML/JWT/diff) + mock server |
| **Support engineer** | Look up a record, check a server, read a log, no code changes | Read-only everything, saved queries, fast search |

## 5. Product principles

1. **Shared context over isolated tools.** If two tools need the same concept
   (connection, environment, variable, secret, file), it lives in `hs-core` and
   both tools use it. No tool keeps a private connection list.
2. **The UI never blocks.** All I/O runs on background tasks with progress and
   cancel. A hung SSH host must not freeze the DB tab next to it.
3. **Destructive actions are guarded, not forbidden.** `DELETE` without `WHERE`,
   `git push --force`, recursive delete over SFTP and deleting an S3 prefix all
   get a confirmation that names the environment. Production environments can
   be marked read-only per tool.
4. **Portable and private by default.** One executable; every setting, every
   workspace and the encrypted secret vault live in a `HyperStackData/` folder beside
   it. No installer, no registry, no telemetry, no background services. Copy
   the folder to move or back up the whole app.
5. **Keyboard first, mouse complete.** Every action is reachable from the
   command palette and has a rebindable shortcut; every action is also reachable
   by mouse.
6. **Every visible string is translatable** (Fluent).
7. **Lightweight means lightweight.** The editor is for configs, scripts, SQL,
   JSON and quick fixes. It does not try to replace an IDE.
8. **Cross-platform core, native integration where it pays.** The core and all
   tools compile on Windows, Linux and macOS. Platform-specific code (Credential
   Manager, ConPTY, Keychain) sits behind traits in `hs-core`.

## 6. Scope

### In scope (v1.0)

- Multi-row, multi-tab workspace with splits, pinning, tab groups and saved
  workspaces.
- Tools: code editor, SQL database client, NoSQL document client (MongoDB,
  CouchDB), Redis cache viewer, API client (REST),
  SSH + SFTP, S3 browser, Git, terminal, log tail viewer (local and SSH
  sources), devtools drawer (JSON/YAML/JWT/Base64/regex/cron/UUID/hash/diff/
  timestamp).
- Shared connection profiles, environments, variables and secrets.
- Command palette, settings, themes, keyboard shortcuts, i18n.
- Automated CI and releases on the project's Forgejo; in-app update check,
  download, verification, install and revert from Settings → Updates.
- Import from Postman, Insomnia/Bruno, OpenAPI, `.pgpass`/DBeaver connections,
  `~/.ssh/config`, AWS credentials/config files.

### In scope (post-1.0, designed for now)

- Containers: Docker/Podman, Kubernetes basics.
- API client: WebSocket, GraphQL, gRPC, collection runner, mock server.
- SQL: ER diagram, schema compare, data compare.
- NoSQL: Elasticsearch/OpenSearch, DynamoDB, Cassandra; index management,
  schema inference, cross-environment collection copy.
- Redis: server dashboard, streams, pub/sub, big-key finder, MONITOR.
- Log viewer: Docker, Kubernetes, journalctl and Windows Event Log sources,
  multi-source merge, alerts.
- Server monitor dashboard over SSH.
- Plugin system (see decisions).

### Out of scope

- Being a full IDE: no debugger, no LSP in v1, no build system integration
  beyond "run a task in the terminal".
- Browser or HTML preview engine.
- Cloud accounts, hosted sync, team features on a server we run.
- Replacing Eden Explorer as a general file manager (HyperStack has a project
  tree and SFTP/S3 browsers, not a Windows shell integration).

## 7. Success criteria for v1.0

- Cold start to usable window under 1 s on a mid-range laptop; binary under
  40 MB; idle memory under 150 MB with ten tabs open.
- A developer can go from "fresh install" to "query staging DB, call staging
  API, SSH to staging box" in under five minutes using imports.
- Every tool survives its remote side disappearing without affecting other tabs.
- Zero plaintext secrets on disk, verified by a test that greps the data folder
  after a full session.
- All P0 features in [02-features.md](02-features.md) implemented; roadmap
  Phase 1 complete.

## 8. Open questions

| # | Question | Leaning |
|---|---|---|
| Q1 | ~~Share code with Eden Explorer?~~ **Decided:** no. Eden Explorer is a design reference only; every module is written fresh in this repo (ADR-014) | — |
| Q2 | Monolithic binary vs. tools as dynamically loaded plugins? | Monolith with one crate per tool; a WASM plugin API is a Phase 3 idea |
| Q3 | Should the editor get LSP support eventually? | Maybe, behind a feature flag; not before 1.0 |
| Q4 | Name of the binary? | `HyperStack.exe` on Windows, `hyperstack` elsewhere; data in `HyperStackData/` beside the executable (ADR-017) |
| Q5 | Licence? | MIT |
| Q6 | Optional AI assistant (bring your own key) for SQL, regex, shell help? | Design the hook (command palette provider) but ship nothing in v1 |
