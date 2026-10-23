# Tool spec — Log tail viewer

**Crate:** `tool-logs` · **Priority:** P0 (local + SSH sources, filters, parsers) · P1 (containers, k8s, journald, Event Log, merge, alerts) · **Status:** P0 implemented 2026-10-22, see §0

## 0. What is implemented (2026-10-22)

- **Sources** (`source::SourceSpec`): a local file followed like
  `tail -F` (the last 64 MB of a large file is read on open, appends are
  polled every 400 ms, a shrink or a changed file identity is a rotation
  and reading starts over - `engine::FileFollower`, tested for append,
  partial lines, truncation, rename+create and a vanishing file); a file
  on an SSH host (`tail -n <history> -F <path>` on an exec channel,
  `hs_ssh::SshClient::exec`, through a saved `ssh` connection whose host
  key is already trusted - the Logs tab does not prompt); any command's
  stdout, local (`cmd /C` / `sh -c`, stderr kept for the status line) or
  on an SSH host; and pasted text. Saved sources live in
  `<workspace>/logs/<slug>.toml` and the sidebar groups them by the
  connection's environment; filter presets in `logs/presets.toml`.
- **Engine**: one task per tab feeds parsed records into a bounded window
  (200 000 records, older ones dropped and counted); continuation lines
  (stack frames, `Caused by:`, indented lines, pretty JSON) are glued to
  the record before them; the format is detected from the first 64 lines
  unless the source pins one.
- **Parsers** (`parse`): plain (leading timestamp, level token), JSON
  lines (`ts`/`time`/`@timestamp`, `level`/`severity`, `msg`/`message`),
  logfmt, syslog (RFC 3164 and 5424, priority → level), Nginx/Apache
  combined (status → level, request time), PostgreSQL (`duration:` pulled
  out), with a container runtime's timestamp prefix stripped. Level
  normalisation covers `WARNING`, `ERR`, `CRIT`, numeric syslog
  severities and the rest; eight fixture tests.
- **Filters and highlights** (`filter`): chips for text, regex, level ≥,
  field = value, after/before a time, each negatable, combined with AND;
  highlight rules (regex → colour, bold) with a default for slow Postgres
  statements; the detail pane's field rows offer *filter by*, *exclude*
  and *pin as column*.
- **View**: a virtualised list (line, time in local or UTC, level,
  pinned columns for structured formats, message), Follow (sticks to the
  newest line, released by scrolling up or clicking a row), Pause, a
  "▲ N new lines" badge, Find (Ctrl+F, F3 / Shift+F3), Jump to Time
  (Ctrl+G, `HH:MM[:SS]` today or a full timestamp, binary search over the
  parsed timestamps), bookmarks (Ctrl+B, shown as a gutter mark and
  counted in the status line), a detail pane with the raw record, its
  fields, Copy / Copy as JSON and **Open in Editor** for a `path:line`
  found in the text, Save Filtered… to a file, Save Source, Reopen.
- Keyboard as §14: F, Space/P, Ctrl+F, F3, Ctrl+L, Ctrl+B, Ctrl+G, Home,
  End.

Not yet (P1/P2): globs, Docker/Kubernetes/journald/Event Log sources,
multi-source merge, statistics and alerts, SFTP range reads for history
beyond the tail. The eight additions commissioned on 2026-10-23 are
specified in §15; "look up in Redis" / "open request" are folded into
§15.3 rather than built separately.

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
source is SSH and the path exists there), send a record to the devtools JSON
viewer. "Look up in Redis" and "open request" are not separate actions: they
are two answers of the one **Find Everywhere** action in §15.3. Everything
that leaves the tab - copy, save, the assistant's context - passes through
the redaction rules of §15.4 first.

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

## 15. Commissioned additions (2026-10-23)

Eight items, asked for together. Nothing here changes a parser or the
engine's reading path.

They are numbered as they were proposed, which is **not** the order to build
them in: the assistant must redact before it sends anything, and both the
Problems inbox and the window diff count by the shape key. The build order is

1. **§15.4 Redaction** - no dependencies, and it closes a real gap today.
2. **§15.5 Collapse repeats** - gives everything else the shape key.
3. **§15.1 The assistant** - redacts its context with §15.4.
4. **§15.2 Problems** - deduplicates with §15.5.
5. **§15.3 Find Everywhere** - independent, but larger than the four above.
6. **§15.7 Saved investigations** - saves whatever the earlier items added.
7. **§15.6 The sparkline** - §10's chart does not exist yet, so this item
   builds it and makes it interactive in one go.
8. **§15.8 Window diff** - needs §15.5's key and §15.6's range selection.

### 15.1 An AI assistant column

Logs is the only data tool without one. It gets the same column every other
tool has (`hs_ui::ai_assistant::split_for_assistant`, the control hard right
on the toolbar, Read Aloud, the microphone, `logs.ai` in the palette), and
the same rule: **review-only**. A code block the assistant returns can be
inserted as a filter chip or a highlight rule; nothing is ever run, and the
tool has nothing to run anyway.

What it is given, and nothing more: the selected record, up to 40 records
either side of it, the active filter chips, the source's description (label,
host, parser) and the record count. The context is built **after** redaction
(§15.4), so a token in a log line does not reach a provider.

Quick actions, in the per-tool pattern:

