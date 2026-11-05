# Tool spec — the Git activity (`tool-git`)

Owner's requirement, 2026-10-29. This document is the specification for
the **Git activity**: a sidebar activity of its own, separate from the
Git view inside the editor ([git.md](git.md) §3a/§3b, which stays as it
is). The editor's Git view answers "what did I change in the file I am
editing"; the Git activity answers "manage this repository", the way
SourceTree, GitKraken or Fork do, for repositories that need not be open
in the editor at all.

The owner gave leave to enhance, modify and add what suits the activity;
every such addition is marked *(addition)* below so the requirement and
the design stay distinguishable.

## 1. Placement

- **Activity rail**: a `Git` entry immediately after `Editor`
  (`crates/hyperstack/src/tools/mod.rs` registers `GitFactory` right
  after `EditorFactory`; `rail_order` appends it after Editor for a fresh
  layout and at the end of a saved order, which Settings → Appearance
  can move). Icon `ph::GIT_BRANCH`, colour `#f0883e` (git's orange, no
  other activity uses it).
- **Settings**: a `Git` page between `Editor` and `Browser`
  (`settings_ui::Page::Git`, key `settings-git`).
- **Tabs**: a permanent home tab that is the dashboard (§6); one working
  tab per repository, titled with the repository's name, icon from the
  entry.

## 2. Stores

### 2.1 Per-workspace repository list — `hs_core::git_repos`

`<workspace>/git-repos.toml`, modelled on `projects.toml`
(`hs_core::projects`): one array, order is the point, **a list of
references that never owns a folder**. Removing an entry forgets it;
nothing in the module can delete a directory.

```toml
schema = 1
[[repo]]
id = "01J…"
name = "hyper-stack"
icon = ""                 # a glyph key from the icon picker, or "" for the default
source = "remote"         # "local" | "localhost" | "remote"
host_url = "https://git.zacx.net"
project = "owner/hyper-stack"   # the host's project path, when fetched from a host
url = "https://git.zacx.net/owner/hyper-stack.git"
path = 'D:\WORKSPACES\RUST\hyper-stack'
credential = "01J…"       # id of a global Git credential, or ""
order = 3
language = "rust"         # cached detection, as projects.toml does
```

`Services.git_repos: Arc<RwLock<GitRepos>>`, loaded at start and on a
workspace switch beside `projects`. The Git sidebar, the dashboard and
Settings → Git all read and write it.

### 2.2 Global Git credentials — `hs_core::git_hosts`

`HyperStackData/git-credentials.toml` (application-wide, like
`globals.json`): a list of `GitCredential { id, name, kind, base_url,
username, token }`. `kind` is `github | gitlab | forgejo | gitea |
bitbucket | azure | generic`; `base_url` defaults from the kind for the
public hosts and is required for self-hosted ones. **`token` is a
`secret:<ulid>` vault reference, never the token** - the Settings form
runs the typed value through `store_typed_secrets` and refuses to save
while the vault is locked, exactly as a connection profile does. The
token is handed to `git` through `GIT_ASKPASS` pointing at a one-shot
helper the app writes under `HyperStackData/tmp/` for the duration of
the command (never into the URL, never into `.git/config`), and to the
host's REST API as a bearer header for **Fetch Project**.

## 3. Settings → Git

Three cards, in the shared `hs_ui::form` style:

1. **General** *(addition)*: Default Clone Folder (where a fetched
   project is cloned unless the form says otherwise); Protected Branch
   Patterns (`main`, `master`, `release/*` - a push or force action onto
   one asks first, `hs_git::write::is_protected`); Git Flow prefixes
   (`feature/`, `release/`, `hotfix/`, develop/production branch names);
   Terminal Profile for Open Terminal (default: the workspace's default
   shell, pwsh on Windows).
2. **Git Credentials**: master list (name, kind, host) with Add / Remove
   and a detail form (Name, Kind, Host URL, Username, Token). **Test**
   calls the host's `whoami` endpoint (`/user`, `/api/v4/user`,
   `/api/v1/user`) and reports the login it answers with.
