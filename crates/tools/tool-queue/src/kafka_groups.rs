//! Kafka consumer groups and their lag, through `kafka-protocol` messages
//! on a small client of our own: `rskafka` has no group API (ADR-027).
//!
//! It asks every broker for its groups (`ListGroups`), each group's
//! coordinator for the group's committed offsets (`FindCoordinator`,
//! `OffsetFetch`) and members (`DescribeGroups`). Lag is the partition's
//! latest offset minus the committed one. The versions of each request
//! are agreed with the broker (`ApiVersions`) rather than fixed, because
//! Kafka 4 dropped many old ones (KIP-896).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use bytes::{Bytes, BytesMut};
use hmac::{Hmac, Mac};
use kafka_protocol::messages::{
    ApiVersionsRequest, DescribeGroupsRequest, FindCoordinatorRequest, GroupId, ListGroupsRequest,
    MetadataRequest, OffsetFetchRequest, RequestHeader, ResponseHeader, SaslAuthenticateRequest,
    SaslHandshakeRequest,
};
use kafka_protocol::protocol::{Decodable, Encodable, HeaderVersion, Request, StrBytes};
use sha2::{Digest, Sha256, Sha512};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::driver::{ConsumerGroup, Lag, Progress, Result};
use crate::profile::Auth;

/// The most groups one look reads; a cluster with more says so.
pub const MAX_GROUPS: usize = 500;

/// OffsetFetch v10 names topics by id rather than name (KIP-848), which
/// this client does not resolve; v9 still names them.
const OFFSET_FETCH_MAX: i16 = 9;

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// How to reach and authenticate to the cluster's brokers.
#[derive(Clone)]
pub struct Settings {
    pub bootstrap: Vec<String>,
    pub tls: Option<Arc<rustls::ClientConfig>>,
    pub auth: Auth,
    /// plain | scram-sha-256 | scram-sha-512 (with a password)
    pub sasl: String,
    pub timeout: Duration,
}

struct Conn {
    io: Box<dyn Io>,
    /// api key → (min, max) the broker accepts.
    versions: HashMap<i16, (i16, i16)>,
    correlation: i32,
    timeout: Duration,
}

fn str_bytes(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_string())
}

fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

/// A Kafka error code as words, for the few a user will meet here.
fn code_text(code: i16) -> String {
    match code {
        14 => "the group coordinator is loading".into(),
        15 => "the group coordinator is not available".into(),
        16 => "this broker is not the group coordinator".into(),
        29 => "not authorised to read this topic".into(),
        30 => "not authorised to read this group".into(),
        31 => "not authorised to read the cluster".into(),
        33 => "the broker does not support that SASL mechanism".into(),
        58 => "SASL authentication failed".into(),
        other => format!("Kafka error {other}"),
    }
}

impl Conn {
    async fn open(host: &str, port: u16, s: &Settings) -> Result<Conn> {
        let tcp = tokio::time::timeout(s.timeout, tokio::net::TcpStream::connect((host, port)))
            .await
            .map_err(|_| {
                format!(
                    "{host}:{port} did not answer within {} s",
                    s.timeout.as_secs()
                )
            })?
            .map_err(|err| format!("{host}:{port}: {err}"))?;
        let io: Box<dyn Io> = match &s.tls {
            Some(cfg) => {
                let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(e)?;
                let tls = tokio_rustls::TlsConnector::from(cfg.clone())
                    .connect(name, tcp)
                    .await
                    .map_err(|err| format!("TLS to {host}:{port}: {err}"))?;
                Box::new(tls)
            }
            None => Box::new(tcp),
        };
        let mut c = Conn {
            io,
            versions: HashMap::new(),
            correlation: 0,
            timeout: s.timeout,
        };
        // ApiVersions v0 is the one request every broker answers before
        // anything is agreed.
        c.versions.insert(ApiVersionsRequest::KEY, (0, 0));
        let v = c.send(ApiVersionsRequest::default(), 0).await?;
        if v.error_code != 0 {
            return Err(code_text(v.error_code));
        }
        c.versions = v
            .api_keys
            .iter()
            .map(|k| (k.api_key, (k.min_version, k.max_version)))
            .collect();
        c.authenticate(s).await?;
        Ok(c)
    }