| Action | Sends |
|---|---|
| Explain This Record | the record and its neighbours |
| Explain This Stack Trace | the record with its continuation lines |
| Why Did This Fail | the selected error and the 40 records before it |
| Write a Filter For… | the user's description plus the parser's field names |
| Summarise This Window | a sample of the filtered window, newest first |

**Write a Filter For…** is the one with a structured answer: the assistant
replies with a fenced `filter` block holding chip JSON, and an **Insert**
button turns it into real chips rather than text the user retypes. That is
the `hs-api` action-block pattern the API client already uses, narrowed to
one block type.

### 15.2 Errors reach the Problems dock tab

The bottom dock already has **Problems**, fed by editor diagnostics and SQL
errors; log errors belong in the same inbox. A highlight rule gains a
**Report to Problems** switch. A matching record posts one entry carrying the
source label, time, level, the message and the record's line number; clicking
it focuses that Logs tab and scrolls to the record.

Two things keep it from being a nuisance. Entries are **deduplicated by
normalised message** (§15.5's shape key) with a count, so a storm is one row
that climbs rather than a thousand rows. And a rule has a cooldown, as the
alerts of §10 do. Posting happens only while the tab exists; nothing runs in
the background.

This needs a sink on `Services` that any tool can write to, which is new;
Problems is editor-owned today.

### 15.3 Find Everywhere: correlate by id

The thing a standalone log viewer cannot do. A request id, trace id or
correlation id in a line is a join key into everything else in the workspace.

Detection: a structured field named `trace_id`, `request_id`,
`correlation_id`, `span_id` or `x-request-id` (case- and separator-
insensitive), or, in plain text, a selected token that looks like a UUID, a
16/32-character hex string or a ULID.

**Find Everywhere** on that token asks, in parallel, each as a Job:

| Where | What is matched |
|---|---|
| Every other open Logs tab | the token in the raw record |
| Saved log sources not open | skipped; offered as "open and search" |
| API client history | request/response headers and bodies |
| SQL query history | the statement text |
| Redis | the token as a key, and as a member of a key named after it |

Results land in a panel under the record detail: grouped by tool, with the
time, a one-line preview and a jump that opens the right tab through
`OpenRequest`. This replaces §11's separate "look up in Redis" and "open
request" ideas, which were the same action seen from two ends.

### 15.4 Redaction rules

Logs carry bearer tokens, passwords, keys and personal data, and **Save
Filtered… writes a plaintext file**. The project's rule is that secrets never
touch disk in plaintext (`docs/06-security.md`), so this is closer to a gap
than a feature.

A rule is a regex and a replacement, kept per workspace in
`logs/redaction.toml`. Defaults ship for `Authorization: Bearer …`, a JWT
shape, `password=`/`passwd=`/`pwd=` values, AWS access key ids and secrets,
`api[-_]?key=` values, and e-mail addresses. Each rule can be turned off.

Where it applies, always: **Copy**, **Copy as JSON**, **Save Filtered…**, the
assistant's context (§15.1), and a Problems entry (§15.2). In the view it is
a toggle, **Mask Secrets**, default on, so what is on screen matches what
would leave the tab. The raw bytes in the window are never rewritten: masking
happens on the way out, so turning the toggle off needs no reload.

Tested the way the vault is: write a redacted file, then grep it for the
fixture's secrets.

### 15.5 Collapse repeats

A **shape key** normalises a message: numbers, UUIDs, hex ids, quoted
strings and paths become placeholders, so `user 4821 not found` and
`user 9 not found` share a shape. Consecutive records with the same shape and
level fold into one row showing the newest record, a count, and the time span.
Expanding shows them individually. The fold is a view concern: filters, find
and export keep seeing every record.

The normaliser is shared with §10's "top messages" statistic and §15.2's
deduplication, so it is written once and tested once.

### 15.6 The sparkline becomes the navigator

§10 adds a per-minute chart. On its own it is decoration. Made
interactive it is the main way to move around a large window:

- **Drag a range** on it to set `after` and `before` chips, replacing the
  pair already there.
- **Click a bucket** to jump the list to the first record in that minute,
  the same jump Ctrl+G performs.
- The chart draws from the **filtered** window, so it answers "when did this
  happen" rather than "when was anything logged".

### 15.7 Saved investigations

Sources (`logs/<slug>.toml`) and filter presets (`logs/presets.toml`) persist
separately today. What is worth reopening after an incident is the whole
set-up, so an **Investigation** saves a source reference, the filter chips,
the highlight rules, the pinned columns, the bookmarks with their notes, the
time zone and whether Follow was on. They live in `logs/views/<slug>.toml`
and get their own sidebar section under Presets. Saving one is "Save
Investigation…" on the toolbar; opening one restores the tab as it was.

### 15.8 Diff two time windows

Did the deploy change anything? Pick range **A** and range **B** (dragged on
the sparkline, §15.6), and the tool counts records per shape key (§15.5) in
each and shows the difference: shapes only in B, shapes only in A, and shapes
whose count moved by more than a threshold, sorted by the size of the move.
Each row filters the list to that shape in the chosen range.

The comparison is a count table, not a text diff, so `hs-git`'s Myers diff is
the wrong tool here; Devtools' diff view stays where it is.

### 15.9 What this does not add

No background daemon, no writing to a log source, no network calls of its own
beyond the assistant provider and the sources already supported. Protected
environments are unaffected except where they already are: a command-stdout
source still asks for confirmation.
