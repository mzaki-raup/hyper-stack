# Tool spec — Log tail viewer

**Crate:** `tool-logs` · **Priority:** P0 (local + SSH sources, filters, parsers) · P1 (containers, k8s, journald, Event Log, merge, alerts) · **Status:** draft

## 1. Purpose

Open and follow logs from wherever they live — a local file, a file on a server
over SSH, a container, a pod, journald — in a viewer that handles multi-gigabyte
files, understands common formats, filters instantly and never freezes the UI.
Replaces `tail -f | grep` in a terminal, `less +F`, BareTail, and copying logs
to the desktop to open in an editor.

## 2. User stories

- As an SRE I tail `/var/log/app/app.log` on `web-01` and `web-02` in one
  merged view, filter to `level >= WARN`, and get a toast when `OutOfMemory`
  shows up.
- As a tester I open a 3 GB JSON-lines log, jump to 14:32 today, and expand the
  request id field for a failing test.
- As a developer I click a stack-trace path and the file opens in the editor
  at that line.
- As a DBA I tail Postgres logs and highlight `duration:` lines over 500 ms with
  a regex rule.

## 3. Layout

```
┌ sidebar ─────────────────┬ main ─────────────────────────────────────────────────────┐
│ Sources                  │ web-01:/var/log/app/app.log  ● following   1.9 GB  ⟳  ⏸   │
│ ▾ staging                │ [level ≥ WARN ✕] [regex: timeout ✕] [exclude: healthz ✕] +│
│   ● web-01 app.log  ●    │ ──────────────────────────────────────────────────────────│
│   ● web-02 app.log  ●    │ 14:32:01.118  WARN  web-01  OrderSvc  payment timeout …   │
│   ○ db-1 postgresql.log  │ 14:32:01.120  ERROR web-01  OrderSvc  java.lang.Illegal…  │
│ ▾ local                  │    at com.shop.OrderSvc.charge(OrderSvc.java:212)   ↗      │
│   ○ C:\logs\api.log      │    at com.shop.Http.handle(Http.java:88)                   │
│ ▾ docker (local)         │ 14:32:03.004  WARN  web-02  Cache    redis timeout …      │
│   ○ shop-api             │ ──────────────────────────────────────────────────────────│
│ Presets                  │ ▲ 12 new lines  · 48,201 of 9,882,100 shown · UTC ▾        │
│   errors-only            ├───────────────────────────────────────────────────────────│
│   slow-queries           │ Record 14:32:01.120   fields ▾  pin: level, service       │
│ [+ Add source]           │ { "ts": "...", "level": "ERROR", "service": "OrderSvc", … }│
└──────────────────────────┴───────────────────────────────────────────────────────────┘
```

- **Sidebar**: saved sources grouped by environment, live indicator, filter
  presets, add-source wizard.
- **Main**: header (source, follow state, size, pause), filter chips row,
  virtualised log list, status line with "new lines" badge and time zone,
  collapsible record detail pane.

## 4. Sources (`LogSource` trait)

```rust
trait LogSource: Send {
    fn open(&mut self, from: Position) -> Result<()>;      // Start | End | Offset(u64) | Time(DateTime)
    async fn next_chunk(&mut self) -> Result<Option<Bytes>>; // streaming reads
    fn supports_random_access(&self) -> bool;               // files: yes, streams: no
    async fn read_range(&mut self, start: u64, len: usize) -> Result<Bytes>;
    fn size_hint(&self) -> Option<u64>;
    fn describe(&self) -> SourceInfo;                       // label, host, path, colour
}
```

| Source | Phase | How |
|---|---|---|
| Local file | P0 | memory-mapped read, `notify` watcher for appends; rotation detected by inode/size shrink → reopen (`tail -F`) |
| Local glob (`app-*.log`) | P1 | newest file followed, rotation handled by re-glob |
| SSH file | P0 | `tail -n +<offset> -F <path>` on an exec channel for following; `SFTP` range reads for history/paging; falls back to SFTP polling if `tail` missing |
| Command stdout | P0 | any command, local or SSH (`journalctl -f -u app`, `kubectl logs -f` as a cheap path) |
| Docker container | P1 | Docker API `/containers/{id}/logs?follow=1&timestamps=1` via `bollard`; local, TCP or over SSH |
| Kubernetes pod | P2 | `kube` log stream per container; pod restart re-attach |
| journald | P1 | `journalctl -o json -f` over SSH/local |
| Windows Event Log | P1 | `windows` crate `EvtSubscribe` (local only) |
| Clipboard / paste | P0 | ad-hoc text |

## 5. Engine

- **Index**: for random-access sources, a background Job builds a line-offset
  index (`Vec<u64>` per 64 MB block, blocks indexed on demand from the end
  first so the tail appears immediately). Multi-GB files open in under a
  second; backward paging reads blocks lazily.
