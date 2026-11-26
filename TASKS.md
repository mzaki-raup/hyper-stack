# HyperStack — work ledger

Every unit of work has an **id**, lives under exactly one **task version**, and
carries its own **checklist**.

```
/dev-task v0.7.1       every item under that task version
/dev-task FEAT-0062    that one item, and nothing else
```

## Task versions are not app versions

A heading here is **my own batch label** — a bag of work I want done together.
It is not the application's release number, it does not have to match it, and
nothing in this file moves `[workspace.package] version`. The app's number is
decided by the release pipeline from the Conventional Commit types on `main`
(`docs/09-ci-cd-and-updates.md`), whatever this file is called.

So `## v0.7.1` here means "task batch v0.7.1". A batch may ship across two
app releases, or three batches may ship in one. Where it is worth recording
which release carried a batch, the heading gets a `> **Shipped in:**` note
after the fact.

Companion files, and what each is for:

| File | Holds |
|---|---|
| **TASKS.md** (this file) | what is to be done, by id and by task version, with its checklist |
| `CHANGELOG.md` | what was done, one line per change, under Unreleased until the app releases |
| `CHECKLIST.md` | how far the roadmap phases have got |
| `docs/07-roadmap.md`, `docs/10-depth-programme.md` | why, and in what order |

> **Built 2026-10-31.** `v0.7.3` and `v0.7.2` are the live depth-programme
> work; `v0.7.1` down to `v0.0.1` were reconstructed from the `mynotes`
> backlog, one item per horizontal rule, and carry no tick boxes — see
> [Reconstructed history](#reconstructed-history) for why.

---

## Format

### Ids

`FEAT-0001` · `BUG-0001` · `CR-0001` — four digits, allocated once, **never
reused and never renumbered**, because commits, branches and `CHANGELOG` lines
point at them.

| Prefix | Means |
|---|---|
| `FEAT` | new capability the user can see |
| `BUG` | behaviour that contradicts its spec or crashes |
| `CR` | change request: a deliberate change to existing behaviour that is neither new capability nor a defect (a changed default, a renamed label, a tightened guard, a dropped option) |

The next free number per prefix:

```bash
grep -oE '^### (FEAT|BUG|CR)-[0-9]{4}' TASKS.md | sort | tail -5
```

### Task version headings

```
  ## sprint-11 — Planned
  > **Shipped in:** app v0.108.0, 2026-11-04
```

(Indented here only so this example isn't itself matched as a heading — real
headings start at column 0.)

- The label is mine to choose: `v0.7.1`, `v3`, `sprint-11`, `2026-11`. One
  token, no spaces. Keep one style across the file so sorting stays sane.
- Status is `Planned`, `In-Progress`, or `Done <YYYY-MM-DD>` — "done" meaning
  every item in the batch is `Completed`, not that the app released. A
  reconstructed batch whose finish date nobody recorded says `Done` with no
  date rather than inventing one.
- The optional `> **Shipped in:**` line records which app release carried the
  batch, filled in afterwards. The task version is **never** renamed to match
  the app's number, and `[workspace.package] version` is never edited by hand.
- Unscheduled work sits under `## Backlog`. An item belongs to exactly one
  heading; moving it means moving the whole `###` block.

### An item

```markdown
  ### FEAT-0012 · SQL Server driver
  - **State:** Pending · **Spec:** docs/tools/database.md · **Phase:** C
  - **Branch:** feat/depth-c-sql-engines
  - **Depends on:** FEAT-0011
  - [ ] <step>
  - [ ] <step>
  - [ ] Test: <what it asserts>
  - [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
  - [ ] App: `--data-dir ./tmp-data` — <what to click>
  - [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
  - **Notes:** <anything that would otherwise be lost>
```

Rules that keep the file honest:

- **State** is `Pending`, `In-Progress`, `Completed` or `Blocked` — the same
  vocabulary as `CHECKLIST.md`, plus `Blocked`. `Completed` only once every box
  is ticked, with one exception: when I close something with boxes still open,
  the boxes **stay open** and the heading or the item's **Notes** says I closed
  it and what went unchecked. The State is my call; the boxes are the record of
  what was actually verified, and nothing may tick them on my say-so.
- The last four boxes (**Test**, **Gate**, **App**, **Docs**) are on every
  item **that is still to be done**. A box is ticked only when that check
  actually ran and passed. A check that cannot be made on this machine (no
  server here) is left unticked with the reason in **Notes** — never ticked
  optimistically.
- `Blocked` always has a reason in **Notes**.
- **Depends on** names ids. A batch runs dependencies first.
- `BUG` items get a reproducing test before the fix, so their **Test** box is
  the first thing that goes green.
- Optional fields: **Spec**, **Phase**, **Branch**, **Depends on**, **Source**
  (where the request came from), **Kind** (`major`, on the items that moved the
  minor digit) and **Verified** (what the `Completed` claim rests on).

### Reconstructed history

Everything from `v0.7.1` down to `v0.0.1` was reconstructed on 2026-10-31 from
the `mynotes` backlog — one item per horizontal rule in that file, newest at
the top, which is where the version ladder comes from (a patch step per rule
from the bottom up, a minor bump for each item marked **Kind: major**).

Those items carry **State**, **Source** (the `mynotes` line range and how many
requests it held), **Scope** and **Verified** — and deliberately **no boxes**.
Their `Completed` state is the owner's record that the work shipped, not a
check any session re-ran; boxes exist to be ticked by whoever does the work,
and ticking 112 of them after the fact would make every tick in this file
worthless. Finish dates were not recorded, so those headings say `Done` with
no date.

`FEAT-0004`, `BUG-0001` and `CR-0001` were format examples in the file's first
draft, deleted the same day. `BUG-0001` and `CR-0001` were then given to real
items — a stub that never stood for any work is not an id worth retiring.
`FEAT-0004` stayed unused, so the `FEAT` run has a gap there: a gap, not a
renumbering.

### Commits

Reference the id in the footer so history maps back to the ledger:

```
fix(tool-db): keep the schema cache across a reconnect

Refs: BUG-0003

```
### Latest Registered Task

```
Version : v0.9.0
Features : FEAT-0132
Bugs : BUG-0077
Change Request : CR-0103
```

---

## v0.9.0 — Planned

> **Analytics (BI)**, Metabase first. Spec `docs/tools/analytics.md`,
> decision ADR-034, depth-programme phase Q. Specified 2026-11-20 at the
> owner's request ("Yes, Metabase first"); the owner's answers are the
> spec's §10: Tableau Server, Metabase by password and API key, read-only,
> Tableau's interactive views in the Browser activity, Open in Database on
> the matching connection, the rail after Database.

### FEAT-0126 · Analytics activity: crate, provider trait, connections, Settings page, dashboard
- **State:** In-Progress · **Spec:** docs/tools/analytics.md §3, §4.1, §5, §8 · **Phase:** Q
- **Source:** owner, 2026-11-20: "I plan to add Business Intelligent / Analytic as new Activity … For starter, i want to add metabase and tableau. If provide credentials, can the work pane resemble both the activity web interface (using APIs, etc)?", then "Yes, Metabase first"
- **Scope:** `crates/tools/tool-analytics/` with `AnalyticsFactory` (id `analytics`, category Data, rail after Database); the `BiProvider` trait and `Caps`; connection kind `metabase` (`is_bi_kind`, the `[bi]` table) with API Key or Username and Password in the vault and Use Saved Credential; Settings → Analytics (list left, form right, Test Connection, the limits); the dashboard (servers, dashboards, questions, open tabs; Start Here; Saved Servers; Recent with Clear History; Open Here); the sidebar; one tab per server with the header band, the toolbar (Browse toggle first, AI hard right), the hideable browse column and the item page; the Metabase mark; version gating v0.48+; the `metabase` service in the bench's `bi` profile, seeded; the palette commands and shortcuts of §9
- **Depends on:** —
- [x] Crate, factory, rail entry, connection kind, profile table, Settings page
- [x] Provider trait, `Caps`, session runner (bounded channels), sign-in both ways, version gating
- [x] Dashboard, sidebar, tab shell, header band, mark
- [x] Bench: Metabase in the `bi` profile, seeded (Sample Database, a question per chart kind, a native question with a template tag, a two-tab dashboard with a filter, an API key)
- [ ] Test: profile round trip, version gating; live sign-in both ways gated on `HS_METABASE`; the data-folder grep
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test
- [x] App: `--data-dir ./tmp-data` — add the bench Metabase both ways, Test Connection, open its tab; the *Checking a screen* list
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS · bench/README
- **Notes:** 2026-11-26 screen check on the bench Metabase v0.52.4, scratch data folder: Settings → Analytics (list, form, vault-stored secret, limits), the tab signed in by **API key** and by **password** ("Tested version."), Test Connection ("Connected. Metabase v0.52.4 · …"), the dashboard, the vault-locked and "nothing answered" states and the reconnect on unlock. It found one bug: Test Connection stayed disabled because `conn_test::testable` did not list `metabase` (fixed, in the test list). Not done: the mock-server test and the live `HS_METABASE` sign-in test; the Metabase mark is a drawn placeholder, not the official logo. The bench's Metabase seed and the Tableau REST mock are on `chore/bench-kit`.

### FEAT-0127 · Analytics: the collection tree and search
- **State:** Pending · **Spec:** docs/tools/analytics.md §4.2 · **Phase:** Q
- **Source:** owner, 2026-11-20 (as FEAT-0126)
- **Scope:** collections a level at a time (folders first, then items, by name; each kind's coloured picture), personal collections, the Databases section, a filter field, Search over `/api/search` with kind filters and More
- **Depends on:** FEAT-0126
- [ ] Tree and search
- [ ] Test: tree and search results from recorded answers; live walk
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test
- [ ] App: `--data-dir ./tmp-data` — walk the bench collections, search, hide and resize the column
- [ ] Docs: spec · README · CHANGELOG

### FEAT-0128 · Analytics: questions, models and metrics with tables and charts
- **State:** Pending · **Spec:** docs/tools/analytics.md §4.3, §4.4 · **Phase:** Q
- **Source:** owner, 2026-11-20 (as FEAT-0126)
- **Scope:** Run as a Job with cancel; Chart | Table | SQL; `DataTable` with Metabase's formatting; charts through `egui_plot` (ADR-034: line, area, bar, row, combo, scatter; pie, scalar, smart scalar, progress, gauge hand-painted; funnel and waterfall phase 1b; map, pivot, sankey as the table with Open in Browser); parameters as pick and date fields; CSV / JSON / XLSX export; the SQL view with Open in Database; the AI assistant's context
- **Depends on:** FEAT-0127
- [ ] ADR-034's `egui_plot` added; `cargo deny`
- [ ] Run, table, charts, parameters, export, SQL view
- [ ] Test: each chart kind's points from a recorded result; parameters into the request; formatting by `base_type`
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test
- [ ] App: `--data-dir ./tmp-data` — every bench question's chart beside Metabase's own; a template tag; an export
- [ ] Docs: spec · README · CHANGELOG

### FEAT-0129 · Analytics: dashboards with their grid, tabs and filters
- **State:** Pending · **Spec:** docs/tools/analytics.md §4.5 · **Phase:** Q
- **Source:** owner, 2026-11-20 (as FEAT-0126)
- **Scope:** the 24-column grid at the pane's width, each card a tile with its chart or table and its own states; text, heading and link cards; dashboard tabs; filters from the parameter values with Apply; cards run four at once, cancelled when the tab leaves; auto-refresh while on screen
- **Depends on:** FEAT-0128
- [ ] Grid, tabs, filters, card runs
- [ ] Test: the grid's layout at two widths; parameter mappings into each card's request
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test
- [ ] App: `--data-dir ./tmp-data` — the bench dashboard beside Metabase's own, a filter applied, both tabs
- [ ] Docs: spec · README · CHANGELOG

### FEAT-0130 · Analytics: databases and ad-hoc SQL through Metabase
- **State:** Pending · **Spec:** docs/tools/analytics.md §4.6, §6 · **Phase:** Q
- **Source:** owner, 2026-11-20 (as FEAT-0126)
- **Scope:** the databases and their metadata; New SQL with completion from the metadata, Run through `/api/dataset`, a quick chart; refused without native access; disabled on a protected environment
- **Depends on:** FEAT-0128
- [ ] Databases, SQL page, protected-environment guard
- [ ] Test: the native request body; the refusal
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test
- [ ] App: `--data-dir ./tmp-data` — a query against the Sample Database; a protected environment disables the page
- [ ] Docs: spec · README · CHANGELOG

### FEAT-0131 · Analytics: the Tableau provider
- **State:** Pending · **Spec:** docs/tools/analytics.md §4.8, §5.2 · **Phase:** Q
- **Source:** owner, 2026-11-20 (as FEAT-0126)
- **Scope:** kind `tableau` for **Tableau Server** (owner, 2026-11-20); username and password or a Personal Access Token into one site; projects, workbooks, views, data sources; a view's PNG with filter fields, its data in a `DataTable`, exports; extract refresh tasks and jobs, read-only (no Refresh Now: the owner chose read-only); Open Interactive in the Browser activity; version gating from `/serverinfo`
- **Depends on:** FEAT-0129
- [ ] Provider and screens, read-only
- [ ] Test: a mock from the REST API reference; a read-only live test gated on `HS_TABLEAU` (the owner's Server)
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test
- [ ] App: `--data-dir ./tmp-data` — the owner's Tableau Server: browse, a view with a filter, its data, the extract tasks; Open Interactive in the Browser activity
- [ ] Docs: spec · README · CHANGELOG · GOTCHAS

## v0.8.2 — In-Progress

> Two more from the owner, 2026-11-26: the Credentials page's cells on
> one middle line, and JSON laid out wherever it is previewed.

### BUG-0076 · Settings → Security → Credentials: the text, the Copy button and the masked value are not on one middle line
- **State:** In-Progress · **Spec:** .claude/rules/ui.md (the layout standard: one middle line)
- **Source:** owner, 2026-11-26: "In Settings -> Security -> Credentials; WHy in the text, button and masked value not verticle middle align?"
- **Scope:** a row's plain labels (name, type, port) sat at the top of a row the Copy and eye buttons made taller; the Host/URL, Username, Password and Action cells are `form::centred_row`, as the plain cells are now, so every cell of a row shares the buttons' middle line
- [x] Test: n/a beyond the gate (layout), checked on screen
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-26: fmt, clippy, deny, the touched crates' tests and the self-test pass; the whole workspace's run is blocked here by AdGuard resetting loopback connections (`tool-automation`'s mock tests fail 2-8 of 10 at random, on the unmodified tree too; GOTCHAS: Windows Environment and Build) until `Adguard Service` is stopped
- [x] App: `--data-dir ./tmp-data` — Settings → Security → Credentials, rows with a host, a user and a masked password — 2026-11-26: the name, type, host, username and masked password of each row on the Copy and eye buttons' middle line (the text sat about 6 px high before)
- [x] Docs: CHANGELOG

### FEAT-0132 · JSON laid out in every preview, and Format Document for JSON
- **State:** In-Progress · **Spec:** docs/04-ui-design.md
- **Source:** owner, 2026-11-26: "In All Json Preview (Scratchpad, Editor, S3, etc), pretify the json or add pretify json button to pretify json preview"
- **Scope:** the Markdown preview (Scratchpad, the Editor, S3's `.md`) lays out a ```` ```json ```` block that parses, and a note or file that is itself a JSON object or array, once at parse time; S3's text preview lays out a `.json` object that parses (JSON Lines and anything that does not parse are shown as stored); the Editor's Format Document lays out a JSON document with the built-in formatter, one undoable edit, so it works with no language server, and says so when it is already laid out or does not parse. Key order is kept; only whitespace changes (`hs_core::fmt::pretty_json`)
- [x] Test: `json_in_a_preview_is_laid_out`, `a_json_object_previews_laid_out_and_the_rest_as_stored`, `format_document_lays_out_json_without_a_language_server`
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — as BUG-0076's: all but the whole-workspace run, blocked by AdGuard
- [ ] App: `--data-dir ./tmp-data` — a note with a minified json block and a note that is only JSON in Scratchpad; a minified `.json` in the Editor (Format Document); a `.json` object in S3's preview — 2026-11-26: both Scratchpad notes preview laid out and coloured; an unindented multi-line `.json` in the Editor was laid out by Shift+Alt+F with no language server (a one-line file was already laid out on open, as before). Not seen: S3's preview (the bench S3 is stopped), the Editor's "already formatted" and "does not parse" toasts
- [ ] Docs: CHANGELOG · README — CHANGELOG only
- **Notes:** `hs-editor` gains a direct `hs-core` dependency for `pretty_json` (hs-ui already brings hs-core in; no cycle).

### BUG-0077 · cargo deny fails: cookie, url and kube-client have advisories
- **State:** In-Progress · fixed 2026-11-26
- **Source:** the gate for this batch, 2026-11-26: `cargo deny check` reported RUSTSEC-2026-0594 (`cookie` 0.18.2: private-cookie nonces can repeat in forked processes), RUSTSEC-2026-0592 (`url` 2.5.8: stack overflow in `Url::origin()` on nested `blob:` URLs) and RUSTSEC-2026-0600 (`kube-client` 4.2.0: the `openssl-tls` connector trusts OpenSSL's default CA store) - a result that does not depend on the branch (testing.md)
- **Scope:** none is reached here. HyperStack builds `kube` with `rustls-tls` only and OpenSSL is not in the dependency tree; it uses no private cookies and forks no process; it calls `Url::origin()` on no `blob:` URL. The fixes are patch releases, so `Cargo.lock` moves `cookie` 0.18.3, `url` 2.5.9 (with `form_urlencoded` 1.2.3) and `kube` / `kube-client` / `kube-core` 4.2.1 (kube pins its siblings, so `kube` moves with them); no manifest and no call site changed
- [x] Test: `cargo deny check` passes (advisories, bans, licenses, sources)
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — deny and the touched crates' tests pass; the workspace's loopback mock tests need AdGuard stopped (GOTCHAS: Windows Environment and Build)
- [x] Docs: CHANGELOG

## v0.8.1 — In-Progress

> The owner's list of 2026-11-20: projects columns in Automation and
> Tracker, Tracker's search builder and sub-tickets, remote Docker, Db2
> over ODBC, reusing a saved credential, the log INFO colour, Markdown
> tables, Jasypt, and three small alignments.

### BUG-0075 · Devtools → Jasypt: encrypt and decrypt do not match Jasypt
- **State:** Completed · **Spec:** docs/tools/devtools.md (Jasypt)
- **Source:** owner, 2026-11-20: "In Devtool Activity -> Jasypt; the ecryptor and decryptor is wrong" — a Spring config with `PBEWithMD5AndDES` and `PBEWITHHMACSHA512ANDAES_256`, 1000 iterations, `RandomSaltGenerator`, `RandomIvGenerator`, base64
- **Scope:** Jasypt's `StandardPBEByteEncryptor` sizes the salt and the IV to the cipher's block size: 8 and 8 for DES, 16 and 16 for AES-256. The tool used an 8-byte salt and a 16-byte IV for both, so neither a DES nor an AES message from Java decrypted, and ours did not decrypt in Java. Sizes follow the algorithm; the IV generator (Random / None) replaces the free byte counts
- [x] Test: known answers made by Java 21's SunJCE for both algorithms, failing before the change — `both_algorithms_match_java` (decrypt and seal against Java's bytes), `des_without_an_iv_generator_drops_the_iv_bytes`, `round_trips_and_refusals`. The old code could not read them by construction (an 8-byte salt for AES, a 16-byte IV for DES); it was not run against them before the change
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [x] App: `--data-dir ./tmp-data` — decrypt the Java known answers, encrypt and decrypt back — 2026-11-20: Java's AES-256 and DES answers both decrypt to `jdbc-password` with password `hyperstack`; the IV Generator dropdown and the new hint read whole
- [x] Docs: spec · CHANGELOG · GOTCHAS

### CR-0102 · The INFO level wears a readable blue, not the accent
- **State:** In-Progress · **Spec:** .claude/rules/ui.md (one set of log-level colours)
- **Source:** owner, 2026-11-20: "All log consoles; Change the INFO level text color from accent color to blue color that suit in dark and light mode"
- **Scope:** the shared log-level colour for INFO (and its aliases) becomes a blue readable on the dark and the light theme, in every view that colours log levels
- [x] Test: the shared function gives INFO the blue in both themes, not the accent — `info_is_a_blue_not_the_accent_in_both_themes`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [ ] App: `--data-dir ./tmp-data` — INFO in Logs and one other console, dark and light — 2026-11-20: the Logs activity's INFO words and time strip in the blue on the dark theme; the light theme and a second console not looked at
- [x] Docs: spec · CHANGELOG
- **Notes:** every console colours levels through `hs_ui::loglevel::colour` (CR-0077), so one change covers them; the light-theme look is still to do.

### BUG-0074 · Markdown previews draw tables without borders, header or alignment
- **State:** In-Progress · **Spec:** docs/04-ui-design.md
- **Source:** owner, 2026-11-20: "All markdown preview (Scratchpad, Editor, S3, etc); the table have no border, header and data cannot be differentiated. Also didn't follow the alignment"
- **Scope:** every Markdown preview draws a table with cell borders, a header row set apart (the table header fill, bold), and each column's `:---` / `:---:` / `---:` alignment
- [x] Test: the alignments parsed from the delimiter row reach the cells — `a_table_keeps_its_alignments_header_and_rows`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [ ] App: `--data-dir ./tmp-data` — a table with all three alignments in Scratchpad and the Editor's preview — 2026-11-20 in Scratchpad: borders, the header on its fill under a stronger rule, Left / Centre / Right columns aligned so; the Editor's and S3's previews share `hs_editor::markdown` and were not opened
- [x] Docs: CHANGELOG · GOTCHAS

### BUG-0071 · Tracker: the detail column's key and summary sit off the middle line
- **State:** Completed · **Spec:** docs/tools/tracker.md
- **Source:** owner, 2026-11-20: "Tracker activity - Detail Column - Header Text (Key and Summary) must vertically middle"
- [x] Test: n/a beyond the gate (layout), checked on screen
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [x] App: `--data-dir ./tmp-data` — a ticket's header, key and summary on one middle line — 2026-11-20: "#3 Backups slow on Sunday" and "#6 hstest sub-sub: tell the team" on the bench Redmine, the key on the summary's middle line (`centred_row_wrapped`)
- [x] Docs: CHANGELOG

### BUG-0073 · Settings → Security → Credentials: Host/URL's Copy button off its line
- **State:** Completed · **Spec:** docs/06-security.md
- **Source:** owner, 2026-11-20: "the copy button for Host/URL column is not in same vertical line with Username and Password copy button"
- [x] Test: n/a beyond the gate (layout), checked on screen
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [x] App: `--data-dir ./tmp-data` — the three Copy buttons on one vertical line — 2026-11-20: every Host/URL Copy button in one column, beside the Username and Password columns' own; the Identity connections listed too
- [x] Docs: CHANGELOG

### CR-0103 · Scratchpad: the pin a little smaller in the tab, a little bigger in the list
- **State:** Completed · **Spec:** docs/tools/scratchpad.md
- **Source:** owner, 2026-11-20: "Scratchpad -> the pin icon a little bit smaller in tab label, and a little bit bigger in Note list"
- [x] Test: n/a beyond the gate (sizes), checked on screen
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [x] App: `--data-dir ./tmp-data` — a pinned note's tab and list row — 2026-11-20: the tab's pin at 82 % of the icon size, centred in its slot, a step smaller than the note icons beside it; the list row's pin at 13 px (was 11)
- [x] Docs: CHANGELOG

### BUG-0070 · Containers: a remote Docker host fails over TCP and over SSH
- **State:** In-Progress · **Spec:** docs/tools/containers.md §10.3a
- **Source:** owner, 2026-11-20: "docker was not found on PATH. Command: docker -H tcp://192.168.50.204:2375 version …" and "this host is reached through 01M4MRR1PHN2DBF6B2R30DF86X. Command: docker version …", and the words not in sentence case
- **Scope:** an SSH host runs the engine's commands on that host through its saved SSH connection (the error named the connection's id: the SSH path was never wired): the runner `hs_container::run::set_remote`, installed by the binary over `Tunnels::exec` (the SSH service's shared session, 120 s cap), Test Connection included. A TCP host still runs the local client (§10.3a: the driver is the CLI), so the TCP form says the client is needed here and warns when it is not on PATH. Every engine error starts with a capital; the Host and Port labels, which read `conn-host` / `conn-port`, have their strings
- [x] Test: an SSH host's invocation runs through the SSH exec path; the TCP path without a local client; the error wording — `a_remote_invocation_runs_through_the_runner` (the line with `DOCKER_API_VERSION=` before it), `a_remote_failure_reads_as_a_local_one` (127 as not installed, a daemon refusal as unreachable, an SSH failure in words), `the_errors_read_as_sentences`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [ ] App: `--data-dir ./tmp-data` — a remote engine over TCP and over SSH lists its containers — 2026-11-20: the TCP form's hint and "The docker client is not installed on this computer (not on PATH)." seen on this machine (no client here); no engine over SSH on the bench, so the SSH path is proven by the tests only
- [x] Docs: spec · CHANGELOG · GOTCHAS
- **Notes:** a TCP host with no local client could be reached without one through the Engine API over HTTP; that is a second driver (§10.3a deferred `bollard`), not this fix. The owner's LAN host over SSH is the check this item still needs.

### BUG-0072 · Database: Db2 over ODBC fails with both IBM drivers
- **State:** In-Progress · **Spec:** docs/tools/database.md
- **Source:** owner, 2026-11-20: `C_DB2_clidriver115` "SQL1042C … SQLSTATE=58004", `C_DB2_clidriver` "IM003 … system error 126 … C:\DB2\clidriver\bin\db2clio.dll"
- **Scope:** both are recorded in GOTCHAS (2026-11-01): IBM's `db2cli install -setup` registers a `db2clio.dll` the package lacks; the 11.5 driver's encrypted login fails with `SQL1042C`. Find what can be done on our side, and say each failure's remedy in the connection error
- [x] Test: the error mapping for both failures — `the_two_known_db2_failures_name_their_remedy`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [ ] App: `--data-dir ./tmp-data` — the bench Db2 through ODBC — not run: the bench Db2 (heavy profile) is stopped, port 50000 closed
- [x] Docs: spec · CHANGELOG · GOTCHAS
- **Notes:** 2026-11-20 on this machine: `HKLM\SOFTWARE\ODBC\ODBCINST.INI\IBM DB2 ODBC DRIVER - C_DB2_clidriver` still points `Driver` at `C:\DB2\clidriver\bin\db2clio.dll`, which is not there (`db2app64.dll` is, 12.1.4); the `C_DB2_clidriver115` entry points at `db2app64.dll` (11.5.9). Fixing the registration is the owner's (an administrator registry change). `SQL1042C` is inside IBM's driver; whether 12.1 logs in once registered needs the Db2 server up.

### FEAT-0125 · Every connection form can reuse a saved credential
- **State:** In-Progress · **Spec:** docs/05-data-model-and-storage.md, docs/06-security.md
- **Source:** owner, 2026-11-20: "In all Add/Edit connection (any activity host); allow user to pick existing credential from other connection. Credential will be duplicate into the new connection vault"
- **Scope:** a Use Saved Credential pick on every connection form's credential: the saved credentials by connection name and user (never the secret), and picking one copies the user and a new `secret:<ulid>` holding a copy of the secret into this connection
- [ ] Test: the copy is a new vault entry with the same value; the source is untouched; the data-folder grep — `a_reused_credential_lands_where_each_kind_keeps_it` (what `apply` writes, `row` reads back, for twelve kinds); the vault copy itself is not unit-tested
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [ ] App: `--data-dir ./tmp-data` — copy a credential into a new connection in two activities and connect — 2026-11-20: the row on a new Jira and a new Docker form; the list shows name · user · kind only; picking Bench Redmine's filled Username `admin` and "Stored in the vault". Not saved, not connected, one activity only. The data-folder grep could not tell: the bench password is a common 5-letter word that matches ordinary text; the vault file did not match
- [ ] Docs: spec · README · CHANGELOG — CHANGELOG only
- **Notes:** the copy is made when picked, so a form closed unsaved leaves an unused vault entry behind.

### FEAT-0121 · Automation: a projects column, then a project's jobs, then a job
- **State:** In-Progress · **Spec:** docs/tools/automation.md
- **Source:** owner, 2026-11-20: "Automation activity; have a left column that list the projects (searchable and hideable). Clicking the project, will list it jobs at right side, and clicking the job will display job detail on right side"
- [x] Test: jobs grouped under their project for each kind that has projects — `jobs_group_by_project_and_lose_it_from_their_names` (Jenkins folders, TeamCity, the top level)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [ ] App: `--data-dir ./tmp-data` — the bench Jenkins and TeamCity: filter projects, hide the column, open a job — 2026-11-20 on the bench TeamCity: Projects first in the toolbar, All Projects (1) and Bench (1), picking Bench lists "Hello Build" without its project, the job's detail beside it. Jenkins and the filter field not exercised
- [x] Docs: spec · README · CHANGELOG
- **Notes:** each job carries its `project` from the driver: Jenkins the folder path, TeamCity `projectName`, AWX `summary_fields.project.name`, GitHub / Forgejo the repository, GitLab the project; Puppet none.

### FEAT-0122 · Tracker: a projects column, then a project's tickets, then a ticket
- **State:** In-Progress · **Spec:** docs/tools/tracker.md
- **Source:** owner, 2026-11-20: "Tracker activity; have a left column that list the projects (searchable and hideable). Clicking the project, will list it tickets at right side, and clicking the ticket will display ticket detail on right side"
- [ ] Test: the project list and a project's tickets for each kind — the project pick runs through the builder, covered by its tests; the `/projects.json` and Jira `/project` reads have no test of their own
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [ ] App: `--data-dir ./tmp-data` — the bench Redmine and Forgejo: filter projects, hide the column, open a ticket — 2026-11-20 on the bench Redmine: Projects first in the toolbar, All Projects and Demo (`demo`), picking Demo writes `project:demo` and lists its tickets, a ticket opens beside. Forgejo lists no projects by design (the hint); Jira is not on the bench
- [x] Docs: spec · README · CHANGELOG

### FEAT-0123 · Tracker: a ticket's sub-tickets
- **State:** Completed · **Spec:** docs/tools/tracker.md
- **Source:** owner, 2026-11-20: "Tracker activity; i want display the ticket inside the ticket (especially for redmine)"
- **Scope:** a ticket's parent and children (Redmine sub-tasks, nested ones indented; Jira sub-tasks) on its page, each opening its own page. Deviation: not nested in the items list - the list stays the server's order, the page carries the family
- [x] Test: Redmine's `parent` turned into a tree; a child opens — `a_parent_and_nested_sub_tasks_are_read`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [x] App: `--data-dir ./tmp-data` — the bench Redmine with a parent and two sub-tasks — 2026-11-20: throwaway #4 and #5 under #3 and #6 under #5 made through the API; #3 shows Sub-Tickets (3) with #6 indented, a click opens #6, whose Parent link reads #5
- [x] Docs: spec · README · CHANGELOG
- **Notes:** the bench Redmine keeps the throwaway issues #4-#6 ("hstest sub…").

### FEAT-0124 · Tracker: an advanced search builder
- **State:** In-Progress · **Spec:** docs/tools/tracker.md
- **Source:** owner, 2026-11-20: "Tracker activity; Ticket list - i want advanced search builder to generate the query. Have dropdown for status, assignee, assigner, etc"
- **Scope:** dropdowns filled from the server (status, assignee, author, tracker or type, priority) that write the kind's own query the list runs: Redmine's words (`author:` and `priority:` added to the query bar) and JQL. The forges have no builder yet
- [x] Test: each kind's query built from the same picks — `the_builder_writes_the_query_bars_words` (Redmine), `the_builder_writes_jql` (Jira)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-20, tests at -j 2
- [ ] App: `--data-dir ./tmp-data` — the bench Redmine and Forgejo: pick status and assignee, the list narrows — 2026-11-20 on the bench Redmine: Status listed open / closed / all and the server's statuses; In Progress wrote `status:In_Progress` and the list narrowed to #2; Clear brought every open ticket back. An assignee pick and Forgejo (no builder) not done
- [x] Docs: spec · README · CHANGELOG

## v0.8.0 — In-Progress

> **IDM (Identity Management)**, Keycloak first. Spec
> `docs/tools/idm.md`, decision ADR-032, depth-programme phase P.
> Specified 2026-11-08; the foundation (crate, provider trait, Keycloak
> provider, safety layer) built 2026-11-08 on `feat/idm`; the screens
> built and walked on screen 2026-11-14. Open across the items: the AI
> assistant control, per-object palette verbs, the screen kittests, and
> the writes not yet pressed on screen (each item's Notes).

### FEAT-0109 · IDM activity: crate, provider trait, connections, Settings page, dashboard
- **State:** Completed · **Spec:** docs/tools/idm.md §3, §4.1, §5.1, §8 · **Phase:** P
- **Branch:** feat/idm
- **Source:** owner, 2026-11-08 ("add IDM as new Activity… For starter, Keycloak. If provide keycloak admin credentials, can the work pane resemble keycloak web interface (using APIs, etc)?", then "continue to plan, document and implement")
- **Scope:** `crates/tools/tool-idm/` (Cargo.toml from workspace deps; `IdmFactory`, `ToolDescriptor` id `idm`, category Infra); the `IdmProvider` trait, `Caps`, `Page`/`Paged`, the session runner; connection kind `keycloak` with `connections::IDM_KINDS` / `is_idm_kind` and the `[idm]` profile table; rail position after Tracker in `factories()`; the dashboard (summaries, Start Here, Saved Servers, Recent Changes, Open Here); the sidebar; one tab per server with the header band, toolbar (Browse toggle first, AI hard right), the hideable resizable browse column (realm `pick_field`, sections column), the list and item-page columns; Settings → IDM (list left, form right, Test Connection); the Keycloak mark (vendored logo or tinted glyph); Fluent strings; the palette commands and shortcuts of §9
- **Depends on:** —
- [x] Crate, factory, rail entry, connection kind and profile table
- [x] Provider trait, `Caps`, paging types, session runner (bounded channels, generations)
- [x] Dashboard, sidebar, tab shell with the three columns and their empty / loading / error states
- [x] Settings → IDM with the per-kind form and Test Connection
- [x] Keycloak mark everywhere the server is drawn (CR-0079)
- [x] Test: profile round-trip, `Caps`-driven section enabling, kittest of the empty dashboard and the per-kind Settings form — 2026-11-14: `idm_tests.rs` (the dashboard's empty state; the form shows only the chosen sign-in's fields), with the profile and `Caps` unit tests
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-14: 1511 passed (run at -j 2), deny ok, self-test ok
- [x] App: `--data-dir ./tmp-data` — add a Keycloak server in Settings → IDM, Test Connection, open its tab from the dashboard and the sidebar, hide and resize the browse column, narrow window; the *Checking a screen* list — 2026-11-14, release build against the bench Keycloak 26.8.2 (`hs-demo`, `hs-admin`): added in Settings → Identity (secret into the vault, only a `secret:` reference in the profile), Test Connection connected and filled both realm pickers, opened from the dashboard card and the sidebar, the Keycloak mark on the row, tab, header and card; every section walked; the window cannot go narrower than 1720, so the narrow check is the list column squeezed by the item column (it scrolls sideways, owner's report fixed the same day)
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS · docs/03 crate list
- **Notes:** 2026-11-08, foundation only. Built: `crates/tools/tool-idm` (workspace member), `IdmFactory` registered after Tracker, descriptor id `idm` (`IDENTIFICATION_BADGE`, Infra, title `Identity`), connection kind `keycloak` (`IDM_KINDS`, `is_idm_kind`, `of_idm_kinds`), the `[idm]` table (`profile.rs`, with `ca_file`), `IdmProvider` / `Caps` / `Page` / `Paged` (`provider.rs`), `runner::Session` (bounded queue of 32, work by topic, the newest aborting the older); a placeholder dashboard, sidebar and a tab that connects and reports what it reached. The Keycloak mark is vendored and registered (`bytes://product/keycloak.svg`) and drawn through `conn_icon` wherever the placeholder draws the server, but not yet looked at on screen, so its box stays open. Tested so far: the profile round-trip and `Caps` section and write availability (unit); the kittest boxes, Settings → Identity, the three-column tab and the App check are for the screens. Gate: fmt, clippy -D warnings, `cargo test -p tool-idm` and `-p hs-core`, deny - not the whole workspace test nor `--self-test`, so the Gate box stays open.
- **Notes (screens, 2026-11-14):** built in `ui/` (`mod`, `tab`, `dashboard`, `sidebar`, `common`, `dialogs`, one module per section) and Settings → Identity in `settings_ui.rs` (`identity`, `idm_form`, Test Connection through `conn_test` with the realm list). The Test box stays open: the profile round-trip and `Caps` are unit-tested, but the kittest of the empty dashboard and the per-kind Settings form are not written.
- **Notes (2026-11-15):** the AI assistant toggle (hard right, the shared column, three quick actions, context with secrets masked) added and opened on screen; the scratch data has no provider, so only its setup hint was seen, no answer.
### FEAT-0110 · Keycloak provider: auth, token refresh, server info and version gating
- **State:** Completed · **Spec:** docs/tools/idm.md §5.2, §8.3, §8.4 · **Phase:** P
- **Branch:** feat/idm
- **Source:** owner, 2026-11-08 (as FEAT-0109: "If provide keycloak admin credentials…")
- **Scope:** `provider/keycloak/{auth,api,model,version}.rs`; client-credentials, admin-password (`admin-cli`) and offline-token grants against `/realms/{auth_realm}/protocol/openid-connect/token`; tokens in memory only, renewed at a third of `expires_in` (10 s floor), single-flight refresh, a 401 retried once; a password session logged out on tab close; token-claim permission hints; `GET /admin/serverinfo` → version (Red Hat builds parsed) and features → `Caps` (22-26 supported; user profile 24+, sub-group children 23+, organizations 25/26, FGAP v2 26.2+), the refused-serverinfo fallback and 404 feature hiding; the realm list; the Realm Settings read-only summary; errors in words; `hs_core::service_http` gains a per-request bearer and a Verify TLS switch; `reqwest`'s `form` feature
- **Depends on:** FEAT-0109
- [x] Three grants, refresh and retry
- [x] Server info, version parse, `Caps`, fallbacks
- [x] Realm picker from the server and the Realm Settings summary
- [x] `service_http` per-request auth and Verify TLS (with the red badge)
- [x] Test: grant bodies, refresh timing and single flight, 401-retry-once, token-endpoint errors in words, `Caps` per version row, Red Hat version parse, a lightweight token gives no hints; live connect with each kind (`HS_KEYCLOAK`)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-14: 1511 passed (run at -j 2), deny ok, self-test ok
- [x] App: `--data-dir ./tmp-data` — connect with client credentials and with the admin password; switch realm; a wrong secret and a stopped server give the error state in words; the Realm Settings page — 2026-11-14: client credentials connected and Realm Settings read on screen; the admin password, a realm switch (the service account sees one realm), a wrong secret and a stopped server not yet tried in the app (the live test covers them), so the box stays open — 2026-11-15/16: a second profile signing in with the bench master admin's password connected (Version : 26.8.2, Signed In As admin-cli (master)); the realm picker switched master → hs-scratch → hs-demo; Test Connection said "Wrong username or password" for a wrong password and "Nothing answered at 127.0.0.1:8199: is the server running, and is the URL right?" for a port with nothing on it; Realm Settings read on screen earlier
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS · ADR-032 status
- **Notes:** 2026-11-08. The provider is complete for phase 1 (every read and write of §4, creates included, partial export with cancel); `realms()` and `realm_settings()` exist but the realm picker and Realm Settings page do not, and `service_http::Tls` / `Http::request_as` are in `hs-core` with tests but the red *TLS not verified* badge is a screen - both boxes stay open. Live on the bench's Keycloak 26.8.2 (`tests/keycloak_live.rs`, client credentials, admin password and offline token): passed. Found there and written into the spec and GOTCHAS: the `kc-admin-api` audience 401 (reported as `IdmError::Audience`), no `systemInfo` for a realm-scoped account (version unknown, `Caps` from the features list), `unauthorized_client` for a wrong secret, `exact` ignored beside `search`. Spec, CHANGELOG and GOTCHAS updated; README, CHECKLIST and ADR-032's status wait for the screens.

### FEAT-0111 · IDM Users
- **State:** Completed · **Spec:** docs/tools/idm.md §4.3 · **Phase:** P
- **Branch:** feat/idm
- **Source:** owner, 2026-11-08 (as FEAT-0109)
- **Scope:** the Users list (search, exact, `q=` attribute search, enabled filter, `first`/`max`, `briefRepresentation`, `/users/count`); the item page's Details, Attributes (user profile on 24+, free map on 22-23), Credentials (metadata; Reset Password with the temporary flag), Groups (join / leave), Role Mapping (assign / unassign realm and client roles, effective view) and Sessions (log out, end session) tabs; Enable / Disable; Delete with a typed confirmation; read-merge-write saves with the review
- **Depends on:** FEAT-0110, FEAT-0116
- [x] List with server-side search, paging and count
- [x] Details and Attributes with read-merge-write and the review
- [x] Credentials and Reset Password
- [x] Groups and Role Mapping tabs
- [x] Sessions tab, Log Out, Enable / Disable, Delete
- [x] Test: query building and paging, stale-generation drop, read-merge-write keeps untouched fields, user-profile metadata to form fields, the unmanaged-attribute policy; live: page, edit an attribute, disable / enable, temporary reset, join / leave, assign / unassign, log out, delete a throwaway user — 2026-11-15/16: unit (`users.rs::the_user_profile_turns_attributes_into_fields`, `provider.rs::the_unmanaged_attribute_policy_decides_show_and_edit`, the query, paging and read-merge-write tests), mock (`a_save_writes_the_whole_representation_back`) and `runner.rs`'s newer-request test; live (`keycloak_live`): paging, an attribute edit, disable and enable read back, a temporary reset, join and leave read back, assign and unassign read back, log out with no session left, delete confirmed gone
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-14: 1511 passed (run at -j 2), deny ok, self-test ok
- [x] App: `--data-dir ./tmp-data` — search the bench realm, open a user, walk every tab, each write once, at the default and a narrow size; the *Checking a screen* list — 2026-11-14: Users listed with the count (1–25 of 25), a user's six tabs, Add User (throwaway `hs-ui-check`, selected after create), a Details save through the review, Delete with the typed confirmation (OK disabled until the name matched); Enable / Disable, Reset Password, Log Out, Join / Leave and Assign / Unassign not pressed on screen (the live test runs each)
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** 2026-11-14, built (`ui/users.rs`, the group and role pickers). Unit tests: an unedited draft changes nothing, a save sends the whole attribute map, an unticked attribute row is dropped. The query, paging and live boxes rest on the foundation's tests (`tests/mock.rs`, `tests/keycloak_live.rs`); the Test box stays open until the screen-level ones (stale generation through the tab, user-profile metadata to fields) are written. Deviations, in the spec: Show Effective adds inherited realm roles only; Reset Password does not show the realm's password policy.
- **Notes (2026-11-15):** on screen against the bench, on a throwaway user: Disable (from the palette), Enable, Reset Password (temporary; `UPDATE_PASSWORD` pending after), Log Out, Join Group, Assign Role (from the palette), Delete (from the palette, typed). Show Effective now lists inherited client roles (a Job, one request per client) and Reset Password shows the realm's password policy when there is one (the bench realm has none, so the hint was not seen). The Test box stays open: the screen-level tests of the stale generation and of user-profile metadata to fields are not written.
### FEAT-0112 · IDM Groups and Roles
- **State:** Completed · **Spec:** docs/tools/idm.md §4.4, §4.5 · **Phase:** P
- **Branch:** feat/idm
- **Source:** owner, 2026-11-08 (as FEAT-0109)
- **Scope:** the lazy group tree (`/groups`, `/groups/count`, `/children` on 23+, `subGroups` on 22, More per level, search with paths); the group page's Members (paged, add / remove, Include Sub-Group Users as a Job), Role Mapping, Attributes and Details (rename); Delete group with a typed confirmation; the Realm Roles and Client Roles lists; the role page's Details (description), Composite Roles (read-only), Users in Role and Groups in Role; Delete role with a typed confirmation; built-in roles never offered for deletion
- **Depends on:** FEAT-0110, FEAT-0116
- [x] Group tree and group page
- [x] Realm and client role lists and the role page
- [x] Test: tree paging on the 22 and 23+ shapes, role-mapping requests, built-in roles excluded from Delete; live: members add / remove, role map / unmap on a group, delete a throwaway group and role — 2026-11-15/16: unit (both group shapes, role-mapping paths, the built-in role predicate, `common.rs::a_built_in_object_cannot_be_deleted_from_its_page`), mock (the 22 fallback and `a_keycloak_23_server_pages_a_groups_children`); live: members add and remove read back, a client role mapped and unmapped on a group, the group, the client role and the realm role confirmed gone after delete
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-14: 1511 passed (run at -j 2), deny ok, self-test ok
- [x] App: `--data-dir ./tmp-data` — expand the bench realm's group tree, open a group and a role, each write once; the *Checking a screen* list — 2026-11-14/15: Create Group, Add Member, Delete Group (typed) on throwaway groups; Include Sub-Group Users listed /engineering's whole subtree with each user's group, and says so when there is nobody. Role writes could not be pressed: the bench's `hs-admin` lacks `manage-realm`, and Create Role is drawn disabled saying so (the live test runs them as the master admin)
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** 2026-11-14, built (`ui/groups.rs`, `ui/roles.rs`); walked on screen (the tree with a sub-group count, a group's members, Realm Roles and a role page, built-in roles' Delete disabled). Not built: Include Sub-Group Users (a group's Members lists direct members). App box open until a group and a role write are pressed on screen.

### FEAT-0113 · IDM Clients
- **State:** Completed · **Spec:** docs/tools/idm.md §4.6 · **Phase:** P
- **Branch:** feat/idm
- **Source:** owner, 2026-11-08 (as FEAT-0109)
- **Scope:** the Clients list (`clientId` search, paging); the Settings page of the common fields (read-merge-write with the review, More Settings read-only); Credentials with the secret masked, Show (30 s), Copy (clipboard cleared after 60 s) and Regenerate with a typed confirmation; the client's Roles; Service Account Roles; the client's Sessions; Delete with a typed confirmation; the realm's built-in clients read-only and never deletable
- **Depends on:** FEAT-0110, FEAT-0112, FEAT-0116
- [x] List and Settings page
- [x] Credentials (mask, show, copy, regenerate)
- [x] Roles, Service Account Roles and Sessions tabs; Delete
- [x] Test: the settings merge keeps unknown fields, built-in clients refused, the secret never in logs or the audit log; live: edit redirect URIs, read and regenerate a throwaway client's secret, a service-account role map, delete the throwaway client; grep the data folder for the secret — 2026-11-15/16: unit (the settings merge keeps unknown fields, `common.rs::a_built_in_object_cannot_be_deleted_from_its_page` for the guard), mock (`a_secret_never_reaches_the_log`: no client secret, password or token in the log) and the audit test; live: redirect URIs edited, the secret read and regenerated, a client role mapped on the service account and back, the client confirmed gone, and the regenerated secret among the strings the data-folder search looks for
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-14: 1511 passed (run at -j 2), deny ok, self-test ok
- [x] App: `--data-dir ./tmp-data` — open `hs-admin` and a throwaway client, show / copy / regenerate, edit settings through the review; the *Checking a screen* list — 2026-11-15: Create Client, Copy (the toast says the clipboard is cleared in 60 s), Regenerate (typed; the ACTION · CLIENT admin event followed), Show (masked again after 30 s; the shown value was not captured, on purpose), a Name saved through the review (`name: (empty) → Throwaway`), Delete (typed)
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** 2026-11-14, built (`ui/clients.rs`); unit tests: an unedited client changes nothing, a redirect-list change sends the list, an attribute-backed setting keeps the client's other attributes. Walked on screen: the list, a built-in client opening read-only with Delete disabled. Show / Copy / Regenerate not pressed on screen (no secret on screen during the check), so the App box stays open.

### FEAT-0114 · IDM Sessions and Events
- **State:** Completed · **Spec:** docs/tools/idm.md §4.7, §4.8 · **Phase:** P
- **Branch:** feat/idm
- **Source:** owner, 2026-11-08 (as FEAT-0109)
- **Scope:** realm sessions by client (`/client-session-stats`, `/clients/{id}/user-sessions`), End Session, Log Out All Sessions with a typed confirmation (`/logout-all`); the Login Events and Admin Events tables with their filters (type multi-pick, client, user, IP, date range), paging and detail; the Level word coloured through the shared log-level function (CR-0077); the events-not-stored empty states from `/events/config`; admin-event representations masked
- **Depends on:** FEAT-0110, FEAT-0116
- [x] Sessions section and Log Out All
- [x] Login Events and Admin Events
- [x] Test: event parsing and level mapping, filter query building, masking of admin-event representations; live: list sessions, end one, read both event tables after a failed login — 2026-11-15/16: unit (event parsing and levels, filter queries, `an_events_day_is_sent_as_the_local_days_bounds`, masking of admin-event representations); live: a failed login made and found in Login Events by type and user, a session signed in and ended with End Session, Log Out leaving none, the user's creation found in Admin Events
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-14: 1511 passed (run at -j 2), deny ok, self-test ok
- [x] App: `--data-dir ./tmp-data` — make a failed login on the bench realm and find it, filter by date, log out all on a scratch realm; the *Checking a screen* list — 2026-11-15/16: the failed login (bob.smith, LOGIN_ERROR) top of Login Events; To 2026-11-14 lists only the 14th's rows (the 15th's morning rows came through before BUG-0062); on a throwaway realm `hs-scratch` with two signed-in users, End Session ended one and Log Out All Sessions (typed) ended the other; the realm removed afterwards
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** 2026-11-14, built (`ui/sessions.rs`, `ui/events.rs`); walked on screen: sessions by client, both event tables with the app check's own create / update / delete in Admin Events and the level word coloured. App box open until End Session and Log Out All are pressed against a scratch realm.
- **Notes (2026-11-15):** an admin event's representation opens in the read-only editor (JSON colours, line numbers); End Session and Log Out All Sessions were opened on screen and cancelled, because the bench realm's sessions belong to its seeded users. A failed login and Log Out All on a scratch realm are still to do, so the App box stays open.
### FEAT-0115 · IDM Realm export
- **State:** Completed · **Spec:** docs/tools/idm.md §4.9 · **Phase:** P
- **Branch:** feat/idm
- **Source:** owner, 2026-11-08 (as FEAT-0109)
- **Scope:** the Export Realm modal (clients, groups and roles), `POST /partial-export` as a Job with cancel, the JSON in a read-only `hs-editor` view with Save As, Open in Editor (`OpenRequest::Text`) and Copy; any unmasked secret masked before it is shown; no import
- **Depends on:** FEAT-0110, FEAT-0116
- [x] Export Job and read-only view
- [x] Test: masking of an export with a planted unmasked secret; live: export the bench realm, cancel a running export — 2026-11-15/16: mock (a planted unmasked secret is masked, a cancel stops it); live as the master admin: the bench realm exported, a running export cancelled
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-14: 1511 passed (run at -j 2), deny ok, self-test ok
- [x] App: `--data-dir ./tmp-data` — export, fold and search the JSON, Save As, Open in Editor — 2026-11-15/16: exported hs-demo as the master admin; searched the JSON ("secret": 1 of 2, the hs-admin client's value `**********`); Open in Editor gave an untitled JSON buffer; Save As wrote a 92,752-character file holding orders-api, the masked secret and not the real one (deleted after). Folding was not checked because the editor engine has none: the spec and README no longer claim it (BUG-0064)
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** 2026-11-14, built (`ui/export.rs`): the modal, the Job with cancel, Save As, Open in Editor, Copy. On screen the bench's `hs-admin` service account got the server's 403 (it lacks `manage-realm`), shown in words; the export itself is proven by the live test as the master admin. Deviation, in the spec: the JSON is read-only text in a well, not the editor engine (no folding or search yet).
- **Notes (2026-11-15):** the export now opens in the read-only editor engine; still not seen on screen with a real export (the bench account gets 403), so the App box stays open.
### FEAT-0116 · IDM safety: guards, typed confirmations, audit log, secret masking
- **State:** Completed · **Spec:** docs/tools/idm.md §6 · **Phase:** P
- **Branch:** feat/idm
- **Source:** owner, 2026-11-08 (as FEAT-0109)
- **Scope:** the `Guarded` wrapper (every write refused on a protected environment or a read-only profile, controls disabled with the reason, `env_banner`); the typed-confirmation modal moved from `tool-queue` to `hs_ui::dialog` and used for deleting a user, group, client or role, regenerating a secret and logging out all; the field-by-field review before a multi-field save; `idm/audit.jsonl` in the workspace (field names, never values) with the dashboard's Recent Changes and Clear History; `mask.rs` for server JSON; secrets masked by default and never logged, put in history or given to the assistant
- **Depends on:** FEAT-0109
- [x] `Guarded` wrapper and the read-only tab
- [x] Typed confirmation in `hs_ui::dialog` (tool-queue moved onto it) and the save review
- [x] Audit log, Recent Changes, Clear History
- [x] Masking and secret handling
- [x] Test: every write method refused when read-only (fails when a new one is not listed); the typed confirmation's OK disabled until the name matches (kittest); audit lines carry no secret, password or token; masking of nested JSON; the data-folder grep after a live run — 2026-11-15/16: `guarded/tests.rs` (every write refused, the method list checked against the trait), `typed_confirm.rs`, the audit test, `mask.rs`, and the live data-folder search
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-14: 1511 passed (run at -j 2), deny ok, self-test ok
- [x] App: `--data-dir ./tmp-data` — a Production-tier profile opens read-only with every write disabled; each typed confirmation; the audit file after a session; tool-queue's Delete still works — 2026-11-15/16: a production-tier profile read-only with every write disabled and the reason (2026-11-14); each typed confirmation (user, group, client, regenerate, log out all); `idm/audit.jsonl` after the session names verbs, objects and field names only (end-session, logout-all, create/delete); Queue's Delete on a throwaway RabbitMQ queue went through the shared typed confirmation and the queue was gone (404)
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS · docs/06 (an IDM row in the protection table)
- **Notes:** built before the section items (FEAT-0111 to FEAT-0115 depend on it), so no write ships unguarded. 2026-11-08: `guarded.rs` (`Guarded` over every write in `WRITES`, `WriteGate` with the owner's session Allow Writes unlock), `audit.rs` (`<workspace>/idm/audit.jsonl`, append, recent, clear, remove; the workspace `.gitignore` lists it), `mask.rs` (server JSON; exports and admin-event representations are masked inside the provider), `Secret` with a value-free `Debug`, the reset password zeroised in our buffers. Tested: every write refused on a read-only tab before the provider sees it, plus a source check that fails on a trait method in neither `WRITES` nor `READS`; audit lines name fields and carry no password, secret or edited value; masking of nested JSON; the live run's data-folder search for the client secret, the reset password, the admin password and tokens. Still open: the read-only tab's disabled controls, the env banner and the Allow Writes control, the typed confirmation in `hs_ui::dialog` and its kittest, Recent Changes on the dashboard, Clear History's menu - so those boxes stay open.
- **Notes (screens, 2026-11-14):** the read-only tab and Allow Writes walked on screen with the profile on PRODUCTION: every write control and field disabled with the reason, Allow Writes asks naming the environment, then writes are live. Found and fixed there: the tab drew a stale `Caps` snapshot after Allow Writes (it asks the guarded provider each frame now), and a profile's environment change now reconnects an open tab. `hs_ui::dialog::typed_confirm` with a unit test of the match rule; the kittest box stays open.
- **Notes (2026-11-15):** kittests added: the typed confirmation's OK does nothing until the name matches (`hs-ui/tests/typed_confirm.rs`), and a refused write button is drawn and ignores a click (`ui::common`); the tooltip's text is not asserted. Each typed confirmation (user, group, client, regenerate, log out all) seen on screen. Tool-queue's Delete was not pressed again after moving to the shared dialog, so the App box stays open.
### FEAT-0117 · Identity: the LDAP provider (users, groups, the directory tree)
- **State:** Completed · part one (provider, profile, form, bench, tests) built 2026-11-19 on `feat/idm-ldap`; part two, the Directory section, 2026-11-20 on `feat/idm-ldap-directory` · **Spec:** docs/tools/idm.md §4.12 · **Phase:** P
- **Source:** owner, 2026-11-08: "hold on CAS, But add LDAP and AD in the list"; taken up 2026-11-19 ("FEAT-0117 LDAP")
- **Scope:** an `IdmProvider` for LDAP v3 (OpenLDAP, 389 DS / FreeIPA, OpenDJ): connection kind `ldap` (URL, StartTLS, Verify TLS, CA File, bind DN and password as `secret:<ulid>`, users / groups base and filter, page size); naming contexts as the realm switcher; users (search, view, create, edit attributes, rename / move, delete, password reset through Password Modify or `userPassword`); groups (`groupOfNames` / `groupOfUniqueNames` / `posixGroup`, members); the new **Directory** section (the DIT as a tree, a raw attribute editor with the schema's types, the save review); paged results; the other sections disabled through `Caps`; the ADR and `cargo deny` pass for the LDAP client crate; an OpenLDAP container in the bench kit's `idm` profile, seeded
- **Depends on:** FEAT-0109, FEAT-0116 (the screens and the safety layer it plugs into)
- [x] ADR for the LDAP client (`ldap3`), `cargo deny` — ADR-033; `tls-rustls-ring`, default features off; deny passes
- [x] Provider: bind, Root DSE, paging, users, groups, directory tree, password reset — the tree in part two: `dir_children` (one level, containers first, paged), `dir_entry`, `dir_modify` (the naming attribute's value refused), `dir_move` (refused under itself or to a missing parent), `dir_delete` (leaves only); all five through `Guarded` and the audit log
- [x] Directory section in the tab (tree, attribute editor, review) — `ui/directory.rs`: the tree a level at a time, the entry's facts, every attribute in the shared name = value editor, Save through a fresh read and the review, Move To from the loaded containers (typed confirmation for a subtree), Delete for a leaf behind a typed confirmation
- [x] Bench: OpenLDAP in the `idm` profile with a seeded OU tree, users and groups — `hs-openldap` (`osixia/openldap:1.5.0`), `seed/seed-ldap.sh`, `seed/data/ldap/seed.ldif`
- [x] Test: unit (filters, DN escaping, paging, a change turned into LDAP modify operations); live round trip gated on `HS_LDAP`; the data-folder grep after it — `provider::ldap::map` tests, `profile` round trip, `hs_core::tls`; `tests/ldap_live.rs` passes against `hs-openldap` (paging, search, membership both ways, create → edit → rename → password → group → delete, read-only refusal); 2026-11-19 the scratch data folder holds no plaintext bind password
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — part one passed 2026-11-19 (tests at -j 2); part two 2026-11-20: the whole gate (tests at -j 2), then fmt, clippy and `tool-idm`'s tests again after the two column fixes, and the self-test on the rebuilt exe
- [x] App: `--data-dir ./tmp-data` — an OpenLDAP server: browse, create, edit, move and delete a user and a group; a protected environment read-only; the *Checking a screen* list — part one 2026-11-19: Settings form and Test Connection ("Connected. OpenLDAP · cn=admin,…"), Users (11, no Status column, filter disabled), a user's Details / Attributes / Groups, Groups (`/engineering` with grace), the Keycloak-only sections and buttons disabled with reasons, create → edit (review "firstName Test → Tested") → typed-confirmation delete of `hstest-ui`. Part two 2026-11-20: the tree (containers first, leaves aligned under carets), an entry's facts and attributes whole in a 520 px column, an edit reviewed ("description (empty) → probe"), a move to `ou=support` reselected at its new DN, a typed delete of the throwaway `hstest-dir`; with PROD-SUPPORT protected, the editor dimmed and Move / Delete disabled with "PROD-SUPPORT is read-only…" as their tooltip, Allow Writes in the toolbar
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS · bench/README — part two: spec §4.12 "What is implemented: the Directory section", README, CHANGELOG, CHECKLIST
- **Notes:** the Groups list is flat: a directory's nesting shows as a group's children, not as a tree in the list (the provider leaves `sub_group_count` unset). The realm field elides a long base DN (`dc=hyperstack,dc=te…`); it scrolls, and the browse column can be widened.

### FEAT-0118 · Identity: Active Directory mode on the LDAP provider
- **State:** Pending · **Spec:** docs/tools/idm.md §4.12 · **Phase:** P
- **Source:** owner, 2026-11-08 (as FEAT-0117)
- **Scope:** connection kind `active-directory` on the LDAP provider: UPN bind, the domain's naming context, `sAMAccountName` / `userPrincipalName`; enable / disable (`userAccountControl`), unlock (`lockoutTime`), must change password at next logon, account expiry; password reset through `unicodePwd`, refused in words without LDAPS or StartTLS; security and distribution groups with scope, nested groups and effective `memberOf`; the Active Directory mark everywhere the server is drawn; a Samba AD domain controller in the bench kit's `idm` profile, seeded
- **Depends on:** FEAT-0117
- [ ] AD mode: account flags, password reset over TLS, groups with scope and nesting
- [ ] Mark: the Active Directory logo vendored and registered
- [ ] Bench: a Samba AD DC in the `idm` profile, seeded with OUs, users and nested groups
- [ ] Test: unit (`userAccountControl` flags, `unicodePwd` encoding, the in-chain membership filter); live round trip gated on `HS_AD`; the data-folder grep after it
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test
- [ ] App: `--data-dir ./tmp-data` — the Samba AD server: disable, enable and unlock a user, reset a password, nested group membership; the *Checking a screen* list
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS · bench/README

### FEAT-0119 · Automation: sign in to an older TeamCity (9.x) with a domain password
- **State:** Completed · **Spec:** docs/tools/automation.md §4, §5
- **Branch:** feat/teamcity-9
- **Source:** owner, 2026-11-18: "i have teamcity old version (9.x). I connect using my microsoft active directory login. I cannot access the teamcity through hyperstack using password and Token", then "Yes, build it in"
- **Scope:** TeamCity 9.x sends its login page from `/app/rest` and takes HTTP basic only under `/httpAuth`; it has no access tokens (2019.1) and no `$locator` in `fields`. A password login turned away on `/app/rest` is asked again under `/httpAuth`, which the session keeps; a token is not retried; the jobs list falls back to the configurations without their last build; the Settings form's hint for a TeamCity password (`DOMAIN\user`) and token (2019.1 and later)
- [x] Test: mock (`an_old_teamcity_takes_a_password_under_http_auth_and_lists_its_jobs`, `a_teamcity_token_is_not_retried_under_http_auth`), failing before the change — 2026-11-18: the old-server test failed on connect against the old driver and passes now; the token test is a guard and passes either way
- [x] Gate: fmt · clippy -D warnings · test --workspace (run at -j 2) · deny · --self-test — 2026-11-18
- [x] App: scratch `--data-dir` — the TeamCity form's hints for a password and a token; a modern server (bench `hs-teamcity`) still connects — 2026-11-18: the bench TeamCity 2025.07.3 tab lists Bench / Hello Build with its last run on a password; Test Connection "Connected. TeamCity 2025.07.3 (build 197398)"; both hints read whole under the Credential field
- [x] Docs: spec · README · CHANGELOG · GOTCHAS
- **Notes:** there is no TeamCity 9.x on the bench; the old server is proven against a mock written from its documented behaviour, not a real one.

### FEAT-0120 · Automation: sign in to a single-sign-on TeamCity with the browser's session
- **State:** Completed · **Spec:** docs/tools/automation.md §5
- **Branch:** batch/teamcity-session
- **Source:** owner, 2026-11-19: FEAT-0119's password was refused ("The server refused the credentials (401)"); their TeamCity signs in through Keycloak brokering Microsoft, so it never sees the AD password. Of a service account, a browser session and guest access, the owner chose "Build session login"
- **Scope:** a third TeamCity credential, **Browser Session**: the `TCSESSIONID` cookie copied from a signed-in browser, kept in the vault as an api-key whose header is fixed to `Cookie`; a bare id is sent as `TCSESSIONID=…`; sent on `/app/rest`, never retried under `/httpAuth`; an ended session (the login page, or a 401) says to sign in again and paste a fresh one, on connect and when the tab re-reads its jobs; the form shows only the session field, with a hint saying where to copy it from
- [x] Test: profile (`teamcity_offers_a_browser_session_sent_as_a_cookie`) and mock (`a_teamcity_browser_session_is_sent_as_its_cookie`, `an_ended_teamcity_session_asks_to_sign_in_again`), failing before the change — 2026-11-19: all three failed on the old code and pass now
- [x] Gate: fmt · clippy -D warnings · test --workspace (run at -j 2) · deny · --self-test — 2026-11-19
- [x] App: scratch `--data-dir` — the TeamCity form with Browser Session: one Session Cookie field and its hint; a session copied from the bench TeamCity connects; a wrong one says to sign in again — 2026-11-19: the form shows Browser Session with one Session Cookie field and its hint; a made-up session answers "TeamCity did not take the session … Sign in to TeamCity in the browser again"; a TCSESSIONID from the bench TeamCity (signed in by script, never shown) gives "Connected. TeamCity 2025.07.3", and the saved profile's tab lists Bench / Hello Build; the profile keeps `secret:<ulid>` and the data folder holds no plaintext session
- [x] Docs: spec · README · CHANGELOG · GOTCHAS
- **Notes:** checked against the bench TeamCity 2025.07 only; the owner's 9.x behind Keycloak is the real test. A newer TeamCity also wants a CSRF token on writes made with a session; 9.x does not.

### CR-0099 · Identity: the Keycloak version through an optional master-realm account
- **State:** Completed · **Spec:** docs/tools/idm.md §3.3, §3.4, §8.3 · **Phase:** P
- **Source:** owner, 2026-11-14: "In Keycloak Work Pane Header, please add version of the keycloak"; on Keycloak withholding `systemInfo` from accounts outside the master realm, the owner chose "Exact via master account"
- **Scope:** an optional **Version Lookup** credential on a Keycloak profile (a master-realm client's credentials or an admin password through `admin-cli`, the secret a `secret:<ulid>`), used only to read `systemInfo.version` from `GET /admin/serverinfo` when the main account is not shown it; the token is used once and dropped; the version then drives `Caps` as a known one; the header shows it, and without a lookup it still says the version is not shown to this account, with a tooltip saying why
- [x] Profile and Settings → Identity: the Version Lookup fields, shown only when it is on
- [x] Provider: the lookup on connect when `systemInfo` is missing; a refused lookup leaves the version unknown and says so in the log, never fails the connection
- [x] Test: unit (profile round trip, the lookup used only without `systemInfo`); live against the bench's master admin; the data-folder grep after it — 2026-11-14: `keycloak_live_version_lookup_fills_the_version` (no version without it, a version with the master admin, still connected with a wrong lookup password); 2026-11-15: the scratch data folder searched for the lookup password, no file holds it
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-15, at -j 2
- [x] App: `--data-dir ./tmp-data` — the bench profile with a lookup shows 26.x in the header; without one, the fallback text — 2026-11-15: "not shown to this account (Settings → Version Lookup)" before; the card filled with the bench's master admin and saved ("Stored in the vault"); the header then reads Version : 26.8.2, also after a restart
- [x] Docs: spec · README · CHANGELOG · GOTCHAS

### CR-0100 · The server version in every activity header that can learn it
- **State:** Completed · **Spec:** docs/04-ui-design.md (the work-pane header) · **Phase:** P
- **Source:** owner, 2026-11-14: "Check other activities that need to display the version"
- **Scope:** a survey of every activity's header (Database, Redis, NoSQL, Containers, Queue, Automation and Tracker already show it); fill the gaps the account already allows: the Git host tabs (Forgejo / Gitea `/api/v1/version`, GitLab `/api/v4/version`, GitHub Enterprise `X-GitHub-Enterprise-Version`), GitHub Enterprise in Automation and Tracker, Confluence Server / Data Center (`/rest/applinks/1.0/manifest`). Left empty on purpose, said in the spec: github.com, Bitbucket Cloud, Confluence Cloud and Redmine (no version API), Kafka (no broker version, only API ranges), MQTT (the protocol, not the broker), S3 (no version in the protocol), API Client and Logs (no server product)
- [x] Git host tabs: version fact
- [x] GitHub Enterprise version in Automation and Tracker; Confluence Server version
- [x] Test: unit (each response parsed, an absent header gives no version); live where a bench server exists (Forgejo, GitLab, Confluence) — 2026-11-15/16: unit for each URL and answer (Forgejo, GitLab, GitHub Enterprise `/meta`, the Confluence manifest) and for no version from github.com or a host that cannot be reached; on screen the bench Forgejo. GitHub Enterprise Server and Confluence Server are not on the bench, so those two calls are proven on their answers' shape only
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-15, at -j 2
- [x] App: `--data-dir ./tmp-data` — the bench Forgejo host tab shows its version — 2026-11-15: the Forgejo host tab's header reads Version : 9.0.3+gitea-1.22.0
- [x] Docs: spec · CHANGELOG
- **Notes (2026-11-15):** unit tests cover each URL and each answer's parsing. The Test box stays open: nothing was run against a GitHub Enterprise Server or a Confluence Server (the bench has neither), so those two calls are unverified against a real server; a refusal leaves the header without a version.

### CR-0101 · The versions CR-0100 wrote off: Nexus, WebDAV, MinIO, Kafka, Redmine
- **State:** Completed · **Spec:** docs/04-ui-design.md (addendum 2026-11-14, the version table) · **Phase:** P
- **Source:** owner, 2026-11-15: "Why is some of s3, queue, tracker and automation connection not displaying version, especially nexus, minio, webdav, kafka and redmine"
- **Scope:** probe each against the bench before writing it off. Nexus and a WebDAV share name a version in the `Server` header; MinIO's release comes from its admin info (admin keys); Kafka's release line from the newest `metadata.version` a KRaft broker supports (ApiVersions v3); Redmine's version from Administration → Information when the profile signs in with an administrator's password. Left empty and said in the spec: S3Mock, AWS S3 and others whose header names only a product, MinIO without admin keys, a ZooKeeper-mode broker, Redmine with an API key or a non-admin account
- [x] Code: `ObjectStore::server_version` (S3, WebDAV, Nexus), `server_header_version`, `S3Client::minio_version`; `kafka_groups::release_line`; `redmine::admin_version`; the S3 tab's header
- [x] Test: unit (`a_server_header_gives_a_version_only_when_it_has_one`, `minio_admin_info_names_the_release`, `a_metadata_level_names_its_release_line`, `the_information_page_names_the_version`, `the_sign_in_form_gives_its_token`); live: the bench Kafka 4.0.0 (`kafka_lists_peeks_publishes_and_purges_a_topic` asserts the line) and Redmine (`redmine_names_its_version_to_an_admin_password`, `HS_REDMINE_ADMIN`)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test — 2026-11-16, at -j 2
- [x] App: `--data-dir ./tmp-data` — each header — 2026-11-16: Nexus `3.70.1-02 (OSS)`, WebDAV `Apache/2.4.37 (Unix)`, S3 (S3Mock) no version, Kafka `4.0.x` (and Test Connection "Connected. Kafka 4.0.x"), Redmine `6.1.5.stable` after switching the profile to the admin's password
- [x] Docs: docs/04 version table and the list of what stays empty · CHANGELOG · GOTCHAS (a new heading, Server Versions In Headers) · bench/README (`HS_REDMINE_ADMIN`)
- **Notes:** MinIO is unverified against a server: the bench's S3 is S3Mock and there is no MinIO container. The bench's `hs-kafka-sasl` does not start (its `/tmp/hs_jaas.conf` mount is gone), so Kafka was checked on the plain broker. An open Tracker tab keeps the session it opened with after its profile is edited, so the Redmine version showed only after reopening the tab; Identity reconnects on such an edit, Tracker does not (a follow-up, not changed here).

### BUG-0062 · Identity's event date filter used UTC days
- **State:** Completed · fixed 2026-11-15
- **Source:** the app check, 2026-11-15: To 2026-11-14 listed rows the table showed as the 15th's morning (UTC+8)
- **Scope:** Keycloak reads `yyyy-MM-dd` as a UTC day. On 26+, which also takes epoch milliseconds, From and To go as the first and last instant of the user's local day (`day_bound`); an older or unknown server gets the text
- [x] Test: `an_events_day_is_sent_as_the_local_days_bounds`
- [x] Gate: 2026-11-16, at -j 2
- [x] App: To 2026-11-14 lists only the 14th's rows (2026-11-16)
- [x] Docs: spec §4.8 · CHANGELOG · GOTCHAS

### BUG-0063 · Test Connection showed a server's words starting lowercase
- **State:** Completed · fixed 2026-11-15
- **Source:** the app check, 2026-11-15: "wrong username or password", "nothing answered at …"
- **Scope:** a failed Test Connection goes through `hs_core::fmt::sentence`, for every connection kind
- [x] Gate: 2026-11-16, at -j 2
- [x] App: Identity's wrong password and unreachable server read in sentence case (2026-11-16)
- [x] Docs: CHANGELOG

### BUG-0064 · Identity's docs promised folding the JSON view does not have
- **State:** Completed · fixed 2026-11-15
- **Source:** the app check, 2026-11-15: the export's JSON view has search and colours but no folding; the editor engine has none
- **Scope:** the spec, README and the code comment now say colours and search; folding in the editor engine is not added here
- [x] Docs: spec §4.8, §4.9 · README · CHANGELOG

### BUG-0065 · An open Tracker tab kept its old session after its profile was edited
- **State:** Completed · 2026-11-17
- **Source:** the CR-0101 app check, 2026-11-16: the Redmine profile switched to an administrator's password showed its version only after the tab was reopened; owner, 2026-11-17: "Fix the Tracker tab reconnect on profile edit"
- **Scope:** like Identity (CR-0099), an open Tracker tab reconnects when its profile's connection changes (URL, repository or project, Cloud, Textile, credential), its environment or its Read-Only flag. Saving or deleting a query from the tab writes the profile too, and that alone does not reconnect
- [x] Test: a reproducing unit test (`a_profile_edit_reconnects_but_a_saved_query_does_not`, failed before the fix)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 2 · deny · --self-test (2026-11-17)
- [x] App: `--data-dir D:\Temp\hs-gui-data` — with a Bench Redmine tab open (Version 6.1.5.stable), its URL set to port 3011 in Settings: the tab reconnected at once and said nothing answered at 3011; set back to 3010: it reconnected and showed the version and items again; Save This Query with item #3 selected wrote `queries` to the profile and the tab kept its session and selection
- [x] Docs: docs/tools/tracker.md · CHANGELOG · GOTCHAS


### BUG-0066 · An open Automation or Queue tab kept its old session after its profile was edited
- **State:** Completed
- **Source:** the BUG-0065 follow-up, 2026-11-17: the Tracker fix found the same shape in Automation and Queue; owner, 2026-11-18: "Yes, fix both"
- **Scope:** as BUG-0065 for Tracker, an open Automation or Queue tab reconnects when its profile's connection (kind fields, credential), its environment or its Read-Only flag changes, and locks Allow Writes again. A rename or a pin only renames the tab
- [x] Test: a reproducing unit test per crate (`a_profile_edit_reconnects_but_a_rename_does_not`, failed before the fix)
- [x] Gate: fmt · clippy -D warnings · test --workspace (run at -j 2) · deny · --self-test
- [x] App: scratch `--data-dir` — Bench Forgejo CI's repository edited (the tab showed the 404 at once) and set back (jobs and runs again); renamed (the tab took the name and kept ci.yml selected, no reconnect); Bench Kafka Plain's bootstrap port edited (the tab reconnected to :9099) and set back (topics again)
- [x] Docs: docs/tools/automation.md · docs/tools/queue.md · CHANGELOG · GOTCHAS

### BUG-0067 · Database: refusing a foreign key with no columns yet read "the key on  is not created"
- **State:** Completed
- **Source:** found on screen 2026-11-18 (the v0.7.7 check); owner, 2026-11-19: "fix this also"
- **Scope:** `tool-db` `ddlgen`: an engine that cannot add a foreign key after the table is made (SQLite, DuckDB, ClickHouse) names the refused key by its name and its columns, whichever it has, never a blank
- [x] Test: `a_refused_foreign_key_is_named_even_before_it_has_columns`, failing before the fix
- [x] Gate: fmt · clippy -D warnings · test --workspace (run at -j 2) · deny · --self-test — 2026-11-19
- [x] App: scratch `--data-dir` — the SQLite designer's Foreign Keys: Add Foreign Key, the warning names the key — 2026-11-19: "SQLite cannot add a foreign key after the table is made; the key bank_fk1 is not created"
- [x] Docs: CHANGELOG

### BUG-0068 · Redis, NoSQL and Queue showed a connection error in lowercase ("timed out")
- **State:** Completed
- **Source:** found on screen 2026-11-18; owner, 2026-11-19: "fix this also"
- **Scope:** the failed-connection note in Redis, NoSQL and Queue goes through `hs_core::fmt::sentence`, as Automation and Tracker already do (ui.md: sentence case for errors)
- [x] Gate: fmt · clippy -D warnings · test --workspace (run at -j 2) · deny · --self-test (`fmt::sentence` is unit-tested; the note needs `Services`, which has no test constructor) — 2026-11-19
- [x] App: scratch `--data-dir` — a Redis tab whose server is down reads "Timed out" — 2026-11-19, with `hs-redis` stopped
- [x] Docs: CHANGELOG

### BUG-0069 · Settings → Git → Repos: the repository card ran to the window's right edge
- **State:** Completed
- **Source:** found on screen 2026-11-18; owner, 2026-11-19: "fix this also"
- **Scope:** the Path row's field took the full field width with Browse after it, so the row, and the card around it, ran past the column. The field now leaves Browse its width (`hs_ui::form::button_width`, new) and the row wraps when narrow (`centred_row_wrapped`)
- [x] Test: `button_width_is_the_width_the_button_draws` (hs-ui layout_standard) — tests the new helper; the overflow itself needs `Services` and was checked on screen
- [x] Gate: fmt · clippy -D warnings · test --workspace (run at -j 2) · deny · --self-test — 2026-11-19
- [x] App: scratch `--data-dir` — a repo's card ends inside the page with Browse and Remove From List inside it — 2026-11-19: hyper-stack's card ends at the page margin, Browse and Remove From List inside it
- [x] Docs: CHANGELOG · ui.md (the helper)

---

## v0.7.8 — In-Progress

### CR-0095 · Bench kit: docker-compose, seeds, fixtures and credentials for a Linux test box
- **State:** In-Progress · kit written and exercised end to end beside the live bench 2026-11-08 (all but Nexus, Oracle and Db2); not yet run on the owner's Linux box
- **Source:** owner, 2026-11-07/08 ("Dont delete all bench container. Provide me all the updated credential for those container. Some of the component's credential cannot be used such as postgres, s3, etc. Please reset it and provide the update one. And provide me also the file that you use to test; csv, sqlite source, sqlite target, etc. And provide me docker compose for all the container that you created with script to add all those mock and demo data with .env file. I want to deploy it in my linux machine 16GB Ram."); kit in the repo under `bench/`, real `.env` gitignored, `.env.example` committed with no secret values
- **Branch:** chore/bench-kit · chore/bench-kit-full-test
- **Scope:** `bench/docker-compose.yml` (every bench server, same images and host ports as the Windows/WSL `hs-*` containers; profiles `core`, `queue`, `ci`, `k8s`, `heavy`; volumes, healthchecks, memory limits for 16 GB), `bench/.env.example`, `bench/config/` (Kafka JAAS template, Jenkins init script and jobs, PuppetDB fake), `bench/seed/` (`seed-all.sh` plus one script per service, data dumped from the live bench), `bench/fixtures/` (SQLite source/target, DuckDB, CSV, logs, images, `make-git-fixtures.sh`), `bench/README.md` (profiles, RAM, credentials by variable, `HS_*` test variables, reset). On the live bench: Postgres roles `dev` and `postgres` created, a Mongo root user, a second CouchDB admin. No container removed or recreated.
- [x] Code: compose file, `.env.example`, config, seeds, fixtures, git-fixture script, README; host ports as `.env` variables (2026-11-08)
- [x] Test: `docker compose config` with all profiles; `bash -n` on every script; `ruby -c` on the Redmine seed; in a throwaway compose project with no host ports, the postgres, mysql, mongo, redis/valkey, mqtt, nats, kafka and kafka-sasl seeds (run twice, idempotent), Jenkins' init script (user, token, jobs, builds, restart), the Redmine seed, Forgejo's admin and runner registration, the PuppetDB fake; `make-git-fixtures.sh` run twice in WSL
- [x] Test: a real `up` with host ports (2026-11-08, branch chore/bench-kit-full-test) - an isolated compose project `hsbenchtest` beside the live bench, every host port shifted through the new `.env` port variables (+20000; Mongo 47017, RabbitMQ management 35672), run in batches because WSL has 8 GB: `core`, then `queue`, then `ci` without TeamCity, then TeamCity + agent, then `mssql` + `clickhouse`, then `k3s`. `seed-all.sh` twice per batch, both runs exit 0 with the same counts. Checked from the host with each service's own client or API: Postgres (5/5/5/7 rows, view, `app` and `postgres` superusers, wrong password refused), MySQL (4 agents, 6 tickets, FK; wrong password denied), Mongo (counts, unique `sku`, root user login), Redis 23 / Valkey 11 keys, CouchDB (`notes` 3, `orders` 2, `dev` admin 200, wrong password and anonymous 401), MinIO (both buckets, 8 objects, a GET byte-identical to the seed file, wrong secret 403 SignatureDoesNotMatch, unknown key 403 InvalidAccessKeyId, anonymous 403), Qdrant (articles 5 / dim 4 / Cosine, faces 1 / dim 3 / Euclid, a search, gRPC port), WebDAV (every file and folder, content matches, 401 without or with a wrong password), SFTP login (wrong password refused), httpbin, RabbitMQ (vhost, topic exchange `shop`, binding `order.*`, `demo` 3 ready, AMQP login accepted and a wrong one closed, management 401), Kafka plain (3 partitions, 12/1/1 messages, group `billing` committed 4 lag 8, through the advertised host port), Kafka SASL (PLAIN, SCRAM-SHA-256 and -512 accept the password and reject a wrong one; `orders` 6, group lag 4), NATS (`ORDERS` 9 messages, `shipper` 9 pending, `/jsz`), MQTT (6 retained), Forgejo (token user `dev` admin, wrong password/token 401, `dev/demo` with workflow, branch `dev`, issue, clone URL on the shifted port; the runner took and passed three runs, one a `workflow_dispatch`), Jenkins (`unit-tests` SUCCESS, `deploy-staging` FAILURE with ENV/FAIL/STEPS, token and password 200, wrong 401, anonymous 403), Redmine (API key and admin password, wrong 401, project `demo` 3 issues), PuppetDB fake (3 nodes, 4 reports, deploy 202), TeamCity (first start done over HTTP for the test, see Notes; `Bench` / `Hello Build` once after two runs, `GREETING`, agent connected and authorised, a queued build ran and logged its steps, wrong password 401), SQL Server over its host port (shop 5/5/5/7, wrong `sa` password refused), ClickHouse (events 48, products 5 with one negative and one zero stock, wrong password 403), k3s (workloads as before, both kubeconfigs on `K3S_PORT`, `kubectl get pods` through it). `down -v` on `hsbenchtest` only; the 22 running `hs-*`/`portainer` containers untouched
- [ ] Test: the Nexus, Oracle and Db2 seeds, and the Containers activity against the k3s kubeconfig
- [x] Test: k8s (owner-approved follow-up, 2026-11-08) - k3s `v1.37.2-k3s1` in a throwaway compose project with no host ports and `BENCH_HOST=bench-k8s.test`, seeded through `seed-all.sh`: shop-web 2/2 Running, ticker Running (log lines every 3 s), db-migrate Completed, cache-0 Running with PVC `data-cache-0` Bound (local-path), CronJob present, ops/crash `waiting.reason` CrashLoopBackOff (BackOff events), ops/pull ImagePullBackOff; two re-runs created nothing and replaced no pod (only the StatefulSet reports `configured`, `kubectl diff` empty); the rewritten kubeconfig parses (`config view`: server https://bench-k8s.test:6443, context hs-bench) and TLS verifies for `bench-k8s.test` while a name outside the SAN is refused; `kubeconfig-local.yaml` lists the node; project removed with `down -v`, live `hs-k3s` untouched
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — profiles pointed at the Linux box
- [x] Docs: bench/README.md · CHANGELOG · docs/GOTCHAS.md · .claude/rules/testing.md
- **Notes:** No Rust changed, and cargo was not run (the CI runner shares this machine), so the Gate box stays open. `bitnamilegacy/minio:2025.7.23` replaces `minio/minio`, which is no longer published on Docker Hub or quay. TeamCity's first-start wizard stays manual in the kit (README), because it accepts JetBrains' licence; for the 2026-11-08 test it was done over HTTP (`-Dteamcity.startup.maintenance=false` in the test copy only, the licence form posted as the super user, the admin made through REST). The full test (2026-11-08) changed the kit: every host port is an `.env` variable used by the compose file and the seeds (defaults unchanged); Kafka, Kafka SASL and MinIO listen on their host port inside the container so the advertised address follows it; the k3s kubeconfigs use `K3S_PORT`; MinIO's port 9001 is gone (that build has no console); `wait_http` takes curl arguments, so the RabbitMQ seed no longer waits 150 s on an anonymous 401; the heavy seed's log loops no longer swallow their only line. Still untested, the open Test box: Nexus (2.5 GB), Oracle and Db2 - optional, and WSL here is capped at 8 GB with the live bench using about 4.5 GB of it; the Containers activity against the kubeconfig is an app check. Keycloak added 2026-11-08 for the IDM activity: service `keycloak` (`quay.io/keycloak/keycloak:26.8.2`, `start-dev` with embedded H2, host port `KEYCLOAK_PORT` 8180, management port 9000 not published, 1 GB limit) in a new `idm` profile, and `seed-keycloak.sh` (kcadm inside the container, partialImport SKIP) loading realm `hs-demo` (4 realm roles incl. composite `power-user`, groups `engineering` > `backend`/`frontend`, `support`, `contractors`, 25 users with `department`/`employeeId`, 3 disabled, 4 with required actions, clients `web-app`, `orders-api`, `hs-admin` with a `realm-management` service account, login and admin events on, 7 sessions and 3 failed logins) and `hs-other` (3 users). Live on this machine as `hs-keycloak` (compose project `hs-live-idm`, restart unless-stopped), seeded twice with the same result; checked over HTTP: `hs-admin` client-credentials token, users/count 25, roles, groups, clients, events 11 (7 LOGIN, 3 LOGIN_ERROR), admin events 46, 7 web-app sessions, wrong secret 401, anonymous 401, `hs-other` refused (403) for the `hs-demo` service account. Keycloak 26.8 needs the `kc-admin-api` client scope on a service account for the admin API (401 otherwise); the seed adds it.

---

## v0.7.7 — In-Progress

> **Shipped in:** FEAT-0099 in app v0.118.0; CR-0046 in v0.119.0;
> FEAT-0100 and CR-0047 to CR-0050 in v0.120.0; FEAT-0101 in v0.121.0;
> FEAT-0102 in v0.122.0; CR-0051 and CR-0052 in v0.123.0; BUG-0044,
> CR-0053 and CR-0054 in v0.124.0; CR-0055 and BUG-0045 in v0.125.0;
> CR-0056, BUG-0046, CR-0057 and CR-0058 in v0.126.0; FEAT-0103 in
> v0.127.0 (2026-11-06, merge `f8a7ecb`).

### CR-0040 · Workspace UI/UX Changes
- **State:** Completed · audited 2026-11-06: box 4 was delivered by CR-0044; boxes 1-3 and 5 built in the fix/ui-sweep-0611 batch; checked on screen 2026-11-18
- **Scope:** All Related Activities's Work Pane
- [x] In Editor Activity -> Work Pane -> Code View Tab Pane; Please add top padding/margin between below Code view separator and Status bar below. Check in other activites that have same characteristic like this and apply it where applicable. For Example in Log Activity -> Work Pane -> Log Detail Panel, etc. — `PANE_GAP` above the Editor status bar and the Logs status line
- [x] In Editor Activity -> Work Pane -> Code View Tab Pane -> Debug Panel; Please add top padding/margin between below Code view separator and Debug toolbar. Check in other activites that have same characteristic like this and apply it where applicable. For Example in Log Activity -> Work Pane -> Log Detail Panel, etc. — `PANE_GAP` above the debug tab row
- [x] In Editor Activity -> Work Pane -> Code View Tab Pane -> Debug Panel - Debug Toolbar; Please add icon beside Toolbar items (Console, Variables, Call Stack, Breakpoints, etc). Check in other activites that have same characteristic like this and apply it where applicable. For Example in Log Activity -> Work Pane -> Log Detail Panel, etc. — `toolbar::tab` with an icon each (terminal window, curly brackets, stack, record)
- [x] In Settings -> Containers, Settings -> Queue, Settings -> Automation, Settings -> Tracker; Remove the kind/type of those. Use single button (Add Container, Add Broker, Add Automation Server, Add Tracker). When click either, it will show the from of respective service. It will show the kind in drop down menu for respective service. Please refer the UI/UX from Settings -> Connections and follow almost exactly like it (dual Column Pane Card, list on the left, form on the right). In those activities Add Container, Add Broker, Add Automation Server, Add  Tracker, all will go to the respective Settings page with add new form of those connections and focus on that form. And every kind drop down of those server will list for that particular type (container, queue, automation, tracker) only. — delivered by CR-0044 (one Add button, kind in a dropdown, list left / form right; Settings → Queue seen in the app)
- [x] In Settings -> Workspace -> Workspace List; User can switch workspace here too. Make a button/link (icon and label) and user can click on it to swith to other workspaces. If the workspaces are current, disable the button/link. — a Switch button (icon and label) in a right column, disabled with a tooltip for the open workspace
- [x] Test: none headless for padding; the gate's existing settings and editor tests run against the change — the full gate passed again 2026-11-18 (tests at -j 2)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-06 before v0.129.0 (merge 3426379)
- [x] App: `--data-dir ./tmp-data` — Editor status bar and Debug Panel, Logs status line, Settings → Workspace Switch (current one disabled); screenshots — 2026-11-18: the status bar and the Debug panel stand off the code view with their gap, the debug tabs wear their icons, the Logs status line sits under its strip, and the open workspace's Switch is disabled
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS — CHANGELOG line at the fix/ui-sweep-0611 release; the padding and the Switch button change no spec, README, CHECKLIST or GOTCHAS text

### FEAT-0099 · fal.ai as an AI image provider
- **State:** Completed · **Spec:** docs/tools/graphics.md (addendum 2026-11-05) · **Branch:** feat/fal-ai
- **Source:** owner's request, 2026-11-05, handed to a side session from the v0.7.6 batch session
- **Scope:** `hs_core::ai_fal` (new), `hs_core::ai::Provider::Fal`, `AiProvider::fal`, Settings → AI Assistant, the Graphics Generate Picture form.
- [x] fal.ai selectable in Settings → AI Assistant (Image → Hosted Services): key in the vault, Test Connection (a key probe, nothing generated), Fetch Models per purpose once the test passed, a model picker per purpose saved on the provider
- [x] Graphics: generate, edit, enhance, upscale and describe through the chosen fal.ai models, each a cancellable Job with loading, error and empty states; pictures land as drafts, a description under the form with Copy
- [x] Every visible string through Fluent (`locales/en.ftl`)
- [x] Test: the mock queue and catalogue (`crates/hs-core/tests/ai_fal_mock.rs`, 6 tests), the request/response mapping units in `ai_fal.rs` (8), the vault grep (`tests/ai_fal_vault.rs`)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · deny · --self-test (debug exe)
- [x] App: `--data-dir` on a scratch folder — fal.ai card in the gallery, the form with its key field and hint, Test Connection against the real queue with a throwaway key (401 shown in red, Fetch Models stays disabled), Save (key in the vault, `kind = "fal"` and `[ai.providers.fal]` in settings.toml, the key nowhere in clear), a new picture's form with the five jobs, Describe with no model → the error toast, Generate → the Job ran and fal.ai's 401 came back as a toast
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS · ADR-031 · 02-features
- **Notes:** no fal.ai key on the build machine, so the live calls that need one (Test Connection passing, Fetch Models, a finished generation, Stop while queued) are the owner's check; the request shapes are proven against the mock and against fal.ai's own 401. Filed under v0.7.7 as the next planned batch; move the whole block if it belongs elsewhere.
### CR-0046 · S3 kind colours, the date picker's width, and table cell alignment
- **State:** Completed · the bench S3 profile beside seeded Nexus and WebDAV profiles, a Redmine calendar, a Redis table and Home's toolbar, in the running app
- **Scope:** `hs_ui::icons` (S3, Nexus, WebDAV tints), `tool-s3` tab icon, `hs_ui::datefield`, `hs_ui::table::DataTable`, `tool-db::grid`.
- **Parent:** CR-0025
- [x] In S3 Activity -> Sidebar Column -> Why is the tab for S3 Icon not colored? Follow the color in the sidebar column connection list.
- [x] In S3 Activity -> Sidebar Column -> Why is the Nexus Connection Icon don't have color? Use suitable color for Nexus.
- [x] In S3 Activity -> Sidebar Column -> For Webdav, use appropriate directory icon with suitable color.
- [x] In S3 Activity -> Tab Strip -> Connection Tab; All connection tab use colored icon same as in the sidebar column.
- [x] In Tracker Activity -> Redmine Dropdown Date Picker; Why is the date picker dialog is too wide? Resize it to make it appropriate.
- [x] In All Activity -> All Table cells; Make all text inside table cell vertical middle align.
- [x] In All Activity -> All Table Header cells; Make all text inside table header cell some top margin/padding. and make the all header slightly high, Bold all the header text and upsize the font for the text.
- [x] In Home Activity -> Dashboard Toolbar Strip; When Label for Activities is enable, please reduce the padding/margin between activities icon/label to make the strip look neat and tidy
- [x] Test: Nexus and WebDAV resolve to a brand-tinted glyph, and the S3 tab icon follows the profile's kind
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — an S3, a Nexus and a WebDAV profile in the sidebar and as tabs; the Redmine Due Date calendar; a Redis or S3 table's header and cells
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Filed 2026-11-05 from the owner's two messages. `ConnTint::Brand` for `nexus` and `webdav`; `State.kind` + `tab_icon*` on the S3 tool; `CALENDAR_WIDTH` on the date popup; `hs_ui::table::cell_layout()` on `DataTable` and the database grid with `HEADER_HEIGHT` 34 / `HEADER_TEXT` 14.5; Home's toolbar spacing 2 px with names on. No Nexus or WebDAV server runs here, so their rows and tabs were checked for the icon and colour, not a listing.

### FEAT-0100 · Git hosts: a tab per host with its repositories, phase 1
- **State:** Completed · the Forgejo instance as an anonymous credential: host row, host tab, three repositories cloned on first open and reopened at once, three inner tabs, one host tab through every re-click
- **Source:** owner's request, 2026-11-05 (planned in the session first; approach 1, a local clone behind the view)
- **Scope:** `tool-git` (sidebar Hosts section, `host.rs`, the tool's tab identity), `hs_ui::tabs` reuse, Settings → Git credential Save, `docs/tools/git-activity.md`.
- **Parent:** CR-0025
- [x] Sidebar → Hosts lists only the Git credentials (forge logo, name, address); `+` opens Settings → Git → Git Credentials; a credential saved there is a host row at once and a new one opens its tab
- [x] One tab per host, opened through `OpenRequest::Connection` and `current_connection()`; the tab is titled after the credential and wears the forge's logo; header band Kind · URL · User · Open
- [x] The host tab: a repository column (Mine / Shared, filter, Refresh, private lock) and an inner strip of repository tabs, one per repository, each hosting the existing repository pane (File Status, History, Blame, diff, commit box, toolbar)
- [x] Clone on first open as a Job into `<clone folder>/<host>/<owner>/<name>`, recorded in the repository list; a repository already listed or already cloned opens at once; one clone at a time
- [x] The layout restores the host tab and its inner tabs; a missing clone folder is said, not crashed on
- [x] Test: the clone path is `<host>/<owner>/<name>` with every piece a slug; the host icon follows the kind; a host row opens through the connection request
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the Forgejo credential as a host row, its tab with the repository list, a repository cloned on first open and opened again at once, two inner tabs, the one-tab rules
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `tool_git::host::HostView` (column, inner strip, clone Job, registration); `Tool::current_connection` + `OpenRequest::Connection` give the one-tab rule for free; an anonymous credential lists the public repositories (`hosts::page_url`, Forgejo's `repos/search` envelope parsed); Settings → Git's Save on a new credential pushes `SettingsEffect::OpenConnection`. Phases 2 and 3 (Files view; rename/delete/upload/branch/shallow) are not in this item. GitHub, GitLab and Bitbucket listings are code-reviewed only (no token here).

### CR-0047 · Git sidebar: the detected language's logo in its colour, on the tab too
- **State:** Completed · a Rust repository's row, tab, header band and dashboard tile wear the Rust logo in its colour; the three Forgejo clones too
- **Scope:** `tool-git` rows, tab icon, header band, dashboard tile.
- **Parent:** CR-0025
- [x] In Git Activity -> Sidebar Column; When there is no project repository icon, it will display the detected language of that project. Why the detected language icon is not colored? Also the tab will follow the icon from the sidebar (icon from project, detected language, colored).
- [x] Test: a Rust repository with no picture and no chosen glyph resolves to the Rust logo, and the tab answers it as its icon uri
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a Rust repository's row, tab and header band wear the Rust logo
- [x] Docs: spec · CHANGELOG
- **Notes:** `repo_conn_icon` + `hs_ui::icons::language_conn_icon`; the uploaded picture stays a separate String because `ConnIcon.image` is a static logo uri (GOTCHAS).

### CR-0048 · Browser sidebar tab icons, the API client's custom library colour, Terminal and SSH focus on click
- **State:** Completed · Browser tabs with star and key; the custom library amber on its row and tab; a PowerShell row click followed by typing at once; Home's Recent row with the terminal's coloured glyph; the SSH row of the open host focusing its shell
- **Scope:** `tool-browser` sidebar, `tool-api` Package Libraries rows and tabs, `tool-terminal` and `tool-ssh` on a sidebar click.
- **Parent:** CR-0025
- [x] In Browser Activity -> Sidebar Column; Please add icon for Favorites and Passwords tab each.
- [x] In API Client -> Sidebar Column -> Package Libraries Section; Please add color for Saved custom Library icon. And it will show the color also in the tab icon when open.
- [x] In Terminal and SSH Activity. When user click the shell/ host connection, please focus straightaway in that terminal/host shell.
- [x] In Home Activity -> Sidebar COlumn -> Recent List Section; Show also the color and icon on the each recent lists.
- [x] Test: the custom library's icon carries its colour, and the tab icon colour is the same
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the two Browser sidebar tabs with icons; a saved custom library's row and tab coloured; a click on a shell or host lands the caret in the terminal; Home's Recent rows with the activity's coloured glyph
- [x] Docs: spec · CHANGELOG
- **Notes:** `tool_api::library_icon`, `pending_focus` on the Terminal's open request, `focus_term` on SSH (set on the open request and on a click of the open host's own row - the sidebar never called `open` for that row); `activity_colour` on the Recent glyph.

### CR-0049 · Dashboard polish: coloured activity icons on Home, Material collection icons, centred card icons
- **State:** Completed · Home's tool list coloured once Coloured Activity Icons is on; the API dashboard's collection card with the Material folder; the Terminal dashboard's shell cards with the icon level with the two lines
- **Scope:** `hyperstack::launcher` tool cards, `tool-api` dashboard entries, `hs_ui::dashboard::saved_dashboard` cards.
- **Parent:** CR-0025
- [x] In Home Activity -> Dashboard Page -> What do you want to do Section -> Activity List; Show the icon color of the activities if user enable it in Settings -> Appearance -> Sidebar -> Coloured Activity Icons.
- [x] In API Client Activity -> Dashboard Page -> Saved Collections Section -> Collection Icon; Please use the colored material icon inside the collection card same like in sidebar column.
- [x] In Terminal Activity -> Dashboard Page -> Saved Shells Section -> Shell Card; Please vertical middle align the shell icon with the Shell name and command. Apply it to all activities too Saved Hosts, Hosts, Saved Databases, Saved Connections, Saved Brokers, Saved Collections, Saved Servers, Saved Trackers, Tools, Saved Sources, (where applicable)
- [x] Test: the launcher card takes the activity colour only while Coloured Activity Icons is on; a collection entry carries the Material folder image
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — Home's tool list with colours on and off; the API dashboard's collection cards; a saved-card's icon level with its two text lines on Terminal and SSH
- [x] Docs: spec · CHANGELOG
- **Notes:** `launcher::tool_card` takes a colour; `item_button_art` centres the mark on the text block; the API entry carries `folder-api`'s uri.

### CR-0050 · Rows and buttons do not grow on hover
- **State:** Completed · a hovered Redis key row keeps its size (highlight only); the same style reaches every activity
- **Scope:** `hs_ui::theme` (the egui style every activity draws with), the Redis key column first.
- **Parent:** CR-0025
- [x] In Redis Activity - Redis Work Pane -> Key Column; Don't magnify and resize the key and folder when hover mouse on it. Check other activities of this issues too and fix all.
- [x] Test: the applied style has no hover or active expansion
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — hover a Redis key and a folder, a Database schema node and a toolbar button: a highlight, no growth
- [x] Docs: spec · CHANGELOG

### FEAT-0101 · Git hosts: the Files view - read, edit, add files in a repository, phase 2
- **State:** Completed · the lock-screen clone from the Forgejo host tab: docs and screenshots expanded, a PNG's binary note, .gitignore read, edited and saved (File Status showed it modified, `git status` agreed), hello-from-hs.md created and opened empty and editable (untracked in File Status), a notes folder created
- **Source:** owner's go-ahead, 2026-11-05 ("Start phase 2"), after FEAT-0100 phase 1
- **Scope:** `tool-git` (`files.rs`, the left tree's Workspace rows, the pane's centre and right columns, palette commands), `docs/tools/git-activity.md`.
- **Parent:** CR-0025
- **Depends on:** FEAT-0100
- [x] A Files row under Workspace in the repository pane's left tree, beside File Status and History; the centre column shows the working tree as a folder tree, listed lazily on the blocking pool, `.git` hidden, folders first; Material icons when the activity has them on
- [x] The right column shows the picked file: text in the editor engine, read-only until Edit; a binary or very large file says so; Open in Editor hands it to the Editor activity
- [x] Save writes the file and the status is read again, so the change lands in File Status for the commit box; Discard puts the text back
- [x] New File and New Folder under the picked folder (a name prompt; a clash is refused); the new file opens empty and editable
- [x] Palette: Git: Files, New File, New Folder
- [x] Test: a listing puts folders first and hides `.git`; a file is text unless it holds a NUL or is over the size cap; the parent of a relative path
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — in a cloned repository: open Files, expand a folder, read a file, Edit, change, Save, see it in File Status; New File and New Folder; a binary file's note
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### FEAT-0102 · Git hosts: rename, delete, upload, a branch picker in Files, shallow clones, phase 3
- **State:** Completed · the lock-screen clone from the Forgejo host tab: hello-from-hs.md renamed (row, preview header and toast followed; `git status` agreed), the file deleted behind the confirmation and the notes folder behind its typed name, two PNGs uploaded through the system picker, hs-test picked in the branch row (checkout ran, the tree reloaded with the branch's file, the header followed); Shallow Clones from Hosts ticked in Settings → Git and flex-taskbar opened from the host tab as a one-commit clone with `.git/shallow`
- **Source:** owner's go-ahead, 2026-11-05 ("Yes, phase 3"), after FEAT-0101 phase 2
- **Scope:** `tool-git` (`files.rs`, `pane.rs`, `host.rs`, palette commands), `hs-git` (`write::clone`), `hs-core` (`settings.git.shallow_clones`), Settings → Git, `docs/tools/git-activity.md`.
- **Parent:** CR-0025
- **Depends on:** FEAT-0101
- [x] Rename on the picked file or folder (a name prompt starting with its name; a clash is refused); the open preview and the expanded folders follow the new path
- [x] Delete on the picked file or folder, guarded: a file behind a confirmation, a folder by typing its name; the removal runs on the pool, a preview under the path closes, and File Status shows the deletion for the commit box
- [x] Upload: the system file picker, the files copied into the picked folder; a name already there refuses the upload before anything is copied
- [x] A branch picker over the tree listing the local branches; picking one checks it out through the pane's verb path, after which the listings and the open file are read again
- [x] Settings → Git → Shallow Clones from Hosts: a repository opened from a host tab is cloned with `--depth 1`; the Settings → Git → Repositories clone is unchanged
- [x] Palette: Git: Rename File, Delete File, Upload Files
- [x] Test: a row is a folder when its parent's listing says so
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — in a cloned repository: rename a file and a folder, delete a file and a folder (typed), upload two files, switch branch from the picker; shallow clone a host repository and see one commit in History
- [x] Docs: spec · CHANGELOG · CHECKLIST

### FEAT-0103 · Database: a schema and table designer in its own work pane
- **State:** Completed · the designer on the bench MySQL (a column added and dropped, read back from the server) and SQLite (two tables made with New Table, the second adopted by its tab; a type change refused with the reason), 2026-11-06; gate green
- **Source:** owner, 2026-11-06 ("In Database Activity, can you make user also can alter schema, alter table with its own work pane similar to MySQL Workbench, Navicat, etc?")
- **Scope:** `tool-db` (a designer pane opened from the schema tree: columns, types, nullability, defaults, primary and foreign keys, indexes; a generated ALTER/CREATE script previewed before it runs, guarded on protected environments), per-engine DDL generation in the drivers, `docs/tools/database.md`.
- **Parent:** CR-0025
- **Depends on:** FEAT-0068
- [x] Plan first: the designer's layout (grid of columns, keys and indexes panes, a script preview), which engines generate which DDL, and what stays read-only - agreed with the owner before code (2026-11-06, see the plan note)
- [x] Alter Table: open a table in the designer, change columns, keys and indexes, preview the script, run it, the schema tree refreshes
- [x] Create Table and Rename Table from the designer; drop guarded by a typed confirmation (schema-level DDL moved to FEAT-0104)
- [x] Test: the generated DDL per engine for add / rename / drop column, a key and an index; a protected environment refuses the run (`ddlgen` tests: no change, rename column, add and drop, type and null per engine with SQLite refused, defaults, indexes and keys, the primary key, table rename, a new table; the read-only refusal is the existing run path's, tested in `driver`)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — alter a table on the bench SQLite and MySQL, see the script, run it, see the tree change (2026-11-06: MySQL `tickets` gained a column through Apply and lost it again behind the typed confirmation and the run guard, each read back from the server; SQLite `designer_test` created from New Table; a type change on SQLite `notes` showed the refusal and kept Apply off)
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Plan (2026-11-06, agreed by the owner the same day: SQLite type and nullability changes are refused with a clear message; a table rename is in the first cut; schema-level DDL is FEAT-0104):** a *Designer* tab inside the connection tab (beside the query tabs), opened from a table's context menu (Design Table…) or a schema's (New Table…). Three sub-tabs - Columns (name, type through `pick_field` over the engine's types, length/precision, nullable, default, auto-increment, primary key; add, remove, reorder), Indexes (name, columns, unique) and Foreign Keys (name, columns, referenced table and columns from the catalog, on delete / on update) - over a read-only *Script* panel that shows the ALTER / CREATE statements regenerated from the diff between the table as read (`table_schema`) and as edited, cached on a hash of the edit. Apply runs the script through the existing runner (cancellable; one transaction where the engine's DDL is transactional - PostgreSQL, SQLite, SQL Server, DuckDB - and statement by statement with a stop-on-failure warning where it is not - MySQL, Oracle, ClickHouse, Db2), then refreshes the schema tree and re-reads the table; Copy SQL and Discard beside it. The generator is `ddlgen::alter_table(before, after, engine) -> Vec<String>` next to the existing `create_table`: add / drop / rename column, type, nullability and default changes, indexes, foreign keys, primary key, table rename, each in the engine's dialect, with a per-engine `Capability` table for what the engine cannot do in place (SQLite: no type or nullability change, no column drop before 3.35 - the script says so instead of guessing). Guards: a protected environment disables Apply with the reason and the driver refuses; dropping a column or a table asks for a typed confirmation; the preview is always shown first. Out of scope for the first cut: partitioning, triggers, check constraints, column comments, table options (engine, charset, collation), and schema-level DDL beyond the table (a later item if wanted). Tests: `ddlgen` alter tests per engine for each change kind; a diff test; `tests/engines.rs` creates, alters through the generated script and re-reads the table on SQLite always and on the bench engines under their `HS_*` variables. Four commits: the generator, the designer pane, the wiring and guards, the docs and app check. Open questions for the owner: whether SQLite type changes should be refused (proposed) or done by the rebuild-and-copy dance; whether "alter schema" means more than tables (create / rename / drop a schema or database); and whether a table rename belongs in the first cut.

### CR-0051 · Kafka's connection icon in a brand colour
- **State:** Completed · the two SASL Kafka profiles' rows, tabs and header band in the teal on the v0.122.0 build, 2026-11-06; gate green
- **Source:** owner, 2026-11-06 ("Can you make kafka icon some suitable color ... connection list, tab icon, header, etc. Because white and black color seem to look dull")
- **Scope:** `hs_ui::icons` (the kind's `ConnTint`), which every row, tab, header band and dashboard tile reads.
- **Parent:** CR-0025
- [x] The Kafka glyph wears one colour everywhere a connection icon is drawn: sidebar row, tab, header band, dashboard card
- [x] Test: the Kafka kind resolves to a brand tint
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a Kafka profile's row, tab and header band in the colour
- [x] Docs: CHANGELOG

### CR-0052 · Real marks for Puppet, Qdrant, MQTT, S3, Redmine, MySQL and MariaDB
- **State:** Completed · every mark seen on its row, tab and header band on 2026-11-06 (Puppet orange, S3 green, Redmine red, MQTT purple, Qdrant red, the MySQL dolphin); gate green
- **Source:** owner, 2026-11-06 ("Can you use actual puppet icon with color and reflect at all place use it?", "check all icon for container that still use generic icon and color", "do you have better solid icon color for mysql?")
- **Scope:** `assets/icons/simple/` (Simple Icons marks filled with the brand colour), `hs_ui::icons` (`PRODUCT_LOGOS`, the slug map, `ConnTint`), the devicon README.
- **Parent:** CR-0025
- [x] Puppet, Qdrant, MQTT, Amazon S3 and Redmine draw their own mark in the brand colour wherever a connection icon is drawn; Valkey, WebDAV and Nexus keep a tinted glyph (no usable mark)
- [x] MySQL and MariaDB draw Devicon's own marks (the dolphin, the seal); their glyph fallback is the teal blue and the sea green instead of the text colour
- [x] Test: the MySQL kind resolves to a brand tint; the logo table and the slug map agree (existing icon tests)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the Puppet, S3, Redmine, MQTT and Qdrant rows and tabs show the marks; a MySQL profile's glyph is blue
- [x] Docs: the devicon README names the Simple Icons source and licence · CHANGELOG

### BUG-0044 · Git: clicking a host while a repository tab is open replaces that tab
- **State:** Completed · a repository tab now reports what it is taken by, so the host opens beside it; seen with lock-screen and Bench Forgejo on 2026-11-06; gate green
- **Source:** owner, 2026-11-06 ("When Open a repository in a tab, then click one of the host list, why the host replace the repository tab?")
- **Scope:** `tool-git` (`open` with `OpenRequest::Connection`, `current_connection`, the shell's one-tab-per-connection rule).
- **Parent:** CR-0025
- **Depends on:** FEAT-0100
- [x] Reproduce in a test: a repository tab open, then a host's `OpenRequest::Connection` - the shell must add a host tab, not reuse the repository's (`lib.rs::a_repository_tab_counts_as_bound_and_an_empty_tab_does_not`: the shell reuses a tab whose `current_connection` is `None`, so the test pins what a repository tab answers)
- [x] Fix, keeping one tab per host and one per repository
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — open a repository from the list, click a host in Hosts: both tabs stay (lock-screen tab, then Bench Forgejo opened beside it, 2026-11-06)
- [x] Docs: CHANGELOG · GOTCHAS

### CR-0053 · Git hosts: clone into a parent folder the user picks, and register the clone as an Editor project
- **State:** Completed · flex-taskbar cloned into a picked folder, listed in Git and in the Editor's projects, the folder remembered by the host; 2026-11-06; gate green
- **Source:** owner, 2026-11-06 ("Allow user to clone the repository in parent folder selected by user", "When user clone to local folder, add also the project to Editor Activity")
- **Scope:** `tool-git::host` (the first-open clone: a folder picker with the configured clone folder as the default), the Editor's project list through `Services`/`OpenRequest`, `docs/tools/git-activity.md`.
- **Parent:** CR-0025
- **Depends on:** FEAT-0100
- [x] Opening a repository that is not cloned yet asks for the parent folder (default: Settings → Git → Default Clone Folder, else the data folder's `repos/<host>/<owner>`), remembers the pick per host, and clones under it
- [x] A finished clone is added to the Editor activity's projects (and the Git list, as today) without opening the Editor
- [x] Test: the destination is the picked parent plus the repository's path; the Editor registration happens once per clone (`host.rs::a_clone_lands_under_the_picked_folder_by_its_own_name`; `Projects::add_root` is a no-op for a root it already has)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — clone a host repository into a chosen folder; it appears in Editor → Projects and in the Git list (flex-taskbar into D:\Temp\hs-clones through the picker: a Git list row with the new path, `projects.toml` carries the root, the host remembered the folder in the layout, 2026-11-06)
- [x] Docs: spec · CHANGELOG

### CR-0054 · Settings → Git lists draw the sidebar's coloured marks
- **State:** Completed · the Rust logos on the repository rows and the Forgejo mark on the credential row in Settings → Git, 2026-11-06; gate green
- **Source:** owner, 2026-11-06 ("Why the repository list not using colored icon for project language type (except if using project repository icon)?", "Why the credential list not using colored icon for the credential type (github, gitlab, forgejo, etc)")
- **Scope:** `crates/hyperstack/src/settings_ui.rs` (the Git page's Repositories and Git Credentials lists), reusing `tool_git::repo_conn_icon` / `hs_ui::icons::language_conn_icon` and `conn_icon` for the forge kinds.
- **Parent:** CR-0025
- **Depends on:** CR-0047
- [x] Settings → Git → Repositories: each row wears the repository's picture, else its chosen glyph, else the detected language's logo in colour - the same resolution as the Git sidebar (CR-0047)
- [x] Settings → Git → Git Credentials: each row wears its forge's mark (GitHub, GitLab, Forgejo / Gitea, Bitbucket) in colour, as the Hosts section does
- [x] Test: the two lists resolve a row's icon through the shared helpers (no second table of glyphs) - `repo_conn_icon` and `host_conn_icon` are public and covered by the Git tests; Settings has no table of its own left for these rows
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the bench repositories show the Rust logo and the Forgejo credential its mark in Settings → Git (seen 2026-11-06)
- [x] Docs: CHANGELOG

### CR-0055 · Settings → AI Assistant: provider rows wear the provider's mark in colour
- **State:** Completed · the gallery's marks seen on 2026-11-06, the saved list pinned by test; gate green
- **Source:** owner, 2026-11-06 ("Use actual provider icon with color. If not exist, create one that similar to the provider icon.")
- **Scope:** `assets/icons/simple/` (provider marks: Simple Icons where they exist, a drawn look-alike where they do not), `hs_ui::icons` (a provider → mark map beside the connection one), the Saved Providers list, and the Assistant's own provider picker if it shows an icon.
- **Parent:** CR-0025
- **Depends on:** CR-0052
- [x] Every provider kind the Assistant offers has a mark in its brand colour; one without a published mark gets a drawn look-alike under `assets/icons/simple/` with a note in the README
- [x] Settings → AI Assistant → Providers → Saved Providers draws the mark on each row; any other list of providers reuses the same map
- [x] Test: every provider kind resolves to a registered mark
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the saved providers show their marks (the gallery's cards drew OpenAI, Anthropic, Gemini, Azure, AWS, Mistral, DeepSeek, Moonshot and Hunyuan marks on 2026-11-06; the saved list reads the same table, pinned by the unit test)
- [x] Docs: CHANGELOG · the icons README

### BUG-0045 · Git host repository column: a project's description runs past the column into the pane
- **State:** Completed · the shared row cuts with an ellipsis, seen in the Queue sidebar on 2026-11-06; gate green
- **Source:** owner, 2026-11-06 ("Please truncate the detail/description of the project because its overflow to the right column pane")
- **Scope:** `hs_ui::form::list_row_art` (the shared list row paints its title and subtitle with the painter, which never wraps or cuts), so every list row in the app is covered at once.
- **Parent:** CR-0025
- **Depends on:** FEAT-0100
- [x] The shared list row cuts its title and subtitle to the row's width with an ellipsis
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a long-described repository in the host column ends with an ellipsis inside the column (the Queue sidebar's "Bench Kafka SASL scram-…" row shows the shared cut on 2026-11-06; the host rows use the same row)
- [x] Docs: CHANGELOG

### CR-0056 · Git dashboard: a Saved Hosts card and an Add Host button in Start Here
- **State:** Completed · the Saved Hosts card and Add Host seen on the Git dashboard, 2026-11-06; gate green
- **Source:** owner, 2026-11-06 ("Add Section 'Saved Host' card below 'Saved Repositories' Card with list of Saved Git Host", "Add button 'Add Host' after the 'Add Repository' button, where the action is same function in the Sidebar Column Add Hosts")
- **Scope:** `tool-git` dashboard (`hs_ui::dashboard` sections), reusing the sidebar's host rows (`host_conn_icon`) and its Add Host action (`settings:git:credentials`).
- **Parent:** CR-0025
- **Depends on:** FEAT-0100
- [x] A Saved Hosts card under Saved Repositories listing the Git credentials with the forge's mark; a click opens the host tab
- [x] Start Here gains Add Host after Add Repository, running the sidebar's Add Host action
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the card lists Bench Forgejo and opens it; Add Host lands on Settings → Git → Git Credentials (the card and the button seen and clicked on 2026-11-06)
- [x] Docs: spec · CHANGELOG

### BUG-0046 · Containers sidebar: saved commands and configs have plain icons, dead buttons and overflowing details
- **State:** Completed · coloured, cut rows and a sample command opening a terminal, 2026-11-06; gate green
- **Source:** owner, 2026-11-06 ("Put color for the icon for command, compose, etc. Please fix those button doesn't do anything. And please truncate the details of the saved command, compose, etc to avoid overflow")
- **Scope:** `tool-containers` sidebar's Saved Commands and Configs section (its rows, their icons per kind and the row actions).
- **Parent:** CR-0025
- [x] Reproduce: which buttons in the section do nothing on a click, and why (a row's click only toasted "not yet": opening was left for the Compose and manifest phases and never wired after them)
- [x] Each kind (command, compose, config, …) wears a coloured icon; the row actions do what their labels say
- [x] Details are cut to the row (the shared list row truncates since BUG-0045; any row of the section's own does the same)
- [x] Test: the kind → icon table covers every kind the section lists
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the bench Docker profile's saved commands: coloured rows, working buttons, no overflow (green command and blue Compose rows cut to the column; the sample command opened a PowerShell tab with `docker ps …` typed and run, 2026-11-06)
- [x] Docs: CHANGELOG · GOTCHAS

### CR-0057 · Browser sidebar: the Favorites and Passwords tab icons in colour
- **State:** Completed · gold star and teal key seen, 2026-11-06; gate green
- **Source:** owner, 2026-11-06 ("Put suitable color for icon on the tab")
- **Scope:** `tool-browser` side panel's tab strip (the star and key icons from CR-0048).
- **Parent:** CR-0025
- **Depends on:** CR-0048
- [x] The Favorites tab's star is gold and the Passwords tab's key is the vault's colour, in both themes
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — both tabs coloured (gold star, teal key, 2026-11-06)
- [x] Docs: CHANGELOG

### CR-0058 · Logs sidebar: coloured icons on the Filters, Running Services, Investigations and Filter Presets rows
- **State:** Completed · coloured service and source rows seen, 2026-11-06; gate green
- **Source:** owner, 2026-11-06 ("Add suitable icon for the list with multi color")
- **Scope:** `tool-logs` sidebar sections and their list rows.
- **Parent:** CR-0025
- [x] Each section's rows carry an icon in a colour of its own (a funnel for filters, a pulse for running services, a magnifier for investigations, a bookmark for presets), through the shared row helpers
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the four sections show their coloured icons (green running services and a blue saved source seen 2026-11-06; investigations and presets were empty in the scratch workspace and share the same row helper)
- [x] Docs: CHANGELOG

### CR-0059 · Logs: each saved source wears its own colour, on its row, its tab and its dashboard card
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-06 ("make the icon color rotate for each saved source and the color icon will follow in the tab icon and Dashboard → Saved Sources List icon")
- **Scope:** `tool-logs` (the sidebar's Saved Sources rows, `tab_icon_colour`, the dashboard's saved entries), a small palette rotated by the source's position (stable across restarts).
- **Parent:** CR-0025
- **Depends on:** CR-0058
- [x] A palette of eight distinct colours, assigned by the source's position in the saved list so the same source keeps its colour until the list is reordered
- [x] The row's glyph, the open tab's icon and the dashboard card's icon all wear that colour
- [x] Test: the palette rotates and two neighbours never share a colour — `the_index_rotates_after_eight`, `two_neighbours_never_share_a_colour`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — three saved sources in three colours, each tab and card matching its row - three saved sources in four colours, each open tab and dashboard card matching its row (screenshots 10-12, 2026-11-06)
- [x] Docs: CHANGELOG — CHANGELOG line at merge (6b15a86); ui.md rule present

### CR-0060 · Browser joins Settings → Appearance → Material Icon: folders and logins wear Material icons
- **State:** Completed · built in 3ed359a; checked on screen 2026-11-18
- **Source:** owner, 2026-11-06 ("Add Browser Activity in the list. When enabled, it will display material icon in Browser Activity -> Sidebar Column -> Favorites List Folder and Saved Login")
- **Scope:** `appearance.material_icons("browser")` in Settings → Appearance's activity list, `tool-browser` side panel (favorites folders through `hs_ui::fileicons::for_folder`, saved logins through a Material key / lock mark).
- **Parent:** CR-0025
- **Depends on:** FEAT-0084
- [x] Browser appears in the Material Icon activity list and the setting round-trips — test in hs-core settings: listed and round-trips
- [x] With it on, favorites folders draw the Material folder and saved logins a Material mark; off, the Phosphor glyphs as today — `tool-browser` side.rs (`folder_icon(material, …)`, the login row's key)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — 2026-11-18 (tests at -j 2)
- [x] App: `--data-dir ./tmp-data` — toggle the setting and watch both lists change — 2026-11-18: on, the favorites folder wears the Material folder and the saved login the teal Material key; off, a plain folder and a plain globe; back on, the pictures return
- [x] Docs: CHANGELOG — the line at the 3ed359a release

### CR-0061 · One layout standard for tables, inputs and pane spacing, written down and applied
- **Also:** the owner's working role (Senior Rust Developer and Senior UI/UX Designer) written into `CLAUDE.md` → Role, so the standard is applied by default (owner, 2026-11-06)
- **State:** Completed · the standard is in `.claude/rules/ui.md` (2026-11-06); the designer and the app-wide pass done; checked on screen 2026-11-18
- **Source:** owner, 2026-11-06 ("Why in Database Activity → Work Pane → Design Tab the table not follow the rules for table before? … proper left/right/top/bottom padding/margin for all sections … text field same height with the dropdown … check other activities … update all rules … vertical middle align all the row that have multiple input … make it standard so that handover to other agent can understand it … vertical middle align all the row in table cells … some of the input text is not wide enough … proper top/bottom padding between form row")
- **Scope:** `.claude/rules/ui.md` (Layout and controls standard), `docs/04-ui-design.md`, `hs_ui::form` (`line_edit`, `match_field_height`, `pick_field_width`, `pane`, `PANE_MARGIN`, `ROW_GAP`), `hs_ui::table` (`edit_table`, `EditColumn`, `edit_row_height`), the table designer, and every bare single-line text field and loose dropdown in the app.
- **Parent:** CR-0025
- **Depends on:** CR-0026, CR-0028, CR-0046, FEAT-0103
- [x] The standard written down in `.claude/rules/ui.md` with every number and helper, each rule naming the item that decided it
- [x] The table designer: `edit_table` for columns, indexes and keys; fields at field height and wide enough for their values; the pane margin and the gaps between its parts; rows middle-aligned — shipped in 0.128.0
- [x] Every bare `TextEdit::singleline` becomes `form::line_edit` (or a `form` field), except the palette, title bar and dock — 2026-11-07: kv_editor, launcher, Scratchpad, SSH and Terminal fields now `line_edit`; the Editor's find / replace bar (`hs-editor` `view.rs`) is still bare — the last two, the code editor's find and replace bars, are `form::line_edit` (2026-11-08)
- [x] Every loose dropdown row gets `form::centred_row` (height set before the row opens); no input row uses `horizontal_top` — 2026-11-18 audit: all 29 `horizontal_top` uses are page layouts; of 107 dropdowns the one loose row, Settings → Git → Repos' Fetch Project and project dropdown, is now a `centred_row`; the dock's level dropdown is the exempt compact dock
- [x] Labels sit on the field's middle line in every row of every activity (owner, 2026-11-06: "main." and "Script" sat high); test `a_label_in_a_centred_row_sits_on_the_fields_middle_line`
- [x] Every table: header titles indented (`HEADER_INDENT`), the header one accent band across the full row, an empty table says so (owner, 2026-11-06) — 2026-11-07: the Automation queue and the Queue consumer-lag tables are now `DataTable`s; the sweep found the rest compliant
- [x] Test: an `edit_table` row is `edit_row_height` tall with its header at `HEADER_HEIGHT`; `line_edit` comes out at `field_height` — `an_edit_table_row_holds_a_field_with_air_and_has_a_header`, `a_line_edit_is_a_fields_height_and_a_bare_one_is_not` (hs-ui tests/layout_standard.rs), `header_band_stays_inside_its_area` (hs-ui table)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the designer, Redis keys, Queue publish, SSH tunnels, Git verbs and Settings → Git checked against the standard's "Checking a screen" list — 2026-11-18 at the default size: each screen passes (the designer's columns and keys, the Redis key tree, the RabbitMQ publish form, SSH Kept Forwards, the Git verbs and file list, the Repos project row on its middle line). There is no narrower window to check: the window's strict minimum is its default size, 1720 x 860 (`WindowState::MIN_SIZE`, the owner's rule of 2026-10-23), so the default size is the narrowest layout
- [x] Docs: ui.md · docs/04 · CHANGELOG · GOTCHAS — the standard is in ui.md and docs/04, the label entry in GOTCHAS; CHANGELOG lines at each release
- **Notes:** 2026-11-07 sweep: mostly compliant already; workspace tests passed (1428 passed, 0 failed). The `line_edit` box stays open for the Editor's find / replace bar; the `centred_row` box was not re-audited. Gate re-run: deny + release self-test pending. App: screen check pending.

### CR-0062 · Devtools: every tool's form to the layout standard
- **State:** Completed · widened by the owner 2026-11-07; built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-06 ("In Devtools Activity Form in all tools, please check proper form, input, label, wording comply to a standard … many form in the tools is lousy and not up to standard")
- **Scope:** every tool in `tool-devtools` (convert, cron, diff, envfile, generate, hash, jwt, net, ports, regex, snippets, url, base64, firebase, yaml …).
- **Parent:** CR-0025
- **Depends on:** CR-0061
- [x] Each tool's inputs in a `form::field_grid` (or an `edit_table` for rows), labels in Title Case beside their fields, hints in sentence case on their own row, outputs in a `form::well` — shared `util::line` / `secret_line` / `button_row` / `button_if`; Hashes options in a grid, Cron's When grid, Unit Conversion and Network Check grids
- [x] Wording reviewed per tool: one name per thing, no jargon where a plain word exists, the casing rules — about 30 strings moved to Title Case
- [x] Every button standard size with padding, a fitting icon and a label; BUTTON_GAP between neighbours (owner, 2026-11-07) — every button through `form::button`; `varpeek::save_to_env_button` is a standard `form::button` too
- [x] Form rows spaced with ROW_GAP; label-and-field rows on one middle line (owner, 2026-11-07) — the Result box has `ROW_GAP` under it
- [x] Tables to the table standard - accent header band, indented titles, centred cells, empty hints: Hashes, Cron Expression, Unit Conversion (every unit), and any other Devtools table (owner, 2026-11-07) — Hashes results and Cron as `edit_table`s; Every Unit, Network Check as `DataTable`s; Regex, Snippets, .env, JWT claims and URL parts as `edit_table`s or `DataTable`s with empty hints; `egui_extras` dropped from tool-devtools
- [x] Jasypt: the inline password field and Save to Environment button on one middle line, with gaps between the parts (owner, 2026-11-07) — `util::secret_line`: password, peek and Save to Environment on one middle line
- [x] Test: the bundled locale parses; no bare text field left in the crate — `no_bare_text_field_button_or_table_is_left_in_the_crate`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny
- [x] App: `--data-dir ./tmp-data` — every Devtools tool opened and checked against the standard — all 19 seen on screen 2026-11-07 (screenshots 64-71, 85-104); two defects found and fixed under CR-0088
- [x] Docs: CHANGELOG · README · docs/tools/devtools.md
- **Notes:** Listening Ports' Kill is drawn disabled until a row is selected and asks through `dialog::confirm`. The App box stays open until every utility has been looked at, not only the four above.

### CR-0063 · Every button an icon and a label, every button row wrapping
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-06 ("some of the button still dont have either label or icon. And if the row overflow, make the button wrap to next row")
- **Scope:** about 300 label-only buttons across every activity and Settings (`ui.button(s.t(..))` with no glyph), and the button rows built with `ui.horizontal`; the rule is in `.claude/rules/ui.md` → Buttons and toolbars.
- **Parent:** CR-0025
- **Depends on:** CR-0061
- [x] Each label-only button gets a fitting Phosphor glyph (a row's small actions keep icon + tooltip) — every activity and Settings, plus the glyphs `hs_ui::dialog`, `row_menu`, `varpeek`, `ai_assistant` and `datefield` draw; `dialog::confirm` adds the glyphs itself, so callers pass plain labels
- [x] Each row of more than two buttons, or of buttons beside a field, is `horizontal_wrapped` — the rows the sweep touched
- [x] Test: a grep finds no `ui.button(s.t(` without a glyph in the workspace — `no_button_is_a_bare_label` (tool-graphics, tool-editor), `no_label_only_button_or_bare_field_in_the_{api,browser,git,logs,tracker}_ui`, `no_bare_text_field_button_or_table_is_left_in_the_crate` (tool-devtools), `plain_confirm_labels_get_the_dialog_glyph`, `confirm_labels_never_carry_two_glyphs` (hs-ui dialog), `every_compose_verb_has_the_matching_glyph` (tool-containers), `every_format_has_a_menu_glyph` (tool-db export)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — every activity at a narrow window: buttons wrap, none without a glyph — glyphed buttons across the screens checked; API sidebar and Queue publish buttons fixed in the check (screenshots 142, 144)
- [x] Docs: CHANGELOG · GOTCHAS
- **Notes:** workspace tests passed (1428 passed, 0 failed), 2026-11-07. Gate: deny + release self-test pending. App: screen check pending.

### CR-0064 · Home dashboard: the activity rail shrinks to fit one row, and wraps when it cannot
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-06 ("Make the card, icon and label smaller to accommodate the rail in one row (if possible), if not, make it wrap to next row")
- **Scope:** `crates/hyperstack/src/tools/home.rs` (the dashboard toolbar's activity buttons).
- **Parent:** CR-0025
- **Depends on:** CR-0038
- [x] The buttons shrink with the window (labelled 76 → 58 px, glyph 22 → 18, name 11 → 10; unlabelled 34 → 30) and the row wraps below that
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — names on and off, wide and narrow windows - one row with every name whole at the default size (screenshot 18, 2026-11-06)
- [x] Docs: CHANGELOG — CHANGELOG line at merge (6b15a86); ui.md rule present

### CR-0065 · Coloured icons on every dashboard's saved cards, and inner tabs that follow Material icons
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-06 ("Some of the dashboard page in activity dont have color icon on the Saved things. And the tab icon also not follow the color icon of the material icon (when enable)")
- **Scope:** dashboard `Entry` in `tool-devtools` (a colour per category) and `tool-logs` (the sidebar's icon and blue, CR-0058); the inner tab strips of `tool-db` (query and design tabs) and `tool-git` (host repository tabs) draw the Material picture when Material icons are on for that activity.
- **Parent:** CR-0025
- **Depends on:** CR-0049, CR-0058
- [x] Devtools and Logs dashboard cards carry a colour; the other activities already did
- [x] Database query/design tabs and Git host tabs draw the Material icon when it is on
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — Material icons on and off, Devtools and Logs dashboards - Devtools cards by category, Logs cards, Database and Git inner tabs seen 2026-11-06 (screenshots 06-07, 12, 33); the Devtools sidebar colour follows in CR-0070
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (6869df8); ui.md rule present

### CR-0066 · Quick Open and Commands: icons in their kind's colour and Material picture
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-06 ("make the icon follow the kind/type colored in Quick Open dropdown list and Command drop down list (where applicable if enable material icon and color activity icon)")
- **Scope:** `hs_ui::palette::PaletteItem` gains `image` and `colour`; `hyperstack::app::palette_items` fills them: activities and tool commands in the rail colour (Coloured Activity Icons), connections in their product mark, Quick Open files in their Material icon (Material icons on for that activity).
- **Parent:** CR-0025
- **Depends on:** CR-0043, CR-0065
- [x] Palette rows draw an image or a coloured glyph
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — Ctrl+P and Ctrl+Shift+P with Material and coloured icons on and off - command palette with a glyph on every row, Quick Open in activity colours (screenshot 08, 2026-11-06)
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (6869df8); ui.md rule present


### BUG-0047 · Database: a new or filled query tab stays behind the Design tab
- **State:** In-Progress · fixed 2026-11-06; checked on screen again 2026-11-18 (Open Data and New SELECT from a table's menu with the designer in front both bring the query tab forward); only the Test box stays open, since `Services` has no test constructor
- **Source:** owner, 2026-11-06 ("When right click table -> Open data, why it not focus on the new tab to open data?"; "When i click button New query, why it not focus to the new query tab?")
- **Scope:** `tool-db` `run_in_new_tab` and the New SELECT template clear `active_designer`, so the query tab they fill comes to the front.
- **Parent:** FEAT-0103
- [x] New Query, Open Data, Count Rows, New SELECT, a saved query, history and templates bring their query tab to the front
- [ ] Test: not reachable headless (the tool needs `Services`, which has no test constructor); checked in the app instead
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — with a Design tab in front, Open Data shows the query tab and its rows - Open Data and New Query with a Design tab in front bring the query tab forward (screenshots 02-03, 2026-11-06)
- [x] Docs: CHANGELOG — CHANGELOG line at merge (6b15a86); ui.md rule present


### CR-0067 · Clear History and Delete saved items from a right-click, in every activity
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-06 ("In Database Query History, make user can right click on the History tree and clear history. Apply this also to all activities that use history, recent, etc"; "Saved query also can be deleted by right click on the list and click delete query")
- **Scope:** the Database query history first; then every History / Recent list across the activities (found by a sweep). Right-click on the heading or a row: Clear History (confirmation) and Remove for one entry; the stored history is cleared, not only the view.
- **Parent:** CR-0025
- [x] Database query history: right-click Clear History and Remove
- [x] Every other history / recent list: the same menu (built so far: API history, Graphics Recent, Home Recent; the rest of the sweep follows) — 2026-11-07: the notification list gained Remove and Clear All (confirmed); the sweep found no other unguarded list
- [x] Database saved queries: right-click Delete Query (confirmation), the file removed
- [x] Every other activity's saved items the same (built so far: SSH kept forwards, Devtools snippets, Editor project cards, Git repo cards) — 2026-11-07: Containers saved command / config Delete, Logs filter preset Delete and Tracker saved query Delete now confirm; the sweep found no other unguarded list
- [x] Test: clearing empties the store and a reload stays empty — `history_entries_are_removed_and_cleared_from_the_file` (tool-api), `remove_drops_one_entry_and_clear_empties_one_connection` (tool-db), `remove_and_clear_change_the_stored_recents` (tool-graphics), `recent_closed_tabs_can_be_removed_one_at_a_time_or_cleared` (hyperstack), `a_deleted_preset_is_gone_from_the_store_after_a_reload` (tool-logs)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — each list cleared on screen, screenshots - every menu seen 2026-11-06: Database, API, Graphics, Home, Devtools snippets, SSH kept forwards, Editor and Git cards (screenshots 04-07, 19-24, 28); glyph consistency follows in CR-0070
- [x] Docs: CHANGELOG, ui.md rule, tool specs — CHANGELOG lines in 0.128.0 and Unreleased; ui.md rule; api-client, ssh-sftp, git specs
- **Notes:** the 2026-11-07 additions (notifications, Containers, Logs, Tracker) are not yet seen on screen. Gate: deny + release self-test pending. App: screen check pending.


### CR-0068 · Material icons on every list row, not only tabs and cards
- **State:** Completed · agreed 2026-11-06; checked on screen, on and off, 2026-11-18/19
- **Source:** owner, 2026-11-06 ("In Saved query list, use also colored material icon on the list (if enable the material icon in settings) same with the tab color icon of it. Check other activities list that still use generic plain icon instead material icon")
- **Scope:** Database Saved Queries and Query History rows wear the query tab's Material picture; every other activity's list rows of an item kind found by a sweep get theirs from `hs_ui::fileicons` when Material icons are on.
- **Parent:** CR-0065
- [x] Database saved queries and history rows
- [x] Every other list found by the sweep (built so far: Git, Editor, Containers, NoSQL, Redis, S3, Scratchpad) — 2026-11-07 sweep: the remaining lists were already compliant
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — Material on and off, screenshots — 2026-11-19, Database off: the saved query, the History rows, the schema and the query tabs all fall back to the plain glyphs together, lists and tabs still alike; back on, the pictures return
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line in 0.128.0; ui.md Icons rule
- **Notes:** the 2026-11-07 sweep changed no code for this item. 2026-11-18, with Material on: the Database Saved Queries and History rows wear the query tab's picture, as do the Git changed files, the Containers resources, the Redis key folders and the SSH file list; off, the Browser lists (CR-0060) and the Database lists (2026-11-19) were seen.


### CR-0069 · Tabbed panes keep a minimum width; side panes can be hidden
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-06 ("In Editor Activity -> Work Pane -> Project directory pane -> Project and Source control tab. Please restrict the pane to resize minimum the size where user can see icon and label on those tabs … I found this also in Browser Activity … check other activities also that use the same multi tab. And follow Editor Activity that can hide the pane")
- **Scope:** the Editor's segmented tabs move to `hs_ui::tabs::segmented` with `segmented_min_width`; the Editor column, the Browser sidebar and every other pane or sidebar holding a tab row use it as their minimum; icons are clipped to their tab. Side panes in work panes get a hide toggle where it fits.
- **Parent:** CR-0025
- [x] Shared segmented tabs with a measured minimum width; nothing drawn outside its tab
- [x] Editor column and Browser sidebar respect it
- [x] Every other tabbed pane found by a sweep respects it; side panes get a hide toggle where suitable — 2026-11-07: the other tabbed panes were already compliant; the S3 bucket column, Redis key list and NoSQL database column gained toggles and resize (CR-0091)
- [x] Test: the minimum fits every label and icon; a segment never paints outside its rect — `the_segmented_minimum_fits_every_label_and_nothing_paints_outside` (hs-ui tabs)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — drag each pane to its minimum, screenshots - Editor and Browser minimums and the Database, Git, diagram and generator toggles seen 2026-11-06 (screenshots 13-16, 25-27); Tracker Browse/Search not seen - it needs a Confluence server this machine does not have — S3, Redis and NoSQL columns resize and hide (screenshots 117, 119, 122)
- [x] Docs: CHANGELOG, ui.md rule, docs/04 — CHANGELOG lines in 0.128.0 and Unreleased (CR-0091); docs/04 2026-11-07 addendum
- **Notes:** Gate: deny + release self-test pending. App: screen check pending.


### CR-0070 · Fixes from the 2026-11-06 screen check of CR-0061…CR-0069
- **State:** Completed · found on screen on v0.128.0, 2026-11-06; each fix seen on screen 2026-11-18
- **Source:** the owner asked for the remaining screens to be checked ("Yes, check them"); these failed the checklist in `.claude/rules/ui.md`.
- **Scope:** selected table rows readable; tab titles never under the close button; Git changed files and the Containers resource list in Material pictures; Redis key tree caret before icon; glyphs on every CR-0067 menu item and Clear disabled on an empty list; the API history's empty state; Devtools sidebar rows and tabs in their category colour; the Tracker query row's buttons at field height; the SSH Kept Forwards list scrolls and its Start button has a glyph.
- **Parent:** CR-0061
- [x] The ten defects fixed
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — each defect's screen again, screenshots — 2026-11-18: a selected run row keeps the normal text colour; tab titles elide before the close button; Git changed files and the Containers resources in Material pictures; the Redis caret before the folder picture; every table menu item glyphed and Clear History disabled on the empty API history, whose empty state says so; Devtools rows, cards and tabs in their tool's colours; the Tracker query, Search and Queries at one height; SSH's Start buttons glyphed (two kept forwards, too few to make the list scroll)
- [x] Docs: CHANGELOG, ui.md rules — the rules are in ui.md; CHANGELOG lines at the 0.128.x releases


### CR-0071 · SSH: tunnel and host marks in colour, a structured monitor, tidy tunnel rows
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-06 (the owner's list of 2026-11-06)
- **Scope:** the Tunnels list and the shell's Tunnels view use a coloured tunnel picture; each host group header wears the host's OS mark in colour; the Monitor view is a table / grid of labelled figures; Kept Forwards has a gap above it and its rows are middle-aligned.
- **Parent:** CR-0061
- [x] Tunnel picture in colour in the sidebar and the Tunnels view
- [x] Host group headers wear the host's coloured mark
- [x] Monitor laid out as a table of figures
- [x] Kept Forwards spacing and middle-aligned rows; the same layout checked in other activities
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-06 before v0.129.0 (merge 3426379)
- [x] App: `--data-dir ./tmp-data` — screenshots of each screen — screenshots 37, 38, 41, 50 (2026-11-06)
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (112e85e); ui.md rule present


### CR-0072 · Fields fillable from a known list become dropdowns (designer FK columns first)
- **State:** Completed · agreed 2026-11-06; seen on screen 2026-11-18
- **Source:** owner, 2026-11-06 (the owner's list of 2026-11-06)
- **Scope:** the table designer's Foreign Keys Columns and Referenced Columns pick from the table's / referenced table's columns; a sweep turns every text field that a known list can fill into `pick_field`.
- **Parent:** CR-0061
- [x] Designer FK columns and referenced columns are pick fields
- [x] Sweep of other activities applied
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-06 before v0.129.0 (merge 3426379)
- [x] App: `--data-dir ./tmp-data` — screenshots of each screen — 2026-11-18: the designer's Foreign Keys Columns picker lists the table's columns (id, name, city, balance, opened), References has its caret, Referenced Columns is plain text until a table is picked; the RabbitMQ publish form's Exchange and Routing Key are pickers
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (112e85e); ui.md rule present


### CR-0073 · Main toolbar buttons larger than sub-toolbar buttons; show/hide toggles first
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-06 (the owner's list of 2026-11-06)
- **Scope:** `hs_ui::toolbar` gets a main and a sub button height (main slightly bigger than today, sub with vertical air); every pane show/hide toggle moves to the far left of the activity's first toolbar row.
- **Parent:** CR-0061
- [x] Main and sub toolbar sizes
- [x] Toggles at the far left in every activity
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-06 before v0.129.0 (merge 3426379)
- [x] App: `--data-dir ./tmp-data` — screenshots of each screen — screenshots 25-27, 37, 43, 51
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (112e85e); ui.md rule present


### CR-0074 · Item-kind columns: larger icons and labels in colour
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-06 (the owner's list of 2026-11-06)
- **Scope:** the Containers item column (Containers, Projects, Images, Volumes, Networks, Swarm …) and every column like it use 20 px coloured pictures and body text.
- **Parent:** CR-0061
- [x] Containers item column
- [x] Other activities with the same column
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-06 before v0.129.0 (merge 3426379)
- [x] App: `--data-dir ./tmp-data` — screenshots of each screen — screenshots 29, 35
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (112e85e); ui.md rule present


### CR-0075 · Detail views: a label beside its action button on one middle line
- **State:** Completed · agreed 2026-11-06; seen on screen 2026-11-18
- **Source:** owner, 2026-11-06 (the owner's list of 2026-11-06)
- **Scope:** a container's Ports beside Open in Browser, and every detail view row of a value with its action, open with `centred_row`.
- **Parent:** CR-0061
- [x] Containers detail views
- [x] Every other activity's detail views
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-06 before v0.129.0 (merge 3426379)
- [x] App: `--data-dir ./tmp-data` — screenshots of each screen — 2026-11-18: hs-redmine's Ports label, its two port buttons and Open in Browser on one middle line; Settings → Git → Repos' Project label beside Fetch Project likewise
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (112e85e); ui.md rule present


### CR-0076 · Devtools and Graphics tools wear their own multi-colour pictures
- **State:** Completed · agreed 2026-11-06; the header audit and screen check 2026-11-18
- **Source:** owner, 2026-11-06 ("In Devtools Activity -> Sidebar Column -> Tools Icon; If possible use multi color icon that represent the tool itself … In Graphics Activity -> Sidebar Column -> Tools Icon … Please use suitable multi color icon and it will reflect in the dashboard page and tab icon")
- **Scope:** a coloured picture per Devtools utility and per Graphics tool (Image, Icon, Diagram Editor, Diagram Generator), used by the sidebar row, the dashboard card and the tab.
- **Parent:** CR-0065
- [x] Devtools: one picture per utility, in the sidebar, the dashboard and the tab
- [x] Graphics: one picture per tool, in the sidebar, the dashboard and the tab
- [x] Every activity's work-pane header wears the same picture and colour as its tab (Graphics first; owner, 2026-11-06) — 2026-11-18 audit: every tool builds its header and tab from the same `ConnIcon`, so they match (Graphics' diagram and icon tabs, Automation, Containers, Database, Redis, SSH, Terminal, Tracker and Queue seen on screen); Devtools and API Client's Manage and Library pages carry the mark in their page title instead of a band, and the Library title was the one without it - it now wears the tab's package in its colour
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-06 before v0.129.0 (merge 3426379)
- [x] App: `--data-dir ./tmp-data` — screenshots — screenshots 39, 51 (Devtools and Graphics)
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (112e85e); ui.md rule present


### CR-0077 · Log-level colours follow the Logs activity everywhere
- **State:** Completed · agreed 2026-11-06; seen on screen 2026-11-18
- **Source:** owner, 2026-11-06 ("In other activities that have logs, please follow Logs Activity Log color for Info, wan, debug, error, other alias, etc) the text color for those")
- **Scope:** the Logs activity's level → colour mapping (with its aliases) moves to one shared `hs_ui` function; every other view that shows log lines uses it.
- **Parent:** CR-0025
- [x] Shared level-colour function with the aliases, unit-tested
- [x] Every other log view uses it
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-06 before v0.129.0 (merge 3426379)
- [x] App: `--data-dir ./tmp-data` — Logs, an Automation run log, a container log; screenshots — 2026-11-18: hs-redmine's container log in the Logs activity colours only the INFO word, the rest of the line in the normal text colour; the bench TeamCity run log has no level words and shows none coloured
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (112e85e); ui.md rule present


### CR-0078 · Options sharing a row are spaced apart
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-06 ("In Git Activity -> Repository -> Work Pane -> Check box below Commit message text area; Please put some left/right margin/padding between Amend and Push after commit checkbox. Check if this characteristic in one row in other activities and apply it too. ANd write in the rule")
- **Scope:** `hs_ui::form::OPTION_GAP`; the Git commit options and Merge options, the Editor merge row, the Database DDL options, Devtools Base64, the password and passphrase generators and the regex flags.
- **Parent:** CR-0061
- [x] `OPTION_GAP` between options on one row, in every row found (16 rows in 7 files)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-06 before v0.129.0 (merge 3426379)
- [x] App: `--data-dir ./tmp-data` — the Git commit area; screenshots — screenshot 43 (Git commit options)
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (112e85e); ui.md rule present


### CR-0079 · Saved shells and other products wear their real multi-colour marks everywhere
- **State:** Completed · agreed 2026-11-06; seen on screen 2026-11-18
- **Source:** owner, 2026-11-06 ("In Terminal Activity -> CLI Icon; Use actual multi color icon for cli that saved in the list. and it will reflect in all location that used it (Tab Icon, Dashboard Page, recent, history, Settings -> Terminal, etc). POlease check other activities that still not have the proper icon and multi color to apply it. Write it in the rule.")
- **Scope:** the Terminal's saved shells resolve to real product logos (bundled SVG/PNG under the product-logo set), used by the sidebar list, the tab, the Terminal dashboard, Home's Recent, history and Settings → Terminal; an audit finds other activities' saved items still on a plain or wrong icon and fixes them.
- **Parent:** CR-0036
- [x] Terminal shell logos in every place a shell is drawn
- [x] Other activities' saved items found by the audit
- [x] Test: every built-in shell kind resolves to a logo or a coloured glyph — `tool-terminal` `every_shell_wears_a_logo_or_a_coloured_glyph`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — passed 2026-11-06 before v0.129.0 (merge 3426379)
- [x] App: `--data-dir ./tmp-data` — Terminal sidebar, tab, dashboard, Settings → Terminal; screenshots — 2026-11-18: PowerShell, Windows PowerShell, Command Prompt, Nushell, Git Bash and Ubuntu wear their marks in the sidebar, the Saved Shells cards, Open Here, the tabs, the header and Settings → Terminal's list
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (112e85e); ui.md rule present


### CR-0080 · Settings forms: left-aligned labels, indented sub-settings, one control column
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-06 ("In All Settings Form; Put the all the label, horizontal left align and sub setting indented left align too. The checkbox, radio button, text input, dropdown input put in the same column position of other horizontal left align too. Write tjhis in rules")
- **Scope:** `hs_ui::form::field_grid` / `label` left-align labels and gain `sub_label` with `SUB_INDENT`; every Settings page (crates/hyperstack/src/settings_ui.rs, settings_ai.rs) and the connection forms built like them use it, with every control in the second column.
- **Parent:** CR-0061
- [x] `field_grid` labels left-aligned; `sub_label` indents a sub-setting
- [x] Every Settings page converted: labels left, sub-settings indented, controls in one column
- [x] Every Settings row, in the grid or outside it, has `ROW_GAP` above and below (owner: "some of the row in settings still not having top/bottom margin/padding")
- [x] Descriptions, guides, paragraphs, hints and standalone labels in Settings have the same gap above and below as the rows around them (owner, 2026-11-06)
- [x] Every Settings button one size (field height, same padding) through `form::button` / `form::primary_button`, with `BUTTON_GAP` between buttons side by side (owner, 2026-11-06)
- [x] Sub-section headings inside Settings cards a step larger and strong (`form::subsection`), between the card title and row labels (owner, 2026-11-06)
- [x] Checkbox lists spaced with descriptions under their labels; reorder (rearrange order) buttons standard size, their rows spaced (owner, 2026-11-06)
- [x] Suggestion lists: rows middle-aligned, Add buttons standard size in an aligned column, rows spaced (owner, 2026-11-06)
- [x] Settings → About → Installed: the Data Folder value and Open Data Folder on one middle-aligned row with a gap (owner, 2026-11-06)
- [x] Settings → About: the Policy and Check for Updates cards merged into one card titled Updates - the policy settings, then Check for Updates and its status (owner, 2026-11-06)
- [x] Test: a label and a sub-label start left; their controls share one x
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — several Settings pages; screenshots - General, About, Terminal seen 2026-11-07 (screenshots 47-49)
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (112e85e); ui.md rule present


### CR-0081 · Dropdown items colour their icons by kind
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-07 ("In All Dropdown menu list that show icon and label, color those icon follow from its kind/type/etc")
- **Scope:** every ComboBox, `pick_field` popup and menu whose items show an icon (workspace, environment, shell, connection, engine, kind, file pickers …) draws each icon in its kind's mark, and the closed box shows the selected item's mark.
- **Parent:** CR-0065
- [x] Every dropdown with icons colours them by kind
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — several dropdowns open; screenshots — screenshots 56, 57, 59
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (a59b8d5); rules and specs updated


### FEAT-0105 · More colour schemes for dark and light mode
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-07 ("can you add more color scheme palete, so that user have many option for choosing best theme/scheme color both dark mode and light mode")
- **Scope:** `hs_ui::theme` gains 18 bundled schemes from their published palettes - dark: Tokyo Night, Rosé Pine, Kanagawa, Everforest Dark, Ayu Dark, Night Owl, Material Palenight, GitHub Dark, Catppuccin Macchiato; light: One Light, Gruvbox Light, Catppuccin Latte, Rosé Pine Dawn, Ayu Light, Everforest Light, Tokyo Night Day, Tomorrow, Light Owl - listed in Settings → Appearance with the others.
- **Spec:** docs/04-ui-design.md (theming)
- [x] 18 schemes added to `Theme::schemes()` / `by_id`
- [x] Test: every scheme's text and muted text keep readable contrast on its window, panel and surface; every id resolves
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — Settings → Appearance palette list, a few schemes applied; screenshots — screenshots 60, 61
- [x] Docs: CHANGELOG, docs/04 theming list, README if it lists schemes — CHANGELOG line at merge (a59b8d5); rules and specs updated


### FEAT-0106 · Environment tiers: an icon and colour per tier, Production and Production Support protected
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-07 (asked for an icon per environment - LOCAL, LOCALHOST, LOWER, STAGING, TESTING, UAT, TRAINING, SIT, PROD, PRODUCTION - then "how production support will fit in?", "if user set the tier manually, the icon should change to the tier level also?", "Make production support also protected and proceed to implement this")
- **Spec:** docs/06-security.md (protected environments), docs/04-ui-design.md (environment marks)
- **Scope:** `hs_core::environments` gains a `Tier` (Local, Development, Integration, Testing, Training, Acceptance, Staging, ProductionSupport, Production; an unmatched name has no tier and shows the globe) matched from the name by keyword (Production Support checked before Production) and an optional manual tier that overrides it; `hs_ui` gives each tier an icon and colour; protected always shows the red shield. A new environment, or one whose tier is set or changed to Production or Production Support, defaults to protected (read-only); the user can still change it. The mark shows in the title-bar Environment dropdown, the environment banner and Settings → Environments (with a Tier dropdown); tabs are no longer tied to an environment, so they carry no mark.
- **Parent:** CR-0025
- [x] `Tier`, keyword matching (case and separators ignored; PROD-SUPPORT before PROD), manual override saved with the environment
- [x] Tier icons and colours; protected → red shield; unknown → neutral globe
- [x] Production and Production Support default to protected on create and on tier change
- [x] Marks in the title-bar dropdown, the banner and Settings → Environments (Tier dropdown)
- [x] Test: name matching for every listed name, precedence (protected > manual > matched > none), the protected default, the setting survives save and reload
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — environments of several tiers, one set by hand, the dropdown and banner; screenshots — screenshots 57, 58, 59
- [x] Docs: CHANGELOG, docs/06, docs/04, ui.md rule — CHANGELOG line at merge (a59b8d5); rules and specs updated


### CR-0082 · A workspace without a logo shows a lettered tile
- **State:** Completed · every box checked 2026-11-07
- **Source:** owner, 2026-11-07 (asked for a default workspace icon; chose "the lettered tile")
- **Scope:** a shared `hs_ui` lettered tile - one or two initials on a rounded square, its colour picked from the name out of the rotating palette of eight (the Logs source palette, moved to hs_ui) - drawn wherever a workspace has no logo: the title-bar dropdown and selected value, Home's Workspaces cards, Settings → Workspace, the startup and copy-from pickers, the rail workspace picker. A set logo still wins.
- **Parent:** CR-0079
- [x] `hs_ui` lettered tile with stable initials and colour
- [x] Used everywhere a workspace mark is drawn
- [x] Test: initials ("default" → D, "hyper-stack" → HS, "My Work Space" → MW), the colour is stable for a name
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — two workspaces without logos; screenshots — screenshot 56
- [x] Docs: CHANGELOG, ui.md rule — CHANGELOG line at merge (a59b8d5); rules and specs updated


### CR-0083 · README features, screenshots and GIF animations refreshed for v0.7.7
- **State:** Completed · agreed 2026-11-07, screens retaken 2026-11-07
- **Source:** owner, 2026-11-07 ("DOn't forget to update README.md with latest screenshot and GIF animations. Please write this in rules too")
- **Scope:** retake every README screenshot whose screen changed in v0.7.7 (layout standard, tables, Settings forms, colour icons and logos, environment tiers, colour schemes, workspace tiles, SSH, Devtools, Database designer …) at the current 1720×860 minimum window, add screenshots for new screens, and rebuild the walkthrough GIF (`hyperstack-demo.gif`) and the per-activity GIFs whose flow changed, per `docs/screenshots/README.md`; update that file's "last refreshed" line.
- **Parent:** CR-0025
- [x] README feature text describes every feature v0.7.7 added (colour schemes, environment tiers, lettered workspace tiles, product logos, coloured dropdown marks, Settings forms, SSH marks and monitor, …)
- [x] Changed screens retaken; new screens added to the README - all 172 README images retaken at 1736x900 from the v0.131.0 release build; six new images (`s3-preview`, `redis-slowlog`, `nosql-document-json`, `queue-publish-picks`, `api-variable-tooltip`, `api-open-here`); `split-scratchpad.png` and its row removed (split panes left the app on 2026-10-23); captions that no longer matched the screens reworded
- [x] Walkthrough and per-activity GIFs rebuilt - `hyperstack-demo.gif` (109 frames, 8.8 MB), `git-walkthrough.gif` (10 frames, 0.87 MB), `database-walkthrough.gif` (7 frames, 0.48 MB), 1200x622 frames
- [x] `docs/screenshots/README.md` refresh date and recipes updated - top paragraph and the 2026-11-07 set section
- [x] Docs: README, CHANGELOG
- **Notes:** three images were kept from the old set: `editor-debug.png` and `editor-debug-stack.png` (the debug panel covers the editor, so the paused line cannot be shown - BUG-0051), and `automation-jenkins-log.png` (no Jenkins profile in the scratch data).


### CR-0084 · List and tree rows on one middle line; air above status lines
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 (reported the Redis key tree's picture sitting high beside the key name, then asked for the same row alignment everywhere; no ledger item covered the Redis fix, so it is filed here)
- **Scope:** a list or tree row sets its height to its tallest item before it opens, so caret, picture, name and badge share one middle line - the Redis key tree, NoSQL collections, API Client collection and folder headers, Database saved-query and history rows, Scratchpad folder headers, Source Control folder headers, the Containers host title; a status line (a count, Scanning…, Find Everywhere's summary, SSH's loading line, the Redis key count) has `form::ROW_GAP` above it - Redis, Browser, Logs, SSH. Rules written in `.claude/rules/ui.md` ("A list or tree row is one middle line", "A status line is a row too").
- **Parent:** CR-0061
- [x] Redis key tree rows on one middle line; the key count has `ROW_GAP` above it
- [x] The same row fix in NoSQL, API Client, Database, Scratchpad, Source Control and Containers
- [x] `ROW_GAP` above the status lines in Browser, Logs (Scanning…, Find Everywhere) and SSH (loading)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny
- [x] App: `--data-dir ./tmp-data` — the swept rows and status lines looked at on screen 2026-11-07, screenshots
- [x] Docs: CHANGELOG · ui.md rules · docs/04
- **Notes:** layout only, so no unit test; the check is the screen.


### CR-0085 · Fixes from the 2026-11-07 screen check: header band in its card, group grips, empty DataTables
- **State:** Completed · found and fixed in the 2026-11-07 screen check
- **Source:** the 2026-11-07 screen check against `.claude/rules/ui.md` → *Checking a screen*, while verifying CR-0062 and CR-0084
- **Scope:** `table::begin_header_band` bounds the band by `ui.max_rect()` as well as the clip rect, so it ends at its card (the Devtools Hashes and Cron tables ran to the window edge); `reorder::Reorder::group_row` centres a group's drag grip on its header line (API Client collections and folders, Scratchpad folders), not halfway down its open children; `DataTable::show` with no columns draws nothing (NoSQL's empty results showed a bare band). Rules in `.claude/rules/ui.md` ("A group's grip is on its own line", "A table stays inside its card"); three GOTCHAS entries.
- **Parent:** CR-0061
- [x] Header band bounded by its card
- [x] `Reorder::group_row` for API Client collections and folders and Scratchpad folders
- [x] A `DataTable` with no columns draws nothing
- [x] Test: `header_band_stays_inside_its_area`, `a_group_rows_grip_sits_on_its_header`, `a_table_with_no_columns_draws_no_band`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny
- [x] App: `--data-dir ./tmp-data` — Devtools Hashes and Cron tables, API Client and Scratchpad grips, NoSQL empty results seen on screen 2026-11-07, screenshots
- [x] Docs: CHANGELOG · ui.md rules · docs/04 · GOTCHAS


### CR-0086 · Devtools tabs show the whole tool name
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("In Devtools Activity -> Tab Strip; Display Full Tool name label inside tab with the icon"); asked whether other activities should follow, the owner chose "Only Devtools, because the label name is fixed. Other will remain where is it currently."
- **Spec:** docs/04-ui-design.md §2.2, docs/tools/devtools.md
- **Scope:** `Tool::tab_title_whole()` (default false; Devtools returns true) → `TabItem::whole_title()` → the strip raises its minimum tab width to the longest whole title among those tabs, above the usual cap, and wraps rows rather than eliding. Other activities keep the shared width and the middle ellipsis. Rule in `.claude/rules/ui.md` ("Devtools tabs show the whole tool name").
- **Parent:** CR-0025
- [x] `Tool::tab_title_whole` and `TabItem::whole_title`; Devtools opts in
- [x] The strip's minimum width follows the longest whole title; rows wrap
- [x] Test: `a_whole_title_minimum_above_the_cap_is_honoured`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny (2026-11-07)
- [x] App: `--data-dir ./tmp-data` — Devtools utilities open in the strip, each title whole beside its icon; screenshots 2026-11-07
- [x] Docs: CHANGELOG · README · docs/04 · docs/tools/devtools.md · ui.md rule
- **Notes:** full test run pending at the time of writing; tick Test and Gate when it passes.


### CR-0087 · Open Here cards wear their tab's coloured icon
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("In All Activity Which have Open Here Section -> Dashboard Page -> Open Here Section; Add respective colored icon to each list of Open here and vertical middle align it with the label title and subtitle")
- **Spec:** docs/04-ui-design.md
- **Scope:** `hs_core::open_tabs::TabInfo` gains `icon`, `icon_uri` and `icon_colour`, filled by the shell from the same tab views the strip draws; the shared `hs_ui::dashboard` Open Here card draws a 20 px mark on the middle line of its title and subtitle. Covers all 17 activities with an Open Here section. Rule in `.claude/rules/ui.md` ("An Open Here card wears its tab's mark").
- **Parent:** CR-0065
- [x] `TabInfo` carries the tab's icon, picture and colour
- [x] The dashboard's Open Here card draws the mark on the middle line of title and subtitle, in every activity
- [x] Test: `an_open_here_card_wears_its_tabs_mark_between_its_two_lines`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny (2026-11-07)
- [x] App: `--data-dir ./tmp-data` — Open Here on activity dashboards with tabs open, each card's coloured icon matching its tab and on the middle line of title and subtitle; screenshots 2026-11-07
- [x] Docs: CHANGELOG · README · docs/04 · ui.md rule
- **Notes:** full test run pending at the time of writing. No unit test covers the card's layout; the check was the screen.


### CR-0088 · Work-pane page titles wear their tab's mark and a description
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("In Devtools Activity -> Tools Work Pane -> Header; Put also colored icon beside the header title of each Tools." · "In API Client -> Manage Collection Work Pane -> Header; Put the same colored icon used in tab for manage collection header" · "In Devtools -> All Network Tools and Workspace Tool; Put some description below the header title, like other tools")
- **Scope:** `hs_ui::form::page_title_marked` (a 26 px mark on the title's middle line); every Devtools tool's title goes through `util::page_title`, which finds the tool's picture in `Utility::ALL`; Network Check, Listening Ports, Snippets and .env Editor gain a title and a description (`dt-net-sub`, `dt-ports-sub`, `dt-snippets-sub`, `dt-env-sub`); API Client's Manage Collection title wears the tab's gear in the activity colour. Found in the screen check and fixed: Devtools pages (except Text Diff) are capped at the cards' width so hints wrap there; TOML/XML with no input show a muted hint instead of a red "nothing to check".
- **Parent:** CR-0025
- [x] Code: `form::page_title_marked`, `util::page_title`, four new subtitles, Manage Collection mark, width cap, empty state
- [x] Test: `a_marked_title_shares_the_titles_middle_line`
- [x] Gate: fmt, clippy, workspace tests, deny, self-test (2026-11-07)
- [x] App: all 19 Devtools tools and API Client's Manage Collection looked at on 2026-11-07, release build, scratch data folder
- [x] Rule: ui.md "A page title wears its tab's mark, and says what the page is for"
- [x] Docs: CHANGELOG, docs/04-ui-design.md, docs/tools/devtools.md, docs/tools/api-client.md


### CR-0089 · Open Here request cards show the coloured method, as the tab does
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("In APi Client -> Dashboard Page -> Open Here Section; Please add colored method text in request card, follow the tab")
- **Scope:** `hs_core::open_tabs::TabInfo::prefix` carries the tab's coloured title piece (an API request's method); the shared Open Here card draws it before the title in its colour, the title in the normal text colour - in every activity that gives its tab a prefix.
- **Parent:** CR-0087
- [x] Code: `TabInfo::prefix`, filled from the strip's tab view; the card draws it
- [x] Test: `an_open_here_card_shows_the_tabs_coloured_method`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — API Client dashboard with a GET and a POST request open; screenshot — API Open Here card shows the green GET (screenshot 130)
- [x] Rule: ui.md, the Open Here card rule
- [x] Docs: CHANGELOG, docs/04 addendum — Unreleased line and the 2026-11-07 docs/04 addendum
- **Notes:** Gate: deny + release self-test pending. App: screen check pending.


### BUG-0048 · Git: an Open Here card misses the repository's own icon
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("In Git Activity -> Dashboard Page -> Open Here Section; Why the icon repository not follow from the sidebar column…")
- **Spec:** docs/tools/git-activity.md
- **Scope:** `tool-git` restored repository tabs: the tab's picture is named from the saved entry when the tab is restored (`hs_ui::icons::image_uri_name`), so the Open Here card (CR-0087) wears the repository's uploaded icon as the sidebar row does; looked up once per change to the saved entries (`hs_core::git_repos::icon_generation`), never per frame.
- **Parent:** CR-0087
- [x] Code: picture named at restore, cached on the repository id and `icon_generation`
- [x] Test: `a_restored_tab_names_its_picture_before_it_is_drawn` (tool-git)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — a repository with an uploaded icon, its tab restored after a restart; the Open Here card wears the same icon as the sidebar row — Git Open Here cards wear the repositories' own marks on restored tabs (screenshot 139)
- [x] Docs: CHANGELOG · docs/tools/git-activity.md · GOTCHAS
- **Notes:** workspace tests passed (1428 passed, 0 failed). Gate: deny + release self-test pending. App: screen check pending.


### BUG-0049 · Devtools .env Editor masks non-secrets and misses secrets
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("Some of the secret not masked (for example, SMTP_PASS), but some of the filed is not password, but masked (for example, AUTH_LOCKOUT_THRESHOLD and AUTH_LOCKOUT_DURATION_MINUTES)")
- **Spec:** docs/tools/devtools.md
- **Scope:** `tool-devtools` `envfile::looks_secret` decides by whole words of the key (split on `_`, `-`, `.` and camelCase), so `SMTP_PASS` is masked and `AUTH_LOCKOUT_THRESHOLD` is not; a secret word run together with another (`DBPASSWORD`) still counts.
- [x] Code: whole-word `looks_secret`
- [x] Test: `secret_looking_keys_are_recognised`, `whole_words_decide_what_is_secret` (tool-devtools)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — a .env with `SMTP_PASS`, `AUTH_LOCKOUT_THRESHOLD`, `AUTH_LOCKOUT_DURATION_MINUTES`; only the first is masked — covered by its tests; the .env editor masks by whole words
- [x] Docs: CHANGELOG · docs/tools/devtools.md
- **Notes:** workspace tests passed (1428 passed, 0 failed). Gate: deny + release self-test pending. App: screen check pending.


### BUG-0050 · API Client: the Body tab's kind buttons grow on hover
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("the buttons still magnify when hover (None, JSON, Text, XML, Form, Multipart, Binary File, etc)")
- **Spec:** docs/tools/api-client.md
- **Scope:** `tool-api` `ui.rs`: the body kinds, the API key's Header / Query choice and the folder picker go through `form::selectable_label` instead of egui's `selectable_label`, which grows 1 px a side under the pointer (GOTCHAS, "Selectable rows and labels grow").
- [x] Code: every selectable in `tool-api`'s `ui.rs` through `form::selectable_label`
- [x] Test: `no_label_only_button_or_bare_field_in_the_api_ui` (tool-api) rejects `.selectable_label(` and `.selectable_value(` in the file
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — hover None, JSON, Text, XML, Form, Multipart and Binary File; none grows — Body tab buttons outline on hover, nothing moves (screenshot 135)
- [x] Docs: CHANGELOG · docs/tools/api-client.md
- **Notes:** workspace tests passed (1428 passed, 0 failed). Gate: deny + release self-test pending. App: screen check pending.


### FEAT-0107 · `{{variable}}` tooltips on hover, in every activity
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("when user mouse hover on variables {{}}, it will show tooltip the variable value. Especially in API Clients -> Request Endpoint, Body , Params, Header. Apply to all activities too.")
- **Spec:** docs/04-ui-design.md · docs/tools/api-client.md
- **Scope:** `hs_ui::varpeek` (`reference_at`, `tip_text`, `install`, `reference_tip`, `hover_tip`, `text_edit_tip`): hovering a `{{name}}` shows the reference and its value in `hs_ui::varfield::var_field`, `hs_ui::kv_editor::KvEditor`, `form::text_field` / `code_field`, the editor engine with `EditorView::var_tips` (API Client Body), and the Devtools inputs. A secret shows masked and is never decrypted for the tooltip; a locked vault and an unset name say so.
- [x] Code: shared tooltip installed once by the shell; every shared field and the API Client's URL, Params, Headers and Body use it
- [x] Test: `finds_references_once_each`, `the_hovered_byte_maps_to_its_reference`, `a_secret_never_appears_in_the_tooltip` (hs-ui varpeek)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — hover a plain, a secret and an unset `{{variable}}` in the API Client URL, Params, Headers and Body, and in another activity's field — hovering {{base_url}} in the API endpoint shows its tooltip (screenshot 132)
- [x] Docs: CHANGELOG · README · docs/04 · docs/tools/api-client.md · docs/06 (masked, never decrypted)
- **Notes:** workspace tests passed (1428 passed, 0 failed). Gate: deny + release self-test pending. App: screen check pending.


### FEAT-0108 · S3: a preview column for the clicked object
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("When click on a file, please show the previewable of that file in the right column of it (text, image, markdown, etc)")
- **Spec:** docs/tools/s3.md
- **Scope:** `tool-s3` `preview` (kind by extension: text, Markdown, image; `PREVIEW_LIMIT` 2 MiB), a preview column right of the details; `hs-core::objstore` `get_bytes_capped` (a ranged read that stops at the limit, for S3, WebDAV and Nexus); the fetch is a cancellable job, cached by bucket, key and ETag; a failed preview has Retry; a larger or unsupported object says so instead of fetching.
- **Depends on:** CR-0091
- [x] Code: preview column, capped ranged read, cancel, cache, Retry, empty / loading / error states
- [x] Test: `picks_the_preview_by_extension`, `a_large_object_is_not_fetched`, `a_new_etag_is_a_new_preview`, `a_text_preview_is_split_into_lines_once` (tool-s3 preview), `a_capped_read_stops_at_the_limit` (hs-core objstore)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — against moto: click a .txt, a .md, a .png and a large object; the column shows each, the large one says it is too large — text, Markdown and a not-an-image file previewed; Retry shown (screenshots 112, 116, 145)
- [x] Docs: CHANGELOG · README · docs/tools/s3.md · GOTCHAS
- **Notes:** workspace tests passed (1428 passed, 0 failed). Gate: deny + release self-test pending. App: screen check pending.


### CR-0090 · Queue: fields the broker can list are pick fields
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("Some of the field that should be a drop down menu with populate value, not done. For example Exchange, Routing Key, etc")
- **Spec:** docs/tools/queue.md
- **Scope:** `tool-queue` Publish: RabbitMQ exchange and routing key (from the broker's exchanges and bindings), NATS subject (from the stream's subjects) and Kafka partition (from the topic's partition count) are `form::pick_field`s; with an empty list (broker unreachable) each stays a plain text field; a failed read is asked again after a refresh.
- **Parent:** CR-0038
- [x] Code: `publish_targets` in the driver (RabbitMQ), `kafka_partitions`, `nats_subjects`, `routing_keys`; the publish form's pick fields
- [x] Test: `publish_targets_list_exchanges_and_their_routing_keys` (rabbitmq), `the_publish_pick_lists_come_from_the_broker`, `a_failed_publish_targets_read_is_asked_again_after_a_refresh` (tool-queue ui)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — RabbitMQ, Kafka and NATS bench brokers: each pick list filled from the broker — RabbitMQ Exchange dropdown lists the broker's exchanges (screenshot 129)
- [x] Docs: CHANGELOG · README · docs/tools/queue.md
- **Notes:** workspace tests passed (1428 passed, 0 failed). Gate: deny + release self-test pending. App: screen check pending.


### CR-0091 · Browse columns can be hidden and resized: S3, Redis, NoSQL
- **State:** Completed · every box checked 2026-11-08
- **Source:** owner, 2026-11-07 - three requests: the S3 bucket column hideable and its key, details and preview columns resizable; the Redis key list hideable and resizable; the NoSQL database column hideable and resizable
- **Spec:** docs/04-ui-design.md · docs/tools/s3.md · docs/tools/redis.md · docs/tools/nosql.md
- **Scope:** each browse column is a resizable panel with a show/hide toggle as the first toolbar item (CR-0073) and a palette command: `s3.buckets` (S3: Toggle Bucket Column), `s3.preview` (S3: Toggle Preview Column), `redis.key_column` (Redis: Toggle Key Column), `nosql.db_column` (NoSQL: Toggle Database Column).
- **Parent:** CR-0069
- [x] Code: resizable panels and toggles in tool-s3, tool-redis and tool-nosql; palette commands
- [x] Test: `hyperstack` `columns_tests::the_s3_bucket_column_hides_and_shows` (no server), `the_s3_columns_hide_and_resize` (HS_S3_ENDPOINT), `the_redis_key_list_hides_shows_and_resizes` (HS_REDIS), `the_nosql_columns_hide_resize_and_hold_the_open_document` (HS_MONGO): toggles hide/show, real drags resize and hold; they also found the S3 columns sized to their content (fixed: `fill_column`)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — hide and show each column, drag each edge, at the default and a narrow window — S3 Buckets toggle hides the column, Redis key list resizes by drag (screenshots 117, 119)
- [x] Docs: CHANGELOG · docs/04 · docs/tools/s3.md · docs/tools/redis.md · docs/tools/nosql.md
- **Notes:** workspace tests passed (1428 passed, 0 failed); the Test box stays open because no test asserts the columns. Gate: deny + release self-test pending. App: screen check pending.


### CR-0092 · Redis Slow Log as a table with a detail pane, its Duration coloured by level
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("Slow Log; It should be in table view. and when clicked one of the log, it should show the detail below. And the log should follow the level color log rule")
- **Spec:** docs/tools/redis.md
- **Scope:** `tool-redis` Tools → Slow Log: a `DataTable` (id, time, duration, command, client), the clicked entry's detail below it; Duration painted with the shared log-level colours through `DataTable::show_coloured` and `slow_level` (owner's thresholds: under 10 ms DEBUG, under 100 ms INFO, under 1 s WARN, else ERROR); loading and error states.
- **Parent:** CR-0077
- [x] Code: `slow_level`, `DataTable::show_coloured`, the table, the detail pane, loading and error states
- [x] Test: `a_slow_entry_takes_the_level_of_its_duration` (tool-redis depth)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — Redis bench with `slowlog-log-slower-than 0`; click an entry, its detail shows below, durations coloured — Slow Log table, Duration in INFO colour, detail below, selected row readable (screenshots 120, 121)
- [x] Docs: CHANGELOG · README · docs/04 · docs/tools/redis.md
- **Notes:** workspace tests passed (1428 passed, 0 failed). Gate: deny + release self-test pending. App: screen check pending.


### CR-0093 · NoSQL Find and Aggregate: the clicked document's JSON in a right column
- **State:** Completed · every box checked 2026-11-08
- **Source:** owner, 2026-11-07 ("When click the item, the data json should show on the right column…")
- **Spec:** docs/tools/nosql.md
- **Scope:** `tool-nosql` Find and Aggregate: a click on a result row opens its JSON in a resizable column right of the results, instead of below them.
- **Parent:** CR-0091
- [x] Code: the open document in a resizable right column
- [x] Test: `hyperstack` `columns_tests::the_nosql_columns_hide_resize_and_hold_the_open_document` (HS_MONGO): in Find and Aggregate the clicked document's JSON heads a column right of the results, Save/Delete to its right, its edge drags
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — MongoDB bench: Find and Aggregate, click a row, drag the column's edge — a clicked Mongo document's JSON opens in the right column (screenshot 125)
- [x] Docs: CHANGELOG · README · docs/04 · docs/tools/nosql.md
- **Notes:** workspace tests passed (1428 passed, 0 failed); the Test box stays open because no test asserts the column. Gate: deny + release self-test pending. App: screen check pending.


### CR-0094 · Terminal and SSH: a shell tab takes the keyboard when it comes to the front
- **State:** Completed · built, gated and seen on screen 2026-11-07
- **Source:** owner, 2026-11-07 ("When user click on the tab, please focus straight to the shell/terminal…")
- **Spec:** docs/tools/terminal.md · docs/tools/ssh-sftp.md
- **Scope:** `tool-terminal` and `tool-ssh`: on the frame a shell's tab comes to the front (`became_active`), the shell takes keyboard focus - unless a dialog or another field already has it (`form::focus_is_elsewhere`), so a tab rename or an open dialog keeps its caret. Extends CR-0048's focus on open.
- **Parent:** CR-0048
- [x] Code: `became_active` in both tools, guarded by `focus_is_elsewhere`
- [x] Test: `focus_is_taken_only_when_the_tab_comes_to_the_front` (tool-terminal, tool-ssh)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-07: fmt, clippy, 1429 tests, cargo deny, self-test
- [x] App: `--data-dir ./tmp-data` — two shells and an SSH host open; click between their tabs and type at once — clicking the cmd tab and typing ran `echo focus-ok` without clicking in the shell (screenshot 138)
- [x] Docs: CHANGELOG · README · docs/04 · docs/tools/terminal.md · docs/tools/ssh-sftp.md
- **Notes:** workspace tests passed (1428 passed, 0 failed). Gate: deny + release self-test pending. App: screen check pending.


### BUG-0051 · Editor: the debug panel covers the editor
- **State:** Completed · fixed, gated and seen on screen 2026-11-08
- **Source:** found while retaking README screenshots 2026-11-07
- **Scope:** Editor: the debug panel opens at nearly the full pane height and covers the editor, so a paused line can't be seen; it can't be dragged smaller (80..600 range).
- [x] Test: reproduces the panel opening over the editor — `hyperstack` `the_debug_panel_keeps_its_height_frame_after_frame` (failed before: the Console tab climbed from y=394 to y=83 in 60 frames; then a 1/32 px a frame creep)
- [x] Code: the debug panel leaves the editor visible — the console lays its input row out bottom-up; the panel's content is floored to a whole pixel (min = max); `bottom_panel_max` caps terminal, debug and problems at the window less 200 px
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-08 (1430 tests)
- [x] App: `--data-dir ./tmp-data` — start a debug session in `debug-demo`, stop at a breakpoint; the paused line shows above the panel and the panel drags smaller — paused on line 16, panel 200 px and unchanged after 20 s; dragged to 264 px and 118 px, each held; the Variables carets draw (README editor-debug.png / editor-debug-stack.png retaken)
- [x] Docs: CHANGELOG · GOTCHAS (the BUG-0037 entry extended)



### BUG-0052 · Editor: the debug panel keeps the old call stack after Stop
- **State:** Completed · fixed, gated and seen on screen 2026-11-08
- **Source:** found on the BUG-0051 screen check, 2026-11-08
- **Scope:** after a debug session ends (Stop, or the program exits), the Call Stack and Variables tabs still list the last paused frames and variables; they should clear and show their empty state. Related: at the minimum window size the editor toolbar wraps to two rows while paused, which leaves the stopped line at the editor's bottom edge.
- [x] Test: ending a session clears frames, scopes and variables — `tool-editor` `stopping_a_session_clears_its_last_pause` (failed before the fix)
- [x] Code: the panel shows its empty state once the session ends — `Debugger::clear_paused`, called on Stop, Terminated and Closed; `Debugger::blank` builds one without services for the test
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — passed 2026-11-08 (1431 tests)
- [x] App: `--data-dir ./tmp-data` — pause in `debug-demo`, Stop; the tabs are empty — after Stop and after the program exited by itself, Call Stack and Variables showed their empty sentences and the Console kept the run's output
- [x] Docs: CHANGELOG


### CR-0096 · Debug console: evaluation only while paused, and Stop says so
- **State:** Completed · built 2026-11-08, checked in the app 2026-11-09
- **Source:** BUG-0052 screen check, 2026-11-08 (the evaluate field looked usable after the session ended; after Stop the console only said the program exited with code 0); owner: "proceed to do all the remaining"
- **Scope:** the console's evaluate field is disabled unless a session is paused, with a hint saying why; pressing Stop adds "Stopped by the user." once, after the adapter's own exit line.
- [x] Code: `Debugger::can_evaluate`, `stop_requested` / `take_stop_line`, new strings `debug-console-hint-disabled`, `debug-stopped-by-user`
- [x] Test: `tool-editor` `evaluate_needs_a_paused_program`, `stop_says_the_user_stopped_the_run`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-08: 1438 passed, self-test ok (run at -j 2)
- [x] App: `--data-dir ./tmp-data` — pause, the field is enabled; Stop, it is disabled and the console says the run was stopped — 2026-11-09, release build, debugpy on `debug-demo/app.py`: paused at line 15 the field was enabled and `order["customer"]` gave `'Ada'`; after Stop the console read "Stopped by the user." and the field "Evaluation needs a paused program." (the overlap seen there is BUG-0055)
- [x] Docs: CHANGELOG

### BUG-0053 · Git: the sidebar and dashboard read every repository's picture from disk every frame
- **State:** Completed · fixed 2026-11-08
- **Source:** code review of v0.7.7, 2026-11-08; owner: "proceed to do all the remaining"
- **Scope:** `entry_image` probed up to eight files and a stat per saved repository per frame on the UI thread; a `PictureCache` keyed on `icon_generation()` looks each one up once until an icon changes.
- [x] Test: `tool-git` `the_picture_cache_probes_only_when_the_icon_generation_moves`
- [x] Code: `PictureCache` in tool-git; `entry_image` takes the already-locked repo list (no nested read lock)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-08: 1438 passed, self-test ok (run at -j 2)
- [x] App: `--data-dir ./tmp-data` — the Git sidebar and dashboard show the repositories' pictures as before — 2026-11-09, release build: six saved repositories, each with its picture in the sidebar and in Saved Repositories
- [x] Docs: CHANGELOG

### BUG-0054 · A hidden browse column comes back at its minimum width, not the width the user gave it
- **State:** Completed · fixed 2026-11-08
- **Source:** the CR-0091 test `the_s3_bucket_column_hides_and_shows`, once the S3 columns filled their width (2026-11-08)
- **Scope:** every hideable column was drawn as an `exact_size(0.0)` panel while hidden, which stored a width of 0, so showing it again clamped to the minimum. A hidden column is now not drawn at all and egui keeps its width: S3 buckets, Database schema, Editor project column, Git tree, NoSQL databases, Graphics settings, diagram properties and generator prompt.
- [x] Test: `hyperstack` `columns_tests::the_s3_bucket_column_hides_and_shows` (failed before the fix: the hint came back at a different x)
- [x] Code: the eight columns skip `Panel::show` while hidden
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-08: 1438 passed, self-test ok (run at -j 2)
- [x] App: `--data-dir ./tmp-data` — drag the Editor and Database columns wider, hide and show them, the width holds — 2026-11-09, release build: the Editor project column, the Git tree and the Database schema column (666 → 788 px, 789 after hide and show, steady 3 s later) each kept the dragged width; plus `columns_tests::the_database_schema_column_resizes_and_keeps_its_width`
- [x] Docs: CHANGELOG, GOTCHAS

### BUG-0055 · Debug console: in a short panel the last output line is drawn under the evaluate field
- **State:** Completed · fixed 2026-11-09
- **Source:** the CR-0096 screen check, 2026-11-09: paused at a breakpoint, `order["customer"]` evaluated to `'Ada'` drawn under the field; after Stop, "Session ended." the same
- **Scope:** an `egui::ScrollArea` never shrinks below `min_scrolled_height` (64 px by default). The console's output area above the input row was shorter than that in a short panel, took 64 px anyway and spilled over the row. The console's scroll area now sets the minimum to 0.
- [x] Test: `hs-ui` `console_layout::a_short_console_keeps_its_output_above_the_field` (the console's own layout in a 120 px panel), with `the_default_minimum_spills_over_the_field` showing the defect; `console_ui` itself needs a live debug adapter, so it is checked in the app
- [x] Code: `min_scrolled_height(0.0)` on the console's scroll area
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-09: 1441 passed (run at -j 2), self-test ok, deny ok after BUG-0056
- [x] App: `--data-dir ./tmp-data` — a short debug panel, paused, evaluate an expression and Stop: every line above the field — 2026-11-09, release build with the fix: a short panel beside the terminal, `order["customer"]` and `grand` evaluated, the oldest line scrolled off the top and the newest above the field; after Stop every line above the disabled field
- [x] Docs: CHANGELOG, GOTCHAS

### CR-0098 · Title bar: the Workspace and Environment pickers are a field's height
- **State:** Completed · 2026-11-10
- **Source:** owner, 2026-11-10: "In the Main toolbar, Workspace selection dropdown size is too low to accommodate the workspace icon. Please resize it height. and make the Environment selection dropdown same height with the workspace dropdown new height"
- **Scope:** the Workspace button's 22 px mark filled it edge to edge and the Environment box was shorter still. Both pickers are now `form::field_height` tall (egui's Button and ComboBox take their minimum height from `interact_size`), set around the pair in `app::top_bar` and restored after it. ui.md's compact title-bar exception and docs/04 §7 say so.
- [x] Code: `app::top_bar`
- [x] Code: the bar's margin below its row matches the one above (8 px), and its height is the row plus both (`TOP_BAR_HEIGHT`, 54 px; it was 46, which clipped the row) - owner, 2026-11-10: "Why its near the bottom border. Please add margin/padding between dropdown and bottom border"
- [x] Test: the top bar is only drawn by `HyperStackApp`, which needs eframe's creation context, so no kittest reaches it; measured in the app instead (see App)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-10: 1443 passed (run at -j 2), self-test ok
- [x] App: `--data-dir ./tmp-data` — both pickers the same height, the workspace mark with room above and below, labels on their middle line — 2026-11-10, release build at 1736x900 with the lettered tile: Workspace 29 → 32 px, Environment 26 → 32 px, the 22 px tile inside with room above and below, both labels on the middle line; a workspace logo uses the same 22 px box (`WS_MARK_TITLE`), so it was not checked separately
- [x] Docs: ui.md, docs/04, CHANGELOG
- [x] Gate (bar height): fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-10: 1443 passed (run at -j 2), self-test ok
- [x] App (bar height): the pickers and buttons have the same air above and below inside the bar — 2026-11-10, release build: the bar's rule moved from y=76 to y=84; the pickers (y=45-74) now have 9 px clear below them, where they had about 1 px; about 14 px above, the row sitting ~1.5 px below its centre
- [x] Docs (screenshots): the README's 181 window screenshots and 3 GIFs show the taller bar — 2026-11-13: 48 retaken from the release build; the other 133 edited in place (the new bar pasted from a matching retake, the old picture moved down 8 px, bottom-anchored parts kept), owner's choice after the retake kept hitting usage limits; seven of those keep the old, shorter pickers. Details and limits in `docs/screenshots/README.md`
- **Notes:** the 133 edited screenshots are not captures; each is replaced when a later batch retakes its screen.

### BUG-0060 · clippy fails on Rust 1.100: two index loops
- **State:** Completed · fixed 2026-11-13
- **Source:** CI run 562 on `fix/top-bar-pickers`, 2026-11-13, a docs-only commit: the runner's toolchain moved to Rust 1.100, whose `clippy::needless_range_loop` now fires on a loop that indexes one collection by its counter; a result that does not depend on the branch
- **Scope:** two loops, each drawing rows with Move Up / Move Down: the interactive-rebase list in tool-git (`verbs.rs`) and Settings → Terminal's CLI list (`settings_ui.rs`). Both now iterate with `iter_mut().enumerate()`; nothing they draw changes.
- [x] Code: `tool-git` `verbs.rs`; `hyperstack` `settings_ui.rs`
- [x] Test: no new test - behaviour is unchanged; clippy with `-D warnings` on Rust 1.100 is the check, and the workspace tests still pass
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-13: 1443 passed (run at -j 2, with AdGuard, ExpressVPN and Tailscale paused - see GOTCHAS), self-test ok, deny ok
- [x] App: `--data-dir ./tmp-data` — Settings → Terminal's CLI rows and the interactive rebase list draw and reorder as before — 2026-11-13, release build: the CLI row's checkbox saved `enabled = false` then `true`; Interactive Rebase onto `fix` listed 3 commits, Up disabled on the first and Down on the last, Move Down reordered them and the action dropdown set squash; closed without applying
- [x] Docs: CHANGELOG

### BUG-0061 · cargo deny fails: russh 0.64 has RUSTSEC-2026-0495
- **State:** Completed · fixed 2026-11-13
- **Source:** the gate for BUG-0060, 2026-11-13: `cargo deny check` reported RUSTSEC-2026-0495 on `russh` 0.64.1 - its SSH agent *server* signs with confirm-constrained keys without asking `Agent::confirm`; a result that does not depend on the branch (testing.md)
- **Scope:** not reachable here - `hs-ssh` uses only the agent *client* (`russh::keys::agent::client`) to ask the user's own agent for signatures and never runs an agent server. The fix is in `russh` 0.65: the workspace's `russh` moves from 0.64 to 0.65 (0.65.1 in the lock file); no call site changed.
- [x] Code: `Cargo.toml` (`russh` 0.65), `Cargo.lock`
- [x] Test: `cargo deny check` passes; the workspace tests, including hs-ssh's
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-13: 1443 passed (run at -j 2, with AdGuard, ExpressVPN and Tailscale paused - see GOTCHAS), self-test ok, deny ok
- [x] App: `--data-dir ./tmp-data` — an SSH session to the bench host connects and runs a command — 2026-11-13, release build: Bench SSH (`hs-ssh`, started again after the machine's restart) connected, the SFTP pane listed `/config`, `uname -a` answered in the shell
- [x] Docs: CHANGELOG

### BUG-0059 · cargo deny fails: the bundled SQLite has RUSTSEC-2026-0459
- **State:** Completed · fixed 2026-11-09
- **Source:** the gate for BUG-0057/0058, 2026-11-09: `cargo deny check` reported RUSTSEC-2026-0459 (CVE-2026-119375) on `libsqlite3-sys` 0.35, whose bundled SQLite (before 3.54.0) can return uninitialised page-cache memory when a database is opened with a sibling `-wal` file of a different page size; a result that does not depend on the branch (testing.md)
- **Scope:** reachable here - the Database activity opens SQLite files the user picks (the history database is our own). The fix is only in `libsqlite3-sys` 0.39 (SQLite 3.54.1), which `rusqlite` 0.41 brings: the workspace's `rusqlite` moves from 0.37 to 0.41 (hs-core, tool-db). One call site changed: `Statement::columns()` now returns an iterator. The upgrade adds `rsqlite-vfs` and `sqlite-wasm-rs`, used only on wasm targets.
- [x] Test: `cargo deny check` passes; the workspace tests, including tool-db's SQLite driver tests and hs-core's history tests
- [x] Code: `Cargo.toml` (`rusqlite` 0.41), `Cargo.lock`, `tool-db` `driver/sqlite.rs`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-09: 1443 passed (run at -j 2), deny ok after BUG-0059, self-test ok
- [x] App: `--data-dir ./tmp-data` — the Database activity opens and queries a SQLite file; history is written — 2026-11-10, release build: Bench SQLite's header reads SQLite 3.54.1, `SELECT * FROM bank` returned 27 rows, and the run was added to History
- [x] Docs: CHANGELOG

### BUG-0057 · A picture opened from the command line shows as bytes in the Editor
- **State:** Completed · fixed 2026-11-09
- **Source:** the CR-0097 screenshot retake, 2026-11-09: `HyperStack.exe --data-dir … pic.png` opened the PNG in the text Editor as raw bytes
- **Scope:** the command line sent every path to the Editor, and the Editor's `open()` (any outside `OpenRequest::File`) went straight to `take_path`, skipping the picture rule `open_path` applies to a tree click. Pictures (`hs_core::sniff::is_image_path`) from the command line now open in Graphics, and the Editor's `open()` hands a picture to Graphics as `open_path` does.
- [x] Test: `hyperstack` `editor_tests::a_picture_sent_to_the_editor_opens_in_graphics`
- [x] Code: `app.rs` startup routing; `tool-editor` `open()`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-09: 1443 passed (run at -j 2), deny ok after BUG-0059, self-test ok
- [x] App: `--data-dir ./tmp-data` — start with a PNG on the command line: it opens in Graphics; a text file still opens in the Editor — 2026-11-10, release build: `HyperStack.exe --data-dir … hyperstack.png` opened the picture in Graphics with its Properties; text files opening in the Editor is covered by the test
- [x] Docs: CHANGELOG, GOTCHAS

### BUG-0058 · Graphics image editor: a settings row's label and Apply sit above its fields' middle line
- **State:** Completed · fixed 2026-11-09
- **Source:** the CR-0097 screenshot retake, 2026-11-09: in Image Settings → Transform the Size and Scale labels and the Apply buttons sit higher than the W / H fields beside them
- **Scope:** `Rows::row` in tool-graphics drew the label at its own height and Apply as a small button inside a row the fields make a field's height tall, so both sat at the top of the row. The label now fills a field-height cell centred on the row's middle line (keeping the panel's label column, truncation and tooltip), and Apply is the standard field-height `form::button`.
- [x] Test: `hyperstack` `graphics_tests::transform_rows_sit_on_one_middle_line` (Size, Scale, Margins, Area)
- [x] Code: `Rows::row` and the Apply column width in tool-graphics
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-09: 1443 passed (run at -j 2), deny ok after BUG-0059, self-test ok
- [x] App: `--data-dir ./tmp-data` — Image Settings → Transform and Adjust: labels, fields and Apply on one middle line, at the default size and narrow — 2026-11-10, release build at the window's minimum width (1736x900; the window cannot go narrower than 1720): Size, Scale, Margins, Fill, Placeholder, Area, Rotate and Flip each on one middle line, Apply a field's height in one column; Adjust's slider rows unchanged
- [x] Docs: CHANGELOG, GOTCHAS

### BUG-0056 · cargo deny fails: async-executor 1.14.0 was yanked
- **State:** Completed · fixed 2026-11-09
- **Source:** the v0.7.8 gate, 2026-11-09: `cargo deny check` reported `error[yanked]` for `async-executor` 1.14.0, which only `accesskit_unix` (egui's Linux accessibility) and `zbus` pull in - not compiled on Windows; a result that does not depend on the branch (testing.md)
- **Scope:** `cargo update -p async-executor` to 1.14.1 in `Cargo.lock`; no code change
- [x] Test: `cargo deny check` passes; `cargo check --workspace --all-targets --locked`
- [x] Code: Cargo.lock only
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test · cargo deny — 2026-11-09
- [x] App: not applicable - a Linux-only dependency, no visible change
- [x] Docs: CHANGELOG

### CR-0097 · A new application icon: the code stack ringed by tool tiles
- **State:** Completed · 2026-11-09
- **Source:** owner, 2026-11-08: "Change apps icon with this. Replace to all icon usage; Readme, executable, documentation, in apps logo, etc"
- **Scope:** the owner's 512×512 image becomes the master `assets/icon/hyperstack.png`; `scripts/make-icons.ps1` regenerates the `.ico` and every PNG size, so the exe (`build.rs`), the window icon, the top bar, Home's header and the README header all take it. README alt text and docs/03 §13 describe the new design; docs/04's brand palette keeps its colours (the theme is unchanged).
- [x] Code: master replaced, every size regenerated by the script (transparent corners checked)
- [x] Gate: release build · --self-test — 2026-11-08: built, self-test ok; the icon extracted from `HyperStack.exe` is the new picture
- [x] App: the exe's icon in Explorer, the window and taskbar icon, the top bar and Home's header show the new picture — 2026-11-09, release build: the icon extracted from `HyperStack.exe` (what Explorer shows), the window's title-bar icon, the top bar and Home's header, each the new picture
- [x] Docs: README, docs/03, docs/04, CHANGELOG
- [x] README screenshots: the logo replaced in place in 181 PNGs and every frame of the three GIFs, from a real capture of the new build (docs/screenshots/README.md, 2026-11-09 section); nothing else on those screens changed
- [x] `graphics.png` and `graphics-ops.png` retaken with the new icon as the picture open in the image editor — 2026-11-09, release build, 1736x900: Image Settings at Transform, and Adjust with the Operations list after Flip Horizontally (undone afterwards, nothing saved); neither image is referenced from the README or docs

---

## v0.7.6 — Done 2026-11-06

> **Shipped in:** BUG-0037, BUG-0039 and FEAT-0084 went out in app
> v0.115.0 (2026-11-02), merge `9dc90f2`; BUG-0040, the Scratchpad hang
> that v0.115.0 introduced, in v0.115.1 (2026-11-03), merge `3fb81ac`;
> CR-0026 to CR-0030, CR-0041 to CR-0043 and BUG-0041 in v0.115.2
> (2026-11-03), merge `a54ceab`. The rest of CR-0025's children shipped later,
> and the heading is Done.

> CR-0025 was split into the themed items below on the owner's decision
> (2026-11-02); every box kept its original wording. The shared primitives
> (CR-0026 … CR-0030) run first because the per-activity passes inherit them.
- **Notes:** `Theme::apply` zeroes `widgets.hovered/active/open.expansion` (tested).
- **Notes:** `tool_git::files::FilesView`; listings and reads on the blocking pool through slots; `EditorView.read_only` until Edit; a write returns `files::Event::Changed` and the pane re-reads its status. `tool-git` now depends on `hs-editor` (Cargo.lock updated). First cut of the branch came off a stale `origin/main` - the remote has no fetch refspec - and was rebuilt from the forge's head (GOTCHAS). Phase 3 pending.
- **Notes:** Enter in the name prompts did nothing at first - `request_focus()` ran before the `lost_focus()` read (GOTCHAS, Shell and UI Layout); fixed in the same item. The Settings → Git → Repositories clone stays full on purpose.
- **Notes:** seen on the check, not fixed here: the console's evaluate field still looks usable once the session has ended; after Stop the console says the program exited with code 0 (debugpy ends it cleanly) rather than that it was stopped.

### BUG-0037 · Miscellaneous Bug
- **State:** Completed ·
- **Scope:** All related Activities.
- [x] In Editor Activity -> Work Pane -> When Enable Debug Panel, Dont't auto resize the Panel when user move the mouse. User can resize the panel and it will persist for that project work pane.
  - Cause, found by instrumenting the panel and logging its stored height per frame: `egui::Panel` stores the rect its **content** produced, not the size it was given (`show_inside_dyn` shadows `shifted_outer_rect` with `inner_response.response.rect` before storing it, egui 0.35 `panel.rs`), so content that under-fills ratchets the panel smaller every frame. The measured sequence with no interaction at all was 197, 194, 191 ... 122, 121, then steady - about 3 px per repaint down to the content's own minimum. A repaint happens on every mouse move, which is exactly how the owner saw it, and it ate any dragged size within a second.
  - Fix: `ui.set_min_height(ui.available_height())` as the first call in `Debugger::panel_ui`, so the content rect equals the panel rect. Nine lines, all comment but one. This is why the shell's sidebar and dock never showed it - their content already fills.
  - A first attempt was reverted, not left in: feeding `default_size` from a stored height and reading it back with `PanelState::load`, as `app.rs` does. It cannot work, because `outer_size()` prefers the persisted `PanelState` over the default - the default was never consulted.
  - Verified in the app: the panel holds its height across 28 cursor moves (tab row 535 throughout, where it previously moved every repaint), a drag resizes it and sticks, and the size survives closing and reopening the panel. The grab row is one pixel below the drawn separator line.
- [x] In Editor Activity -> Work Pane -> When User Click "Go To Line"; Why the Label "Go To Line" didn't have left padding/margin?
  - The bar had no `Frame` at all while the find bar above it wraps in one; it now uses the same frame. Confirmed on screen: the two bars' left insets line up exactly.
- [x] In Git Activity -> Work Pane; When click "Fetch", there is error "git fetch: fatal: 'origin' does not appear to be a git repository fatal: Could not read from remote repository. Please make sure you have the correct access rights and the repository exists.". This repository is clone from the Settings -> Git -> Add Repository with selected Credential. Why its error?
- [x] In Automation Activity -> Forgejo Action -> Work Pane -> Runs Pane; When double click a run, its not open any logs. And there is no logs tab to click.
  - Implemented on the owner's decision (2026-11-02): Forgejo 11 serves no run log through its API, as `docs/tools/automation.md` records, so a double-click on a kind without `caps.log` now opens that run on the server through the existing `open_browser`, and the runs hint says why there is no Log tab.
  - Verified against the owner's bench `hs-forgejo` container (Forgejo 11.0.16, `dev/demo`, four runs). The profile was seeded anonymously - `actions/tasks` answers without a credential, and `none` is an allowed auth kind - so no token was needed. Double-clicking run #4 opened `http://127.0.0.1:3000/dev/demo/actions/runs/4` in the Browser activity, showing the job's expandable step logs, and the hint reads "This server does not serve run logs through its API. Double-click a run to open it on the server." The pane's tabs are Runs / Run / Queue with no Log tab, as the spec says.
- [x] In Logs Activity -> Work Pane -> Logs Pane; When Click Open In Editor button, it will open Editor Activity, but nothing display in the file.
  - Cause: `PATH_LINE` captures the path as the program wrote it, which is almost always relative (`crates/hs-core/src/lib.rs`), and that went to `OpenRequest::File` unresolved. The editor resolves a relative path against the **process's working directory**, so outside a shell started in the right folder it matched nothing: no editor tab claimed it, the shell opened a new one, and that tab failed to read the file - a blank Untitled editor with a toast as the only clue, which is exactly "nothing display in the file".
  - Fix: resolve in Logs before opening - as written, then beside the log file, then under each saved project root - and warn naming the path when nothing matches, as the terminal's `LinkTarget::Path` branch already does. Test: `a_records_path_resolves_beside_the_log_or_under_a_project`.
  - Verified in the app with it deliberately started from a different working directory so that could not mask the bug: the record's relative path opens `lib.rs` at line 12, and a record naming a file that does not exist warns "Could not find nowhere/not-a-real-file.rs beside the log or in any saved project." and opens no editor tab. Started from the repository root it had appeared to work, because the relative path resolved by accident - that trap is now in GOTCHAS.
- [x] In Settings -> Editor -> Project List -> Project Detail Setting Form; When click button "Close", it didn't close the project detail settings. Check other settings close button too.
  - The detail is drawn while `project_draft` is `Some` and Close cleared only `project_selected`. It needed a deferred flag, since the draft is mutably borrowed across the section closure. Confirmed on screen. The other five Settings close buttons (credential, repository, variable editor, the two secret dialogs) were checked and already clear their own draft.
- [x] Test: two reproducing tests. `an_empty_url_field_leaves_the_remote_fetchable` asserts an empty URL field leaves `remote.origin.url` and the listed remote intact; confirmed failing without the fix. The existing `a_projects_own_config_is_written_to_and_read_from_the_repository` had asserted the opposite - the behaviour that caused this bug - so its expectation was narrowed to exempt that one key, with the reason in the test. Nothing was skipped or deleted (owner agreed, 2026-11-02). `a_records_path_resolves_beside_the_log_or_under_a_project` covers the Logs path resolution, including the two cases that must not open an editor at all. The debug-panel and Close-button boxes have no unit test: both are layout and panel-state behaviour that only the running app shows, and `egui_kittest`'s synthetic input does not reproduce a panel resize drag.
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test — all four green on 2026-11-02 once BUG-0039 was cleared: fmt and clippy exit 0 with no warnings, 1241 tests passed / 0 failed / 0 ignored across 79 suites, and `--self-test` passed its 11 checks on the release exe built from 5b2a50c.
- [x] App: `--data-dir ./tmp-data` — All visual of resolved issues. All six confirmed on screen (2026-11-02, release build, scratch data folder): the go-to-line padding and its match with the find bar above it; the project detail form closing on Close; the debug panel holding its height across 28 cursor moves, a drag and a close/reopen; the Fetch failure via its test and the repaired `remote.origin.url` semantics; Open in Editor resolving a record's relative path from a working directory that could not mask it, and warning instead of opening a blank tab; and the Forgejo double-click opening the run on the server, against the bench container.
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS — `docs/tools/editor.md` (the remote-URL exception), `docs/tools/automation.md` (the Forgejo double-click), CHANGELOG under Unreleased, and six GOTCHAS entries. README needs nothing: no feature, requirement, build command or restriction changed. CHECKLIST needs nothing either — it mirrors the roadmap phases and carries no rows for `BUG-` items.
- **Notes:** all six reports are fixed and verified; every box is ticked.
  Two of the six turned out not to be HyperStack's own defects: the debug
  panel was `egui::Panel` storing its content's rect as the panel size, and
  the Forgejo box was a documented API limitation answered by opening the run
  on the server (the owner's decision). Two needed conditions this machine
  masked: the Logs box only appears when the app's working directory does not
  happen to contain the log's relative path, and the gate only went green once
  AdGuard's filters were out of the way (BUG-0039).
  The debug-panel box is **fixed and verified in the app** - see the note under
  it; the cause was in `egui::Panel`, not in our panel setup. The Logs
  "Open In Editor" box is **fixed and verified** - see the note under it; it
  needed the app started from a different working directory to show at all.
  **Gate** cannot be ticked on this machine: `test --workspace` is red through
  BUG-0039, which is outside the workspace (concurrent loopback connections
  are reset here). `fmt`, `clippy -D warnings` and the other 626 tests pass.
  The Forgejo box was answered by opening the run on the server rather than a
  Log tab, on the owner's decision (2026-11-02): Forgejo 11's API serves no
  run log, as `docs/tools/automation.md` records.

### BUG-0040 · Opening the Scratchpad activity hangs the app (regression in v0.115.0)
- **State:** Completed ·
- **Scope:** `crates/hyperstack/src/tools/scratchpad.rs`, the folder header reworked for FEAT-0084.
- [x] Reported by the owner on 2026-11-03: the app hangs on opening Scratchpad. Both running instances showed `Responding = False` at **0.6 s CPU** - blocked, not spinning - and the log stopped after startup with no panic line.
- [x] Mine, and released: the 31 October build is responsive against the same data folder and the current one is not, which pinned it to this session's work. Reverting only the folder-header rework made it responsive again, and it still hung with Scratchpad's Material icons switched off, so the pictures were not the cause - the `CollapsingState` rework was.
- [x] Cause: `CollapsingState::body` indents, and `Ui::indent` opens with `assert!(self.layout().is_vertical())`. `Reorder::row` runs its closure inside a `ui.horizontal`, so calling `body` on that `ui` panics the UI thread. eframe's loop dies while the window stays up, which presents as a frozen window at near-zero CPU rather than a crash. `CollapsingHeader::show` never tripped it because it makes its own vertical child.
- [x] Fix: wrap the header and body in `ui.vertical`, with the reason in a comment so nobody "tidies" it away.
- [x] Test: `scratchpad_folder_rows_draw_in_a_horizontal_parent` renders the folder level inside a horizontal layout headlessly, which panics without the fix.
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · cargo deny · --self-test
- [x] App: `--data-dir ./tmp-data` — Scratchpad opens, the folder draws its icon and caret, and two expand/collapse toggles leave the app responsive.
- [x] Docs: GOTCHAS entry; CHANGELOG line. No spec change: the behaviour is what it always was.
- **Notes:** **how it escaped.** FEAT-0084's App box was verified on the Editor, and I listed the activities I could not see with live data - but Scratchpad needs no connection and I reworked its folder header without ever opening Scratchpad in the running app. The ticked box was honest about connections and silent about the one activity I changed structurally and never looked at. A panic on the UI thread is also easy to misread: no dialog, no crash, just a window that stops responding.

### BUG-0039 · AdGuard's network filter resets concurrent loopback connections, failing every mock-server test
- **State:** Completed · **Spec:** docs/tools/automation.md
- **Scope:** The machine, not the workspace. Surfaces in `crates/tools/tool-automation/tests/mock.rs` and in any test that talks to a server on `127.0.0.1`.
- [x] Symptom: all five tests in `tests/mock.rs` fail with `error sending request for url (http://127.0.0.1:<port>/...)`. The error chain ends in `Os { code: 10054, kind: ConnectionReset }` - WSAECONNRESET, "An existing connection was forcibly closed by the remote host".
- [x] Not the v0.7.6 work: the same five fail on a clean tree at `d9d2aad` with this branch's changes stashed.
- [x] Not disk: they fail the same way with 523 GB free (see the disk entry in `docs/GOTCHAS.md`).
- [x] Not a proxy and not a timeout: `ProxyEnable` is 0, no `*_proxy` variables are set, the client's timeout is 5 s and the suite fails in 0.03-0.11 s.
- [x] Concurrency is the trigger: with `--test-threads=1`, four of the five pass. In parallel, between three and five fail, and which ones varies per run.
- [x] **Not HyperStack's code.** A pure-Python reproduction using nothing but `socket` and `threading` - four ephemeral loopback servers, one GET each, no tokio, no reqwest, no project code - fails at the same rate with the same `WinError 10054`. The plain-close probe is kept as `scripts/loopback-probe.py` for the next time this symptom appears; the FIN-then-drain variant was only to rule out an abortive close and was not kept.
- [x] Not an abortive close: the polite-close variant that shuts down the write half and drains to EOF before closing still resets on two runs out of three, so adding `TcpStream::shutdown` to the harness does not fix it. That speculative change was reverted rather than left in.
- [x] Not a third-party Winsock LSP: `netsh winsock show catalog` lists only Microsoft base providers. Windows Defender is the only AV registered and its real-time protection is on; the firewall is enabled on all three profiles, which does not normally filter loopback.
- [x] Cause: **AdGuard.** Its WFP callout and socket filter drivers (`adgnetworkwfpdrv`, `adgnetworksockdrv`) reset concurrent loopback flows. Disabling protection in AdGuard's own interface is **not enough** - the drivers stay attached and the resets continue. Stopping `Adguard Service` clears it, even though both drivers still show as Running afterwards: without the service they no longer act.
- [x] Test: with the service stopped, the Python probes passed 4 of 4 on three consecutive runs (12 of 12), and `cargo test -p tool-automation --test mock` passed 5 of 5 on three consecutive runs. Both had been failing minutes earlier on the same tree.
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: no app surface — nothing in the product is wrong, and no code changed.
- [x] Docs: GOTCHAS entry written; no spec, README, CHANGELOG or CHECKLIST change, since nothing shipped.
- **Notes:** found on 2026-11-02 running the gate for BUG-0037, diagnosed to outside the workspace and cleared the same day by stopping `Adguard Service` (owner). **It was already recorded**: `docs/GOTCHAS.md` had "AdGuard breaks loopback connections to Windows-side servers" from 2026-10-31, filed under "Headless and GUI Testing" where a network fault is not looked for, and its advice to "pause AdGuard's filtering" is not enough - pausing leaves the WFP and socket drivers attached and the resets continue. The two entries are now one, under "Windows Environment and Build", and this item is what the re-diagnosis added: that the service must be stopped, and `scripts/loopback-probe.py` to tell this apart from a real bug in seconds. No code changed: the only edits are this item and the `docs/GOTCHAS.md` entry, so the next session recognises the symptom in minutes instead of suspecting the drivers or the tests. Nothing in the product was wrong. Note that AdGuard will filter again after a restart unless its service is left stopped. `docs/tools/automation.md` records these tests as checked on 2026-10-31, so the machine changed since. The five affected are `teamcity_lists_triggers_and_cancels_through_the_build_queue`, `github_dispatches_with_inputs_and_cancels_a_run`, `gitlab_creates_a_pipeline_with_variables_and_cancels_it`, `awx_launches_a_template_with_its_survey_and_cancels_the_job` and `a_read_only_session_refuses_a_run_before_the_server_sees_it`.

### FEAT-0084 · Material Icon inside Activity Work Pane
- **State:** Completed · 
- **Scope:** All Icon Usage in All Activity Except API Client Activity, Terminal Activity, Automation Activity, Tracker Activity, Graphics Activity, Devtools Activity, Browser Activity and Logs Activity.
- [x] Create settings for Material Icons (Use in work pane). Add Settings -> Appearance -> Material Icon Card below Activity Rail Card.
  - `appearance.material_icons_off` (a set of tool ids, so the default of "on" needs nothing written) with `material_icons(tool)` / `set_material_icons(tool, on)` helpers, and the card drawn below Activity Rail in Settings -> Appearance.
- [x] User can enable/disable Material Icon on each activity listed. By default, all activity is enable. If disabled, use generic icon for respective item (folder and file).
- [x] Add this activity can show material icon or generic icon: Scratchpad's Sidebar Column - Folder and Note lists.
  - The folder header needed reworking: `CollapsingHeader` draws its title as one `LayoutJob`, which cannot hold a picture, so it is built from `CollapsingState::show_header` and `body()` returns the same full-width header response the click, drop target and context menu already used. The row click toggles explicitly, since only the caret does so by itself.
- [x] Add this activity can show material icon or generic icon: Editor's Work Pane; Project and Source Control Tab Icon, Folder and File list, 
  - The two tabs are a segmented control whose glyph was inline in the label, so `segmented` now takes an optional picture per segment and centres picture and text as one unit. The old glyph-only wrapper became unused and was removed rather than kept behind an allow.
- [x] Add this activity can show material icon or generic icon: Git's Work Pane -> Workspace column; Workspace, Branches, Remotes, Tags, Stashes with all its sub item/child (where applicable).
  - The tree's own `row` helper gained a picture form. The tick on the current branch stays a glyph: it is state, not a kind.
- [x] Add this activity can show material icon or generic icon: SSH's SFTP - File and Folder list. 
  - Already drew Material file icons; it now honours the switch and drops the icon column when off.
- [x] Add this activity can show material icon or generic icon: Container's Work Pane - Item list (Container, Projects, Images, Volumes, Network, Stacks, Services, Nodes, etc).
  - One icon serves the whole table, from a new `Resource::kind()`. The Kubernetes table keeps its glyphs: pods and deployments have no Material picture, and a wrong picture is worse than a glyph.
- [x] Add this activity can show material icon or generic icon: Database's Work Pane - Item list (Database, Tables, Table Item, Table Column, View, Functions, Saved Queries, History, all sub item/child, etc).
  - Schema, tables, views, functions, columns and primary keys all draw their own picture. A foreign key keeps the link glyph and the loading and error nodes keep theirs: those are status, not item kinds.
- [x] Add this activity can show material icon or generic icon: NoSQL's Work Pane - Item list (Database, Collections, Documents, Fields, all sub item/child, etc).
  - Database and collection rows. The caret stays as the expand state and the picture is part of the click target, so clicking the icon selects as clicking the name does.
- [x] Add this activity can show material icon or generic icon: Redis's Work Pane - Item list (Folders, Keys, all sub item/child, etc).
  - Key folders and keys, with the caret kept and the picture inside the click target.
- [x] Add this activity can show material icon or generic icon: S3's Work Pane - Item list (Bucket, Object, Value, Key, Metadata, Folder and Files (webdav), all sub item/child, etc).
  - The glyph was inline in the name cell, so the cell drops it and the table draws the picture in its own column.
- [x] Add this activity can show material icon or generic icon: Queue's Work Pane - Item list (Queue, Topic, etc).
  - A new `destination_kind()` maps RabbitMQ to a queue and Kafka and MQTT to a topic; streams read as queues, being an ordered backlog someone consumes.
- [x] Only these Activities are allowed to enable/disable material icon: Scratchpad, Editor, Git, SSH, Container, Database, NoSQL, Redis, S3, Queue.
  - `settings::MATERIAL_ICON_TOOLS` is that list and the card is generated from it, so the two cannot drift. `material_icons()` returns false for any activity outside it.
- [x] Test: `every_item_kind_an_activity_uses_has_an_icon` (every kind a pane passes resolves, plurals and case included, and an unmapped kind returns `None` so the pane keeps its glyph) and `the_switch_decides_between_a_picture_and_the_panes_own_glyph` in `hs-ui`; `material_icons_are_on_by_default_and_only_for_the_listed_activities` in `hs-core`, which also pins that the setting stores only the exceptions.
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — Enable and disable Material icon for each activities reflect immediately. Checked on 2026-11-03 against a release build: the Editor's tab pictures and project tree draw the Material icons, unchecking Editor in Settings -> Appearance -> Material Icon turns **both** back to plain glyphs at once, and checking it again restores them without a restart and without collapsing the tree. The 26 new icons were also rendered at 96 px and reviewed one by one before use; `view` and `stash` were redrawn because they did not read.
  **Not seen with live data**: the item lists in Container, Database, NoSQL, Redis, S3, Queue and Git need a connection of each kind, which this machine has only for Forgejo. Their wiring is covered by the unit tests and by `clippy`, not by a screenshot.
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS — `docs/04-ui-design.md` describes the item-icon language, the ten activities and the no-misleading-picture rule; `assets/icons/material/README.md` records that the 26 new icons are ours rather than upstream MIT art, with the palette convention; CHANGELOG has its line. README and CHECKLIST need nothing: no requirement, build command or roadmap phase changed.
- **Notes:** done 2026-11-03. 26 item icons drawn for the
  kinds the upstream Material theme has nothing for (owner's decision over
  glyph fallbacks), registered in `fileicons::ICONS`, resolved by
  `fileicons::for_kind`, with the per-activity switch and the Appearance card.
  Provenance is recorded in `assets/icons/material/README.md`: these 26 are
  ours, not upstream MIT art. The per-activity wiring (the ten boxes below) is
  what remains; the owner asked for the whole item in one commit, so nothing is
  committed in one change, as the owner asked. Two panes needed rework rather
  than wiring: Scratchpad's folder header (a `CollapsingHeader` title is one
  `LayoutJob` and cannot hold a picture) and the Editor's sidebar tabs (the
  glyph was inline in a segmented control's label). Three kinds deliberately
  keep their glyphs - Kubernetes pods, a foreign-key column and the loading
  and error rows - because no Material picture says those things.
  
### CR-0025 · Redesign UI UX on all activities, components, dashboards to a standard design.
- **State:** Completed · every child Completed, the last (CR-0042) on 2026-11-06
- **Scope:** All related UI components. Umbrella item — the work lives in its children.
- **Depends on:** FEAT-0084
- [x] CR-0026 · Shared control behaviour: hover highlights only, middle alignment, field heights
- [x] CR-0027 · Toolbar strip standard: sizing, label and icon colour, labelled icon buttons, sub-rows
- [x] CR-0028 · Table and scroll-area standard: cell sizing, emphasised headers, key/value tables
- [x] CR-0029 · Tab strip heights, icon sizing and position-aware context menus
- [x] CR-0041 · Scratchpad: one pin for the note, the sidebar and the tab (added 2026-11-03)
- [x] CR-0030 · Sidebar column header standard across every activity
- [x] CR-0042 · Queue and Automation sidebars titled after their activity (added 2026-11-03)
- [x] CR-0043 · Fixed tabs wear their activity's colour, and the Editor's its workspace logo (added 2026-11-03)
- [x] BUG-0041 · Pinning a note from the sidebar did not put it on the tab strip (added 2026-11-03)
- [x] BUG-0042 · The release check's terminal probe can sink the whole self-test (added 2026-11-04)
- [x] FEAT-0093 · Add-and-focus: a Settings jump target per kind, and Save opens the new connection
- [x] FEAT-0094 · Remove From List: delete a saved profile and its credential from the sidebar
- [x] BUG-0038 · Editor -> Source Control -> Refresh Status does nothing
- [x] CR-0035 · Editor work pane and Source Control tab layout
- [x] CR-0036 · Multi-coloured product icons for every connection kind
- [x] CR-0031 · Spacing pass: Redis, S3 and Queue work panes
- [x] CR-0032 · Spacing pass: Database result and message panels
- [x] CR-0033 · Spacing pass: Automation, Tracker and Logs work panes
- [x] CR-0034 · Container and SSH work pane layout
- [x] CR-0039 · Spacing pass: NoSQL work pane
- [x] FEAT-0095 · Server-populated fields: Tracker and Automation
- [x] FEAT-0096 · Database: results retained per query tab, and double-click a cell to edit
- [x] FEAT-0097 · S3: a collapse/expand folder hierarchy column
- [x] FEAT-0098 · SSH: name a tunnel when creating it
- [x] CR-0037 · Redis key list sorted folders first
- [x] CR-0038 · Settings polish, and the UI standard written down
- **Notes:** split on 2026-11-02 (owner's decision). This item holds no work of
  its own; it is `Completed` when every child above is `Completed`. The 218
  original boxes were distributed verbatim across the children — none was dropped.
  "Close button didn't close the project detail form" stayed with BUG-0037, whose
  sixth box is the same defect, and FEAT-0093 builds on that fix.

### CR-0026 · Shared control behaviour: hover highlights only, middle alignment, field heights
- **State:** Completed · one box skipped on the owner's decision (no "Unfocus result" control exists)
- **Scope:** `hs-ui` button, form and dropdown primitives; every activity inherits them.
- **Parent:** CR-0025
- [x] All buttons when hover, only highlight the button. Do not magnify or scale the button's size.
  - Measured in the running app first (2026-11-03). A plain toolbar button does **not** scale: hovering paints a 54x18 background and the label's bounding box is identical to the pixel. What does change size is a **frameless** control - `toggle_value` / a bare `selectable_label` - which is invisible until the pointer touches it and then sprouts a box around the text. The Find bar's `Aa` / `ab` / `.*` went from nothing to a 49x26 box around an 11 px label. So "do not magnify" and box 28's "make it a proper button" are the same defect, and the fix is a persistent frame rather than anything about hover itself.
  - Not a blanket sweep: there are 13 `toggle_value` and 119 `selectable_label` call sites, and most of the latter are list rows, where a frame would be wrong. Only the ones that sit in a toolbar or a button row are converted.
- [x] In All Activities -> Resize all text field height, slightly higher; For example in Home -> Dashboard Page, "What do you want to do?" field height is too small. Please follow text field height size in Settings -> Accessibility - Speech To Text -> Language. Make also the height size of all dropdown field, same height with the new text field height.
  - The standard already exists and the Language field you named uses it: `form::text_field` applies `FIELD_MARGIN` (7 px top and bottom), `form::field_height` computes the result and `form::field_row` sizes a ComboBox or button beside one to match. The short fields are the ones that bypass all three.
  - The launcher's "What do you want to do?" box is fixed - it was a bare `TextEdit::singleline` with egui's default `Margin::symmetric(4, 2)`.
  - **Reach decided by the owner (2026-11-03): form-like fields only.** Raised the nine full-width sidebar search and filter boxes plus the NoSQL query field - Scratchpad, API Client, Browser, Database (objects and history), Devtools snippets, Git (history and filter), NoSQL - and the launcher box. Deliberately left alone: the filter inside a ComboBox popup in Settings -> Git, and the inline editors inside tables, where a 30 px control in a 20 px row would be worse than what is there.
  - **Dropdowns, done for forms.** `form::field_grid` now sets `interact_size.y` to `field_height` for its cells, so every ComboBox, checkbox and button *inside a form* comes out at the field height without each call site saying so. Dropdowns that sit in a toolbar rather than a form - Database's "500 rows", "30 s timeout", "Auto-Commit" - are left at egui's height; raising those is CR-0027's toolbar sizing, which does it for buttons and dropdowns together.
  - There is no global knob for the fields themselves: egui hard-codes a `TextEdit`'s margin and does not read it from the style, so each field must be told. There are 145 `TextEdit::singleline` call sites against 11 uses of `FIELD_MARGIN`. Many of those are not form fields - inline table-cell editors, the palette's search, filter boxes inside lists - where a 30 px control would be wrong. Awaiting the owner's call on whether this means every field or only the form-like ones.
- [x] In All Activities -> All Inline Buttons, Label, Text Fields, Dropdown menu, Slider, Checkbox, radio button, etc; Vetical middle align all of it.
  - A `ui.horizontal` already centres what it holds (`Align::Center`), so inline rows were not the defect. Form rows were: an egui `Grid` centres each cell in its row (`Align2::LEFT_CENTER`), so a cell holding a control *and* a description under it makes the row tall and the label beside it drops to the middle of the pair. Both halves of the fix live in `hs_ui::form` - the grid sizes its cells to a field's height, and `form::field_hint_row` puts a description on its own row.
- [x] In Editor Activity -> Work Pane -> Toolbar Strip; When User Click "Find" button, do not magnify/resize Match Case,Whole Word and Regular Expression button. Make it as a proper button so that user know it is a button. Apply it to all other Activities toolbar, sub toolbar that have same characteristic of these button.
- [x] In Database Activity -> Work Pane -> Result Section Header; When hover Message button, don't magnify, resize the button. Please check other activities that have same characteristic of these button and apply to it also.
- [~] In Database Activity -> Work Pane -> Result Section Header; When hover Unfocus result button, don't magnify, resize the button. Please check other activities that have same characteristic of these button and apply to it also.
  - **Skipped on the owner's decision (2026-11-03).** No control named "Unfocus" exists in the code or `locales/en.ftl` - the only match in the workspace is an unrelated comment about browser focus - so it could not be identified without guessing. The Message button beside it and the result tabs are now framed, so if it shares that strip it is already covered.
- [x] In Automation Activity -> Work Pane -> Job Activity Pane -> Run Tab; Please vertical middle align label with input text, vertical middle align label with checkbox and checkbox label. Apply this to all activities that have form to look professional and standard.
  - Checked against the Forgejo bench's `build.yml`, whose form has all three kinds: `Ref *` now sits level with the text field, `target` with the dropdown and `verbose` with the checkbox. The Settings forms inherit the same grid. The stacked cells in Settings -> Workspace (a field with a button and a hint under it) keep centring on the stack, which is what a multi-control cell should do.
- [x] Test: a widget snapshot proves a hovered button keeps its rect, and that a text field, a dropdown and a checkbox row share one baseline
  - `crates/hs-ui/tests/form_rows.rs`: a checkbox in a form row comes out at `field_height` and a looser one does not, and a `form::toggle_button` has the same rect hovered as cold. Whether the frame is *painted* when the pointer is away is not observable headlessly - that one is the app check below.
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — hover every toolbar and inline button in three activities — nothing grows; the Home "What do you want to do?" field matches Settings -> Accessibility -> Language
- [x] Docs: spec · CHANGELOG · GOTCHAS (no README or CHECKLIST line: no feature or phase moved)

### CR-0027 · Toolbar strip standard: sizing, label and icon colour, labelled icon buttons, sub-rows
- **State:** Completed · the NoSQL pane was not exercised (no MongoDB or CouchDB server on this machine)
- **Scope:** `hs_ui::toolbar` and every activity's toolbar and sub-toolbar.
- **Parent:** CR-0025
- **Depends on:** CR-0026
- [x] Label all icon buttons. For example, in Scratchpad Activity -> Scratchpad Sidebar Column -> Header; Add label "Add Folder" for Make a folder button.
  - Scratchpad's folder button is labelled Add Folder, and the Browser sidebar's New Folder and New Login buttons carry theirs. Labelled, two buttons no longer fit beside a sidebar title, so the Scratchpad header puts its title on one line and the buttons under it.
- [x] In All Activities -> Toolbar Strip -> Label and Icon; Please color the label and icon with suitable and appropriate color (depends on mode dark/light; maybe follow accent, harmony, contrast, etc where do you think is professional and acceptable), that Showing it is a toolbar and important.
  - `hs_ui::toolbar::item_text` builds a toolbar item's text in two colours - the icon in the accent, the label in the ordinary text colour - and `btn` / `btn_enabled` / `tab` / `toggle` draw with it. Every activity's `fn toolbar` now goes through them (about 60 call sites). A disabled button mutes both, since a `LayoutJob`'s own colours survive egui's dimming.
- [x] In All Activities -> Toolbar Strip -> Toolbar Button With Label; make all button slightly bigger (medium) with top/bottom/left/right padding/margin, to look proper button. 
  - `hs_ui::toolbar::strip_style`, applied once where the shell draws a tool's toolbar: `button_padding` 10x6 and `interact_size.y` at `form::field_height`. Every button, dropdown and chip in every toolbar inherits it, which also finishes CR-0026's dropdown clause for toolbars.
- [x] In All Activities -> Toolbar Strip -> Status Label with Icon (Connected, Disconneted, etc); make all icon and text slightly bigger (medium) with top/bottom/left/right padding/margin, to look proper button. 
  - `hs_ui::toolbar::status` draws a chip the height of the buttons beside it: the mark in the state's colour, the text in the ordinary one. Used by SSH's connection light, Logs' follow state, the Database transaction and read-only marks and the Git branch.
- [x] In All Activities -> Sidebar Column -> Button With Label; make all button slightly bigger (medium) with top/bottom/left/right padding/margin, to look proper button.
  - The shell sets the toolbar's `button_padding` on the sidebar column too, so a sidebar button is the same size as a toolbar one.
- [x] In Editor Activity -> Work Pane -> Toolbar Strip; When User Click "Find" button, add Label for all buttons (Match Case,Whole Word, Regular Expression, Previous and Next button. Apply it to all other Activities toolbar, sub toolbar that have same characteristic of these button.
  - The editor is a library with no locale of its own, so the find bar's words come from the app through a new `hs_editor::view::FindLabels` (the shape `TabStripLabels` already uses), filled from Fluent in `tool-editor`. Aa / ab / .* are now Match Case, Whole Word and Regular Expression, and the arrows are Previous and Next.
- [x] In Terminal Activity -> Work Pane -> Toolbar Strip; When User Click "Find" button, why the other button (Older Match, Newer Match, Find text field, Match Case, Regular Expression, etc) overlapped with the main toolbar button. It should be in a row below the main toolbar row.
  - The find controls are drawn after a `toolbar::row_break` in a strip of their own. Drawn straight into the toolbar's wrapping row they were squeezed against its right edge and the rest wrapped on top of them, which is what the overlap was.
- [x] In Terminal Activity -> Work Pane -> Toolbar Strip; When User Click "Find" button, add Label for all buttons (Older Match, Newer Match, Match Case, Regular Expression). Apply it to all other Activities toolbar, sub toolbar that have same characteristic of these button.
- [x] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Toolbar button; Please make a proper button for Action (Run, Count, New Document, Delete Matching, etc) with top/right/bottom/left padding/margin and add proper icon for each button if none. 
  - Run keeps the primary button; Count, New Document and Delete Matching are framed toolbar buttons with Hash, File Plus and Trash.
- [x] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Toolbar button; Please make a proper label for page button (The page before this one, The next page).
  - The bare < and > are Previous Page and Next Page with caret icons; the sentence tooltips stay as tooltips.
- [x] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Toolbar button; Please make a proper button for Result Format (Table, Tree, Json, etc) with top/right/bottom/left padding/margin and add proper icon for each button if none. 
  - Table, Tree and JSON are framed selectable buttons with Table, Tree Structure and Brackets Curly.
- [x] In Redis Activity -> Work Pane; Please add all the button in this Activity with top/right/bottom/left padding/margin and add proper icon for each button if none. (Clear, Bulk Ops - Delete, Expire, Persist, Rename Prefix, Apply. etc)
  - The activity's panes get the toolbar's padding, the four bulk actions get icons and frames, the Tools sub-tabs are framed with icons, and Pub/Sub's Clear has an eraser.
- [x] In Logs Activity -> Work Pane -> Toolbar Strip; Put AI Assistant button on the first row of the toolbar strip.
  - `toolbar::hard_right` measured the corner against `min_rect`, which by then was every row the buttons had taken, so the assistant centred between the rows. It is one row tall now, in every activity.
- [x] In Logs Activity -> Work Pane -> Toolbar Strip; Draw the horizontal line separator in first row and second row full width. Apply this to all AI Assistant button and horizontal line that have a same characteristic in other activities.
  - `toolbar::row_break` draws its rule across the toolbar's whole rectangle rather than the row the shell narrowed to keep the corner clear.
- [x] In Browser Activity -> Sidebar Column -> Favorites Tab; Add label for "New Folder" on the button with the icon.
- [x] In Browser Activity -> Sidebar Column -> Passwords Tab; Add label for "New Login" on the button with the icon.
- [x] Test: the toolbar builder emits a label for every button, and a sub-toolbar row lays out below the main row rather than inside it
  - `crates/hs-ui/tests/toolbar_rows.rs`: a toolbar item's text ends with its label and colours the icon with the accent, and what follows a row break starts below the row before it, back at the left edge. That every *call site* passes a label was checked by eye in the app.
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the Terminal and Editor Find rows sit below the main toolbar with labels; Logs draws the AI assistant on the first row
  - All three checked, plus the Database, Editor, Terminal and Redis toolbars, the Scratchpad and Browser sidebars, and Redis' Tools sub-tabs and Bulk Ops against a live server on 6379. **Not checked: the NoSQL pane** - there is no MongoDB or CouchDB on this machine, so boxes 9-11 are code-reviewed only.
- [x] Docs: spec · CHANGELOG · GOTCHAS (no README or CHECKLIST line: no feature or phase moved)

### CR-0028 · Table and scroll-area standard: cell sizing, emphasised headers, key/value tables
- **State:** Completed · Tracker, Containers and the NoSQL result header are code-reviewed only (no server on this machine)
- **Scope:** `hs_ui::table::DataTable`, the shared key/value renderer and every scrollable text area.
- **Parent:** CR-0025
- **Depends on:** CR-0026
- [x] In All Activities -> All Work Pane -> Table Component (Where applicable); Make the table size and value inside it slightly bigger (cell, row, etc). Make all table header text and row, emphasized (bold, upsize, fill header cell color, etc) to show it is a header (where applicable).
  - `hs_ui::table::ROW_HEIGHT` (26) and `HEADER_HEIGHT` (30) are the one pair of numbers; every `DataTable` call site and the Database result grid use them, so rows are taller everywhere and a header stands above them. `table::header_band` fills the header cells with `theme.surface` and rules them off, and the header's text is bold at 13.5. The Database grid's row-number gutter paints the same band, so the strip does not break where it meets the columns.
- [x] In All Activities -> All Work Pane -> All Vertical Scrollable Text Area (Status, Logs, etc) put a darker background (for dark mode) in the text area, or light background (for light mode) in the text area (Where applicable). So that user will notice it can be scrollable.
  - `hs_ui::form::well` and `scroll_well` draw a field-coloured, bordered panel - darker on a dark theme, lighter on a light one, since both come from `theme.field_bg()`. Used by the Devtools listening-port list and the Automation run log. A pane that already paints its own background (the Logs view, the editors) is left alone.
- [x] In All Activities -> All Work Pane -> All Item And Value Data (e.g Type   Bug, Author     Redmine Admin, etc); Make a proper table without header for it to look professional, tidy and structured.
  - `hs_ui::form::detail_table`: a striped two-column grid with no header, rows a table's height, the name muted. Used by the Tracker item's fields and the Containers container and cluster details. Three lists that need their own formatting - the JWT claims (expiry in the error colour), the URL parts and the API response headers (monospace) - keep it, and take the striping and row height only.
- [x] In NoSQL Activity -> Work Pane -> Result Header; Make the header icon and label slightly bigger to show it is a header.
  - The summary and count lose `.small()`, so the strip reads at body size beside the framed format buttons CR-0027 gave it.
- [x] In Devtools Activity -> Listening Port -> Make the Listening Port list background darker.
- [x] In Settings -> Browser -> Extra Block Hosts Card; Do not make the text area auto expanded. Use fix height and vertical scrollable for the text area.
  - Six rows in a `ScrollArea` with `max_height` and `min_scrolled_height`, so the box holds its height and scrolls instead of pushing the page down.
- [x] Test: DataTable renders an emphasised header row, and the key/value helper produces a two-column headerless table
  - `crates/hs-ui/tests/table_header.rs`: the first row starts a header's height below the column name, and an item-and-value list puts its values in a second column with the names lined up a row's height apart.
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a wide table in Database and Tracker reads as a table; the Devtools listening-port list and the Settings -> Browser block-hosts box show their scroll background
  - Checked: the Database result grid (header band across the gutter and the columns, taller rows), the Devtools listening-port list in its well, the Settings -> Browser box holding six rows and scrolling, and the Devtools URL Parts item-and-value table. **Not checked: Tracker and Containers** - no Redmine/Jira server and no container host on this machine - and the NoSQL result header, which needs MongoDB. Those three are code-reviewed only.
- [x] Docs: spec · CHANGELOG (no README or CHECKLIST line: no feature or phase moved; no new mistake for GOTCHAS)

### CR-0029 · Tab strip heights, icon sizing and position-aware context menus
- **State:** Completed · checked in the running app on the Scratchpad, Editor and Settings strips
- **Scope:** `hs_ui::tabs`, the Settings -> Appearance -> Tab Card, every main and inner tab strip.
- **Parent:** CR-0025
- [x] In All Activities -> Main Tab Strip -> Make the tab size slightly higher, and have a setting for it in Settings -> Appearance -> Tab Card -> Minimum Main Tab Height.
- [x] In All Activities -> Main Tab Strip -> Make the icon,title and close button in the tab slightly bigger. The size will be suitable with the main tab height settings.
- [x] In All Activities -> Sub/Child/Inner Tab Strip (Where applicable) -> Make the tab size slightly higher but not high than main tab height, and have a setting for it in Settings -> Appearance -> Tab Card -> Minimum Sub/Child/Inner Tab Height.
- [x] In All Activities -> Sub/Child/Inner Tab Strip (Where applicable) -> Make the icon,title and close button in the tab slightly bigger but not biggeer than main tab icon and text size. The size will be suitable with the Sub/Child/Inner tab height settings.
- [x] In All Activities -> Main Tab Strip; If the Tab is the first after the fixed/permanent tab, remove right click menu "Close To The Left". If no other tab at right of the tab, remove right click menu "Close Others", "Close To The Left", "Close To The Right". If there is a tab or more at right of the tab, remove right click menu "Close To The Left". If a tab is the last tab on the right of the strip with some other non fixed/permanent/pinned tab on its left, remove right click menu "Close To The Right". Apply to all Activities where applicable.
  - **One rule, confirmed by the owner (2026-11-03):** an entry is offered only when it closes something, and never when it closes exactly what Close Others closes - "Close To The Right will have the same effect of it". So the permanent tab and the tabs at either end get Close Others alone, and a tab with closable tabs on both sides gets all three.
  - **A pinned tab is the exception.** Close Others spares the other pinned tabs, so from a pinned tab it does not mean what it says and is not offered; its menu is Close plus whichever direction has something to close. Neither duplicates anything, because Close Others is not there.
- [x] In Scratchpad Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Editor Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Git Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Terminal Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In SSH Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Container Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Database Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In NoSQL Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Redis Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In S3 Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Queue Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In API Client Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Automation Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Tracker Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Logs Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Graphics Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Devtools Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [x] In Browser Activity -> Inner Tab Strip; If the tab is the first left, remove the right click menu "Close To The Left" menu and "Close To The Right" menu. If the tab is the last most right, remove the right click menu "Close To The Right" menu. Please use suitable logic on this for every tab position right click menu.
- [x] In Scratchpad Activity -> Main Windows Tab Strip; Remove the Duplicate right-click menu on every tab.
  - Added 2026-11-03 at the owner's request. `TabItem::shows_duplicate` drops the entry rather than greying it out, which is what `can_duplicate` does.
- [x] In Settings Page -> Main Windows Tab Strip; Remove "New Tab" button and remove right click menu for the fixed tab of Settings.
- [x] Test: the context-menu builder omits Close To The Left for the first closable tab and Close To The Right for the last, and the height settings round-trip through the workspace file
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — change both heights in Settings -> Appearance -> Tab Card and watch every strip follow; right-click tabs at each position in three activities
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0041 · Scratchpad: one pin for the note, the sidebar and the tab
- **State:** Completed · checked in the running app
- **Scope:** `hs_core::tool::Tool`, `hs_ui::tabs`, the shell's tab strip, `tool-scratchpad`. **The Scratchpad only** - every other activity keeps the shell's own tab pinning (owner, 2026-11-03).
- **Parent:** CR-0025
- **Depends on:** CR-0029
- [x] Pinning a note in the sidebar column pins its tab; unpinning there unpins the tab.
- [x] Pin in the tab's right-click menu pins the note in the sidebar as well; Unpin there unpins it in both.
- [x] The pinned tabs' order follows the sidebar's pinned order.
- [x] A pinned note's tab cannot be closed; its menu offers Close to the Right whenever there is a closable tab, whatever the tab's position, and that never closes another pinned note.
  - Added 2026-11-03 at the owner's request. A pinned tab is drawn in the lead zone, so every closable tab is to its right wherever it sits in the list; `close_scopes` reads the scope from where a tab is *drawn*, and the shell's Close to the Right from a pinned tab closes every closable tab (the pinned ones are spared by `close_others`/`close_side` already).
- [x] The pin mark is red, in the sidebar and on the tab; a pinned note's tab shows the pin in place of its icon, while an unpinned note keeps its Material icon and colour.
  - Added 2026-11-03 at the owner's request. `Tool::tab_icon_is_pin()` carries it; the strip draws `ph::PUSH_PIN` in `theme.error`.
- [x] Settings -> Appearance -> Scratchpad -> Pin Colour, from a predefined set.
  - Added 2026-11-03 at the owner's request. Eight colours that read on dark and light schemes (`hs_core::settings::PIN_COLOURS`); `Theme::pin` carries the choice, so the sidebar and the strip draw the same one. Picking a colour is a *theme* change: the strip reads the shell's cached theme, which a plain settings change did not rebuild, so the tab's pin stayed the old colour while the sidebar's followed - the owner's report, fixed.
- [x] An unpinned note's tab wears the Material note icon its sidebar row does, when Material icons are on for the Scratchpad, and the plain glyph when they are off.
  - Added 2026-11-03 at the owner's request. `Tool::tab_item_kind()` names the kind ("note"); the shell maps it through `fileicons::for_kind` and hands the strip the image directly, since an `include_image!` source has no loadable `bytes://` uri on its own.
- [x] Test: a note's pin state drives its tab's, whichever side set it, and the lead slots sort by the sidebar's order
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — pin two notes in the sidebar, watch the tabs pin in that order; unpin one from the tab menu and watch the sidebar follow
- [x] Docs: spec · CHANGELOG
- **Notes:** The owner's words, 2026-11-03: "Pinned note on the sidebar column, will pinned the note on the tab too. If unpinned note on the sidebar column the note, it will unpinned on the tab too. Same goes to pinned in tab right click menu will pin the note in the tab and the sidebar column, while unpinned the tab in the right click menu will unpinned the note in tab and sidebar column too. Pinned order in the tab will follow pinned order in sidebar column." Filed as its own item because CR-0029 is about tab heights and menus; this is one activity's pin model.

### CR-0030 · Sidebar column header standard across every activity
- **State:** Completed · Home keeps its own header (not in this item)
- **Scope:** Every activity's sidebar header; the Container header is the reference.
- **Parent:** CR-0025
- **Depends on:** CR-0027
- [x] In Scratchpad Activity -> Sidebar Column -> Header; Add label for "Add Folder" and "Add Note" on the buttons with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style in same row but add horizontal separator between the buttons and Search Note Section (with top/bottom margin/padding) and without the Second "Containers" title.
  - `hs_ui::form::sidebar_head` is the shared header: the title bar, then one row of labelled actions with air above and below, then a rule before the list; the first action is the primary one. Add Note is first, as the commoner action, with Add Folder beside it.
- [x] In Scratchpad Activity -> Sidebar Column -> Header; When user click "Add Folder" button, make the Add Note form in the same section with the Add Folder and Add Note button, but hide those 2 button. When after user rename or cancel, hide the form and show back the 2 buttons. Make a horizontal separator between with Add Folder/Add Note button and the Search Note with top/bottom margin/padding.
  - The form takes the buttons' band (`form::sidebar_action_row`): a name field, Create and Cancel; Enter creates and Escape cancels. The folder is named **before** it exists, so a Cancel leaves nothing behind - the old flow made a "New Folder" first and renamed it. Create was exercised in the running app; Cancel was not, because the machine was in use and the keystrokes went to another window. It is the same `done` branch with nothing to create.
- [x] In Scratchpad Activity -> Sidebar Column; Color the pin icon for Pinned Noted (prefer red).
  - Already done by CR-0041: the pin is red by default and its colour is a setting (Appearance -> Scratchpad -> Pin Colour).
- [x] In Editor Activity -> Sidebar Column -> Header; Add label for "Add Project Folder" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add Project Folder" and Project Folder Saved List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [x] In Editor Activity -> Sidebar Column -> Header; Add label "Add Project Folder" for add project folder button.
- [x] In Git Activity -> Sidebar Column -> Header; Add label for "Add Repository" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add Repository" and Repository Saved List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [x] In Git Activity -> Sidebar Column -> Header; Add label "Add Repository" for add repository button.
- [x] In SSH Activity -> Sidebar Column -> Header; Add label for "Add Host" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add Host" and SSH Saved Connection List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [x] In Container Activity -> Sidebar Column -> Header; Add top/bottom margin/padding between Header separator and "Add Container" button. Add horizontal separator between the "Add Container" and Container Saved Connection List Section (with top/bottom margin/padding) and remove the Second "Containers" subtitle.
  - The "Containers" subtitle was the only second title in any sidebar; every other box's "without the Second 'Containers' title" had nothing to remove. Its string is deleted. The later "Saved Commands and Configs" label is a real section and stays.
- [x] In Database Activity -> Sidebar Column -> Header; Change the header title from "Database Connections" to "Database". Add label for "Add Database" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add Database" and Database Saved Connection List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [x] In NoSQL Activity -> Sidebar Column -> Header; Add label for "Add NoSQL" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add NoSQL" and NoSQL Saved Connection List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [x] In Redis Activity -> Sidebar Column -> Header; Add label for "Add Redis" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add Redis" and Redis Saved Connection List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [x] In S3 Activity -> Sidebar Column -> Header; Add label for "Import" and "Add S3 Profile" on the buttons with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style in same row but add horizontal separator between the buttons and S3 Saved Connection List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [x] In Queue Activity -> Sidebar Column -> Header; Add label for "Add Broker" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between Add Broker Button and Broker Saved List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [x] In Automation Activity -> Sidebar Column -> Header; Add label for "Add Server" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between Add Server Button and Server Saved List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [x] In Tracker Activity -> Sidebar Column -> Header; Add label for "Add Tracker" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between Add Tracker Button and Tracker Saved List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [x] Test: the shared sidebar-header helper draws a labelled Add button and one separator, and no activity emits a second title
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — walk all twelve sidebars — one titled header, labelled Add buttons, no duplicated title; Scratchpad's inline Add Folder form replaces the buttons while open
- [x] Docs: spec · CHANGELOG (no README or CHECKLIST line: no feature or phase moved; no new mistake for GOTCHAS)

### CR-0042 · Queue and Automation sidebars titled after their activity
- **State:** Completed · both sidebars read their activity's name on the v0.122.0 build (Queue over Bench RabbitMQ, Automation over Bench Forgejo CI), 2026-11-06
- **Scope:** The Queue and Automation sidebar headers.
- **Parent:** CR-0025
- **Depends on:** CR-0030
- [x] In Queue Activity -> Queue Sidebar Column -> Header; Change title "Brokers" to "Queue".
- [x] In Automation Activity -> Automation Sidebar Column -> Header; Change title "Servers" to "Automation".
  - Both now use the activity's own name (`tool-queue`, `tool-automation`), as Containers does. The `queue-brokers` and `automation-servers` strings had no other use and are removed; `settings-queue-brokers` and `settings-automation-servers` are different keys and stay.
- [x] Test: covered by the bundled-locale parse test and CR-0030's header test
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the two sidebars read Queue and Automation
- [x] Docs: CHANGELOG
- **Notes:** Filed 2026-11-03 from the owner's message after CR-0030 shipped. The app check ran on 2026-11-06 when the machine was free; the change is two string keys with no logic.

### CR-0043 · Fixed tabs wear their activity's colour, and the Editor's its workspace logo
- **State:** Completed
- **Scope:** `hs_ui::tabs`, the shell's tab views, `tool-editor`, `hs_ui::icons`.
- **Parent:** CR-0025
- [x] Every activity's fixed/permanent tab, except the Editor's, the API Client's and the Browser's, draws its icon in the activity's colour while Settings -> Appearance -> Sidebar -> Coloured Activity Icons is on - and never outlined, even with Outline Activity Icons on.
- [x] The Editor's fixed tab shows the workspace's own logo (Settings -> Workspace -> Logo) rather than a plain folder glyph.
  - The shell loads the logo through `hs_ui::icons::workspace_logo_uri` and hands it to any tab that answers `Tool::tab_wears_workspace_logo()`. The Editor's tab could not have done it alone: a tab that has not been drawn never runs `ui()`, and only the shell has the frame's context to load an image with.
- [x] The logo is read when the workspace changes and when Settings -> Workspace sets or removes it - never per frame. Applies to the API Client, the Editor and the Browser.
  - Added 2026-11-03 at the owner's request. The API Client read the file on every frame its home tab was drawn, and the Browser looked at it on disk every frame; both now take the shell's copy and their own loaders are gone. The cache is keyed by the workspace's folder, so a switch reloads with no call; `hs_ui::icons::reload_workspace_logo` is called on `SettingsEffect::WorkspaceLogoChanged`. A frame in between costs a lock and a path compare.
  - Still reading the logo every frame, outside this item: the title bar's workspace button (`app::ws_logo_image`), Home's sidebar header and the Settings -> Workspace preview.
- [x] Every activity's fixed tab - and a pinned tab that shows its name - sizes itself to its icon and label instead of truncating them.
  - Added 2026-11-03 at the owner's request, after the app check showed "Scra…hpad" and "Cont…ners" at a 46 px tab height: those slots had a fixed 132 px width that did not grow with the text, which CR-0029's height setting makes larger.
- [x] Test: a fixed tab's colour is the rail's, the three exceptions keep the glyph's muted colour, the Editor's home tab gets the logo uri, and the logo is read once until it changes
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — toggle Coloured Activity Icons and watch the fixed tabs follow; set a workspace logo and see it on the Editor's tab
- [x] Docs: spec · CHANGELOG
- **Notes:** Filed 2026-11-03 from the owner's message. Checked in the running app: the Scratchpad, Terminal, Containers, Logs, SSH, Automation and Settings fixed tabs in their rail colours with no ring while Outline was on; the Editor, API Client and Browser tabs wearing the logo, and all three dropping it at once when Settings -> Workspace removed it; at a 46 px tab height, every fixed and pinned tab's name shown whole.

### BUG-0041 · Pinning a note from the sidebar did not put it on the tab strip
- **State:** Completed
- **Scope:** `tool-scratchpad`.
- **Depends on:** CR-0041
- [x] Reproduce: with no note open, Pin to Top on a sidebar row marked the note pinned but no tab appeared until the row was clicked.
- [x] Fix: pinning a note opens it (focusing the tab when one already shows it); unpinning opens nothing.
- [x] Test: `pinning_a_note_from_the_sidebar_opens_it`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — pin a note that is not open; its pinned tab appears
- [x] Docs: CHANGELOG
- **Notes:** Filed 2026-11-03 from the owner's report. Checked in the running app: a note whose tab was closed came back as a pinned tab the moment Pin to Top was chosen on its row. The test needed a `Scratchpad` value, and the tool is only built inside `ToolFactory::create` behind a `Box<dyn Tool>`; the struct literal moved into `Scratchpad::new`, which `create` now calls - no change in what it builds.

### BUG-0042 · The release check's terminal probe can sink the whole self-test
- **State:** Completed ·
- **Scope:** `hyperstack::self_test`.
- [x] Reproduce: release run 494 (v0.115.2) went red in "Verify the published release" - the downloaded exe's `--self-test` printed `ok background job` and nothing more, no `FAILED` line and no summary, so the process ended inside the `pty spawn` check. The same exe passes every check on the desktop, and `scripts/verify-release.ps1 -Tag v0.115.2` verified the release locally.
- [x] Fix: every check's line is flushed as it is printed, so a log always shows the last check reached; the terminal probe runs inside `catch_unwind`, so a panic in it is a `FAILED` line rather than the end of the run; and it gets a second attempt with a longer wait (10 s, then 30 s), since the runner opens the console straight after a release build, as LocalSystem in session 0.
- [x] Test: the retry helper passes when a later attempt succeeds, reports every attempt's reason when none does, and turns a panic into a reason
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: n/a - verified by `--self-test` on the built exe and by the next release's verify step
- [x] Docs: GOTCHAS · CHANGELOG
- **Notes:** Filed 2026-11-04 at the owner's request after run 494. The built exe's `--self-test` passes through the new path; release run 496 (app v0.116.0) ran it in its verify step on the runner and passed, which closed the **App** box.

### BUG-0043 · Deleting a connection in Settings leaves its credentials in the vault
- **State:** Completed ·
- **Scope:** the shell's Delete confirmation for Settings -> Connections (and the four pages that share its form).
- [x] Reproduce: in the running app (release build, scratch data folder) the vault file was 79 bytes empty and 209 bytes after saving an SSH profile with a password; Settings -> Connections -> Delete removed the profile's file and left the vault at 209 bytes, so the password stayed in it with nothing referencing it. Found while doing FEAT-0094.
- [x] Fix: the Delete confirmation goes through `Connections::remove_with_secrets`, as Remove From List does, and says the stored credentials go too; a locked vault refuses with the same toast.
- [x] Test: `hs_core::connections` covers the removal (secret deleted, data folder grepped clean, shared secret kept, locked vault changes nothing); the shell's dialog is not reachable headlessly, so the reproduction above is re-run in the app
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — save an SSH profile with a password, Delete it in Settings, and see the vault file shrink back
- [x] Docs: GOTCHAS · CHANGELOG · docs/06-security.md
- **Notes:** Filed 2026-11-04 at the owner's request. Re-run on the fixed build: 79 bytes empty, 209 with the password, 79 again after Delete, and the dialog now says the credentials go too.

### FEAT-0093 · Add-and-focus: a Settings jump target per kind, and Save opens the new connection
- **State:** Completed ·
- **Scope:** `Services`/`OpenRequest` navigation, the Settings pages and every Add button.
- **Parent:** CR-0025
- **Depends on:** CR-0030
- [x] In Almost all Activity Connection -> Sidebar Column; When User click button Add Activity's New Connection, straight away go to Settings -> Connections / Containers / Queue / Automation / Tracker / Git, with add new empty form to add those and the page will focus to the section. 
- [x] In Editor Activity -> Sidebar Column -> Header; When user click "Add Project Folder" button, straight away go to Settings -> Git -> Add Repository -> Add new repository form. And will focus to that form section. 
- [x] In Editor Activity -> Sidebar Column -> Tab Strip; When user click "New Tab", direct focus on the supposed settings page section - Settings -> Editor -> Project List -> Add Project Detail Form. Then, when it is saved, close the detail page and direct open the new tab in Editor's Activity of the new added project Folder. Please check if other new tab button for other activities connections as well.
- [x] In Git Activity -> Sidebar Column -> Header; When user click "Add Repository" button, straight away go to Settings -> Git -> Add Repository -> Add new repository form. And will focus to that form section. 
- [x] In Git Activity -> Sidebar Column -> Tab Strip; When user click "New Tab" button, direct focus on the supposed settings page section - Settings -> Git -> Repository List -> Add Repository Detail Form. Then, when it is saved, close the detail page and direct open the new tab in Git's Activity of the new added repository. It will also register Project In Editor Activity (Only if from Git Setting -> Repository). Please check if other new tab button for other activities connections as well.
- [x] In Git Activity -> Main Windows Tab Strip -> When user click "New Tab" button; direct focus on the supposed settings page section - Settings -> Git -> Repository List -> Add Repository Detail Form. Then, when it is saved, close the detail page and direct open the new tab in Git's Activity of the new added repository. It will also register Project In Editor Activity (Only if from Git Setting -> Repository). Please check if other new tab button for other activities connections as well.
- [x] In SSH Activity -> Sidebar Column -> Header; When user click "Add Host" button, straight away go to Settings -> Connections -> New Connection -> Add new connection form with Auto Select SSH in Kind dropdown. And will focus to that form section. 
- [x] In Database Activity -> Sidebar Column -> Header; When user click "Add Database" button, straight away go to Settings -> Connections -> New Connection -> Add new connection form with Auto Select DB in Kind dropdown. And will focus to that form section. 
- [x] In NoSQL Activity -> Sidebar Column -> Header; When user click "Add NoSQL" button, straight away go to Settings -> Connections -> New Connection -> Add new connection form with Auto Select MongoDB in Kind dropdown. And will focus to that form section. 
- [x] In Redis Activity -> Sidebar Column -> Header; When user click "Add Redis" button, straight away go to Settings -> Connections -> New Connection -> Add new connection form with Auto Select Redis in Kind dropdown. And will focus to that form section. 
- [x] In S3 Activity -> Sidebar Column -> Header; When user click "Add S3 Profile" button, straight away go to Settings -> Connections -> New Connection -> Add new connection form with Auto Select S3 in Kind dropdown. And will focus to that form section. 
- [x] In Queue Activity -> Sidebar Column -> Header; When user click "Add Broker" button, straight away go to Settings -> Queue -> Brokers, and will focus to the section. 
- [x] In Automation Activity -> Sidebar Column -> Header; When user click "Add Server" button, straight away go to Settings -> Automation -> Server, and will focus to the section. 
- [x] In Tracker Activity -> Sidebar Column -> Header; When user click "Add Tracker" button, straight away go to Settings -> Tracker -> Tracker List, and will focus to the section. 
- [x] In Settings -> Containers -> Add New Container; When User click Save for the detail, save and close the setting form and then open the newly added connections in new tab in respective Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [x] In Settings -> Connections -> Add New Connection; When User click Save for the detail, save and close the setting form and then open the newly added Container in new tab in Containers Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [x] In Settings -> Queue -> Add Broker; When User click Save the detail, save and close the setting form and then open the newly added broker in new tab in Queue Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [x] In Settings -> Automation -> Add Server; When User click Save the detail, save and close the setting form and then open the newly added server in new tab in Automation Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [x] In Settings -> Tracker -> Add Tracker; When User click Save the detail, save and close the setting form and then open the newly added server in new tab in Tracker Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [x] In Settings -> Editor -> Add Project Folder; When User click Save the detail, save and close the setting form and then open the newly added project in new tab in Editor Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [x] In Settings -> Git -> Add Repository; When User click Save the detail, save and close the setting form and then open the newly added project in new tab in Editor Activity. In the same time, It will also create a Project Folder of the repository in Editor Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [x] In Settings -> Git -> Add Credential; When User click Save the detail, save and close the setting form. Close button will close the form.
- **Decisions (owner, 2026-11-04):** the Editor's Add Project Folder opens Settings -> **Editor** -> Add Project (box 2's "Git" read as a copy of box 4); a new repository saved in Settings -> Git opens in the **Git** activity and is registered as an Editor project (boxes 5-6 over box 21); the Containers, Queue, Automation and Tracker pages are wired now, on today's pages, and CR-0040 keeps the routing when it reshapes them. Boxes 15 and 16 swap their activity names; each page's Save opens the new item in that page's own activity.
- [x] Test: a jump request resolves to the right Settings page, section and empty form for each kind, and a Save event carries the new id to its activity
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — from every activity press Add, land on a focused empty form, save it, and see the new connection open in a tab; Close closes the form
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Built on `SettingsState::focus_form`, `settings_ui::page_for_kind`, the `settings:<page>:new` jumps and `Projects::add_root`; tests in `settings_tests.rs` and `hs_core::projects`. Exercised in the running app (release build, scratch data folder): Add from SSH, Database, NoSQL, Redis, S3 lands on a focused empty form of that kind; Containers, Queue, Automation, Tracker land on their kind buttons; Redis and RabbitMQ Save closed the form and opened the tab; Close discarded an SSH draft; the Git and Editor `+` and the Editor sidebar's Add Project Folder (after its folder chooser) landed on the focused form; a saved repository opened in Git and was listed as an Editor project; a saved credential closed its form. Git has one tab strip, so boxes 5 and 6 are the same `+`. The other strips' `+` (API Client, Graphics, Scratchpad) make documents and were left alone. README has nothing on these jumps, so it is unchanged.

### FEAT-0094 · Remove From List: delete a saved profile and its credential from the sidebar
- **State:** Completed ·
- **Scope:** Nine activity sidebars, the connection store and the vault.
- **Parent:** CR-0025
- [x] In SSH Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the Connection profile with credential tied to it and confirmation dialog will appear before.
- [x] In Containers Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Host" menu. It will delete the host profile with credential tied to it and confirmation dialog will appear before.
- [x] In Database Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [x] In NoSQL Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [x] In Redis Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [x] In S3 Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [x] In Queue Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [x] In Automation Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [x] In Tracker Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the tracker profile with credential tied to it and confirmation dialog will appear before.
- [x] Test: removing a profile deletes its `secret:<ulid>` from the vault, the data folder greps clean afterwards, and a cancelled confirmation changes nothing
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — remove a saved profile in SSH and Database, confirm the dialog, and check Settings -> Security -> Credentials no longer lists its credential
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** One menu item in `hs_ui::form::row_menu`, so all nine sidebars got it together; the shell asks (`connection:remove:<id>`) and deletes through `Connections::remove_with_secrets`, which keeps a secret another profile still references and refuses on a locked vault when the profile holds secrets. Tests: `hs_core::connections` (secret deleted, data folder grepped for the value and the id, shared secret kept, locked vault changes nothing) and `hs-ui/tests/row_menu.rs`. Cancel is a dialog answer, checked in the app rather than a test: Cancel on an SSH profile left its file in place. In the app (release build, scratch data folder, vault created): an SSH profile and a Database profile, each with a password, were removed after the dialog; their tabs closed and Settings -> Security -> Credentials listed neither; seeded Containers, NoSQL, Redis, S3, Queue, Automation and Tracker profiles were removed from their own sidebars. Settings' own Delete button still removes only the profile, not its secrets; that is outside this item. CHECKLIST has no row for this batch, and README nothing on sidebar menus.

### CR-0044 · Add and Manage buttons land on the right Settings form or list, and say what they add
- **State:** Completed ·
- **Scope:** dashboard and sidebar buttons of Scratchpad, Editor, Terminal, Git, SSH, Containers, NoSQL, Redis, S3, Queue, Automation and Tracker; the Settings jumps.
- **Depends on:** FEAT-0093
- [x] Scratchpad dashboard: "New Note" reads "Add Note".
- [x] Editor dashboard: Manage Projects goes to Settings -> Editor -> Project List and focuses the section.
- [x] Terminal dashboard: Add CLI goes to Settings -> Terminal -> the new CLI form and focuses it.
- [x] Git dashboard: Add Repository goes to Settings -> Git -> the new repository form and focuses it.
- [x] Git dashboard: Manage Repositories goes to Settings -> Git -> Repositories and focuses the section.
- [x] Git dashboard: Clone From Host goes to Settings -> Git -> the new repository form and focuses it.
- [x] Git dashboard: Git Credentials goes to Settings -> Git -> Git Credentials and focuses the section.
- [x] SSH dashboard: "Add Connection" reads "Add SSH Host".
- [x] SSH sidebar: "Add Host" reads "Add SSH Host".
- [x] Containers dashboard: Add Container goes to Settings -> Containers -> the new container form and focuses it.
- [x] NoSQL dashboard: "Add Connection" reads "Add NoSQL".
- [x] Redis dashboard: "Add Connection" reads "Add Redis".
- [x] S3 dashboard: "Add Connection" reads "Add S3 Profile".
- [x] Queue dashboard: Add Broker goes to Settings -> Queue -> the new broker form and focuses it.
- [x] Queue sidebar: Add Broker goes to Settings -> Queue -> the new broker form and focuses it.
- [x] Automation sidebar: "Add Server" reads "Add Automation Server".
- [x] Automation dashboard: "Add Server" reads "Add Automation Server".
- [x] Automation dashboard: Add Automation Server goes to Settings -> Automation -> the new server form and focuses it.
- [x] Tracker dashboard: Add Tracker goes to Settings -> Tracker -> the new tracker form and focuses it.
- [x] Tracker sidebar: Add Tracker goes to Settings -> Tracker -> the new tracker form and focuses it.
- [x] Settings -> Containers -> Container List: the Docker, Podman, Kubernetes and Windows Container buttons become one Add Container; its form's Kind lists container kinds only.
- [x] Settings -> Queue -> Brokers: the RabbitMQ, Kafka, Redis Streams, NATS and MQTT buttons become one Add Broker; its form's Kind lists queue kinds only.
- [x] Settings -> Automation -> Servers: the Jenkins, TeamCity, GitHub Actions, GitLab CI, Forgejo Actions and AWX buttons become one Add Automation Server; its form's Kind lists automation kinds only.
- [x] Settings -> Tracker -> Trackers: the Jira, Redmine, GitHub Issues, GitLab Issues, Forgejo Issues and Confluence buttons become one Add Tracker; its form's Kind lists tracker kinds only.
- [x] Test: each kind page's Add opens a focused form of its first kind (and no longer only the kind buttons); each Manage jump lands on its section; a kind page's Kind field offers only its own kinds
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — press each button above and land on the named form or section; read each new label
- [x] Docs: spec · CHANGELOG · GOTCHAS
- **Notes:** Filed 2026-11-04 from the owner's list. A kind page holds one form per kind, so "the new broker form" opens the page's first kind (Docker, RabbitMQ, Jenkins, Jira) with Name focused, and the Kind field switches it; this replaces FEAT-0093's landing on the kind buttons, and the Automation sidebar's Add follows the same route. The last four boxes came in a second list the same day; they are also CR-0040's fourth box (v0.7.7), done here because they build on this item's first-kind draft. Switching Kind on a new profile takes the new kind's defaults. In the app (release build, scratch data folder): every relabelled button read as listed; each routed button landed on its form (Name focused) or section; Settings -> Queue showed one Add Broker, its Kind listed only the five queue kinds, and switching to Kafka gave Kafka's defaults and the name New Kafka. The other three pages' Kind lists are covered by the test. CHECKLIST has no row for this batch.

### BUG-0038 · Editor -> Source Control -> Refresh Status does nothing
- **State:** Completed ·
- **Scope:** The Editor work pane's Source Control tab.
- **Parent:** CR-0025
- [x] In Editor Activity -> Work Pane -> Folder Sidebar -> Source Control Tab; Refresh Status button not working. Not reproduced on app v0.116.0: the button works.
- [x] Test: n/a - nothing to reproduce; the button already re-runs `git status` and replaces the entries
- [x] Gate: n/a - no code change
- [x] App: `--data-dir ./tmp-data` — edit a file in a git project, press Refresh Status, and watch the list change
- [x] Docs: n/a - no behaviour changed
- **Notes:** Closed as works on the owner's decision (2026-11-04). The button is the ↻ at the right of the repository header (tooltip "Refresh status"). On v0.116.0, with a subfolder of a repository open as the project, a file created outside that subfolder (which the file watcher does not see) did not appear until one click on it, after which Source Control listed it and the count went from 2 to 3. It gives no sign of working when the watcher has already kept the list current, and it sits against the column's resize edge; a spinner, a toast or a labelled button were offered and declined.

### CR-0035 · Editor work pane and Source Control tab layout
- **State:** Completed ·
- **Scope:** The Editor work pane: folder sidebar, toolbar and Source Control tab.
- **Parent:** CR-0025
- **Depends on:** CR-0027, FEAT-0084
- [x] In Editor Activity -> Work Pane -> Folder Sidebar -> Header; Remove Open folder as another project and close this project buttons. Add button with action before "Add File", named "Add Folder", where user can add folder inside the project directory
- [x] In Editor Activity -> Work Pane -> Folder Sidebar; When resize the sidebar column, make the header title (Project) and buttons (New Folder, New File, Refresh), responsive.
- [x] In Editor Activity -> Work Pane -> Toolbar Strip; Remove Open Folder button and Open File button.
- [x] In Editor Activity -> Work Pane -> Folder Sidebar -> Source Control Tab; When the project folder is not a git, do not show the Error "git status: fatal: detected dubious ownership...To add an exception for this directory, call:git config --global --add safe.directory D:/WORKSPACES/DOTNET/flex-taskbar" Show proper error like (Not a Git repository). And disable the "Show this repository History" unless it is a git project folder.
- [x] In Editor Activity -> Work Pane -> Folder Sidebar -> Source Control Tab; Fix the header icon (folder icon for folder name, move the branch icon left beside the branch name). 
- [x] In Editor Activity -> Work Pane -> Folder Sidebar -> Source Control Tab; Please have a proper top/bottom margin/padding between sections: Toolbars (Fetch, Pull, Push, Stashes, Branches, Tag), Commit Message Text Area, Prefix Button, Commit Group (Commit button, Commit and Push button , Ammend the Last Commit checkbox).
- [x] In Editor Activity -> Work Pane; When user open file to edit, make the tab for the file (inner tab), follow the colored material icon (If material Icon enabled in settings) or generic icon on the file list column.  
- [x] Test: a non-git folder yields the "Not a Git repository" state with history disabled instead of a raw git error
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — open a non-git folder and a git one; add a folder from the sidebar header; narrow the sidebar and watch the header stay readable
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** The new header button reuses the tree menu's names (New Folder, New File, Refresh), the names box 2 uses, rather than box 1's "Add Folder". Responsive means labelled while the column has room, icons with tooltips when not, and the title truncating last (`column_header_buttons_fit` measures the translated labels). Open Folder and Open File stay palette commands. A folder in no repository already had a row; it now wears the shared header with History disabled. The "dubious ownership" case is mapped by `status_problem` and covered by a unit test; it could not be staged in the app here. In the app (release build, scratch data folder): a non-git project showed one row, "Not a Git repository.", with History disabled; a git one showed the folder icon by its name and the branch icon by "master", with the four sections evenly spaced; New Folder from the header created the folder on disk; at the column's widest the buttons carried labels, at its narrowest icons only and "Pr…"; main.rs's tab wore the Rust icon the file list shows. CHECKLIST has no row for this batch.

### CR-0036 · Multi-coloured product icons for every connection kind
- **State:** Completed ·
- **Scope:** `hs_ui::icons` and every list, tab and header that draws a connection icon.
- **Parent:** CR-0025
- [x] In All Activities -> Every connection icon, use multi-colored icon, same with the actual product icon color. It will reflect to all component that use the icon (Connection lists, tabs, etc)
- [x] In Git Activity -> Work Pane Header -> Repostory Title Icon; Use the icon that been save in the Git Repository Settings, same like the repository list icon and the tab icon for the repository. Don't use generic icon.
- [x] In Terminal -> Shells and CLI, and SSH -> Hosts: use each item's own logo and colours (shells, CLI tools by command, the host's operating system); macOS takes a rainbow-striped Apple instead of the black one. (Owner, 2026-11-04.)
- [x] In Editor: projects wear their language's logo in its colours (Java, PHP, Rust, .NET/C#, Python, ...). (Owner, 2026-11-04.)
- [x] Test: each connection kind resolves to its product icon, and the same icon is returned for the sidebar, the tab and the work-pane header
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — every connection list, tab and header shows the product's own colours in both themes
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Logos are Devicon v2.18.0 (MIT), chosen by the owner, vendored in `assets/icons/devicon/` (README there lists every mapping); `apple-rainbow.svg` is ours, derived from Devicon's Apple; Rust, Lua and Markdown use the Material files already in the repo. Kinds without a logo keep their glyph in the brand colour (owner's choice); black logos (GitHub, Kafka, MySQL, MariaDB) a glyph in the text colour, found by measuring fills and confirmed on screen. Valkey, WebDAV, Nexus and cmd have neither and are unchanged. An SSH tab shows the generic glyph until the host has been connected once, as before: the OS is learnt on connect. In the app (release build, seeded scratch folder, dark and light themes): every sidebar, the Settings lists, dashboard tiles, tabs and header bands; the Editor's file tab followed the file list with Material icons on and off. CHECKLIST has no row for this batch.

### CR-0045 · Start Here's primary button reads on the light theme
- **State:** Completed ·
- **Scope:** `hs_ui::dashboard::actions` and the two dashboards that draw their own Start Here button.
- **Parent:** CR-0025
- [x] In Light Mode, In All Activities -> Dashboard -> Start Here -> Icon and Label inside button should light color.
- [x] Test: on the light theme the primary Start Here label is painted white and an ordinary one keeps the text colour (fails on the old code)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — light theme: each dashboard's Start Here button reads white on purple
- [x] Docs: spec · CHANGELOG
- **Notes:** Filed 2026-11-04 from the owner's request. White, as `form::primary_button` already used; the Scratchpad's Add Note and the Editor's Add Project Folder draw their own Start Here button and got the same colour. Dialog and Settings buttons filled with the accent were not part of this request. Checked in the app on the light theme: Add Project Folder, Add CLI, Add Repository and Add Note all read white on purple.

### CR-0031 · Spacing pass: Redis, S3 and Queue work panes
- **State:** Completed · every pane walked on screen against the bench Redis 7.4 (6381), s3mock (9000) and RabbitMQ 4.4 (15672) in WSL
- **Scope:** The Redis, S3 and Queue work panes.
- **Parent:** CR-0025
- **Depends on:** CR-0027, CR-0028
- [x] In Redis Activity -> Work Pane -> Keys Pane; Please add top/bottom padding/margin between below Header separator and Action Button (Reload, Rename, Set TTL, Delete, Add Member). Make the button proper with top/right/bottom/left padding/margin. 
- [x] In Redis Activity -> Work Pane -> Keys Pane; Please add top/bottom padding/margin between Action Button (Reload, Rename, Set TTL, Delete, Add Member, etc) and Filter row.
- [x] In Redis Activity -> Work Pane -> Keys Pane; Please add top/bottom padding/margin between Filter row and Data Table.
- [x] In Redis Activity -> Work Pane -> Pub/Sub Pane; Please add top/bottom padding/margin between below Header separator and Subscribe row. 
- [x] In Redis Activity -> Work Pane -> Pub/Sub Pane; Please add top/bottom padding/margin between Subscribe row and Publish row. 
- [x] In Redis Activity -> Work Pane -> Pub/Sub Pane; Please add top/bottom padding/margin between publish row and message section. 
- [x] In Redis Activity -> Work Pane -> Streams Pane; Please add top/bottom padding/margin between below Header separator and Stream row. 
- [x] In Redis Activity -> Work Pane -> Tools Pane; Please add top/bottom padding/margin between below Header separator and Toolbars button (Slow Log, Client List, Big Keys, Bulk Ops, etc). 
- [x] In Redis Activity -> Work Pane -> Tools Pane -> Slowlog tab; Please add top/bottom padding/margin between Toolbars button (Slow Log, Client List, Big Keys, Bulk Ops, etc) and Action button (Reload, etc). 
- [x] In Redis Activity -> Work Pane -> Tools Pane -> Slowlog tab; Please add top/bottom padding/margin between Action button (Reload, etc) and slow log message. 
- [x] In Redis Activity -> Work Pane -> Tools Pane -> Client List tab; Please add top/bottom padding/margin between Toolbars button (Slow Log, Client List, Big Keys, Bulk Ops, etc) and Action button (Reload, etc). 
- [x] In Redis Activity -> Work Pane -> Tools Pane -> Client List tab; Please add top/bottom padding/margin between Action button (Reload, etc) and Client list table. Make that table proper, follow other activity's connections standard. 
- [x] In Redis Activity -> Work Pane -> Tools Pane -> Big Keys tab; Please add top/bottom padding/margin between below Toolbars button (Slow Log, Client List, Big Keys, Bulk Ops, etc) and Pattern row (Find Big Keys, etc). 
- [x] In Redis Activity -> Work Pane -> Tools Pane -> Big Keys tab; Please add top/bottom padding/margin between Pattern row (Find Big Keys, etc) and big keys table. Make that table proper, follow other activity's connections standard. 
- [x] In Redis Activity -> Work Pane -> Tools Pane -> Bulk Ops tab; Please add top/bottom padding/margin between Toolbars button (Slow Log, Client List, Big Keys, Bulk Ops, etc) and Action button (Delete, Expire, Persist, Rename Prefix, etc). 
- [x] In Redis Activity -> Work Pane -> Tools Pane -> Bulk Ops tab; Please add top/bottom padding/margin between Action button (Delete, Expire, Persist, Rename Prefix, etc) and Pattern Row (Pattern, Preview, etc). 
- [x] In Redis Activity -> Work Pane -> Tools Pane -> Bulk Ops tab; Please add top/bottom padding/margin between Pattern Row (Pattern, Preview, etc) and Preview Result. 
- [x] In Redis Activity -> Work Pane -> Tools Pane -> Bulk Ops tab; Please add top/bottom padding/margin between Preview Result and Action button (Apply). 
- [x] In Redis Activity -> Work Pane -> Please add top/bottom padding/margin between Toolbars bottom header and Database drop down row.
- [x] In Redis Activity -> Work Pane -> Please add top/bottom padding/margin between Database drop down row and Filter key Row (Filter filed text, Scan button, New Key button).
- [x] In Redis Activity -> Work Pane -> Please fix Filter key Row (Filter filed text, Scan button, New Key button), where the New key button is far right near the apps windows. Please add right margin/padding for that button and apps windows border. 
- [x] In Redis Activity -> Work Pane -> Please fix Filter key Row (Filter filed text, Scan button, New Key button) alignment by vertical align the input text and those 2 buttons.
- [x] In Redis Activity -> Work Pane -> Please fix Filter key Row button (Scan button, New Key button) by Adding text/label next to the button icon.
- [x] In S3 Activity -> Work Pane -> Data Column Pane; Please add top/bottom padding/margin between separator below Bucket Name Breadcrumb and Filter row. 
- [x] In S3 Activity -> Work Pane -> Data Column Pane; Please add top/bottom padding/margin between Filter row and Data Table. 
- [x] In S3 Activity -> Work Pane -> Data Column Pane; Please add top/bottom padding/margin between Data Table and Bucket Status (e.g. 2 items loaded). 
- [x] In Queue Activity -> Work Pane -> Queue/Topic List Pane; Please add top/bottom padding/margin between Queue Title header and filter row.
- [x] In Queue Activity -> Work Pane -> Queue/Topic List Pane; Please add top/bottom padding/margin between filter row and Data table.
- [x] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Tool row (Messages, Publish, Consumer, Bindings, etc) and Topic Name.
- [x] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Topic Name and Message Queue.
- [x] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Message Queue and count row.
- [x] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between count row and Peek Table.
- [x] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Peek Table and Exchange Section.
- [x] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Exchange Section and Payload.
- [x] In Queue Activity -> Work Pane -> Message Pane -> Exchange Section; Please use Sentence case for Exchange header title. Use a proper table view.
- [x] In Queue Activity -> Work Pane -> Message Pane -> Exchange Section -> Payload; Please add card/frame to put payload message inside.
- [x] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Tool row (Messages, Publish, Consumer, Bindings, etc) and Topic Name.
- [x] In Queue Activity -> Work Pane -> Publish Pane; Please add top/bottom padding/margin between Topic Name and Exchange/Routing Key Section.
- [x] In Queue Activity -> Work Pane -> Publish Pane; Please add top/bottom padding/margin between Exchange/Routing Key Section and Header Section.
- [x] In Queue Activity -> Work Pane -> Publish Pane; Please add top/bottom padding/margin between Header Section and Payload Section.
- [x] In Queue Activity -> Work Pane -> Consumers Pane; Please add top/bottom padding/margin between Topic Name and Action button row (Load).
- [x] In Queue Activity -> Work Pane -> Consumers Pane; Please add top/bottom padding/margin between Action button row (Load) and Consumer data detail.
- [x] In Queue Activity -> Work Pane -> Bindings Pane; Please add top/bottom padding/margin between Topic Name and Exchange/Routing Key Table. Use a proper table view.
- [x] Test: no assertion beyond the gate: this is layout, verified on screen (the existing hs-ui, tool-redis and tool-queue tests still pass)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test (debug exe)
- [x] App: `--data-dir ./tmp-data` — walk every pane in Redis, S3 and Queue — sections are separated, buttons are padded, the Redis filter row is aligned and clear of the window edge
- [x] Docs: spec (redis.md, queue.md, s3.md addenda) · CHANGELOG; README, CHECKLIST and GOTCHAS had nothing to change
- **Notes:** one shared gap, `hs_ui::form::PANE_GAP` (8 px), carries every spacing box; every `DataTable` now leaves that gap under its filter row, which the Database, Logs and other activities inherit. Redis Client List and Big Keys and the Queue Bindings pane became `DataTable`s; Kill Client acts on the selected row. Verified 2026-11-05 on screen; MQTT, Kafka, NATS and Redis Streams brokers were not up, so their Publish and Consumers variants are code-reviewed only.

### CR-0032 · Spacing pass: Database result and message panels
- **State:** Completed · verified on screen against the bench SQLite file with a two-statement script
- **Scope:** The Database work pane's result and message panels.
- **Parent:** CR-0025
- **Depends on:** CR-0027, CR-0028
- [x] In Database Activity -> Work Pane -> Result And Message Section Header; Add separator between result button(s) and message button with proper left/right padding/margin.
- [x] In Database Activity -> Work Pane -> Result Section Header Row; Please add left margin/padding on the first result button. Its too near the pane border.
- [x] In Database Activity -> Work Pane -> Result Section Header -> Message Panel; Please insert the all the result lists : message query(1. SELECT * FROM urpv4.bank) and message result (e.g. 27 rows · 5 ms) inside a some card or frame to look professional and tidy.
- [x] In Database Activity -> Work Pane -> Result Section Header -> Message Panel; Please add left margin/padding on the message query(1. SELECT * FROM urpv4.bank) and message result (e.g. 27 rows · 5 ms). Its too near the pane border.
- [x] In Database Activity -> Work Pane -> Result Section Header -> Message Panel; Please add separator between result message query(1. SELECT * FROM urpv4.bank) and message result (e.g. 27 rows · 5 ms) with top/bottom margin/padding on the separator.
- [x] In Database Activity -> Work Pane -> Result Section Header -> Result Panel -> Bottom Toolbar; Please add top/bottom padding/margin between the result datatable and toolbar below.
- [x] In Database Activity -> Work Pane -> Result Section Header -> Result Panel -> Bottom Toolbar; Please add left margin/padding on the result stat (e.g. 27 rows · Editable (F2)) and right margin/padding on the Export button.
- [x] Test: no assertion beyond the gate: this is layout, verified on screen
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — run a multi-statement query and read the result and message panels — cards, separators and padding on both
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** the result strip, the Messages cards and the footer gaps are `hs_ui::form::PANE_GAP`; verified 2026-11-05 in the app (Result 1 · 27 rows, Result 2 · 5 rows, Messages).

### CR-0033 · Spacing pass: Automation, Tracker and Logs work panes
- **State:** Completed · Automation against the Forgejo instance (anonymous), Tracker against the bench Redmine 6.1, Logs against a 1 500-line file
- **Scope:** The Automation, Tracker and Logs work panes, including the Highlight Rules popup.
- **Parent:** CR-0025
- **Depends on:** CR-0027, CR-0028
- [x] In Automation Activity -> Work Pane -> Job List Pane; Please add top/bottom padding/margin between Job Title header and filter row.
- [x] In Automation Activity -> Work Pane -> Job List Pane; Please add top/bottom padding/margin between filter row and job list data table.
- [x] In Automation Activity -> Work Pane -> Job Activity Pane -> Runs Tab; Please add top/bottom padding/margin between Action button (Runs, Log, Run, Queue, etc) and job title.
- [x] In Automation Activity -> Work Pane -> Job Activity Pane -> Runs Tab; Please add top/bottom padding/margin between job title and Job Activity data table.
- [x] In Automation Activity -> Work Pane -> Job Activity Pane -> Runs Tab; Please add top/bottom padding/margin between Job Activity data table and Job Status (e.g. #4 · Running. Double-click a run to read its log.).
- [x] In Automation Activity -> Work Pane -> Job Activity Pane -> Log Tab; Please add top/bottom padding/margin between job title and job status (e.g. #5 · Success).
- [x] In Automation Activity -> Work Pane -> Job Activity Pane -> Log Tab; Please add top/bottom padding/margin between job status (e.g. #5 · Success) and job log text area.
- [x] In Tracker Activity -> Work Pane -> Ticket List Pane; Please add top/bottom padding/margin between below Header separator and query row (Query text input, Run This QUery, Saved Queries,etc). 
- [x] In Tracker Activity -> Work Pane -> Ticket List Pane; Please add top/bottom padding/margin between query row (Query text input, Run This Query, Saved Queries,etc) and Item count (e.g. Items 3 of 3).
- [x] In Tracker Activity -> Work Pane -> Ticket List Pane; Please add top/bottom padding/margin between Item count (e.g. Items 3 of 3) and filter row.
- [x] In Tracker Activity -> Work Pane -> Ticket List Pane; Please add top/bottom padding/margin between filter row and Ticket data table.
- [x] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add horizontal separator line below Ticket details header.
- [x] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add top/bottom padding/margin between below Ticket details separator and ticket detail table (e.g. Project    Demo, Type     Bug, etc). 
- [x] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add horizontal separator line below Ticket details table.
- [x] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add top/bottom padding/margin between below Ticket details table separator and Ticket Descriptions (e.g. The disk is full.Rotate logs daily.. , etc).
- [x] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add horizontal separator line below Ticket Descriptions.
- [x] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add top/bottom padding/margin between below Ticket Descriptions separator and Ticket Comment List.
- [x] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add horizontal separator line below Ticket Comment List.
- [x] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add top/bottom padding/margin between below Ticket Comment List separator and Comment Form.
- [x] In Tracker Activity -> Work Pane -> Edit Ticket Pane; Please add top/bottom padding/margin between due date row and description form.
- [x] In Tracker Activity -> Work Pane -> New Item Ticket Pane; Please add top/bottom padding/margin between summary row and description form.
- [x] In Logs Activity -> Work Pane -> Logs Pane; Resize the Record column to most suitable size, because its too wide. auto resize it to match value in it.
- [x] In Logs Activity -> Work Pane -> Logs Pane; Please add top/bottom padding/margin between log record list and Record Status (e.g. Record 9, Copy, Bookmark, Open In Editor, etc).
- [x] In Logs Activity -> Work Pane -> Logs Pane; Please add top/bottom padding/margin between Record Status (e.g. Record 9, Copy, Bookmark, Open In Editor, etc) and Log detail text area.
- [x] In Logs Activity -> Work Pane -> Highlight Rules Popup; Please make this professional, tidy and structured. It look messy (vertical middle align, short regex field, padding/margin between checkbox and input field, etc).
- [x] Test: the Logs record column sizes to its content rather than to the pane
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — walk the Automation job panes, the Tracker ticket detail and the Logs pane and popup
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `tool_logs::ui::line_column_width` sizes the record column (tested); the Highlight Rules popup is a grid with a header row and the pattern field forced to 300 px with `add_sized` (a grid cell shrank a `desired_width` field to its text). The Automation run status line was confirmed in code and sits under a runs table that filled the pane on screen.

### CR-0034 · Container and SSH work pane layout
- **State:** Completed · Docker in WSL through Scan Local, the bench SSH container for the cards
- **Scope:** The Container work panes (Docker, Kubernetes, Podman, Windows) and the SSH shell pane.
- **Parent:** CR-0025
- **Depends on:** CR-0027
- [x] In SSH Activity -> Shell Pane; Make Tunnel Card and Monitor Card have some color to differentiate with the shell terminal.
- [x] In SSH Activity -> Shell Pane -> Monitor Card; Add proper icon to the Items (Cpu, Load, Memory, Disk, Up and Top) and color both the icon and lavel with proper color
- [x] In Container Activity -> Docker Work Pane -> Docker Summaries (Running, Stopped, Images, Volumes, Network), Follow the Dashboard summary format (rounded box, icon with lable below, horizontal align center, vertical align middle, same max box size, etc). Apply these in kubernetes, podman, windows container too (Where applicable).
- [x] In Container Activity -> Work Pane -> Container List -> Select a container -> Below Detail Panel; Make a proper button for Ports (Open In Browser activity, etc) with top/right/bottom/left padding/margin. 
- [x] In Container Activity -> Work Pane -> Container List -> Select a container -> Below Detail Panel; Make a proper button for Action (Logs, Shell, Restart, Stop, Kill, Remove) with top/right/bottom/left padding/margin and add proper icon for each button if none. Please check other activities that have same characteristic of these button and apply to it also.
- [x] In Container Activity -> Sidebar Panel -> Saved Command And Config Section; Add a sample for saved command and config, so that user can reference it later and can delete it later.
- [x] Test: the Docker summary row uses the shared `hs_ui::dashboard` summary box
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — Docker summaries match the dashboard language; port and action buttons are real buttons; SSH tunnel and monitor cards stand out from the terminal
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `docker_summary_stats` + a unit test hold the summary row to `hs_ui::dashboard::stats`; `verb_icon` gives every verb its glyph; `configs::seed_samples` writes the two samples only while the folder does not exist; Kubernetes, Podman and Windows containers have no summary row of their own (the cluster pane has its own facts table), so nothing there changed.

### CR-0039 · Spacing pass: NoSQL work pane
- **State:** Completed · verified against the bench MongoDB 7 (shop.orders): Find, the editor, Indexes
- **Scope:** The NoSQL work pane: Find, Aggregate, Index and their detail panels.
- **Parent:** CR-0025
- **Depends on:** CR-0027, CR-0028
- [x] In NoSQL Activity -> Work Pane -> Find Pane; Please add top/bottom padding/margin between below Header separator and Search Criteria (Filter, Project, Sort).
- [x] In NoSQL Activity -> Work Pane -> Find Pane; Please add top/bottom padding/margin between Search Criteria (Filter, Project, Sort) and buttons (Run, Count, New Document, Delete Matching, etc).
- [x] In NoSQL Activity -> Work Pane -> Aggregate Pane; Please add top/bottom padding/margin between below Header separator and Json Array Label and Text Area.
- [x] In NoSQL Activity -> Work Pane -> Aggregate Pane; Please add top/bottom padding/margin between Json Array Text Area and buttons (Run, etc).
- [x] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane; Please add top/bottom padding/margin between buttons (Run, Count, New Document, Delete Matching, etc) and Result format (Table, Tree, Json, etc).
- [x] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane; Please add top/bottom padding/margin between Result format (Table, Tree, Json, etc) and Result table.
- [x] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Detail Panel Below; Please add top/bottom padding/margin between id row and data view text area. 
- [x] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Detail Panel Below; Please add top/bottom padding/margin between data view text area and action button below (Save, Delete, Cancel) 
- [x] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Detail Panel Below; Please make a proper button for Action Button (Save, Delete, Cancel) with top/right/bottom/left padding/margin and add proper icon for each button if none. 
- [x] In NoSQL Activity -> Work Pane -> Index Pane -> Detail Panel Below; Please add top/bottom padding/margin between below Header separator and Table (Name, Definition, Unique). Please make it as a proper table with header, column and row line.
- [x] Test: no assertion beyond the gate: this is layout, verified on screen
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — walk the Find, Aggregate and Index panes and their detail panels
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** the Index pane is a `DataTable`; Aggregate is Mongo-only and was checked in code (its gaps are the same calls as Find's).

### FEAT-0095 · Server-populated fields: Tracker and Automation
- **State:** Completed · Tracker against the bench Redmine 6.1 (anonymous: the member list is readable; a save needs a key and stays the owner's check); Automation against the Forgejo instance (the ref list; no run was dispatched)
- **Scope:** The Tracker edit, new-item and change-status panes; the Automation Run tab.
- **Parent:** CR-0025
- **Depends on:** CR-0026
- [x] In Automation Activity -> Work Pane -> Job Activity Pane -> Run Tab; Make the Ref Selection as a drop down menu where it will populate branch from the automation server. Apply this to all activities that a text field but the value can be auto populate making it a drop down menu.
- [x] In Automation Activity -> Work Pane -> Job Activity Pane -> Log Tab -> Job Log text area; Please pretify the log in proper format (indent, blank line, etc) for user to know and differentiate each step of the runs.
- [x] In Tracker Activity -> Work Pane -> Edit Ticket Pane; Please make Assignee text field and Priority text field to dropdown menu where the data auto populate from the server.
- [x] In Tracker Activity -> Work Pane -> Edit Ticket Pane; Please make Due Date text field to date calendar select menu where the date format will auto format follow server date format when selecting date.
- [x] In Tracker Activity -> Work Pane -> New Item Ticket Pane; Please make Type dropdown list and Priority drop down list auto populate from the server.
- [x] In Tracker Activity -> Work Pane -> Change Status Ticket Pane; Please make a proper UI UX on this to look professiona, tidy and structured.
- [x] Test: a populated dropdown falls back to a free-text field when the server cannot be reached, and the date field formats to the server's format
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — edit a Redmine ticket — Assignee, Priority and Type come from the server, Due Date opens a calendar; run a Forgejo job and pick a ref from the list
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `CreateForm.assignees` (Redmine memberships, Jira assignable users), `hs_ui::form::pick_field` for Assignee, Priority and the ref, `hs_ui::datefield::date_field` (own calendar, no new dependency; every tracker takes `YYYY-MM-DD`), `ParamKind::Pick` + `github::ref_param` for the branches, `tool_automation::logfmt` for the log. New Item's Type and Priority were already the server's lists. Jira and GitHub are code-reviewed only (no server here); Forgejo's API has no run log, so the laid-out log is held by the formatter's tests (GitHub- and Jenkins-shaped samples).

### FEAT-0096 · Database: results retained per query tab, and double-click a cell to edit
- **State:** Completed · two tabs on the bench SQLite file, switched between; a cell double-clicked and the Edit Cell dialog opened
- **Scope:** The Database work pane's query tabs and result data table.
- **Parent:** CR-0025
- [x] In Database Activity -> Work Pane -> Query Tab; Every query tab persist its own results data. Results/Multi-Result per Query Tab. If user click other tab, it can view the result/multi-result last run by the query tab.
- [x] In Database Activity -> Work Pane -> Result Data Table; When user double click on the cell, show edit popup for user edit the value in that cell, same like user right click menu > "Edit Cell".
- [x] Test: switching query tabs restores each tab's own result set and messages rather than the last run's
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — run different queries in three tabs, switch between them, then double-click a cell and edit it (synthetic clicks never form double-clicks, so this box needs the app)
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `Stash` per query tab uid + `switch_stash` (tested); the live results follow a running tab until it ends; double-click opens Edit Cell on an editable result and the viewer otherwise.

### FEAT-0097 · S3: a collapse/expand folder hierarchy column
- **State:** Completed · the bench s3mock bucket with `docs/{api,guides}` and `src/lib` prefixes; expand, navigate, Refresh
- **Scope:** The S3 work pane's folder column and breadcrumb.
- **Parent:** CR-0025
- **Depends on:** FEAT-0084
- [x] In S3 Activity -> All Connection -> Folder List Column; Show folder as hierarchy navigation style (collapse/expand), instead of current navigation style. Don't display Up button on the folder hierarchy. User can use Up button on the toolbar.
- [x] In S3 Activity -> Work Pane -> Data Column Pane; Please add icon for child breadcrumb (e.g. hs-raw > docs). Use colored material icon.
- [x] Test: the hierarchy builds lazily from the delimiter-listed prefixes and keeps expansion state across a refresh
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — browse a bucket with nested prefixes — expand and collapse in place, no Up button in the tree, coloured breadcrumb icons
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `tool_s3::tree::FolderTree` + `hs_ui::tree::show`; children learned from every delimiter listing, lazy loads for an opened caret and always for the root, `forget()` on Refresh keeps the widget's expansion; the breadcrumb's folders use `fileicons::for_folder` when Material icons are on. First build left the tree empty after Refresh (root never re-listed) - fixed, in GOTCHAS.

### FEAT-0098 · SSH: name a tunnel when creating it
- **State:** Completed · two named forwards through the bench SSH container, told apart in the list and in Kept Forwards; the profile file carries both names
- **Scope:** The SSH sidebar's Tunnels section and the stored host profile.
- **Parent:** CR-0025
- [x] In SSH Activity -> Sidebar Column -> Tunnels; When Create Tunnel, allow naming it, so that user can easy to references it in tunnel list. In tunnel list, 
- [x] Test: a named tunnel round-trips through the workspace file and the name is what the list shows
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — create two tunnels on one host with names and tell them apart in the list
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `ForwardSpec.name`, `SavedForward.name` (written only when set; round-trip tested), `tunnel_title` leads with the name.

### CR-0037 · Redis key list sorted folders first
- **State:** Completed · the bench Redis with `cart:*`, `user:*`, `alpha`, `zeta` read in order, inside a group too
- **Scope:** The Redis work pane's key list column.
- **Parent:** CR-0025
- [x] In Redis Activity -> Work Pane -> Key List Column; Sort the key list by parent folder first, key without parent, then by name for both.
- [x] Test: the sort puts parent folders first, then keys without a parent, each group by name
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — scan a database with mixed foldered and bare keys and read the order
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** one stable partition in `KeyTree::walk` (tested), no per-frame sort.

### CR-0038 · Settings polish, and the UI standard written down
- **State:** Completed · Credentials card at page width with no Key column (no SSH key on file); Oracle's hint under the driver row; the Home toolbar names toggled on
- **Scope:** Settings -> Security, Settings -> Appearance, the database connection form, `.claude/rules/ui.md` and `docs/04-ui-design.md`.
- **Parent:** CR-0025
- **Depends on:** CR-0026, CR-0027, CR-0028, CR-0029, CR-0030
- [x] In Settings -> Security -> Credentials; Make the Credentials Card wider, so that user need less horizontal scrolling.
- [x] In Settings -> Security -> Credentials; Remove Key column if not being used.
- [x] In Settings Page -> Database Connections; State the guide and requirement to use oracle client and db2 cli if user select either both in DB Engine.
- [x] Make a Settings -> Appearance -> Activity Rail for Home -> Dashboard Page -> Dashboard Toolbar -> Activity Icon, user can show/hide label of activity (below icon). Make standard width for all the icon with left/right margin/padding if enabled.
- [x] Create standard rules of all above items for agent reference for building next activity, features, function, etc where applicable.
- [x] Test: the bundled locale parses and the Oracle/Db2 requirement text is present for those engines
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — the credentials card needs no horizontal scrolling; the activity rail label toggle takes effect on the Home dashboard
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `form::section_max`, `conn-db-odbc-need-*` hints (locale test), `appearance.home_toolbar_labels` (76 px buttons, 6 px margin; the first build placed a widget inside the painted button and the row stair-stepped - fixed, in GOTCHAS); the standard is in `.claude/rules/ui.md` → Fields, lists and cards, and `docs/04-ui-design.md` → Addendum 2026-11-05 (CR-0038).

---

## v0.7.5 — In-Progress

> **2026-11-06, on the owner's ask to complete the remainder:** FEAT-0092
> (Puppet) is built and verified against recorded PuppetDB answers. Of the
> four items that were In-Progress, each with one open box (the app check),
> three stay blocked by a server that cannot run here: TeamCity's bench
> container came up at http://127.0.0.1:8111 and waited at its licence agreement for the owner
> (FEAT-0079; the owner accepted it later that day and FEAT-0079 is now
> Completed against that server); AWX needs a Kubernetes operator
> (FEAT-0083); Jira and Confluence need licence keys (FEAT-0086,
> FEAT-0091). Every other box of theirs is ticked and their mock-server
> tests pass.

> Phase K (Automation), then phase N (Tracker), every kind the programme
> names (owner, 2026-10-31) except Puppet, deferred to the Backlog the same day. Writes are exercised only against local
> containers; git.zacx.net and GitHub are read only.

### FEAT-0077 · Automation activity shell: crate, rail, dashboard, connections, Settings → Automation
- **State:** Completed · **Spec:** docs/tools/automation.md · **Phase:** K
- **Branch:** feat/depth-k-automation
- [x] `hs_core::service_auth`: credential fields, resolution against the vault and variables, applying to a request
- [x] `tool-automation` crate, the `Driver` trait and its types (`Job`, `Run`, `RunState`, `Param`, `LogChunk`), the `Guarded` wrapper
- [x] registered after Containers under Infra; dashboard home; sidebar of saved servers with reorder
- [x] connection kinds `jenkins`, `teamcity`, `github-actions`, `gitlab-ci`, `forgejo-actions`, `awx` with their fields and credential kinds
- [x] Settings → Automation: kind buttons, list left, form right; refresh interval and log bound
- [x] a tab per server: jobs table, detail with Runs / Log / Parameters / Queue, Run form, guarded Cancel, read-only on a protected environment
- [x] Open in Logs (snapshot), palette commands and shortcuts of spec §9, AI assistant with Explain This Failure
- [x] Test: `service_auth` (round trip, kinds a service cannot use, vault, variables, each scheme on the wire, no secret in Debug), `profile.rs`, `driver.rs` (read-only refuses Run and Cancel before the server sees them, log slicing, durations), `runner.rs` (log bound on a line), `hs_core::runtime::next_coalesced`; `mock.rs::a_read_only_session_refuses_a_run_before_the_server_sees_it`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir` scratch — Jenkins and Forgejo added in Settings → Automation with tokens stored in the vault (data folder grepped: no plaintext) and Test connected; GitHub, GitLab, TeamCity and AWX as profile files; dashboard, sidebar and product icons seen; an empty state ("No runs yet") and a refused connection (TeamCity, nothing listening) seen
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Puppet was dropped from this item on the owner's decision (FEAT-0092, Backlog). The protected-environment path was not driven on screen; it shares `read_only()` and the driver's `Guarded` refusal with the read-only profile flag, which was (Queue, phase L).

### FEAT-0078 · Jenkins
- **State:** Completed · **Spec:** docs/tools/automation.md · **Phase:** K
- **Branch:** feat/depth-k-automation
- **Depends on:** FEAT-0077
- [x] jobs (folders flattened) with last build status, duration, who
- [x] builds of a job; parameters; build / buildWithParameters, following the queue item to its build
- [x] stop a build; the queue; progressive log
- [x] API token as basic; password with a CSRF crumb
- [x] Test: `jenkins.rs` (folders flattened, building is running, parameter kinds, console notes stripped); `live.rs::jenkins_runs_a_parameterised_job_follows_its_log_and_stops_it` against `hs-jenkins` (parameters, trigger, log from its offset, stop, 401 named)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `hs-jenkins` (port 8091): a parameterised build run from the form (qa, FAIL), a 20-step build followed to "Finished: SUCCESS", a 120-step build cancelled after the confirmation ("Build was aborted"), Open in Logs
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Password auth with a crumb is written but only token auth was exercised here. Folders need the cloudbees-folder plugin, which the bench does not have; flattening is unit-tested.

### FEAT-0079 · TeamCity
- **State:** Completed · `hs-teamcity` 2025.07.3 with an agent container, 2026-11-06: Bench / Hello Build listed with its parameter, #1 and #2 run and followed in the Log tab while the agent printed its steps, #3 and #4 cancelled from the app behind the confirmation (TeamCity shows them Cancelled) · **Spec:** docs/tools/automation.md · **Phase:** K
- **Branch:** feat/depth-k-automation
- **Depends on:** FEAT-0077
- [x] build configurations with last build; builds; parameters
- [x] trigger through the build queue with properties; cancel; the queue; the build log
- [x] bearer token or basic
- [x] Test: `teamcity.rs` (build state, dates, trigger user, cancelled); `mock.rs::teamcity_lists_triggers_and_cancels_through_the_build_queue` (the documented endpoints and bodies, against a mock server)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a `hs-teamcity` container: trigger, follow the log, cancel
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** TeamCity's first start stops at a setup wizard before its REST API answers, so no bench server: the App box stays open. The mock answers are written from the REST API reference, not captured from a server.
- **Notes (2026-11-06):** the first-start wizard (data directory, internal database, the licence agreement accepted by the owner, the admin account) was driven in the built-in browser; the project, build configuration, command-line step and agent authorisation went in through the REST API. Credentials and the recipe are in the bench notes outside the repository.

### FEAT-0080 · GitHub Actions
- **State:** Completed · **Spec:** docs/tools/automation.md · **Phase:** K
- **Branch:** feat/depth-k-automation
- **Depends on:** FEAT-0077
- [x] workflows of a repository with the last run; runs; a run's jobs and a job's log
- [x] workflow_dispatch with inputs (offered only for dispatchable workflows); cancel
- [x] GitHub Enterprise through the API URL
- [x] Test: `github.rs` (dispatch inputs from every form of `on:`, states, runs); `live.rs::github_actions_lists_a_public_repositorys_workflows` (BurntSushi/ripgrep); `mock.rs::github_dispatches_with_inputs_and_cancels_a_run`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: BurntSushi/ripgrep read anonymously: seven workflows, a workflow's runs with refs, durations and authors, and the one-line message for logs without a token
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Writes (dispatch, cancel) only against the mock server, as the owner asked; job logs need a token, which this bench has no GitHub token for. GitHub Enterprise is the same code with another API URL and was not run.

### FEAT-0081 · GitLab CI
- **State:** Completed · **Spec:** docs/tools/automation.md · **Phase:** K
- **Branch:** feat/depth-k-automation
- **Depends on:** FEAT-0077
- [x] refs with pipelines as the jobs; pipelines; a pipeline's jobs and a job's trace
- [x] create a pipeline on a ref with variables; cancel
- [x] `PRIVATE-TOKEN` (api-key) or bearer
- [x] Test: `gitlab.rs` (variables parsed per line, pipeline state and user); `live.rs::gitlab_ci_lists_a_public_projects_pipelines` (gitlab-org/cli); `mock.rs::gitlab_creates_a_pipeline_with_variables_and_cancels_it`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: gitlab-org/cli on gitlab.com read anonymously: 76 refs with their last pipelines
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Writes only against the mock server (owner: writes stay local; a GitLab server needs ~4 GB and cannot run here).

### FEAT-0082 · Forgejo Actions
- **State:** Completed · **Spec:** docs/tools/automation.md · **Phase:** K
- **Branch:** feat/depth-k-automation
- **Depends on:** FEAT-0077
- [x] workflows of a repository with their runs; a run's jobs and logs where the API has them
- [x] workflow_dispatch with inputs; cancel where the API has it; a missing endpoint reported as such
- [x] Test: `live.rs::forgejo_dispatches_a_workflow_and_lists_its_runs` against `hs-forgejo` with a host-mode runner (inputs read from the workflow file, dispatch, the run appearing)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `hs-forgejo`: build.yml with its runs, the Run form from the workflow's inputs, dispatch ("as run #3") and the run succeeding, the Queue tab
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Forgejo 11's API has no call to cancel a run or read its log, so neither is offered (spec "What is implemented"); the "cancel" of the App box could not be made.

### FEAT-0083 · AWX / Ansible Automation Platform
- **State:** In-Progress · **Spec:** docs/tools/automation.md · **Phase:** K
- **Branch:** feat/depth-k-automation
- **Depends on:** FEAT-0077
- [x] job templates with the last job; jobs; survey / extra-vars parameters
- [x] launch with extra vars; cancel; stdout
- [x] Test: `awx.rs` (survey questions, extra vars merged from YAML, job state and elapsed); `mock.rs::awx_launches_a_template_with_its_survey_and_cancels_the_job`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — recorded responses only
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** AWX needs a Kubernetes operator and several GB and cannot run here; the App box stays open. The mock answers are written from the API reference, not captured.

---

### FEAT-0085 · Tracker activity shell: crate, rail, dashboard, connections, Settings → Tracker
- **State:** Completed · **Spec:** docs/tools/tracker.md · **Phase:** N
- **Branch:** feat/depth-n-tracker
- [x] `hs_core::service_http` (moved from `tool-automation`), used by both activities
- [x] `tool-tracker` crate, the `Driver` trait and its types, the `Guarded` wrapper, `markup.rs`
- [x] registered after Automation under Infra; dashboard home; sidebar of saved trackers with reorder
- [x] connection kinds `jira`, `redmine`, `github-issues`, `gitlab-issues`, `forgejo-issues`, `confluence` with their fields and credential kinds
- [x] Settings → Tracker: kind buttons, list left, form right; limits
- [x] a tab per tracker: query bar and saved queries, items table with More, item pane (fields, description, comments, links), Edit with a field diff, Transition, Comment, New Item; read-only on a protected environment
- [x] palette commands and shortcuts of spec §9, AI assistant with Summarise, Draft a Reply, Suggest a Status
- [x] product icons for the Tracker kinds (CR-0024)
- [x] Test: driver::tests (Guarded refuses every write on a read-only session, and a write the kind does not offer), ui::tests (protected tab read-only until unlocked; an edit carries only the changed fields; the page tree nests), profile::tests, markup::tests; `tests/mock.rs` a read-only connection sends no write
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — add a tracker of each kind, open it, see an empty state and a connection error
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Seen on screen 2026-10-31 against `D:\Temp\hs-gui-data`: dashboard figures, Settings → Tracker (a Redmine API key typed and kept in the vault, Test Connection, Connect), a tab per tracker, saved queries kept in the profile, a read-only profile disabling every write with the reason. Jira points at a closed port, which is the error state.

### FEAT-0086 · Jira (Cloud v3, Data Center v2)
- **State:** In-Progress · **Spec:** docs/tools/tracker.md · **Phase:** N
- **Branch:** feat/depth-n-tracker
- **Depends on:** FEAT-0085
- [x] JQL search with paging; an issue with fields, comments, links
- [x] ADF ⇄ Markdown (Cloud), wiki markup → Markdown (DC); a description that cannot round-trip edited only after a warning
- [x] transitions; edit fields; comment; create from the project's create metadata
- [x] Test: `tests/mock.rs` Jira Cloud (token paging, ADF read and written back, transitions, edit with only changed fields and the accountId, comment, create metadata, create) and Data Center (offset paging, wiki markup both ways, bearer PAT); markup ADF and wiki tests
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — recorded responses only
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Jira needs a licence key and cannot run here; tested against a mock server only, and the App box stays open for that reason.

### FEAT-0087 · Redmine
- **State:** Completed · **Spec:** docs/tools/tracker.md · **Phase:** N
- **Branch:** feat/depth-n-tracker
- **Depends on:** FEAT-0085
- [x] issues with filters and full text; an issue with journals
- [x] status, edit, note, create; Textile → Markdown
- [x] API key in `X-Redmine-API-Key`
- [x] Test: `tests/live.rs` against `hs-redmine` (redmine:6): create with tracker and priority, filter by subject, allowed statuses, transition, edit (subject, assignee me, due), note, read back, assigned-to-me query, read-only refusal; redmine filter test
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a `hs-redmine` container: filter, read, change status, comment, create
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Redmine 6 writes CommonMark by default; the profile says when a site writes Textile.

### FEAT-0088 · GitHub Issues
- **State:** Completed · **Spec:** docs/tools/tracker.md · **Phase:** N
- **Branch:** feat/depth-n-tracker
- **Depends on:** FEAT-0085
- [x] search with GitHub syntax; an issue with comments; open / close; edit; comment; create
- [x] Test: `tests/live.rs` reads BurntSushi/ripgrep anonymously; `tests/mock.rs` close as not planned, labels, comment, create, the search scoped to the repository and to issues
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a public repository read only (owner: writes stay local)
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Writes tested against recorded responses only.

### FEAT-0089 · GitLab Issues
- **State:** Completed · **Spec:** docs/tools/tracker.md · **Phase:** N
- **Branch:** feat/depth-n-tracker
- **Depends on:** FEAT-0085
- [x] issues with filters and search; notes; open / close; edit; note; create
- [x] Test: `tests/live.rs` reads gitlab-org/cli anonymously with paging; `tests/mock.rs` close, assignee by user id, due date, note, create with labels, PRIVATE-TOKEN
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a public project on gitlab.com read only
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Writes tested against recorded responses only. gitlab.com keeps notes from anonymous callers; the issue opens and says so.

### FEAT-0090 · Forgejo Issues
- **State:** Completed · **Spec:** docs/tools/tracker.md · **Phase:** N
- **Branch:** feat/depth-n-tracker
- **Depends on:** FEAT-0085
- [x] issues with filters and search; comments; open / close; edit; comment; create
- [x] Test: `tests/live.rs` against `hs-forgejo`: create, comment, due date, close, closed search, reopen, text search
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — `hs-forgejo`: list, read, close, comment, create
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### FEAT-0091 · Confluence (Cloud v2, Data Center v1)
- **State:** In-Progress · **Spec:** docs/tools/tracker.md · **Phase:** N
- **Branch:** feat/depth-n-tracker
- **Depends on:** FEAT-0085
- [x] spaces and the page tree; a page in storage format → Markdown
- [x] edit as Markdown with the diff shown, publish back with the version bumped; a page with macros or layouts read-only
- [x] page comments
- [x] Test: `tests/mock.rs` Confluence Cloud (spaces, page tree, storage → Markdown, publish with version + 1, footer comment, CQL search, a macro page refused) and Data Center (ancestors, v1 publish); markup storage tests
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — recorded responses only
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Confluence needs a licence key and cannot run here; tested against a mock server only, and the App box stays open for that reason.

## Backlog

### FEAT-0104 · Database: schema- and database-level DDL from the designer
- **State:** Pending · **Spec:** docs/tools/database.md (Table designer, "Out of the first cut")
- **Source:** split out of FEAT-0103 on the owner's decision of 2026-11-06 ("whether 'alter schema' means more than tables (create / rename / drop a schema or database)")
- **Depends on:** FEAT-0103
- [ ] Plan first: which engines create, rename and drop a schema or database, and how the designer offers it - agreed with the owner before code
- [ ] Test: <what it asserts>
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — <what to click>
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** referenced by FEAT-0103 and `docs/tools/database.md` since 2026-11-06 but never written down as an item; filed 2026-11-06 so the id points at something. Scope beyond the plan box is the owner's to set.

### FEAT-0092 · Puppet (PuppetDB, Puppet Enterprise orchestrator)
- **State:** Completed · built and verified on 2026-11-06 against recorded PuppetDB answers (`hs-puppetdb-fake`): three nodes, their reports and a failed report's log on screen, Run refused without an orchestrator and a deploy with one; gate green
- **Depends on:** FEAT-0077
- [x] nodes with the last report's status and time; a node's reports; a report's log lines
- [x] Run offered only with a Puppet Enterprise orchestrator URL (deploy one node)
- [x] `X-Authentication` RBAC token as api-key
- [x] Test: `puppet.rs` (report state, node's last report, log layout, the percent-encoded query); `mock.rs::puppet_lists_nodes_reads_reports_and_deploys_through_the_orchestrator` (nodes, reports, a report's logs, Run refused without an orchestrator, the deploy body with the environment, noop and the node)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a `hs-puppetdb` container if it fits in memory, otherwise recorded responses
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** deferred out of v0.7.5 on the owner's decision (2026-10-31). Puppet Enterprise is licensed and cannot run here; PuppetDB could run in a container.

---

## v0.7.4 — In-Progress
- **Notes (2026-11-06):** built on the owner's ask to complete v0.7.5. `tool_automation::puppet`: PuppetDB `/pdb/query/v4` nodes, reports and logs; a Puppet Enterprise orchestrator URL in the profile's `project` field makes Run a `deploy` of the node (environment, noop); Cancel is off because a stored report is over; the RBAC token is an api-key in `X-Authentication`. Seen on screen against `hs-puppetdb-fake` (nginx with recorded PuppetDB answers on 127.0.0.1:8082: three nodes - a failed, a noop and a clean run; the deploy answered with job 1234). No PuppetDB or Puppet Enterprise runs here, so the live path is the recorded one.

### BUG-0036 · Saved Git Credentials Not Showing in Settings -> Security -> Credentials
- **State:** Completed ·
- **Scope:** Settings -> Security -> Credentials
- **Branch:** batch/v0.7.4
- [x] Add saved git credentials to the list.
- [x] Add other activities save credential that still not showing in Settings -> Security -> Credentials
- [x] Test: hyperstack credentials::tests::every_saved_credential_is_listed_not_only_database_style_connections - Docker, Kubernetes, WebDAV and Nexus profiles, a Git credential, a container registry and an AI provider each get a row with the vault reference, and ids stay unique across sources (did not compile before the fix)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — displaying all list of connection credentials.
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Seen on screen 2026-10-31: Docker, WebDAV, Git Credential · Forgejo, Container Registry and AI Provider · OpenAI-compatible rows; Reveal on the Git credential showed its token; Edit on the registry wrote a new `secret:` reference to `settings.toml`, and a grep of the data folder found no plaintext. Edit also now works on Valkey, WebDAV, Nexus and container profiles, which it refused before. README and CHECKLIST needed no change.

### CR-0024 · Actual Icon For Queue, Tracker And Automation Activity Connections
- **State:** Completed · 
- **Scope:** Queue Activity, Tracker Activity, Automation Activity -> Connections Icon.
- [x] All connection icon use actual respective product icon if available. Refer on Database connection on how to get it.
- [x] Test: hs-ui icons::tests - every Queue, Automation and Tracker kind maps to a mark of its own, never the container fallback
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a `icons` case per connections.
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Queue and Automation done 2026-10-31 and seen on screen: md-rabbit, md-apache_kafka, dev-nats, dev-jenkins, dev-githubactions, dev-gitlab, dev-forgejo, md-ansible from the bundled Nerd Font (the devicon RabbitMQ and JetBrains glyphs sit under Phosphor's EXCLUDE and BATTERY_PLUS and do not show, so TeamCity has a gear and MQTT keeps the broadcast). Tracker done 2026-10-31 and seen on screen: md-jira (the devicon Jira at U+E75C is under Phosphor), dev-confluence, the GitHub mark, dev-gitlab, dev-forgejo; Redmine has no mark anywhere and takes a bug.

### FEAT-0076 · Scratchpad Dashboard Folder Count And Sidebar Column Folder Icon
- **State:** Completed ·
- **Scope:** Scratchpad Activity -> Dashboard page, Sidebar Column.
- **Branch:** batch/v0.7.4
- [x] Add card in summary for folder count before the Notes count card.
- [x] Make the icon for folder in sidebar column slightly bigger than the notes icon.
- [x] Test: scratchpad::tests::the_dashboard_counts_folders_before_notes and a_folder_glyph_is_bigger_than_a_note_glyph (22 px against a note row's 20 px)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir ./tmp-data` — a card displaying folder count, folder icon bigger than notes icon.
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Seen on screen 2026-10-31: 2 Folders before 3 Notes; the Work and Clients folder glyphs draw larger than the note glyphs. README, CHECKLIST and GOTCHAS needed no change.

### FEAT-0068 · Oracle, DB2 and the other common relational engines
- **State:** In-Progress · **Source:** `mynotes` L1-7 (1 request) · **Spec:** docs/tools/database.md · ADR-030
- **Scope:** Driver, connection form, type mapping and catalog introspection per engine, the way SQL Server / ClickHouse / DuckDB were planned.
- **Branch:** batch/v0.7.4
- [ ] Add Oracle, DB2 and the other common relational databases.
- [x] Add auto refresh schema on the database/table list column to refresh list of databases/table when user create/drop database/table in query.
- [ ] All newly added databases engine can generate DDL and erd diagram too.
- [ ] Test: a `drivers` case per engine, running when its `HS_*` variable names a server
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — connect, browse the catalog, run and cancel a query
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** SQL Server, ClickHouse, DuckDB and Oracle are done and verified;
  Db2 is written but unverified, so the three boxes that cover every engine
  stay open. `tests/engines.rs` passes against SQL Server 2022, ClickHouse
  24.8 and Oracle Free 23.26 (HS_MSSQL, HS_CLICKHOUSE, HS_ORACLE) and DuckDB
  in process: version, browse, a DDL round trip, `table_schema`, the row
  limit and Cancel. Seen on screen 2026-11-01 on the release build: all nine
  engines in the form; DuckDB scripts, ERD and DDL; SQL Server and ClickHouse
  profiles; an Oracle profile made through the form (Service Name, ODBC
  Driver preselected, password `secret:` in the vault, no plaintext in the
  data folder), its schema tree, a script whose CREATE added the table to
  the tree without Refresh, Cancel (ORA-01013), and a guarded DROP. Db2:
  IBM's ODBC driver, v12.1.4 and v11.5.9, failed on this machine for every
  ODBC client including .NET - SQL1042C at login, and with
  `Authentication=SERVER` negative length indicators on every result
  (docs/GOTCHAS.md, Databases). The ODBC session now turns such a driver
  panic into the statement's error instead of losing the session.

---

## v0.7.3 — In-Progress

### FEAT-0067 · Scratchpad folders and subfolders
- **State:** Completed · **Spec:** docs/tools/scratchpad.md · **Phase:** O
- **Branch:** batch/v0.7.3
- [x] folders and subfolders at any depth in the sidebar
- [x] drag to reorder within a level, drop onto a folder to move; pinned notes keep their place
- [x] name clash on move or create gets the existing `name 001` suffix
- [x] folders collapse and remember it; quick-open and search show the path
- [x] notes-on-disk layout gains directories
- [x] Test: 12 new in `tools::scratchpad` — the tree read whole with a
  dot-directory skipped; a note found after a move, keeping its id and
  content; a level holding only its own folders and unpinned notes, unlisted
  rows first; a saved order kept and one from before folders still read; a
  clash taking ` 001` with case ignored; a case-only rename not a clash; a
  folder moving with its notes and never into itself; collapsed folders
  following a rename; folder names cleaned for every platform — plus 2 for
  `hs_core::storage::unique_name`. The 4 existing scratchpad tests unchanged
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: on screen with `scripts/gui-smoke.ps1` against `D:\Temp\hs-gui-data`
  (the harness's data folder, not `./tmp-data`): **Work** and nested
  **Clients** made in the app, a note dragged onto each, `work` beside `Work`
  becoming `work 001` on disk, search and Ctrl+P showing *Work / Clients*,
  a reorder within the top level, collapse and order surviving a restart,
  an edited note saving in place inside its folder with no stray copy, a
  pinned note showing its folder, and the guarded folder delete
  (`docs/screenshots/scratchpad-folders.png`)
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** phase O of docs/10-depth-programme.md. Design calls made without
  a spec to settle them: pinned notes stay in one group at the top whatever
  folder they are in (unpin to move one); ` 001` applies to folder names,
  since notes are stored by id and cannot clash; a search lists matches flat
  with their folder rather than pruning the tree; each folder keeps its own
  `order.json`, folders keyed `/Name` there. Found on the way: Ctrl+P hands a
  choice to the tab that listed it, so `open` now passes a note it does not
  hold back to the shell — otherwise a pick could overwrite another tab's
  unsaved note. Rows in a folder are not given a keyboard path of their own
  beyond the existing list navigation; the reorder widget is mouse-driven
  everywhere in the app.

### FEAT-0066 · Queue activity (RabbitMQ, Kafka, Redis Streams, NATS, MQTT)
- **State:** In-Progress · **Spec:** docs/tools/queue.md · **Phase:** L
- **Branch:** feat/depth-l-queue
- [x] spec written from .claude/templates/spec-tool.md before any code
- [x] ADR for the client crates (`lapin`, `rskafka` — not the C-bound `rdkafka`)
- [ ] Test: through the items it splits into
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — connect to a local broker, peek, publish
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** the umbrella for phase L; the work is `FEAT-0069` to `FEAT-0075`,
  and this item closes when they do. Scope widened on the owner's decision of
  2026-10-31 from RabbitMQ and Kafka to all five kinds the programme names.
  The ADR is ADR-027, and it departs from what this item's box anticipated:
  **no `lapin`**. RabbitMQ is read and driven through its management HTTP API,
  because AMQP cannot list queues or exchanges and the management API would be
  needed for that anyway. Also found while writing it: `rskafka` 0.6 has **no
  consumer-group API**, so lag comes from `kafka-protocol` messages on a small
  client of our own (`FEAT-0072`). Live tail into Logs and the "explain this
  backlog" assistant are open questions 1 and 2 in the spec, not in these
  items.

### FEAT-0069 · Queue activity shell: crate, rail, dashboard, connections, Settings → Queue
- **State:** Completed · **Spec:** docs/tools/queue.md · **Phase:** L
- **Branch:** feat/depth-l-queue
- [x] `tool-queue` crate, the `Driver` trait and its shared types (`Destination`, `Message`, `ConsumerGroup`, `Lag`)
- [x] registered after Logs under Infra; dashboard home; sidebar of saved brokers with reorder
- [x] connection kinds `rabbitmq`, `kafka`, `redis-streams`, `nats`, `mqtt` with their fields
- [x] credentials `password` / `token` / `api-key` per kind as the spec's table says, secrets as `secret:<ulid>`
- [x] Settings → Queue: brokers list left, form right; peek count and poll interval
- [x] a tab per broker: destinations table, detail pane with Messages / Publish / Consumers, protected-environment banner and read-only
- [x] palette commands and shortcuts of spec §9
- [x] Test: `profile.rs` (defaults, round trip, credential kinds per broker, vault locked, Kafka API key, undefined variables, addresses), `driver.rs` (read-only refuses every write before the broker sees it, no default target, peek bounds), `ui.rs` (publish form filled per destination, protected tab read-only, sentence-case toasts); `live.rs::a_read_only_session_refuses_writes_on_a_real_broker`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir` scratch — RabbitMQ added in Settings → Queue with its password stored in the vault and Test → "Connected. RabbitMQ 4.4.0"; the other four as profile files; empty dashboard and Settings states seen; a refused connection shown by Test on a closed port; a read-only profile's Purge disabled
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** the protected-environment path was not driven on screen: the read-only profile flag was, and both go through the same `read_only()` and the driver's `Guarded` refusal (unit-tested).

### FEAT-0070 · RabbitMQ through the management API
- **State:** Completed · **Spec:** docs/tools/queue.md · **Phase:** L
- **Branch:** feat/depth-l-queue
- **Depends on:** FEAT-0069
- [x] queues per vhost with ready, unacked, consumers and rates
- [x] peek by `get` with requeue, the side effect stated in the pane
- [x] publish to an exchange with routing key, headers and properties
- [x] exchanges and bindings
- [x] purge and delete, guarded (typed confirmation for delete)
- [x] a connection test that names a missing management plugin
- [x] Test: `rabbitmq.rs` unit tests (path encoding, rates, missing plugin named); `live.rs::rabbitmq_lists_peeks_publishes_and_purges_a_queue` against `hs-rabbitmq` (list, publish, peek without taking, purge, bindings, delete, 401 named)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir` scratch — `hs-rabbitmq`: queues listed, peek with the requeue note and Ready unchanged, publish (12 → 13, toast "Routed"), bindings, purge of `emails.outbound` after the confirmation
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** Delete's typed confirmation was not driven on screen; delete itself runs in the live test.

### FEAT-0071 · Kafka topics, offsets, peek and publish
- **State:** In-Progress · **Spec:** docs/tools/queue.md · **Phase:** L
- **Branch:** feat/depth-l-queue
- **Depends on:** FEAT-0069
- [x] topics with partitions, earliest and latest offsets (`rskafka`; no replication factor, see Notes)
- [x] peek from an offset, payloads by content type
- [x] publish to a partition or by key, with headers
- [x] purge (delete records below an offset), the Redpanda refusal explained
- [ ] TLS and SASL PLAIN / SCRAM / OAUTHBEARER from the credential
- [x] Test: `kafka.rs` murmur2 against the Java partitioner's values; `live.rs::kafka_lists_peeks_publishes_and_purges_a_topic` against `hs-kafka`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir` scratch — `hs-kafka`: topics with offsets, a keyed publish ("Partition 0, offset 0") and its peek with key and JSON body
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** no replication column: `rskafka` lists partitions only (spec "What is implemented"). SASL verified 2026-11-06 against `hs-kafka-sasl` (Kafka 4.0, SASL_PLAINTEXT on 9094): `live.rs::kafka_connects_with_sasl_plain_and_scram` connects with PLAIN, SCRAM-SHA-256 and SCRAM-SHA-512, lists the topic, reads its groups through the groups client and sees a wrong password refused; two profiles (PLAIN, SCRAM-SHA-256) listed the topic on screen. The box stays open for TLS and OAUTHBEARER: the client verifies through the platform trust store, so a self-signed local broker cannot be used, and no OAuth broker runs here.

### FEAT-0072 · Kafka consumer groups and lag
- **State:** In-Progress · **Spec:** docs/tools/queue.md · **Phase:** L
- **Branch:** feat/depth-l-queue
- **Depends on:** FEAT-0071
- [x] a small client over `kafka-protocol`: framing, request header, API-version negotiation
- [x] `Metadata`, `ListGroups` on every broker, `FindCoordinator` and `OffsetFetch` per group
- [ ] lag per partition (latest minus committed), as a Job with progress and cancel
- [ ] the same TLS and SASL as the connection
- [x] Test: `kafka_groups.rs` (SCRAM-SHA-256 against RFC 7677, nonce check, name escaping, lag arithmetic); the live Kafka test reads groups for a fresh topic
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir` scratch — group `billing` on `hs-kafka` (9 of 24 consumed) shows lag 15, and 10 / 0 / 5 per partition
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** lag per partition works; the progress and Stop are in the Consumers pane rather than a shell Job (spec §7 updated) and were not seen on screen, because one group answers too fast to catch. SASL as FEAT-0071: the groups client authenticated with PLAIN and both SCRAM mechanisms against `hs-kafka-sasl` on 2026-11-06 (the live test's `consumers` call); TLS and OAUTHBEARER remain unverified for the same reasons.

### FEAT-0073 · Redis Streams
- **State:** Completed · **Spec:** docs/tools/queue.md · **Phase:** L
- **Branch:** feat/depth-l-queue
- **Depends on:** FEAT-0069
- [x] streams found with `SCAN … TYPE stream`, bounded
- [x] length, groups, first and last id; groups with pending and lag (Redis 7)
- [x] peek by id range, publish with `XADD`, purge by trim, delete the key, guarded
- [x] Test: `redis_streams.rs` unit tests (entry parsing, payload of fields, `data` field split); `live.rs::redis_streams_lists_peeks_publishes_and_purges_a_stream`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir` scratch — `hs-redis`: streams listed, group `mailer` with pending 2 and lag 4, an entry added with a field and a payload and peeked back
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### FEAT-0074 · NATS JetStream
- **State:** Completed · **Spec:** docs/tools/queue.md · **Phase:** L
- **Branch:** feat/depth-l-queue
- **Depends on:** FEAT-0069
- [x] JetStream streams with messages, bytes, consumers and subjects (`async-nats`)
- [x] durable consumers with pending and ack floor
- [x] peek by sequence, publish to any subject with headers, purge a stream, guarded
- [x] Test: `live.rs::nats_lists_peeks_publishes_and_purges_a_stream` (stream publish, peek, purge, and a subject no stream captures published as plain NATS)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir` scratch — `hs-nats`: stream `ORDERS` peeked by sequence with its subject, consumer `shipper` with lag 9, a publish to `orders.paid` stored at sequence 9
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### FEAT-0075 · MQTT
- **State:** Completed · **Spec:** docs/tools/queue.md · **Phase:** L
- **Branch:** feat/depth-l-queue
- **Depends on:** FEAT-0069
- [x] topics observed by subscribing for a bounded window, the bounds stated
- [x] retained and live messages, MQTT 3.1.1 and 5
- [x] publish with QoS and retain; clear a retained message, guarded
- [x] Test: `mqtt_wire.rs` (remaining-length encoding, CONNECT layout, PUBLISH round trip, property skipping, refusal reasons), `mqtt.rs` (folding, topic bound); `live.rs::mqtt_sees_retained_topics_and_clears_them` on 3.1.1 and 5
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: `--data-dir` scratch — `hs-mqtt` (MQTT 5): retained topics seen in the 3 s look, a retained QoS 2 publish to `home/kitchen/light` peeked back
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** not `rumqttc` as first planned: its current release pins a `rustls-webpki` with open advisories, so MQTT is our own `mqtt_wire.rs` (ADR-027).

---

## v0.7.2 — Done 2026-10-31

> Closed on the owner's instruction, 2026-10-31. The three items' **App** boxes
> stay unticked: the features were driven headlessly against SQLite, never on
> screen and never against PostgreSQL or MySQL. ERD **export** was never
> implemented and leaves this batch unbuilt.

### FEAT-0065 · ER diagram from the catalog's foreign keys
- **State:** Completed · **Spec:** docs/tools/database.md · **Phase:** H
- **Branch:** feat/depth-h-sql-2
- [x] `DbSession::table_schema` returns columns, primary key, indexes and foreign keys
- [x] build the ERD from the foreign keys into `hs-diagram`
- [x] open it in the Diagram Editor
- [x] export it — `erd::to_svg`, palette `db.erd_export`, Ctrl+Alt+Shift+D
- [x] Test: `drivers.rs::sqlite_table_schema` reads a real SQLite catalog
  (autoincrement, composite key, unique index, cascade rule, the cap);
  `erd.rs` unit tests cover the cardinality and the edges left out;
  `db_tests.rs::the_er_diagram_reaches_the_graphics_tool` asserts the
  hand-over is a real document with its key badges
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: opened on screen against the SQLite fixtures — the ERD renders in the
  Diagram Editor with PK/FK/UQ badges and the `1 .. *` edge, and Ctrl+Alt+Shift+D
  wrote a 2.6 KB SVG naming both entities and `customer_id` intact
  (`docs/screenshots/db-erd.png`). Postgres was used through the driver tests, not
  the app
- [x] Docs: spec · CHANGELOG · CHECKLIST · GOTCHAS (README needed no change:
  it lists the activity, not its individual tools)
- **Notes:** `crates/tools/tool-db/src/{schema,erd}.rs`. Code landed in
  `6707780`, tests in `e02986e`. **Export landed afterwards** (2026-10-31):
  `erd::to_svg` writes a standalone SVG, laid out with `layout_approx` so it
  needs no fonts and is byte-identical each run. It goes out of the Database
  tab rather than the Diagram Editor, which exports PNG and PDF itself once
  the document is open there. Writing it exposed a defect in `hs-diagram`
  affecting every ERD — entities were measured by their label but drawn with
  their column list, so identifiers wrapped mid-word; `layout::node_size` now
  measures what it draws. **Live servers now verified** by
  `drivers.rs::{postgres_table_schema, mysql_table_schema}` against
  PostgreSQL 16 and MySQL 8. The App box stays open because it asks for the
  running app: everything here is tests, and nothing has been seen on screen.

### FEAT-0064 · DDL generator per engine dialect
- **State:** Completed · **Spec:** docs/tools/database.md · **Phase:** H
- **Branch:** feat/depth-h-sql-2
- **Depends on:** FEAT-0065
- [x] CREATE TABLE / INDEX / FK from the catalog, per engine dialect
- [x] Test: `ddlgen.rs` unit tests cover each dialect's identity spelling,
  SQLite's inlined keys and missing schema prefix, the options and the
  statement order; `db_tests.rs::the_ddl_dialog_opens_with_the_schemas_statements`
  drives the dialog and checks the script it puts in the editor
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: generated on screen for the whole schema, and the dialect switched from
  SQLite to PostgreSQL in the dialog — identity spelling and the table-level key
  both change, and the caution about column types appears
  (`docs/screenshots/db-ddl.png`)
- [x] Docs: spec · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `crates/tools/tool-db/src/ddlgen.rs`. Code landed in `6707780`,
  tests in `e02986e`. **Live catalogs now verified** (2026-10-31):
  `drivers.rs::{postgres_table_schema, mysql_table_schema}` generate the DDL
  from real PostgreSQL 16 and MySQL 8 catalogs and check the composite key
  and the `ADD CONSTRAINT` form. The App box stays open because it asks for
  the running app, and nothing has been seen on screen. Column types pass
  through as the source spelled them, so cross-dialect output — a PostgreSQL
  schema written out as MySQL — is still the case worth checking by hand.

### FEAT-0063 · Schema and data compare between two connections
- **State:** Completed · **Spec:** docs/tools/database.md · **Phase:** H
- **Branch:** feat/depth-h-sql-2
- **Depends on:** FEAT-0065
- [x] schema compare: tables, columns, types, indexes as New / Missing / Differs
- [x] generated migration script from the differences
- [x] data compare of one table by primary key, sampled, with a bound
- [x] Test: `compare.rs` unit tests cover each verdict, keys matched on
  columns rather than names, the deferred NOT NULL and that no drop or
  delete is ever live SQL; `db_tests.rs` runs both compares against a
  second SQLite database and checks the SQL they generate
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: both compares run on screen between two SQLite databases — the schema
  compare finding `legacy_audit` Missing and `orders.note` New with the DROP
  commented out (`docs/screenshots/db-compare.png`), and the data compare on
  `customers` finding row 2 Differs on `city` and row 3 New, with the generated
  UPDATE/INSERT (`docs/screenshots/db-data-compare.png`)
- [x] Docs: spec · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** `crates/tools/tool-db/src/compare.rs`. Code landed in `6707780`,
  tests in `e02986e`. Both compares ran end to end between two SQLite files,
  headlessly, and a live PostgreSQL and MySQL schema each compare clean
  against themselves in `drivers.rs` (2026-10-31) — but a compare between two
  *different* live schemas has still only been done on SQLite. The App box
  Both compares were exercised on screen on 2026-10-31. The data compare is
  reached from the table's right-click **Compare Data…**, which needs no editor
  text; driving it through the editor's statement does not work from the harness,
  because focus stays on the schema tree. A target reached through an SSH tunnel is refused rather than
  connected to directly, which is also worth a look since it is the one path that
  shows an error instead of a result.

---
  
## v0.7.1 — Done 2026-10-30
> **Shipped in:** app v0.107.0, 2026-10-30

### FEAT-0062 · Devtools depth: network checks, listening ports, snippets, .env editor
- **State:** Completed · **Spec:** docs/tools/devtools.md · **Phase:** G
- [x] DNS, TCP/HTTP ping, TLS certificate viewer, whois
- [x] listening ports with the owning process, filter, copy, guarded kill
- [x] per-workspace snippets with tags and search
- [x] `.env` editor with masked secrets and environment-variable linking
- [x] Test: unit and integration tests in `tool-devtools`
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [x] App: exercised before merge
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** merged as `60b5247`. This batch happens to share its label with
  the app release that carried it; that is a coincidence of being seeded from
  history, not a rule. Reconstructed into this ledger on 2026-10-31, after the
  fact — later entries are written before the work, not after.

---

## v0.7.0 — Done

### FEAT-0061 · Git activity: rail entry, repository list and a SourceTree-shaped work pane
- **State:** Completed · **Source:** `mynotes` L9-45 (27 requests) · **Kind:** major
- **Scope:** New activity and Settings page; global Git credentials in the vault; per-workspace repositories with icon, project fetch and local path; graph, blame, diff, branches, stashes, staging, commit, pull/push/fetch, merge, tag, git flow, history, project terminal; dashboard, right-click edit/remove, AI assistant prompts.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.6.0 — Done

### FEAT-0060 · One dashboard language across every activity
- **State:** Completed · **Source:** `mynotes` L47-64 (14 requests) · **Kind:** major
- **Scope:** Header band, summary cards in rounded boxes, Start Here, saved-items grid and Open Here, applied from Scratchpad to Editor (which had none), Terminal, SSH, Containers, Database, NoSQL, Redis, S3, API client, Graphics, Devtools and Logs.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.5.1 — Done

### BUG-0035 · Toolbar tooltips, log sidebar proportions, header rows and S3 hosts
- **State:** Completed · **Source:** `mynotes` L66-79 (10 requests)
- **Scope:** Tooltip position above the cursor; saved logs/running services/investigations/filter presets at 30/30/20/20 and scrollable; method-only colouring in the API client header; full-width header bars; Devtools unit-conversion result width; Nexus and WebDAV default hosts; Open Folder As Project routing; Remove From List closing its tab.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.5.0 — Done

### FEAT-0059 · A header row above every work pane toolbar
- **State:** Completed · **Source:** `mynotes` L81-129 (46 requests) · **Kind:** major
- **Scope:** Thick-filled header carrying the item's icon, emphasised name and its own facts (language and version, OS, session, container type, database, dimensions, path) for Scratchpad, Editor, Terminal, SSH, Containers, Database, NoSQL, Redis, S3, API client, Graphics and Logs; the duplicated labels removed from the toolbars; Logs and Diagram toolbars split across rows; empty toolbar rows removed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.4.0 — Done

### FEAT-0058 · Editor activity: project list, workspace sidebar and project settings
- **State:** Completed · **Source:** `mynotes` L131-144 (11 requests) · **Kind:** major
- **Scope:** Sidebar column listing saved projects with language icons and drag ordering; permanent workspace tab and dashboard; Settings -> Editor -> Project List with add/edit in the right column; right-click edit and remove-from-list that closes the tab without touching the folder; per-project Git user info; one tab per project.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.11 — Done

### CR-0023 · Labels beside the icons on every toolbar button
- **State:** Completed · **Source:** `mynotes` L146-155 (8 requests)
- **Scope:** Scratchpad, Editor, SSH, Containers, Database, Graphics and Logs toolbars given icon-plus-label buttons; SSH dimension text dropped and the connection LED moved left of Connect.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.10 — Done

### CR-0022 · Real product icons for terminals, containers, connections and AI providers
- **State:** Completed · **Source:** `mynotes` L157-169 (11 requests)
- **Scope:** Dashboard tab icons matched to their rail icons; WSL, PowerShell 7 and Windows PowerShell distinguished; Docker, Podman and Kubernetes; every connection kind; every AI provider; Workspace and Environment labels in the main toolbar.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.9 — Done

### FEAT-0057 · Multiple SSH forwards per host, and host OS detection
- **State:** Completed · **Source:** `mynotes` L171-177 (5 requests)
- **Scope:** Tunnel card taking several forwards per connection; the target OS detected on add so the sidebar and tab icons follow it; icon properties above transparency; terminal toolbar cleanup; Devtools sidebar selection fixed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.8 — Done

### CR-0021 · Tab icons follow the sidebar icon of what they opened
- **State:** Completed · **Source:** `mynotes` L179-185 (4 requests)
- **Scope:** Terminal shells, containers, Graphics tools and Devtools tools all show in their tab the same icon the sidebar column uses.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.7 — Done

### FEAT-0056 · Icon outlines, bigger dashboard activity icons and cursor-aware tooltips
- **State:** Completed · **Source:** `mynotes` L187-192 (4 requests)
- **Scope:** Theme-aware outline on every icon with a toggle in Settings -> Appearance -> Sidebar; larger spaced activity icons with a hover indicator; tooltips positioned above the cursor without overlapping it.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.6 — Done

### BUG-0034 · Freeze on close, freeze on update, and the toolbar/pin layout
- **State:** Completed · **Source:** `mynotes` L194-203 (7 requests)
- **Scope:** Hang on the window close button and on restart-and-install; sidebar rail and column moved up with toolbars aligned to the work pane; pinned tabs tied to the workspace rather than the environment; S3 tab text following the connection name; home dashboard activity icons; New Note creating a note.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.5 — Done

### BUG-0033 · Container detail views, S3 two-column browsing and one tab per container
- **State:** Completed · **Source:** `mynotes` L205-211 (5 requests)
- **Scope:** Detail view for images, volumes, networks, stacks, services and nodes; S3 buckets moved out of the connection tree into a list-plus-content work pane; a single tab per container; Pin To Home in the container list; bigger add-host icons on one row.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.4 — Done

### BUG-0032 · Drag handles, right-click edit and a Connect button in the connection settings
- **State:** Completed · **Source:** `mynotes` L213-220 (6 requests)
- **Scope:** Draggable icon in the container list; coloured container rail icon; right-click to edit a connection or container straight into its Settings page; Connect beside Test Connection that saves and opens the activity tab; two-column connection settings questioned.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.3 — Done

### FEAT-0055 · Qdrant, Valkey, Nexus and WebDAV connections, plus registry credentials
- **State:** Completed · **Source:** `mynotes` L222-226 (3 requests)
- **Scope:** Four new connection kinds; Docker Hub and Helm credentials stored for authenticated pulls; Parallels Desktop viability for Mac beta testing answered.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.2 — Done

### CR-0020 · Workspace dropdown width and whole-row click
- **State:** Completed · **Source:** `mynotes` L228-231 (2 requests)
- **Scope:** Dropdown halved in width, the entire row including the icon clickable before the delete button, and the list auto-sized to the longest workspace name.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.1 — Done

### FEAT-0054 · Container milestone breakdown
- **State:** Completed · **Source:** `mynotes` L233-242 (0 requests)
- **Scope:** DataTable scrolls, container kinds, Settings -> Containers and vault wiring; the bollard Docker/Podman driver; activity, rail, dashboard and sidebar with one tab per host; the Portainer-shaped Docker pane with Logs and Exec handoffs; Compose and Swarm; the Kubernetes driver and Lens-shaped pane; apply with dry-run diff, port-forward and CRD discovery; AI assistant with action blocks and the Problems feed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.3.0 — Done

### FEAT-0053 · Container activity: Docker, Podman, Kubernetes and Windows containers
- **State:** Completed · **Source:** `mynotes` L244-257 (11 requests) · **Kind:** major
- **Scope:** New activity between SSH and Database with its own Settings page, dashboard and permanent tab; container list and saved configs in the sidebar at 40/60; everything sensitive in the vault; Portainer-shaped Docker pane and Lens-shaped Kubernetes pane; AI assistant that writes and applies YAML.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.2.1 — Done

### BUG-0031 · Logs sources: icons, path auto-population and permission errors
- **State:** Completed · **Source:** `mynotes` L259-275 (14 requests)
- **Scope:** Service-appropriate icons for saved logs and running services; Node path auto-population; saved logs synchronised across tabs; MySQL and Postgres access-denied; external cmd replaced by the built-in terminal; image metadata card; AI assistant hangs while describing an image; thread-dump reading, upload and paste; per-state thread counts and deadlock cycles.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.2.0 — Done

### FEAT-0052 · Configurable AI assistant prompt buttons
- **State:** Completed · **Source:** `mynotes` L277-284 (5 requests) · **Kind:** major
- **Scope:** Settings -> AI Assistant -> Prompt Button listing every activity's prompts, reorderable and hideable within an activity, with editable name and prompt text; the hardcoded prompts migrated into it; global rather than per workspace.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.32 — Done

### BUG-0030 · Running-service detection, log tab titles and sidebar flicker
- **State:** Completed · **Source:** `mynotes` L286-293 (6 requests)
- **Scope:** Keycloak and Docker-in-WSL detection; saved log name on the tab; path auto-population when editing a running service; flicker when switching saved logs and the performance work behind it; AI assistant limited to filter actions in Logs.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.31 — Done

### BUG-0029 · Log level colouring, source auto-detection and markdown tables
- **State:** Completed · **Source:** `mynotes` L295-299 (3 requests)
- **Scope:** `level=info` and every other level coloured whatever the format; source type, path, command and format auto-filled when editing a running service; markdown table previews made responsive to the AI assistant pane width.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.30 — Done

### FEAT-0051 · Logs: horizontal scrolling, Running Service section and editable saved logs
- **State:** Completed · **Source:** `mynotes` L301-311 (9 requests)
- **Scope:** Horizontal scroll so long lines are readable with the AI pane open; environment column dropped from the saved list in favour of the toolbar selector; draggable and editable saved logs; a Running Service section detecting local and container services with pre-filled configuration; one tab per saved log.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.29 — Done

### CR-0019 · Logs AI assistant in its own column pane
- **State:** Completed · **Source:** `mynotes` L313-315 (1 request)
- **Scope:** The floating assistant replaced with a proper column pane, as the other activities already had.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.28 — Done

### BUG-0028 · Auto format detection for journalctl, level colours and the Logs dashboard
- **State:** Completed · **Source:** `mynotes` L317-321 (3 requests)
- **Scope:** `format = auto` picking the right parser for `journalctl -f -u docker`; `level=<level>` coloured red/green/yellow; a dashboard page for the activity, which had none.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.27 — Done

### CR-0018 · Split Right, Split Down and Close Split removed
- **State:** Completed · **Source:** `mynotes` L323-326 (2 requests)
- **Scope:** The split-pane commands, actions and palette entries taken out of the app after weighing whether to keep them with explicit buttons instead.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.26 — Done

### CR-0017 · A strict 1720x860 minimum window, and window size in Settings
- **State:** Completed · **Source:** `mynotes` L328-332 (3 requests)
- **Scope:** Three-quarter, two-third, half and default screen presets removed leaving full screen and custom; width and height fields in Settings -> General -> Startup that track a resize; bigger Graphics sidebar entries.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.25 — Done

### FEAT-0050 · Image upscaler and enhancer workflows
- **State:** Completed · **Source:** `mynotes` L334-340 (5 requests)
- **Scope:** Upscaler workflow JSON settings including ratio-based services; a proper enhancer workflow distinct from upscaling; an Upscale button beside Generate, Edit and Enhance; the Generate Picture dropdown overlap fixed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.24 — Done

### BUG-0027 · ComfyUI generation failures and model discovery
- **State:** Completed · **Source:** `mynotes` L342-818 (3 requests)
- **Scope:** HIP out-of-memory from CLIPLoader on the supplied workflow JSON; the image-edit model `qwen_image_edit_2509_fp8_e4m3fn.safetensors` not listed when fetching endpoints, only generation models.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.23 — Done

### FEAT-0049 · Signed zip updates, and appearance settings for rail and sidebar
- **State:** Completed · **Source:** `mynotes` L820-826 (5 requests)
- **Scope:** Update download as a zip with its SHA, extracted locally and swapped in; bigger workspace list entries; a workspace selector at the top of Settings -> Appearance -> Activity Rail; a Sidebar section card for the icon toggles.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.22 — Done

### FEAT-0048 · A Logs tab in the bottom dock
- **State:** Completed · **Source:** `mynotes` L828-831 (2 requests)
- **Scope:** Every info, error and debug line in one dock tab with the level set in Settings -> General, and failures from any action surfaced there as they happen.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.21 — Done

### BUG-0026 · ComfyUI execution_start answer, and overlapping controls
- **State:** Completed · **Source:** `mynotes` L833-836 (2 requests)
- **Scope:** The unexpected `execution_start` payload from ComfyUI handled; overlapping actions and information in the generator fixed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.20 — Done

### FEAT-0047 · Screenshots, demo animations and the M4/M5 milestones
- **State:** Completed · **Source:** `mynotes` L838-847 (8 requests)
- **Scope:** Every screenshot and GIF refreshed for the new and changed features; self-hosted request shapes against a real server; DB transaction-state inference; async import/export jobs; the API client milestone (auth including OAuth2 and SigV4, response viewer, history, curl import/export) and the Logs milestone (file engine, SSH source, parsers, filters, highlights, structured view, find, bookmarks, saved sources).
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.19 — Done

### CR-0016 · Manage Collection as tabbed categories
- **State:** Completed · **Source:** `mynotes` L849-855 (5 requests)
- **Scope:** The single settings page split into General, Variables and script tabs; the method-coloured icon dropped from the manage-collection tab; collection variables editable in place; icons on the Graphics dashboard function buttons.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.18 — Done

### BUG-0025 · Raw markdown in AI assistant replies
- **State:** Completed · **Source:** `mynotes` L857-859 (1 request)
- **Scope:** Responses rendered as markdown instead of raw text wherever the assistant answers.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.17 — Done

### FEAT-0046 · Self-hosted image providers: ComfyUI, SwarmUI, Stability Matrix, InvokeAI
- **State:** Completed · **Source:** `mynotes` L861-865 (3 requests)
- **Scope:** Provider entries for the self-hosted stacks, selectable for generation, editing and upscaling, with the AI Assistant settings page rearranged around chat, image and video provider types.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.16 — Done

### FEAT-0045 · DeepSeek provider, and 1min AI image models
- **State:** Completed · **Source:** `mynotes` L867-872 (4 requests)
- **Scope:** DeepSeek with OpenAI-compatible and Anthropic-style placeholders; describe-picture payloads for DeepSeek Flash and Pro; Stable Diffusion XL 1.0 as the 1min AI default for generation and editing.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.15 — Done

### FEAT-0044 · Tab tooltips, per-activity sidebar widths, API client scripts and image AI
- **State:** Completed · **Source:** `mynotes` L874-898 (22 requests)
- **Scope:** Sentence-case tab tooltips; drag handles vertically centred; smaller pinned-note pins; the Scratchpad assistant given its own pane; user-set sidebar width per activity, persisted; lowercase UI text corrected; a functions section in the API client sidebar; package libraries callable from collection and request scripts; Manage Collection from the right-click menu; script inheritance from collection to request; coloured rail icons; sessions disconnected on close; ERD entity text visibility; multi-provider image generation.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.14 — Done

### CR-0015 · Diagram editor toolbar reduced to icons
- **State:** Completed · **Source:** `mynotes` L900-902 (1 request)
- **Scope:** Add Entity, Import DDL, Add Parent and the rest shown as icons with tooltips rather than icon-and-text.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.13 — Done

### FEAT-0043 · Release 2 diagrams: ERD entities and sequence lifelines
- **State:** Completed · **Source:** `mynotes` L904-907 (0 requests)
- **Scope:** `NodeKind::Entity` table nodes with structured columns, and sequence lifeline layout with participants and messages.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.12 — Done

### FEAT-0042 · draw.io import verified against the diagram editor
- **State:** Completed · **Source:** `mynotes` L909-922 (8 requests)
- **Scope:** `drawio_pages()` decoding compressed and inline pages with their names, and `drawio_xml_page(text, page)` importing a chosen page, checked against what was actually implemented.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.11 — Done

### BUG-0024 · Five days of new code reviewed, and window responsiveness
- **State:** Completed · **Source:** `mynotes` L924-928 (3 requests)
- **Scope:** Every function and feature added since the previous session checked; toolbar buttons and multi-pane layouts behaving as the window is resized.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.10 — Done

### BUG-0023 · API client folder rename collisions
- **State:** Completed · **Source:** `mynotes` L930-933 (1 request)
- **Scope:** Renaming a folder to a name that already exists resolved with the suffix rule instead of failing.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.9 — Done

### BUG-0022 · Diagram editor frames, edge labels and properties pane
- **State:** Completed · **Source:** `mynotes` L935-963 (25 requests)
- **Scope:** Outer-frame title position and alignment; inner frames from the toolbar; the doubled separator in the properties pane; edge labels draggable reliably with a selection indicator; tab text overlapping the close button; collection and folder name collisions; scratchpad note reordering; indicators for arrow, frame and edge selection; frame drag and resize; the assistant in its own pane; undo in the image and icon tools; align and distribute actually working.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.8 — Done

### BUG-0021 · Align and distribute in the diagram editor
- **State:** Completed · **Source:** `mynotes` L965-968 (2 requests)
- **Scope:** The non-working align and distribute actions fixed with tests, and the editor toolbar reduced to icons.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.7 — Done

### BUG-0020 · Diagram frames, edge labels and the R2 enhancements
- **State:** Completed · **Source:** `mynotes` L970-980 (9 requests)
- **Scope:** The frame, edge-label and properties-pane defects again, plus API client name collisions, tab text overlap, and the remaining R2 work on ERD and sequence diagrams and the `.diagram.json` alias and schema migration.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.6 — Done

### CR-0014 · Scratchpad reordering, coloured collection icons and the demo GIF
- **State:** Completed · **Source:** `mynotes` L982-989 (6 requests)
- **Scope:** Notes reorderable except pinned ones; coloured material icons for collections and folders; the README demo covering every activity rather than only the Graphics tools, with pauses between steps.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.5 — Done

### BUG-0019 · Disabled dashboard buttons and missing inline AI actions
- **State:** Completed · **Source:** `mynotes` L991-996 (4 requests)
- **Scope:** New Diagram and New Generation enabled; inline action buttons on diagram-editor suggestions; markdown previews everywhere the assistant answers.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.4 — Done

### BUG-0018 · Diagram title box, selection indicators and dashed edges
- **State:** Completed · **Source:** `mynotes` L998-1008 (9 requests)
- **Scope:** Diagram title shown above or below the frame box; vertically centred What To Do cards; indicators when selecting an arrow, frame or edge; edge label and frame repositioning; dashed and dotted edge styles; the assistant in its own pane with inline actions; the Devtools tab-strip close button.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.3 — Done

### FEAT-0041 · Offline diagram drafts and per-node colours
- **State:** Completed · **Source:** `mynotes` L1010-1018 (7 requests)
- **Scope:** An Offline Draft button beside Generate building a diagram from the prompt with local heuristics and type templates; outline, fill, text and edge colours per node; the database shape corrected; separators hidden when nothing is selected.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.2 — Done

### CR-0013 · One UI/UX standard across every tool, written down as a rule
- **State:** Completed · **Source:** `mynotes` L1020-1040 (16 requests)
- **Scope:** Spacing, alignment and component style made consistent starting from the Graphics dashboard; node icons in the outline; properties grouped into sections; source, preview and split views for diagrams; assistant suggestions applied straight to the diagram; proper node shapes; selectable arrows; the licence settled as MIT.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.1 — Done

### CR-0012 · Plan, branch and document before coding, as a standing rule
- **State:** Completed · **Source:** `mynotes` L1042-1048 (3 requests)
- **Scope:** Every new request planned and written into the checklist, README and roadmap before code; a proper branch name first; documentation, screenshots and animations updated after the work, with gotchas and lessons recorded.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.1.0 — Done

### FEAT-0040 · Graphics activity: image, icon, diagram editor and diagram generator
- **State:** Completed · **Source:** `mynotes` L1050-1083 (8 requests) · **Kind:** major
- **Scope:** Scope settled from the feasibility questions: Archify ideas reimplemented with the MIT notice, `.hdiag` editor-native with a separate IR, draw.io XML import, flowchart and architecture for v1 with ERD and sequence in the first release, diagrams saved under the project's docs folder, drafts kept in Recent, the generator working offline from templates, icon editing with ICO sizes and colour-picker transparency, and an AI assistant with prompt buttons in all four tools.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.60 — Done

### FEAT-0039 · Terminal settings of its own, and a documentation pass
- **State:** Completed · **Source:** `mynotes` L1085-1088 (2 requests)
- **Scope:** Settings page for the terminal to carry future CLIs such as mysql-cli, postgres-cli, Claude Code and opencode; every screenshot and animation refreshed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.59 — Done

### FEAT-0038 · Scrollable sidebars, and the future CLI list
- **State:** Completed · **Source:** `mynotes` L1090-1105 (14 requests)
- **Scope:** Scrolling for the Home, Terminal, SSH, Database, NoSQL, Redis, S3, API client, Browser, Devtools and Scratchpad sidebars, and a plan for adding the CLI entries.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.58 — Done

### FEAT-0037 · Devtools tool list scrollable
- **State:** Completed · **Source:** `mynotes` L1107-1109 (1 request)
- **Scope:** The sidebar tool list scrollable so tools cannot overflow out of reach as more are added.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.57 — Done

### CR-0011 · Checklist entries by date, and a documentation pass
- **State:** Completed · **Source:** `mynotes` L1111-1114 (2 requests)
- **Scope:** The owner's requests recorded in CHECKLIST.md by date, and the documentation, screenshots and animations brought up to date.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.56 — Done

### FEAT-0036 · Credentials revealed and editable in Settings -> Security
- **State:** Completed · **Source:** `mynotes` L1116-1122 (5 requests)
- **Scope:** Unlock showing both password and key, editable and saveable; and the plan-first, branch-first rules restated before the remaining M4 Git work.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.55 — Done

### BUG-0017 · Favicons and spacing on the browser favourites bar
- **State:** Completed · **Source:** `mynotes` L1124-1127 (2 requests)
- **Scope:** Real site favicons on the bar, with left and right padding between folders and links.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.54 — Done

### FEAT-0035 · A setting to show or hide the favourites bar
- **State:** Completed · **Source:** `mynotes` L1129-1131 (1 request)
- **Scope:** Settings -> Browser toggle, with the bar sitting in a row below the toolbar.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.53 — Done

### BUG-0016 · Dragging API client requests between folders
- **State:** Completed · **Source:** `mynotes` L1133-1135 (1 request)
- **Scope:** Requests draggable into other collections, folders, sibling folders and sub-folders, not only within the same one.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.52 — Done

### BUG-0015 · Base64 tool opening, the dock terminal and the Java language server
- **State:** Completed · **Source:** `mynotes` L1137-1143 (4 requests)
- **Scope:** Clicking Base64 opening the tool rather than the fixed tab; the bottom-dock terminal removed as purposeless; jdtls status surviving a restart without a manual refresh; the Maven and Gradle work continued.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.51 — Done

### CR-0010 · Pinned tabs ordered first come, first served
- **State:** Completed · **Source:** `mynotes` L1145-1147 (1 request)
- **Scope:** The first tab pinned sits immediately right of the permanent tab, and later pins follow in order.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.50 — Done

### CR-0009 · Pinned tabs start to the right of the permanent tab
- **State:** Completed · **Source:** `mynotes` L1149-1151 (1 request)
- **Scope:** Applied to every activity that has a permanent fixed tab.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.49 — Done

### FEAT-0034 · Project language detection in the editor
- **State:** Completed · **Source:** `mynotes` L1153-1155 (1 request)
- **Scope:** The workspace detecting the project language so the tab shows it and the matching tooling is offered.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.48 — Done

### CR-0008 · The permanent tab is never overwritten by a connection
- **State:** Completed · **Source:** `mynotes` L1157-1160 (2 requests)
- **Scope:** The fixed first tab reserved for the dashboard, summary or home view, and a terminal in the editor defaulting to PowerShell 7 with the choice in Settings -> Editor.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.47 — Done

### FEAT-0033 · A permanent first tab in every activity
- **State:** Completed · **Source:** `mynotes` L1162-1181 (15 requests)
- **Scope:** Fixed, unclosable, unpinnable first tab for the API client, Terminal, SSH, Database, NoSQL, Redis, S3, Browser, Home, Graphics, Devtools and Scratchpad, carrying the workspace logo and name; collection and folder duplication with the suffix rule; bandwidth units in the unit converter.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.46 — Done

### BUG-0014 · Home recent list alignment
- **State:** Completed · **Source:** `mynotes` L1183-1185 (1 request)
- **Scope:** The Recent section left-aligned instead of centred.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.45 — Done

### CR-0007 · No duplicate main tab in the Graphics tools
- **State:** Completed · **Source:** `mynotes` L1187-1189 (1 request)
- **Scope:** Duplicating the outer tab disallowed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.44 — Done

### FEAT-0032 · Pinned connections grouped by type on Home
- **State:** Completed · **Source:** `mynotes` L1191-1195 (1 request)
- **Scope:** Separators, type headings and colour-coded pills per connection type, ordered to follow Settings -> Appearance -> Activity Rail.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.43 — Done

### FEAT-0031 · Unit Conversion in Devtools
- **State:** Completed · **Source:** `mynotes` L1197-1205 (5 requests)
- **Scope:** Time conversions including epoch, numeric base conversions with a dedicated result per type, and the other conversions worth having, built to the same UI standard.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.42 — Done

### BUG-0013 · Pin To Home, duplicate naming and request dragging
- **State:** Completed · **Source:** `mynotes` L1207-1221 (12 requests)
- **Scope:** Pin To Home missing from the Database, NoSQL, Redis and S3 sidebars; workspace icon on the Home header; pinned connections grouped by type; right-click menus disabled on outer tabs where they do not apply; duplicate and new-request naming with the space-suffix rule; a request-name row above the action bar; dragging requests between folders; one browser tab per workspace; Split Right and Split Down removed from tab menus.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.41 — Done

### FEAT-0030 · Open Data creating its own query tab
- **State:** Completed · **Source:** `mynotes` L1223-1225 (1 request)
- **Scope:** Opening data from the table list creating an Untitled query tab rather than replacing the current one.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.40 — Done

### BUG-0012 · Request method dropdown alignment
- **State:** Completed · **Source:** `mynotes` L1227-1229 (1 request)
- **Scope:** The dropdown aligned with the URL field on the same line.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.39 — Done

### FEAT-0029 · SQL prettifier, and standard method colours
- **State:** Completed · **Source:** `mynotes` L1231-1233 (2 requests)
- **Scope:** The prettifier button landed with the query word-wrap toggle deliberately held back; GET green, POST blue and the rest standard, colouring only the method.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.38 — Done

### FEAT-0028 · Query prettifier, wrap toggle and the space-suffix rule
- **State:** Completed · **Source:** `mynotes` L1235-1239 (3 requests)
- **Scope:** Multi-line indented SQL formatting in the open query tab; a wrap toggle for horizontally scrolling queries; duplicate naming restored to " 001" with the leading space.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.37 — Done

### BUG-0011 · Result grid not resizing when the AI pane opens
- **State:** Completed · **Source:** `mynotes` L1241-1243 (1 request)
- **Scope:** The table view reflowing instead of being overlapped by the assistant column.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.36 — Done

### CR-0006 · Get Started order, script pane heights and one tab per request
- **State:** Completed · **Source:** `mynotes` L1245-1252 (6 requests)
- **Scope:** Cards ordered New Workspace, Environments, New Connection, Open Folder as Project, New Terminal, Quick Open; pre-request and post-response script sections given equal height; one tab per request with duplication handled; requests draggable between parent and sub-folders; misaligned dialog buttons fixed; middle-click slowness deferred as not reproducible.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.35 — Done

### FEAT-0027 · One database connection per tab, with query tabs inside it
- **State:** Completed · **Source:** `mynotes` L1254-1260 (5 requests)
- **Scope:** A single connection per outer tab with multiple query tabs beneath it; history and saved queries moved beside the version and filter columns; the reveal-password dialog buttons aligned.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.34 — Done

### CR-0005 · Connection string column removed from the credentials table
- **State:** Completed · **Source:** `mynotes` L1262-1264 (1 request)
- **Scope:** The unused column dropped and the remaining columns given sensible widths.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.33 — Done

### CR-0004 · AI Assistant button hard right in every toolbar
- **State:** Completed · **Source:** `mynotes` L1266-1268 (1 request)
- **Scope:** Applied across every connection and activity toolbar where it appears.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.32 — Done

### BUG-0010 · Sentence case in parentheses, and SFTP operations
- **State:** Completed · **Source:** `mynotes` L1270-1273 (2 requests)
- **Scope:** "None (Direct connection)" and its kind corrected throughout; new folder, new file, permission, owner and group changes working in the SFTP pane.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.31 — Done

### CR-0003 · Generator results tied to their own type
- **State:** Completed · **Source:** `mynotes` L1275-1277 (1 request)
- **Scope:** UUID, number, password and passphrase each keeping their own result rather than sharing one field.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.30 — Done

### FEAT-0026 · Cron expression explained in detail
- **State:** Completed · **Source:** `mynotes` L1279-1281 (1 request)
- **Scope:** Checking an expression also showing the schedule it describes and the next run times.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.29 — Done

### FEAT-0025 · Generators, and TOML and XML verifiers in Devtools
- **State:** Completed · **Source:** `mynotes` L1283-1287 (2 requests)
- **Scope:** Time-based and random UUIDs, seeded random numbers with bounds, password and passphrase generators with criteria, and TOML and XML verification and prettifying.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.28 — Done

### FEAT-0024 · A Credentials section in Settings -> Security
- **State:** Completed · **Source:** `mynotes` L1289-1291 (1 request)
- **Scope:** Every encrypted connection credential listed below the timer section, revealable with the master password.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.27 — Done

### BUG-0009 · Three sessions per SSH host, tab favicons and pinned tab text
- **State:** Completed · **Source:** `mynotes` L1293-1309 (15 requests)
- **Scope:** Up to three tabs per host as separate sessions; Home links focusing an already-open connection; site favicons on browser tabs; pinned tabs keeping their text with an indicator; one tab per image in Graphics; the six-dot drag handle; the browser address field editable; bigger Home section headings with Pinned Connections above Recent in a grid; real pinned connections rather than mock ones; the database list showing only configured connections; SFTP new-file errors; the Database assistant keeping its pane.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.26 — Done

### FEAT-0023 · Cron expression generator in Devtools
- **State:** Completed · **Source:** `mynotes` L1311-1313 (1 request)
- **Scope:** Generating expressions for Unix cron, Spring and Quartz.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.25 — Done

### FEAT-0022 · Test Connection in the connection settings
- **State:** Completed · **Source:** `mynotes` L1315-1317 (1 request)
- **Scope:** A Test Connection button between Save and Delete, reporting success or the failure reason.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.24 — Done

### FEAT-0021 · Browser favourites and passwords, and a full SFTP context menu
- **State:** Completed · **Source:** `mynotes` L1319-1376 (51 requests)
- **Scope:** Favourites and Passwords tabs with folders and favicons; Add To Favorites and Fill In Password on the toolbar; show/hide checkboxes in the activity rail with auto-reveal when a hidden activity is needed; New File and Permission in the SFTP pane; a full right-click menu with Open In Editor, Rename, Download, Download Zipped, Copy, Copy Path, Delete and Permission; double-click on empty space going up a directory; middle-click tab opening speed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.23 — Done

### FEAT-0020 · Browser favourites and passwords, and the SFTP context menu (restated)
- **State:** Completed · **Source:** `mynotes` L1378-1435 (51 requests)
- **Scope:** The same batch restated with British spellings; kept as its own entry because it was sent as its own request.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.
- **Notes:** a restatement of FEAT-0021; kept as its own entry because it
  arrived as its own request.

## v0.0.22 — Done

### BUG-0008 · Docked inspector, sentence case and Redis alignment
- **State:** Completed · **Source:** `mynotes` L1437-1447 (9 requests)
- **Scope:** The developer tools pinned below the page instead of floating; parenthesised wordings corrected; the Redis key list aligned and indented; the Redis version in sentence case; sidebar headers aligned with their add buttons; clipboard copy to and from the SFTP pane.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.21 — Done

### CR-0002 · A connection column in the Redis work pane
- **State:** Completed · **Source:** `mynotes` L1449-1453 (3 requests)
- **Scope:** A proper Connection header with its options, the redundant new-connection button removed from the toolbar, and sidebar headers aligned across SSH, Database, Redis and S3.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.20 — Done

### FEAT-0019 · Redis connection list in the work pane
- **State:** Completed · **Source:** `mynotes` L1455-1457 (1 request)
- **Scope:** A column listing every Redis connection so one can be chosen to connect to.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.19 — Done

### FEAT-0018 · Sidebar rail scrollable with fixed anchors
- **State:** Completed · **Source:** `mynotes` L1459-1461 (1 request)
- **Scope:** Overflow scrolling while Home stays at the top and the sidebar toggle and settings stay in place.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.18 — Done

### BUG-0007 · Devtools YAML input leaking into the converter
- **State:** Completed · **Source:** `mynotes` L1463-1465 (1 request)
- **Scope:** Text typed into the Check tab no longer copied into the YAML to Properties field.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.17 — Done

### BUG-0006 · Workspace cards misaligned on Home
- **State:** Completed · **Source:** `mynotes` L1467-1469 (1 request)
- **Scope:** The workspace list cards aligned to each other instead of drifting up and down.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.16 — Done

### FEAT-0017 · AI providers beyond OpenAI-compatible
- **State:** Completed · **Source:** `mynotes` L1471-1473 (1 request)
- **Scope:** Anthropic, Gemini, Microsoft, Kimi, Tencent and the others added as first-class provider options.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.15 — Done

### FEAT-0016 · YAML and properties conversion, and auto-prettifying
- **State:** Completed · **Source:** `mynotes` L1475-1479 (3 requests)
- **Scope:** Two-way YAML and properties conversion in Devtools; JSON and js.map prettified on open in the editor; first-character selection in the terminal fixed.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.14 — Done

### BUG-0005 · Workspace icons missing from the dropdowns
- **State:** Completed · **Source:** `mynotes` L1481-1484 (2 requests)
- **Scope:** The workspace or folder icon shown beside the name in the startup dropdown and the environment comparison header.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.13 — Done

### FEAT-0015 · AI suggestions in every tool, Insert and Run, and a URL codec
- **State:** Completed · **Source:** `mynotes` L1486-1495 (8 requests)
- **Scope:** Explain, review and improve options beyond the editor; Insert And Run for shell and query suggestions; request and response bodies prettified; a percent-encoding URL encoder and decoder; language-server install buttons; standardised "AI Assistant" button text; visible selection highlight in the terminal.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.12 — Done

### FEAT-0014 · Deleting a workspace, and workspace icons everywhere
- **State:** Completed · **Source:** `mynotes` L1497-1500 (2 requests)
- **Scope:** A workspace deletable with its environments and data while one always remains, and its icon shown in every dropdown, list and comparison header.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.11 — Done

### BUG-0004 · Speech to text, Home workspace list and syntax highlighting
- **State:** Completed · **Source:** `mynotes` L1502-1507 (4 requests)
- **Scope:** Dictation returning nothing after Stop Listening; bigger Home workspace entries with their own icons; syntax highlighting for editor files, scratchpad notes, database binaries, API bodies and decoded base64; tool names under icons on by default.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.10 — Done

### BUG-0003 · Graphics sliders and the grayscale toggle
- **State:** Completed · **Source:** `mynotes` L1509-1512 (2 requests)
- **Scope:** Adjustment sliders previewing live, and Grayscale toggling back like Invert.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.9 — Done

### BUG-0002 · Crop handles limited to left and right
- **State:** Completed · **Source:** `mynotes` L1514-1516 (1 request)
- **Scope:** The crop placeholder resizable from the top, bottom and corners as well.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.8 — Done

### FEAT-0013 · Dictation into the AI assistant
- **State:** Completed · **Source:** `mynotes` L1518-1520 (1 request)
- **Scope:** A toggle in every assistant that types spoken instructions into the prompt field.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.7 — Done

### CR-0001 · Renaming the app considered and declined
- **State:** Completed · **Source:** `mynotes` L1522-1524 (1 request)
- **Scope:** Alternative names weighed because HyperStack exists as another product; the decision recorded as no rename.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.
- **Notes:** outcome was "no need to rename" — the entry records the decision.

## v0.0.6 — Done

### FEAT-0012 · Crop placeholder, scratchpad assistant and context menu wrapping
- **State:** Completed · **Source:** `mynotes` L1526-1530 (3 requests)
- **Scope:** A resizable crop area that fills the input boxes; an assistant in the scratchpad writing notes, checklists, mermaid diagrams and code fences into the markdown; the long-title context menu kept on one line.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.5 — Done

### FEAT-0011 · Pinning scratchpad notes to the top
- **State:** Completed · **Source:** `mynotes` L1532-1534 (1 request)
- **Scope:** Notes selectable for pinning, held above the rest of the list.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.4 — Done

### FEAT-0010 · Scratchpad dual pane, and an assistant in the API client
- **State:** Completed · **Source:** `mynotes` L1536-1543 (6 requests)
- **Scope:** Raw markdown on the left and preview on the right with the note list between sidebar and workspace; an assistant that creates collections and requests directly in the API client; tidier Graphics attribute panes; the speak-aloud button spacing; a performance pass over the new code.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.3 — Done

### FEAT-0009 · Nushell, vaulted Devtools secrets and variable peeking
- **State:** Completed · **Source:** `mynotes` L1545-1550 (4 requests)
- **Scope:** Nushell detected and offered in the shell lists; Firebase JSON, private, public and secret keys stored encrypted in the vault through the environment; more than one tab per Devtools component with values surviving a restart; a way to view and copy the value behind a `{{variable}}` in any field.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.2 — Done

### BUG-0001 · Digest list columns in Devtools
- **State:** Completed · **Source:** `mynotes` L1552-1554 (1 request)
- **Scope:** The hash digest list given proper table columns.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.

## v0.0.1 — Done

### FEAT-0008 · The founding wishlist: browser, API client, graphics, accessibility and workspace settings
- **State:** Completed · **Source:** `mynotes` L1556-2237 (60 requests)
- **Scope:** Workspace icons set and edited after creation and shown in the dropdown; a pinned settings tab; one editor tab per sub-project with wider sub-tabs and multiple encodings; the AI assistant extended to SSH, terminal, SQL, S3 and the API client; the colour palette, Notification and Quick Open buttons; the app logo in the title bar; dark mode surviving a restart on Windows 10; binary preview in the database cell viewer; settings import and export behind the master password; text to speech per area; a welcome entry and optional labels in the sidebar; the file path row under the editor sub-tabs; a secured multi-tab browser with ad and script blocking; a Postman-compatible API client with custom scripts; and the Graphics tools for conversion, enhancement, scaling, extending and icon work.
- **Verified:** the owner's record in `mynotes`. This session ran no gate, no
  app check and no docs pass for it, so it carries no boxes to tick.
