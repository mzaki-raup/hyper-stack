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
Version : v0.7.5
Features : FEAT-0098
Bugs : BUG-0038
Change Request : CR-0039
```

---

## v0.7.6 — Planned

> CR-0025 was split into the themed items below on the owner's decision
> (2026-11-02); every box kept its original wording. The shared primitives
> (CR-0026 … CR-0030) run first because the per-activity passes inherit them.

### BUG-0037 · Miscellaneous Bug
- **State:** In-Progress ·
- **Scope:** All related Activities.
- [ ] In Editor Activity -> Work Pane -> When Enable Debug Panel, Dont't auto resize the Panel when user move the mouse. User can resize the panel and it will persist for that project work pane.
- [ ] In Editor Activity -> Work Pane -> When User Click "Go To Line"; Why the Label "Go To Line" didn't have left padding/margin?
- [x] In Git Activity -> Work Pane; When click "Fetch", there is error "git fetch: fatal: 'origin' does not appear to be a git repository fatal: Could not read from remote repository. Please make sure you have the correct access rights and the repository exists.". This repository is clone from the Settings -> Git -> Add Repository with selected Credential. Why its error?
- [ ] In Automation Activity -> Forgejo Action -> Work Pane -> Runs Pane; When double click a run, its not open any logs. And there is no logs tab to click.
- [ ] In Logs Activity -> Work Pane -> Logs Pane; When Click Open In Editor button, it will open Editor Activity, but nothing display in the file.
- [ ] In Settings -> Editor -> Project List -> Project Detail Setting Form; When click button "Close", it didn't close the project detail settings. Check other settings close button too.

- [x] Test: `an_empty_url_field_leaves_the_remote_fetchable` asserts an empty URL field leaves `remote.origin.url` and the listed remote intact; confirmed failing without the fix. The existing `a_projects_own_config_is_written_to_and_read_from_the_repository` had asserted the opposite - the behaviour that caused this bug - so its expectation was narrowed to exempt that one key, with the reason in the test. Nothing was skipped or deleted (owner agreed, 2026-11-02).
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — All visual of resolved issues.
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** four of the six are fixed in code (go-to-line padding, the Fetch
  failure, the Forgejo double-click, the Settings Close button) and committed;
  only the Fetch box is ticked, because a test proves it. The padding, the
  double-click and the Close button are implemented and pass `fmt` and
  `clippy -D warnings`, but they are visual or interactive and stay unticked
  until they are seen in the running app.
  The debug-panel resize and the Logs "Open In Editor" boxes are **not done**:
  both need reproducing in the app. The panel is set up identically to the
  problems panel beside it, which was not reported, and the Logs path does
  toast `editor-open-failed` for an unreadable file and does open a fresh tab
  for a path outside the project, so neither cause is visible in the source.
  **Gate** cannot be ticked on this machine: `test --workspace` is red through
  BUG-0039, which is outside the workspace (concurrent loopback connections
  are reset here). `fmt`, `clippy -D warnings` and the other 626 tests pass.
  The Forgejo box was answered by opening the run on the server rather than a
  Log tab, on the owner's decision (2026-11-02): Forgejo 11's API serves no
  run log, as `docs/tools/automation.md` records.

### BUG-0039 · This machine resets concurrent loopback connections, which fails every mock-server test
- **State:** Blocked · **Spec:** docs/tools/automation.md
- **Scope:** The machine, not the workspace. Surfaces in `crates/tools/tool-automation/tests/mock.rs` and in any test that talks to a server on `127.0.0.1`.
- [x] Symptom: all five tests in `tests/mock.rs` fail with `error sending request for url (http://127.0.0.1:<port>/...)`. The error chain ends in `Os { code: 10054, kind: ConnectionReset }` - WSAECONNRESET, "An existing connection was forcibly closed by the remote host".
- [x] Not the v0.7.6 work: the same five fail on a clean tree at `d9d2aad` with this branch's changes stashed.
- [x] Not disk: they fail the same way with 523 GB free (see the disk entry in `docs/GOTCHAS.md`).
- [x] Not a proxy and not a timeout: `ProxyEnable` is 0, no `*_proxy` variables are set, the client's timeout is 5 s and the suite fails in 0.03-0.11 s.
- [x] Concurrency is the trigger: with `--test-threads=1`, four of the five pass. In parallel, between three and five fail, and which ones varies per run.
- [x] **Not HyperStack's code.** A pure-Python reproduction using nothing but `socket` and `threading` - four ephemeral loopback servers, one GET each, no tokio, no reqwest, no project code - fails at the same rate with the same `WinError 10054`. Scripts: `loopback_probe.py` (plain close) and `loopback_probe2.py` (FIN then drain before close).
- [x] Not an abortive close: the polite-close variant that shuts down the write half and drains to EOF before closing still resets on two runs out of three, so adding `TcpStream::shutdown` to the harness does not fix it. That speculative change was reverted rather than left in.
- [x] Not a third-party Winsock LSP: `netsh winsock show catalog` lists only Microsoft base providers. Windows Defender is the only AV registered and its real-time protection is on; the firewall is enabled on all three profiles, which does not normally filter loopback.
- [ ] Find what resets the connections - the remaining suspects are a WFP callout driver (Defender's network inspection), the Hyper-V or WSL virtual switch, or a container networking stack. **This needs the owner: it is a machine-level change, not a code change.**
- [ ] Test: `cargo test -p tool-automation --test mock` passes repeatedly in parallel, and the Python probes pass four of four on several consecutive runs.
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: no app surface — nothing in the product is wrong.
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** found on 2026-11-02 running the gate for BUG-0037, and diagnosed to outside the workspace. While it stands, `cargo test --workspace` cannot go green on this machine for any item in this batch, and `--test-threads=1` is a partial workaround only (four of five). `docs/tools/automation.md` records these tests as checked on 2026-10-31, so the machine changed since. The five affected are `teamcity_lists_triggers_and_cancels_through_the_build_queue`, `github_dispatches_with_inputs_and_cancels_a_run`, `gitlab_creates_a_pipeline_with_variables_and_cancels_it`, `awx_launches_a_template_with_its_survey_and_cancels_the_job` and `a_read_only_session_refuses_a_run_before_the_server_sees_it`.

### FEAT-0084 · Material Icon inside Activity Work Pane
- **State:** Pending · 
- **Scope:** All Icon Usage in All Activity Except API Client Activity, Terminal Activity, Automation Activity, Tracker Activity, Graphics Activity, Devtools Activity, Browser Activity and Logs Activity.
- [ ] Create settings for Material Icons (Use in work pane). Add Settings -> Appearance -> Material Icon Card below Activity Rail Card.
- [ ] User can enable/disable Material Icon on each activity listed. By default, all activity is enable. If disabled, use generic icon for respective item (folder and file).
- [ ] Add this activity can show material icon or generic icon: Scratchpad's Sidebar Column - Folder and Note lists.
- [ ] Add this activity can show material icon or generic icon: Editor's Work Pane; Project and Source Control Tab Icon, Folder and File list, 
- [ ] Add this activity can show material icon or generic icon: Git's Work Pane -> Workspace column; Workspace, Branches, Remotes, Tags, Stashes with all its sub item/child (where applicable).
- [ ] Add this activity can show material icon or generic icon: SSH's SFTP - File and Folder list. 
- [ ] Add this activity can show material icon or generic icon: Container's Work Pane - Item list (Container, Projects, Images, Volumes, Network, Stacks, Services, Nodes, etc).
- [ ] Add this activity can show material icon or generic icon: Database's Work Pane - Item list (Database, Tables, Table Item, Table Column, View, Functions, Saved Queries, History, all sub item/child, etc).
- [ ] Add this activity can show material icon or generic icon: NoSQL's Work Pane - Item list (Database, Collections, Documents, Fields, all sub item/child, etc).
- [ ] Add this activity can show material icon or generic icon: Redis's Work Pane - Item list (Folders, Keys, all sub item/child, etc).
- [ ] Add this activity can show material icon or generic icon: S3's Work Pane - Item list (Bucket, Object, Value, Key, Metadata, Folder and Files (webdav), all sub item/child, etc).
- [ ] Add this activity can show material icon or generic icon: Queue's Work Pane - Item list (Queue, Topic, etc).
- [ ] Only these Activities are allowed to enable/disable material icon: Scratchpad, Editor, Git, SSH, Container, Database, NoSQL, Redis, S3, Queue.
- [ ] Test: <what it asserts>
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — Enable and disable Material icon for each activities reflect immediately.
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0025 · Redesign UI UX on all activities, components, dashboards to a standard design.
- **State:** In-Progress ·
- **Scope:** All related UI components. Umbrella item — the work lives in its children.
- **Depends on:** FEAT-0084
- [ ] CR-0026 · Shared control behaviour: hover highlights only, middle alignment, field heights
- [ ] CR-0027 · Toolbar strip standard: sizing, label and icon colour, labelled icon buttons, sub-rows
- [ ] CR-0028 · Table and scroll-area standard: cell sizing, emphasised headers, key/value tables
- [ ] CR-0029 · Tab strip heights, icon sizing and position-aware context menus
- [ ] CR-0030 · Sidebar column header standard across every activity
- [ ] FEAT-0093 · Add-and-focus: a Settings jump target per kind, and Save opens the new connection
- [ ] FEAT-0094 · Remove From List: delete a saved profile and its credential from the sidebar
- [ ] BUG-0038 · Editor -> Source Control -> Refresh Status does nothing
- [ ] CR-0035 · Editor work pane and Source Control tab layout
- [ ] CR-0036 · Multi-coloured product icons for every connection kind
- [ ] CR-0031 · Spacing pass: Redis, S3 and Queue work panes
- [ ] CR-0032 · Spacing pass: Database result and message panels
- [ ] CR-0033 · Spacing pass: Automation, Tracker and Logs work panes
- [ ] CR-0034 · Container and SSH work pane layout
- [ ] CR-0039 · Spacing pass: NoSQL work pane
- [ ] FEAT-0095 · Server-populated fields: Tracker and Automation
- [ ] FEAT-0096 · Database: results retained per query tab, and double-click a cell to edit
- [ ] FEAT-0097 · S3: a collapse/expand folder hierarchy column
- [ ] FEAT-0098 · SSH: name a tunnel when creating it
- [ ] CR-0037 · Redis key list sorted folders first
- [ ] CR-0038 · Settings polish, and the UI standard written down
- **Notes:** split on 2026-11-02 (owner's decision). This item holds no work of
  its own; it is `Completed` when every child above is `Completed`. The 218
  original boxes were distributed verbatim across the children — none was dropped.
  "Close button didn't close the project detail form" stayed with BUG-0037, whose
  sixth box is the same defect, and FEAT-0093 builds on that fix.

### CR-0026 · Shared control behaviour: hover highlights only, middle alignment, field heights
- **State:** Pending ·
- **Scope:** `hs-ui` button, form and dropdown primitives; every activity inherits them.
- **Parent:** CR-0025
- [ ] All buttons when hover, only highlight the button. Do not magnify or scale the button's size.
- [ ] In All Activities -> Resize all text field height, slightly higher; For example in Home -> Dashboard Page, "What do you want to do?" field height is too small. Please follow text field height size in Settings -> Accessibility - Speech To Text -> Language. Make also the height size of all dropdown field, same height with the new text field height.
- [ ] In All Activities -> All Inline Buttons, Label, Text Fields, Dropdown menu, Slider, Checkbox, radio button, etc; Vetical middle align all of it.
- [ ] In Editor Activity -> Work Pane -> Toolbar Strip; When User Click "Find" button, do not magnify/resize Match Case,Whole Word and Regular Expression button. Make it as a proper button so that user know it is a button. Apply it to all other Activities toolbar, sub toolbar that have same characteristic of these button.
- [ ] In Database Activity -> Work Pane -> Result Section Header; When hover Message button, don't magnify, resize the button. Please check other activities that have same characteristic of these button and apply to it also.
- [ ] In Database Activity -> Work Pane -> Result Section Header; When hover Unfocus result button, don't magnify, resize the button. Please check other activities that have same characteristic of these button and apply to it also.
- [ ] In Automation Activity -> Work Pane -> Job Activity Pane -> Run Tab; Please vertical middle align label with input text, vertical middle align label with checkbox and checkbox label. Apply this to all activities that have form to look professional and standard.
- [ ] Test: a widget snapshot proves a hovered button keeps its rect, and that a text field, a dropdown and a checkbox row share one baseline
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — hover every toolbar and inline button in three activities — nothing grows; the Home "What do you want to do?" field matches Settings -> Accessibility -> Language
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0027 · Toolbar strip standard: sizing, label and icon colour, labelled icon buttons, sub-rows
- **State:** Pending ·
- **Scope:** `hs_ui::toolbar` and every activity's toolbar and sub-toolbar.
- **Parent:** CR-0025
- **Depends on:** CR-0026
- [ ] Label all icon buttons. For example, in Scratchpad Activity -> Scratchpad Sidebar Column -> Header; Add label "Add Folder" for Make a folder button.
- [ ] In All Activities -> Toolbar Strip -> Label and Icon; Please color the label and icon with suitable and appropriate color (depends on mode dark/light; maybe follow accent, harmony, contrast, etc where do you think is professional and acceptable), that Showing it is a toolbar and important.
- [ ] In All Activities -> Toolbar Strip -> Toolbar Button With Label; make all button slightly bigger (medium) with top/bottom/left/right padding/margin, to look proper button. 
- [ ] In All Activities -> Toolbar Strip -> Status Label with Icon (Connected, Disconneted, etc); make all icon and text slightly bigger (medium) with top/bottom/left/right padding/margin, to look proper button. 
- [ ] In All Activities -> Sidebar Column -> Button With Label; make all button slightly bigger (medium) with top/bottom/left/right padding/margin, to look proper button.
- [ ] In Editor Activity -> Work Pane -> Toolbar Strip; When User Click "Find" button, add Label for all buttons (Match Case,Whole Word, Regular Expression, Previous and Next button. Apply it to all other Activities toolbar, sub toolbar that have same characteristic of these button.
- [ ] In Terminal Activity -> Work Pane -> Toolbar Strip; When User Click "Find" button, why the other button (Older Match, Newer Match, Find text field, Match Case, Regular Expression, etc) overlapped with the main toolbar button. It should be in a row below the main toolbar row.
- [ ] In Terminal Activity -> Work Pane -> Toolbar Strip; When User Click "Find" button, add Label for all buttons (Older Match, Newer Match, Match Case, Regular Expression). Apply it to all other Activities toolbar, sub toolbar that have same characteristic of these button.
- [ ] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Toolbar button; Please make a proper button for Action (Run, Count, New Document, Delete Matching, etc) with top/right/bottom/left padding/margin and add proper icon for each button if none. 
- [ ] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Toolbar button; Please make a proper label for page button (The page before this one, The next page).
- [ ] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Toolbar button; Please make a proper button for Result Format (Table, Tree, Json, etc) with top/right/bottom/left padding/margin and add proper icon for each button if none. 
- [ ] In Redis Activity -> Work Pane; Please add all the button in this Activity with top/right/bottom/left padding/margin and add proper icon for each button if none. (Clear, Bulk Ops - Delete, Expire, Persist, Rename Prefix, Apply. etc)
- [ ] In Logs Activity -> Work Pane -> Toolbar Strip; Put AI Assistant button on the first row of the toolbar strip.
- [ ] In Logs Activity -> Work Pane -> Toolbar Strip; Draw the horizontal line separator in first row and second row full width. Apply this to all AI Assistant button and horizontal line that have a same characteristic in other activities.
- [ ] In Browser Activity -> Sidebar Column -> Favorites Tab; Add label for "New Folder" on the button with the icon.
- [ ] In Browser Activity -> Sidebar Column -> Passwords Tab; Add label for "New Login" on the button with the icon.
- [ ] Test: the toolbar builder emits a label for every button, and a sub-toolbar row lays out below the main row rather than inside it
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — the Terminal and Editor Find rows sit below the main toolbar with labels; Logs draws the AI assistant on the first row
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0028 · Table and scroll-area standard: cell sizing, emphasised headers, key/value tables
- **State:** Pending ·
- **Scope:** `hs_ui::table::DataTable`, the shared key/value renderer and every scrollable text area.
- **Parent:** CR-0025
- **Depends on:** CR-0026
- [ ] In All Activities -> All Work Pane -> Table Component (Where applicable); Make the table size and value inside it slightly bigger (cell, row, etc). Make all table header text and row, emphasized (bold, upsize, fill header cell color, etc) to show it is a header (where applicable).
- [ ] In All Activities -> All Work Pane -> All Vertical Scrollable Text Area (Status, Logs, etc) put a darker background (for dark mode) in the text area, or light background (for light mode) in the text area (Where applicable). So that user will notice it can be scrollable.
- [ ] In All Activities -> All Work Pane -> All Item And Value Data (e.g Type   Bug, Author     Redmine Admin, etc); Make a proper table without header for it to look professional, tidy and structured.
- [ ] In NoSQL Activity -> Work Pane -> Result Header; Make the header icon and label slightly bigger to show it is a header.
- [ ] In Devtools Activity -> Listening Port -> Make the Listening Port list background darker.
- [ ] In Settings -> Browser -> Extra Block Hosts Card; Do not make the text area auto expanded. Use fix height and vertical scrollable for the text area.
- [ ] Test: DataTable renders an emphasised header row, and the key/value helper produces a two-column headerless table
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — a wide table in Database and Tracker reads as a table; the Devtools listening-port list and the Settings -> Browser block-hosts box show their scroll background
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0029 · Tab strip heights, icon sizing and position-aware context menus
- **State:** Pending ·
- **Scope:** `hs_ui::tabs`, the Settings -> Appearance -> Tab Card, every main and inner tab strip.
- **Parent:** CR-0025
- [ ] In All Activities -> Main Tab Strip -> Make the tab size slightly higher, and have a setting for it in Settings -> Appearance -> Tab Card -> Minimum Main Tab Height.
- [ ] In All Activities -> Main Tab Strip -> Make the icon,title and close button in the tab slightly bigger. The size will be suitable with the main tab height settings.
- [ ] In All Activities -> Sub/Child/Inner Tab Strip (Where applicable) -> Make the tab size slightly higher but not high than main tab height, and have a setting for it in Settings -> Appearance -> Tab Card -> Minimum Sub/Child/Inner Tab Height.
- [ ] In All Activities -> Sub/Child/Inner Tab Strip (Where applicable) -> Make the icon,title and close button in the tab slightly bigger but not biggeer than main tab icon and text size. The size will be suitable with the Sub/Child/Inner tab height settings.
- [ ] In All Activities -> Main Tab Strip; If the Tab is the first after the fixed/permanent tab, remove right click menu "Close To The Left". If no other tab at right of the tab, remove right click menu "Close Others", "Close To The Left", "Close To The Right". If there is a tab or more at right of the tab, remove right click menu "Close To The Left". If a tab is the last tab on the right of the strip with some other non fixed/permanent/pinned tab on its left, remove right click menu "Close To The Right". Apply to all Activities where applicable.
- [ ] In Scratchpad Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Editor Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Git Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Terminal Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In SSH Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Container Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Database Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In NoSQL Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Redis Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In S3 Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Queue Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In API Client Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Automation Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Tracker Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Logs Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Graphics Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Devtools Activity -> Main Windows Tab Strip -> Fixed/Permanent Far Left Tab; Remove "Close to the Right" in right click menu but remain "Close Others" menu.
- [ ] In Browser Activity -> Inner Tab Strip; If the tab is the first left, remove the right click menu "Close To The Left" menu and "Close To The Right" menu. If the tab is the last most right, remove the right click menu "Close To The Right" menu. Please use suitable logic on this for every tab position right click menu.
- [ ] In Settings Page -> Main Windows Tab Strip; Remove "New Tab" button and remove right click menu for the fixed tab of Settings.
- [ ] Test: the context-menu builder omits Close To The Left for the first closable tab and Close To The Right for the last, and the height settings round-trip through the workspace file
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — change both heights in Settings -> Appearance -> Tab Card and watch every strip follow; right-click tabs at each position in three activities
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0030 · Sidebar column header standard across every activity
- **State:** Pending ·
- **Scope:** Every activity's sidebar header; the Container header is the reference.
- **Parent:** CR-0025
- **Depends on:** CR-0027
- [ ] In Scratchpad Activity -> Sidebar Column -> Header; Add label for "Add Folder" and "Add Note" on the buttons with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style in same row but add horizontal separator between the buttons and Search Note Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] In Scratchpad Activity -> Sidebar Column -> Header; When user click "Add Folder" button, make the Add Note form in the same section with the Add Folder and Add Note button, but hide those 2 button. When after user rename or cancel, hide the form and show back the 2 buttons. Make a horizontal separator between with Add Folder/Add Note button and the Search Note with top/bottom margin/padding.
- [ ] In Scratchpad Activity -> Sidebar Column; Color the pin icon for Pinned Noted (prefer red).
- [ ] In Editor Activity -> Sidebar Column -> Header; Add label for "Add Project Folder" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add Project Folder" and Project Folder Saved List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] In Editor Activity -> Sidebar Column -> Header; Add label "Add Project Folder" for add project folder button.
- [ ] In Git Activity -> Sidebar Column -> Header; Add label for "Add Repository" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add Repository" and Repository Saved List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] In Git Activity -> Sidebar Column -> Header; Add label "Add Repository" for add repository button.
- [ ] In SSH Activity -> Sidebar Column -> Header; Add label for "Add Host" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add Host" and SSH Saved Connection List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] In Container Activity -> Sidebar Column -> Header; Add top/bottom margin/padding between Header separator and "Add Container" button. Add horizontal separator between the "Add Container" and Container Saved Connection List Section (with top/bottom margin/padding) and remove the Second "Containers" subtitle.
- [ ] In Database Activity -> Sidebar Column -> Header; Change the header title from "Database Connections" to "Database". Add label for "Add Database" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add Database" and Database Saved Connection List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] In NoSQL Activity -> Sidebar Column -> Header; Add label for "Add NoSQL" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add NoSQL" and NoSQL Saved Connection List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] In Redis Activity -> Sidebar Column -> Header; Add label for "Add Redis" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between the "Add Redis" and Redis Saved Connection List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] In S3 Activity -> Sidebar Column -> Header; Add label for "Import" and "Add S3 Profile" on the buttons with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style in same row but add horizontal separator between the buttons and S3 Saved Connection List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] In Queue Activity -> Sidebar Column -> Header; Add label for "Add Broker" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between Add Broker Button and Broker Saved List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] In Automation Activity -> Sidebar Column -> Header; Add label for "Add Server" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between Add Server Button and Server Saved List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] In Tracker Activity -> Sidebar Column -> Header; Add label for "Add Tracker" on the button with the icon. Follow like in Container Sidebar Column the position of the button (with top/bottom margin/padding with the header separator) and the button style but add horizontal separator between Add Tracker Button and Tracker Saved List Section (with top/bottom margin/padding) and without the Second "Containers" title.
- [ ] Test: the shared sidebar-header helper draws a labelled Add button and one separator, and no activity emits a second title
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — walk all twelve sidebars — one titled header, labelled Add buttons, no duplicated title; Scratchpad's inline Add Folder form replaces the buttons while open
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### FEAT-0093 · Add-and-focus: a Settings jump target per kind, and Save opens the new connection
- **State:** Pending ·
- **Scope:** `Services`/`OpenRequest` navigation, the Settings pages and every Add button.
- **Parent:** CR-0025
- **Depends on:** CR-0030
- [ ] In Almost all Activity Connection -> Sidebar Column; When User click button Add Activity's New Connection, straight away go to Settings -> Connections / Containers / Queue / Automation / Tracker / Git, with add new empty form to add those and the page will focus to the section. 
- [ ] In Editor Activity -> Sidebar Column -> Header; When user click "Add Project Folder" button, straight away go to Settings -> Git -> Add Repository -> Add new repository form. And will focus to that form section. 
- [ ] In Editor Activity -> Sidebar Column -> Tab Strip; When user click "New Tab", direct focus on the supposed settings page section - Settings -> Editor -> Project List -> Add Project Detail Form. Then, when it is saved, close the detail page and direct open the new tab in Editor's Activity of the new added project Folder. Please check if other new tab button for other activities connections as well.
- [ ] In Git Activity -> Sidebar Column -> Header; When user click "Add Repository" button, straight away go to Settings -> Git -> Add Repository -> Add new repository form. And will focus to that form section. 
- [ ] In Git Activity -> Sidebar Column -> Tab Strip; When user click "New Tab" button, direct focus on the supposed settings page section - Settings -> Git -> Repository List -> Add Repository Detail Form. Then, when it is saved, close the detail page and direct open the new tab in Git's Activity of the new added repository. It will also register Project In Editor Activity (Only if from Git Setting -> Repository). Please check if other new tab button for other activities connections as well.
- [ ] In Git Activity -> Main Windows Tab Strip -> When user click "New Tab" button; direct focus on the supposed settings page section - Settings -> Git -> Repository List -> Add Repository Detail Form. Then, when it is saved, close the detail page and direct open the new tab in Git's Activity of the new added repository. It will also register Project In Editor Activity (Only if from Git Setting -> Repository). Please check if other new tab button for other activities connections as well.
- [ ] In SSH Activity -> Sidebar Column -> Header; When user click "Add Host" button, straight away go to Settings -> Connections -> New Connection -> Add new connection form with Auto Select SSH in Kind dropdown. And will focus to that form section. 
- [ ] In Database Activity -> Sidebar Column -> Header; When user click "Add Database" button, straight away go to Settings -> Connections -> New Connection -> Add new connection form with Auto Select DB in Kind dropdown. And will focus to that form section. 
- [ ] In NoSQL Activity -> Sidebar Column -> Header; When user click "Add NoSQL" button, straight away go to Settings -> Connections -> New Connection -> Add new connection form with Auto Select MongoDB in Kind dropdown. And will focus to that form section. 
- [ ] In Redis Activity -> Sidebar Column -> Header; When user click "Add Redis" button, straight away go to Settings -> Connections -> New Connection -> Add new connection form with Auto Select Redis in Kind dropdown. And will focus to that form section. 
- [ ] In S3 Activity -> Sidebar Column -> Header; When user click "Add S3 Profile" button, straight away go to Settings -> Connections -> New Connection -> Add new connection form with Auto Select S3 in Kind dropdown. And will focus to that form section. 
- [ ] In Queue Activity -> Sidebar Column -> Header; When user click "Add Broker" button, straight away go to Settings -> Queue -> Brokers, and will focus to the section. 
- [ ] In Automation Activity -> Sidebar Column -> Header; When user click "Add Server" button, straight away go to Settings -> Automation -> Server, and will focus to the section. 
- [ ] In Tracker Activity -> Sidebar Column -> Header; When user click "Add Tracker" button, straight away go to Settings -> Tracker -> Tracker List, and will focus to the section. 
- [ ] In Settings -> Containers -> Add New Container; When User click Save for the detail, save and close the setting form and then open the newly added connections in new tab in respective Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [ ] In Settings -> Connections -> Add New Connection; When User click Save for the detail, save and close the setting form and then open the newly added Container in new tab in Containers Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [ ] In Settings -> Queue -> Add Broker; When User click Save the detail, save and close the setting form and then open the newly added broker in new tab in Queue Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [ ] In Settings -> Automation -> Add Server; When User click Save the detail, save and close the setting form and then open the newly added server in new tab in Automation Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [ ] In Settings -> Tracker -> Add Tracker; When User click Save the detail, save and close the setting form and then open the newly added server in new tab in Tracker Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [ ] In Settings -> Editor -> Add Project Folder; When User click Save the detail, save and close the setting form and then open the newly added project in new tab in Editor Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [ ] In Settings -> Git -> Add Repository; When User click Save the detail, save and close the setting form and then open the newly added project in new tab in Editor Activity. In the same time, It will also create a Project Folder of the repository in Editor Activity. Close button will close the form. Please check if other connection can follow this flow and implement it where applicable.
- [ ] In Settings -> Git -> Add Credential; When User click Save the detail, save and close the setting form. Close button will close the form.
- [ ] Test: a jump request resolves to the right Settings page, section and empty form for each kind, and a Save event carries the new id to its activity
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — from every activity press Add, land on a focused empty form, save it, and see the new connection open in a tab; Close closes the form
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### FEAT-0094 · Remove From List: delete a saved profile and its credential from the sidebar
- **State:** Pending ·
- **Scope:** Nine activity sidebars, the connection store and the vault.
- **Parent:** CR-0025
- [ ] In SSH Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the Connection profile with credential tied to it and confirmation dialog will appear before.
- [ ] In Containers Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Host" menu. It will delete the host profile with credential tied to it and confirmation dialog will appear before.
- [ ] In Database Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [ ] In NoSQL Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [ ] In Redis Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [ ] In S3 Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [ ] In Queue Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [ ] In Automation Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the connection profile with credential tied to it and confirmation dialog will appear before.
- [ ] In Tracker Activity -> Sidebar Column -> Right Click On The List; Add "Remove From List" menu below "Edit This Connection" menu. It will delete the tracker profile with credential tied to it and confirmation dialog will appear before.
- [ ] Test: removing a profile deletes its `secret:<ulid>` from the vault, the data folder greps clean afterwards, and a cancelled confirmation changes nothing
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — remove a saved profile in SSH and Database, confirm the dialog, and check Settings -> Security -> Credentials no longer lists its credential
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### BUG-0038 · Editor -> Source Control -> Refresh Status does nothing
- **State:** Pending ·
- **Scope:** The Editor work pane's Source Control tab.
- **Parent:** CR-0025
- [ ] In Editor Activity -> Work Pane -> Folder Sidebar -> Source Control Tab; Refresh Status button not working.
- [ ] Test: a reproducing test: Refresh Status re-runs `git status` and replaces the cached entries
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — edit a file in a git project, press Refresh Status, and watch the list change
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0035 · Editor work pane and Source Control tab layout
- **State:** Pending ·
- **Scope:** The Editor work pane: folder sidebar, toolbar and Source Control tab.
- **Parent:** CR-0025
- **Depends on:** CR-0027, FEAT-0084
- [ ] In Editor Activity -> Work Pane -> Folder Sidebar -> Header; Remove Open folder as another project and close this project buttons. Add button with action before "Add File", named "Add Folder", where user can add folder inside the project directory
- [ ] In Editor Activity -> Work Pane -> Folder Sidebar; When resize the sidebar column, make the header title (Project) and buttons (New Folder, New File, Refresh), responsive.
- [ ] In Editor Activity -> Work Pane -> Toolbar Strip; Remove Open Folder button and Open File button.
- [ ] In Editor Activity -> Work Pane -> Folder Sidebar -> Source Control Tab; When the project folder is not a git, do not show the Error "git status: fatal: detected dubious ownership...To add an exception for this directory, call:git config --global --add safe.directory D:/WORKSPACES/DOTNET/flex-taskbar" Show proper error like (Not a Git repository). And disable the "Show this repository History" unless it is a git project folder.
- [ ] In Editor Activity -> Work Pane -> Folder Sidebar -> Source Control Tab; Fix the header icon (folder icon for folder name, move the branch icon left beside the branch name). 
- [ ] In Editor Activity -> Work Pane -> Folder Sidebar -> Source Control Tab; Please have a proper top/bottom margin/padding between sections: Toolbars (Fetch, Pull, Push, Stashes, Branches, Tag), Commit Message Text Area, Prefix Button, Commit Group (Commit button, Commit and Push button , Ammend the Last Commit checkbox).
- [ ] In Editor Activity -> Work Pane; When user open file to edit, make the tab for the file (inner tab), follow the colored material icon (If material Icon enabled in settings) or generic icon on the file list column.  
- [ ] Test: a non-git folder yields the "Not a Git repository" state with history disabled instead of a raw git error
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — open a non-git folder and a git one; add a folder from the sidebar header; narrow the sidebar and watch the header stay readable
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0036 · Multi-coloured product icons for every connection kind
- **State:** Pending ·
- **Scope:** `hs_ui::icons` and every list, tab and header that draws a connection icon.
- **Parent:** CR-0025
- [ ] In All Activities -> Every connection icon, use multi-colored icon, same with the actual product icon color. It will reflect to all component that use the icon (Connection lists, tabs, etc)
- [ ] In Git Activity -> Work Pane Header -> Repostory Title Icon; Use the icon that been save in the Git Repository Settings, same like the repository list icon and the tab icon for the repository. Don't use generic icon.
- [ ] Test: each connection kind resolves to its product icon, and the same icon is returned for the sidebar, the tab and the work-pane header
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — every connection list, tab and header shows the product's own colours in both themes
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0031 · Spacing pass: Redis, S3 and Queue work panes
- **State:** Pending ·
- **Scope:** The Redis, S3 and Queue work panes.
- **Parent:** CR-0025
- **Depends on:** CR-0027, CR-0028
- [ ] In Redis Activity -> Work Pane -> Keys Pane; Please add top/bottom padding/margin between below Header separator and Action Button (Reload, Rename, Set TTL, Delete, Add Member). Make the button proper with top/right/bottom/left padding/margin. 
- [ ] In Redis Activity -> Work Pane -> Keys Pane; Please add top/bottom padding/margin between Action Button (Reload, Rename, Set TTL, Delete, Add Member, etc) and Filter row.
- [ ] In Redis Activity -> Work Pane -> Keys Pane; Please add top/bottom padding/margin between Filter row and Data Table.
- [ ] In Redis Activity -> Work Pane -> Pub/Sub Pane; Please add top/bottom padding/margin between below Header separator and Subscribe row. 
- [ ] In Redis Activity -> Work Pane -> Pub/Sub Pane; Please add top/bottom padding/margin between Subscribe row and Publish row. 
- [ ] In Redis Activity -> Work Pane -> Pub/Sub Pane; Please add top/bottom padding/margin between publish row and message section. 
- [ ] In Redis Activity -> Work Pane -> Streams Pane; Please add top/bottom padding/margin between below Header separator and Stream row. 
- [ ] In Redis Activity -> Work Pane -> Tools Pane; Please add top/bottom padding/margin between below Header separator and Toolbars button (Slow Log, Client List, Big Keys, Bulk Ops, etc). 
- [ ] In Redis Activity -> Work Pane -> Tools Pane -> Slowlog tab; Please add top/bottom padding/margin between Toolbars button (Slow Log, Client List, Big Keys, Bulk Ops, etc) and Action button (Reload, etc). 
- [ ] In Redis Activity -> Work Pane -> Tools Pane -> Slowlog tab; Please add top/bottom padding/margin between Action button (Reload, etc) and slow log message. 
- [ ] In Redis Activity -> Work Pane -> Tools Pane -> Client List tab; Please add top/bottom padding/margin between Toolbars button (Slow Log, Client List, Big Keys, Bulk Ops, etc) and Action button (Reload, etc). 
- [ ] In Redis Activity -> Work Pane -> Tools Pane -> Client List tab; Please add top/bottom padding/margin between Action button (Reload, etc) and Client list table. Make that table proper, follow other activity's connections standard. 
- [ ] In Redis Activity -> Work Pane -> Tools Pane -> Big Keys tab; Please add top/bottom padding/margin between below Toolbars button (Slow Log, Client List, Big Keys, Bulk Ops, etc) and Pattern row (Find Big Keys, etc). 
- [ ] In Redis Activity -> Work Pane -> Tools Pane -> Big Keys tab; Please add top/bottom padding/margin between Pattern row (Find Big Keys, etc) and big keys table. Make that table proper, follow other activity's connections standard. 
- [ ] In Redis Activity -> Work Pane -> Tools Pane -> Bulk Ops tab; Please add top/bottom padding/margin between Toolbars button (Slow Log, Client List, Big Keys, Bulk Ops, etc) and Action button (Delete, Expire, Persist, Rename Prefix, etc). 
- [ ] In Redis Activity -> Work Pane -> Tools Pane -> Bulk Ops tab; Please add top/bottom padding/margin between Action button (Delete, Expire, Persist, Rename Prefix, etc) and Pattern Row (Pattern, Preview, etc). 
- [ ] In Redis Activity -> Work Pane -> Tools Pane -> Bulk Ops tab; Please add top/bottom padding/margin between Pattern Row (Pattern, Preview, etc) and Preview Result. 
- [ ] In Redis Activity -> Work Pane -> Tools Pane -> Bulk Ops tab; Please add top/bottom padding/margin between Preview Result and Action button (Apply). 
- [ ] In Redis Activity -> Work Pane -> Please add top/bottom padding/margin between Toolbars bottom header and Database drop down row.
- [ ] In Redis Activity -> Work Pane -> Please add top/bottom padding/margin between Database drop down row and Filter key Row (Filter filed text, Scan button, New Key button).
- [ ] In Redis Activity -> Work Pane -> Please fix Filter key Row (Filter filed text, Scan button, New Key button), where the New key button is far right near the apps windows. Please add right margin/padding for that button and apps windows border. 
- [ ] In Redis Activity -> Work Pane -> Please fix Filter key Row (Filter filed text, Scan button, New Key button) alignment by vertical align the input text and those 2 buttons.
- [ ] In Redis Activity -> Work Pane -> Please fix Filter key Row button (Scan button, New Key button) by Adding text/label next to the button icon.
- [ ] In S3 Activity -> Work Pane -> Data Column Pane; Please add top/bottom padding/margin between separator below Bucket Name Breadcrumb and Filter row. 
- [ ] In S3 Activity -> Work Pane -> Data Column Pane; Please add top/bottom padding/margin between Filter row and Data Table. 
- [ ] In S3 Activity -> Work Pane -> Data Column Pane; Please add top/bottom padding/margin between Data Table and Bucket Status (e.g. 2 items loaded). 
- [ ] In Queue Activity -> Work Pane -> Queue/Topic List Pane; Please add top/bottom padding/margin between Queue Title header and filter row.
- [ ] In Queue Activity -> Work Pane -> Queue/Topic List Pane; Please add top/bottom padding/margin between filter row and Data table.
- [ ] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Tool row (Messages, Publish, Consumer, Bindings, etc) and Topic Name.
- [ ] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Topic Name and Message Queue.
- [ ] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Message Queue and count row.
- [ ] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between count row and Peek Table.
- [ ] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Peek Table and Exchange Section.
- [ ] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Exchange Section and Payload.
- [ ] In Queue Activity -> Work Pane -> Message Pane -> Exchange Section; Please use Sentence case for Exchange header title. Use a proper table view.
- [ ] In Queue Activity -> Work Pane -> Message Pane -> Exchange Section -> Payload; Please add card/frame to put payload message inside.
- [ ] In Queue Activity -> Work Pane -> Message Pane; Please add top/bottom padding/margin between Tool row (Messages, Publish, Consumer, Bindings, etc) and Topic Name.
- [ ] In Queue Activity -> Work Pane -> Publish Pane; Please add top/bottom padding/margin between Topic Name and Exchange/Routing Key Section.
- [ ] In Queue Activity -> Work Pane -> Publish Pane; Please add top/bottom padding/margin between Exchange/Routing Key Section and Header Section.
- [ ] In Queue Activity -> Work Pane -> Publish Pane; Please add top/bottom padding/margin between Header Section and Payload Section.
- [ ] In Queue Activity -> Work Pane -> Consumers Pane; Please add top/bottom padding/margin between Topic Name and Action button row (Load).
- [ ] In Queue Activity -> Work Pane -> Consumers Pane; Please add top/bottom padding/margin between Action button row (Load) and Consumer data detail.
- [ ] In Queue Activity -> Work Pane -> Bindings Pane; Please add top/bottom padding/margin between Topic Name and Exchange/Routing Key Table. Use a proper table view.
- [ ] Test: no assertion beyond the gate: this is layout, verified on screen
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — walk every pane in Redis, S3 and Queue — sections are separated, buttons are padded, the Redis filter row is aligned and clear of the window edge
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0032 · Spacing pass: Database result and message panels
- **State:** Pending ·
- **Scope:** The Database work pane's result and message panels.
- **Parent:** CR-0025
- **Depends on:** CR-0027, CR-0028
- [ ] In Database Activity -> Work Pane -> Result And Message Section Header; Add separator between result button(s) and message button with proper left/right padding/margin.
- [ ] In Database Activity -> Work Pane -> Result Section Header Row; Please add left margin/padding on the first result button. Its too near the pane border.
- [ ] In Database Activity -> Work Pane -> Result Section Header -> Message Panel; Please insert the all the result lists : message query(1. SELECT * FROM urpv4.bank) and message result (e.g. 27 rows · 5 ms) inside a some card or frame to look professional and tidy.
- [ ] In Database Activity -> Work Pane -> Result Section Header -> Message Panel; Please add left margin/padding on the message query(1. SELECT * FROM urpv4.bank) and message result (e.g. 27 rows · 5 ms). Its too near the pane border.
- [ ] In Database Activity -> Work Pane -> Result Section Header -> Message Panel; Please add separator between result message query(1. SELECT * FROM urpv4.bank) and message result (e.g. 27 rows · 5 ms) with top/bottom margin/padding on the separator.
- [ ] In Database Activity -> Work Pane -> Result Section Header -> Result Panel -> Bottom Toolbar; Please add top/bottom padding/margin between the result datatable and toolbar below.
- [ ] In Database Activity -> Work Pane -> Result Section Header -> Result Panel -> Bottom Toolbar; Please add left margin/padding on the result stat (e.g. 27 rows · Editable (F2)) and right margin/padding on the Export button.
- [ ] Test: no assertion beyond the gate: this is layout, verified on screen
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — run a multi-statement query and read the result and message panels — cards, separators and padding on both
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0033 · Spacing pass: Automation, Tracker and Logs work panes
- **State:** Pending ·
- **Scope:** The Automation, Tracker and Logs work panes, including the Highlight Rules popup.
- **Parent:** CR-0025
- **Depends on:** CR-0027, CR-0028
- [ ] In Automation Activity -> Work Pane -> Job List Pane; Please add top/bottom padding/margin between Job Title header and filter row.
- [ ] In Automation Activity -> Work Pane -> Job List Pane; Please add top/bottom padding/margin between filter row and job list data table.
- [ ] In Automation Activity -> Work Pane -> Job Activity Pane -> Runs Tab; Please add top/bottom padding/margin between Action button (Runs, Log, Run, Queue, etc) and job title.
- [ ] In Automation Activity -> Work Pane -> Job Activity Pane -> Runs Tab; Please add top/bottom padding/margin between job title and Job Activity data table.
- [ ] In Automation Activity -> Work Pane -> Job Activity Pane -> Runs Tab; Please add top/bottom padding/margin between Job Activity data table and Job Status (e.g. #4 · Running. Double-click a run to read its log.).
- [ ] In Automation Activity -> Work Pane -> Job Activity Pane -> Log Tab; Please add top/bottom padding/margin between job title and job status (e.g. #5 · Success).
- [ ] In Automation Activity -> Work Pane -> Job Activity Pane -> Log Tab; Please add top/bottom padding/margin between job status (e.g. #5 · Success) and job log text area.
- [ ] In Tracker Activity -> Work Pane -> Ticket List Pane; Please add top/bottom padding/margin between below Header separator and query row (Query text input, Run This QUery, Saved Queries,etc). 
- [ ] In Tracker Activity -> Work Pane -> Ticket List Pane; Please add top/bottom padding/margin between query row (Query text input, Run This Query, Saved Queries,etc) and Item count (e.g. Items 3 of 3).
- [ ] In Tracker Activity -> Work Pane -> Ticket List Pane; Please add top/bottom padding/margin between Item count (e.g. Items 3 of 3) and filter row.
- [ ] In Tracker Activity -> Work Pane -> Ticket List Pane; Please add top/bottom padding/margin between filter row and Ticket data table.
- [ ] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add horizontal separator line below Ticket details header.
- [ ] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add top/bottom padding/margin between below Ticket details separator and ticket detail table (e.g. Project    Demo, Type     Bug, etc). 
- [ ] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add horizontal separator line below Ticket details table.
- [ ] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add top/bottom padding/margin between below Ticket details table separator and Ticket Descriptions (e.g. The disk is full.Rotate logs daily.. , etc).
- [ ] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add horizontal separator line below Ticket Descriptions.
- [ ] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add top/bottom padding/margin between below Ticket Descriptions separator and Ticket Comment List.
- [ ] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add horizontal separator line below Ticket Comment List.
- [ ] In Tracker Activity -> Work Pane -> Ticket Detail Pane; Please add top/bottom padding/margin between below Ticket Comment List separator and Comment Form.
- [ ] In Tracker Activity -> Work Pane -> Edit Ticket Pane; Please add top/bottom padding/margin between due date row and description form.
- [ ] In Tracker Activity -> Work Pane -> New Item Ticket Pane; Please add top/bottom padding/margin between summary row and description form.
- [ ] In Logs Activity -> Work Pane -> Logs Pane; Resize the Record column to most suitable size, because its too wide. auto resize it to match value in it.
- [ ] In Logs Activity -> Work Pane -> Logs Pane; Please add top/bottom padding/margin between log record list and Record Status (e.g. Record 9, Copy, Bookmark, Open In Editor, etc).
- [ ] In Logs Activity -> Work Pane -> Logs Pane; Please add top/bottom padding/margin between Record Status (e.g. Record 9, Copy, Bookmark, Open In Editor, etc) and Log detail text area.
- [ ] In Logs Activity -> Work Pane -> Highlight Rules Popup; Please make this professional, tidy and structured. It look messy (vertical middle align, short regex field, padding/margin between checkbox and input field, etc).
- [ ] Test: the Logs record column sizes to its content rather than to the pane
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — walk the Automation job panes, the Tracker ticket detail and the Logs pane and popup
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0034 · Container and SSH work pane layout
- **State:** Pending ·
- **Scope:** The Container work panes (Docker, Kubernetes, Podman, Windows) and the SSH shell pane.
- **Parent:** CR-0025
- **Depends on:** CR-0027
- [ ] In SSH Activity -> Shell Pane; Make Tunnel Card and Monitor Card have some color to differentiate with the shell terminal.
- [ ] In SSH Activity -> Shell Pane -> Monitor Card; Add proper icon to the Items (Cpu, Load, Memory, Disk, Up and Top) and color both the icon and lavel with proper color
- [ ] In Container Activity -> Docker Work Pane -> Docker Summaries (Running, Stopped, Images, Volumes, Network), Follow the Dashboard summary format (rounded box, icon with lable below, horizontal align center, vertical align middle, same max box size, etc). Apply these in kubernetes, podman, windows container too (Where applicable).
- [ ] In Container Activity -> Work Pane -> Container List -> Select a container -> Below Detail Panel; Make a proper button for Ports (Open In Browser activity, etc) with top/right/bottom/left padding/margin. 
- [ ] In Container Activity -> Work Pane -> Container List -> Select a container -> Below Detail Panel; Make a proper button for Action (Logs, Shell, Restart, Stop, Kill, Remove) with top/right/bottom/left padding/margin and add proper icon for each button if none. Please check other activities that have same characteristic of these button and apply to it also.
- [ ] In Container Activity -> Sidebar Panel -> Saved Command And Config Section; Add a sample for saved command and config, so that user can reference it later and can delete it later.
- [ ] Test: the Docker summary row uses the shared `hs_ui::dashboard` summary box
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — Docker summaries match the dashboard language; port and action buttons are real buttons; SSH tunnel and monitor cards stand out from the terminal
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0039 · Spacing pass: NoSQL work pane
- **State:** Pending ·
- **Scope:** The NoSQL work pane: Find, Aggregate, Index and their detail panels.
- **Parent:** CR-0025
- **Depends on:** CR-0027, CR-0028
- [ ] In NoSQL Activity -> Work Pane -> Find Pane; Please add top/bottom padding/margin between below Header separator and Search Criteria (Filter, Project, Sort).
- [ ] In NoSQL Activity -> Work Pane -> Find Pane; Please add top/bottom padding/margin between Search Criteria (Filter, Project, Sort) and buttons (Run, Count, New Document, Delete Matching, etc).
- [ ] In NoSQL Activity -> Work Pane -> Aggregate Pane; Please add top/bottom padding/margin between below Header separator and Json Array Label and Text Area.
- [ ] In NoSQL Activity -> Work Pane -> Aggregate Pane; Please add top/bottom padding/margin between Json Array Text Area and buttons (Run, etc).
- [ ] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane; Please add top/bottom padding/margin between buttons (Run, Count, New Document, Delete Matching, etc) and Result format (Table, Tree, Json, etc).
- [ ] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane; Please add top/bottom padding/margin between Result format (Table, Tree, Json, etc) and Result table.
- [ ] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Detail Panel Below; Please add top/bottom padding/margin between id row and data view text area. 
- [ ] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Detail Panel Below; Please add top/bottom padding/margin between data view text area and action button below (Save, Delete, Cancel) 
- [ ] In NoSQL Activity -> Work Pane -> Find and Aggregate Pane -> Detail Panel Below; Please make a proper button for Action Button (Save, Delete, Cancel) with top/right/bottom/left padding/margin and add proper icon for each button if none. 
- [ ] In NoSQL Activity -> Work Pane -> Index Pane -> Detail Panel Below; Please add top/bottom padding/margin between below Header separator and Table (Name, Definition, Unique). Please make it as a proper table with header, column and row line.
- [ ] Test: no assertion beyond the gate: this is layout, verified on screen
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — walk the Find, Aggregate and Index panes and their detail panels
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### FEAT-0095 · Server-populated fields: Tracker and Automation
- **State:** Pending ·
- **Scope:** The Tracker edit, new-item and change-status panes; the Automation Run tab.
- **Parent:** CR-0025
- **Depends on:** CR-0026
- [ ] In Automation Activity -> Work Pane -> Job Activity Pane -> Run Tab; Make the Ref Selection as a drop down menu where it will populate branch from the automation server. Apply this to all activities that a text field but the value can be auto populate making it a drop down menu.
- [ ] In Automation Activity -> Work Pane -> Job Activity Pane -> Log Tab -> Job Log text area; Please pretify the log in proper format (indent, blank line, etc) for user to know and differentiate each step of the runs.
- [ ] In Tracker Activity -> Work Pane -> Edit Ticket Pane; Please make Assignee text field and Priority text field to dropdown menu where the data auto populate from the server.
- [ ] In Tracker Activity -> Work Pane -> Edit Ticket Pane; Please make Due Date text field to date calendar select menu where the date format will auto format follow server date format when selecting date.
- [ ] In Tracker Activity -> Work Pane -> New Item Ticket Pane; Please make Type dropdown list and Priority drop down list auto populate from the server.
- [ ] In Tracker Activity -> Work Pane -> Change Status Ticket Pane; Please make a proper UI UX on this to look professiona, tidy and structured.
- [ ] Test: a populated dropdown falls back to a free-text field when the server cannot be reached, and the date field formats to the server's format
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — edit a Redmine ticket — Assignee, Priority and Type come from the server, Due Date opens a calendar; run a Forgejo job and pick a ref from the list
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### FEAT-0096 · Database: results retained per query tab, and double-click a cell to edit
- **State:** Pending ·
- **Scope:** The Database work pane's query tabs and result data table.
- **Parent:** CR-0025
- [ ] In Database Activity -> Work Pane -> Query Tab; Every query tab persist its own results data. Results/Multi-Result per Query Tab. If user click other tab, it can view the result/multi-result last run by the query tab.
- [ ] In Database Activity -> Work Pane -> Result Data Table; When user double click on the cell, show edit popup for user edit the value in that cell, same like user right click menu > "Edit Cell".
- [ ] Test: switching query tabs restores each tab's own result set and messages rather than the last run's
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — run different queries in three tabs, switch between them, then double-click a cell and edit it (synthetic clicks never form double-clicks, so this box needs the app)
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### FEAT-0097 · S3: a collapse/expand folder hierarchy column
- **State:** Pending ·
- **Scope:** The S3 work pane's folder column and breadcrumb.
- **Parent:** CR-0025
- **Depends on:** FEAT-0084
- [ ] In S3 Activity -> All Connection -> Folder List Column; Show folder as hierarchy navigation style (collapse/expand), instead of current navigation style. Don't display Up button on the folder hierarchy. User can use Up button on the toolbar.
- [ ] In S3 Activity -> Work Pane -> Data Column Pane; Please add icon for child breadcrumb (e.g. hs-raw > docs). Use colored material icon.
- [ ] Test: the hierarchy builds lazily from the delimiter-listed prefixes and keeps expansion state across a refresh
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — browse a bucket with nested prefixes — expand and collapse in place, no Up button in the tree, coloured breadcrumb icons
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### FEAT-0098 · SSH: name a tunnel when creating it
- **State:** Pending ·
- **Scope:** The SSH sidebar's Tunnels section and the stored host profile.
- **Parent:** CR-0025
- [ ] In SSH Activity -> Sidebar Column -> Tunnels; When Create Tunnel, allow naming it, so that user can easy to references it in tunnel list. In tunnel list, 
- [ ] Test: a named tunnel round-trips through the workspace file and the name is what the list shows
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — create two tunnels on one host with names and tell them apart in the list
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0037 · Redis key list sorted folders first
- **State:** Pending ·
- **Scope:** The Redis work pane's key list column.
- **Parent:** CR-0025
- [ ] In Redis Activity -> Work Pane -> Key List Column; Sort the key list by parent folder first, key without parent, then by name for both.
- [ ] Test: the sort puts parent folders first, then keys without a parent, each group by name
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — scan a database with mixed foldered and bare keys and read the order
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

### CR-0038 · Settings polish, and the UI standard written down
- **State:** Pending ·
- **Scope:** Settings -> Security, Settings -> Appearance, the database connection form, `.claude/rules/ui.md` and `docs/04-ui-design.md`.
- **Parent:** CR-0025
- **Depends on:** CR-0026, CR-0027, CR-0028, CR-0029, CR-0030
- [ ] In Settings -> Security -> Credentials; Make the Credentials Card wider, so that user need less horizontal scrolling.
- [ ] In Settings -> Security -> Credentials; Remove Key column if not being used.
- [ ] In Settings Page -> Database Connections; State the guide and requirement to use oracle client and db2 cli if user select either both in DB Engine.
- [ ] Make a Settings -> Appearance -> Activity Rail for Home -> Dashboard Page -> Dashboard Toolbar -> Activity Icon, user can show/hide label of activity (below icon). Make standard width for all the icon with left/right margin/padding if enabled.
- [ ] Create standard rules of all above items for agent reference for building next activity, features, function, etc where applicable.
- [ ] Test: the bundled locale parses and the Oracle/Db2 requirement text is present for those engines
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — the credentials card needs no horizontal scrolling; the activity rail label toggle takes effect on the Home dashboard
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS

---

## v0.7.5 — In-Progress

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
- **State:** In-Progress · **Spec:** docs/tools/automation.md · **Phase:** K
- **Branch:** feat/depth-k-automation
- **Depends on:** FEAT-0077
- [x] build configurations with last build; builds; parameters
- [x] trigger through the build queue with properties; cancel; the queue; the build log
- [x] bearer token or basic
- [x] Test: `teamcity.rs` (build state, dates, trigger user, cancelled); `mock.rs::teamcity_lists_triggers_and_cancels_through_the_build_queue` (the documented endpoints and bodies, against a mock server)
- [x] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — a `hs-teamcity` container: trigger, follow the log, cancel
- [x] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** TeamCity's first start stops at a setup wizard before its REST API answers, so no bench server: the App box stays open. The mock answers are written from the REST API reference, not captured from a server.

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

### FEAT-0092 · Puppet (PuppetDB, Puppet Enterprise orchestrator)
- **State:** Pending · **Spec:** docs/tools/automation.md · **Phase:** K
- **Depends on:** FEAT-0077
- [ ] nodes with the last report's status and time; a node's reports; a report's log lines
- [ ] Run offered only with a Puppet Enterprise orchestrator URL (deploy one node)
- [ ] `X-Authentication` RBAC token as api-key
- [ ] Test: <what it asserts>
- [ ] Gate: fmt · clippy -D warnings · test --workspace -j 4 · --self-test
- [ ] App: `--data-dir ./tmp-data` — a `hs-puppetdb` container if it fits in memory, otherwise recorded responses
- [ ] Docs: spec · README · CHANGELOG · CHECKLIST · GOTCHAS
- **Notes:** deferred out of v0.7.5 on the owner's decision (2026-10-31). Puppet Enterprise is licensed and cannot run here; PuppetDB could run in a container.

---

## v0.7.4 — In-Progress

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
- **Notes:** no replication column: `rskafka` lists partitions only (spec "What is implemented"). The TLS / SASL box stays open: the code is written (rskafka's SASL config; SCRAM in our groups client tested against RFC 7677's exchange) but no TLS or SASL broker was run here.

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
- **Notes:** lag per partition works; the progress and Stop are in the Consumers pane rather than a shell Job (spec §7 updated) and were not seen on screen, because one group answers too fast to catch. TLS / SASL as FEAT-0071.

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
