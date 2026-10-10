# Tool spec — Redis cache viewer

**Crate:** `tool-redis` · **Priority:** P0 (core), P1 (dashboard, streams, pub/sub) · **Status:** draft

## 1. Purpose

Let developers, DBAs and SREs see what is actually in a Redis instance — keys,
values, TTLs, memory — and operate on it safely, without pasting `KEYS *` into
a production console. Covers Redis 6/7, Valkey, KeyDB, and any RESP-compatible
server (Dragonfly, Garnet) on a best-effort basis.

## 2. User stories

- As a developer I open the `staging` Redis, type `session:` and see the keys
  grouped by the next segment, click one and read the JSON inside with a TTL
  countdown.
- As a tester I delete every `cart:test-*` key before a run, with a dry-run
  count first.
- As an SRE I watch hit rate and memory on the dashboard while a deploy rolls,
  and find the three biggest keys.
- As an integration engineer I subscribe to `events.*` and watch messages
  arrive while I trigger a webhook from the API tab.

## 3. Layout

```
┌ sidebar ───────────────┬ main ──────────────────────────────────────────────────────┐
│ ● cache — staging   ▾  │ [ Keys ] [ Console ] [ Pub/Sub ] [ Info ]      db 0 ▾  ⟳    │
│ db: 0 (12,480 keys) ▾  │ ┌ user:1042:profile ─────────────────────────── string ─┐ │
│ 🔍 session:*            │ │ TTL 00:14:52 ⏱  ·  142 B  ·  encoding embstr  ·  idle 3s│ │
│ ▸ cart (1,201)          │ │ view: JSON ▾        [Edit] [Rename] [Set TTL] [Delete]│ │
│ ▾ session (8,990)       │ │ {                                                      │ │
│   ▸ 1042 (3)            │ │   "name": "Ada",                                       │ │
│     ● profile    string │ │   "plan": "pro",                                       │ │
│     ● tokens     set    │ │   "lastSeen": 1760102400                               │ │
│     ● history    list   │ │ }                                                      │ │
│ ▸ ratelimit (2,289)     │ └────────────────────────────────────────────────────────┘ │
│                         │ ┌ hash: cart:77 ───────── 24 fields ─ page 1/1 ─ 🔍 ──────┐ │
│ [+ New key]             │ │ field          │ value                        │ actions │ │
│                         │ │ sku:A1         │ 2                            │ ✎ ✕     │ │
└─────────────────────────┴────────────────────────────────────────────────────────────┘
```

- **Sidebar**: connection switcher, database selector with key counts
  (`INFO keyspace`), pattern box, lazy key tree built with `SCAN` (never
  `KEYS`) grouped by delimiter (`:` default, configurable), type badges,
  "load more" nodes beyond the page size.
- **Main**: tabs for Keys (value viewer/editor for the selected key), Console,
  Pub/Sub, Info. Each tab in the strip is one connection; the main area
  sub-tabs are internal.

## 4. Key tree

- `SCAN cursor MATCH pattern COUNT 500` on a background job; results stream
  into the tree; a cancel button stops long scans. Cluster mode scans each
  master node.
- Grouping is purely client-side on the delimiter; a group node shows the count
  of children discovered so far and a ≈ sign when the scan is incomplete.
- Flat list mode toggles grouping off. Sorting: name, type, TTL, size (size
  requires `MEMORY USAGE` sampling, opt-in).
- Context menu: open, copy key, rename, duplicate, set TTL, persist, delete,
  "delete all under this prefix…" (dry run → count → confirm by typing the
  prefix), "look up in DB tool" when the key looks like an id.

## 5. Value viewers and editors

