//! Network tools (docs/tools/devtools.md §2 Network): DNS lookups, a TCP
//! connect check, an HTTP check and a TLS certificate viewer. Every
//! lookup runs on the runtime and writes into a slot the UI reads at
//! frame start; nothing here blocks the UI thread (ADR-013).
//!
//! These reach the network, so each one is started by a button the user
//! presses - nothing is looked up as they type.

use std::sync::Arc;

use hs_core::Services;
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

/// What a finished lookup leaves behind.
type Slot = Arc<Mutex<Option<Result<Outcome, String>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Check {
    #[default]
    Dns,
    Tcp,
    Http,
    Tls,
}

impl Check {
    pub const ALL: [Check; 4] = [Check::Dns, Check::Tcp, Check::Http, Check::Tls];

    pub fn label_key(self) -> &'static str {
        match self {
            Check::Dns => "dt-net-dns",
            Check::Tcp => "dt-net-tcp",
            Check::Http => "dt-net-http",
            Check::Tls => "dt-net-tls",
        }
    }
}

/// DNS record types the lookup offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Record {
    #[default]
    A,
    Aaaa,
    Cname,
    Mx,
    Txt,
    Ns,
}

impl Record {
    pub const ALL: [Record; 6] = [
        Record::A,
        Record::Aaaa,
        Record::Cname,
        Record::Mx,
        Record::Txt,
        Record::Ns,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Record::A => "A",
            Record::Aaaa => "AAAA",
            Record::Cname => "CNAME",
            Record::Mx => "MX",
            Record::Txt => "TXT",
            Record::Ns => "NS",
        }
    }
}

/// What a check found: rows of (label, value) plus how long it took.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    pub rows: Vec<(String, String)>,
    pub millis: u128,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct State {
    pub check: Check,
    pub host: String,
    pub record: Record,
    pub port: String,
    pub url: String,
    /// Not serialised: a result belongs to the run that produced it.
    #[serde(skip)]
    slot: Option<Slot>,
    #[serde(skip)]
    pub busy: bool,
    #[serde(skip)]
    pub outcome: Option<Result<Outcome, String>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            check: Check::default(),
            host: String::new(),
            record: Record::default(),
            port: "443".into(),
            url: String::new(),
            slot: None,
            busy: false,
            outcome: None,
        }
    }
}

/// `host:port` for the TCP and TLS checks, with the port filled in.
fn addr(host: &str, port: &str) -> Result<(String, u16), String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("no host".into());
    }
    let port: u16 = port.trim().parse().map_err(|_| "bad port".to_string())?;
    Ok((host.to_string(), port))
}

/// Look `name` up and return the answers as rows. One generic lookup
/// serves every record type: the typed helpers return different shapes,
/// and the answer is printed the same way whichever it is.
async fn dns(name: String, record: Record) -> Result<Vec<(String, String)>, String> {
    use hickory_resolver::Resolver;
    use hickory_resolver::proto::rr::RecordType;
    let resolver = Resolver::builder_tokio()
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    let kind = match record {
        Record::A => RecordType::A,
        Record::Aaaa => RecordType::AAAA,
        Record::Cname => RecordType::CNAME,
        Record::Mx => RecordType::MX,
        Record::Txt => RecordType::TXT,
        Record::Ns => RecordType::NS,
    };
    let answer = resolver
        .lookup(name.trim(), kind)
        .await
        .map_err(|e| e.to_string())?;
    Ok(answer
        .answers()
        .iter()
        .map(|r| (r.record_type().to_string(), r.data.to_string()))
        .collect())
}

/// Open a TCP connection and report how long it took.
async fn tcp(host: String, port: u16) -> Result<Vec<(String, String)>, String> {
    let started = std::time::Instant::now();
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map_err(|_| "timed out after 8 s".to_string())?
    .map_err(|e| e.to_string())?;
    let peer = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    Ok(vec![
        ("open".into(), format!("{host}:{port}")),
        ("peer".into(), peer),
        (
            "connect".into(),
            format!("{:.1} ms", started.elapsed().as_secs_f64() * 1000.0),
        ),
    ])
}

/// A GET with the status, timing and the headers worth seeing.
async fn http(url: String) -> Result<Vec<(String, String)>, String> {
    let url = if url.contains("://") {
        url
    } else {
        format!("https://{url}")
    };
    let client = hs_core::ai::http_client();
    let started = std::time::Instant::now();
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let elapsed = started.elapsed();
    let mut rows = vec![
        ("status".into(), resp.status().to_string()),
        ("version".into(), format!("{:?}", resp.version())),
        (
            "time".into(),
            format!("{:.0} ms", elapsed.as_secs_f64() * 1000.0),
        ),
        ("url".into(), resp.url().to_string()),
    ];
    for name in [
        "server",
        "content-type",
        "content-length",
        "location",
        "cache-control",
        "strict-transport-security",
    ] {
        if let Some(v) = resp.headers().get(name).and_then(|v| v.to_str().ok()) {
            rows.push((name.to_string(), v.to_string()));
        }
    }
    Ok(rows)
}

