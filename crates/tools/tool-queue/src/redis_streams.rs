//! Redis Streams through the workspace's `redis` client, directly: the
//! Redis activity's crate is another tool's, which this one may not
//! depend on (docs/tools/queue.md §4).
//!
//! Streams are found with `SCAN … TYPE stream`, bounded like the Redis key
//! browser; groups and lag come from `XINFO GROUPS`, where lag needs
//! Redis 7 and an older server shows pending only.

use async_trait::async_trait;
use redis::Value;
use redis::aio::MultiplexedConnection;

use crate::driver::{
    Caps, ConsumerGroup, Destination, Driver, Listing, Message, PeekRequest, Progress,
    PublishRequest, Result, ServerInfo,
};
use crate::profile::{Auth, ConnectConfig, host_port};

/// The most streams one listing reads, and the most keys its scan walks.
const MAX_STREAMS: usize = 500;
const MAX_SCANNED: usize = 100_000;

pub struct Streams {
    conn: MultiplexedConnection,
    info: ServerInfo,
}

fn err(e: redis::RedisError) -> String {
    e.to_string()
}

fn bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::BulkString(b) => b.clone(),
        Value::SimpleString(s) => s.as_bytes().to_vec(),
        Value::Int(i) => i.to_string().into_bytes(),
        Value::Double(d) => d.to_string().into_bytes(),
        Value::Nil => Vec::new(),
        other => format!("{other:?}").into_bytes(),
    }
}

fn text(v: &Value) -> String {
    String::from_utf8_lossy(&bytes(v)).into_owned()
}

/// A RESP2 flat `[k, v, k, v]` reply, or a RESP3 map, as pairs.
fn pairs(v: &Value) -> Vec<(String, &Value)> {
    match v {
        Value::Array(a) => a
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| (text(&c[0]), &c[1]))
            .collect(),
        Value::Map(m) => m.iter().map(|(k, v)| (text(k), v)).collect(),
        _ => Vec::new(),
    }
}

fn field<'a>(p: &'a [(String, &'a Value)], name: &str) -> Option<&'a Value> {
    p.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
}

fn int(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Int(i) => Some(*i),
        Value::Nil => None,
        other => text(other).parse().ok(),
    }
}

/// A stream entry: its id and its fields.
type Entry = (String, Vec<(String, Vec<u8>)>);

/// `[id, [field, value, …]]`, an entry as XRANGE and XINFO return it.
fn entry(v: &Value) -> Option<Entry> {
    let Value::Array(pair) = v else {
        return None;
    };
    let id = text(pair.first()?);
    let fields = match pair.get(1) {
        Some(Value::Array(flat)) => flat
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| (text(&c[0]), bytes(&c[1])))
            .collect(),
        _ => Vec::new(),
    };
    Some((id, fields))
}

/// The ms part of a stream id is when it was added.
fn id_time(id: &str) -> String {
    id.split('-')
        .next()
        .and_then(|ms| ms.parse::<i64>().ok())
        .map(crate::driver::time_ms)
        .unwrap_or_default()
}

/// An entry as a message: a `data` field - what Publish writes the body
/// as - is the payload and the other fields are its headers; without one,
/// the fields together are the payload.
fn split_entry(fields: Vec<(String, Vec<u8>)>) -> (Vec<u8>, Vec<(String, String)>) {
    let Some(at) = fields.iter().position(|(k, _)| k == "data") else {
        return (payload_of(&fields), Vec::new());
    };
    let mut fields = fields;
    let (_, payload) = fields.remove(at);
    let headers = fields
        .into_iter()
        .map(|(k, v)| {
            let text: String = String::from_utf8_lossy(&v).chars().take(200).collect();
            (k, text)
        })
        .collect();
    (payload, headers)
}

/// An entry's fields as one payload: a single field is its value; several
/// are a JSON object when they are all text, so the pane can lay them
/// out, and `field=value` lines when they are not.
fn payload_of(fields: &[(String, Vec<u8>)]) -> Vec<u8> {
    if let [(_, v)] = fields {
        return v.clone();
    }
    let texts: Option<Vec<(&str, &str)>> = fields
        .iter()
        .map(|(k, v)| std::str::from_utf8(v).ok().map(|v| (k.as_str(), v)))
        .collect();
    match texts {
        Some(t) => {
            let obj: serde_json::Map<String, serde_json::Value> = t
                .into_iter()
                .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
                .collect();
            serde_json::to_vec(&obj).unwrap_or_default()
        }
        None => {
            let mut out = Vec::new();
            for (k, v) in fields {
                out.extend_from_slice(k.as_bytes());
                out.push(b'=');
                out.extend_from_slice(v);
                out.push(b'\n');
            }
            out
        }
    }
}

