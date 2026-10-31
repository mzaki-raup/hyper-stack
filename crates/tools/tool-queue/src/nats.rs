//! NATS through `async-nats` (ADR-027): the JetStream streams and their
//! durable consumers. Core NATS keeps nothing, so a subject no stream
//! captures has nothing to list or peek; Publish still reaches any
//! subject (docs/tools/queue.md §4).
//!
//! Peek reads a stream's messages by sequence (`STREAM.MSG.GET`), which
//! consumes nothing and moves no consumer.

use async_trait::async_trait;
use futures_util::StreamExt;

use crate::driver::{
    Caps, ConsumerGroup, Destination, Driver, Listing, Message, PeekRequest, Progress,
    PublishRequest, Result, ServerInfo,
};
use crate::profile::{Auth, ConnectConfig};

/// The most streams one listing reads.
const MAX_STREAMS: usize = 500;
/// How many sequence numbers a peek walks past deleted messages before
/// it gives up looking for more.
const MAX_GAPS: u64 = 1000;

pub struct Nats {
    client: async_nats::Client,
    js: async_nats::jetstream::Context,
    info: ServerInfo,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// JetStream's answer when no stream captures a subject; the publish then
/// goes out as plain NATS.
fn no_stream(e: &str) -> bool {
    e.contains("no responders") || e.contains("no stream")
}

impl Nats {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Nats> {
        crate::tls::install_crypto_provider();
        let mut opts = async_nats::ConnectOptions::new()
            .name("HyperStack")
            .connection_timeout(cfg.timeout)
            // A tab reconnects by asking again; it must not sit retrying
            // a server that went away.
            .max_reconnects(Some(1));
        if cfg.tls {
            opts = opts.require_tls(true);
        }
        opts = match &cfg.auth {
            Auth::Password { user, password } => {
                opts.user_and_password(user.clone(), password.clone())
            }
            Auth::Token(t) => opts.token(t.clone()),
            Auth::ApiKey { .. } | Auth::None => opts,
        };
        let servers: Vec<async_nats::ServerAddr> = cfg
            .address
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<async_nats::ServerAddr>().map_err(err))
            .collect::<Result<_>>()?;
        let client = opts.connect(servers).await.map_err(err)?;
        let si = client.server_info();
        let js = async_nats::jetstream::new(client.clone());
        Ok(Nats {
            client,
            js,
            info: ServerInfo {
                product: "NATS".into(),
                version: si.version,
            },
        })
    }
}

#[async_trait]
impl Driver for Nats {
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
        let mut streams = self.js.streams();
        let mut destinations = Vec::new();
        let mut cut = false;
        while let Some(info) = streams.next().await {
            let info = info.map_err(|e| {
                let text = err(e);
                if text.contains("no responders") || text.contains("timed out") {
                    "JetStream is not enabled on this server, so there are no streams to list"
                        .to_string()
                } else {
                    text
                }
            })?;
            if destinations.len() >= MAX_STREAMS {
                cut = true;
                break;
            }
            let s = &info.state;
            destinations.push(Destination {
                name: info.config.name.clone(),
                scope: String::new(),
                figures: vec![
                    s.messages.to_string(),
                    s.bytes.to_string(),
                    s.consumer_count.to_string(),
                    info.config.subjects.join(", "),
                ],
                depth: Some(s.messages),
            });
        }
        destinations.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Listing {
            destinations,
            note: cut.then(|| format!("the first {MAX_STREAMS} streams")),
        })
    }

    async fn peek(&mut self, dest: &Destination, req: &PeekRequest) -> Result<Vec<Message>> {
        let mut stream = self.js.get_stream(&dest.name).await.map_err(err)?;
        let state = stream.info().await.map_err(err)?.state.clone();
        if state.messages == 0 {
            return Ok(Vec::new());
        }
        let mut seq = if req.from.trim().is_empty() {
            state.first_sequence
        } else {
            req.from
                .trim()
                .parse()
                .map_err(|_| format!("“{}” is not a sequence number", req.from.trim()))?
        };
        let mut out = Vec::new();
        let mut gaps = 0;
        while out.len() < req.count && seq <= state.last_sequence && gaps < MAX_GAPS {
            match stream.get_raw_message(seq).await {
                Ok(m) => {
                    let mut headers = vec![("subject".to_string(), m.subject.to_string())];
                    for (k, vs) in m.headers.iter() {
                        for v in vs {
                            headers.push((k.to_string(), v.to_string()));
                        }
                    }
                    out.push(Message {
                        position: m.sequence.to_string(),
                        headers,
                        payload: m.payload.to_vec(),
                        timestamp: crate::driver::time_ms(
                            (m.time.unix_timestamp_nanos() / 1_000_000) as i64,
                        ),
                        ..Default::default()
                    });
                }
                // A deleted message leaves a gap in the sequence.
                Err(_) => gaps += 1,
            }
            seq += 1;
        }
        Ok(out)
    }

    async fn publish(&mut self, req: &PublishRequest) -> Result<String> {
        let mut headers = async_nats::HeaderMap::new();
        for (k, v) in &req.headers {
            headers.insert(k.as_str(), v.as_str());
        }
        let subject = req.target.clone();
        // Through JetStream first, so a stream's acknowledgement says
        // where it landed; a subject no stream captures is plain NATS.
        match self
            .js
            .publish_with_headers(subject.clone(), headers.clone(), req.payload.clone().into())
            .await
        {
            Ok(ack) => match ack.await {
                Ok(a) => return Ok(format!("stored in {} at sequence {}", a.stream, a.sequence)),
                Err(e) if !no_stream(&err(&e)) => return Err(err(e)),
                Err(_) => {}
            },
            Err(e) if !no_stream(&err(&e)) => return Err(err(e)),
            Err(_) => {}
        }
        self.client
            .publish_with_headers(subject.clone(), headers, req.payload.clone().into())
            .await
            .map_err(err)?;
        self.client.flush().await.map_err(err)?;
        Ok(format!("published to {subject}; no stream stores it"))
    }

    async fn consumers(
        &mut self,
        dest: &Destination,
        _progress: &Progress,
    ) -> Result<Vec<ConsumerGroup>> {
        let stream = self.js.get_stream(&dest.name).await.map_err(err)?;
        let mut list = stream.consumers();
        let mut out = Vec::new();
        while let Some(c) = list.next().await {
            let c = c.map_err(err)?;
            out.push(ConsumerGroup {
                name: c.name.clone(),
                state: format!("ack floor {}", c.ack_floor.stream_sequence),
                members: c.num_waiting as u64,
                pending: Some(c.num_ack_pending as u64),
                lag: Some(c.num_pending as i64),
                detail: Vec::new(),
            });
        }
        Ok(out)
    }

    async fn purge(&mut self, dest: &Destination) -> Result<String> {
        let stream = self.js.get_stream(&dest.name).await.map_err(err)?;
        let r = stream.purge().await.map_err(err)?;
        Ok(format!("purged {} messages from {}", r.purged, dest.name))
    }
}
