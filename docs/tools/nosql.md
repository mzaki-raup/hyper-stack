# Tool spec — NoSQL / document database client

**Crate:** `tool-nosql` · **Priority:** P0 (MongoDB, CouchDB browse/edit) · P1 (Elasticsearch/OpenSearch, DynamoDB, indexes, schema inference, import/export, CouchDB views) · P2 (Cassandra/ScyllaDB) · **Status:** implemented 2026-10-16 (P0; see §12)

## 1. Purpose

View and manipulate document and key-document stores the way DBAs and
developers actually need: browse containers, query in the store's native
language with completion, read documents as a tree or a flattened table, edit
them as JSON with the store's types preserved, and run bulk changes with a
dry-run count first. One UI, several back-ends, same environments and guards
as the SQL and Redis tools. Replaces MongoDB Compass, Fauxton, Kibana Dev
Tools and NoSQL Workbench for daily work.

## 2. Stores

| Store | Phase | Driver | Query front-end | Notes |
|---|---|---|---|---|
| **MongoDB** (4.4+, Atlas, DocumentDB, Cosmos Mongo API, FerretDB) | P0 | `mongodb` crate (tokio, rustls) | filter / projection / sort / limit form + raw filter editor; aggregation pipeline editor | Extended JSON v2 relaxed and canonical; change streams viewer (P2) |
| **CouchDB** (3.x, PouchDB Server, Cloudant basics) | P0 browse/edit, P1 views + replication | HTTP API via `reqwest` (no maintained crate) | Mango `_find` editor; `_all_docs` paging; view query form (key ranges, reduce, group) | `_rev` conflict handling first-class; attachments |
| **Elasticsearch / OpenSearch** (7.x, 8.x, OpenSearch 2.x) | P1 | HTTP via `reqwest` | Query DSL editor with completion; KQL/Lucene quick bar; `_sql` where available | hits as table/tree with `_score`; mappings, aliases, `_cat` dashboards |
| **DynamoDB** (AWS, DynamoDB Local) | P1 | SigV4 + `reqwest` (reuse S3 signer) | key-condition builder (PK/SK, GSI/LSI), filter expressions, PartiQL console | capacity units shown per operation; scan warnings |
| **Cassandra / ScyllaDB** | P2 | `scylla` crate | CQL console with completion, paged results | keyspaces/tables tree, token-aware paging |
| Firestore, Cosmos DB SQL API, Neo4j, ArangoDB, Couchbase | Idea | — | — | evaluate after Phase 2 |

## 3. User stories

- Open `shop — staging` MongoDB, expand `orders`, type `{ status: "failed",
  createdAt: { $gt: ISODate("2026-10-01") } }`, see 42 documents as a table
  with `customer.email` flattened, open one as a tree, fix a typo in a field,
  save; the tool shows the generated `updateOne` before applying.
- Build an aggregation pipeline stage by stage and see the output of each
  stage with 20 sample documents.
- In CouchDB, open a document that has conflicts, see the `_conflicts` list,
  pick the winner, delete the losers.
- Run `deleteMany({ test: true })` on staging: the dialog shows "this will
  delete 1,204 documents on *staging*", on production it is refused unless
  the environment is unprotected.
- Copy the `products` collection from `prod` (read-only) to `dev`, streamed,
  with progress and an option to anonymise email fields (P1).
- Infer the shape of a collection: field names, types, frequency, example
  values; export a JSON Schema for the API tests.

## 4. Layout