impl Streams {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Streams> {
        let (host, port) = host_port(&cfg.address, 6379)?;
        let addr = if cfg.tls {
            redis::ConnectionAddr::TcpTls {
                host,
                port,
                insecure: false,
                tls_params: None,
            }
        } else {
            redis::ConnectionAddr::Tcp(host, port)
        };
        let mut settings = redis::RedisConnectionInfo::default().set_db(cfg.db);
        match &cfg.auth {
            Auth::Password { user, password } => {
                if !user.is_empty() {
                    settings = settings.set_username(user);
                }
                settings = settings.set_password(password);
            }
            // A token is sent as the password (`AUTH token`), which is
            // how managed Redis services that issue tokens take them.
            Auth::Token(t) => settings = settings.set_password(t),
            Auth::ApiKey { .. } | Auth::None => {}
        }
        use redis::IntoConnectionInfo;
        let info = addr
            .into_connection_info()
            .map_err(err)?
            .set_redis_settings(settings);
        let client = redis::Client::open(info).map_err(err)?;
        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(err)?;
        let server: String = redis::cmd("INFO")
            .arg("server")
            .query_async(&mut conn)
            .await
            .unwrap_or_default();
        let get = |name: &str| {
            server
                .lines()
                .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
                .map(|v| v.trim().to_string())
        };
        let info = match get("valkey_version") {
            Some(v) => ServerInfo {
                product: "Valkey".into(),
                version: v,
            },
            None => ServerInfo {
                product: "Redis".into(),
                version: get("redis_version").unwrap_or_default(),
            },
        };
        Ok(Streams { conn, info })
    }

    async fn stream_keys(&mut self) -> Result<(Vec<String>, bool)> {
        let mut cursor: u64 = 0;
        let mut keys = Vec::new();
        let mut scanned = 0usize;
        loop {
            let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("COUNT")
                .arg(1000)
                .arg("TYPE")
                .arg("stream")
                .query_async(&mut self.conn)
                .await
                .map_err(err)?;
            scanned += 1000;
            keys.extend(batch);
            cursor = next;
            if cursor == 0 {
                break;
            }
            if keys.len() >= MAX_STREAMS || scanned >= MAX_SCANNED {
                keys.truncate(MAX_STREAMS);
                keys.sort();
                return Ok((keys, true));
            }
        }
        keys.sort();
        keys.dedup();
        Ok((keys, false))
    }
}

#[async_trait]
impl Driver for Streams {
    fn info(&self) -> &ServerInfo {
        &self.info
    }

    fn caps(&self) -> Caps {
        Caps {
            consumers: true,
            purge: true,
            delete: true,
            bindings: false,
            peek_from: true,
        }
    }

    async fn destinations(&mut self) -> Result<Listing> {
        let (keys, cut) = self.stream_keys().await?;
        let mut destinations = Vec::with_capacity(keys.len());
        for key in keys {
            let v: Value = match redis::cmd("XINFO")
                .arg("STREAM")
                .arg(&key)
                .query_async(&mut self.conn)
                .await
            {
                Ok(v) => v,
                // Deleted between the scan and now.
                Err(_) => continue,
            };
            let p = pairs(&v);
            let length = int(field(&p, "length")).unwrap_or(0).max(0) as u64;
            let first = field(&p, "first-entry")
                .and_then(entry)
                .map(|(id, _)| id)
                .unwrap_or_default();
            let last = field(&p, "last-generated-id").map(text).unwrap_or_default();
            destinations.push(Destination {
                name: key,
                scope: String::new(),
                figures: vec![
                    length.to_string(),
                    int(field(&p, "groups")).unwrap_or(0).to_string(),
                    first,
                    last,
                ],
                depth: Some(length),
            });
        }
        Ok(Listing {
            destinations,
            note: cut.then(|| format!("the first {MAX_STREAMS} streams found")),
        })
    }

    async fn peek(&mut self, dest: &Destination, req: &PeekRequest) -> Result<Vec<Message>> {
        let from = if req.from.trim().is_empty() {
            "-"
        } else {
            req.from.trim()
        };
        let v: Value = redis::cmd("XRANGE")
            .arg(&dest.name)
            .arg(from)
            .arg("+")
            .arg("COUNT")
            .arg(req.count)
            .query_async(&mut self.conn)
            .await
            .map_err(err)?;
        let Value::Array(entries) = v else {
            return Ok(Vec::new());
        };
        Ok(entries
            .iter()
            .filter_map(entry)
            .map(|(id, fields)| {
                let (payload, headers) = split_entry(fields);
                Message {
                    timestamp: id_time(&id),
                    payload,
                    headers,
                    position: id,
                    ..Default::default()
                }
            })
            .collect())
    }

