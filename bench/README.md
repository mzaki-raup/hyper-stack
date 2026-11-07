# HyperStack bench kit

Every server HyperStack's activities are tested against, as one Docker
Compose file, with the mock and demo data the tests and README screenshots
used, and the fixture files (SQLite source/target, CSV, logs, images, git
repositories). It mirrors the Windows/WSL bench on the owner's dev machine
(the `hs-*` containers): same images, same host ports, same user names, so a
HyperStack profile works against either by changing only the host.

> **No password is committed.** Every password, secret and token lives only in
> your `.env`, which is gitignored; `.env.example` has `change-me` in their
> place, and the scripts and config read them from the environment. These are
> still test servers: run the kit on a private network and never forward its
> ports to the internet.

## What is here

| Path | What |
|---|---|
| `docker-compose.yml` | 30 services in five profiles, named volumes, healthchecks, memory limits |
| `.env.example` | Every variable; passwords are `change-me`. Copy to `.env` (gitignored) and fill them in |
| `config/` | Files mounted into containers: the Kafka SASL JAAS template, Jenkins' init script and job definitions, the PuppetDB fake's nginx config and recorded answers |
| `seed/seed-all.sh` | Loads all demo data into whatever is running; idempotent |
| `seed/seed-<service>.sh` | One script per service (`seed-k8s.sh` for k3s, `seed-heavy.sh` for the whole heavy profile) |
| `seed/data/` | The data the seeds load: SQL dumps, Mongo/Couch/Qdrant JSON, Redis commands, S3 and WebDAV files, RabbitMQ definitions, Kafka messages, the Redmine seed, the Kubernetes manifests |
| `fixtures/` | Files the app was tested with (below) |
| `fixtures/make-git-fixtures.sh` | Recreates the git fixture repositories from scratch |
| `.state/` | Written by the seeds: generated tokens, the k3s kubeconfigs (gitignored) |

## Requirements

- Linux with Docker Engine and the Compose plugin **v2.24 or later**.
- `bash`, `curl` 7.75 or later (the S3 seed signs with `--aws-sigv4`), and
  `git` for the git fixtures. Nothing else on the host: every other tool runs
  inside the containers.
- 16 GB of RAM for `core` + `queue` + `ci` (or `core` + `queue` + `k8s`) together; see below for `heavy`.

## Quick start

```bash
cd bench
cp .env.example .env            # then set every change-me, and BENCH_HOST to this machine's LAN IP
chmod +x seed/*.sh fixtures/*.sh
docker compose --profile core --profile queue up -d
./seed/seed-all.sh
```

Add `--profile ci` or `--profile k8s` (and later `--profile heavy`, a service or two at a time)
when you need them, then run `./seed/seed-all.sh` again: it skips what is not
running and re-loads the rest without duplicating anything. Run one service's
seed alone with `./seed/seed-all.sh redis` or `./seed/seed-kafka.sh`.

**`BENCH_HOST` matters when HyperStack runs on another machine.** Kafka hands
clients the address in `KAFKA_ADVERTISED_LISTENERS`; with the default
`127.0.0.1` a client on your Windows box connects, receives `127.0.0.1:9092`
and then fails. Set `BENCH_HOST` to the Linux box's IP (or DNS name) before
the first `up`, or `docker compose up -d kafka kafka-sasl forgejo` again after
changing it. It also sets Forgejo's links and the k3s certificate's SAN.

## Profiles and memory

The limits are ceilings (`mem_limit`); idle use is much lower.

| Profile | Services | Limit total | Typical use |
|---|---|---|---|
| `core` | postgres, mysql, mongo, redis, valkey, couch, s3 (MinIO), qdrant, ssh, webdav, httpbin | 5.1 GB | 1.5-2 GB |
| `queue` | rabbitmq, kafka, kafka-sasl, nats, mqtt | 2.9 GB | 1-1.5 GB |
| `ci` | forgejo, forgejo-runner, jenkins, teamcity, teamcity-agent, redmine, puppetdb-fake | 5.8 GB | 3-4 GB |
| `k8s` | k3s (single-node Kubernetes) with the demo workloads | 2 GB | about 0.8-1 GB |
| `heavy` | mssql (2 GB), oracle (2.5 GB), db2 (4 GB), clickhouse (1.5 GB), nexus (2.5 GB) | 12.5 GB | each at its limit once warm |
| `tools` | nats-box (only run by the NATS seed) | - | - |

