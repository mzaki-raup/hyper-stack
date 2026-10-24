# Tool spec — Containers (Docker / Podman / Kubernetes)

**Crates:** `hs-container` (drivers, no UI) + `tool-containers` (UI) · **Priority:** owner's request 2026-10-24, pulled forward from Phase 2 ahead of M6 · **Status:** §1-§9 draft, §10 specified and planned in eight phases (ADR-026); all eight phases built

## 1. Purpose

See and operate the containers developers run locally and operators run on
servers — without Docker Desktop's weight — and the Kubernetes basics an SRE
needs ten times a day: pods, logs, exec, port-forward, describe, apply. Logs go
to the log viewer, exec goes to the terminal, YAML goes to the editor.

## 2. User stories

- Open the local Docker engine: see `shop-api` unhealthy, click Logs (log
  viewer tab opens following), click Exec (shell in the container), restart
  it.
- Connect to the Docker engine on `web-01` over SSH and pull up the compose
  project running there; `compose logs` for two services merged in the log
  viewer.
- Kubernetes `staging` context: find pods of `deploy/api`, tail logs of all
  replicas merged, port-forward 8080 to local and hit it from the API tab.
- Prune dangling images after a confirmation that shows reclaimable size.

## 3. Layout

```
┌ sidebar: engines ────────┬ main ───────────────────────────────────────────────────────────┐
│ ▾ 🐳 docker (local)      │ ▌dev▐  Containers (6)   [All] [Running]  🔍 filter          ⟳   │
│   Containers (6)         │ name       │ image           │ status            │ ports   │ cpu │ mem │
│   Images (41)            │ shop-api   │ shop/api:1.4    │ Up 3h (unhealthy) │ 8080    │ 2 % │ 210M│
│   Volumes (12)           │ shop-db    │ postgres:16     │ Up 3h             │ 5432    │ 1 % │ 95M │
│   Networks (4)           │ shop-redis │ redis:7         │ Up 3h             │ 6379    │ 0 % │ 12M │
│   Compose: shop (3)      ├─────────────────────────────────────────────────────────────── │
│ ▾ 🐳 web-01 (ssh)        │ shop-api · [Logs] [Exec] [Inspect] [Stats] [Files] [Restart] [Stop] [Remove] │
│ ▾ ☸ staging (k8s)        │ env: DB_URL=postgres://… · mounts: ./data:/data · health: curl -f … (failing 12×) │
│   ns: shop ▾             │                                                                    │
│   Pods (7) Deploys (3)   │                                                                    │
└──────────────────────────┴─────────────────────────────────────────────────────────────── ┘
```

## 4. Docker / Podman (P1)

- **Transport**: local socket / named pipe, TCP with TLS, **over SSH**
  (forward the remote socket through the `Tunnels` service, exactly what
  `docker -H ssh://` does), Podman via its Docker-compatible API. Crate:
  `bollard`.
- **Containers**: list with live stats (opt-in, `stats` stream), filter by
  state/name/label/compose project, start/stop/restart/pause/kill/remove
  (with volumes option), rename, inspect (JSON tree + summary: env, mounts,
  ports, networks, health, restart policy), logs (→ `tool-logs` with
  `LogSource::Docker`, follow, since, timestamps), exec (→ `hs-term`
  `DockerExec` with shell detection `/bin/bash` → `/bin/sh`), attach, files
  (browse `/` via `exec ls`/`docker cp` style archive API, download/upload),
  stats history sparklines, "open port in API client" for mapped ports, copy
  `docker run` equivalent.
- **Images**: list (repo/tag/size/created/dangling), pull (with progress by
  layer), push (P2), tag, remove, history/layers viewer, inspect, prune
  dangling, search registry (P2), build from a Dockerfile in the project
  (Job with streamed output) (P2).
- **Volumes**: list, inspect, usage (`df`), remove, prune, browse contents via
  a helper container (P2).
- **Networks**: list, inspect (containers attached), create/remove, connect/
  disconnect container.
