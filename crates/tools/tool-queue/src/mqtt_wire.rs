//! A small MQTT client: exactly what the Queue activity does with a broker,
//! which is connect, subscribe, read what arrives for a while, publish at
//! QoS 0, 1 or 2, and disconnect, for MQTT 3.1.1 and 5, over TCP or the
//! workspace's rustls (ADR-027).
//!
//! Written rather than taken from `rumqttc`, whose current release pins a
//! `rustls-webpki` with open advisories (docs/GOTCHAS.md, Queue). Nothing
//! here keeps a session between calls, so there is no reconnect logic, no
//! in-flight store and no keep-alive timer: a session is shorter than its
//! keep-alive.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub type Result<T> = std::result::Result<T, String>;

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// Control packet types (MQTT 3.1.1 §2.2.1, 5.0 §2.1.2).
const CONNECT: u8 = 1;
const CONNACK: u8 = 2;
const PUBLISH: u8 = 3;
const PUBACK: u8 = 4;
const PUBREC: u8 = 5;
const PUBREL: u8 = 6;
const PUBCOMP: u8 = 7;
const SUBSCRIBE: u8 = 8;
const SUBACK: u8 = 9;
const DISCONNECT: u8 = 14;

/// The user-property identifier in MQTT 5 properties.
const USER_PROPERTY: u8 = 0x26;

/// How to reach a broker and who to say we are.
#[derive(Clone)]
pub struct Options {
    pub host: String,
    pub port: u16,
    pub tls: Option<Arc<rustls::ClientConfig>>,
    pub v5: bool,
    pub client_id: String,
    pub credentials: Option<(String, String)>,
    pub timeout: Duration,
}

/// A message the broker sent us.
#[derive(Debug, Clone, PartialEq)]
pub struct Received {
    pub topic: String,
    pub payload: Vec<u8>,
    pub retain: bool,
    /// MQTT 5 user properties; empty for 3.1.1.
    pub properties: Vec<(String, String)>,
}

pub struct Client {
    io: Box<dyn Io>,
    v5: bool,
    timeout: Duration,
    next_id: u16,
}

// ---- encoding ----------------------------------------------------------

fn put_len(out: &mut Vec<u8>, mut n: usize) {
    loop {
        let mut b = (n % 128) as u8;
        n /= 128;
        if n > 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
}

fn put_str(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(&(s.len() as u16).to_be_bytes());
    out.extend_from_slice(s);
}

/// A whole packet: fixed header, remaining length, body.
fn packet(kind: u8, flags: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 5);
    out.push((kind << 4) | flags);
    put_len(&mut out, body.len());
    out.extend_from_slice(body);
    out
}

/// MQTT 5 properties: length, then user properties only.
fn put_properties(out: &mut Vec<u8>, user: &[(String, String)]) {
    let mut props = Vec::new();
    for (k, v) in user {
        props.push(USER_PROPERTY);
        put_str(&mut props, k.as_bytes());
        put_str(&mut props, v.as_bytes());
    }
    put_len(out, props.len());
    out.extend_from_slice(&props);
}

pub fn connect_packet(o: &Options) -> Vec<u8> {
    let mut b = Vec::new();
    put_str(&mut b, b"MQTT");
    b.push(if o.v5 { 5 } else { 4 });
    let mut flags = 0x02; // clean session / clean start
    if let Some((user, _)) = &o.credentials {
        // A token sent as the password has no username; MQTT 3.1.1 does
        // not allow a password without one, so an empty name is sent.
        flags |= 0x40;
        if !user.is_empty() || !o.v5 {
            flags |= 0x80;
        }
    }
    b.push(flags);
    b.extend_from_slice(&30u16.to_be_bytes()); // keep-alive, seconds
    if o.v5 {
        put_len(&mut b, 0);
    }
    put_str(&mut b, o.client_id.as_bytes());
    if let Some((user, password)) = &o.credentials {
        if !user.is_empty() || !o.v5 {
            put_str(&mut b, user.as_bytes());
        }
        put_str(&mut b, password.as_bytes());
    }
    packet(CONNECT, 0, &b)
}