```
┌ sidebar: containers ─────┬ main ───────────────────────────────────────────────────────────┐
│ ● shop — staging  ▾      │ ▌staging▐  orders  ·  Find ▾ | Aggregate | Indexes | Schema | Stats   │
│ 🔍 filter                │ filter   { status: "failed", createdAt: { $gt: ISODate("2026-10-01") } } │
│ ▾ shop (db)              │ project  { customer: 1, total: 1, status: 1 }    sort { createdAt: -1 } │
│   ▾ Collections (14)     │ limit 50   skip 0        ▶ Run (Ctrl+Enter)   Explain   Count: 42      │
│     ● orders    1.2M     ├──────────────────────────────────────────────────────────────── │
│     ● customers 88k      │ view: [Table] [Tree] [JSON]        42 documents · 61 ms · page 1 │
│     ● products  5.1k     │ _id            │ status │ total  │ customer.email        │ createdAt │
│   ▸ Views (2)            │ 68e8…a1 (OID)  │ failed │ 129.90 │ ada@example.com       │ 2026-10-… │
│   ▸ Indexes              │ 68e8…b4 (OID)  │ failed │  40.00 │ …                     │ …         │
│ ▸ admin                  ├──────────────────────────────────────────────────────────────── │
│ ▸ History                │ Document 68e8…a1                           [Edit] [Duplicate] [Delete] │
│ ▸ Saved queries          │ ▾ { 11 fields }                                                       │
│                          │   _id: ObjectId("68e8…a1")   status: "failed"   total: Decimal128(…)│
└──────────────────────────┴──────────────────────────────────────────────────────────────── ┘
```

A tab = one container (collection / database / index / table) on one
connection, with sub-tabs for the store's operations. The CouchDB and
Elasticsearch sidebars show databases/indices directly (no nesting); DynamoDB
shows tables with their GSIs/LSIs as children.

## 5. `DocStore` abstraction

```rust
#[async_trait]
trait DocStore: Send + Sync {
    fn kind(&self) -> StoreKind;
    async fn containers(&self, scope: Scope) -> Result<Vec<ContainerInfo>>;         // dbs › collections, indices, tables
    async fn stats(&self, c: &ContainerRef) -> Result<ContainerStats>;             // count (exact/estimate), size, indexes
    async fn find(&self, c: &ContainerRef, q: NativeQuery, page: Page, sink: &mut dyn DocSink, cancel: CancelToken) -> Result<FindSummary>;
    async fn get(&self, c: &ContainerRef, id: &DocId) -> Result<Document>;
    async fn put(&self, c: &ContainerRef, doc: Document, mode: PutMode) -> Result<PutResult>;   // Insert | Replace{rev/version} | Patch(ops)
    async fn delete(&self, c: &ContainerRef, ids: &[DocId], guard: GuardToken) -> Result<u64>;
    async fn bulk(&self, c: &ContainerRef, op: BulkOp, dry_run: bool, cancel: CancelToken) -> Result<BulkSummary>;  // updateMany / deleteMany / _bulk_docs / batch-write
    async fn explain(&self, c: &ContainerRef, q: NativeQuery) -> Result<serde_json::Value>;
    fn extensions(&self) -> &dyn StoreExtensions;    // indexes, views, aliases, mappings, change streams, replication …
    fn query_language(&self) -> &dyn QueryLanguage;  // completion, validation, examples, formatting
}
```

`Document` is a typed JSON tree (`serde_json::Value` plus a side table of
store types: ObjectId, Decimal128, DateTime, Binary, UUID, Timestamp, RegEx,
Long vs Int for Mongo; `_id`/`_rev`/`_attachments` for CouchDB; `_index`,
`_score`, `_source` for Elasticsearch; attribute types S/N/B/SS/M/L for
DynamoDB). Editing round-trips through Extended JSON (Mongo), raw JSON
(CouchDB, ES) or DynamoDB JSON, with a plain-JSON "simplified" view that is
read-only when it would lose types.

## 6. Querying

- **Form + raw**: every store gets a small form for the common case (filter,
  projection, sort, limit/skip or page size) and a raw editor (`hs-editor` in
  JSON/JS-ish mode) with store-specific completion: operator names
  (`$gt`, `$regex`, `$lookup`; `bool.must`, `match_phrase`; `begins_with`),
  field names sampled from the container, enum-like values when the schema
  inference has run.
- **MongoDB aggregation**: stages as a vertical list; each stage is a JSON
  editor; "preview" runs the pipeline up to that stage with `$limit: 20` and
  shows output beside it; stages can be disabled, reordered, saved; export as
  shell, Node, Python, Rust code (P1).
- **CouchDB**: `_all_docs`/`_find` with Mango selector editor and index hints;
  view runner with start/end key, descending, reduce/group level, include_docs.