/// Pick **ring** as the process-wide rustls provider.
///
/// Two providers are compiled into this process (reqwest brings
/// aws-lc-rs, the rest of the app uses ring) and rustls then refuses to
/// choose: the first handshake panics with "could not automatically
/// determine the process-level CryptoProvider", which here left the
/// check spinning for ever. The SQL driver and the Kubernetes client
/// already learned this (docs/GOTCHAS.md).
fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    });
}

/// Connect with TLS and read the certificate the server presented.
async fn tls(host: String, port: u16) -> Result<Vec<(String, String)>, String> {
    use tokio_rustls::TlsConnector;
    use tokio_rustls::rustls::pki_types::ServerName;
    install_crypto_provider();
    let verifier = rustls_platform_verifier::BuilderVerifierExt::with_platform_verifier(
        tokio_rustls::rustls::ClientConfig::builder(),
    )
    .map_err(|e| e.to_string())?;
    let config = verifier.with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map_err(|_| "timed out after 10 s".to_string())?
    .map_err(|e| e.to_string())?;
    let name = ServerName::try_from(host.clone()).map_err(|e| e.to_string())?;
    let tls = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        connector.connect(name, stream),
    )
    .await
    .map_err(|_| "the handshake timed out after 10 s".to_string())?
    .map_err(|e| e.to_string())?;
    let (_, conn) = tls.get_ref();
    let mut rows = vec![
        (
            "protocol".into(),
            conn.protocol_version()
                .map(|v| format!("{v:?}"))
                .unwrap_or_default(),
        ),
        (
            "cipher".into(),
            conn.negotiated_cipher_suite()
                .map(|c| format!("{:?}", c.suite()))
                .unwrap_or_default(),
        ),
    ];
    let Some(chain) = conn.peer_certificates() else {
        rows.push(("certificates".into(), "none presented".into()));
        return Ok(rows);
    };
    rows.push(("chain".into(), format!("{} certificate(s)", chain.len())));
    if let Some(first) = chain.first() {
        rows.extend(describe_cert(first.as_ref()));
    }
    Ok(rows)
}

/// The fields of the leaf certificate the viewer shows.
fn describe_cert(der: &[u8]) -> Vec<(String, String)> {
    use x509_cert::der::Decode;
    let Ok(cert) = x509_cert::Certificate::from_der(der) else {
        return vec![("certificate".into(), "could not be parsed".into())];
    };
    let tbs = &cert.tbs_certificate;
    let mut rows = vec![
        ("subject".into(), tbs.subject.to_string()),
        ("issuer".into(), tbs.issuer.to_string()),
        ("serial".into(), tbs.serial_number.to_string()),
        ("not before".into(), tbs.validity.not_before.to_string()),
        ("not after".into(), tbs.validity.not_after.to_string()),
    ];
    // Subject alternative names: the field people actually check.
    if let Some(exts) = &tbs.extensions {
        use x509_cert::der::oid::AssociatedOid;
        for ext in exts.iter() {
            if ext.extn_id == x509_cert::ext::pkix::SubjectAltName::OID
                && let Ok(san) =
                    x509_cert::ext::pkix::SubjectAltName::from_der(ext.extn_value.as_bytes())
            {
                let names: Vec<String> = san
                    .0
                    .iter()
                    .map(|n| match n {
                        x509_cert::ext::pkix::name::GeneralName::DnsName(d) => d.to_string(),
                        other => format!("{other:?}"),
                    })
                    .collect();
                rows.push(("alt names".into(), names.join(", ")));
            }
        }
    }
    rows
}

/// Start the chosen check.
pub fn run(state: &mut State, services: &Services) {
    let slot: Slot = Arc::new(Mutex::new(None));
    state.slot = Some(slot.clone());
    state.busy = true;
    state.outcome = None;
    let waker = services.runtime.waker();
    let (check, host, record, port, url) = (
        state.check,
        state.host.clone(),
        state.record,
        state.port.clone(),
        state.url.clone(),
    );
    services.runtime.spawn(async move {
        let started = std::time::Instant::now();
        let rows = match check {
            Check::Dns => dns(host, record).await,
            Check::Tcp => match addr(&host, &port) {
                Ok((h, p)) => tcp(h, p).await,
                Err(e) => Err(e),
            },
            Check::Http => http(url).await,
            Check::Tls => match addr(&host, &port) {
                Ok((h, p)) => tls(h, p).await,
                Err(e) => Err(e),
            },
        };
        *slot.lock() = Some(rows.map(|rows| Outcome {
            rows,
            millis: started.elapsed().as_millis(),
        }));
        waker.request_repaint();
    });
}

