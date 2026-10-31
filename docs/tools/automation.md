# Tool spec — Automation

**Crate:** `tool-automation` · **Status:** draft · **Last updated:** 2026-10-31 ·
**Phase:** K of [docs/10-depth-programme.md](../10-depth-programme.md) ·
**Ledger:** `FEAT-0077` to `FEAT-0084` ·
**Decisions:** [ADR-028](../08-decisions.md)

## 1. Purpose

See what the team's build servers and configuration-management tools are
doing, and do the few things a developer reaches for between commits:
start a build with its parameters, stop one that went wrong, read why it
failed, and see whether the last Puppet run on a node succeeded.

It is **not** a CI administration console. It does not create or edit jobs,
pipelines, workflows, job templates, agents, runners or credentials on the
server, and it does not replace the server's own UI for that.

## 2. User stories

- As a developer, I want to see that the `deploy-staging` job failed ten
  minutes ago and read the end of its log, without opening the Jenkins UI.
- As a developer, I want to start a build of my branch with the parameters
  the job asks for, and watch it run.
- As an on-call engineer, I want to stop a pipeline that is deploying the wrong
  thing, and I want the app to stop me doing that on production by accident.
- As an operator, I want to see which Puppet nodes failed their last run and
  open the report.
- As a developer, I want the assistant to read a failed log with me and say
  what most likely broke.

## 3. Layout

The connection-per-tab shape of the Queue and Redis activities.

- **Dashboard** (the permanent home tab), through `hs_ui::dashboard`: the
  band; the summaries *Servers*, *Running*, *Failed Today*, *Open Tabs*;
  Start Here with **Add Server**; **Saved Servers** (a grid, each with its
  kind's icon); Open Here. Figures come from the servers open in tabs; a
  server that is not open contributes nothing.
- **Sidebar**: the saved servers of every kind, one list, each row its kind's
  icon, name and address; the grip to reorder (`hs_ui::reorder`).
- **A tab per server**: the header band (name, kind, server version, the
  environment), the toolbar (`hs_ui::toolbar`, the AI assistant hard right),
  and two panes:
  - left: the server's **jobs** as a `hs_ui::table::DataTable` — Jenkins jobs,
    TeamCity build configurations, workflows, GitLab projects' pipelines,
    AWX job templates, Puppet nodes — with the last run's status, when, how
    long and who, and a filter box;
  - right: the selected job's **detail** in tabs: *Runs* (the recent runs
    with status, number, branch or ref, started, duration, who), *Log* (the
    selected run's log, followed while it runs), *Parameters* (what Run asks
    for), and *Queue* where the kind has one.
- `hs_ui::env_banner` across a tab whose server belongs to a protected
  environment; the tab is then read-only (§6).
- **Settings → Automation**: the saved servers, kind buttons across the top,
  list left and form right, like Settings → Queue; and the limits of §7.

## 4. Kinds

One `Driver` per kind behind one trait (§8). What each shows and does:

| | Jenkins | TeamCity | GitHub Actions | GitLab CI | Forgejo Actions | AWX / Automation Platform | Puppet |
|---|---|---|---|---|---|---|---|
| Server is | a Jenkins URL | a TeamCity URL | a repository (`owner/name`) on github.com or GHE | a project (`group/name` or id) on a GitLab URL | a repository on a Forgejo / Gitea URL | an AWX URL | a PuppetDB URL (Puppet Enterprise: also the orchestrator) |
| A job is | a job (folders flattened to `folder/job`) | a build configuration | a workflow | the project's pipelines, by ref | a workflow | a job template | a node |
| Runs | builds | builds | workflow runs | pipelines | runs | jobs | reports |
| Run (trigger) | `build` / `buildWithParameters` | `buildQueue` with properties | `workflow_dispatch` with inputs | create a pipeline on a ref with variables | `workflow_dispatch` with inputs | `launch` with extra vars | Puppet Enterprise orchestrator `deploy` of one node |
| Cancel | stop | cancel | cancel | cancel | cancel | cancel | — |
| Log | progressive text | build log | a run's job logs | a job's trace | a run's job logs | stdout | the report's log lines |
| Queue | yes | yes | — | — | — | — | — |

Notes per kind:

- **Jenkins** is read and driven through its JSON API. A job with parameters
  is triggered with `buildWithParameters`; one without, with `build`. Jenkins
  answers a trigger with a queue item, not a build number; the tab follows the
  queue item until it becomes a build. A username and API token are sent as
  HTTP basic, which Jenkins exempts from the CSRF crumb; a password is sent
  with a crumb fetched first.
- **TeamCity** through its REST API (`/app/rest`), JSON.
- **GitHub Actions** through the REST API. A run's log is per job; the tab
  lists the run's jobs and shows the selected job's log. Only workflows with a
  `workflow_dispatch` trigger can be run; the others say so.
- **GitLab CI** through REST v4. A project's "jobs" in the table are the refs
  that have pipelines (branches and tags), so the table reads like the others;
  a pipeline's log is per job, as for GitHub.
- **Forgejo Actions** through the Forgejo API (`/api/v1/repos/{owner}/{repo}/actions/...`).
  What the API offers depends on the Forgejo version; a call the server does
  not have (404) is reported as such, not as an error in the tab.
- **AWX / Ansible Automation Platform** through `/api/v2`. A template that
  asks for variables on launch shows them as parameters.
