# Tool spec — API client

**Crate:** `tool-api` · **Priority:** P0 (REST, collections, environments, importers) · P1 (scripts, runner, WebSocket/SSE, GraphQL, cookies/proxy/certs) · P2 (gRPC, mock server) · **Status:** P0 and the scripting part of P1 implemented (2026-10-15); see §0

## 0. What is implemented (2026-10-15)

- **Requests**: method, URL with `{{variables}}`, params, headers, body
  (none, JSON, text, XML, form, multipart with file parts, binary file),
  auth (inherit, none, basic, bearer, API key in header or query), docs,
  pre-request and post-response scripts. Send (Enter in the URL field or
  F5), cancel, Copy as curl, Import curl.
- **Collections and folders** as one JSON file per collection under
  `<workspace>/api/collections/<id>.json` (not `.req.toml` per request as
  §7 planned: one file imports and exports Postman cleanly and keeps a
  tree in order). Collection variables, collection and folder auth and
  scripts (inherited outermost first), collection **libraries** (pasted
  JavaScript evaluated before every script). Rename, duplicate, delete,
  New Folder, Save As into a collection or folder, filter.
- **Variables**: local (runtime) → active HyperStack environment (with its
  parents and the built-ins `{{$uuid}}`, `{{$timestamp}}`, …) → collection
  → globals (`<workspace>/api/globals.json`). `secret:` references resolve
  through the vault only when the request is built; unresolved names block
  the send with their list.
- **Scripts** in JavaScript on Boa with the `pm` API (ADR-020):
  environment / globals / collectionVariables / variables, `pm.request`
  editable before sending, `pm.response`, `pm.test`, `pm.expect`,
  `console`. A Snippets menu inserts the common patterns. Results show in
  the response's Tests and Console tabs; variable changes are saved where
  they belong.
- **Download**: the request body (JSON, XML or text) saves to a file from
  the Body tab; the response body has Save…
- **Response**: status, time, size, HTTP version, remote address; body as
  Pretty (highlighted JSON/XML/HTML/text), Raw, or Preview (pictures as
  pictures, HTML as sanitised text); headers; cookies; Save…, Copy.
- **Import/export**: Postman v2.1 collections (folders, requests, auth,
  bodies incl. GraphQL as JSON, scripts) and Postman environments (into a
  HyperStack environment); export a collection as Postman v2.1.
- **History** (`<workspace>/api/history.jsonl`, last 200), one click
  re-opens an entry as a draft.
- **Guards**: a non-GET request against a protected environment asks
  first, naming the environment, method and URL.
- **AI**: the shared assistant sees the request, the last response and
  the collection tree (names only). It can **act on the client**: its
  system prompt (`assist::system_prompt`) describes a JSON action schema
  and asks for one fenced `hs-api` block per answer; the block shows an
  **Apply** button that edits the open request and the collections exactly
  as the sidebar would (`assist::Action`: `new_collection`, `new_folder`,
  `new_request`, `open_request`, `set_name`, `set_method`, `set_url`,
  `set_param`/`remove_param`, `set_header`/`remove_header`, `set_body`,
  `set_auth`, `set_script`, `set_docs`, `set_variable`, `save`). Every
  action is attempted, failures are reported in the toast, the request tab
  of the last change is focused, and applying the same block again reuses
  the collection and request it made. Sending stays a user action. Plain
  code blocks still Insert into the body or the post-response script.

Not yet: OAuth 2 flows, AWS SigV4 and digest auth, `api` connections
(§14), the collection runner, WebSocket/SSE/GraphQL/gRPC, cookie jar,
proxies and client certificates, response diff, `pm.sendRequest`.

## 1. Purpose

A Postman-class client whose collections are plain files in the workspace,
whose environments are the same environments the DB, SSH and S3 tools use, and
which never needs an account. Optimised for the loop: edit request → send →
read response → tweak → save → commit.

## 2. User stories

- Import the team's Postman collection and environment; secrets land in the
  vault; the collection folder is committed to the repo.