pub fn ui(state: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    // Drain a finished run.
    if let Some(slot) = state.slot.clone()
        && let Some(done) = slot.lock().take()
    {
        state.outcome = Some(done);
        state.busy = false;
        state.slot = None;
    }
    form::section(ui, theme, &s.t("dt-net"), |ui| {
        form::hint(ui, theme, &s.t("dt-net-hint"));
        ui.horizontal_wrapped(|ui| {
            for c in Check::ALL {
                if ui
                    .add(egui::Button::selectable(
                        state.check == c,
                        s.t(c.label_key()),
                    ))
                    .clicked()
                {
                    state.check = c;
                    state.outcome = None;
                }
            }
        });
        ui.add_space(6.0);
        let mut go = false;
        match state.check {
            Check::Http => {
                ui.horizontal(|ui| {
                    ui.label(s.t("dt-net-url"));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut state.url)
                            .hint_text("https://example.com/health")
                            .desired_width(360.0),
                    );
                    go |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.label(s.t("dt-net-host"));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut state.host)
                            .hint_text("example.com")
                            .desired_width(240.0),
                    );
                    go |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if state.check == Check::Dns {
                        for rec in Record::ALL {
                            if ui
                                .add(egui::Button::selectable(state.record == rec, rec.label()))
                                .clicked()
                            {
                                state.record = rec;
                            }
                        }
                    } else {
                        ui.label(s.t("dt-net-port"));
                        ui.add(egui::TextEdit::singleline(&mut state.port).desired_width(70.0));
                    }
                });
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let ready = match state.check {
                Check::Http => !state.url.trim().is_empty(),
                _ => !state.host.trim().is_empty(),
            };
            if ui
                .add_enabled(
                    ready && !state.busy,
                    egui::Button::new(format!("{} {}", ph::MAGNIFYING_GLASS, s.t("dt-net-run"))),
                )
                .clicked()
            {
                go = true;
            }
            if state.busy {
                ui.spinner();
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
            }
        });
        if go && !state.busy {
            run(state, s);
        }
        match &state.outcome {
            None => {}
            Some(Err(e)) => {
                ui.add_space(6.0);
                form::note(ui, theme, ph::WARNING, e);
            }
            Some(Ok(outcome)) => {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        s.i18n
                            .t_args("dt-net-took", &[("ms", &outcome.millis.to_string())]),
                    )
                    .small()
                    .color(theme.text_muted),
                );
                if outcome.rows.is_empty() {
                    form::hint(ui, theme, &s.t("dt-net-empty"));
                }
                egui::Grid::new("dt-net-rows")
                    .num_columns(2)
                    .striped(true)
                    .spacing([16.0, 3.0])
                    .show(ui, |ui| {
                        for (k, v) in &outcome.rows {
                            ui.label(egui::RichText::new(k).small().color(theme.text_muted));
                            ui.add(
                                egui::Label::new(egui::RichText::new(v).small().monospace()).wrap(),
                            );
                            ui.end_row();
                        }
                    });
                ui.add_space(4.0);
                if ui.button(s.t("dt-copy")).clicked() {
                    let text = outcome
                        .rows
                        .iter()
                        .map(|(k, v)| format!("{k}: {v}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.ctx().copy_text(text);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_needs_a_host_and_a_number() {
        assert_eq!(addr("example.com", "443"), Ok(("example.com".into(), 443)));
        assert_eq!(addr("  h  ", " 80 "), Ok(("h".into(), 80)));
        assert!(addr("", "443").is_err());
        assert!(addr("h", "nope").is_err());
        assert!(addr("h", "99999").is_err());
    }

    #[test]
    fn every_check_and_record_is_named() {
        for c in Check::ALL {
            assert!(c.label_key().starts_with("dt-net-"));
        }
        for r in Record::ALL {
            assert!(!r.label().is_empty());
        }
        assert_eq!(Record::Mx.label(), "MX");
    }

    #[test]
    fn a_certificate_that_is_not_one_says_so() {
        let rows = describe_cert(b"not a certificate");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.contains("could not be parsed"));
    }
}
