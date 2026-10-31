//! One trait every broker kind implements (docs/tools/queue.md §8), and
//! the types the tab sees. The tab never asks which broker it is talking
//! to; what a kind cannot do it says through [`Caps`], and the tab leaves
//! that control out.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;

use crate::profile::ConnectConfig;

pub type Result<T> = std::result::Result<T, String>;

/// The most Peek reads at once, and the most payload it brings back in
/// all (docs/tools/queue.md §7).
pub const PEEK_MAX: usize = 50;
pub const PEEK_MAX_BYTES: usize = 1024 * 1024;

/// What the broker said it is, for the tab's header band.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {
    /// "RabbitMQ", "Apache Kafka", "Redis", "NATS", "MQTT".
    pub product: String,
    pub version: String,
}

/// What a kind can do, so the tab offers only that.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    pub consumers: bool,
    pub purge: bool,
    pub delete: bool,
    pub bindings: bool,
    /// Peek can start from a position the user gives (offset, id,
    /// sequence).
    pub peek_from: bool,
}

/// A queue, topic, stream or subject: whatever the kind calls the thing
/// messages go to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Destination {
    pub name: String,
    /// RabbitMQ's vhost; empty for the other kinds.
    pub scope: String,
    /// One value per entry of [`columns`] for this kind, already as text.
    pub figures: Vec<String>,
    /// Messages waiting, for the dashboard's total: ready messages, a
    /// stream's length. `None` where the kind has no such figure.
    pub depth: Option<u64>,
}

/// One message as Peek shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Message {
    /// Where it is: an offset, a stream id, a sequence, a position.
    pub position: String,
    pub partition: Option<i32>,
    pub key: Option<Vec<u8>>,
    /// Headers or properties, as text.
    pub headers: Vec<(String, String)>,
    pub payload: Vec<u8>,
    pub timestamp: String,
}

/// What Peek is asked for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PeekRequest {
    pub count: usize,
    /// Where to start, in the kind's own terms; empty means the oldest
    /// message the broker still holds.
    pub from: String,
    /// Kafka: one partition, or every partition when `None`.
    pub partition: Option<i32>,
}

impl PeekRequest {
    /// Refuse a request over the bounds, naming the bound
    /// (docs/tools/queue.md §7).
    pub fn check(&self) -> Result<()> {
        if self.count == 0 || self.count > PEEK_MAX {
            return Err(format!(
                "Peek reads 1 to {PEEK_MAX} messages; {} was asked for",
                self.count
            ));
        }
        Ok(())
    }
}

/// What Publish sends.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PublishRequest {
    /// RabbitMQ: the exchange (empty is the default exchange). Otherwise
    /// the topic, stream or subject.
    pub target: String,
    /// RabbitMQ: the vhost the exchange is in.
    pub scope: String,
    /// RabbitMQ: the routing key. Kafka: the record key. Unused elsewhere.
    pub key: String,
    pub headers: Vec<(String, String)>,
    pub payload: Vec<u8>,
    /// Kafka: a partition, or `None` to let the key choose.
    pub partition: Option<i32>,
    /// MQTT: QoS 0, 1 or 2, and whether the broker keeps it.
    pub qos: u8,
    pub retain: bool,
}

/// A consumer group, a durable consumer, or a queue's consumer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConsumerGroup {
    pub name: String,
    pub state: String,
    pub members: u64,
    /// Messages delivered and not yet acknowledged.
    pub pending: Option<u64>,
    /// Total lag, when the kind can tell.
    pub lag: Option<i64>,
    /// Per partition, for Kafka.
    pub detail: Vec<Lag>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lag {
    pub partition: i32,
    pub committed: Option<i64>,
    pub latest: Option<i64>,
    pub lag: Option<i64>,
}

/// A RabbitMQ binding of a queue to an exchange.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Binding {
    pub source: String,
    pub routing_key: String,
    pub arguments: String,
}

/// A destinations listing, and what it could not include.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listing {
    pub destinations: Vec<Destination>,
    /// Said above the table when set: MQTT's "observed for 3 s", or a
    /// bound that was reached.
    pub note: Option<String>,
}

/// How far a long look has got, and the user's wish to stop it: Kafka's
/// group discovery asks every broker and every group's coordinator
/// (docs/tools/queue.md §7).
#[derive(Debug, Default)]
pub struct Progress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
}