    /// The highest version both sides speak, capped at `cap`.
    fn version<R: Request>(&self, cap: i16) -> Result<i16> {
        let (bmin, bmax) = self
            .versions
            .get(&R::KEY)
            .copied()
            .ok_or_else(|| format!("the broker does not offer API {}", R::KEY))?;
        let v = bmax.min(R::VERSIONS.max).min(cap);
        if v < bmin.max(R::VERSIONS.min) {
            return Err(format!(
                "no version of API {} that both this client and the broker speak",
                R::KEY
            ));
        }
        Ok(v)
    }

    async fn send<R: Request>(&mut self, req: R, cap: i16) -> Result<R::Response> {
        let version = self.version::<R>(cap)?;
        self.correlation += 1;
        let mut header = RequestHeader::default();
        header.request_api_key = R::KEY;
        header.request_api_version = version;
        header.correlation_id = self.correlation;
        header.client_id = Some(str_bytes("hyperstack"));
        let mut buf = BytesMut::new();
        header
            .encode(&mut buf, R::header_version(version))
            .map_err(e)?;
        req.encode(&mut buf, version).map_err(e)?;
        let timeout = self.timeout;
        let body = tokio::time::timeout(timeout, async {
            self.io.write_u32(buf.len() as u32).await?;
            self.io.write_all(&buf).await?;
            self.io.flush().await?;
            let len = self.io.read_u32().await? as usize;
            let mut body = vec![0u8; len];
            self.io.read_exact(&mut body).await?;
            Ok::<_, std::io::Error>(body)
        })
        .await
        .map_err(|_| format!("the broker did not answer within {} s", timeout.as_secs()))?
        .map_err(e)?;
        let mut body = Bytes::from(body);
        let rh =
            ResponseHeader::decode(&mut body, R::Response::header_version(version)).map_err(e)?;
        if rh.correlation_id != self.correlation {
            return Err("the broker answered a different request".into());
        }
        R::Response::decode(&mut body, version).map_err(e)
    }

    async fn sasl_step(&mut self, bytes: Vec<u8>) -> Result<Vec<u8>> {
        let mut req = SaslAuthenticateRequest::default();
        req.auth_bytes = Bytes::from(bytes);
        let r = self.send(req, i16::MAX).await?;
        if r.error_code != 0 {
            return Err(r
                .error_message
                .map(|m| m.to_string())
                .unwrap_or_else(|| code_text(r.error_code)));
        }
        Ok(r.auth_bytes.to_vec())
    }

    async fn authenticate(&mut self, s: &Settings) -> Result<()> {
        let (mechanism, user, secret) = match &s.auth {
            Auth::None => return Ok(()),
            Auth::Password { user, password } => (
                match s.sasl.as_str() {
                    "scram-sha-256" => "SCRAM-SHA-256",
                    "scram-sha-512" => "SCRAM-SHA-512",
                    _ => "PLAIN",
                },
                user.as_str(),
                password.as_str(),
            ),
            Auth::Token(t) => ("OAUTHBEARER", "", t.as_str()),
            // Confluent Cloud's form: the key id and secret as PLAIN.
            Auth::ApiKey { name, key } => ("PLAIN", name.as_str(), key.as_str()),
        };
        let mut hs = SaslHandshakeRequest::default();
        hs.mechanism = str_bytes(mechanism);
        let r = self.send(hs, 1).await?;
        if r.error_code != 0 {
            let offered: Vec<String> = r.mechanisms.iter().map(|m| m.to_string()).collect();
            return Err(format!(
                "the broker does not accept {mechanism} (it offers {})",
                offered.join(", ")
            ));
        }
        match mechanism {
            "PLAIN" => {
                self.sasl_step(format!("\0{user}\0{secret}").into_bytes())
                    .await?;
            }
            "OAUTHBEARER" => {
                self.sasl_step(format!("n,,\x01auth=Bearer {secret}\x01\x01").into_bytes())
                    .await?;
            }
            m => {
                let algo = if m == "SCRAM-SHA-512" {
                    Algo::Sha512
                } else {
                    Algo::Sha256
                };
                let mut scram = Scram::new(algo, user, secret, &nonce());
                let server_first = self.sasl_step(scram.client_first().into_bytes()).await?;
                let client_final = scram.client_final(&String::from_utf8_lossy(&server_first))?;
                let server_final = self.sasl_step(client_final.into_bytes()).await?;
                scram.verify(&String::from_utf8_lossy(&server_final))?;
            }
        }
        Ok(())
    }
}