- **Puppet** through PuppetDB's query API (v4): nodes with their last report's
  status and time, a node's reports, and a report's log lines. Open source
  Puppet has no API to start a run; **Run** is offered only when the
  connection names a Puppet Enterprise orchestrator.

## 5. Connections and credentials

Connections live in `hs_core::connections` with the kinds `jenkins`,
`teamcity`, `github-actions`, `gitlab-ci`, `forgejo-actions`, `awx` and
`puppet`, so they travel with the workspace and take `{{variables}}` from
its environments. Fields per kind:

| Kind | Fields |
|---|---|
| jenkins | URL |
| teamcity | URL |
| github-actions | API URL (default `https://api.github.com`), repository `owner/name` |
| gitlab-ci | URL, project (`group/name` or numeric id) |
| forgejo-actions | URL, repository `owner/name` |
| awx | URL |
| puppet | PuppetDB URL; Puppet Enterprise orchestrator URL (optional) |

**Credentials** follow the rule for every newer activity
([.claude/rules/security.md](../../.claude/rules/security.md)): one of
`password`, `token` or `api-key`, the form showing only that kind's fields,
the secret always a `secret:<ulid>`. They are read and applied through the
shared `hs_core::service_auth` (ADR-028), which the Tracker activity uses too.

| | password | token | api-key |
|---|---|---|---|
| Jenkins | basic, with a CSRF crumb | user + API token as basic | — |
| TeamCity | basic | `Authorization: Bearer` | — |
| GitHub Actions | — | `Authorization: Bearer` | — |
| GitLab CI | — | `Authorization: Bearer` | `PRIVATE-TOKEN` (the named header) |
| Forgejo Actions | basic | `Authorization: token …` | — |
| AWX | basic | `Authorization: Bearer` | — |
| Puppet | — | — | `X-Authentication` (Puppet Enterprise RBAC token) |

A Puppet open-source PuppetDB is usually reached over plain HTTP on a trusted
network or through a TLS client certificate; client certificates are out of
scope for this phase (§10).

## 6. Guards

- **Run** and **Cancel** are writes. On a protected environment the tab is
  read-only: the controls are disabled with the reason as their tooltip, and
  the driver refuses them as well, so a palette command cannot get round the
  disabled button (docs/06-security.md §3).
- **Run** always opens its form (the parameters, or a line saying the job has
  none) and is sent only from there; there is no one-click run.
- **Cancel** asks first, naming the run.
- Nothing is triggered except the job the user chose.

## 7. Work and progress

All server I/O runs on the tokio runtime and lands through the bounded
channels drained at frame start; nothing blocks the UI thread.

- The jobs list and its last-run figures are read once per poll, every 10 s
  while the tab is visible (Settings → Automation), never while it is not.
- A running build's log is fetched incrementally every 2 s while the Log pane
  shows it, from the offset already read; a finished one once. At most 5 MB of
  log is kept per run; the beginning is dropped first, and the pane says so.
- **Open in Logs** hands the log read so far to the Logs activity as text, as
  a snapshot (a live stream there would need the Logs activity to depend on
  this one).
- A server that has not answered in 15 s is reported, not waited on.
- Listings are bounded: 500 jobs, 50 runs per job.

## 8. Implementation notes

- `crates/tools/tool-automation/`, depending on `hs-core` and `hs-ui` only.
  `src/driver.rs` holds the trait — `jobs`, `runs`, `parameters`, `trigger`,
  `cancel`, `log`, `queue` — and the shared types (`Job`, `Run`, `RunState`,
  `Param`, `LogChunk`); one module per kind (`jenkins.rs`, `teamcity.rs`,
  `github.rs`, `gitlab.rs`, `forgejo.rs`, `awx.rs`, `puppet.rs`), all over
  `reqwest`. A `Guarded` wrapper refuses writes on a read-only session, as in
  the Queue activity.
- `hs_core::service_auth` (new): the credential fields (`auth`, `user`,
  `secret`, `header`), their resolution against the vault and variables, and
  applying the result to a `reqwest::RequestBuilder` in the scheme a kind asks
  for. The Queue activity keeps its own copy for now (ADR-028).
- Registered in `crates/hyperstack/src/tools/mod.rs::factories()` after
  Containers, under **Infra** (the programme's place).
- The AI assistant (ADR-019, review-only) is told the selected job, its last
  runs and the tail of the selected log, with an **Explain This Failure**
  quick action.

## 9. Keyboard

| Command | Id | Default |
|---|---|---|
| Automation: Refresh | `automation.refresh` | F5 (`run.alt`, tool-local) |
| Automation: Run… | `automation.run` | Ctrl+Alt+R |
| Automation: Cancel Run… | `automation.cancel` | — |
| Automation: Open Log in Logs | `automation.open_log` | Ctrl+Alt+L |
| Automation: Add Server | `automation.add` | — |
| Toggle AI Assistant | `automation.ai` | — |

All rebindable in Settings → Shortcuts. Nothing on Ctrl+C/V/X, Ctrl+Shift+V or
Shift+Insert.

## 10. Open questions

1. **Live log into the Logs activity** needs a log source the Logs activity can
   own without depending on this crate; until then it is a snapshot.
2. **Client-certificate TLS** (PuppetDB's usual setup, Jenkins behind mTLS).
3. **Artifacts** (download a build's artifacts) and **test reports**.
4. **Bamboo, CircleCI, Azure Pipelines, Buildkite, Argo Workflows** as later
   kinds.

## What is implemented (2026-10-31)

Nothing yet: this spec precedes the code, as the template requires.