- Send `POST {{api_base}}/v1/orders` against staging, see 201 in 120 ms, copy
  the `id` from the response into a saved variable, run `GET /orders/{{id}}`.
- Switch environment to `prod` (protected): a confirmation names the URL before
  any non-GET request is sent.
- Paste a `curl` from a bug report, send it, save it into the collection.
- Open a WebSocket to the notifications endpoint and watch messages while
  triggering events from the DB tab.

## 3. Layout

```
┌ sidebar: collections ────┬ main ──────────────────────────────────────────────────────────┐
│ 🔍 filter                │ ▌staging▐   POST ▾  {{api_base}}/v1/orders                 Send ▶ │
│ ▾ shop                   │ [Params] [Headers (3)] [Body ●] [Auth: bearer] [Tests] [Scripts] [Docs] │
│   ▾ auth                 │ 1 │ {                                                            │
│     POST login           │ 2 │   "customerId": "{{customer_id}}",                           │
│   ▾ orders               │ 3 │   "items": [{ "sku": "A1", "qty": 2 }]                      │
│     POST create order ●  │ 4 │ }                                                            │
│     GET  get order       ├────────────────────────────────────────────────────────────── │
│     GET  list orders     │ 201 Created · 118 ms · 1.2 KB · HTTP/2 · TLS 1.3        ⇩ Save  │
│ ▸ payments               │ [Body ▾ pretty JSON] [Headers (12)] [Cookies (1)] [Timing] [Tests ✓3] │
│ ▸ History (today 41)     │ 1 │ {                                                            │
│ ▸ Environments           │ 2 │   "id": "ord_01J9…",                                         │
│                          │ 3 │   "status": "pending"                                        │
└──────────────────────────┴────────────────────────────────────────────────────────────── ┘
```

Request and response are stacked by default; a toggle puts them side by side.
Each tab is one request (saved or unsaved draft).

## 4. Request model

See the file format in [05-data-model-and-storage.md](../05-data-model-and-storage.md).

- **Method** any standard plus custom.
- **URL** with `{{var}}`, path params `:id` (auto-listed in Params with
  values), query params table (enable/disable, bulk edit as text), URL
  encoding toggle.
- **Headers** table with completion of common names and values; inherited
  collection/connection defaults shown greyed; disable per row.
- **Body**: none, JSON (editor with validation and formatting), text, XML,
  form-urlencoded, multipart (fields and files with content types), binary
  (file), GraphQL (query + variables, P1). Body editor is `hs-editor`.
- **Auth**: inherit from folder/collection, none, basic, bearer, API key
  (header/query), digest, OAuth 1.0a (P2), OAuth 2.0 (client credentials,
  authorization code with PKCE using a local redirect listener on
  `127.0.0.1:<random>`, password, refresh; token cache per environment with
  expiry and "refresh now"), AWS SigV4 (reusing the S3 signer), NTLM (P2),
  mTLS client cert per connection.
- **Settings** per request: follow redirects, timeout, retries, SSL verify
  (flagged), HTTP version, proxy override, send cookies.
- **Docs**: Markdown description rendered in a tab; used by export.

## 5. Sending

`reqwest` client per `api` connection (connection pool, cookie jar, proxy,
TLS settings). Sending is a Job: cancellable, shows elapsed time live.
Response capture: status, version, headers, cookies set, body (streamed to a
temp file over 5 MB), timing breakdown (DNS, connect, TLS, TTFB, download),
size (compressed/uncompressed), remote IP, TLS details (cipher, cert chain
viewer). Large and binary bodies: hex view, save to file, image preview for
image types, PDF via text extraction (`pdf-extract`).

Body views: pretty (JSON/XML/HTML/YAML with folding, search, JSONPath filter
box), raw, preview (sanitised HTML text rendering, no scripts), as table (when
the JSON is an array of flat objects), diff against the previous response of
the same request (P1).

## 6. Variables and environments