- **Compose**: detect projects from labels (`com.docker.compose.project`) and
  from `docker-compose.yml`/`compose.yaml` in the project tree; per project:
  services with state, up/down/restart/pull/build (shell out to `docker
  compose` for fidelity, streamed into Jobs), logs per service or merged
  (log viewer), edit the compose file (editor), `config` (rendered YAML).
- **System**: `docker info`, disk usage (`system df`) with prune actions,
  events stream (log viewer source), contexts import from Docker CLI.

## 5. Kubernetes (P2)

- `kube` + `k8s-openapi` crates; kubeconfig import with contexts; `kind =
  "k8s"` connection = kubeconfig + context + default namespace; auth via
  kubeconfig exec plugins (cloud CLIs) supported by running them.
- Resource browser: namespaces, pods (status, restarts, node, age, containers,
  ready), deployments/statefulsets/daemonsets/jobs/cronjobs with scale and
  rollout restart, services/ingresses with ports, configmaps/secrets (values
  masked, reveal per key, edit guarded), nodes with conditions and capacity,
  events (log viewer source), PVCs, HPAs; generic CRD listing via discovery.
- Actions: describe (YAML + events), edit YAML in the editor and apply with a
  server-side dry-run diff first, delete (guarded), logs (→ log viewer, all
  containers / previous / multi-pod merge by label selector), exec (→
  terminal), port-forward (managed in Jobs and `Tunnels`, local port shown,
  "open in API client"), cp (download/upload), scale, rollout status/undo,
  cordon/drain nodes (guarded), top pods/nodes if metrics-server exists.
- Helm (P3 idea): list releases, values, history, rollback.

## 6. Guards

Protected environments: stop/remove/prune/`compose down`, scale to 0, delete
resources, `drain` require confirmation naming the engine/context and
namespace; `--force`/`--volumes` need an extra checkbox; secrets editing is
read-only unless unlocked for the tab.

## 7. Integration

Logs → `tool-logs`; exec/attach → `hs-term`; YAML/compose/Dockerfile →
editor with schema validation (compose and k8s JSON schemas bundled for
completion in the editor, P2); mapped ports → API client base URL suggestion;
DB/Redis containers → "connect with DB/Redis tool" using the exposed port and
env credentials found in `inspect` (offered, never automatic).

## 8. Keyboard

| Shortcut | Action |
|---|---|
| F5 | refresh |
| Ctrl+L | logs |
| Ctrl+E | exec |
| Ctrl+I | inspect |
| Ctrl+Shift+R | restart |
| Delete | remove / delete (confirm) |
| Ctrl+Shift+P | port-forward (k8s) |

## 9. Tests

`bollard` against a Docker-in-Docker CI service: list/start/stop/logs/exec;
compose detection fixtures; kubeconfig parsing fixtures; `kind` cluster in CI
for the k8s basics (P2).

## 10. The Container activity (owner's request, 2026-10-24)

Commissioned ahead of M6, pulled forward from Phase 2. §1–§9 above are
the resource-level requirements and still stand; this section adds the
activity shell the owner asked for, and records the decisions §1–§9 left
open. ADR-026.

### 10.1 What was already decided, and is confirmed

The draft above got four things right that everything here depends on:

- **A container host is a connection.** §5 already says `kind = "k8s"`
  *is* a profile of kubeconfig + context + namespace, and `KINDS` in
  `hs-core::connections` already reserves `docker` and `k8s` with
  nothing consuming them. **`podman` and `wincontainer` join that
  list**; no second store is created. That is what gives the activity
  the vault, per-environment hosts, read-only/protected handling,
  folders, tags, pinning and ordering without writing any of it.
- **Logs go to `tool-logs`, exec to `hs-term`, YAML to the editor**
  (§7). More valuable now than when it was written: the Logs tool has
  since gained filter chips, level colouring, the time strip, saved
  investigations and its own assistant. A log viewer inside this
  activity would be a worse copy that has to be fixed twice.
- **Apply does a server-side dry-run diff first** (§5).
- **Protected environments guard every destructive verb** (§6).

### 10.2 kubeconfig is referenced, not copied into the vault

The owner asked for every credential to be vault-bound. For passwords,
registry tokens, bearer tokens and TLS client keys: yes,
unconditionally, as `secret:<ulid>`.

