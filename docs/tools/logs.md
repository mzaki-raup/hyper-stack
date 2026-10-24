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
| **Thread Dump (Java)** | one record per thread: `thread`, `id`, `daemon`, `prio`, `os_prio`, `cpu`, `elapsed`, `tid`, `nid`, `activity`, `state`, `state_detail`, `top`, `frames`, `locked`, `waiting_for`, `kind` |
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

### 15.10 Three reports from the field (2026-10-23)

Found while the additions above were being built, all three on a
`journalctl -f -u docker` source over SSH.

- **Auto withheld the log.** `engine::ingest` collected lines into a sample
  and fed the view nothing until sixty-four had arrived. A followed source
  that is quiet never reaches sixty-four, so the tab looked empty, and
  picking any parser by hand fixed it because that path fed immediately.
  Lines are shown at once now with the best guess so far; while the sample
  fills, a changed verdict re-reads it (`Shared::reparse`, bounded by
  `SAMPLE_LINES`). Once the sample is full the parser is fixed as before.
- **A journalctl line kept its level to itself.** The payload of a syslog
  envelope is usually logfmt - `time="…" level=info msg="…"` is what every
  Go daemon under systemd writes. The level was never read, so the row had
  no colour and `level ≥ WARN` did not match it.
  `parse::absorb_embedded_logfmt` reads those pairs for the Syslog and
  Plain parsers: `level` sets the level, `msg` becomes the message, `time`
  is preferred over the envelope's timestamp, and the rest become fields.
  Two pairs is the bar, so prose with one equals sign in it is left alone.
  The list still shows the raw line for these formats; only the level
  column and the filters change.
- **There was no dashboard.** Logs had a home tab but not
  `home_is_dashboard`, so it showed the empty state. It now shows what the
  activity has: saved sources, filter presets and open tabs as counts,
  Start Here actions, the saved sources as rows that open them, and the
  shared Open Here previews. As the Devtools home tab learned the hard way,
  it reports **no** `current_uri` and **no** `uri_affinity`, or it would
  take every `OpenRequest` for the activity.

## 16. The sidebar and Running Service (2026-10-23)

The column is three stacked sections, each scrolling on its own so a long
list never pushes the others off: **Saved Logs** at about 40% of the
height, **Running Service** at 30%, and **Filter Presets** with the rest.

**Saved Logs is a flat list the user arranges.** It used to be grouped by
the environment of each source's connection, which said the same thing
twice: the environment is chosen in the title bar and every tab already
follows it. Rows carry a drag grip, as every other activity's list does
(`hs_ui::reorder`), and the order lives on each source's own file
(`SavedSource::order`), so a move rewrites only the files that shifted and
a source saved before this reads 0 and keeps its alphabetical place until
something is moved. The context menu offers Open, **Edit Source…** and
Delete; Edit opens the Add Source form filled in, keeps the source's place
and removes the old file when the name changes.

**Clicking a saved log opens it in its own tab, and only one.** A log that
is already open is focused instead of opened twice. `TabInfo` carries the
`current_uri` of each tab so any tool can ask that question.

**Running Service** lists what is running on this machine so a log can be
opened without knowing where it lives (`services.rs`). It asks the process
list, Docker, Podman, systemd and kubectl, matches each executable or
image against a table of well-known servers (nginx, Apache, Caddy, Tomcat,
JBoss, WildFly, Jetty, Quarkus, IIS, Kestrel, Node, gunicorn and friends,
PostgreSQL, MySQL, Redis) and offers the paths that server conventionally
writes to. Adding a server is a row in `KNOWN`, not code elsewhere.

Three rules keep it honest:

- **It never starts, stops or configures anything.** Every probe is a
  read, and a runtime that is not installed contributes nothing.
- **A guessed path is checked before it is offered.** A server with no
  file here is offered as the command that follows it instead
  (`docker logs -f`, `journalctl -f -u`, `kubectl logs -f`), and one with
  neither opens the form on the file picker.
- **The scan is a Job.** It shells out, so it runs on a thread and the
  result arrives through a channel; the UI thread never waits. The first
  scan happens when the tab is first drawn, not at start, so a workspace
  that never opens Logs pays nothing.

