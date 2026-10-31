# Tool spec — Queue

**Crate:** `tool-queue` · **Status:** implemented · **Last updated:** 2026-10-31 ·
**Phase:** L of [docs/10-depth-programme.md](../10-depth-programme.md) ·
**Ledger:** `FEAT-0066` and the items it splits into ·
**Decisions:** [ADR-027](../08-decisions.md)

## 1. Purpose

Look into the message brokers a team runs — what queues and topics exist, how
deep they are, who consumes them and how far behind — and do the small things a
developer reaches for while debugging: read a few messages without taking them,
publish a test message, clear a queue that filled up in a test environment.

It is **not** a broker administration console. It does not create users,
policies, vhosts, ACLs, shovels or cluster topology, and it does not replace the
broker's own UI for that. Nothing here acknowledges or removes a message as a
side effect of looking at it.

## 2. User stories

- As a backend developer, I want to see that `orders.created` has 40 000
  messages ready and no consumer, so I know my worker is down before I read a
  single log line.
- As a developer, I want to read the next ten messages of a queue, payloads
  pretty-printed, **without consuming them**, so I can see what the producer
  actually sent.
- As a developer, I want to publish one message to an exchange or topic with
  headers I choose, so I can drive a consumer by hand.
- As a tester, I want to purge a test queue, and I want the app to stop me
  doing that on production.
- As an on-call engineer, I want consumer-group lag per partition for a Kafka
  topic, so I can tell a slow consumer from a stuck one.

## 3. Layout

The activity follows the connection-per-tab shape of `tool-redis` and
`tool-db`, so nothing about it has to be learned.