pub fn publish_packet(
    v5: bool,
    id: u16,
    topic: &str,
    payload: &[u8],
    qos: u8,
    retain: bool,
    user: &[(String, String)],
) -> Vec<u8> {
    let mut b = Vec::new();
    put_str(&mut b, topic.as_bytes());
    if qos > 0 {
        b.extend_from_slice(&id.to_be_bytes());
    }
    if v5 {
        put_properties(&mut b, user);
    }
    b.extend_from_slice(payload);
    packet(PUBLISH, (qos.min(2) << 1) | u8::from(retain), &b)
}

fn subscribe_packet(v5: bool, id: u16, filter: &str) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&id.to_be_bytes());
    if v5 {
        put_len(&mut b, 0);
    }
    put_str(&mut b, filter.as_bytes());
    b.push(0); // QoS 0: a look keeps nothing in flight
    packet(SUBSCRIBE, 0x02, &b)
}

// ---- decoding ----------------------------------------------------------

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|e| *e <= self.b.len())
            .ok_or("the broker sent a short packet")?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn str(&mut self) -> Result<String> {
        let n = self.u16()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn varint(&mut self) -> Result<usize> {
        let mut n = 0usize;
        for shift in 0..4 {
            let b = self.u8()?;
            n |= usize::from(b & 0x7f) << (7 * shift);
            if b & 0x80 == 0 {
                return Ok(n);
            }
        }
        Err("the broker sent a malformed length".into())
    }
    fn rest(&self) -> &'a [u8] {
        &self.b[self.at..]
    }
}

/// MQTT 5 properties: the user properties, everything else skipped by its
/// declared size.
fn read_properties(r: &mut Reader) -> Result<Vec<(String, String)>> {
    let len = r.varint()?;
    let mut p = Reader {
        b: r.take(len)?,
        at: 0,
    };
    let mut user = Vec::new();
    while !p.rest().is_empty() {
        let id = p.u8()?;
        match id {
            USER_PROPERTY => {
                let k = p.str()?;
                let v = p.str()?;
                user.push((k, v));
            }
            // One-byte properties.
            0x01 | 0x17 | 0x19 | 0x24 | 0x25 | 0x28 | 0x29 | 0x2A => {
                p.u8()?;
            }
            // Two-byte integers.
            0x13 | 0x21 | 0x22 | 0x23 => {
                p.u16()?;
            }
            // Four-byte integers.
            0x02 | 0x11 | 0x18 | 0x27 => {
                p.take(4)?;
            }
            // Variable-byte integer (subscription identifier).
            0x0B => {
                p.varint()?;
            }
            // Strings and binary data.
            0x03 | 0x08 | 0x09 | 0x12 | 0x15 | 0x16 | 0x1A | 0x1C | 0x1F => {
                let n = p.u16()? as usize;
                p.take(n)?;
            }
            other => return Err(format!("the broker sent an unknown property {other:#x}")),
        }
    }
    Ok(user)
}

pub fn parse_publish(v5: bool, flags: u8, body: &[u8]) -> Result<(Received, Option<u16>)> {
    let mut r = Reader { b: body, at: 0 };
    let topic = r.str()?;
    let qos = (flags >> 1) & 0x03;
    let id = if qos > 0 { Some(r.u16()?) } else { None };
    let properties = if v5 {
        read_properties(&mut r)?
    } else {
        Vec::new()
    };
    Ok((
        Received {
            topic,
            payload: r.rest().to_vec(),
            retain: flags & 0x01 == 1,
            properties,
        },
        id,
    ))
}

fn connack_error(v5: bool, code: u8) -> String {
    let why = match (v5, code) {
        (false, 1) => "unacceptable protocol version",
        (false, 2) => "client id rejected",
        (false, 3) => "server unavailable",
        (false, 4) | (true, 0x86) => "bad user name or password",
        (false, 5) | (true, 0x87) => "not authorised",
        (true, 0x84) => "unsupported protocol version",
        (true, 0x85) => "client id rejected",
        (true, 0x88) => "server unavailable",
        (true, 0x8C) => "bad authentication method",
        _ => "refused",
    };
    format!("the broker refused the connection: {why} ({code:#x})")
}