**kubeconfig is the deliberate exception.** It normally lives at
`~/.kube/config`, is rewritten by `az`/`gcloud`/`eksctl`/`rancher`,
holds many contexts, and usually delegates auth to an `exec` plugin that
mints short-lived tokens. A copy in the vault would go **stale** the
moment a cluster is re-provisioned while still looking authoritative,
would duplicate a secret that already exists on disk, and would break
the `exec` plugins that resolve relative to the real file.

So the stored form is **path + context name** (relative to the data
folder when it sits inside it, ADR-017). A **Paste kubeconfig** option
puts the document in the vault for a portable workspace or a static
token, and the form says plainly that a pasted copy will not follow
rotation. Inline `token`, `client-key-data` and passwords inside a
pasted document are vault-bound like any other secret.

### 10.3a The Docker/Podman driver is the CLI, not `bollard` (2026-10-24)

§4 chose `bollard`, and phase 2 was planned around it. Building phase 2
changed the answer, and the reasons are worth keeping:

- **The CLI covers every transport; `bollard` covers two.** An Engine
  API client can reach a local socket and a TCP host. It cannot reach
  an engine inside a WSL distribution - which on Windows is where the
  engine usually is - without someone first exposing a socket, and it
  cannot reach one over SSH without forwarding a socket through the
  tunnel manager. The CLI reaches all four by construction, because
  `wsl.exe -d <d> -- docker …` and `ssh host docker …` are just
  prefixes.
- **One parser, both engines.** `docker` and `podman` accept the same
  `--format '{{json .}}'`, so one code path serves both. Two API
  clients would not.
- **It is verified against a real engine.** `tests/live.rs` runs against
  Docker 29.5.0 in WSL and reads containers, images, volumes and
  networks; `bollard` would have needed the same tests plus a socket to
  point them at.

What the CLI cannot do is **stream**: `/events` and `/stats` are what a
live pane would want, and polling `docker stats --no-stream` per refresh
is not the same thing. So `bollard` is **not rejected, it is deferred**
to the point where streaming is actually needed - phase 4's live pane,
and only for the Local and Tcp transports, behind the same `Engine`
interface so nothing above it changes. Until then the refresh interval
in Settings governs, which is honest for a list of containers.

The parsing is where the surprises were, and they are the reason this is
testable in isolation: `docker ps --format '{{json .}}'` is
**line-delimited** JSON whose `Labels`, `Ports` and `Mounts` are *human
strings*, while `podman ps --format json` is a **JSON array** with `Id`
rather than `ID`, `Names` as an array, `Labels` as an object and `Ports`
as objects. The parsers accept either shape for each, which also buys
tolerance of version drift.

### 10.3 The Kubernetes driver: the `kube` crate, with `kubectl` as the escape hatch

§5 chose `kube` + `k8s-openapi`. That stays, for two reasons that
outweigh the dependency weight: typed **watches** are what make a live
pane possible, and needing no external binary matches the portable
promise. `kube` also runs `exec` credential plugins, which is the part
nobody should reimplement.

The escape hatch is explicit: where a cluster's auth cannot be satisfied
in-process, a profile may be marked **Use kubectl** and the driver
shells out to `kubectl --context <c> -o json` through
`hs_core::paths::background_command`. Detected binaries and their
versions are listed in Settings the way language servers are, including
the note that a tool installed while the app is open needs **Refresh
Status** (`hs_core::env_path::refresh`).

### 10.4 `DataTable` must scroll before any pane is built

A deferred bug becomes a prerequisite. `hs_ui::table::DataTable` divides
the width it is handed and then **clips**, so columns off the edge
cannot be reached; it is already a known fault against S3 and the SFTP
pane. Both panes here are wide tables, and the owner asked for
horizontal and vertical scrolling.

`tool-db::grid` has the working pattern: an
`egui::ScrollArea::horizontal` whose content width is computed from the
columns' own widths, with **both** `set_min_width` and `set_max_width`
on the inner `Ui` — `available_rect_before_wrap` is unbounded inside a
horizontal scroll area, so a trailing `remainder()` column would grow
without bound. The fix lands in `DataTable`, where S3 and SFTP get it
too.