- **Dashboard** (the permanent home tab), through `hs_ui::dashboard`: the
  band, the summaries — *Brokers*, *Queues and Topics*, *Messages Ready*,
  *Open Tabs* — Start Here with **Add Broker**, **Saved Brokers** (a grid,
  each with its kind's icon), and Open Here. Figures come from the brokers
  open in tabs; a broker that is not open contributes nothing rather than
  being connected to just to fill a number.
- **Sidebar**: the saved brokers of every kind, one list, each row its kind's
  icon, name and `host:port`; the usual grip to reorder (`hs_ui::reorder`);
  a click opens the broker's tab or focuses it.
- **A tab per broker**: a header band (name, kind, server version, the
  environment), the toolbar (`hs_ui::toolbar`, the AI assistant hard right),
  and two panes:
  - left: the **destinations** of that broker as a `hs_ui::table::DataTable` —
    queues, topics, streams or subjects, whatever the kind calls them (§4) —
    with a filter box and the columns of §4 for that kind;
  - right: the selected destination's **detail**, in tabs: *Messages* (peek),
    *Publish*, *Consumers* (groups, lag), and *Bindings* for RabbitMQ.
- `hs_ui::env_banner` across a tab whose broker belongs to a protected
  environment; the whole tab is then read-only (§6).
- **Settings → Queue**: the saved brokers, list left and form right, like
  Settings → Git; and the defaults of §7 (peek count, poll interval).

## 4. Brokers

One `Driver` per kind behind one trait (§8), so the tab never asks what it is
talking to. What each kind shows and does:

| | RabbitMQ | Kafka | Redis Streams | NATS | MQTT |
|---|---|---|---|---|---|
| Destination | queue (per vhost) | topic | stream key | JetStream stream | topic, as seen |
| Columns | vhost, ready, unacked, consumers, publish / deliver rate, state | partitions, earliest / latest offset (summed), messages | length, groups, first / last id | messages, bytes, consumers, subjects | last payload size, retained, last seen |
| Peek | yes, requeued | yes, from an offset | yes, by id range | yes, by sequence | retained and live, for a window |
| Publish | to an exchange, routing key, headers, properties | to a partition or by key, headers | `XADD`, fields | to a subject, headers | topic, QoS, retain |
| Consumers | consumers per queue | groups, lag per partition | groups, pending, lag | durable consumers, pending, ack floor | — |
| Purge | queue contents | records below an offset | trim to empty | stream purge | clear retained |
| Delete | queue | — | stream key | — | — |
| Extra | exchanges and bindings | — | — | — | — |

Notes per kind:

- **RabbitMQ** is read and driven through its **management HTTP API**, not
  AMQP (ADR-027). *Peek* is the API's `get` with
  `ackmode = reject_requeue_true`: the message goes back on the queue. That is
  the same call the RabbitMQ web UI's *Get Messages* makes, and it has the same
  side effect, which the Messages pane states: a peeked message comes back with
  `redelivered = true`.
- **Kafka** lists topics, partitions and offsets and fetches and produces
  records through `rskafka`. Consumer groups and their committed offsets come
  from the group coordinator through `kafka-protocol` messages on a small client
  of our own, because `rskafka` has no group API (ADR-027). Lag is the latest
  offset minus the committed one, per partition. *Purge* deletes records below
  an offset, which Apache Kafka supports and Redpanda refuses; the error says
  so.
- **Redis Streams** uses the workspace's `redis` client directly — not
  `tool-redis`, which another tool may not depend on. Streams are found with
  `SCAN … TYPE stream`, bounded like the Redis key browser; groups and lag come
  from `XINFO GROUPS` (lag needs Redis 7; older servers show pending only).
- **NATS** lists **JetStream** streams and their consumers through
  `async-nats`. Core NATS keeps nothing, so a subject with no stream has
  nothing to list or peek; *Publish* still works to any subject.
- **MQTT** has no catalogue. Topics are what the broker has shown us:
  subscribing to `#` (or a filter the user sets) for a bounded window collects
  retained messages and whatever is published meanwhile. The table says it is
  an observation, not a list. *Purge* on a topic publishes an empty retained
  message, which is how MQTT clears one.

**Payloads** are shown by content type — JSON pretty-printed, text as text,
anything else as a hex dump with its length — with the headers or properties
above, through the same `hs_core::sniff` and `hs_core::fmt` helpers the
database cell viewer uses (the viewer itself is `tool-db`'s, which this tool
may not depend on).

## 5. Connections and credentials

Connections live in `hs_core::connections` with the new kinds `rabbitmq`,
`kafka`, `redis-streams`, `nats` and `mqtt`, so they travel with the workspace
and take `{{variables}}` from its environments. Fields per kind:

| Kind | Fields |
|---|---|
| rabbitmq | management URL (`http(s)://host:15672`), default vhost |
| kafka | bootstrap servers (comma-separated), TLS, SASL mechanism |
| redis-streams | host, port, database, TLS |
| nats | server URL(s), TLS |
| mqtt | host, port, client id, TLS, protocol version (3.1.1 / 5) |

**Credentials** follow the rule for every newer activity
([.claude/rules/security.md](../../.claude/rules/security.md)): a credential
is one of `password`, `token` or `api-key`, the form shows only that kind's
fields, and the secret is always a `secret:<ulid>` in the vault. How each kind
uses them:

| | password | token | api-key |
|---|---|---|---|
| RabbitMQ | HTTP basic | `Authorization: Bearer` (OAuth 2 plugin) | the named header |
| Kafka | SASL PLAIN or SCRAM-SHA-256/512 | SASL OAUTHBEARER | key and secret as SASL PLAIN (Confluent Cloud's form) |
| Redis Streams | `AUTH user password` | `AUTH` with the token as password | — |
| NATS | user and password | token | — |
| MQTT | username and password | the token as password (brokers that accept a JWT) | — |

A kind that has no use for a credential kind does not offer it, rather than
offering a field that would be ignored.

## 6. Guards

- **Publish**, **Purge** and **Delete** are writes. On a protected environment
  the tab is read-only: those controls are disabled with the reason as their
  tooltip, and the driver refuses them as well, so a palette command cannot get
  round the disabled button (docs/06-security.md §3).
- **Purge** and **Delete** ask first, naming the destination and how many
  messages go with it; **Delete** additionally asks for the destination's name
  to be typed, as the database guards do for `DROP`.
- **Peek** never consumes. Where the broker cannot read without a side effect
  (RabbitMQ's requeue), the pane says what the side effect is before the first
  peek.
- Nothing is published to a destination the user did not choose. The Publish
  form is filled for the destination selected in the table — RabbitMQ's
  default exchange with the queue's name as routing key, a NATS stream's first
  plain subject, otherwise the topic or stream itself — and shows it before
  anything is sent; with no destination selected there is no form at all.

## 7. Work and progress

All broker I/O runs on the tokio runtime and lands through the bounded
channels drained at frame start; nothing blocks the UI thread.

- Listing destinations and refreshing their figures is one call per poll,
  every 5 s while a tab is visible (Settings → Queue), never while it is not.
- **Peek** reads at most 50 messages (default 10) and at most 1 MB of payload
  in all; a larger request is refused with the bound named.
- **MQTT** observation runs for a bounded window (default 3 s) and a bounded
  number of topics (2 000); the table says when either bound was reached.
- Kafka group discovery asks every broker (`ListGroups`) and each group's
  coordinator (`FindCoordinator`, `OffsetFetch`), so it shows progress
  ("Group n of m") and can be stopped between groups, in the Consumers pane
  as the Redis big-key finder does, because a large cluster has many groups.
- A connection that has not answered in 10 s is reported, not waited on.

## 8. Implementation notes

- `crates/tools/tool-queue/`, depending on `hs-core` and `hs-ui` only.
  `src/driver.rs` holds the trait — `destinations`, `peek`, `publish`,
  `consumers`, `purge`, `delete`, `bindings` — and the shared types
  (`Destination`, `Message`, `ConsumerGroup`, `Lag`); one module per kind
  implements it (`rabbitmq.rs`, `kafka.rs`, `kafka_groups.rs`,
  `redis_streams.rs`, `nats.rs`, `mqtt.rs` on `mqtt_wire.rs`). Drivers are plain async and
  tested without the UI; the tab only sees the trait.
- Dependencies (ADR-027): `reqwest` and `redis` are already in the workspace;
  `rskafka` (with `transport-tls`, gzip and snappy only), `kafka-protocol`
  and `async-nats` (with `ring`) are new. MQTT is a small client of our own
  (`mqtt_wire.rs`): `rumqttc`'s current release pins a `rustls-webpki` with
  open advisories. No C-bound crate: not `rdkafka`, not `lapin`'s native-tls
  build, not the lz4 or zstd codecs.
- Registered in `crates/hyperstack/src/tools/mod.rs::factories()` after Logs,
  under **Infra**. The programme placed it after Automation, which does not
  exist yet; it moves there when phase K lands. The rail is user-ordered per
  workspace in any case (ADR-026).
- The cell viewer, `hs_ui::table::DataTable`, `hs_ui::dashboard`,
  `hs_ui::form`, `hs_ui::reorder` and `hs_ui::env_banner` are reused as they
  are.

## 9. Keyboard

| Command | Id | Default |
|---|---|---|
| Queue: Refresh | `queue.refresh` | F5 (tool-local) |
| Queue: Peek Messages | `queue.peek` | Ctrl+Alt+M |
| Queue: Publish Message | `queue.publish` | Ctrl+Alt+Shift+M |
| Queue: Purge… | `queue.purge` | — |
| Queue: Add Broker | `queue.add` | — |
| Toggle AI Assistant | `queue.ai` | — |

All rebindable in Settings → Shortcuts. Nothing on Ctrl+C/V/X, Ctrl+Shift+V or
Shift+Insert.

## 10. Open questions

1. **Live tail into the Logs activity.** The programme asks for a live tail of
   a queue or topic as a Logs source. It is a cross-tool feature (a Logs source
   fed through `Services`) and is left to its own ledger item rather than
   blocking the five drivers.
2. **AI Assistant "explain this backlog".** Planned as the shared
   review-only panel with the destination's figures as context; its own item,
   after the drivers.
3. **Amazon SQS and Azure Service Bus** are named by the programme as later
   kinds; not in this phase.
4. **Kafka Schema Registry** (Avro / Protobuf payloads) would make peeked
   records readable; out of scope until someone asks.

## What is implemented (2026-10-31)

All five kinds, as §3-§9 describe, checked on screen against local brokers
(RabbitMQ 4.4, Kafka 4.0, Redis 7.4, NATS 2.11, Mosquitto 2) and by
`tests/live.rs` against the same servers. Where the build differs from the
text above:

- **Kafka** shows no replication factor (`rskafka` lists partitions only) and
  hides topics whose names start with `__`. Its offsets are read for the first
  200 topics. Records compressed with lz4 or zstd cannot be peeked.
- **Settings → Queue** holds the brokers and three limits: messages per Peek,
  the refresh interval and the MQTT listening window.
- **Kafka group discovery** runs inside the tab rather than as a shell Job: the
  Consumers pane shows "Group n of m" and a Stop button that ends the look
  between groups.
- The **Explain This Backlog** quick action (open question 2) is in: the
  assistant is told the selected destination's figures. The live tail into
  Logs, SQS / Service Bus and the Schema Registry remain open.
- **Delete** is offered for RabbitMQ queues and Redis streams only, as §4's
  table says.
