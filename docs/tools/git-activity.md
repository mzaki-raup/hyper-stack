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

## 4. Sidebar

The repository list, in the saved order: icon, name, path; the row of
the repository the active tab shows is highlighted. Drag to reorder
(`hs_ui::reorder`, the grip owns the drag). Right-click: **Edit
Repository Settings** (Settings → Git with the entry in the form) and
**Remove From List** (asks first; closes the repository's tabs; **never
deletes the folder**). The `+` in the sidebar header opens Settings →
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

Status: **phase 1 landed 2026-10-29** (the stores, the crate, the
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