`core` + `queue` fit easily in 16 GB with the OS (about 3-4 GB in practice);
`core` + `queue` + `k8s` fit with room to spare (about 4-5 GB in practice), and
`core` + `queue` + `ci` fit too. Start `heavy` services **one or two at a
time** (`docker compose --profile heavy up -d mssql`) and stop them when done
(`docker compose stop mssql`). SQL Server, Oracle and Db2 accept connections
minutes before they are ready (Oracle 3-5 min on first start, Db2 up to 10);
their healthchecks probe the database itself, and the seed waits on them.

## Credentials

The values are in your `.env` (see `.env.example`); the table names the
variable that holds each one. User names, ports and databases are the same on
this kit and on the Windows/WSL bench unless the last column says otherwise.
`<host>` is `BENCH_HOST` (or `127.0.0.1` on the box).

| Service | Address | User | Password / token | Database / scope | Windows/WSL bench |
|---|---|---|---|---|---|
| PostgreSQL | `<host>:5432` | `$POSTGRES_USER` (`dev`); also `$PG_APP_USER` (`app`) and `postgres` | `$POSTGRES_PASSWORD`; `$PG_APP_PASSWORD`; `$PG_SUPERUSER_PASSWORD` | `shop` (also `postgres`) | same three roles |
| MySQL | `<host>:3307` | `$MYSQL_USER` (`dev`), also `root` | `$MYSQL_PASSWORD`; root `$MYSQL_ROOT_PASSWORD` | `shop` | same |
| MongoDB | `<host>:27017` | none needed (auth off), or `$MONGO_USER` (`dev`) | `$MONGO_PASSWORD` | `shop`, `catalogue`, `hstest`; authSource `admin` | same |
| Redis | `<host>:6381` | - | none | db 0 | same |
| Valkey | `<host>:6380` | - | none | db 0 | same |
| CouchDB | `http://<host>:5984` | `$COUCHDB_USER` (`admin`), also `dev` | `$COUCHDB_PASSWORD`; `dev`: `$COUCH_DEV_PASSWORD` | `notes`, `orders` | same |
| S3 | `http://<host>:9000` (console `:9001`) | access key `$S3_ACCESS_KEY` (`dev`) | `$S3_SECRET_KEY`, region `us-east-1`, path-style | buckets `hyperstack`, `backups` | **adobe/s3mock: it checks no credentials at all; any key and secret work** |
| Qdrant | `http://<host>:6333` (gRPC `:6334`) | - | no API key | `articles`, `faces` | same |
| SSH / SFTP | `<host>:2222` | `$SSH_USER` (`dev`) | `$SSH_PASSWORD` (sudo allowed) | - | same |
| WebDAV | `http://<host>:8086/` | `$WEBDAV_USER` (`dev`) | `$WEBDAV_PASSWORD` | served at `/` (not `/dav`) | same; a profile pointing at `/dav` gets 404 |
| httpbin | `http://<host>:8088` | - | none | - | same (container stopped) |
| RabbitMQ | AMQP `<host>:5672`, management `http://<host>:15672` | `$RABBITMQ_USER` (`dev`) | `$RABBITMQ_PASSWORD` | vhost `/` | same |
| Kafka | `<host>:9092` PLAINTEXT | - | none | `orders.created`, `payments`, `billing` | same |
| Kafka SASL | `<host>:9094` SASL_PLAINTEXT | `$KAFKA_SASL_USER` (`dev`: PLAIN, SCRAM-SHA-256, SCRAM-SHA-512); `$KAFKA_SASL_ADMIN_USER` (`admin`: PLAIN) | `$KAFKA_SASL_PASSWORD`; `$KAFKA_SASL_ADMIN_PASSWORD` | `orders` | same |
| NATS | `nats://<host>:4222`, monitor `http://<host>:8222` | - | none | JetStream stream `ORDERS` | same |
| MQTT | `<host>:1883` | anonymous | none | `home/#`, `factory/#`, `fleet/#` | same |
| Forgejo | `http://<host>:3000` | `$FORGEJO_ADMIN_USER` (`dev`, admin) | `$FORGEJO_ADMIN_PASSWORD`; token in `.state/forgejo-token` | repo `dev/demo` | same user; its own token |
| Jenkins | `http://<host>:8091` | `$JENKINS_USER` (`dev`) | `$JENKINS_PASSWORD`; API token in `.state/jenkins-token` | jobs `unit-tests`, `deploy-staging` | same user; its own token |
| TeamCity | `http://<host>:8111` | `$TEAMCITY_ADMIN_USER` (`admin`) | `$TEAMCITY_ADMIN_PASSWORD` | project `Bench`, `Bench_Hello` | same |
| Redmine | `http://<host>:3010` | `admin` | `$REDMINE_ADMIN_PASSWORD`; API key in `.state/redmine-api-key` | project `demo` | same user; its own key |
| PuppetDB fake | `http://<host>:8082` | - | none | 3 nodes, 4 reports | same |
| SQL Server | `<host>:1433` | `sa` | `$MSSQL_SA_PASSWORD` | `shop` | same (stopped) |
| Oracle Free | `<host>:1521`, service `FREEPDB1` | `$ORACLE_APP_USER` (`app`), also `system` | `$ORACLE_APP_PASSWORD`; system `$ORACLE_PASSWORD` | schema `APP` | same (stopped) |
| Db2 | `<host>:50000` | `db2inst1` | `$DB2INST1_PASSWORD` | `$DB2_DBNAME` (`TESTDB`) | same (stopped) |
| ClickHouse | `http://<host>:8123` | `$CLICKHOUSE_USER` (`default`) | `$CLICKHOUSE_PASSWORD` | `shop` | same (stopped) |
| Nexus | `http://<host>:8081` | `admin` | `$NEXUS_ADMIN_PASSWORD` | repository `hs-raw` | same (stopped) |
| k3s (profile `k8s`) | `https://<host>:6443` | context `hs-bench` (client certificate) | `.state/k3s/kubeconfig.yaml`; cluster token `$K3S_TOKEN` | namespaces `shop`, `ops` | did not start (needs `--privileged`); the kit runs it privileged |