3. **Repositories**: master/detail. The list has Add Repository, Remove
   and a search box; rows show icon, name and path. The detail form:
   - **Source**: Local Folder (Browse…), Localhost (a bare repository or
     `file://` path on this machine), Remote (host).
   - **Name**, **Icon** (a picker over the Phosphor set plus the Nerd
     Font brand marks, with the host's avatar as the default when the
     project came from a host - *addition*, phase 6).
   - **URL / Host**: for Remote, the host URL comes from the chosen
     **Credential** (dropdown over §2.2) and can be overridden.
   - **Fetch Project**: asks the host API for the projects the
     credential can see (paged, cached per credential for the session)
     and fills the searchable **Project** dropdown (fuzzy, matching
     `owner/name`, description and language). Choosing one fills URL,
     Name and the default Path (`<Default Clone Folder>/<name>`).
   - **Path**: the local folder. For a Remote or Localhost source that
     is not yet cloned, Save **clones** into it as a `Job` with
     progress; for a folder that already holds the repository, Save
     only records it.
   - **Save** is disabled until the mandatory fields hold: Name, Path,
     and for Remote a URL; the hint under the button names what is
     missing. **Close** discards the draft.
   - Saving a **new** repository closes the form, opens the repository
     in the Git activity, and lists its folder as an **Editor project**
     too (`Projects::add_root`), unless that folder is already a
     project, whose name is then left alone (FEAT-0093). The Git tab
     strip's `+` lands on this form with the Name field focused, as the
     sidebar's Add Repository does.

## 4. Sidebar

The repository list, in the saved order: icon, name, path; the row of
the repository the active tab shows is highlighted. Drag to reorder
(`hs_ui::reorder`, the grip owns the drag). Right-click: **Edit
Repository Settings** (Settings → Git with the entry in the form) and
**Remove From List** (asks first; closes the repository's tabs; **never
deletes the folder**). The work pane's header band wears the same icon as
the row and the tab: the uploaded picture when there is one (CR-0036). The `+` in the sidebar header opens Settings →
Git with an empty Add Repository form. A click opens the repository's
tab, or focuses it if it is open.

## 5. The work pane

### 5.1 Header band