- **Elasticsearch**: Query DSL with `_search` options (size, from, sort,
  aggregations), results as hits table with `_score` and highlight, aggregation
  results as tree/table; quick bar accepts Lucene/KQL translated to
  `query_string`.
- **DynamoDB**: Query (key conditions on table or index) vs Scan (with a
  warning and projected RCU estimate), filter expression, pagination by
  `LastEvaluatedKey`; PartiQL console.
- **Paging** is cursor-based where the store supports it (Mongo cursors with
  `batchSize`, ES `search_after`/PIT, Dynamo keys, CouchDB `startkey`/
  `skip`), streamed into the view; "count" is a separate, cancellable action
  that uses estimates where exact counts are expensive.
- **Explain**: Mongo `explain("executionStats")` rendered as a plan tree with
  index usage and docs examined; ES `_validate/query?explain` and
  `_explain`; Dynamo shows consumed capacity and whether a Scan was used.

## 7. Viewing documents

- **Table**: documents flattened to dotted paths (`customer.email`), arrays
  shown as `[3 items]` with expand-in-cell, columns from the union of fields
  in the page (or from the inferred schema), type badges, nulls and missing
  distinguished, sort/filter client-side, pin columns, copy as CSV/JSON/TSV.
- **Tree**: collapsible with types and lengths, search within document, copy
  path, copy value, "filter by this value", "exclude", "add as column".
- **JSON**: `hs-editor` read-only with folding and Extended JSON toggle.
- Attachments (CouchDB) and binary fields: size, content type, download,
  image preview.

## 8. Editing and manipulation

| Action | Behaviour |
|---|---|
| Edit document | opens in `hs-editor` as Extended JSON / raw JSON; validation on save; diff of changes shown; applies as replace (with `_rev`/optimistic version where the store has one) or as a minimal `$set/$unset` patch (Mongo, configurable) |
| Field-level edit | inline in the tree for scalars; type can be changed via a menu (string → int, string → ObjectId, etc.) |
| Insert | from template (empty, copy of selected, from JSON clipboard, from inferred schema), single or batch (JSON array / lines) |
| Duplicate | copies without `_id`/`_rev` |
| Delete | selected documents, confirm with count; on protected environments requires typing the container name |
| Bulk update / delete by query | `updateMany` / `deleteMany` / `_bulk_docs` / batch-write composed in the query editor; **dry run** shows the matched count and 10 sample documents before the real run; progress Job with cancel |
| Rename / drop container, create container, create database | guarded; drop refused on protected environments |
| Indexes (P1) | list with usage stats (`$indexStats`, ES mappings), create (fields, order, unique, sparse, TTL, partial filter, text/geo/wildcard), drop; CouchDB Mango indexes; Dynamo GSIs read-only |
| CouchDB conflicts (P0) | `_conflicts` listed; side-by-side diff of revisions; pick winner, delete losers |
| CouchDB views (P1) | design document editor with JavaScript map/reduce in `hs-editor`, run with parameters, save |
| Replication (P1) | CouchDB `_replicator` docs and `_active_tasks` status; start one-shot/continuous replication |
| Change streams / `_changes` (P2) | live tail of changes in a container, feeding the log viewer |

## 9. Schema inference (P1)

Sample N documents (default 1,000, random or latest) and produce a field
report: path, types seen with percentages, presence %, min/max/length, example
values, nested arrays. Rendered as a tree with a histogram per field; export as
JSON Schema draft 2020-12 (for API tests), as a Mongo `$jsonSchema` validator,
or as TypeScript/Rust types (P2). The inferred field list powers completion
and table columns.

## 10. Import, export, copy (P1)

- Export container or query result: JSON array, JSON lines, Extended JSON,
  CSV (flattened with a chosen column set), ndjson for Elasticsearch `_bulk`.
  Streams all documents as a Job.
- Import: JSON/JSON lines/CSV with field type mapping, upsert key choice,
  batch size, error handling (skip/stop), dry run.
- Copy container between connections and environments (Mongo → Mongo,
  CouchDB → CouchDB, and cross-store JSON copy for simple documents),
  streamed, resumable by `_id` cursor, optional field transforms (drop,
  mask, rename) for anonymising production data into dev.