### 10.5 Shell: rail, dashboard, sidebar, tabs

- **Rail:** between S3 and Database, which is "between SSH and
  Database" as the owner described it. The rail is per workspace and
  user-ordered (`[rail]` in `workspace.toml`, Settings → Appearance), so
  this is only the default. Activity icon `ph::SHIPPING_CONTAINER`.
- **Per-kind icons everywhere** — Docker, Podman, Kubernetes
  (`ph::POLYGON`), Windows containers (`ph::WINDOWS_LOGO`) — the same
  "what the thing *is*, not where it was found" rule the Logs sidebar
  now follows.
- **Dashboard:** a permanent pinned home tab. Stats: hosts configured ·
  reachable now · running · stopped · images · volumes · and for a
  cluster nodes ready/total, pods running, pods unhealthy. Start Here:
  Add Container, Open Compose File…, Open Manifest…, **Scan Local**
  (finds a Docker or Podman socket, WSL distributions and a kubeconfig
  without being told where — read-only, and it never starts anything).
  Each finding is a row with **Add**, which saves it as an ordinary
  `ConnectionProfile` — nothing here is a second store. A finding the
  workspace already has reads **Already added** instead: both sides
  answer `hs_container::scan::key_of`, one definition on purpose,
  because two functions that must agree about identity are two
  functions that will stop agreeing.
  Rows: each saved host, recent manifests, and anything unhealthy as a
  direct link. Counts come from a Job with a short per-host cache; the
  Logs service scan learned the cost of a probe per tab the hard way.
  The dashboard reports **no** `current_uri` and **no** `uri_affinity`,
  or it would take every `OpenRequest` for the activity (the Devtools
  lesson).
- **Sidebar**, two sections each scrolling on its own, as Logs does:
  **Containers** (~40%) — saved hosts and clusters with a drag grip,
  kind icon, name, subtitle and a live dot; and **Saved Commands and
  Configs** (~60%) — compose files, stack files, kubeconfig contexts,
  manifests and saved one-off commands. An **Add Container** button goes
  to Settings → Containers → Container List with the add form open,
  exactly as Add Connection does from the other activities. Saved
  commands live in `<workspace>/containers/*.toml`, each file named
  `slug(name)` — a file under any other name is duplicated the first
  time anything saves it (the connection-profile gotcha).
  Each host row carries the shared connection-row context menu
  (`hs_ui::form::row_menu`): **Edit This Host…**, which opens Settings →
  Containers with that host selected, and Pin to Home.
- **One tab per host.** `current_uri` is `container://<profile-id>`, so
  clicking a host focuses the tab that already shows it rather than
  opening a second (`TabInfo::uri`, as the Logs sidebar uses).

### 10.6 Work panes

**Docker, Portainer-shaped:** two columns, the right one in two rows — a
resource rail (Containers, Stacks, Images, Volumes, Networks, and under
Swarm: Services, Nodes), a filterable table, and a detail block with
tabs (Overview, Env, Mounts, Networks, Stats) over a fixed row of verbs
(Start, Stop, Restart, Logs, Exec, Inspect).

**Every list has a detail block (2026-10-25).** Containers and Compose
projects have verbs of their own; images, volumes, networks, stacks,
services and nodes have facts, and the facts are exactly what a column
width cuts off — a twelve-character image id, a mount point, a port map.
They share one `facts_detail` block fed by `docker::summary`, which lives
beside `docker::table_data` because it is the long form of the same
columns. An image also says **which containers are running it**, matched
on the reference or on the id (by prefix: the engine abbreviates it in
some outputs), since that is the question an image row is opened to ask.