fn nonce() -> String {
    let bytes: [u8; 18] = rand::random();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ---- SCRAM (RFC 5802, RFC 7677) -------------------------------------------

#[derive(Clone, Copy)]
enum Algo {
    Sha256,
    Sha512,
}

fn hmac(algo: Algo, key: &[u8], data: &[u8]) -> Vec<u8> {
    match algo {
        Algo::Sha256 => {
            let mut m = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key");
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }
        Algo::Sha512 => {
            let mut m = Hmac::<Sha512>::new_from_slice(key).expect("HMAC takes any key");
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }
    }
}

fn hash(algo: Algo, data: &[u8]) -> Vec<u8> {
    match algo {
        Algo::Sha256 => Sha256::digest(data).to_vec(),
        Algo::Sha512 => Sha512::digest(data).to_vec(),
    }
}

/// `Hi()`: PBKDF2 with the HMAC as its PRF and one block of output.
fn hi(algo: Algo, password: &[u8], salt: &[u8], iterations: u32) -> Vec<u8> {
    let mut first = salt.to_vec();
    first.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac(algo, password, &first);
    let mut out = u.clone();
    for _ in 1..iterations {
        u = hmac(algo, password, &u);
        for (o, b) in out.iter_mut().zip(&u) {
            *o ^= b;
        }
    }
    out
}

struct Scram {
    algo: Algo,
    password: String,
    nonce: String,
    client_first_bare: String,
    server_signature: Vec<u8>,
}

impl Scram {
    fn new(algo: Algo, user: &str, password: &str, nonce: &str) -> Scram {
        let user = user.replace('=', "=3D").replace(',', "=2C");
        Scram {
            algo,
            client_first_bare: format!("n={user},r={nonce}"),
            password: password.to_string(),
            nonce: nonce.to_string(),
            server_signature: Vec::new(),
        }
    }

    fn client_first(&self) -> String {
        format!("n,,{}", self.client_first_bare)
    }

    fn client_final(&mut self, server_first: &str) -> Result<String> {
        let attr = |k: &str| {
            server_first
                .split(',')
                .find_map(|p| p.strip_prefix(k)?.strip_prefix('='))
                .map(str::to_string)
        };
        let nonce = attr("r").ok_or("SCRAM: the server sent no nonce")?;
        if !nonce.starts_with(&self.nonce) {
            return Err("SCRAM: the server's nonce does not extend ours".into());
        }
        let salt = base64::engine::general_purpose::STANDARD
            .decode(attr("s").ok_or("SCRAM: the server sent no salt")?)
            .map_err(e)?;
        let iterations: u32 = attr("i")
            .ok_or("SCRAM: the server sent no iteration count")?
            .parse()
            .map_err(e)?;
        let salted = hi(self.algo, self.password.as_bytes(), &salt, iterations);
        let client_key = hmac(self.algo, &salted, b"Client Key");
        let stored_key = hash(self.algo, &client_key);
        let without_proof = format!("c=biws,r={nonce}");
        let auth_message = format!("{},{server_first},{without_proof}", self.client_first_bare);
        let signature = hmac(self.algo, &stored_key, auth_message.as_bytes());
        let proof: Vec<u8> = client_key
            .iter()
            .zip(&signature)
            .map(|(a, b)| a ^ b)
            .collect();
        let server_key = hmac(self.algo, &salted, b"Server Key");
        self.server_signature = hmac(self.algo, &server_key, auth_message.as_bytes());
        Ok(format!(
            "{without_proof},p={}",
            base64::engine::general_purpose::STANDARD.encode(proof)
        ))
    }

    /// The server proves it knew the password too.
    fn verify(&self, server_final: &str) -> Result<()> {
        if let Some(err) = server_final.strip_prefix("e=") {
            return Err(format!("SCRAM: {err}"));
        }
        let v = server_final
            .strip_prefix("v=")
            .ok_or("SCRAM: the server sent no signature")?;
        let v = base64::engine::general_purpose::STANDARD
            .decode(v)
            .map_err(e)?;
        if v != self.server_signature {
            return Err("SCRAM: the server's signature does not match".into());
        }
        Ok(())
    }
}

// ---- groups ----------------------------------------------------------------

/// Connections to the brokers met during one look, by address.
struct Pool<'a> {
    s: &'a Settings,
    conns: HashMap<(String, u16), Conn>,
}

