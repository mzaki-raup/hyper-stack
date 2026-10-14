# 05 — Data model and storage

**Status:** draft · **Last updated:** 2026-10-10

## 1. Portable first: one data folder beside the executable

HyperStack is portable by default (ADR-017). Everything the app writes lives
in **one directory**, resolved once at startup:

1. `--data-dir <path>` command-line flag, or
2. `HYPERSTACK_DATA` environment variable, or
3. **`HyperStackData/` next to the executable** when that location is writable
   (the normal case: a tools folder, a USB stick, an extracted zip), or
4. a per-user fallback only when the executable's folder is read-only
   (`%LOCALAPPDATA%\HyperStack`, `$XDG_DATA_HOME/hyperstack`,
   `~/Library/Application Support/HyperStack`), with a one-time notice.

No registry keys, no `%APPDATA%`, no `%TEMP%` (temporary files go to
`HyperStackData/tmp/`), no files in the user's home. Deleting `HyperStackData/`
is the factory reset; copying the folder is the backup. The folder is named
`HyperStackData` rather than a generic `data` so it stays recognisable when the
executable sits in a shared tools folder next to other programs' files.

### Three kinds of state

| Kind | Where | Format | Shareable? |
|---|---|---|---|
| **Personal / machine state** — settings, themes, shortcuts, history, caches, known hosts, window geometry, logs | `HyperStackData/` root | TOML + SQLite | No (but travels with the folder) |
| **Workspace state** — connections, environments, API collections, saved queries, snippets, tab groups, layout | `HyperStackData/workspaces/<name>/` by default, or any folder the user chooses (e.g. `.hyperstack/` in a repo) | TOML / JSON files, one object per file | Yes (commit it) |
| **Secrets** — passwords, tokens, keys, passphrases | `HyperStackData/vault.hsv` encrypted with a master password (OS keyring opt-in) | binary, authenticated encryption | Never in plaintext |

### The data folder

```
HyperStack\
├── HyperStack.exe
└── HyperStackData\
    ├── settings.toml            # includes which workspace to open and the secret-store choice
    ├── shortcuts.toml
    ├── themes\custom-*.toml
    ├── vault.hsv                # encrypted secrets (+ vault.hsv.bak)
    ├── vault.dpapi              # optional: master password wrapped by Windows DPAPI for this user+machine
    ├── known_hosts              # OpenSSH format
    ├── workspaces.toml          # registry of known workspace folders (relative paths when inside HyperStackData\)
    ├── workspaces\
    │   └── default\             # built-in workspace, created on first run (layout below)
    ├── history.sqlite           # queries, requests, commands, recent files, job log (optionally encrypted)
    ├── cache\                   # icons, schema caches, S3 listings, OpenAPI specs, response bodies
    ├── tmp\                     # scratch files; cleared on start
    ├── logs\hyperstack.YYYY-MM-DD.log
    ├── update\                  # staged executable during an update
    └── instance.lock            # single-instance lock (PID), never a registry key or named mutex
```

Rules that keep the folder movable:

- Paths inside `HyperStackData/` are stored **relative** to it. External paths are
  stored as given, and may use the tokens `${DATA_DIR}` and `${EXE_DIR}` so a
  key file on the same USB stick keeps working on another machine.
- The SQLite databases use WAL mode with `synchronous=NORMAL` and are
  checkpointed on exit, so removable media stays consistent.
- The app detects well-known cloud-sync folder paths (OneDrive, Dropbox,
  Google Drive) and warns once about possible conflicts.
- Several isolated copies can coexist on one machine (different folders or
  `--data-dir`), each with its own lock file.
- Linux and macOS behave the same way when the binary's folder is writable
  (tarball, AppImage next to a `HyperStackData/` folder); app bundles and `/usr/bin`
  installs hit the per-user fallback.

### Workspace folder

Default location `HyperStackData/workspaces/<name>/`; the same layout is used when the
user points a workspace at a repo's `.hyperstack/` folder:

```
<workspace>/
├── workspace.toml               # id, name, created, default environment, protected envs
├── icon.png                     # optional workspace logo (icon.<ext>); travels with the folder
├── environments/
│   ├── dev.toml
│   ├── staging.toml
│   └── prod.toml
├── connections/
│   ├── db/orders-staging.toml
│   ├── ssh/web-01.toml
│   ├── s3/assets.toml
│   ├── api/shop-api.toml
│   └── docker/local.toml
├── api/
│   └── shop/                    # one collection = one folder (Bruno-style)
│       ├── collection.toml
│       ├── auth/login.req.toml
│       └── orders/create-order.req.toml
├── queries/
│   └── orders-staging/top-customers.sql   # front-matter header for metadata
├── snippets/*.toml
├── tab-groups/*.toml
├── layout.json                  # last layout; gitignored by default (template .gitignore written)
└── .gitignore                   # layout.json, *.local.toml
```

