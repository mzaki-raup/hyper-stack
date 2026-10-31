//! RabbitMQ through its management HTTP API (ADR-027): the same calls the
//! broker's own web UI makes, so what this shows is what that shows.
//!
//! Peek is the API's `get` with `ackmode = reject_requeue_true`: the
//! message goes back on the queue, marked redelivered. There is no way to
//! read a RabbitMQ queue without that side effect, which is why the
//! Messages pane says so before the first peek (docs/tools/queue.md §6).

use async_trait::async_trait;
use base64::Engine;
use serde_json::{Value, json};

use crate::driver::{
    Binding, Caps, ConsumerGroup, Destination, Driver, Listing, Message, PeekRequest, Progress,
    PublishRequest, Result, ServerInfo,
};
use crate::profile::{Auth, ConnectConfig};

pub struct Rabbit {
    http: reqwest::Client,
    base: String,
    auth: Auth,
    vhost: String,
    info: ServerInfo,
}

/// A path segment, percent-encoded: vhost `/` is `%2F`, and a queue name
/// may hold anything.
fn seg(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The vhost a destination belongs to; `/` when the listing had none.
fn vhost_of(d: &Destination) -> &str {
    if d.scope.is_empty() { "/" } else { &d.scope }
}

/// A broker with no management plugin answers on 5672 and nothing on
/// 15672, or 404 behind a proxy: say what to enable rather than only
/// what failed (ADR-027).
fn no_management(base: &str, e: String) -> String {
    // reqwest says "error sending request" when nothing listens.
    if e.starts_with("404") || e.contains("error sending request") {
        format!(
            "{e}. No RabbitMQ management API answered at {base}; this activity reads RabbitMQ through it (rabbitmq-plugins enable rabbitmq_management)"
        )
    } else {
        e
    }
}

fn num(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

fn rate(v: &Value, k: &str) -> String {
    v.get("message_stats")
        .and_then(|s| s.get(k))
        .and_then(|d| d.get("rate"))
        .and_then(Value::as_f64)
        .map(|r| format!("{r:.1}/s"))
        .unwrap_or_else(|| "0.0/s".into())
}

/// A header value as text: strings as they are, anything else as JSON.
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl Rabbit {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Rabbit> {
        let base = cfg.address.trim_end_matches('/').to_string();
        if !base.starts_with("http://") && !base.starts_with("https://") {
            return Err(format!(
                "“{base}” is not a management URL; it looks like http://host:15672"
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .build()
            .map_err(|e| e.to_string())?;
        let mut r = Rabbit {
            http,
            base,
            auth: cfg.auth.clone(),
            vhost: cfg.vhost.clone(),
            info: ServerInfo::default(),
        };
        let overview = r
            .get("/api/overview")
            .await
            .map_err(|e| no_management(&r.base, e))?;
        r.info = ServerInfo {
            product: overview
                .get("product_name")
                .and_then(Value::as_str)
                .unwrap_or("RabbitMQ")
                .to_string(),
            version: overview
                .get("product_version")
                .or_else(|| overview.get("rabbitmq_version"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };
        Ok(r)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let rb = self.http.request(method, format!("{}{path}", self.base));
        match &self.auth {
            Auth::None => rb,
            Auth::Password { user, password } => rb.basic_auth(user, Some(password)),
            Auth::Token(t) => rb.bearer_auth(t),
            Auth::ApiKey { name, key } => rb.header(name.as_str(), key.as_str()),
        }
    }

    async fn send(&self, rb: reqwest::RequestBuilder) -> Result<Value> {
        let resp = rb.send().await.map_err(|e| e.to_string())?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            // The API explains itself in `reason`; say that, not the body.
            let reason = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("reason").and_then(Value::as_str).map(String::from))
                .unwrap_or(body);
            return Err(match status.as_u16() {
                401 => "the broker refused the credentials (401)".into(),
                403 => format!("not allowed (403): {reason}"),
                code => format!("{code}: {reason}"),
            });
        }
        if body.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body).map_err(|e| e.to_string())
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.send(self.request(reqwest::Method::GET, path)).await
    }

    fn queue_path(d: &Destination) -> String {
        format!("/api/queues/{}/{}", seg(vhost_of(d)), seg(&d.name))
    }
}

#[async_trait]
impl Driver for Rabbit {
    fn info(&self) -> &ServerInfo {
        &self.info
    }

    fn caps(&self) -> Caps {
        Caps {
            consumers: true,
            purge: true,
            delete: true,
            bindings: true,
            peek_from: false,
        }
    }

    async fn destinations(&mut self) -> Result<Listing> {
        let path = if self.vhost.is_empty() {
            "/api/queues".to_string()
        } else {
            format!("/api/queues/{}", seg(&self.vhost))
        };
        let list = self.get(&path).await?;
        let destinations = list
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|q| {
                        let ready = num(q, "messages_ready");
                        Destination {
                            name: q
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            scope: q
                                .get("vhost")
                                .and_then(Value::as_str)
                                .unwrap_or("/")
                                .to_string(),
                            figures: vec![
                                q.get("vhost")
                                    .and_then(Value::as_str)
                                    .unwrap_or("/")
                                    .to_string(),
                                ready.to_string(),
                                num(q, "messages_unacknowledged").to_string(),
                                num(q, "consumers").to_string(),
                                rate(q, "publish_details"),
                                rate(q, "deliver_get_details"),
                                q.get("state")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                            ],
                            depth: Some(ready),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Listing {
            destinations,
            note: None,
        })
    }

    async fn peek(&mut self, dest: &Destination, req: &PeekRequest) -> Result<Vec<Message>> {
        let body = json!({
            "count": req.count,
            "ackmode": "reject_requeue_true",
            "encoding": "auto",
            "truncate": crate::driver::PEEK_MAX_BYTES,
        });
        let path = format!("{}/get", Self::queue_path(dest));
        let got = self
            .send(self.request(reqwest::Method::POST, &path).json(&body))
            .await?;
        let mut out = Vec::new();
        for (i, m) in got.as_array().into_iter().flatten().enumerate() {
            let raw = m.get("payload").and_then(Value::as_str).unwrap_or_default();
            let payload = if m.get("payload_encoding").and_then(Value::as_str) == Some("base64") {
                base64::engine::general_purpose::STANDARD
                    .decode(raw)
                    .map_err(|e| e.to_string())?
            } else {
                raw.as_bytes().to_vec()
            };
            let mut headers = Vec::new();
            for k in ["exchange", "routing_key", "redelivered"] {
                if let Some(v) = m.get(k) {
                    headers.push((k.to_string(), text_of(v)));
                }
            }
            if let Some(props) = m.get("properties").and_then(Value::as_object) {
                for (k, v) in props {
                    if k == "headers" {
                        for (hk, hv) in v.as_object().into_iter().flatten() {
                            headers.push((hk.clone(), text_of(hv)));
                        }
                    } else {
                        headers.push((k.clone(), text_of(v)));
                    }
                }
            }
            let timestamp = m
                .get("properties")
                .and_then(|p| p.get("timestamp"))
                .and_then(Value::as_i64)
                .map(|s| crate::driver::time_ms(s * 1000))
                .unwrap_or_default();
            out.push(Message {
                position: (i + 1).to_string(),
                payload,
                headers,
                timestamp,
                ..Default::default()
            });
        }
        Ok(out)
    }

    async fn publish(&mut self, req: &PublishRequest) -> Result<String> {
        let vhost = if req.scope.is_empty() {
            "/"
        } else {
            &req.scope
        };
        // The default exchange is called "amq.default" in a URL.
        let exchange = if req.target.is_empty() {
            "amq.default"
        } else {
            &req.target
        };
        let headers: serde_json::Map<String, Value> = req
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        let (payload, encoding) = match std::str::from_utf8(&req.payload) {
            Ok(s) => (s.to_string(), "string"),
            Err(_) => (
                base64::engine::general_purpose::STANDARD.encode(&req.payload),
                "base64",
            ),
        };
        let body = json!({
            "properties": { "headers": headers },
            "routing_key": req.key,
            "payload": payload,
            "payload_encoding": encoding,
        });
        let path = format!("/api/exchanges/{}/{}/publish", seg(vhost), seg(exchange));
        let r = self
            .send(self.request(reqwest::Method::POST, &path).json(&body))
            .await?;
        if r.get("routed").and_then(Value::as_bool) == Some(true) {
            Ok("routed".into())
        } else {
            // Accepted, but no queue was bound to take it.
            Ok("not routed: no queue is bound for that routing key".into())
        }
    }

    async fn consumers(
        &mut self,
        dest: &Destination,
        _progress: &Progress,
    ) -> Result<Vec<ConsumerGroup>> {
        let q = self.get(&Self::queue_path(dest)).await?;
        Ok(q.get("consumer_details")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|c| {
                let peer = c
                    .get("channel_details")
                    .and_then(|ch| ch.get("peer_host"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                ConsumerGroup {
                    name: c
                        .get("consumer_tag")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    state: if c.get("active").and_then(Value::as_bool) == Some(false) {
                        "waiting".into()
                    } else {
                        format!("active {peer}").trim().to_string()
                    },
                    members: 1,
                    pending: c.get("prefetch_count").and_then(Value::as_u64),
                    lag: None,
                    detail: Vec::new(),
                }
            })
            .collect())
    }

    async fn purge(&mut self, dest: &Destination) -> Result<String> {
        let path = format!("{}/contents", Self::queue_path(dest));
        self.send(self.request(reqwest::Method::DELETE, &path))
            .await?;
        Ok(format!("purged “{}”", dest.name))
    }

    async fn delete(&mut self, dest: &Destination) -> Result<()> {
        self.send(self.request(reqwest::Method::DELETE, &Self::queue_path(dest)))
            .await?;
        Ok(())
    }

    async fn bindings(&mut self, dest: &Destination) -> Result<Vec<Binding>> {
        let path = format!("{}/bindings", Self::queue_path(dest));
        let list = self.get(&path).await?;
        Ok(list
            .as_array()
            .into_iter()
            .flatten()
            .map(|b| Binding {
                // Empty is the default exchange; the tab names it.
                source: b
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                routing_key: b
                    .get("routing_key")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                arguments: b
                    .get("arguments")
                    .filter(|a| a.as_object().is_some_and(|o| !o.is_empty()))
                    .map(Value::to_string)
                    .unwrap_or_default(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vhost_and_a_queue_name_are_encoded_into_the_path() {
        let d = Destination {
            name: "orders created".into(),
            scope: "/".into(),
            ..Default::default()
        };
        assert_eq!(Rabbit::queue_path(&d), "/api/queues/%2F/orders%20created");
    }

    #[test]
    fn a_missing_management_plugin_is_named() {
        let e = no_management("http://h:15672", "404: Not Found".into());
        assert!(e.contains("rabbitmq_management"));
        // Other failures are left as they are.
        assert_eq!(
            no_management(
                "http://h:15672",
                "the broker refused the credentials (401)".into()
            ),
            "the broker refused the credentials (401)"
        );
    }

    #[test]
    fn a_queue_without_rates_reads_as_zero_not_blank() {
        assert_eq!(rate(&json!({}), "publish_details"), "0.0/s");
        assert_eq!(
            rate(
                &json!({"message_stats": {"publish_details": {"rate": 2.26}}}),
                "publish_details"
            ),
            "2.3/s"
        );
    }
}