| Type | Viewer | Editor |
|---|---|---|
| string | auto-detect JSON / text / number / hex / MessagePack / gzip-then-JSON; pretty JSON on `hs-editor` read-only with fold | `hs-editor` with validation for JSON; `SET` with `KEEPTTL` |
| hash | `DataTable` field/value, paged with `HSCAN`, filter | inline edit `HSET`, add, delete field |
| list | `DataTable` index/value, `LRANGE` paging from head or tail | push left/right, set index, remove by value, trim |
| set | `SSCAN` table | add, remove |
| zset | `ZSCAN` / `ZRANGE … WITHSCORES` table, score sort | add with score, update score, remove, range delete |
| stream | entries table (id, fields), consumer groups panel (pending, lag), `XRANGE` paging | `XADD`, `XACK`, `XDEL`, create group (P1) |
| HyperLogLog | `PFCOUNT` | `PFADD` |
| bitmap | `BITCOUNT`, bit viewer for small values | `SETBIT` |
| JSON (RedisJSON) | path tree | `JSON.SET` at path (P1) |

Every viewer shows: TTL with live countdown (pulled every 5 s, decremented
locally), `MEMORY USAGE`, `OBJECT ENCODING`, `OBJECT IDLETIME`, length/cardinality.
Values over 1 MB load the first 256 KB with a "load full" button.

## 6. Console

- Input line with completion from the `COMMAND DOCS` catalogue (cached per
  server version), argument hints, history (↑/↓, Ctrl+R search), multi-line
  paste runs sequentially.
- Output rendered per RESP type (arrays as tables, maps as key/value, errors
  red). "Open as key" on any string that is an existing key.
- Guards: `FLUSHALL`, `FLUSHDB`, `KEYS`, `DEBUG`, `SHUTDOWN`, `CONFIG SET`
  require confirmation and are refused on protected environments (override in
  connection settings). `MONITOR` is allowed with a warning banner and a stop
  button.

## 7. Pub/Sub (P1)

Subscribe to channels and patterns (`PSUBSCRIBE`), see messages with time and
channel in a live table with pause and filter; publish form; sharded pub/sub
for cluster.

## 8. Info dashboard (P1)

Refreshes every 2 s (configurable): memory used/peak/fragmentation, clients,
ops/sec, hit/miss ratio, evicted/expired keys, keyspace per db, replication
role and lag, persistence (RDB/AOF status), uptime, version. `SLOWLOG GET` and
`CLIENT LIST` tables with kill-client action (guarded). Small sparklines keep
the last 5 minutes.

## 9. Connections

`kind = "redis"` profile: host, port, user/ACL, password (SecretRef), db, TLS
(CA, client cert), mode `standalone | sentinel { master_name, nodes } | cluster
{ nodes }`, SSH tunnel, read-only flag, protected via environment. Import from
a `redis://` / `rediss://` URI.

## 10. Implementation notes

- `redis` crate with `tokio-comp`, `tls-rustls`, `cluster-async`, `sentinel`
  features; one multiplexed connection per tab for commands plus a dedicated
  connection for pub/sub and for `MONITOR`.
- All scans and pagers are Jobs with cancel tokens.
- Delimiter grouping is a pure function with unit tests (`a:b:c`, trailing
  delimiter, empty segment, Unicode).
- Bulk delete uses `SCAN` + `UNLINK` in batches of 500 with a progress Job and
  a dry-run pass first.
- Key tree and value viewers share `hs-ui::Tree`/`DataTable`; JSON view uses
  `hs-editor` read-only mode.

## 11. Keyboard

| Shortcut | Action |
|---|---|
| Ctrl+K | focus pattern box |
| F5 | rescan / refresh value |
| Ctrl+E | edit value |
| Delete | delete key (confirm) |
| Ctrl+Shift+T | set TTL |
| Ctrl+` | focus console |
| Ctrl+Enter | run console command |

## 12. Open questions

- Should `tool-redis` also serve Memcached (text protocol, far simpler)? A
  separate tiny `tool-memcached` is probably cleaner. Deferred.
- RedisJSON / RediSearch module support depth. Start with detection and raw
  commands.