- **Window**: the UI keeps a bounded window of parsed `Record`s (default
  200 k) around the viewport; older records are dropped and re-parsed on
  demand from the index.
- **Follow**: new bytes are parsed incrementally; when the user is scrolled to
  the bottom the view sticks; otherwise a "▲ N new lines" badge appears and
  clicking it jumps down. Pause stops rendering but keeps reading.
- **Filters** run on a filter task over the index, producing a `Vec<LineNo>`
  of matches that the virtualised list maps onto. Changing a filter cancels
  and restarts; partial results show with a progress bar. Filter chips combine
  with AND; a chip can be negated.
- **Multi-line records**: parsers may claim continuation lines (stack traces,
  indented lines, JSON pretty-printed over lines) so a record is the unit of
  filtering and copying.

## 6. Parsers and formats

Auto-detected from the first 64 lines, overridable per source:

| Format | Fields extracted |
|---|---|
| Plain | raw line, timestamp if a leading one is found (many patterns), level if a known token appears |
| JSON lines | all fields; configurable mapping for `ts`/`time`/`@timestamp`, `level`/`severity`, `msg`/`message` |
| logfmt | key=value pairs |
| syslog (RFC 3164/5424) | pri, timestamp, host, app, pid, msg |
| Nginx/Apache combined | ip, time, method, path, status, bytes, referer, ua, request time |
| Postgres / MySQL slow log | timestamp, pid, level, statement, duration |
| Java / .NET / Python / Node stack traces | continuation grouping, file:line extraction |
| Docker/k8s timestamps prefix | strips and uses the runtime timestamp |

Parsers are pure functions with fixture tests. Level normalisation:
`TRACE DEBUG INFO WARN ERROR FATAL` from many spellings (`WARNING`, `ERR`,
`CRIT`, numeric syslog severities, Go's `warn`).

## 7. Filters, highlights, search

- Filter chips: text contains, regex, level ≥, time range, field equals
  (structured), source equals (merged view), exclude variants of each.
- Highlight rules (independent of filters): regex → colour/bold; per workspace
  with defaults for levels. Example: `duration: (\d{3,}) ms` → orange.
- Find (Ctrl+F): incremental, match count, F3/Shift+F3, context lines ±N.
- "Jump to time": binary search over the index using parsed timestamps
  (falls back to linear scan around a guess for files without time on every
  line).
- Bookmarks (Ctrl+B) with notes; shown in the scrollbar gutter.

## 8. Structured view

When a source is JSON/logfmt, the list switches to columns: time, level,
pinned fields, message. The detail pane shows the full record as a tree with
copy-as-JSON, "add field as column", "filter by this value", "exclude this
value". Field statistics (top values) on right-click (P1).

## 9. Multi-source merge (P1)

Add several sources to one view; records are merged by parsed timestamp with a
source colour tag; clock skew per source can be adjusted; sources without
timestamps are interleaved by arrival. Follow works across all of them.

## 10. Statistics and alerts (P1)

- Sparkline of records per minute per level over the loaded window; top
  messages after normalising numbers and ids; error rate.
- Alert rules: when a filter matches a new record → toast, tab badge, optional
  sound; cooldown to avoid storms. Alerts only run while the tab exists (no
  background daemon).

## 11. Actions

Copy selected records (raw or as JSON), save filtered output to a file, open
`path:line` in the editor (local path, or remote path via `sftp://` when the
source is SSH and the path exists there), "look up in Redis" for key-like
tokens, "open request" when a URL and method look like one of the API
collection's requests, send a record to the devtools JSON viewer.

## 12. Connections and persistence

Log sources are saved in the workspace folder as `logs/<name>.toml` referencing an
`ssh`/`docker`/`k8s` connection id plus path/container/unit and parser
overrides; environment binding gives them colours and protection (no writes
exist here, so protection only affects nothing destructive except
command-stdout sources, which require confirmation on protected environments).

## 13. Implementation notes

- `memmap2` for local files; `notify` for appends; `regex` with `RegexSet`
  for highlight rules; `chrono`/`jiff` for timestamp parsing with a table of
  formats; `serde_json` streaming for JSON lines.
- All source reads, indexing and filtering are Jobs; the UI only touches the
  record window and match vector.
- Row rendering caches layout per record; very long lines are truncated at
  4 k characters with an expand action.
- Tests: parser fixtures, rotation simulation (truncate, rename+create), index
  correctness on CRLF/LF/mixed, filter cancellation.

## 14. Keyboard

| Shortcut | Action |
|---|---|
| F | toggle follow |
| Space / P | pause / resume rendering |
| Ctrl+F / F3 | find / next |
| Ctrl+L | add filter chip |
| Ctrl+B | bookmark |
| Ctrl+G | jump to time |
| End / Home | newest / oldest loaded |
| Ctrl+Shift+C | copy record as JSON |