Shared engine from `hs-core::environments`. Scope chain: request literal →
folder → collection → environment → parent env → workspace globals → built-ins.
Hover any `{{var}}` to see its resolved value (masked if secret) and scope.
"Set variable from response": right-click a JSON value → store as collection
or environment variable (writes to the file; secrets to the vault). Unresolved
variables block Send with an inline fix.

## 7. Collections

- Folder on disk, one file per request, `collection.toml` for metadata, order
  by `seq`. Sidebar tree with drag reorder/move (rewrites `seq`), folders with
  auth/variables/scripts inheritance, duplicate, rename, delete (to a
  `.trash/` folder for a session).
- Import: Postman v2.1 (collections + environments + globals), Insomnia v4,
  Bruno folders, OpenAPI 3.x / Swagger 2 (operations → requests with example
  bodies from schemas, servers → `api` connections, security schemes → auth),
  HAR (P1), curl (single). Postman JavaScript tests/pre-request scripts are
  translated to Rhai for the common `pm.test`, `pm.expect`, `pm.environment.set`
  patterns; untranslatable parts are kept as commented text.
- Export: Postman v2.1, OpenAPI 3 (P2), curl per request, Markdown docs.

## 8. Tests and scripts (P1)

- **Assertions** without code: status is / in range, header equals/contains,
  body contains, JSONPath equals / exists / type / length / matches regex,
  response time below, schema validation against a JSON Schema (from OpenAPI
  components when imported).
- **Scripts** in Rhai with a `req`, `res`, `env`, `vars`, `console`, `crypto`
  (hash, HMAC, base64, JWT decode) API; pre-request can mutate the request;
  post-response can set variables and add assertions. Sandbox: no I/O, step
  and memory limits, 5 s timeout.
- Results in the response "Tests" tab and the Problems dock.

## 9. Collection runner (P1)

Select collection/folder, environment, iterations, delay, data file (CSV/JSON
rows become variables), stop-on-failure, "allow protected environment" (off by
default). Runs as a Job with a live table of request/status/time/assertions and
an HTML/JSON report saved under `reports/`. CLI entry `hyperstack run
<collection> --env <name>` for CI (P2).

## 10. WebSocket and SSE (P1)

WebSocket tab: URL, headers, subprotocols; connect/disconnect; message
composer (text/JSON/binary, saved messages); message log with direction, time,
size, pretty JSON; filters; auto-reconnect option; `tokio-tungstenite`. SSE
tab: event stream list with event type and data, reconnection per spec.

## 11. GraphQL (P1)

Body type GraphQL with schema introspection on demand (cached per connection),
completion and validation from the schema, variables pane, operation picker,
explorer tree to build queries by ticking fields, subscriptions over WebSocket
(graphql-ws) (P2).

## 12. gRPC (P2)

`tonic` with server reflection or imported `.proto` files; service/method
tree; request editor as JSON mapped to protobuf; unary and server streaming;
metadata as headers; TLS.

## 13. Mock server (P2)

Start a local HTTP server from a collection or OpenAPI spec: returns saved
example responses, supports simple matching rules and templated responses
(`{{$uuid}}`, request echo), request log tab. Runs as a Job with a stop
button; port shown in status bar.

## 14. Connections

`kind = "api"` profiles: base URL, default headers, default auth, proxy, TLS
(verify, CA, client cert + key as SecretRefs), cookie jar on/off, HTTP version
preference, timeout. A collection may reference a connection; otherwise the
environment's default `api` connection applies.

## 15. Keyboard (API scope)

| Shortcut | Action |
|---|---|
| Ctrl+Enter | send |
| Ctrl+S | save request |
| Ctrl+Shift+C | copy as curl |
| Ctrl+Shift+V | paste curl as new request |
| Ctrl+Alt+E | set variable from selected response value |
| Ctrl+Shift+D | duplicate request |
| Alt+1…6 | request sub-tabs |
| Ctrl+Shift+R | run collection (runner) |

## 16. Tests

Importer fixtures for each format; URL/param/variable resolution tests;
auth signers (SigV4 known-answer tests, digest RFC vectors); Rhai sandbox
limits; integration against a local `httpbin`-style test server; snapshot
tests of the response viewer.