## 11. Connections

Profile kinds `mongodb`, `couchdb`, `elasticsearch`, `dynamodb`, `cassandra`:

| Kind | Fields |
|---|---|
| mongodb | URI (`mongodb://`, `mongodb+srv://`) with SecretRef placeholders or host list + options; auth mechanism (SCRAM, X.509, AWS IAM, LDAP), auth source, TLS (CA, client cert), read preference, default database, SSH tunnel, read-only flag |
| couchdb | base URL, auth (basic, cookie session, JWT), TLS, default database, SSH tunnel |
| elasticsearch | node URLs, auth (basic, API key, bearer), TLS/CA, cloud id, default index pattern, SSH tunnel |
| dynamodb | region, endpoint (for Local), credentials profile (shares the AWS credential import with S3), SecretRefs |
| cassandra | contact points, port, keyspace, credentials, TLS, datacenter |

Test connection button, import from URI paste, Compass connection strings,
`~/.aws` for DynamoDB.

## 12. Guards and safety

- Protected environments: read-only by default; writes need the per-tab
  toggle with confirmation; `drop`, `deleteMany` with empty filter,
  `DELETE /<db>` (CouchDB), `DELETE /<index>` (ES), `DeleteTable` (Dynamo),
  `TRUNCATE`/`DROP` (CQL) are refused.
- Every bulk operation offers a dry run first; the real run shows the exact
  matched count from the dry run and requires confirmation when it exceeds a
  threshold (default 100).
- Scans on DynamoDB and `_all_docs` on large CouchDB databases warn about
  cost/time and page conservatively.
- History records queries and write operations with document counts, never
  document contents.

## 13. Integration

- Document → editor (`OpenRequest::Text`), → API client request body, →
  devtools JSON tool; API response or SQL row → insert as document.
- Log viewer: a log line with an `ObjectId`-looking token offers "open in
  MongoDB" when a connection in the current environment has a matching `_id`
  (looked up lazily on click).
- Quick-open (`#` prefix) lists saved document queries alongside SQL ones.

## 14. Keyboard (NoSQL scope)

| Shortcut | Action |
|---|---|
| Ctrl+Enter | run query / stage preview |
| Ctrl+Shift+Enter | count |
| Ctrl+E | edit selected document |
| Ctrl+N | new document |
| Delete | delete selected (confirm) |
| Ctrl+1/2/3 | table / tree / JSON view |
| Ctrl+Shift+X | explain |
| Ctrl+Shift+I | infer schema |
| Alt+→ / Alt+← | next / previous page |

## 15. Tests

Extended JSON round-trip property tests; flattening and column union on
nested/array documents; dry-run counting; `_rev` conflict flows against a
CouchDB container; Mongo 7, CouchDB 3, OpenSearch 2, DynamoDB Local
integration via `testcontainers`; guard classifier for each store's
destructive operations; snapshot tests of the table/tree views.

## 12. What is implemented (2026-10-16)

The P0 core of this spec is in `tool-nosql` and exercised against MongoDB 7
and CouchDB 3 in containers (`HS_MONGO=127.0.0.1:27017
HS_COUCH=http://admin:…@127.0.0.1:5984 cargo test -p tool-nosql --test live`):

- **One tool, two stores.** `DocStore` (§5) is implemented by the MongoDB
  driver (`mongodb` crate) and the CouchDB driver (HTTP through `reqwest`).
  The sidebar lists both kinds of connection together, through the same
  header every other tool's sidebar uses, and a connection has at most one
  tab.
- **Containers** (§4): the tree lists databases and, under each, its
  collections with an estimated document count. A CouchDB database holds
  documents directly, so it appears as its own container.
- **Find** (§6): filter, projection and sort as JSON, with a page size and
  skip. MongoDB reads Extended JSON, so `{"_id": {"$oid": "…"}}` addresses a
  document by its real type. CouchDB uses `_all_docs` when there is no
  selector and Mango `_find` when there is. **Count** is a separate action,
  because an exact count is expensive.
