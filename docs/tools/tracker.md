# Tool spec — Tracker

**Crate:** `tool-tracker` · **Status:** implemented · **Last updated:** 2026-10-31 ·
**Phase:** N of [docs/10-depth-programme.md](../10-depth-programme.md) ·
**Ledger:** `FEAT-0085` to `FEAT-0091` ·
**Decisions:** [ADR-029](../08-decisions.md)

## 1. Purpose

Read and work the team's tickets and pages without leaving the workspace:
find the issues a query names, read one with its comments, change its status
through the tracker's own workflow, edit its fields, answer it, file a new
one - and read and edit a Confluence page.

It is **not** a project administration console. It does not create
projects, edit workflows, schemes, custom field definitions, boards or
permissions, and it does not replace the tracker's own UI for those.

## 2. User stories

- As a developer, I want the issues assigned to me across Jira and GitLab in
  one place, so I start the day from one list.
- As a developer, I want to move a ticket to *In Review* the way the
  project's workflow allows, without opening the browser.
- As a support engineer, I want to read a ticket's comments and answer it.
- As a lead, I want to file a bug with its priority and labels from here.
- As a writer, I want to read a Confluence page as Markdown, edit it and
  publish it back.
- As anyone, I want the assistant to summarise a long ticket and draft a
  reply I review before sending.

## 3. Layout

The connection-per-tab shape of the Queue and Automation activities.

- **Dashboard** (the permanent home tab), through `hs_ui::dashboard`: the
  band; the summaries *Trackers*, *Assigned to Me*, *Due This Week*,
  *Open Tabs*; Start Here with **Add Tracker**; **Saved Trackers**; Open
  Here. Figures come from the trackers open in tabs.
- **Sidebar**: the saved trackers, one list, each with its product's mark,
  name and address; the grip to reorder.
- **A tab per tracker**: the header band, the toolbar (`hs_ui::toolbar`, the
  AI assistant hard right), and two panes:
  - left: a **query bar** in the tracker's own language (JQL for Jira,
    Redmine's filters, GitHub / GitLab / Forgejo search syntax) with saved
    queries, over a `DataTable` of **items** - key, summary, status, assignee,
    priority, updated;
  - right: the selected **item** - its fields, the description rendered as
    Markdown, comments, attachments (names, open in the Browser), links - with
    **Edit**, **Transition**, **Comment** and **Open in Browser**.
- **Confluence** tabs show a **space → page tree** on the left instead of the
  items table, and the page on the right, rendered from its storage format
  to Markdown, with **Edit** (Markdown, converted back on publish).
- **New Item** opens a form of the project's fields.
- `hs_ui::env_banner` and read-only on a protected environment (§6).
- **Settings → Tracker**: kind buttons, list left, form right; the limits of §7.

## 4. Kinds

| | Jira | Redmine | GitHub Issues | GitLab Issues | Forgejo Issues | Confluence |
|---|---|---|---|---|---|---|
| Connection is | a site (Cloud or Data Center) | a Redmine URL | a repository on github.com or GHE | a project on a GitLab URL | a repository on a Forgejo / Gitea URL | a site (Cloud or Data Center) |
| Query | JQL | project, status, assignee, tracker filters and full text | GitHub search syntax (`is:open label:bug`) | GitLab issue filters and search | Forgejo issue filters and search | CQL, or browse spaces |
| Item text | Atlassian Document Format (Cloud) or wiki markup (DC) → Markdown | Textile or Markdown | Markdown | Markdown | Markdown | storage format (XHTML) → Markdown |
| Status change | the issue's transitions | any status the role may set | open / closed | open / closed (`state_event`) | open / closed | — |
| Edit | summary, description, assignee, priority, labels, due date | subject, description, assignee, priority, due date | title, body, assignees, labels | title, description, assignees, labels, due date | title, body, assignees, labels, due date | the page's body, as Markdown |
| Comment | yes | yes (a journal note) | yes | yes (a note) | yes | yes (page comment) |
| Create | yes | yes | yes | yes | yes | — (pages: open question) |

Notes per kind:

- **Jira** through REST v3 (Cloud) or v2 (Data Center); the kind's form says
  which. Descriptions in Atlassian Document Format are converted to Markdown
  for reading and back for writing, for the subset ADF and Markdown share
  (paragraphs, headings, lists, code, links, emphasis); a description with
  other nodes is edited as text with a warning before it is overwritten.
- **Redmine** through its REST API (`.json`), API key in
  `X-Redmine-API-Key`.
- **GitHub, GitLab and Forgejo Issues** through the same REST APIs as the
  Automation activity's kinds, with the same credentials.
- **Confluence** through REST v2 (Cloud) or v1 (Data Center). A page is
  converted from its storage format to Markdown and back for the subset the
  two share; a page holding macros or layouts is shown read-only with a
  note, so publishing can never drop content the converter does not know.

## 5. Connections and credentials

Connections live in `hs_core::connections` with the kinds `jira`,
`redmine`, `github-issues`, `gitlab-issues`, `forgejo-issues` and
`confluence`. Fields per kind:

| Kind | Fields |
|---|---|
| jira | site URL, Cloud or Data Center, default project key |
| redmine | URL, default project identifier |
| github-issues | API URL (default `https://api.github.com`), repository `owner/name` |
| gitlab-issues | URL, project |
| forgejo-issues | URL, repository `owner/name` |
| confluence | site URL, Cloud or Data Center, default space key |