impl Client {
    pub async fn connect(o: &Options) -> Result<Client> {
        let tcp = tokio::time::timeout(
            o.timeout,
            tokio::net::TcpStream::connect((o.host.as_str(), o.port)),
        )
        .await
        .map_err(|_| format!("{}:{} did not answer in time", o.host, o.port))?
        .map_err(|e| format!("{}:{}: {e}", o.host, o.port))?;
        let io: Box<dyn Io> = match &o.tls {
            Some(cfg) => {
                let name = rustls::pki_types::ServerName::try_from(o.host.clone())
                    .map_err(|e| e.to_string())?;
                Box::new(
                    tokio_rustls::TlsConnector::from(cfg.clone())
                        .connect(name, tcp)
                        .await
                        .map_err(|e| format!("TLS to {}:{}: {e}", o.host, o.port))?,
                )
            }
            None => Box::new(tcp),
        };
        let mut c = Client {
            io,
            v5: o.v5,
            timeout: o.timeout,
            next_id: 1,
        };
        c.write(&connect_packet(o)).await?;
        let deadline = Instant::now() + o.timeout;
        let (kind, _, body) = c
            .read_until(deadline)
            .await?
            .ok_or("the broker did not answer in time")?;
        if kind != CONNACK {
            return Err(format!(
                "the broker answered CONNECT with packet type {kind}"
            ));
        }
        let code = *body.get(1).ok_or("the broker sent a short CONNACK")?;
        if code != 0 {
            return Err(connack_error(o.v5, code));
        }
        Ok(c)
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        tokio::time::timeout(self.timeout, async {
            self.io.write_all(bytes).await?;
            self.io.flush().await
        })
        .await
        .map_err(|_| "the broker stopped reading".to_string())?
        .map_err(|e| e.to_string())
    }

    /// The next packet, or `None` when `deadline` passes first.
    async fn read_until(&mut self, deadline: Instant) -> Result<Option<(u8, u8, Vec<u8>)>> {
        let left = deadline.saturating_duration_since(Instant::now());
        let read = async {
            let first = self.io.read_u8().await?;
            let mut len = 0usize;
            for shift in 0..4 {
                let b = self.io.read_u8().await?;
                len |= usize::from(b & 0x7f) << (7 * shift);
                if b & 0x80 == 0 {
                    break;
                }
            }
            let mut body = vec![0u8; len];
            self.io.read_exact(&mut body).await?;
            Ok::<_, std::io::Error>((first >> 4, first & 0x0f, body))
        };
        match tokio::time::timeout(left, read).await {
            Err(_) => Ok(None),
            Ok(Err(e)) => Err(format!("the broker closed the connection: {e}")),
            Ok(Ok(p)) => Ok(Some(p)),
        }
    }