The generated tokens (Forgejo, Jenkins, Redmine) differ per installation;
`seed-all.sh` writes each to `.state/` and prints where.

## What the seeds load

| Service | Data |
|---|---|
| PostgreSQL | `shop`: `customers` 5, `products` 5, `orders` 5, `order_lines` 7, view `order_totals` |
| MySQL | `shop`: `agents` 4, `tickets` 6 (enum priority, a foreign key) |
| MongoDB | `shop.orders` 3, `shop.customers` 1, `catalogue.products` 5 (unique `sku`), `catalogue.reviews` 4, `hstest.orders` (empty) |
| Redis / Valkey | 23 / 11 keys: `app:*`, `cart:*`, `user:*` hashes, `session:*`, `queue:*` lists, `tags:*` sets, `leaderboard` zset, `events` / `events:signup` streams, `alpha`, `zeta` |
| CouchDB | `notes` 3 docs, `orders` 2 docs |
| S3 | `hyperstack`: `data.csv`, `docs/{api/b.txt,guides/a.txt,notes.md,readme.txt}`, `images/logo.png`, `src/{lib/c.txt,main.rs}`; `backups` empty |
| Qdrant | `articles` (5 points, dim 4, cosine), `faces` (1 point, dim 3, euclid) |
| WebDAV | `/documents/plan.md`, `/documents/2026/standup.txt`, `/photos/holiday.jpg`, `/hs-check/`, `/hello.txt` |
| RabbitMQ | exchange `shop` (topic) bound to `orders.created` on `order.*`; queues `demo` (3 ready messages), `orders.created`, `emails.outbound` |
| Kafka | `orders.created` (3 partitions, 12 keyed orders), `payments`, `billing`; group `billing` read 4 and lags 8 |
| Kafka SASL | SCRAM users; `orders` (3 partitions, 6 orders: 4 paid, 2 pending); group `billing` read 2 and lags 4 |
| NATS | stream `ORDERS` (`orders.>`, 9 messages), durable pull consumer `shipper` with nothing delivered |
| MQTT | retained `home/kitchen/temperature`, `home/kitchen/humidity`, `home/garage/door`, `factory/line1/status`, `factory/line2/status` (fault E42), `fleet/truck-7/gps` |
| Forgejo | admin `dev`, repo `dev/demo` with `.forgejo/workflows/build.yml` (`workflow_dispatch` with a choice and a boolean input), branch `dev`, an issue; the runner registered as `hs-runner` (`docker:host`) |
| Jenkins | `unit-tests` built green, `deploy-staging` (parameters ENV, FAIL, STEPS) built red |
| TeamCity | project `Bench`, build configuration `Hello Build` (`GREETING` parameter, a step that prints for five minutes), agent `hs-agent` authorised |
| Redmine | default trackers, statuses and priorities; REST API on, CommonMark; project `demo` with 3 issues |
| PuppetDB fake | `db1.example.com` clean, `staging1.example.com` noop with changes, `web1.example.com` failed (nginx won't start) |
| SQL Server / Oracle / Db2 | the Postgres `shop` tables and rows, in `shop` / schema `APP` / `TESTDB` |
| ClickHouse | `shop.products` (one negative, one zero stock), `shop.events` (48 hourly rows) |
| Nexus | raw hosted `hs-raw` with `docs/readme.txt`; the admin password changed from `admin123` |
| k3s | `shop` and `ops` - see [Kubernetes](#kubernetes-profile-k8s) |

### TeamCity's first start (by hand, once)

TeamCity's first-start wizard has no API. After `up`, open
`http://<host>:8111`, choose **Proceed**, keep the **internal (HSQLDB)**
database, accept the licence agreement, and create the administrator
`$TEAMCITY_ADMIN_USER` / `$TEAMCITY_ADMIN_PASSWORD` (your `.env` values). Then run
`./seed/seed-teamcity.sh`. Until then the `teamcity` container reports
unhealthy and the seed says what is missing.

## Kubernetes (profile `k8s`)

A single-node k3s (`rancher/k3s:v1.37.2-k3s1`, the k3s `stable` channel's
release on 2026-11-08) for the Containers activity's Kubernetes views. It
runs privileged (k3s runs containerd and the kubelet inside the container)
and needs about 0.8-1 GB once the demo workloads are up.

```bash
docker compose --profile k8s up -d
./seed/seed-all.sh k8s          # or just ./seed/seed-all.sh
```

The seed waits for the node to be Ready, applies `seed/data/k8s/*.yaml` with
`kubectl apply`, and writes two kubeconfigs with the cluster, user and
context all named `hs-bench`:

| File | Server | Use it from |
|---|---|---|
| `.state/k3s/kubeconfig.yaml` | `https://$BENCH_HOST:6443` | other machines (your Windows box) |
| `.state/k3s/kubeconfig-local.yaml` | `https://127.0.0.1:6443` | the Linux box itself |

A re-run creates nothing new and restarts nothing. kubectl still prints
`statefulset.apps/cache configured` each time, a known client-side apply
quirk with `volumeClaimTemplates`: `kubectl diff` is empty and the pod is
not replaced.

The API server's certificate carries `BENCH_HOST` as a SAN (`--tls-san`),
so TLS verifies for that name. If you change `BENCH_HOST`, recreate k3s
(`docker compose --profile k8s up -d --force-recreate k3s`) and run the seed
again. The kubeconfig holds a client certificate and key: treat it as a
credential (it stays in `.state/`, which is gitignored).

**In HyperStack:** copy `kubeconfig.yaml` to the Windows machine (for
example into the data folder, so the profile stores a relative path). Then,
in the Containers activity (or Settings → Containers → Container List), use
**Add** → **Kubernetes**: set **kubeconfig File** to that path, **Context**
to `hs-bench`, and **Default Namespace** to `shop`. The file is read where
it is, not copied into the vault (docs/tools/containers.md §10.2); **Paste
kubeconfig** is the alternative when you want the document in the vault.

What the cluster holds:

| Namespace | Object | State |
|---|---|---|
| `shop` | Deployment `shop-web` (nginx:alpine, 2 replicas, readiness probe) + Service | 2/2 Running |
| `shop` | StatefulSet `cache` (redis:7-alpine, 1 replica) + headless Service, PVC `data-cache-0` (64Mi, `local-path`) | Running, PVC Bound |
| `shop` | Pod `ticker` (busybox) logging a line every 3 s at INFO/DEBUG/WARN | Running - for log follow and exec |
| `shop` | Job `db-migrate` | Completed |
| `shop` | CronJob `nightly-report` (`0 2 * * *`) | scheduled, no runs yet |
| `shop` | ConfigMap `shop-config`, Secret `shop-secrets` (the dummy value `demo-not-secret`) | - |
| `ops` | Deployment `crash` (exits 1 after logging an ERROR line) | CrashLoopBackOff, for the Problems inbox (`kubectl get pods` prints the last exit, `Error`, between restarts) |
| `ops` | Deployment `pull` (image `busybox:0.0.0-does-not-exist`) | ImagePullBackOff / ErrImagePull |

Every container requests at most 5m CPU and 16Mi memory. To start over:
`docker compose rm -sf k3s && docker volume rm hs-bench_k3s-data`.

## Fixtures (`fixtures/`)

| File | Used for |
|---|---|
| `sqlite/hs-bench.sqlite` | The "Bench SQLite" profile: `bank` (27 rows), `notes` (5) |
| `sqlite/compare-source.db`, `sqlite/compare-target.db` | Schema/data compare source and target (`customers`, `orders`, `order_lines`; target adds `legacy_audit`) |
| `sqlite/depth-h-source.db` | The depth programme's compare script source |
| `duckdb/shop.duckdb` | DuckDB driver checks |
| `csv/new-tickets.csv` | CSV import into MySQL `shop.tickets` |
| `logs/*.log` | Logs activity sources: plain-text levels (`hs-bench*.log`), JSON lines (`app.log`, `billing-worker.log`, `checkout-api.log`), a build log, file:line links (`hs-logprobe.log`) |
| `images/shop-banner.png` | Graphics / image viewer sample |
| `diagram/architecture-mermaid.txt` | Diagram generator input (flowchart + sequence diagram) |
| `s3-sync-local/` | The local side of the S3 folder sync |
| `editor/debug-demo/app.py`, `editor/lsp-demo/` | Debugger and LSP demos in the Editor |
| `git/shop-service-architecture.md` | Used by `make-git-fixtures.sh` |

`./fixtures/make-git-fixtures.sh [dir]` (default `./git-fixtures`) builds:

- `hs-git-fixture/` - branches `main`, `feature`, `topic`, `fix`; a merge of
  `fix` paused on a `README.md` conflict; an unstaged change to `main.rs`; a
  stash with an untracked file; tag `v0.1.0`.
- `shop-service/` - `main`, `develop`, `feature/payments`, `topic`, `fix`;
  tags `v0.1.0` (annotated) and `v0.2.0`; a stash; an unstaged change; an
  untracked `Cargo.lock`; a merge of `fix` paused on `README.md`; remote
  `origin` = `../hs-bare.git`.
- `hs-bare.git` - the bare "Localhost" remote of `shop-service`.

Re-running rebuilds those three folders only.

## Pointing HyperStack at the kit

In the app, use the addresses and credentials above in each activity's
profile. For the live driver tests (`.claude/rules/testing.md`), on the
machine that runs `cargo test`, with the values from your `.env` in the
shell (`set -a; . bench/.env; set +a` works when no value contains spaces):

```bash
H=192.168.1.50   # BENCH_HOST
export HS_PG=$H:5432:$POSTGRES_USER:$POSTGRES_PASSWORD:postgres
export HS_MYSQL=$H:3307:$MYSQL_USER:$MYSQL_PASSWORD:shop
export HS_REDIS=$H:6381
export HS_MONGO=$H:27017
export HS_COUCH=http://$COUCHDB_USER:$COUCHDB_PASSWORD@$H:5984
export HS_S3_ENDPOINT=http://$H:9000 HS_S3_ACCESS_KEY=$S3_ACCESS_KEY HS_S3_SECRET_KEY=$S3_SECRET_KEY
export HS_RABBITMQ=$H:15672:$RABBITMQ_USER:$RABBITMQ_PASSWORD
export HS_KAFKA=$H:9092
export HS_KAFKA_SASL=$H:9094:$KAFKA_SASL_USER:$KAFKA_SASL_PASSWORD
export HS_NATS=$H:4222
export HS_MQTT=$H:1883
export HS_REDIS_STREAMS=$H:6381
export HS_JENKINS=$H:8091:$JENKINS_USER:$(cat .state/jenkins-token)
export HS_FORGEJO=$H:3000:$FORGEJO_ADMIN_USER/demo:$(cat .state/forgejo-token)
export HS_FORGEJO_ISSUES=$H:3000:$FORGEJO_ADMIN_USER/demo:$(cat .state/forgejo-token)
export HS_REDMINE=$H:3010:$(cat .state/redmine-api-key)
```

(The `.state/` files live on the Linux box; copy the values across.) Note
the bench's MySQL is on `3307`, not the usual `3306`.

## Resetting

On **your Linux box** only:

```bash
docker compose --profile core --profile queue --profile ci --profile k8s --profile heavy down -v
rm -rf .state
```

`down -v` deletes this kit's containers and named volumes (`hs-bench_*`), so
the next `up` + `seed-all.sh` starts clean. It acts only on the compose
project in this folder: it never touches the `hs-*` containers of the
Windows/WSL bench, which are not part of any compose project. To reset one
service, `docker compose rm -sf <svc> && docker volume rm hs-bench_<svc>-data`.