impl Progress {
    pub fn start(&self, total: usize) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
    }
    pub fn step(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

#[async_trait]
pub trait Driver: Send {
    fn info(&self) -> &ServerInfo;
    fn caps(&self) -> Caps;
    async fn destinations(&mut self) -> Result<Listing>;
    async fn peek(&mut self, dest: &Destination, req: &PeekRequest) -> Result<Vec<Message>>;
    /// Returns what the broker said about it, for the toast.
    async fn publish(&mut self, req: &PublishRequest) -> Result<String>;
    async fn consumers(
        &mut self,
        _dest: &Destination,
        _progress: &Progress,
    ) -> Result<Vec<ConsumerGroup>> {
        Ok(Vec::new())
    }
    /// Returns what was removed, for the toast.
    async fn purge(&mut self, dest: &Destination) -> Result<String>;
    async fn delete(&mut self, dest: &Destination) -> Result<()> {
        Err(format!("“{}” cannot be deleted from here", dest.name))
    }
    async fn bindings(&mut self, _dest: &Destination) -> Result<Vec<Binding>> {
        Ok(Vec::new())
    }
}

/// The Fluent keys of a kind's destination columns, after the name
/// (docs/tools/queue.md §4). [`Destination::figures`] holds one value per
/// key, in this order.
pub fn columns(kind: &str) -> &'static [&'static str] {
    match kind {
        "rabbitmq" => &[
            "queue-col-vhost",
            "queue-col-ready",
            "queue-col-unacked",
            "queue-col-consumers",
            "queue-col-publish-rate",
            "queue-col-deliver-rate",
            "queue-col-state",
        ],
        "kafka" => &[
            "queue-col-partitions",
            "queue-col-earliest",
            "queue-col-latest",
            "queue-col-messages",
        ],
        "redis-streams" => &[
            "queue-col-length",
            "queue-col-groups",
            "queue-col-first-id",
            "queue-col-last-id",
        ],
        "nats" => &[
            "queue-col-messages",
            "queue-col-bytes",
            "queue-col-consumers",
            "queue-col-subjects",
        ],
        "mqtt" => &[
            "queue-col-last-size",
            "queue-col-retained",
            "queue-col-last-seen",
        ],
        _ => &[],
    }
}

/// Open a session to the broker `cfg` describes, within its timeout.
pub async fn connect(cfg: &ConnectConfig) -> Result<Box<dyn Driver>> {
    let open = async {
        let d: Box<dyn Driver> = match cfg.kind.as_str() {
            "rabbitmq" => Box::new(crate::rabbitmq::Rabbit::connect(cfg).await?),
            "kafka" => Box::new(crate::kafka::Kafka::connect(cfg).await?),
            "redis-streams" => Box::new(crate::redis_streams::Streams::connect(cfg).await?),
            "nats" => Box::new(crate::nats::Nats::connect(cfg).await?),
            "mqtt" => Box::new(crate::mqtt::Mqtt::connect(cfg).await?),
            other => return Err(format!("“{other}” is not a broker kind")),
        };
        Ok(d)
    };
    let inner = tokio::time::timeout(cfg.timeout, open)
        .await
        .map_err(|_| {
            format!(
                "the broker did not answer within {} s",
                cfg.timeout.as_secs()
            )
        })??;
    Ok(Box::new(Guarded {
        inner,
        read_only: cfg.read_only,
    }))
}

/// Refuses every write on a read-only session, whatever asked for it: the
/// tab disables the controls, and this is what stops a palette command
/// getting round them (docs/tools/queue.md §6).
pub struct Guarded {
    inner: Box<dyn Driver>,
    read_only: bool,
}