    fn id(&mut self) -> u16 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    /// Wait for the acknowledgement of type `kind` for packet `id`,
    /// passing over anything else that arrives meanwhile.
    async fn wait_ack(&mut self, kind: u8, id: u16) -> Result<()> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let (k, _, body) = self
                .read_until(deadline)
                .await?
                .ok_or("the broker did not acknowledge in time")?;
            if k == kind && body.len() >= 2 && u16::from_be_bytes([body[0], body[1]]) == id {
                // MQTT 5 acks carry a reason code; 0x80 and up is a failure.
                if self.v5 && body.get(2).is_some_and(|c| *c >= 0x80) && kind != SUBACK {
                    return Err(format!("the broker refused it (reason {:#x})", body[2]));
                }
                return Ok(());
            }
        }
    }

    pub async fn publish(
        &mut self,
        topic: &str,
        payload: &[u8],
        qos: u8,
        retain: bool,
        user: &[(String, String)],
    ) -> Result<()> {
        let id = self.id();
        let p = publish_packet(self.v5, id, topic, payload, qos, retain, user);
        self.write(&p).await?;
        match qos {
            0 => Ok(()),
            1 => self.wait_ack(PUBACK, id).await,
            _ => {
                self.wait_ack(PUBREC, id).await?;
                let mut b = id.to_be_bytes().to_vec();
                if self.v5 {
                    b.push(0);
                }
                self.write(&packet(PUBREL, 0x02, &b)).await?;
                self.wait_ack(PUBCOMP, id).await
            }
        }
    }

    /// Subscribe to `filter` and collect what arrives until `window`
    /// passes or `max` messages have come; `true` when it stopped at `max`.
    pub async fn observe(
        &mut self,
        filter: &str,
        window: Duration,
        max: usize,
    ) -> Result<(Vec<Received>, bool)> {
        let id = self.id();
        let p = subscribe_packet(self.v5, id, filter);
        self.write(&p).await?;
        self.wait_ack(SUBACK, id).await?;
        let deadline = Instant::now() + window;
        let mut out = Vec::new();
        while let Some((kind, flags, body)) = self.read_until(deadline).await? {
            if kind != PUBLISH {
                continue;
            }
            let (m, id) = parse_publish(self.v5, flags, &body)?;
            // Subscribed at QoS 0, but a broker may still send QoS 1.
            if let Some(id) = id {
                let mut b = id.to_be_bytes().to_vec();
                if self.v5 {
                    b.push(0);
                }
                self.write(&packet(PUBACK, 0, &b)).await?;
            }
            out.push(m);
            if out.len() >= max {
                return Ok((out, true));
            }
        }
        Ok((out, false))
    }

    pub async fn disconnect(mut self) {
        let p = if self.v5 {
            packet(DISCONNECT, 0, &[0, 0])
        } else {
            packet(DISCONNECT, 0, &[])
        };
        let _ = self.write(&p).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_lengths_use_the_variable_byte_encoding() {
        // MQTT 3.1.1 §2.2.3's examples.
        for (n, bytes) in [
            (0usize, vec![0x00]),
            (127, vec![0x7f]),
            (128, vec![0x80, 0x01]),
            (16_383, vec![0xff, 0x7f]),
            (16_384, vec![0x80, 0x80, 0x01]),
            (2_097_152, vec![0x80, 0x80, 0x80, 0x01]),
        ] {
            let mut out = Vec::new();
            put_len(&mut out, n);
            assert_eq!(out, bytes, "{n}");
            assert_eq!(Reader { b: &out, at: 0 }.varint().unwrap(), n);
        }
    }

    #[test]
    fn a_3_1_1_connect_with_a_login_is_laid_out_as_the_spec_says() {
        let o = Options {
            host: String::new(),
            port: 0,
            tls: None,
            v5: false,
            client_id: "hs".into(),
            credentials: Some(("u".into(), "p".into())),
            timeout: Duration::from_secs(1),
        };
        assert_eq!(
            connect_packet(&o),
            vec![
                0x10, 20, // CONNECT, remaining length
                0, 4, b'M', b'Q', b'T', b'T', 4,    // protocol name and level
                0xC2, // user name, password, clean session
                0, 30, // keep-alive
                0, 2, b'h', b's', // client id
                0, 1, b'u', // user name
                0, 1, b'p', // password
            ]
        );
    }

    #[test]
    fn a_published_message_round_trips_through_the_parser() {
        for v5 in [false, true] {
            let user = vec![("k".to_string(), "v".to_string())];
            let p = publish_packet(v5, 7, "a/b", b"hi", 1, true, &user);
            assert_eq!(p[0], 0x30 | 0x02 | 0x01, "PUBLISH, QoS 1, retain");
            let body_at = 2; // a short packet's length fits in one byte
            let (m, id) = parse_publish(v5, p[0] & 0x0f, &p[body_at..]).unwrap();
            assert_eq!(id, Some(7));
            assert_eq!(m.topic, "a/b");
            assert_eq!(m.payload, b"hi");
            assert!(m.retain);
            assert_eq!(m.properties, if v5 { user } else { Vec::new() });
        }
    }

    #[test]
    fn unknown_lengths_in_properties_are_skipped_by_size() {
        // Message expiry (4 bytes), content type (string), a user property.
        let mut props = vec![0x02, 0, 0, 0, 60, 0x03];
        put_str(&mut props, b"application/json");
        props.push(USER_PROPERTY);
        put_str(&mut props, b"a");
        put_str(&mut props, b"b");
        let mut b = Vec::new();
        put_len(&mut b, props.len());
        b.extend_from_slice(&props);
        let user = read_properties(&mut Reader { b: &b, at: 0 }).unwrap();
        assert_eq!(user, vec![("a".to_string(), "b".to_string())]);
    }

    #[test]
    fn a_refused_connection_says_why() {
        assert!(connack_error(false, 4).contains("user name or password"));
        assert!(connack_error(true, 0x87).contains("not authorised"));
    }
}
