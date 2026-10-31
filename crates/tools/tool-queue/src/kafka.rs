//! Kafka through `rskafka` for topics, offsets, records and produce, and
//! [`crate::kafka_groups`] for consumer groups (ADR-027).
//!
//! Only gzip and snappy record batches can be read: `lz4` and `zstd` are
//! C libraries, which this workspace keeps out (ADR-027), and a peek of a
//! topic compressed with one of them says so rather than failing
//! obscurely.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use rskafka::client::partition::{Compression, OffsetAt, PartitionClient, UnknownTopicHandling};
use rskafka::client::{Client, ClientBuilder, Credentials, OauthBearerCredentials, SaslConfig};
use rskafka::record::Record;

use crate::driver::{
    Caps, ConsumerGroup, Destination, Driver, Listing, Message, PeekRequest, Progress,
    PublishRequest, Result, ServerInfo,
};
use crate::kafka_groups::{self, Settings};
use crate::profile::{Auth, ConnectConfig};

/// The most topics one listing reads offsets for.
const MAX_TOPICS: usize = 200;

pub struct Kafka {
    client: Client,
    info: ServerInfo,
    groups: Settings,
    /// Partition clients are found through metadata; keep them rather
    /// than look each one up again every poll.
    partitions: HashMap<(String, i32), Arc<PartitionClient>>,
    /// Topic → its partitions, from the last listing.
    topics: BTreeMap<String, Vec<i32>>,
}

fn err(e: rskafka::client::error::Error) -> String {
    let text = e.to_string();
    if text.contains("ompression") {
        format!(
            "{text}. Records compressed with lz4 or zstd cannot be read by this build; gzip, snappy and uncompressed can"
        )
    } else {
        text
    }
}