**One tab per host, through the connection and not a mime type.** The
shell reuses a tab for an `OpenRequest::Connection` whose
`current_connection` matches and for an `OpenRequest::File` whose uri
matches — a `Text` request has no such rule, so clicking a host in the
sidebar opened another tab every time (owner's report, 2026-10-25). A
container host *is* a saved connection profile, so `open_host` asks for
it as one and the tool answers `current_connection`; `current_uri` stays
for the Logs and search handoffs. The `Text` route still opens a host, so
nothing that already sends one breaks.

**Kubernetes, Lens-shaped:** a resource rail, a namespace-scoped list,
and a detail block (Summary, containers, YAML; Logs / Exec / Scale /
Restart arrive with phase 7, which is where the actions live).

Built as **two columns with the detail under the list** rather than as
three columns (phase 6): at this window's 1720 px minimum a third
column leaves the list too narrow for a pod name and a status, the
Docker pane next door already puts its detail underneath, and "one
UI/UX language across every activity" settles the rest. The namespace
picker sits **above the rail**, not in the list's toolbar, because it
narrows every list rather than the one on screen — and changing it is a
**re-read**, not a filter over what is already held, since a widened
namespace has rows nothing fetched.

**A secret's values never reach the YAML view.** They are replaced with
a marker before the object is rendered and its keys are kept — which
keys exist is the question a reader has. Redacting `data` alone is not
enough: `kubectl apply` leaves a
`kubectl.kubernetes.io/last-applied-configuration` annotation holding
the object **as applied, values in the clear**, and a bench secret duly
showed `"DB_PASSWORD":"hunter2"` under a redacted `data` block
(2026-10-24). That annotation is therefore dropped for **every** kind,
not for secrets alone — one rule with no hole for the next kind that
turns out to carry something. `managedFields` is dropped with it, as
server-side-apply bookkeeping nobody opened the view to read.

YAML is fetched **per object, on request**. Carrying it with the list
would make scrolling a list of pods a conversation with the API server.

**Writing to a cluster (phase 7).** A pod offers Logs; a workload
offers Restart, Scale and Delete; everything else offers Delete. Three
rules decide how they behave, and each was learned next door:

- **Restart is a rollout, not a delete.** The API has no restart verb -
  a controller only reacts to a change in its pod template - so the
  `kubectl.kubernetes.io/restartedAt` annotation *is* the restart.
  Deleting the pods or scaling to zero and back would lose the surge
  and availability guarantees that make a rollout safe.
- **Scale asks first**, with Delete rather than with Restart: scaling
  to zero takes a service down as surely as deleting it, and scaling up
  costs money on a cluster that autoscales its nodes. The question
  shows the number back before it is sent.
- A refused verb is **disabled and says why**, never hidden, and a
  write **re-reads the cluster on the same worker** so the list that
  comes back is the one after it.

**Apply goes through the server, never through a guess.** The YAML view
is editable; **Dry Run** sends it as a server-side apply with
`dryRun=All` and the preview is the difference between what the cluster
holds now and what the server says it would hold - defaulting,
admission webhooks and other controllers' fields included. **Apply
stays disabled until a dry run has come back**, so nothing reaches a
cluster that the server has not already described. The field manager is
`hyperstack`: what this app owns is recorded, and a later `kubectl
apply` conflicts loudly instead of silently reverting someone's work.
A manifest of several documents is split and applied one at a time, so
a failure names *which* document failed rather than leaving the rest in
doubt. The one exception to server-side apply is **Scale**, which sends
a merge patch to the `scale` subresource exactly as `kubectl scale`
does - applying the replica count would make this app the owner of a
field the deployment's own manifest sets.

**Custom resources are a rail entry, not a special case.** The
definitions are read from `apiextensions.k8s.io` rather than by walking
discovery over every group - a dozen round trips to answer a question
about the handful of kinds somebody installed - and the list shows
kind, group, version and scope. A definition is itself an object, so
its YAML is its schema; **List Objects** then lists the objects of that
kind by name, namespace and age, and no further columns, because a
custom resource's fields are whatever its author chose and the YAML
view is where they are read. The storage version is the one listed: a
CRD may serve several and only one is what the cluster keeps.

**Port forward** opens `127.0.0.1:<free port>` onto a pod's port
through the API server's `portforward` subresource, shows the address
as a link, and stops on request. It does **not** go through the
`Tunnels` service: that interface is SSH-shaped - its `ForwardSpec`
carries an `ssh_id` and its implementation is `hs_ssh`'s tunnel
manager - and routing a Kubernetes forward through it would mean
inventing an SSH connection that does not exist. The local port is
always chosen by the OS, which is the only answer that cannot collide
with something already listening; the pod's first **declared**
container port fills the field as a starting point, and the field
stays editable because a pod may listen on a port it never declared.
A forward survives a change of selection - it is a connection someone
is using, not part of the detail block - and is closed by Stop or by
closing the tab.

**Exec opens the pod's shell in the Terminal activity** (§10.1), with
no `kubectl` and no local command: the API server's `exec` subresource
is a WebSocket, `hs_container::exec` wraps it as an `hs_term`
transport exactly as `hs-ssh` wraps a shell channel, and the Terminal
activity runs it without knowing which kind it has. The hand-over
travels in `OpenRequest::Terminal`'s existing `host` field as
`k8s://<conn id>/<namespace>/<pod>/<container>`, so no new request
variant is needed. Which shell runs is **Settings → Containers → Shell
for Exec**, the same setting the Docker pane uses, because a minimal
image has no bash and a distroless one has no shell at all.

A cluster Exec is **guarded like a verb**: a shell inside a pod is
unbounded write access, and protected environments default to
read-only in every tool. The Docker pane's Exec is not guarded today -
it types a command into a terminal the user owns - so the two panes
differ; aligning them belongs with a change to the Docker side rather
than with this one.

**"All namespaces" is a choice, not an absence.** The profile's
namespace seeds the picker until the reader picks one; without
distinguishing "not yet chosen" from "chosen: all", selecting All
namespaces empties the field, and the read that follows puts the
profile's namespace straight back - the option is then unreachable on
every profile that names one (found on the bench, 2026-10-24).

Both panes scroll vertically and horizontally per §10.4.

**A published port is a link (owner's request, 2026-10-24).** Every port
in a pane that can actually be reached is a button that opens the
address in the Browser activity, with a right-click menu offering the
same by name and Copy Address. The arithmetic is `hs_container::web`,
and each part of it has a wrong answer that looks right: `0.0.0.0` and
`::` mean *every* interface rather than an address, so they become the
engine's own machine — `localhost` for a local or **WSL** engine
(Windows forwards a published port to the Windows localhost), the SSH
profile's hostname for a tunnelled one, which only the tool can resolve
because `hs-container` must not depend on `hs-ssh`. An IPv6 literal is
bracketed exactly once. The conventional TLS ports (443, 8443, 9443,
4443, 10443) get `https`, the rest `http`: a wrong guess costs one
redirect, while refusing to guess costs every HTTPS service a broken
link. A port that is exposed but not published, one that is not TCP,
and one on a host that cannot be named are shown as plain text with a
tooltip saying why — a link that goes nowhere is worse than no link.

**Logs and Exec are handed to the tools that already do them** (§10.1).
View Logs opens a Logs tab following `<engine> logs --follow --tail
<n> --timestamps <id> 2>&1`; the redirect is not cosmetic — `docker
logs` writes the container's **stderr to its own stderr** and a Logs
command source reads stdout, so without it a tab following portainer,
nginx or any of the many images that log to stderr stays empty
(measured, 2026-10-24). Exec types `<engine> exec -it <id> <shell>` into
a Terminal tab, where the shell is **Settings → Containers → Shell for
Exec**: no single answer suits every image — a minimal one has no bash,
and a distroless one (portainer) has no shell at all and says so.

**Compose and Swarm (phase 5).** The rail gains **Projects** above the
images, and a **Swarm** group — Stacks, Services, Nodes — below them.

A Compose project is not something the engine stores: it is a set of
containers wearing `com.docker.compose.project`, so the pane rebuilds
projects from a plain `ps` and never keeps a registry of its own that
would go stale the moment someone ran `docker compose` in a terminal.
The *files* a project came from cannot be read from those labels
reliably, though: Compose joins several with a comma and `docker ps
--format '{{json .}}'` joins the whole **label set** with a comma too,
so the second file arrives looking like a label with no `=` and is
dropped. `docker compose ls -a --format json` reports them as their own
field and wins where the two disagree; the label is the fallback for an
engine too old to have it, where one file — the common case — is still
right. `-a`, or a project brought down vanishes from the pane and looks
deleted.

The verbs shell out to `docker compose` rather than being reimplemented
against the engine API: dependency order, profiles, `depends_on`
conditions and build contexts are Compose's job. `up` is always `-d`,
or the command tails the logs and the Job waits for a process that will
not end. **A project with no known compose file is offered only the
verbs that work from the project label** (Start, Stop, Restart, Down)
and told why: `up` without a file is run against whatever is in the
current directory, which is how a project gets recreated as something
else entirely. `down` is destructive — it removes the containers and
the network — so it asks first under the same setting that guards a
container's own destructive verbs, and the question says that volumes
are kept.

Swarm is the one surface that may simply not be there, and "not in a
swarm" is a **state to show, not an error to report**: the remedy is
`docker swarm init`, which is a decision for the person. `docker info`
is read first and the three Swarm rows are disabled with that answer as
their tooltip — disabled rather than hidden, because a rail that
changes length between hosts is harder to use. A **worker** is told
apart from "not a swarm": it is in one but cannot be asked about
stacks, and the words differ. Off a swarm the three listings are not
run at all; three failing processes per refresh to learn what `info`
already said is waste.

Compose and `stack deploy` write their progress to **stderr** and keep
stdout for machine-readable output they mostly do not produce, so these
verbs go through `run::output_with_stderr` and the pane shows what came
back. A caller reading stdout alone would report a successful deploy as
an empty string and look like it had done nothing. (The listings keep
using `run::output`: they parse stdout as JSON and must not have
progress text mixed in.)

**Windows containers** use the Docker pane against a Windows engine,
showing `isolation` (process or hyperv), with `cmd`/`powershell` for
Exec. Honest scope: no Hyper-V VM management, and no Windows networking
beyond what the engine reports.

### 10.7 Settings → Containers

A page between `Connections` and `Editor` in `Page::all()`. Cards:

1. **Container List** — the kind-filtered view of the connection store,
   sharing the row component with Settings → Connections: drag grip,
   kind icon, name, subtitle, environment and read-only badges; Add,
   Duplicate, Test, Delete. One store, so the two lists can never show
   different orders for the same host. The four **Add** buttons (Docker,
   Podman, Kubernetes, Windows containers) are one row across the whole
   card, at a readable size: inside the 260 px list column they wrapped
   over three lines and read as a list of their own (owner's
   requirement, 2026-10-25).
2. **Connection form**, per kind. Docker/Podman: socket, named pipe or
   TCP, TLS (CA, cert, key — vault), over SSH through a saved `ssh`
   profile, WSL distribution, registry credentials (vault), API version
   pin. Kubernetes: kubeconfig path + context or a pasted document
   (§10.2), default namespace, bearer token (vault), impersonation,
   proxy, Use kubectl (§10.3), `insecure-skip-tls-verify` with a warning
   that says what it gives up. Common: name, folder, tags, colour,
   environment, read-only, connect timeout.
3. **Defaults and Safety** — confirm before stop/restart/kill/remove;
   require typing the name for remove and namespace delete; protected
   environments read-only (on, with the switch explaining it); mask
   Secret values until revealed; `system prune` off by default and never
   offered on a protected environment.
4. **Refresh and Limits** — live refresh interval; whether to stream
   `/events` and `/stats` (off for a slow link); list page size; log
   tail lines handed to the Logs tab; driver timeout.
5. **Tools** — `docker`, `podman`, `kubectl`, `helm` and
   `docker compose` found on PATH, their versions and a path override
   each, in the table shape the Editor page already uses.
6. **YAML and Manifests** — manifest folder; Apply always dry-runs first
   (on); keep a copy of what was applied; YAML tab width.

### 10.8 The AI assistant

Hard right of the toolbar through `hs_ui::toolbar::hard_right`, as every
activity does. Answers already render as Markdown — the shared panel
does that, with tables that fit the pane.

**Prompt buttons need no migration.** `prompts::resolve` already lets
the saved list win and carry the user's edits, then appends any shipped
default it does not mention, so a button added by a later version is
appended rather than lost. Adding `container` to `prompts::ACTIVITIES`
and its buttons to `DEFAULTS` is the whole change, and no existing
activity's buttons are touched. The buttons: Explain This Object · Write
a Deployment · Write a Service · Write an Ingress · Write a ConfigMap ·
Write a PVC · Write a Compose File · Review This YAML · Fix This YAML ·
Why Is This Pod Unhealthy? · Right-Size Requests and Limits · Explain
This Event.

**Action blocks**, mirroring the API client's `hs-api`:

| Block | Offered |
|---|---|
| `yaml` | Apply (dry run first), Replace the YAML pane, Save to Manifests… |
| `hs-container` | Apply — a structured action the tool carries out itself (scale, restart, set image) |
| `sh`, `powershell` | Insert, and **Insert and Run** into the container's shell |

**As built (phase 8).** The `yaml` block lands in the **YAML pane**
rather than being applied from the answer: Dry Run and Apply already
live there, so a manifest the assistant wrote reaches a cluster by
exactly the route a typed one does - the server describes it first, and
a person presses Apply. An `hs-container` block is one JSON object with
an `action` of `scale`, `restart` or `set_image`; it is parsed, shown
back in a **confirmation naming the object**, and then carried out
through the same verb and apply paths a click uses, guards included. An
action this pane does not know is refused in words - the one thing
worse than "I cannot do that" is doing something else. **Set Image is
an apply, not a verb**: the manifest names the workload and the single
container being changed, so server-side apply changes that image and
claims ownership of nothing else.

The `sh` and `powershell` blocks are Insert-only for now: the Docker
pane's Exec types a command into a terminal the user owns, and running
one from an answer would be a second path to the same place with
weaker provenance. The `can_run` route stays available for when that
is wanted.

`can_run` is **false on a protected environment**, so the button is
absent rather than present and refusing (ADR-019).

### 10.9 Unhealthy objects reach the Problems inbox

`hs_core::problems` did not exist when §1–§9 were written. A container
that exits non-zero, a pod in `CrashLoopBackOff`, a failing readiness
probe: one row per shape with a climbing count, cleared when the tab
closes, visible in the dock without watching the pane.

**As built (phase 8).** The shape is the **object**, not the message,
so a pod that flaps between `CrashLoopBackOff` and `Error` is one row
that climbs rather than two that interleave. What counts as a problem
is the same judgement the header's "needs attention" makes - a pod or
workload the driver marked as trouble, a node that is not ready, a
container that stopped or whose health check is failing - so a row in
the inbox and a number in the pane never disagree. Posting happens
**once per read**, not once per frame: a pane that posted every repaint
would be leaning on the cooldown to do the caller's job.

### 10.10 Phases

Each ends green on the full gate and is releasable on its own.

| # | What |
|---|---|
| 1 | `DataTable` scrolls (§10.4, fixing S3 and SFTP too); `podman`/`wincontainer` kinds; Settings → Containers with the Container List and the per-kind forms; vault wiring |
| 2 | `hs-container` Docker/Podman driver: the CLI across all four transports (§10.3a), with live tests behind an env var as the DB and Redis drivers have. `bollard` deferred to phase 4, where streaming is actually needed |
| 3 | The activity, rail entry, dashboard, two-section sidebar, one tab per host |
| 4 | Docker pane: containers, images, volumes, networks; start/stop/restart; the Logs and Exec handoffs; clickable published ports; **Scan Local** on the dashboard |
| 5 | Compose and Swarm (`docker compose`, `docker stack` — CLI-only surfaces) |
| 6 | Kubernetes driver and the Lens-shaped pane: namespaces, workloads, events, YAML |
| 7 | Apply with the dry-run diff, port-forward, CRD discovery, and a pod's Logs / Exec / Scale / Restart. The forward is **not** routed through `Tunnels`: that service is SSH-shaped (`ForwardSpec.ssh_id`) and a Kubernetes forward has no SSH connection to name |
| 8 | AI assistant, prompt buttons, action blocks, the Problems feed |

### 10.11 Still out of scope

Image building and registry browsing beyond pulling by name (§4 already
defers these); Helm release management past detecting the binary;
cluster provisioning; Hyper-V VM management; editing a kubeconfig's auth
plugins; and any polling while the activity has no tab open.
