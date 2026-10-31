//! MQTT 3.1.1 or 5, through the small client in [`crate::mqtt_wire`]
//! (ADR-027).
//!
//! MQTT has no catalogue of topics. What the table shows is what the
//! broker showed us: one look subscribes to `#` for a bounded window and
//! keeps the retained messages it is sent and whatever is published
//! meanwhile (docs/tools/queue.md §4). Peek shows what that look kept for
//! the topic; Purge clears a retained message by publishing an empty
//! retained one, which is how MQTT does it.
//!
//! Each look and each publish is a session of its own, so nothing here
//! holds a subscription open between polls.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;

use crate::driver::{
    Caps, Destination, Driver, Listing, Message, PeekRequest, PublishRequest, Result, ServerInfo,
};
use crate::mqtt_wire;
use crate::profile::{Auth, ConnectConfig, host_port};

/// The most topics one look keeps, and the most messages per topic.
pub const MAX_TOPICS: usize = 2000;
const PER_TOPIC: usize = crate::driver::PEEK_MAX;

/// One message a look received: topic, payload, whether it was retained,
/// and its MQTT 5 user properties.
type Observed = (String, Vec<u8>, bool, Vec<(String, String)>);

/// What one look saw on one topic.
#[derive(Debug, Clone, Default)]
struct Seen {
    retained: bool,
    last_size: usize,
    last_seen: String,
    messages: Vec<Message>,
}

pub struct Mqtt {
    cfg: ConnectConfig,
    host: String,
    port: u16,
    v5: bool,
    info: ServerInfo,
    seen: BTreeMap<String, Seen>,
}

fn now() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string()
}

fn client_id(cfg: &ConnectConfig) -> String {
    if cfg.client_id.trim().is_empty() {
        let n: u32 = rand::random();
        format!("hyperstack-{n:08x}")
    } else {
        cfg.client_id.trim().to_string()
    }
}

/// The username and password MQTT sends: a token goes as the password,
/// which is how brokers that take a JWT read it.
fn credentials(auth: &Auth) -> Option<(String, String)> {
    match auth {
        Auth::Password { user, password } => Some((user.clone(), password.clone())),
        Auth::Token(t) => Some((String::new(), t.clone())),
        Auth::ApiKey { .. } | Auth::None => None,
    }
}

impl Mqtt {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Mqtt> {
        let (host, port) = host_port(&cfg.address, if cfg.tls { 8883 } else { 1883 })?;
        let v5 = cfg.protocol == "5";
        let mut m = Mqtt {
            cfg: cfg.clone(),
            host,
            port,
            v5,
            info: ServerInfo {
                product: "MQTT".into(),
                version: if v5 { "5".into() } else { "3.1.1".into() },
            },
            seen: BTreeMap::new(),
        };
        // Connecting is the test: a short look with nothing to keep.
        m.session(None, Duration::ZERO).await?;
        Ok(m)
    }

    /// One session: connect, optionally publish, listen on `#` for
    /// `window`, disconnect. Returns what was seen, and whether the look
    /// stopped at its bound.
    async fn session(
        &mut self,
        publish: Option<&PublishRequest>,
        window: Duration,
    ) -> Result<(Vec<Observed>, bool)> {
        let opts = mqtt_wire::Options {
            host: self.host.clone(),
            port: self.port,
            tls: if self.cfg.tls {
                Some(crate::tls::client_config()?)
            } else {
                None
            },
            v5: self.v5,
            client_id: client_id(&self.cfg),
            credentials: credentials(&self.cfg.auth),
            timeout: self.cfg.timeout,
        };
        let mut c = mqtt_wire::Client::connect(&opts).await?;
        if let Some(p) = publish {
            // MQTT 3.1.1 has no headers; MQTT 5 carries them as user
            // properties.
            let user: &[(String, String)] = if self.v5 { &p.headers } else { &[] };
            c.publish(&p.target, &p.payload, p.qos, p.retain, user)
                .await?;
        }
        let mut out = Vec::new();
        let mut cut = false;
        if !window.is_zero() {
            let (got, stopped) = c.observe("#", window, MAX_TOPICS * PER_TOPIC).await?;
            cut = stopped;
            out = got
                .into_iter()
                .map(|m| (m.topic, m.payload, m.retain, m.properties))
                .collect();
        }
        c.disconnect().await;
        Ok((out, cut))
    }
}