Clicking a row opens the Add Source form pre-filled, and saving it puts
the service in Saved Logs. **It then drops off Running Service**: it
belongs to the saved list now, and the two should not show the same thing
twice (owner's request).

### 15.9 What this does not add

No background daemon, no writing to a log source, no network calls of its own
beyond the assistant provider and the sources already supported. Protected
environments are unaffected except where they already are: a command-stdout
source still asks for confirmation.

## 17. Thread dumps (2026-10-24)

Owner's request. A JVM thread dump (`jstack`, `kill -3`, a crash log) is a
**Thread Dump** source rather than a wall of plain text.

**One record per thread.** The thread header starts a record and every
frame, monitor and state line below it folds in, so a dump of 400 threads
is 400 collapsible rows rather than 8,000 lines. Nothing is dropped: the
records put back together are the file again, which
`tests/thread_dump.rs` asserts directly.

**The fields a dump is searched by come from below the header.** A thread's
authoritative state (`java.lang.Thread.State: BLOCKED (on object monitor)`)
and its monitors (`- locked <0x…>`, `- waiting to lock <0x…>`) are printed
*under* the header, so `parse::absorb_continuation` reads them as those
lines are folded in. `locked` and `waiting_for` hold the same address for
the holder and the waiter, which is what makes contention findable: filter
on one address and both threads appear.

**State is a field, not a level** — a waiting thread is not a warning, and
pretending otherwise would make every dump look alarming. The two
exceptions get a level, and therefore a colour, because they are what a
dump is opened to find: `BLOCKED` threads and the `Found one Java-level
deadlock` section are **ERROR**.

**A quoted name is not always a header.** The deadlock section is full of
references — `"worker-8":`, `which is held by "worker-7"` — and reading
those as headers invented threads that do not exist and split the section
apart. `parse::is_thread_header` therefore requires one of the JVM's own
attributes (`tid=`, `nid=`, `prio=`, `daemon`, `#<id>`) and not merely a
leading quote.

**Detection is by presence, not proportion.** Most of a dump's lines are
stack frames, so no 60% ratio test could ever pass; the VM line, a
`tid=`/`nid=` header or a `java.lang.Thread.State:` line anywhere in the
sample is conclusive. A plain log that happens to contain a stack trace
still reads as Plain.

**Repeated dumps stay separate.** `jstack` on a loop writes many snapshots
into one file; each `Full thread dump` line starts its own record, as does
the leading timestamp, both tagged `kind=snapshot`.

**Getting one in.** **Open Thread Dump…** sits beside **Open File…** in
the toolbar, the dashboard and the empty state, and reads the file as a
dump whatever its extension; **Add Source** takes a pasted dump with the
same format picker; **Auto** recognises one either way.

### 17.1 The dump's summary bar

A dump of 400 threads is unreadable as a list, so a **Thread Dump**
source gets its own summary between the filter chips and the list. It
appears for a dump whatever picked the format — the bar reads
`Shared::format`, the format the detector settled on, not the source's
saved `Auto`.

**A count per state, commonest first, with `BLOCKED` always leading** —
three blocked threads out of four hundred is the reason the dump was
taken, so they are never sorted to the bottom, and `BLOCKED` is the one
state painted in the error colour because it is a problem rather than a
fact. A thread whose record was cut off mid-dump counts as `UNKNOWN`
rather than disappearing.

**Every count is a filter.** Clicking `BLOCKED 3` adds a
`state = BLOCKED` chip, which is an ordinary field chip: negatable,
combinable and savable as a preset like any other.

**A deadlock banner follows the cycle into the threads it names.**
`parse::deadlock_cycle` reads the section into `DeadlockEdge { waiter,
monitor, holder }` entries, and each end is a button that adds a
`thread = <name>` chip — the next question after "there is a deadlock"
is always "show me those two threads". An entry is reported only once
the waiter, monitor and holder are all known, so a section the parser
does not recognise yields nothing rather than a half-read edge.

The summary is derived from every record, so it is computed when
`Shared::version` moves rather than every frame.

### 17.2 Not yet

Non-JVM dumps (Go's goroutine dump, .NET's `dotnet-dump`) are a
different grammar and are not covered. The cycle is followed by thread
*name*, which is what the JVM prints; two threads sharing a name would
both match.

## 18. Four reports from the field (2026-10-24)

**A row's icon says what the thing is.** `services::icon_for` maps a
server kind to a glyph - a database for MySQL/PostgreSQL/Mongo, a globe
for nginx/Apache/Caddy/IIS, a stack for an application server, a queue
for Kafka/RabbitMQ, a shield for Keycloak, the JS and Python file marks
for Node and gunicorn - and the host's own glyph is only the fallback.
Saved Logs rows get the same treatment through `saved_icon`, which
recognises the server from the path or follow command the user typed,
using the same `KNOWN` table; without that a saved MySQL log and a
running one looked like different kinds of thing.

**A runtime's log lives beside the application, not beside the
runtime.** `node D:/shop/server.js` has no conventional log path of its
own, which is why the configure form always opened empty for Node.
`services::app_home` takes the first argument that is a real file and
uses its directory, `logs_under` looks for `logs/`, `log/`, `var/log/`
or the directory itself, and the app's own directory is the fallback -
not a made-up filename, which would only fail to open.

**The lists are shared, so every tab shows the same ones.** Each tab
used to load `logs/*.toml` once when it was created, so another tab's
save, rename, reorder or delete was invisible to it - and because
Running Service is the scan *minus* what is saved, a service saved in
one tab stayed on the other tab's running list. A module-level
`LISTS_VERSION` is bumped by every writer and compared by every tab in
`sync_lists`; the files stay the state, since loading them is cheap and
every tab already knows how.

**A refused log file says what to do.** MySQL and PostgreSQL on Windows
keep their logs under `ProgramData` with an ACL that admits only the
service account and the administrators group, so a normal user's
process gets "Access is denied. (os error 5)" and no idea what to do
about it. `engine::explain_open` keeps the real error - it is the ground
truth - and adds the remedy: grant the account Read, start as an
administrator, or follow the server's own log through a Command source.
A missing file asks whether the server logs to a file at all. Any other
error is passed through untouched, because inventing advice for an
error we have not seen is worse than the error.

## 19. The M5 additions built on 2026-10-24

§15.2, §15.6, §15.7 and §15.8 as built. §15.3 is still pending and §19.5
says what it is waiting on.

### 19.1 The time strip (§10, §15.6)

`chart.rs` is pure arithmetic with the drawing in `ui::time_strip`.

**The bucket follows the span.** A minute is right for an incident and
useless for a week - 10 080 bars in a 300 px strip is one bar per
thirty-three pixels of nothing - so `Bucket::pick` returns the smallest
*round* unit (1 s, 5 s, 15 s, 30 s, a minute, 5, 15, 30, an hour, 3, 6,
12, a day, a week) that keeps the bar count at or under `TARGET_BARS`.
Only round units: a bucket of "37 seconds" would make the axis
unreadable.

**It counts the filtered window**, so it answers "when did this happen"
rather than "when was anything logged", and it is rebuilt when
`Shared::version` or the chips version moves, never per frame.

**A gap stays a gap.** Empty buckets inside the span are kept: a log that
stopped for ten minutes should look like it stopped.

**A bar takes the colour of its worst level**, because one error in a
minute of chatter is the thing worth seeing, and a bar is never thinner
than a pixel so a lone record is not rounded away.

**Records with no timestamp are reported, not hidden** - "N without a
time" beside the strip. Silently dropping half a plain log would be a lie
about the window.

Clicking a bar jumps to the first record in it (and releases Follow,
which would otherwise scroll straight back). Dragging selects a range;
the drag is measured from `press_origin`, not from where egui first
reports the drag, or the range is clipped by the drag threshold. **Filter
to Range** replaces the after/before pair already there rather than
adding a second one, which would filter to nothing and read as a bug.

### 19.2 Compare Windows (§15.8)

Set the dragged range as **A** or **B**, then Compare. `diff_windows`
counts shapes in each and reports what appeared, what went away and what
moved, biggest move first with ties broken by shape so the table does not
reshuffle under the pointer. A shape that appeared or vanished survives
any threshold - it is the most interesting row there is. A record with no
timestamp is in neither window. Clicking a row filters to that shape in
the window it is interesting in; the shape key is normalised text, so the
longest surviving literal is used as the contains chip.

This needed a fix in `shape::key` first: there is no word boundary
between a digit and a letter, so `\b\d+\b` never matched the `31` in
`31ms` and `took 31ms` and `took 904ms` were two shapes. Measurements
with a unit attached are now normalised, which the repeat collapse of
§15.5 benefits from equally.

### 19.3 Investigations (§15.7)

`views.rs`, `logs/views/<slug>.toml`, its own sidebar section. The source
is kept **by value**, not as a reference to a saved source: an
investigation has to survive that source being renamed or deleted, which
is exactly what happens while someone tidies up after an incident.
Reopening restores the chips before the source is opened, so the filter
is in place before the first records arrive. Follow is off by default.

### 19.4 Problems (§15.2)

`hs_core::problems` is the inbox, on `Services` because editor
diagnostics and SQL errors belong in the same one; the dock's Problems
tab had been a placeholder.

- **Deduplicated by shape with a count**, so a storm is one row that
  climbs. The caller supplies the shape, because only the caller knows
  how to normalise its own text.
- **A cooldown per source and shape**, per rule, defaulting to ten
  seconds. `post` returns false when it refuses, so the caller can stop
  building entries.
- **Capped at 500 rows**, oldest out first.
- **A closing tab takes its problems with it** (`clear_from`): a row that
  jumps to a tab which no longer exists is worse than no row. Its
  cooldown is forgotten too, so reopening reports immediately rather than
  looking broken.
- Reporting is **off on every rule by default**. A highlight is a way of
  seeing while reading; turning all of them into inbox entries would make
  the inbox useless. A tab with no reporting rule skips the scan
  entirely.
- A match is redacted on the way out (§15.4) - the inbox is outside the
  tab - and each record is considered once however many frames it
  survives. Turning a rule on does not post the backlog.
- The jump goes by `current_uri`, not by tool: two tabs of one activity
  are two different places. It uses `GOTO_MIME` rather than a `File`
  request, which would reopen the source and lose the place, and it
  scrolls by **line number** rather than by position, because the window
  drops its oldest records.

### 19.5 Find Everywhere (§15.3) and the searcher registry

Built on the same day, once the registry it needed existed.

**The problem.** A request id in a log line is a join key into the rest
of the workspace, but ADR-004 forbids a tool depending on another tool
crate. The API client's `history.jsonl` and the SQL `query_history`
table belong to those tools; reading them from `tool-logs` would couple
the tools through a file format instead of an interface.

**The registry.** `hs_core::search` holds a `SearchRegistry` on
`Services`. Each tool implements `Searcher` - an id, a Fluent label key
and a blocking `search` - and the binary registers them in `bootstrap`,
because only the binary knows them all. The asking tool queries the
registry and never learns who answers; the same registry serves the
other direction.

Decisions worth keeping:

- **An error is not an empty list.** `search` returns a `Result`, so
  "could not look" and "found nothing" stay different answers and the
  panel can say which.
- **One searcher per store.** Registering twice under an id replaces,
  rather than searching the same store twice and letting start-up order
  decide which wins.
- **A panicking searcher becomes a failed answer**: one broken store
  must not hide the other three.
- **Paths are resolved at search time**, never captured. A searcher is
  registered once for the life of the app, so capturing
  `<workspace>/api/history.jsonl` would leave it reading the previous
  workspace's requests after a switch.
- **Excluding a source is not skipping a searcher.** `ask_all` skips a
  whole searcher by id; `Query::exclude` skips one of its sources. The
  asking Logs tab excludes *itself* that way, so the other Logs tabs
  still answer - the record on screen is not a useful search hit.
- **Answers come back in a stable order**, so the panel does not
  reshuffle between runs.

**The three searchers.** `tool-logs::searcher` keeps a process-wide
registry of open tabs, holding each tab's record window **weakly**: a
closed tab's window drops as it should, and the searcher skips the dead
entry rather than keeping a tab's worth of records alive for ever.
`tool-db::searcher` reads `query_history` read-only, scoped to this
workspace - another workspace's run is not the same incident - with the
token passed as a parameter and LIKE wildcards escaped, or a token
containing `%` would match everything. `tool-api::searcher` looks in the
headers first (where a correlation id is usually minted), then the URL,
the query and the body, and says which in the row; a disabled header
was never sent, so it is not a match.

**Detection.** A field named like a correlation id
(`hs_core::search::is_id_field`: `trace_id`, `x-request-id`,
`correlation_id` and friends, compared ignoring case, dashes and
underscores), else the first token in the line that
`looks_like_id` accepts - a UUID, 16 or more hex characters, or a ULID.
Deliberately strict: offering to search four stores for the word
"timeout" would be noise.

**Redis is not wired up.** The spec asks for the token as a key and as a
member of a key named after it, which needs a live connection and the
driver `tool-redis` owns. The registry is the hard part and it is done -
adding Redis is now a `Searcher` impl in that crate and one line in
`bootstrap`, with the open question being which connection to use, since
searching every configured Redis unasked would be a surprise.

## 20. Terminal escapes are stripped at ingest (2026-10-24)

A great many programs colour their output when they think a terminal is
watching, and a container keeps those bytes in its log for ever.
portainer writes its level as `ESC[32mINF ESC[0m`, which the viewer
showed literally as `[32mINF`.

This is not only ugly. **The escapes sit between the timestamp and the
level, which is exactly where every parser looks**, so a coloured line
was an unparsed line too: the Level column was empty for a log that
names its level on every row. Stripping fixed the display and the
parsing in one move, which is how it was found.

`ansi::strip` runs in `Shared::push_line` — the one choke point every
source passes through, pasted text, local file, SSH and command alike —
and again over the sample `pick_format` detects from, because a JSON
line behind a colour code does not start with `{` and the whole log
would be read as plain. It handles CSI (colour, cursor movement, erase),
OSC strings ended by either BEL or ST, and the charset selectors, and
consumes an escape that a tail cut in half rather than printing its
innards.

Two deliberate limits:

- **It strips rather than renders.** Showing the colour would be a
  different feature; the records carry text.
- **It never guesses.** A line that merely contains the *text* `[90m`
  with no escape byte in front of it is data and is left exactly as it
  is — a test holds that. A clean line is borrowed, not copied, so the
  cost on the overwhelming majority of lines is one scan for a byte
  that is not there.

One consequence worth stating: a record's `raw` is now the line with its
escapes removed, so the §17 reassembly invariant reads "the records put
back together are the file again, as the file would be without its
terminal escapes".