Files that end in `.local.toml` override the same-named file and are gitignored,
so a team can commit `staging.toml` while each member keeps a `staging.local.toml`
with their own username.

A workspace is the project boundary. The workspace menu in the title bar
creates a new workspace (optionally seeding its `environments/` and
`connections/` from any existing workspace chosen in the dialog, and giving it
a logo), switches between them — saving the leaving workspace's layout and
restoring the entered one's tabs and pinned connections — and deletes one, with
an option to remove its folder. The built-in `default` workspace cannot be
deleted, and a delete only ever removes files inside
`HyperStackData/workspaces/`.

A workspace logo is an image file stored in the workspace folder as
`icon.<ext>` (PNG, ICO, JPG, BMP, GIF, WEBP or SVG). Because it lives inside
the portable folder, copying the workspace to another machine carries its logo.
The menu and the selected workspace switcher button show it. Settings →
Workspace can change or remove the icon after creation and rename any registered
workspace except the built-in `default`; renaming updates workspace metadata and
the registry without moving its folder.

## 2. Core entities

### 2.1 Identity

Every entity has a stable `id` (ULID, 26 chars) so renames do not break
references, and a human `name`. Secrets are referenced by `SecretRef`, which is
`"secret:<ulid>"` and resolves through the secret store.

### 2.2 Connection profile

```toml
# connections/db/orders-staging.toml
id        = "01J9ZX4K7Q9M8N0P1R2S3T4U5V"
name      = "orders — staging"
kind      = "db"                      # db | ssh | s3 | api | docker | k8s | redis | mongo
folder    = "Shop/Staging"
tags      = ["shop", "staging"]
colour    = "#3B82F6"
environment = "staging"               # optional default environment binding
read_only = false

[db]
engine   = "postgres"                 # postgres | mysql | mariadb | sqlite | mssql | clickhouse | duckdb
host     = "{{db_host}}"              # variables allowed in any string field
port     = 5432
database = "orders"
user     = "{{db_user}}"
password = "secret:01J9ZX5…"          # SecretRef, or "{{db_password}}" resolved from env (which may itself be a SecretRef)
ssl      = "prefer"                   # disable | prefer | require | verify-ca | verify-full
ssl_ca   = ""                         # path
options  = { application_name = "hyperstack", statement_timeout = "30s" }

[tunnel]                              # optional
ssh      = "01J9ZX6…"                 # id of an ssh connection
```

Kind-specific tables:

| Kind | Fields |
|---|---|
| `ssh` | host, port, user, auth = `password \| key \| agent \| interactive`, key_path, passphrase (SecretRef), jump = [ssh ids], keepalive, startup_command, default_remote_dir, terminal profile overrides |
| `s3` | endpoint, region, access_key, secret_key (SecretRef), session_token (SecretRef), path_style (bool), provider hint (`aws \| minio \| r2 \| b2 \| spaces \| wasabi \| other`), default bucket |
| `api` | base_url, default headers, auth block (same schema as request auth), proxy, tls (verify, client cert), cookie jar on/off |
| `docker` | transport = `local \| tcp \| ssh`, host, ssh = id, tls paths |
| `k8s` | kubeconfig path, context, namespace |
| `redis` | host, port, user, password (SecretRef), db, tls, cluster (bool), sentinel |
| `mongo` | uri (with SecretRef placeholders), tls, auth source |

### 2.3 Environment

```toml
# environments/staging.toml
id        = "01J9ZX7…"
name      = "staging"
colour    = "#F59E0B"
protected = false                      # true → confirmations + read-only defaults
parent    = "01J9ZX8…"                 # optional: inherit variables from another env (e.g. "base")

[variables]
db_host     = "staging-db.internal"
db_user     = "app"
db_password = "secret:01J9ZX9…"
api_base    = "https://staging.api.shop.example"
bucket      = "shop-assets-staging"

[defaults]                             # which connection each tool uses when "current environment" is selected
db     = "01J9ZX4K…"
ssh    = ["01J9ZXA…", "01J9ZXB…"]
s3     = "01J9ZXC…"
api    = "01J9ZXD…"
docker = "01J9ZXE…"
```

### 2.4 Variable substitution

Syntax `{{name}}`; scopes resolve innermost first. Environment-aware text
fields (`hs_ui::varfield`) open an autocomplete when the caret enters an
unclosed `{{`: the active environment's variables are fuzzy-matched, a key is
picked with the mouse or the arrow keys plus Enter, and a brand-new variable
can be defined inline (key and value) and is written straight to the active
environment. The SSH connection form uses it for host, user, remote directory
and startup command; other tools adopt it as they are built.

