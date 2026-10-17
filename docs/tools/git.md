# Tool spec — Git

**Crate:** `tool-git` · **Priority:** P0 (status, stage, commit, diff, branches, fetch/pull/push) · P1 (log graph, blame, merge/rebase, conflicts) · P2 (submodules, worktrees, forge integration) · **Status:** draft

## 1. Purpose

The everyday Git loop — see what changed, stage hunks, commit, pull, push,
switch branches, look at history — next to the editor and terminal that
produced the changes, with behaviour identical to the user's command-line Git
(hooks, signing, credential helpers, config). Not a replacement for advanced
Git GUIs; the terminal is one tab away for anything exotic.

## 2. User stories

- Edit three files in the editor; the Git sidebar shows them; stage two hunks
  of one file, write a message with the template, commit, push — the Jobs
  panel shows the push progress and the remote's response.
- Switch to `release/1.4`, pull, see a conflict in `compose.yml`, open the
  three-way merge editor, resolve, continue.
- Browse the log graph, click a commit, see its diff, open a file at that
  revision read-only, blame a line to find the ticket.
- Protected remote (`prod` pattern): force push and branch deletion are
  disabled in the UI.

## 3. Layout

```
┌ sidebar: changes ────────┬ main (diff) ────────────────────────────────────────────────────┐
│ ⌥ shop-api · main ↑2 ↓0  │ compose.yml · working tree ↔ index      [Inline] [Side by side] ⚙ │
│ ▾ Staged (1)             │ @@ -12,7 +12,8 @@                                 [Unstage hunk] │
│   M deploy/compose.yml   │ 12   services:                                                     │
│ ▾ Changes (3)            │ 13     api:                                                        │
│   M src/main.rs          │ 14 -     image: shop/api:1.3                                       │
│   A src/handlers/pay.rs  │ 14 +     image: shop/api:1.4                                       │
│   ? notes.md             │ 15 +     restart: unless-stopped                                   │
│ ▾ Stashes (1)            │                                                                    │
│ ───────────────────────  ├────────────────────────────────────────────────────────────────── │
│ Message                  │ Branches ▾   main ●  release/1.4  feature/pay   origin/main …       │
│ ┌──────────────────────┐ │ Log: ● 7f3a1c  fix compose image tag        Ada · 2 h ago           │
│ │ fix: bump api image  │ │      │ 2b9e0d  add payment handler            Ada · 3 h ago           │
│ └──────────────────────┘ │      ●─┐ 91c2aa  Merge release/1.3           CI · yesterday        │
│ [✓ Sign-off] [Amend]     │                                                                    │
│ [ Commit ] [Commit & Push]│                                                                    │
└──────────────────────────┴──────────────────────────────────────────────────────────────── ┘
```

A Git tab is bound to one repository (detected from the editor project tree or
chosen). Main area sub-tabs: Diff, Log, Branches, Stashes, Conflicts.

## 3a. The Git view in the editor (implemented 2026-10-12)