- **Views** (§7): Table flattens documents to dotted paths with `_id` first,
  missing fields left blank rather than shown as null, and arrays as their
  length; Tree lists each document's paths; JSON shows them laid out.
- **Editing**: clicking a row opens the document as JSON. Save replaces it
  (CouchDB sends `_rev`, so a stale edit is refused as a conflict rather
  than overwriting), and New Document inserts one.
- **Guards** (§6 of docs/06-security.md): a read-only profile, and a
  protected environment until the tab unlocks writes, refuse every write.
  **Delete Matching** counts first and, on a protected environment, asks for
  the environment's name to be typed.
- **Aggregation** (§6): a pipeline as a JSON array, run with the page limit
  appended. MongoDB only.
- **Indexes**: name, definition and whether it is unique.

Not written yet, and left in the spec above: Elasticsearch and DynamoDB,
CouchDB views and attachments, schema inference, import and export,
change streams, and the explain plan rendered as a tree rather than JSON.
A document's fields are shown in name order: `serde_json::Value` is a
sorted map, so the order the store keeps them in is gone before the view
sees it.

## Addendum (2026-10-17): the AI assistant

The NoSQL tool has the shared review-only assistant (ADR-019), in the same
column and behind the same hard-right button as everywhere else.

- **What it is told**: the connection, the store and its version, the open
  database and container, the query on screen (filter, projection, sort,
  limit and skip — or the pipeline on the Aggregate tab) and how the last
  run went. Not the documents: the query is the thing being worked on.
- **Review only.** A filter or pipeline it writes lands in the field it
  belongs to and the user presses Run, so the tool's guards on a protected
  environment are untouched.
- Quick actions: Write a Filter, Explain This Query, Suggest an Index.
- `nosql.ai` in the palette toggles it, and Read Aloud can read its last
  answer.

## 13. Qdrant (2026-10-24)

`kind = "qdrant"` opens in this activity: a **collection** is a
container and a **point** is a document with an id, so the tree, Find,
the table/tree/JSON views, paging and the guards all serve it
unchanged.

Two things do not line up, and both change what the pane may do.

**There are no databases.** One pseudo-scope, `qdrant`, stands in so the
tree has a root.

**A point is `{id, payload, vector}`**, and a vector can be hundreds of
floats. Listing them would make the viewer unreadable and the response
large, so a scroll asks for payloads **with `with_vector: false`**.

That decides how writing works, and it is the one real hazard. Qdrant's
upsert *requires* a vector: sending back a point that was read without
one would be refused, or — worse — replace a real vector with nothing.
So a save whose document carries no vector goes to `/points/payload`,
which sets the payload and leaves the vector alone; only a document
that actually carries a vector is upserted whole. **Editing what you
can see cannot destroy what you cannot.**

Smaller things the driver is careful about:

- **Id types.** `1` and `"1"` are different points. A numeric id goes
  back as a number and a UUID as a string, including for a document
  typed by hand where only the display id is available.
- **The filter language is Qdrant's own**, not Mongo's:
  `{"must": [{"key": "author", "match": {"value": "ada"}}]}`. Each
  store now shows its own example, because a Mongo filter over a Qdrant
  box is an instruction that does not work.
- **No projection and no sort.** A scroll returns whole points in id
  order, so those two boxes are hidden rather than left to be typed
  into and ignored.
- **Indexes** are the collection's `payload_schema`. A payload index is
  not a uniqueness constraint, so `unique` is never claimed.
- **Delete Matching** counts first and always; a dry run touches
  nothing.

Verified against Qdrant 1.20.0: both seeded collections listed with
their point counts, all five points of `articles` returned with their
payload columns flattened, and the header band reporting the server
version (it moved out of the toolbar on 2026-10-26, with the container
path following it over the right column on 2026-10-27; the container
tree's reload button carries its label rather than a bare glyph).

## Dashboard (2026-10-29)

The activity's home tab wears the shape every activity now shares
(`hs_ui::dashboard::saved_dashboard`): a band with the activity's
mark, name and subtitle; the figures in equal rounded boxes;
**Start Here** with its first moves; **Saved Connections** - every store over its `host:port`;
and the shared **Open Here** grid, each section ruled apart.