impl Pool<'_> {
    async fn get(&mut self, host: &str, port: u16) -> Result<&mut Conn> {
        let key = (host.to_string(), port);
        if !self.conns.contains_key(&key) {
            let c = Conn::open(host, port, self.s).await?;
            self.conns.insert(key.clone(), c);
        }
        Ok(self.conns.get_mut(&key).expect("inserted above"))
    }
}

/// Every group with committed offsets on `topic`, and its lag per
/// partition against `latest` (partition → latest offset).
pub async fn topic_groups(
    s: &Settings,
    topic: &str,
    latest: &HashMap<i32, i64>,
    progress: &Progress,
) -> Result<(Vec<ConsumerGroup>, bool)> {
    let mut pool = Pool {
        s,
        conns: HashMap::new(),
    };
    // The brokers, from the first bootstrap server that answers.
    let mut brokers: Vec<(String, u16)> = Vec::new();
    let mut last_err = String::from("no bootstrap servers");
    for b in &s.bootstrap {
        let (host, port) = crate::profile::host_port(b, 9092)?;
        match pool.get(&host, port).await {
            Ok(c) => {
                let mut md = MetadataRequest::default();
                md.topics = Some(Vec::new());
                let r = c.send(md, 12).await?;
                brokers = r
                    .brokers
                    .iter()
                    .map(|b| (b.host.to_string(), b.port as u16))
                    .collect();
                break;
            }
            Err(err) => last_err = err,
        }
    }
    if brokers.is_empty() {
        return Err(last_err);
    }
    // Every broker coordinates some of the groups.
    let mut groups: Vec<(String, String)> = Vec::new();
    for (host, port) in &brokers {
        let c = pool.get(host, *port).await?;
        let r = c.send(ListGroupsRequest::default(), 5).await?;
        if r.error_code != 0 {
            return Err(code_text(r.error_code));
        }
        for g in r.groups {
            // Connect and share groups only: a "consumer" protocol, or
            // the new consumer protocol's empty type.
            let proto = g.protocol_type.to_string();
            if proto.is_empty() || proto == "consumer" {
                groups.push((g.group_id.0.to_string(), g.group_state.to_string()));
            }
        }
    }
    groups.sort();
    groups.dedup_by(|a, b| a.0 == b.0);
    let cut = groups.len() > MAX_GROUPS;
    groups.truncate(MAX_GROUPS);
    let (bhost, bport) = brokers[0].clone();
    let mut out = Vec::new();
    progress.start(groups.len());
    for (group, listed_state) in groups {
        if progress.cancelled() {
            return Err("stopped".into());
        }
        progress.step();
        // Where this group lives.
        let coord = {
            let c = pool.get(&bhost, bport).await?;
            let v = c.version::<FindCoordinatorRequest>(i16::MAX)?;
            let mut req = FindCoordinatorRequest::default();
            if v >= 4 {
                req.coordinator_keys = vec![str_bytes(&group)];
            } else {
                req.key = str_bytes(&group);
            }
            let r = c.send(req, i16::MAX).await?;
            if v >= 4 {
                let Some(k) = r.coordinators.first() else {
                    continue;
                };
                if k.error_code != 0 {
                    continue;
                }
                (k.host.to_string(), k.port as u16)
            } else {
                if r.error_code != 0 {
                    continue;
                }
                (r.host.to_string(), r.port as u16)
            }
        };
        let c = pool.get(&coord.0, coord.1).await?;
        // Its committed offsets on this topic.
        let v = c.version::<OffsetFetchRequest>(OFFSET_FETCH_MAX)?;
        let mut req = OffsetFetchRequest::default();
        let mut committed: Vec<(i32, i64)> = Vec::new();
        if v >= 8 {
            let mut g =
                kafka_protocol::messages::offset_fetch_request::OffsetFetchRequestGroup::default();
            g.group_id = GroupId(str_bytes(&group));
            g.topics = None;
            req.groups = vec![g];
            let r = c.send(req, OFFSET_FETCH_MAX).await?;
            for g in r.groups {
                for t in g.topics.iter().filter(|t| &*t.name.0 == topic) {
                    for p in &t.partitions {
                        if p.error_code == 0 {
                            committed.push((p.partition_index, p.committed_offset));
                        }
                    }
                }
            }
        } else {
            req.group_id = GroupId(str_bytes(&group));
            req.topics = None;
            let r = c.send(req, OFFSET_FETCH_MAX).await?;
            for t in r.topics.iter().filter(|t| &*t.name.0 == topic) {
                for p in &t.partitions {
                    if p.error_code == 0 {
                        committed.push((p.partition_index, p.committed_offset));
                    }
                }
            }
        }
        if committed.is_empty() {
            continue;
        }
        // Its members, and its state as the coordinator sees it now.
        let mut dg = DescribeGroupsRequest::default();
        dg.groups = vec![GroupId(str_bytes(&group))];
        let (state, members) = match c.send(dg, 5).await {
            Ok(r) => r
                .groups
                .first()
                .map(|g| (g.group_state.to_string(), g.members.len() as u64))
                .unwrap_or((listed_state, 0)),
            Err(_) => (listed_state, 0),
        };
        out.push(group_lag(group, state, members, committed, latest));
    }
    Ok((out, cut))
}