The read side of this spec ships first inside the editor, where the files
are (owner's request): every project tab knows its repository.

- **Project tree**: the project root shows the branch (`⎇ main ↑2 ↓1`);
  changed files carry their letter (M, A, D, R, U untracked, ! conflict) in
  the change's colour, and folders holding changes get a tinted dot.
- **Source Control** (sidebar switch next to Projects, Ctrl+Shift+G): per
  repository, Conflicts / Staged / Changes / Untracked with the change
  letter, the file's folder, deleted files struck through; a click shows the
  diff in the main area, a double click (or Open File, Enter in the diff)
  opens the file; right-click: Open Changes, Open File, Copy Path. The
  header has the branch with ahead/behind (upstream in the tooltip), History
  and Refresh.
- **Diff view** (main area): inline or side by side (remembered per tab),
  line numbers, added/removed backgrounds, hunk headers, notes (new file,
  rename, mode change), binary notice. Up/Down steps through the changes,
  Esc or **Back to Editor** returns; it reloads when the status changes.
- **History** (Ctrl+Shift+H, or the header's clock): the current branch's
  commits (200 at a time, more as you scroll) with branch, tag and remote
  chips, author and date; Up/Down moves; the selected commit shows its full
  message, author, committer when different, id (copy), changed files and
  the selected file's diff against the first parent.
- **Gutter markers** in the editor: added (green bar), modified (blue bar)
  and removed (red wedge) lines against the last commit, recomputed on the
  blocking pool as you type, not only on save.
- Status refreshes on file-system events in the work tree and in `.git`
  (HEAD, index, refs; object and lock churn ignored), debounced 400 ms, plus
  the Refresh button and `git.refresh`.

Staging, commit, branches, stash, fetch/pull/push, merge and the rest stay
for the Git tool proper (M4/P1). Engine: `crates/hs-git`.

## 4. Engine (hybrid, see ADR-006)

**As built (2026-10-12):** every read goes through the `git` CLI with
porcelain output (`status --porcelain=v2 -z`, `diff`, `log` with a
separator format, `for-each-ref`, `diff-tree --name-status -z`, `show
HEAD:path`), `--no-optional-locks` so a refresh never takes the index lock,
`LC_ALL=C`, no console window on Windows. Line markers use `similar` with a
150 ms timeout. `gix` remains the plan for when status/log speed on large
repositories needs it; the table below is that target.

| Operation | Implementation |
|---|---|
| Discover repo, read config, refs, HEAD, remotes | `gix` |
| Status (index vs worktree, untracked, ignored), with `.gitignore` semantics | `gix` status API, incremental on file watcher events, debounced |
| Diff (worktree ↔ index, index ↔ HEAD, commit ↔ commit, file at revision) | `gix` object access + `similar` for text diff and word-level highlighting; binary detection; rename detection via `git diff -M` output when needed |
| Log, graph, blame, file history | `gix` revwalk; graph layout computed client-side; blame via `gix::blame` with `git blame --porcelain` fallback |
| Stage/unstage file | `gix` index write |
| Stage/unstage hunk or lines | build a patch and run `git apply --cached [--reverse]` |
| Commit, amend, sign-off, GPG/SSH signing, hooks | `git commit` CLI (honours hooks, `commit.gpgsign`, templates) |
| Fetch, pull, push, remotes | `git` CLI with `--progress --porcelain`, streamed into Jobs; credential helpers and SSH config just work |
| Branch create/checkout/rename/delete, tags, stash, merge, rebase, cherry-pick, revert, reset, clean | `git` CLI with porcelain parsing |
| Conflict detection and resolution | `gix` index stages 1/2/3 to feed the three-way editor; `git add` to mark resolved; `git merge --continue/--abort`, `git rebase --continue/--abort` |

If `git` is not on `PATH` (or configured path), read-only features work and
write actions show an install hint with a link. The chosen `git` binary and its
version appear in Settings → Git.

## 5. Sidebar: changes

Grouped Staged / Changes / Untracked / Conflicts with status letters and
colours, folder or flat view, filter, multi-select, context menu (stage,
unstage, discard changes with confirmation, delete untracked, ignore pattern,
open file, open in terminal, copy path, stash selected). Stashes list with
apply/pop/drop/show. Commit message editor (`hs-editor` single-field mode)
with: configured template, conventional-commit prefix picker, subject length
ruler at 50/72, history of recent messages, sign-off, amend, GPG/SSH signing
indicator, co-author trailer helper. Buttons: Commit, Commit & Push, Commit &
Sync (pull --rebase then push, opt-in).

## 6. Diff viewer

Built on `hs-editor` Diff mode: side by side or inline, syntax highlighting,
word-level highlights, hunk navigation, stage/unstage/discard per hunk and per
selected lines, whitespace-ignore toggle, context expansion, image diff
(side by side / swipe for PNG/JPG/SVG), large-file and binary notices, "open
file at line", "open at this revision" (read-only buffer). Git gutter signs in
the editor come from this engine via events.

## 7. Log and history (P1)

Graph with lanes and colours, columns (graph, subject, refs badges, author,
date, SHA), search (message, author, SHA, path, `--grep`-style), filters by
branch/all/remote, lazy loading (500 commits per page), commit details pane
(metadata, parents, signature status, changed files with diffs), actions
(checkout, create branch/tag here, cherry-pick, revert, reset soft/mixed/hard
with guards, copy SHA, compare with working tree). File history and blame
views open from the editor ("Blame" gutter annotation with hover details).

## 8. Branches, remotes, tags

Branches panel (local, remote, with ahead/behind and upstream), create from
here, checkout (auto-stash offer when dirty), rename, delete (guard for
unmerged), set upstream, merge into current, rebase current onto, compare.
Remotes: add/edit/remove, fetch, prune. Tags: list, create (annotated,
signed), push tag, delete.

## 9. Merge, rebase, conflicts (P1)

Merge dialog (fast-forward options, squash, no-commit), rebase (onto, with a
simple interactive list: pick/squash/fixup/reword/drop/reorder written to a
todo file for `git rebase -i` with `GIT_SEQUENCE_EDITOR` pointing at a helper),
cherry-pick/revert with continue/abort states shown in a banner. Conflicts
panel lists files; each opens in the three-way merge editor (base/ours/theirs
+ result, accept left/right/both/manual, conflict navigation); "Mark resolved"
stages the file; Continue/Abort buttons for the ongoing operation.

## 10. Guards

Protected remote patterns (default: remotes whose URL or branch matches
configured `prod`/`release` patterns, plus any environment-flagged repo):
`push --force` (and `--force-with-lease` unless enabled), `branch -D`,
`reset --hard`, `clean -fd`, `push --delete` disabled or double-confirmed.
Discarding changes always confirms with the file count. Amending a pushed
commit warns.

## 11. Integration

- Editor: gutter signs, blame, "Open changes", "Stage file", status in the
  project tree (status letter and colour per entry), diff from the
  tree's context menu.
- Terminal: "Open terminal here" on the repo; the Git tab refreshes on
  terminal activity in the repo (watcher on `.git/index`, `.git/HEAD`, refs).
- Jobs: every network operation; output shown in the Output dock.
- Forge integration (P2): GitHub/GitLab/Forgejo tokens in the vault; PR list,
  create PR from branch, CI status badge on the branch, open issue numbers
  from commit messages. Forgejo/Gitea is a first-class target alongside
  GitHub and GitLab, since self-hosted forges are common in the target
  audience.

## 12. Keyboard (Git scope)

| Shortcut | Action |
|---|---|
| Ctrl+Enter (in message) | commit |
| Ctrl+Shift+Enter | commit and push |
| Ctrl+Shift+K / Ctrl+Shift+U | push / pull |
| Ctrl+Shift+F | fetch |
| Alt+Enter | stage/unstage selected files |
| Ctrl+Alt+S | stash |
| F7 / Shift+F7 | next / previous hunk |
| Ctrl+Shift+B | branches |
| Ctrl+Shift+G | Source Control in the editor sidebar (toggle) |
| Ctrl+Shift+H | history |

## 13. Tests

Fixture repositories built in tests (`git init` + commits) covering status
states, renames, submodules present, merge conflicts; porcelain parsers for
`status --porcelain=v2`, `push --porcelain`, `rebase` todo; hunk patch
generation round-trips; guard rules; integration with a local bare remote.

## 3b. The write side in the editor (implemented 2026-10-18)

The first slice of M4's Git work, built on `hs-git::write` (every call is
the user's own `git`, so hooks, signing, credential helpers and config
apply as on the command line) and shown in the editor's Source Control
sidebar under each repository's header:

- **Operations row**: Fetch (`--all --prune`), Pull (with `--rebase` when
  Settings → Editor → Git says so, the default), Push (`-u origin
  <branch>` for a branch with no upstream yet; a plain `--force` is never
  issued), the **stash** menu (stash with a message, untracked files too
  by default; Apply, Pop, Drop per entry) and the **branch** menu (switch,
  New Branch…, delete with its own question). Fetch, pull and push are
  Jobs, so the dock lists them and keeps git's output.
- **Commit box**: message, Amend, Commit and Commit and Push; Ctrl+Enter
  in the message commits. Nothing staged says so rather than failing.
- **Change rows**: Stage / Unstage / Discard Changes… / Delete… on the
  context menu; Stage All / Unstage All on a group's header.
- **Guards** (§10): discard, delete and branch delete always ask with what
  would be lost; a push to a remote whose URL, or a branch whose name,
  matches a protected pattern (default `prod`, `release`) asks once more
  and shows a shield beside the buttons. `unstage` is `git reset --`
  rather than `restore --staged`, which resolves HEAD and fails before the
  first commit.
- Every finished operation refreshes the status and the lists and reports
  in a toast. State lives per repository (`git_ops::Ops`), so two projects
  in one tab do not share a message.

Still to come from §5–§9: per-hunk stage/unstage/discard, the commit
message template and prefix picker, rename/upstream/merge/rebase of
branches, remotes and tags management, and the conflict editor.
