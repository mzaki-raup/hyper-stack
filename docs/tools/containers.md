# Tool spec — Containers (Docker / Podman / Kubernetes)

**Crate:** `tool-containers` · **Priority:** P1 (Docker/Podman, Compose, remote over SSH) · P2 (Kubernetes) · **Status:** draft

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