impl Guarded {
    fn allow(&self, what: &str) -> Result<()> {
        if self.read_only {
            Err(format!(
                "{what} refused: this connection is read-only (a protected environment, or the profile says so)"
            ))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl Driver for Guarded {
    fn info(&self) -> &ServerInfo {
        self.inner.info()
    }
    fn caps(&self) -> Caps {
        self.inner.caps()
    }
    async fn destinations(&mut self) -> Result<Listing> {
        self.inner.destinations().await
    }
    async fn peek(&mut self, dest: &Destination, req: &PeekRequest) -> Result<Vec<Message>> {
        req.check()?;
        let mut out = self.inner.peek(dest, req).await?;
        // The byte bound applies to whatever a driver brought back.
        let mut total = 0usize;
        out.retain(|m| {
            total += m.payload.len();
            total <= PEEK_MAX_BYTES
        });
        Ok(out)
    }
    async fn publish(&mut self, req: &PublishRequest) -> Result<String> {
        self.allow("Publish")?;
        if req.target.is_empty() && req.key.is_empty() {
            // The form has no default target, and nor does this.
            return Err("Publish needs a destination".into());
        }
        self.inner.publish(req).await
    }
    async fn consumers(
        &mut self,
        dest: &Destination,
        progress: &Progress,
    ) -> Result<Vec<ConsumerGroup>> {
        self.inner.consumers(dest, progress).await
    }
    async fn purge(&mut self, dest: &Destination) -> Result<String> {
        self.allow("Purge")?;
        self.inner.purge(dest).await
    }
    async fn delete(&mut self, dest: &Destination) -> Result<()> {
        self.allow("Delete")?;
        self.inner.delete(dest).await
    }
    async fn bindings(&mut self, dest: &Destination) -> Result<Vec<Binding>> {
        self.inner.bindings(dest).await
    }
}

/// A Unix time in milliseconds as the Messages pane shows it.
pub fn time_ms(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A broker that records what reached it.
    struct Fake {
        info: ServerInfo,
        writes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait]
    impl Driver for Fake {
        fn info(&self) -> &ServerInfo {
            &self.info
        }
        fn caps(&self) -> Caps {
            Caps::default()
        }
        async fn destinations(&mut self) -> Result<Listing> {
            Ok(Listing::default())
        }
        async fn peek(&mut self, _: &Destination, req: &PeekRequest) -> Result<Vec<Message>> {
            Ok((0..req.count)
                .map(|i| Message {
                    position: i.to_string(),
                    payload: vec![b'x'; 300 * 1024],
                    ..Default::default()
                })
                .collect())
        }
        async fn publish(&mut self, _: &PublishRequest) -> Result<String> {
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(String::new())
        }
        async fn purge(&mut self, _: &Destination) -> Result<String> {
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(String::new())
        }
    }

    fn guarded(read_only: bool) -> (Guarded, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let writes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        (
            Guarded {
                inner: Box::new(Fake {
                    info: ServerInfo::default(),
                    writes: writes.clone(),
                }),
                read_only,
            },
            writes,
        )
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
    }

    #[test]
    fn a_read_only_session_refuses_every_write_before_the_broker_sees_it() {
        rt().block_on(async {
            let (mut g, writes) = guarded(true);
            let dest = Destination::default();
            let publish = PublishRequest {
                target: "orders".into(),
                ..Default::default()
            };
            assert!(g.publish(&publish).await.unwrap_err().contains("read-only"));
            assert!(g.purge(&dest).await.is_err());
            assert!(g.delete(&dest).await.is_err());
            assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 0);
            // Reading is still allowed.
            assert!(g.destinations().await.is_ok());
        });
    }

    #[test]
    fn publish_has_no_default_target() {
        rt().block_on(async {
            let (mut g, writes) = guarded(false);
            assert!(g.publish(&PublishRequest::default()).await.is_err());
            assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 0);
        });
    }

    #[test]
    fn peek_is_bounded_in_count_and_in_bytes() {
        rt().block_on(async {
            let (mut g, _) = guarded(true);
            let dest = Destination::default();
            let too_many = PeekRequest {
                count: PEEK_MAX + 1,
                ..Default::default()
            };
            assert!(g.peek(&dest, &too_many).await.unwrap_err().contains("50"));
            // Ten messages of 300 KiB: only what fits in 1 MiB comes back.
            let ten = PeekRequest {
                count: 10,
                ..Default::default()
            };
            assert_eq!(g.peek(&dest, &ten).await.unwrap().len(), 3);
        });
    }

    #[test]
    fn every_kind_has_columns() {
        for k in hs_core::connections::QUEUE_KINDS {
            assert!(!columns(k).is_empty(), "{k}");
        }
    }
}