The Environments settings page can also copy variables between environments
with "Copy Variables From…": it reads any environment in any workspace (the
current workspace's live, others loaded from their folder), compares the
source keys against the environment being edited, marks each as new or
already-present, and copies the multi-selected keys in (missing keys ticked by
default, existing values left untouched unless re-ticked).

```
request / query literal  →  collection vars  →  environment  →  parent environment  →  workspace globals  →  built-ins
```

Built-ins: `{{$uuid}}`, `{{$ulid}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`,
`{{$randomInt(1,100)}}`, `{{$env:HOME}}` (OS env), `{{$file:path}}`,
`{{$secret:<ulid>}}`. Unresolved variables are highlighted red in every editor
and block execution with a "define it now" quick-fix. Secret-backed variables
display masked everywhere except an explicit "reveal" click, and are redacted
from logs and history.

### 2.5 API collection and request

```toml
# api/shop/orders/create-order.req.toml
id     = "01J9ZXF…"
name   = "Create order"
method = "POST"
url    = "{{api_base}}/v1/orders"
seq    = 20                                   # ordering inside the folder

[[params]]
key = "dryRun"; value = "true"; enabled = false

[[headers]]
key = "Content-Type"; value = "application/json"

[auth]
type   = "bearer"                              # none | inherit | basic | bearer | apikey | oauth2 | awsv4 | digest
token  = "{{access_token}}"

[body]
type = "json"                                  # none | json | text | xml | form | multipart | binary | graphql
text = '''
{ "customerId": "{{customer_id}}", "items": [ { "sku": "A1", "qty": 2 } ] }
'''

[[tests]]                                      # P1
name   = "created"
assert = "status == 201"
[[tests]]
name   = "has id"
assert = "json.id exists"

[scripts]                                      # P1, Rhai
pre  = ''' env.set("ts", now()); '''
post = ''' if res.status == 201 { env.set("order_id", res.json.id); } '''
```

`collection.toml` holds collection-level variables, auth, default headers and
an optional OpenAPI source for re-import.

### 2.6 Saved query

```sql
-- hyperstack: { "id": "01J9ZXG…", "name": "Top customers", "connection": "01J9ZX4K…", "params": ["since"] }
SELECT customer_id, sum(total) FROM orders WHERE created_at > {{since}} GROUP BY 1 ORDER BY 2 DESC LIMIT 50;
```

Plain `.sql` with a one-line JSON header so it stays usable by other tools.

### 2.7 Tab group and layout

```toml
# tab-groups/morning-checks.toml
name = "Morning checks"
[[tabs]]
tool  = "db";  state = { connection = "01J9ZX4K…", query_file = "queries/orders-staging/errors-last-24h.sql" }
[[tabs]]
tool  = "ssh"; state = { connection = "01J9ZXA…" }
[[tabs]]
tool  = "api"; state = { request = "01J9ZXF…" }
```

`layout.json` is the serialised `TabSpace`: tab list (factory id + `save_state`
JSON), active id, split tree, sidebar/dock sizes, active environment.

## 3. History database (SQLite)

Tables (all with `workspace_id`, `created_at`, `environment_id`):

| Table | Columns |
|---|---|
| `query_history` | connection_id, sql (secrets redacted), duration_ms, row_count, status, error |
| `request_history` | request_id?, method, url (resolved, secrets redacted), status, duration_ms, size, response_path (cache file for bodies < 5 MB) |
| `command_history` | host_id?, cwd, command, exit_code |
| `recent_items` | kind, ref, title, last_opened |
| `jobs` | tool, title, status, started, finished, log_path |
| `transfer_history` | source_uri, target_uri, bytes, status |

Retention: configurable (default 90 days / 50 k rows per table). `VACUUM` on
startup when more than 20 % free.

## 4. Import sources

| Source | What we read | Notes |
|---|---|---|
| Postman v2.1 collection + environment JSON | folders, requests, auth, variables, tests (translated where possible) | most common ask |
| Insomnia v4 export, Bruno folders | same | Bruno's `.bru` grammar is simple |
| OpenAPI 3.x (JSON/YAML), Swagger 2 | servers → api connection, operations → requests with example bodies, schemas kept for completion | re-sync keeps user edits via `seq` and id mapping |
| curl command | single request | from clipboard |
| `~/.ssh/config` | hosts, users, ports, identity files, ProxyJump | watch and offer re-import |
| `~/.aws/credentials` + `config` | S3 profiles | secrets go straight to the vault |
| DBeaver `data-sources.json`, `.pgpass`, `.my.cnf`, DataGrip (partial) | DB connections | passwords are not importable from DBeaver's encrypted store; prompt |
| Docker contexts, kubeconfig | docker / k8s connections | |
| `.env` files | environment variables | option to mark keys matching `/(pass|secret|token|key)/i` as secrets |

Exports: whole workspace folder (already files), Postman collection JSON,
OpenAPI from a collection (P1), CSV/JSON/SQL from results.

## 5. Migrations

Every TOML file carries `schema = N` at top level. `hs-core::storage` runs
forward migrations on load and writes the new version; a backup copy
`<file>.bak-<schema>` is kept once. SQLite uses numbered migration scripts.
Because a portable folder may be opened by an older executable (two sticks,
two versions), files written by a newer schema are opened read-only with a
clear message instead of being downgraded silently.
