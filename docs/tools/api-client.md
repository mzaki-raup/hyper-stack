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
- **Formatting**: a JSON or XML body is laid out when it is loaded (opening
  a request, importing, an assistant block, loading from history) and when
  the body kind is switched; **Format** does it on demand and says so when
  there is nothing to do. The response body is laid out the same way for
  JSON, XML and HTML. Keys, values and numbers are never rewritten: only
  whitespace changes (`hs_core::fmt` re-indents the text rather than
  re-serialising it, because `serde_json::Value` sorts object keys).
- **Download**: the request body (JSON, XML or text) saves to a file from
  the Body tab; the response body has Save…
- **Response**: status, time, size, HTTP version, remote address; body as
  Pretty (highlighted JSON/XML/HTML/text), Raw, or Preview (pictures as
  pictures, HTML as sanitised text); headers; cookies; Save…, Copy.
- **Import/export**: Postman v2.1 collections (folders, requests, auth,
  bodies incl. GraphQL as JSON, scripts) and environments from Postman,
  Insomnia environment resources/exports, Bruno environment JSON (`vars`),
  or generic JSON objects with a `values` or `variables` map (all imported
  into a HyperStack environment). The same environment formats can also be
  imported from Settings → Environments → Variables, where they merge into
  the selected draft and require Save to persist; export a collection as
  Postman v2.1. An environment exports as Postman v2.1, Insomnia v4, Bruno
  or flat JSON; `secret:` values are written empty because the vault is
  never decrypted for an export.
- **History** (`<workspace>/api/history.jsonl`, last 200), one click
  re-opens an entry as a draft.
- **Guards**: a non-GET request against a protected environment asks
  first, naming the environment, method and URL.
- **AI Assistant**: quick actions write a **Pre-request Script**, a
  **Post-response Script**, a script that saves response values to
  environment, collection or global variables, **Generate Tests**,
  **Explain Response** and **Write Docs**; each answers with an `hs-api`
  block the user applies. The shared assistant sees the request, the last response and
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

## Addendum (2026-10-17): drag to reorder, at every level

The sidebar is a tree, so reordering happens **within a level**: a request
moves among its siblings, a folder among its siblings, a collection among
the collections. Dragging never moves an item into another folder — that
would be a different gesture with a different meaning, and the move people
want from a drag here is "put this one above that one".

- Each row carries the same six-dot grip as every other list in the app
  (`hs_ui::reorder`), so the row itself keeps its clicks and its context
  menu.
- One `Reorder` per level, keyed by collection and folder, so two levels
  on screen at once never read each other's drag.
- Items are stored in order inside the collection file, so a move is saved
  by writing that collection. Collections live one-per-file in a directory,
  which has no order of its own, so each carries an `order` field; the
  list falls back to the name when two are equal.

The folder and request context menus went through Fluent at the same time
— they had been written in English in the code, which the rest of the app
does not do.

### Across levels, and one tab per request (2026-10-17)

A row can also be dragged **out of its level into another** — out of a
folder, into one, or between a folder and its parent's siblings. Each
level names itself in the drag payload (`Reorder::scoped`), so the level
a row is dropped in knows where it came from and the tool takes it from
there and inserts it here. Dropping a folder inside itself would take the
branch out of the tree, so the item goes back where it was instead.

**Including into another collection** (2026-10-18). This was refused at
first - another collection is another file, and the move looked more like
a copy - but the owner reported it as the thing that did not work, and it
is: a request written in the wrong collection has to be able to leave it.
The move carries both ends now (`from_collection` → `to_collection`),
takes the row out of one file and inserts it in the other, and saves both;
a tab showing the moved request follows it, or it would save back into
the file it left. A **collection's own header** is a drop target as well,
so a collapsed or empty collection can be dropped into - the same reason
a folder's header is one.

**A name already in use where a row lands gets numbered**, the same way a
duplicate does: `List` arriving beside another `List` becomes `List 001`.

**Clicking a request opens it in its own window tab**, at most one per
request: the tab that already holds it is focused, a tab with nothing open
takes it rather than leaving a blank one behind, and otherwise a new tab
opens (`uri_affinity` claims an `api://` URI only while the tab is empty).
Owner's requirement, 2026-10-17.

### The method has a colour (2026-10-17)

Each HTTP method is shown in a fixed colour rather than a theme role —
**GET** green, **POST** blue, **PUT** amber, **PATCH** violet, **DELETE**
red — so the verb is picked out before the word is read. HEAD, OPTIONS and
TRACE have none of their own: they are rare enough that a colour would be
noise. The same colours are used in the sidebar, the method picker and the
**tab**, where only the method is coloured and the request's name keeps
the tab's own text colour (`TabItem::title_prefix`). Owner's requirement,
2026-10-17.

### The list is where requests live (2026-10-17)

Three paths could make a request and only one of them put it in the list,
which is why the sidebar's **New Request** button looked dead: it cleared
the editor to an unsaved draft, and nothing appeared. All three now write
into a collection on disk — the button (beside the open request, else the
first collection, offering to make one when there are none), a folder's
**New Request**, and **Duplicate** on a window tab. Each numbers its name
against the folder it lands in, so a second New Request is `New Request
001` rather than a collision.

Duplicate on a window tab used to copy the tab's state, which gave two
tabs claiming to be the same request. `Tool::duplicate_state` lets a tool
say what a duplicate *is*: the default copies the tab, and the API client
writes a numbered copy of the request into the same collection and folder
for the new tab to open.

A request is **renamed from the request itself**, in a name row above the
method and URL. The rename reaches the collection when the field is left
or Enter is pressed, so the list row and the tab title follow without a
separate Save, and a name already in use beside it is numbered.

**Dropping onto a folder** is its own kind of drop. Between-rows drops can
only reach a folder that is open and has rows to aim between, so moving a
request into a collapsed or empty one was impossible — which is what
"dragging into a folder does not work" turned out to mean.
`hs_ui::reorder::drop_onto` paints the folder as a target and reports the
source, and the folder's header suppresses the between-rows indicator so
only one is shown. Owner's requirements, 2026-10-17.

### Importers: Insomnia, Bruno, OpenAPI (2026-10-18)

The Import button takes more than Postman now (`tool-api::importers`):
Insomnia's v4 export (workspace → collection, request groups → folders,
the base environment → variables, ordered by `metaSortKey`), a **Bruno**
collection (pick any `.bru` file or `bruno.json`; the folder holding
`bruno.json` is the collection, sub-folders are folders, files are
requests in `seq` order, with `~` marking a disabled header or query), and
an **OpenAPI 3** document in JSON or YAML (one request per operation, the
first tag as its folder, `servers[0]` as a `baseUrl` variable, query and
header parameters with their examples, the first request-body example as
the body). The YAML reader is the same small subset the Devtools YAML tool
uses — the workspace carries no YAML crate, and an API description needs
no anchors or tags. Whatever a client stores that the model has no place
for is left behind rather than kept as opaque text. Postman v2.1 still
comes first, and an unrecognised file says which formats are accepted.