`Project icon · Name │ Language │ Version │ URL │ Path`, through
`Tool::header()` so the shell draws it like the editor's. Language and
version come from `hs_core::projects::detect_language` and the editor's
toolchain probe pattern (`hs_core::versions`); URL is the `origin`
remote (or the entry's); Path is the folder.

### 5.2 Toolbar

Left to right: **Commit**, **Pull**, **Push**, **Fetch**, **Branch**,
**Merge**, **Stash**, **Tag**, **Discard**, **Git Flow**, **Terminal**,
then a search box over the history, and the **AI Assistant** toggle as
the hard-right item (`hs_ui::toolbar::hard_right`). Each verb opens an
inline panel (not a nested menu with text fields - see the GOTCHAS on
`TextEdit` in menus) with its options and an **Apply** button.

### 5.3 Layout

Three columns, the SourceTree arrangement:

- **Left tree** (resizable): Workspace (File Status, History, Search),
  Branches (local, with the current one bold and ahead/behind counts),
  Remotes (expandable to their branches), Tags, Stashes, Submodules
  *(addition, read-only list)*. Right-click on a branch: Checkout,
  Merge into Current, Rebase Current onto, Rename, Delete, Set
  Upstream, Push; on a remote: Fetch, Edit URL, Remove; on a tag: Push,
  Delete; on a stash: Apply, Pop, Drop.
- **Centre**:
  - **File Status**: Unstaged and Staged lists with Stage / Unstage
    all and per-file buttons, a filter, and the commit box under them
    (message, Amend, Sign-off, Push after commit; the message field
    takes the assistant's suggestion). Selecting a file shows its diff
    on the right with per-hunk Stage / Unstage / Discard
    (`hs_git::write::apply_hunk`).
  - **History**: the commit graph - lanes drawn from the parent list
    (`hs_git::log` already gives parents), with branch and tag labels
    on the rows, author, date, subject; a search box filters by
    message, author, hash or path. Selecting a commit shows its details
    (hash, parents, author, committer, message) and changed files, each
    opening its diff.
  - **Blame**: for a file chosen from a commit or the working tree,
    `git blame --porcelain` parsed into per-line commit, author and
    date, coloured by age *(addition)*.
- **Right**: the diff viewer (`hs_git::diff`, side-by-side or inline,
  whitespace toggle, word-level highlights from `line_changes`); the
  conflict editor (`hs_git::conflict`, Take Ours / Take Theirs / both,
  Mark Resolved) when the repository is mid-merge or mid-rebase; the
  AI Assistant column when toggled.

### 5.4 Operations

Every write goes through `hs_git::write` on the blocking pool with the
result delivered through a channel drained at frame start; a slow one
(clone, fetch, push) is a `Job` with cancel. Guards: a push or a branch
deletion onto a protected pattern asks first; a discard asks always;
`force` is a checkbox that is off and names itself.

- Commit (amend, sign-off, GPG per the repo config), stage/unstage
  (file and hunk), discard, remove untracked.
- Pull (merge or rebase), push (with upstream, tags, force-with-lease
  *(addition - never bare force)*), fetch (one remote or all, prune).
- Branch: create (from any ref), checkout, rename, delete, set
  upstream; merge (no-ff, squash, no-commit), rebase (onto, interactive
  through `hs_git::write::rebase_interactive`), cherry-pick, continue /
  abort / skip for each.
- Tags: annotated or lightweight, push, delete.
- Stashes: push (with untracked), apply, pop, drop.
- **Git Flow**: Initialise (writes the prefixes into the repo config),
  Start / Finish for feature, release and hotfix, built from plain git
  (`checkout -b`, `merge --no-ff`, `tag`, `branch -d`) so no `git-flow`
  extension is needed; the panel shows the exact commands before Apply.
- **Terminal**: opens a Terminal activity tab with pwsh (or the
  configured profile) in the repository root (`OpenRequest::Terminal {
  cwd }`), the same path the editor's Run uses.
- Remotes: add, edit URL, remove.
- File history *(addition)*: right-click a file → History, a log
  filtered to that path.

## 6. Dashboard (the home tab)

The shared `hs_ui::dashboard::Page` layout every activity uses:

- **Header band**: `ph::GIT_BRANCH`, "Git", a one-line description.
- **Summaries**: Repositories (count), With Changes (repositories whose
  status is dirty - probed lazily, cached per repository), Ahead /
  Behind (sum over repositories with an upstream), Credentials (count).
- **Start Here**: Add Repository (Settings → Git with the empty form),
  Clone From Host (the same form on the Remote source), Git Credentials
  (the credentials card), Open Settings.
- **Saved Repositories**: the grid of saved entries (icon, name, path),
  a click opens the tab; empty state tells where to add one.
- **Open Here**: the activity's open tabs.

## 7. AI Assistant

The shared `hs_ui::ai_assistant` column, hard right, review-only by
default with **Insert and Run** for the command blocks (ADR-019 as
amended). Quick actions: **Write Commit Message** (from the staged
diff), **Explain These Changes**, **Review Before Push**, **Resolve
This Conflict** (the conflict file's three sides), **Suggest Branch
Name**, **Explain This Commit**. A fenced `git` or `bash` block in an
answer renders as a runnable command block: **Insert** types it into
the repository's terminal tab, **Run** runs it there (through the
Terminal activity - the activity never spawns a shell of its own). The
assistant is given the repository's name, branch, upstream and a
bounded slice of the diff; never the token.

## 8. Phases

Status: **phase 6 landed 2026-10-30** - the dashboard's With Changes
and Ahead / Behind figures (`GitTool::probe_figures`, one `git status`
per saved repository, kept for a minute), the host avatar as the
repository icon when a project is chosen in Fetch Project
(`hosts::fetch_avatar`, `GitRepos::set_icon_bytes`; an uploaded icon is
never replaced), a palette command for every verb (`GitTool::commands`,
bindable in Settings → Shortcuts), Up / Down over the file lists and
History, and the README screenshots. All six phases of §8 are in.

Phase 5 status: **landed 2026-10-30** - the assistant column
(`RepoPane::assistant_column`), its six prompt buttons shipped through
`hs_core::prompts` so Settings → AI Assistant → Prompt Buttons edits
them, and the command hand-off: **Insert and Run** opens a Terminal tab
in the repository root with the command (the terminal runs it, as the
editor's Run does); plain **Insert** on a command copies it to the
clipboard and opens that terminal, because the Terminal request has no
"type but do not run" form and review-only means exactly that. A
```commit block's Insert fills the commit box. Phase 4 landed the day
before.

Phase 4 status: **landed 2026-10-29** - the host clients
(`tool-git::hosts`), Fetch Project with the fuzzy dropdown, Clone and
Save as a Job, the credential Test, and the token hand-off
(`tool-git::auth`). One design change from §2.2: the token reaches git
as a one-command **credential helper** (`GIT_CONFIG_COUNT` /
`GIT_CONFIG_KEY_0=credential.helper` / `!f() { cat "<file>"; }; f`)
rather than `GIT_ASKPASS` - git runs the helper through its own shell on
every platform, the file holds `username=` / `password=` lines, and it
is written for one command and deleted after, which is the promise that
mattered. Fetch Project is covered by fixture tests of each host's
answer shape, not by a live token. Phase 3 landed earlier the same day.

Phase 3 status: **landed 2026-10-29** - the verbs (`tool-git::verbs`:
Branch, Merge, Rebase and Interactive Rebase, Stash, Tag, Git Flow,
Rename, Remote panels), the tree's and History's context menus, the
paused-operation bar and the guards. Git Flow writes nothing into the
repository's config: the names live in Settings → Git → Git Flow and
the panel builds plain git from them. Phase 2 landed earlier the same
day.

Phase 2 status: **landed 2026-10-29** - the tree, History with the
graph, commit details and file diffs, Blame, per-hunk Stage / Unstage /
Discard, the conflict editor, File History; and uploaded repository
icons (§3, Icon). Phase 1 landed earlier the same day.

**Repository icons** (owner's requirement, 2026-10-29): the Icon row
takes an uploaded image beside the glyph picker. The file is copied to
`<workspace>/git/icons/<id>.<ext>` (`GitRepos::set_icon`, same formats
as a workspace logo) and shown on the tab (`Tool::tab_icon_uri`), the
sidebar row, the dashboard cell and the Settings list through
`hs_ui::icons::image_uri`, which reads the bytes once per modification
stamp and registers them with the context under a `bytes://` name.
Remove Image deletes the file; Remove From List removes it with the
entry.

Phase 1 status: **landed 2026-10-29** (the stores, the crate, the
dashboard, the sidebar, the header band, File Status with the commit box,
Fetch / Pull / Push / Terminal on the toolbar, Settings → Git with the
Local Folder source; the Remote and Localhost sources are offered in the
form but Fetch Project and cloning wait for phase 4, so the folder must
already hold the repository). `hs_core::langver` moved out of the editor
so both header bands share one toolchain-version cache.

| Phase | Scope | Release |
|---|---|---|
| **1 — Foundation** | `hs_core::git_repos`, `hs_core::git_hosts`, `Services.git_repos`, the `tool-git` crate registered after Editor (rail entry, colour, permanent dashboard home, sidebar list with reorder and context menu, header band, repository tab with File Status and the commit box as the first working surface), Settings → Git (General, Git Credentials, Repositories master/detail with Local Folder source), routes, Remove From List, locale, docs | `feat:` minor |
| **2 — Work pane** | Left tree (branches, remotes, tags, stashes), History with the graph and commit details, Blame, diff viewer with hunk actions, conflict editor, search | `feat:` minor |
| **3 — Operations** | Pull / push / fetch, branch verbs, merge / rebase / cherry-pick with continue/abort, tags, stashes, discard, remotes, Git Flow panel, Open Terminal, protected-branch guards, file history | `feat:` minor |
| **4 — Hosts** | Remote and Localhost sources, host API clients (GitHub, GitLab, Forgejo/Gitea, Bitbucket), Fetch Project with the searchable fuzzy dropdown, clone as a `Job`, `GIT_ASKPASS` token hand-off, credential Test | `feat:` minor |
| **5 — Assistant** | The AI Assistant column, git quick actions, runnable command blocks through the Terminal activity | `feat:` minor |
| **7 — Host tabs** | Hosts section in the sidebar (the credentials), one tab per host with the repository column, the inner repository strip over the existing pane, clone-on-open into the clone folder and registration in the list; then the Files view (tree, preview, edit, new file/folder); then rename, delete, upload, branch picker, shallow clones (FEAT-0100, three items) | `feat:` minor each |
| **6 — Polish** | Host avatars as repository icons, dashboard summaries probed and cached, palette commands and shortcuts for every verb, keyboard navigation of the lists, screenshots and the README | `feat:`/`docs:` |

Each phase is gated (fmt, clippy, tests), exercised in the running app
with the GUI harness, documented (this file, `CHANGELOG.md`,
`CHECKLIST.md`, `docs/07-roadmap.md`, `README.md`) and released from
`main` before the next begins.

## 9. Non-goals

- Replacing the editor's Git view ([git.md](git.md) §3a/§3b); both stay,
  on the same `hs-git` crate.
- A `gix`-based engine: the activity reads and writes through the git
  CLI like the rest of `hs-git` (ADR-006); nothing here depends on a
  `git-flow` extension either.
- Deleting anything on disk from the list: Remove From List forgets,
  never deletes (owner's requirement; `hs_core::projects` has the same
  rule).

## Addendum (2026-11-05): host tabs (FEAT-0100, phase 1)

**Sidebar → Hosts.** Under the repository list, a Hosts section lists the
Git credentials themselves (Settings → Git → Git Credentials), each with
the forge's logo and its address - nothing else, no repositories. The
list is read from `Services.git_credentials` every frame, so a credential
saved in Settings is a host row at once, a renamed one renames its row
and its tab, and a removed one takes its row with it (an open host tab
stays and says the credential is gone). The `+` opens Settings → Git →
Git Credentials; saving a **new** credential there also opens its host
tab (`SettingsEffect::OpenConnection { tool: "git" }`), as Add lands on
what it added elsewhere.

**One tab per host.** A host row opens through `OpenRequest::Connection
{ id: <credential id> }`; the tool answers `current_connection()` with
the credential id, so the shell's one-tab-per-connection rule focuses the
tab that already has the host. The tab is titled after the credential,
wears the forge's logo, and its header band says Kind · URL · User ·
Open (the inner tabs).

**The host tab's work pane** (`tool_git::host::HostView`):

- *Left column:* the host's repositories from `hosts::list_projects`
  (owned, collaborated and organisation repositories; GitHub, GitLab,
  Forgejo/Gitea, Bitbucket - Azure and Generic have no listing and say
  so), grouped **Mine** (the credential's login owns them) and
  **Shared**, with a filter and a Refresh. Read once when the tab opens
  and on Refresh, never per frame. A private repository wears a lock.
- *Right:* an inner tab strip of repositories (`hs_ui::tabs::TabStrip`,
  compact, the Database tab's shape), one tab per repository - opening a
  repository that has a tab focuses it - and under it the repository pane
  itself (`RepoPane`: left tree, File Status / History / Blame, diff
  viewer, commit box, toolbar), exactly as a repository's own tab draws
  it. Closing an inner tab forgets the tab; the clone and the list entry
  stay.
- *Clone on first open.* A repository that is not cloned yet is cloned as
  a Job (git's progress lines, cancel) into
  `<Settings → Git → Default Clone Folder>/<host>/<owner>/<name>` (every
  piece a slug; `HyperStackData/repos/` when no folder is set), then
  recorded in the workspace's repository list with `Source::Remote`, the
  host, the project and the credential - so the plain sidebar and the
  dashboard see it and the next open is instant. A repository the list
  already has, or whose clone folder already exists, opens without
  cloning. One clone at a time per host tab.
- The layout saves the host id, the inner tabs (project, name, path) and
  which is in front; a restored tab whose clone folder is gone says so
  and offers nothing else until it is closed and opened again.

Phases 2 and 3 (the Files view with the tree and the editor; rename,
delete, upload, the branch picker, shallow clones) follow in their own
items.

## Addendum (2026-11-05): the language's logo in its colour (CR-0047)

A repository with no uploaded picture and no chosen glyph wears its
detected language's logo in the language's own colour -
`hs_ui::icons::language_conn_icon`, the mark the Editor gives a project
of that language - in the sidebar row, on the tab (`tab_icon_uri` /
`tab_icon_colour`), in the header band and on the dashboard tile. A
picture still wins; a chosen glyph stays a plain glyph.

## Addendum (2026-11-05): the Files view (FEAT-0101, phase 2)

**Files** is a third row under Workspace in the repository pane's left
tree, beside File Status and History, for a repository opened any way - a
host's inner tab or the list's own tab.

- *Centre column:* the working tree as a folder tree (`hs_ui::tree`),
  listed one folder at a time on the blocking pool as carets are opened,
  `.git` never listed, folders first then files by name, Material icons
  when Settings → Appearance → Material Icon has Git on. Above it: New
  File, New Folder, Refresh (forgets the listings; open folders are
  listed again as they are drawn).
- *Right column:* the picked file. Text (no NUL in its first 8 KB, at
  most 2 MB) opens in the editor engine (`hs_editor::EditorView`,
  `read_only` until **Edit**); a binary or larger file says so and offers
  **Open in Editor**, which every file has (an `OpenRequest::File` to the
  Editor activity). **Save** (the button, or the editor's save key)
  writes the file on the pool and the pane reads its status again, so the
  change appears in File Status and is committed through the commit box
  as any other change; **Discard** puts the text back.
- *New File / New Folder:* a name prompt (no slashes), created under the
  picked folder - the picked file's folder, or the root - and refused
  when something of that name is there; the folder is listed again, and
  a new file opens empty and editable at once.
- Palette: Git: Files, Git: New File, Git: New Folder.
- `tool-git` now depends on `hs-editor` for the engine, as the Database
  tool does.

## Addendum (2026-11-05): Files, phase 3 (FEAT-0102)

The Files view's toolbar row gains **Upload**, **Rename** and **Delete**
(the last two enabled by a picked row), and a branch row under it.

- *Rename:* a name prompt starting with the row's name; the new path must
  be free. The open preview, the picked row and the expanded folders
  follow the new path at once; the parent folder is listed again when the
  rename lands.
- *Delete:* guarded, not forbidden - a file behind a confirmation button,
  a folder by typing its name (everything inside goes too). The removal
  runs on the pool; a preview at or under the path closes; the status is
  read again so File Status shows the deletion for the commit box.
- *Upload:* the system file picker (`rfd`), then every file copied into
  the picked folder on the pool. A name already there refuses the whole
  upload before anything is copied.
- *Branch picker:* a combo over the local branches from the pane's refs,
  showing the current one; picking another runs the pane's checkout verb.
  Whenever a verb finishes the Files view forgets its listings and, if
  the open file has no unsaved edits, reads it again.
- Palette: Git: Rename File, Git: Delete File, Git: Upload Files.
- *Shallow clones:* Settings → Git → **Shallow Clones from Hosts** makes
  a host tab's first-open clone `git clone --depth 1`
  (`settings.git.shallow_clones`, off by default;
  `hs_git::write::clone` takes `depth: Option<u32>`). The Settings → Git →
  Repositories clone stays full.

## Addendum (2026-11-06): the host tab stays apart, the clone folder is the user's, Settings draws the marks (BUG-0044, CR-0053, CR-0054)

- **A host never takes a repository tab over.** The shell binds a
  connection to a tab of the same tool that holds no connection yet; a
  repository tab reported none, so clicking a host while one was open
  turned that tab into the host's. A repository tab now reports what it
  is taken by (`repo:<id>`, or `path:<folder>` for a folder opened outside
  the list), so the shell opens the host beside it. One tab per host and
  one per repository still hold (BUG-0044).
- **Clone into a folder of the user's choice.** Opening a host repository
  that is not cloned asks for the parent folder with the system picker,
  offering the host's last pick, else the default layout's parent
  (Settings → Git → Default Clone Folder, or `repos/` in the data folder).
  The clone lands at `<picked folder>/<repository name>`. The pick is
  remembered per host across restarts. A clone found at the default
  layout's path still opens without asking (CR-0053).
- **A finished clone is an Editor project too**: it is added to the
  Editor's project list (`Projects::add_root`), without opening the
  Editor, beside its entry in the Git list (CR-0053).
- **Settings → Git draws the sidebar's marks**: the Repositories list
  shows the picture, else the chosen glyph, else the language's logo in
  colour (`tool_git::repo_conn_icon`), and the Git Credentials list shows
  the forge's mark (`tool_git::host_conn_icon`) - both helpers are the
  activity's own, now public (CR-0054).

## Addendum (2026-11-06): the dashboard's Saved Hosts card and Add Host (CR-0056)

The Git dashboard draws a second grid, **Saved Hosts**, under Saved
Repositories: one card per Git credential with the forge's mark
(`host_conn_icon`), its name and its host; a click opens the host tab, one
per host as the sidebar's Hosts section does. Start Here gains **Add
Host** right after Add Repository, running the sidebar's Add Host action
(Settings → Git → Git Credentials with the form open). The shared
`hs_ui::dashboard::Page` carries the second grid as `extra`
(`Pick::OpenExtra`), so any activity can add one.