/// Kafka's default partitioner: murmur2 of the key, made positive, modulo
/// the partition count, so a keyed record lands where a Java producer
/// would put it.
fn murmur2(data: &[u8]) -> i32 {
    const SEED: u32 = 0x9747_b28c;
    const M: u32 = 0x5bd1_e995;
    const R: u32 = 24;
    let len = data.len() as u32;
    let mut h: u32 = SEED ^ len;
    let (chunks, rest) = data.as_chunks::<4>();
    for c in chunks {
        let mut k = u32::from_le_bytes(*c);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    if rest.len() >= 3 {
        h ^= u32::from(rest[2]) << 16;
    }
    if rest.len() >= 2 {
        h ^= u32::from(rest[1]) << 8;
    }
    if !rest.is_empty() {
        h ^= u32::from(rest[0]);
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h as i32
}

pub fn partition_for_key(key: &[u8], partitions: usize) -> i32 {
    ((murmur2(key) & 0x7fff_ffff) as usize % partitions.max(1)) as i32
}

fn sasl(cfg: &ConnectConfig) -> Option<SaslConfig> {
    match &cfg.auth {
        Auth::None => None,
        Auth::Password { user, password } => {
            let c = Credentials::new(user.clone(), password.clone());
            Some(match cfg.sasl.as_str() {
                "scram-sha-256" => SaslConfig::ScramSha256(c),
                "scram-sha-512" => SaslConfig::ScramSha512(c),
                _ => SaslConfig::Plain(c),
            })
        }
        Auth::ApiKey { name, key } => Some(SaslConfig::Plain(Credentials::new(
            name.clone(),
            key.clone(),
        ))),
        Auth::Token(t) => {
            let token = t.clone();
            Some(SaslConfig::Oauthbearer(OauthBearerCredentials {
                callback: Arc::new(move || {
                    let token = token.clone();
                    Box::pin(async move { Ok(token) })
                }),
                authz_id: None,
                bearer_kvs: Vec::new(),
            }))
        }
    }
}

impl Kafka {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Kafka> {
        let bootstrap: Vec<String> = cfg
            .address
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let tls = if cfg.tls {
            Some(crate::tls::client_config()?)
        } else {
            None
        };
        let mut builder = ClientBuilder::new(bootstrap.clone())
            .client_id("hyperstack")
            // rskafka retries for ever by default; a tab must hear back.
            .backoff_config(rskafka::BackoffConfig {
                deadline: Some(cfg.timeout),
                ..Default::default()
            });
        if let Some(t) = &tls {
            builder = builder.tls_config(t.clone());
        }
        if let Some(s) = sasl(cfg) {
            builder = builder.sasl_config(s);
        }
        let client = builder.build().await.map_err(err)?;
        Ok(Kafka {
            client,
            info: ServerInfo {
                product: "Kafka".into(),
                version: String::new(),
            },
            groups: Settings {
                bootstrap,
                tls,
                auth: cfg.auth.clone(),
                sasl: cfg.sasl.clone(),
                timeout: cfg.timeout,
            },
            partitions: HashMap::new(),
            topics: BTreeMap::new(),
        })
    }

    async fn partition(&mut self, topic: &str, p: i32) -> Result<Arc<PartitionClient>> {
        let key = (topic.to_string(), p);
        if let Some(c) = self.partitions.get(&key) {
            return Ok(c.clone());
        }
        let c = Arc::new(
            self.client
                .partition_client(topic, p, UnknownTopicHandling::Error)
                .await
                .map_err(err)?,
        );
        self.partitions.insert(key, c.clone());
        Ok(c)
    }

    async fn partitions_of(&mut self, topic: &str) -> Result<Vec<i32>> {
        if let Some(p) = self.topics.get(topic) {
            return Ok(p.clone());
        }
        let t = self
            .client
            .list_topics()
            .await
            .map_err(err)?
            .into_iter()
            .find(|t| t.name == topic)
            .ok_or_else(|| format!("no topic “{topic}”"))?;
        let p: Vec<i32> = t.partitions.into_iter().collect();
        self.topics.insert(topic.to_string(), p.clone());
        Ok(p)
    }

    /// (earliest, latest) of every partition of a topic.
    async fn offsets(&mut self, topic: &str, parts: &[i32]) -> Result<Vec<(i32, i64, i64)>> {
        let mut clients = Vec::with_capacity(parts.len());
        for p in parts {
            clients.push((*p, self.partition(topic, *p).await?));
        }
        let reads = clients.iter().map(|(p, c)| async move {
            let earliest = c.get_offset(OffsetAt::Earliest).await.map_err(err)?;
            let latest = c.get_offset(OffsetAt::Latest).await.map_err(err)?;
            Ok::<_, String>((*p, earliest, latest))
        });
        futures_util::future::join_all(reads)
            .await
            .into_iter()
            .collect()
    }
}

#[async_trait]
impl Driver for Kafka {
    fn info(&self) -> &ServerInfo {
        &self.info
    }

    fn caps(&self) -> Caps {
        Caps {
            consumers: true,
            purge: true,
            delete: false,
            bindings: false,
            peek_from: true,
        }
    }

    async fn destinations(&mut self) -> Result<Listing> {
        let topics = self.client.list_topics().await.map_err(err)?;
        // `__consumer_offsets` and the like are Kafka's own bookkeeping.
        let mut topics: Vec<_> = topics
            .into_iter()
            .filter(|t| !t.name.starts_with("__"))
            .collect();
        topics.sort_by(|a, b| a.name.cmp(&b.name));
        let cut = topics.len() > MAX_TOPICS;
        self.topics = topics
            .iter()
            .map(|t| (t.name.clone(), t.partitions.iter().copied().collect()))
            .collect();
        let mut destinations = Vec::with_capacity(topics.len());
        for (i, t) in topics.iter().enumerate() {
            let parts: Vec<i32> = t.partitions.iter().copied().collect();
            let (earliest, latest, messages) = if i < MAX_TOPICS {
                match self.offsets(&t.name, &parts).await {
                    Ok(o) => {
                        let e: i64 = o.iter().map(|(_, e, _)| e).sum();
                        let l: i64 = o.iter().map(|(_, _, l)| l).sum();
                        (e.to_string(), l.to_string(), Some((l - e).max(0) as u64))
                    }
                    Err(_) => ("—".into(), "—".into(), None),
                }
            } else {
                ("—".into(), "—".into(), None)
            };
            destinations.push(Destination {
                name: t.name.clone(),
                scope: String::new(),
                figures: vec![
                    parts.len().to_string(),
                    earliest,
                    latest,
                    messages
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| "—".into()),
                ],
                depth: messages,
            });
        }
        Ok(Listing {
            destinations,
            note: cut.then(|| format!("offsets read for the first {MAX_TOPICS} topics")),
        })
    }

    async fn peek(&mut self, dest: &Destination, req: &PeekRequest) -> Result<Vec<Message>> {
        let all = self.partitions_of(&dest.name).await?;
        let parts: Vec<i32> = match req.partition {
            Some(p) if all.contains(&p) => vec![p],
            Some(p) => return Err(format!("“{}” has no partition {p}", dest.name)),
            None => all,
        };
        let from: Option<i64> = if req.from.trim().is_empty() {
            None
        } else {
            Some(
                req.from
                    .trim()
                    .parse()
                    .map_err(|_| format!("“{}” is not an offset", req.from.trim()))?,
            )
        };
        let mut out = Vec::new();
        for p in parts {
            let c = self.partition(&dest.name, p).await?;
            let start = match from {
                Some(o) => o,
                None => c.get_offset(OffsetAt::Earliest).await.map_err(err)?,
            };
            let (records, _high) = c
                .fetch_records(start, 1..crate::driver::PEEK_MAX_BYTES as i32, 500)
                .await
                .map_err(err)?;
            for r in records.into_iter().take(req.count) {
                out.push(Message {
                    position: r.offset.to_string(),
                    partition: Some(p),
                    key: r.record.key,
                    headers: r
                        .record
                        .headers
                        .into_iter()
                        .map(|(k, v)| (k, String::from_utf8_lossy(&v).into_owned()))
                        .collect(),
                    payload: r.record.value.unwrap_or_default(),
                    timestamp: crate::driver::time_ms(r.record.timestamp.timestamp_millis()),
                });
            }
        }
        // Across partitions, oldest first, then the bound.
        out.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        out.truncate(req.count);
        Ok(out)
    }

    async fn publish(&mut self, req: &PublishRequest) -> Result<String> {
        let parts = self.partitions_of(&req.target).await?;
        let partition = match req.partition {
            Some(p) if parts.contains(&p) => p,
            Some(p) => return Err(format!("“{}” has no partition {p}", req.target)),
            None if !req.key.is_empty() => {
                parts[partition_for_key(req.key.as_bytes(), parts.len()) as usize]
            }
            None => *parts.first().ok_or("the topic has no partitions")?,
        };
        let record = Record {
            key: (!req.key.is_empty()).then(|| req.key.as_bytes().to_vec()),
            value: Some(req.payload.clone()),
            headers: req
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.as_bytes().to_vec()))
                .collect(),
            timestamp: chrono::Utc::now(),
        };
        let c = self.partition(&req.target, partition).await?;
        let offsets = c
            .produce(vec![record], Compression::NoCompression)
            .await
            .map_err(err)?;
        Ok(format!(
            "partition {partition}, offset {}",
            offsets.first().copied().unwrap_or_default()
        ))
    }

    async fn consumers(
        &mut self,
        dest: &Destination,
        progress: &Progress,
    ) -> Result<Vec<ConsumerGroup>> {
        let parts = self.partitions_of(&dest.name).await?;
        let latest: HashMap<i32, i64> = self
            .offsets(&dest.name, &parts)
            .await?
            .into_iter()
            .map(|(p, _, l)| (p, l))
            .collect();
        let (groups, _cut) =
            kafka_groups::topic_groups(&self.groups, &dest.name, &latest, progress).await?;
        Ok(groups)
    }

    async fn purge(&mut self, dest: &Destination) -> Result<String> {
        let parts = self.partitions_of(&dest.name).await?;
        let mut removed = 0i64;
        for p in parts {
            let c = self.partition(&dest.name, p).await?;
            let earliest = c.get_offset(OffsetAt::Earliest).await.map_err(err)?;
            let latest = c.get_offset(OffsetAt::Latest).await.map_err(err)?;
            if latest > earliest {
                c.delete_records(latest, 10_000).await.map_err(|e| {
                    let text = err(e);
                    format!("{text} (Redpanda and some managed services refuse record deletion)")
                })?;
                removed += latest - earliest;
            }
        }
        Ok(format!("deleted {removed} records from “{}”", dest.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values the Java client's `Utils.murmur2` produces.
    #[test]
    fn murmur2_matches_the_java_partitioner() {
        assert_eq!(murmur2(b"21"), -973932308);
        assert_eq!(murmur2(b"foobar"), -790332482);
        assert_eq!(murmur2(b"a-little-bit-long-string"), -985981536);
        assert_eq!(murmur2(b"a-little-bit-longer-string"), -1486304829);
        assert_eq!(
            murmur2(b"lkjh234lh9fiuh90y23oiuhsafujhadof229phr9h19h89h8"),
            -58897971
        );
        assert_eq!(murmur2(b"abc"), 479470107);
    }

    #[test]
    fn a_key_always_lands_on_the_same_partition() {
        let p = partition_for_key(b"order-42", 6);
        assert!((0..6).contains(&p));
        assert_eq!(p, partition_for_key(b"order-42", 6));
    }
}