/// One group's lag from its committed offsets. A committed offset of -1
/// means nothing committed for that partition yet.
fn group_lag(
    name: String,
    state: String,
    members: u64,
    mut committed: Vec<(i32, i64)>,
    latest: &HashMap<i32, i64>,
) -> ConsumerGroup {
    committed.sort();
    let detail: Vec<Lag> = committed
        .into_iter()
        .map(|(partition, off)| {
            let committed = (off >= 0).then_some(off);
            let latest = latest.get(&partition).copied();
            Lag {
                partition,
                committed,
                latest,
                lag: match (committed, latest) {
                    (Some(c), Some(l)) => Some((l - c).max(0)),
                    _ => None,
                },
            }
        })
        .collect();
    let lags: Vec<i64> = detail.iter().filter_map(|d| d.lag).collect();
    ConsumerGroup {
        name,
        state,
        members,
        pending: None,
        lag: (!lags.is_empty()).then(|| lags.iter().sum()),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7677 §3, the SCRAM-SHA-256 example exchange.
    #[test]
    fn scram_sha_256_matches_the_rfc_example() {
        let mut s = Scram::new(Algo::Sha256, "user", "pencil", "rOprNGfwEbeRWgbNEkqO");
        assert_eq!(s.client_first(), "n,,n=user,r=rOprNGfwEbeRWgbNEkqO");
        let server_first = "r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096";
        assert_eq!(
            s.client_final(server_first).unwrap(),
            "c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,p=dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ="
        );
        assert!(
            s.verify("v=6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=")
                .is_ok()
        );
        assert!(s.verify("v=AAAA").is_err());
        assert!(
            s.verify("e=invalid-proof")
                .unwrap_err()
                .contains("invalid-proof")
        );
    }

    #[test]
    fn a_server_nonce_that_does_not_extend_ours_is_refused() {
        let mut s = Scram::new(Algo::Sha512, "u", "p", "abc");
        assert!(s.client_final("r=xyz,s=AAAA,i=4096").is_err());
    }

    #[test]
    fn usernames_are_escaped_for_scram() {
        let s = Scram::new(Algo::Sha256, "a=b,c", "p", "n");
        assert_eq!(s.client_first(), "n,,n=a=3Db=2Cc,r=n");
    }

    #[test]
    fn lag_is_latest_minus_committed_and_unknown_without_a_commit() {
        let latest = HashMap::from([(0, 100), (1, 50)]);
        let g = group_lag(
            "g".into(),
            "Stable".into(),
            2,
            vec![(1, -1), (0, 90)],
            &latest,
        );
        assert_eq!(g.detail[0].partition, 0);
        assert_eq!(g.detail[0].lag, Some(10));
        assert_eq!(g.detail[1].lag, None);
        assert_eq!(g.lag, Some(10));
    }
}