**Credentials** through `hs_core::service_auth` (ADR-028), one of
`password`, `token` or `api-key`:

| | password | token | api-key |
|---|---|---|---|
| Jira | Data Center basic | Cloud: email + API token as basic; DC: personal access token as bearer | — |
| Redmine | basic | — | `X-Redmine-API-Key` |
| GitHub Issues | — | bearer | — |
| GitLab Issues | — | bearer | `PRIVATE-TOKEN` |
| Forgejo Issues | basic | `token …` | — |
| Confluence | Data Center basic | as Jira | — |

## 6. Guards

- **Transition**, **Edit**, **Comment** and **Create** are writes. On a
  protected environment the tab is read-only: the controls are disabled with
  the reason, and the driver refuses them as well.
- **Edit** shows what changes, field by field, before it is sent; **Publish**
  of a Confluence page shows the page's diff first.
- Nothing is sent from the assistant: it drafts, the user sends.

## 7. Work and progress

- A query reads at most 100 items a page, with **More** for the next page;
  the open tab re-reads its query every 60 s while it is visible.
- An item is read when it is selected; its comments up to 200.
- A server that has not answered in 15 s is reported, not waited on.

## 8. Implementation notes

- `crates/tools/tool-tracker/`, depending on `hs-core` and `hs-ui` only, with
  `src/driver.rs` (the trait: `search`, `item`, `comments`, `transitions`,
  `transition`, `edit`, `comment`, `create`, `fields`; and for Confluence
  `spaces`, `pages`, `page`, `publish`), one module per kind, a `Guarded`
  wrapper, and `src/markup.rs` for the ADF, Jira wiki, Textile and
  Confluence storage conversions.
- The HTTP plumbing (`Http`, refusals in words) is shared with
  `tool-automation` through `hs-core` rather than copied (ADR-029).
- Registered after Automation in the rail, under **Infra**.
- The assistant (ADR-019, review-only) is told the selected item and its
  comments, with **Summarise This Ticket**, **Draft a Reply** and **Suggest a
  Status**; a reply it drafts can be put in the comment box, never sent by it.

## 9. Keyboard

| Command | Id | Default |
|---|---|---|
| Tracker: Refresh | `tracker.refresh` | F5 (`run.alt`) |
| Tracker: Search… | `tracker.search` | Ctrl+Alt+F |
| Tracker: New Item… | `tracker.new` | Ctrl+Alt+N |
| Tracker: Change Status… | `tracker.transition` | Ctrl+Alt+T |
| Tracker: Comment… | `tracker.comment` | — |
| Tracker: Open in Browser | `tracker.browser` | — |
| Tracker: Add Tracker | `tracker.add` | — |
| Toggle AI Assistant | `tracker.ai` | — |

## 10. Open questions

1. **YouTrack** and **Azure Boards**: later kinds.
2. Creating **Confluence pages** (and moving them in the tree).
3. **Attachments** upload.
4. **Notifications / mentions** ("unread mentions" on the dashboard needs
   each tracker's notification API).

## What is implemented (2026-10-31)

All six kinds as §3-§9 describe. Checked on screen and by `tests/live.rs`
against a Redmine 6 container (filter, read, change status through the
workflow's allowed statuses, edit with the field review, comment, create,
the API key kept in the vault) and the Forgejo 11 container (create,
comment, close, reopen), and read-only against public projects on
github.com and gitlab.com. Jira and Confluence need an Atlassian licence key
and could not run here, and the owner kept writes off GitHub and GitLab:
those calls are tested in `tests/mock.rs` against a mock server whose answers
are written from each API's reference, not captured from a server. Where the
build differs from the text above:

- The driver trait folds `comments` into `item` (an item is always read
  with its comments) and adds `mine_query`, the query the dashboard's
  *Assigned to Me* counts; *Due This Week* counts the items listed in open
  tabs.
- **Redmine** has no search language: the query bar takes `project:`,
  `status:` (`open`, `closed`, `all` or a status name), `assignee:` (`me` or
  an id) and `tracker:`, and searches the rest in the subject. An assignee
  is set as `me` or a user id. Redmine 6 writes CommonMark by default, so
  the profile says whether the site writes **Textile** instead; only then is
  text converted.
- **Forgejo and GitLab** take the same words (`is:open`, `label:`,
  `assignee:`), since neither has a search language either.
- **gitlab.com** serves a public issue anonymously but not its notes; the
  issue opens and says a token is needed for the comments.
- **Jira Cloud** pages its search by token (`/search/jql`); Data Center by
  offset. A description converted from ADF or wiki markup that dropped
  anything says what (`Shown as Markdown without: panel, mention`), and
  Edit repeats it before a new description is saved over it.
- **Confluence**: Browse (a space and its page tree) or Search (CQL); Edit
  is offered only on a page the converter can keep whole, and Publish shows
  the line diff and sends the next version number.
- Saved queries are kept in the profile (`queries`), so every tab of that
  tracker shares them.
- Kind icons are the products' own marks from the bundled Nerd Font where
  Phosphor does not shadow them (CR-0024); Redmine has no mark and takes a
  bug.