    async fn publish(&mut self, req: &PublishRequest) -> Result<String> {
        // The fields are the form's header rows; a body, when there is
        // one, travels as a `data` field.
        let mut cmd = redis::cmd("XADD");
        cmd.arg(&req.target).arg("*");
        let mut n = 0;
        for (k, v) in &req.headers {
            cmd.arg(k).arg(v);
            n += 1;
        }
        if !req.payload.is_empty() {
            cmd.arg("data").arg(&req.payload);
            n += 1;
        }
        if n == 0 {
            return Err("a stream entry needs at least one field".into());
        }
        let id: String = cmd.query_async(&mut self.conn).await.map_err(err)?;
        Ok(format!("added {id}"))
    }

    async fn consumers(
        &mut self,
        dest: &Destination,
        _progress: &Progress,
    ) -> Result<Vec<ConsumerGroup>> {
        let v: Value = redis::cmd("XINFO")
            .arg("GROUPS")
            .arg(&dest.name)
            .query_async(&mut self.conn)
            .await
            .map_err(err)?;
        let Value::Array(groups) = v else {
            return Ok(Vec::new());
        };
        Ok(groups
            .iter()
            .map(|g| {
                let p = pairs(g);
                ConsumerGroup {
                    name: field(&p, "name").map(text).unwrap_or_default(),
                    state: format!(
                        "last delivered {}",
                        field(&p, "last-delivered-id").map(text).unwrap_or_default()
                    ),
                    members: int(field(&p, "consumers")).unwrap_or(0).max(0) as u64,
                    pending: int(field(&p, "pending")).map(|n| n.max(0) as u64),
                    // Redis 7 and later; nil when the server cannot tell.
                    lag: int(field(&p, "lag")),
                    detail: Vec::new(),
                }
            })
            .collect())
    }

    async fn purge(&mut self, dest: &Destination) -> Result<String> {
        let n: i64 = redis::cmd("XTRIM")
            .arg(&dest.name)
            .arg("MAXLEN")
            .arg(0)
            .query_async(&mut self.conn)
            .await
            .map_err(err)?;
        Ok(format!("trimmed {n} entries from “{}”", dest.name))
    }

    async fn delete(&mut self, dest: &Destination) -> Result<()> {
        let _: i64 = redis::cmd("DEL")
            .arg(&dest.name)
            .query_async(&mut self.conn)
            .await
            .map_err(err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_field_is_the_payload_and_several_are_an_object() {
        assert_eq!(
            payload_of(&[("data".into(), b"{\"a\":1}".to_vec())]),
            b"{\"a\":1}"
        );
        let two = payload_of(&[("a".into(), b"1".to_vec()), ("b".into(), b"x".to_vec())]);
        let v: serde_json::Value = serde_json::from_slice(&two).unwrap();
        assert_eq!(v["a"], "1");
        assert_eq!(v["b"], "x");
        // Binary values cannot go in JSON; they are listed instead.
        let bin = payload_of(&[("a".into(), vec![0xff]), ("b".into(), b"x".to_vec())]);
        assert!(bin.starts_with(b"a="));
    }

    #[test]
    fn a_data_field_is_the_payload_and_the_rest_are_headers() {
        let (payload, headers) = split_entry(vec![
            ("kind".into(), b"order".to_vec()),
            ("data".into(), b"{}".to_vec()),
        ]);
        assert_eq!(payload, b"{}");
        assert_eq!(headers, vec![("kind".to_string(), "order".to_string())]);
        let (payload, headers) = split_entry(vec![("a".into(), b"1".to_vec())]);
        assert_eq!(payload, b"1");
        assert!(headers.is_empty());
    }

    #[test]
    fn an_entry_reply_is_read_into_its_id_and_fields() {
        let v = Value::Array(vec![
            Value::BulkString(b"1700000000000-0".to_vec()),
            Value::Array(vec![
                Value::BulkString(b"k".to_vec()),
                Value::BulkString(b"v".to_vec()),
            ]),
        ]);
        let (id, fields) = entry(&v).unwrap();
        assert_eq!(id, "1700000000000-0");
        assert_eq!(fields, vec![("k".to_string(), b"v".to_vec())]);
        assert!(id_time(&id).starts_with("2023-11-1"));
    }
}