/// Fold one look's messages into the per-topic table, bounded.
fn fold(got: Vec<Observed>) -> (BTreeMap<String, Seen>, bool) {
    let mut seen: BTreeMap<String, Seen> = BTreeMap::new();
    let mut cut = false;
    let at = now();
    for (topic, payload, retained, headers) in got {
        if !seen.contains_key(&topic) && seen.len() >= MAX_TOPICS {
            cut = true;
            continue;
        }
        let s = seen.entry(topic).or_default();
        s.retained |= retained;
        s.last_size = payload.len();
        s.last_seen = at.clone();
        if s.messages.len() < PER_TOPIC {
            let mut headers = headers;
            if retained {
                headers.insert(0, ("retained".into(), "true".into()));
            }
            s.messages.push(Message {
                position: (s.messages.len() + 1).to_string(),
                headers,
                payload,
                timestamp: at.clone(),
                ..Default::default()
            });
        }
    }
    (seen, cut)
}

#[async_trait]
impl Driver for Mqtt {
    fn info(&self) -> &ServerInfo {
        &self.info
    }

    fn caps(&self) -> Caps {
        Caps {
            consumers: false,
            purge: true,
            delete: false,
            bindings: false,
            peek_from: false,
        }
    }

    async fn destinations(&mut self) -> Result<Listing> {
        let window = self.cfg.mqtt_window;
        let (got, cut_messages) = self.session(None, window).await?;
        let (seen, cut_topics) = fold(got);
        self.seen = seen;
        let destinations = self
            .seen
            .iter()
            .map(|(topic, s)| Destination {
                name: topic.clone(),
                scope: String::new(),
                figures: vec![
                    s.last_size.to_string(),
                    if s.retained {
                        "yes".into()
                    } else {
                        "no".into()
                    },
                    s.last_seen.clone(),
                ],
                depth: None,
            })
            .collect();
        let mut note = format!("topics seen in {} s on #", window.as_secs());
        if cut_topics || cut_messages {
            note.push_str(&format!("; stopped at {MAX_TOPICS} topics"));
        }
        Ok(Listing {
            destinations,
            note: Some(note),
        })
    }

    async fn peek(&mut self, dest: &Destination, req: &PeekRequest) -> Result<Vec<Message>> {
        Ok(self
            .seen
            .get(&dest.name)
            .map(|s| s.messages.iter().take(req.count).cloned().collect())
            .unwrap_or_default())
    }

    async fn publish(&mut self, req: &PublishRequest) -> Result<String> {
        if req.target.contains(['#', '+']) {
            return Err("a topic to publish to cannot hold the wildcards # or +".into());
        }
        self.session(Some(req), Duration::ZERO).await?;
        Ok(format!("published to {}", req.target))
    }

    async fn purge(&mut self, dest: &Destination) -> Result<String> {
        let clear = PublishRequest {
            target: dest.name.clone(),
            qos: 1,
            retain: true,
            ..Default::default()
        };
        self.session(Some(&clear), Duration::ZERO).await?;
        self.seen.remove(&dest.name);
        Ok(format!("cleared the retained message on {}", dest.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_is_folded_into_topics_with_their_last_size() {
        let (seen, cut) = fold(vec![
            ("a/b".into(), b"12345".to_vec(), true, Vec::new()),
            ("a/b".into(), b"1".to_vec(), false, Vec::new()),
            ("c".into(), Vec::new(), false, Vec::new()),
        ]);
        assert!(!cut);
        assert_eq!(seen.len(), 2);
        let ab = &seen["a/b"];
        assert!(ab.retained);
        assert_eq!(ab.last_size, 1);
        assert_eq!(ab.messages.len(), 2);
        assert_eq!(ab.messages[0].headers[0].0, "retained");
    }

    #[test]
    fn a_look_stops_keeping_topics_at_the_bound() {
        let got = (0..MAX_TOPICS + 5)
            .map(|i| (format!("t/{i}"), Vec::new(), false, Vec::new()))
            .collect();
        let (seen, cut) = fold(got);
        assert!(cut);
        assert_eq!(seen.len(), MAX_TOPICS);
    }
}
