//! JSON Web Tokens: decode with claim explanations and validity, verify
//! with a secret or public key, and build a signed token for testing.
//! HS256/384/512 through HMAC, RS256/384/512 and ES256 through `ring`.

use hmac::{Hmac, Mac};
use hs_core::Services;
use hs_ui::form;
use hs_ui::theme::Theme;

use crate::crypto::{self, SigAlg, b64_decode, b64url_encode};
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alg {
    HS256,
    HS384,
    HS512,
    RS256,
    RS384,
    RS512,
    ES256,
}

impl Alg {
    pub const ALL: [Alg; 7] = [
        Alg::HS256,
        Alg::HS384,
        Alg::HS512,
        Alg::RS256,
        Alg::RS384,
        Alg::RS512,
        Alg::ES256,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Alg::HS256 => "HS256",
            Alg::HS384 => "HS384",
            Alg::HS512 => "HS512",
            Alg::RS256 => "RS256",
            Alg::RS384 => "RS384",
            Alg::RS512 => "RS512",
            Alg::ES256 => "ES256",
        }
    }
    pub fn parse(name: &str) -> Option<Alg> {
        Alg::ALL.into_iter().find(|a| a.name() == name)
    }
    pub fn is_hmac(self) -> bool {
        matches!(self, Alg::HS256 | Alg::HS384 | Alg::HS512)
    }
    fn sig_alg(self) -> Option<SigAlg> {
        match self {
            Alg::RS256 => Some(SigAlg::RsaSha256),
            Alg::RS384 => Some(SigAlg::RsaSha384),
            Alg::RS512 => Some(SigAlg::RsaSha512),
            Alg::ES256 => Some(SigAlg::EcdsaP256Sha256),
            _ => None,
        }
    }
}

fn hmac_sign(alg: Alg, key: &[u8], data: &[u8]) -> Vec<u8> {
    macro_rules! mac {
        ($h:ty) => {{
            let mut m = Hmac::<$h>::new_from_slice(key).expect("any key length");
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }};
    }
    match alg {
        Alg::HS256 => mac!(sha2::Sha256),
        Alg::HS384 => mac!(sha2::Sha384),
        _ => mac!(sha2::Sha512),
    }
}

/// `header.payload.signature` for the given claims. `key` is the HMAC
/// secret or the private key (PEM/Base64) depending on the algorithm.
pub fn encode(
    alg: Alg,
    header_json: &str,
    payload_json: &str,
    key: &str,
) -> Result<String, String> {
    let mut header: serde_json::Value =
        serde_json::from_str(header_json).map_err(|e| format!("header: {e}"))?;
    // Keep the author's key order (jwt.io-style tokens compare byte for
    // byte) unless the header has to be completed.
    let header_text = if header.get("alg").and_then(|a| a.as_str()) == Some(alg.name())
        && header.get("typ").is_some()
    {
        compact_json(header_json)
    } else {
        header["alg"] = serde_json::Value::String(alg.name().into());
        if header.get("typ").is_none() {
            header["typ"] = serde_json::Value::String("JWT".into());
        }
        serde_json::to_string(&header).unwrap()
    };
    serde_json::from_str::<serde_json::Value>(payload_json).map_err(|e| format!("payload: {e}"))?;
    let signing_input = format!(
        "{}.{}",
        b64url_encode(header_text.as_bytes()),
        b64url_encode(compact_json(payload_json).as_bytes())
    );
    let sig = if alg.is_hmac() {
        hmac_sign(alg, key.as_bytes(), signing_input.as_bytes())
    } else {
        crypto::sign(alg.sig_alg().unwrap(), key, signing_input.as_bytes())?
    };
    Ok(format!("{signing_input}.{}", b64url_encode(&sig)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub header: serde_json::Value,
    pub payload: serde_json::Value,
    pub signing_input: String,
    pub signature: Vec<u8>,
    pub alg: Option<Alg>,
}

pub fn decode(token: &str) -> Result<Decoded, String> {
    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() != 3 {
        return Err("a JWT has three dot-separated parts".into());
    }
    let header: serde_json::Value = serde_json::from_slice(&b64_decode(parts[0])?)
        .map_err(|e| format!("header is not JSON: {e}"))?;
    let payload: serde_json::Value = serde_json::from_slice(&b64_decode(parts[1])?)
        .map_err(|e| format!("payload is not JSON: {e}"))?;
    let signature = if parts[2].is_empty() {
        Vec::new()
    } else {
        b64_decode(parts[2])?
    };
    let alg = header
        .get("alg")
        .and_then(|a| a.as_str())
        .and_then(Alg::parse);
    Ok(Decoded {
        header,
        payload,
        signing_input: format!("{}.{}", parts[0], parts[1]),
        signature,
        alg,
    })
}

/// Verify the signature with a secret (HS*) or a public key (RS*/ES*).
pub fn verify(d: &Decoded, key: &str) -> Result<bool, String> {
    let Some(alg) = d.alg else {
        return Err("unsupported or missing alg".into());
    };
    if alg.is_hmac() {
        let expected = hmac_sign(alg, key.as_bytes(), d.signing_input.as_bytes());
        return Ok(constant_time_eq(&expected, &d.signature));
    }
    crypto::verify(
        alg.sig_alg().unwrap(),
        key,
        d.signing_input.as_bytes(),
        &d.signature,
    )
}

/// JSON with the whitespace outside strings removed, key order untouched.
pub fn compact_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_str = false;
    let mut escaped = false;
    for c in text.chars() {
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Human reading of the registered claims: what they mean, times as local
/// dates, and whether the token is currently valid by time.
pub fn explain(payload: &serde_json::Value) -> Vec<(String, String)> {
    let now = chrono::Utc::now().timestamp();
    let mut out = Vec::new();
    let time = |v: &serde_json::Value| -> Option<String> {
        let t = v.as_i64()?;
        let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(t, 0)?;
        let local = dt.with_timezone(&chrono::Local);
        let rel = t - now;
        let rel_text = if rel.abs() < 90 {
            "now".to_string()
        } else if rel < 0 {
            format!("{} ago", human_duration(-rel))
        } else {
            format!("in {}", human_duration(rel))
        };
        Some(format!(
            "{} ({rel_text})",
            local.format("%Y-%m-%d %H:%M:%S %Z")
        ))
    };
    let Some(obj) = payload.as_object() else {
        return out;
    };
    for (k, v) in obj {
        let text = match k.as_str() {
            "exp" => time(v).map(|t| {
                if v.as_i64().is_some_and(|e| e < now) {
                    format!("Expires {t} — EXPIRED")
                } else {
                    format!("Expires {t}")
                }
            }),
            "iat" => time(v).map(|t| format!("Issued at {t}")),
            "nbf" => time(v).map(|t| {
                if v.as_i64().is_some_and(|n| n > now) {
                    format!("Not valid before {t} — NOT YET VALID")
                } else {
                    format!("Not before {t}")
                }
            }),
            "auth_time" => time(v).map(|t| format!("Authenticated at {t}")),
            "iss" => Some(format!("Issuer: {}", v)),
            "sub" => Some(format!("Subject: {}", v)),
            "aud" => Some(format!("Audience: {}", v)),
            "jti" => Some(format!("Token id: {}", v)),
            "scope" | "scp" => Some(format!("Scopes: {}", v)),
            "azp" => Some(format!("Authorized party: {}", v)),
            _ => None,
        };
        if let Some(t) = text {
            out.push((k.clone(), t));
        }
    }
    out
}

fn human_duration(secs: i64) -> String {
    if secs < 3600 {
        format!("{} min", secs / 60)
    } else if secs < 86_400 {
        format!("{} h {} min", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{} d {} h", secs / 86_400, (secs % 86_400) / 3600)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Decode,
    Encode,
}

pub struct State {
    pub mode: Mode,
    pub token: String,
    pub key: String,
    pub alg: Alg,
    pub header: String,
    pub payload: String,
    pub encoded: Option<Result<String, String>>,
}

impl Default for State {
    fn default() -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            mode: Mode::Decode,
            token: String::new(),
            key: String::new(),
            alg: Alg::HS256,
            header: "{\n  \"alg\": \"HS256\",\n  \"typ\": \"JWT\"\n}".into(),
            payload: format!(
                "{{\n  \"sub\": \"1234567890\",\n  \"name\": \"Jane Doe\",\n  \"iat\": {now},\n  \"exp\": {}\n}}",
                now + 3600
            ),
            encoded: None,
        }
    }
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::page_title(ui, theme, &s.t("dt-jwt"), Some(&s.t("dt-jwt-sub")));
    ui.horizontal(|ui| {
        ui.selectable_value(&mut st.mode, Mode::Decode, s.t("dt-jwt-decode"));
        ui.selectable_value(&mut st.mode, Mode::Encode, s.t("dt-jwt-encode"));
    });
    ui.add_space(8.0);
    match st.mode {
        Mode::Decode => decode_ui(st, ui, s, theme),
        Mode::Encode => encode_ui(st, ui, s, theme),
    }
}

fn key_field(st: &mut State, ui: &mut egui::Ui, s: &Services, hmac: bool) {
    if hmac {
        ui.add(
            egui::TextEdit::singleline(&mut st.key)
                .hint_text(s.t("dt-jwt-secret-hint"))
                .password(true)
                .desired_width(f32::INFINITY),
        );
    } else {
        util::input(ui, &mut st.key, &s.t("dt-jwt-key-hint"), 4);
    }
}

fn decode_ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::section(ui, theme, &s.t("dt-jwt-token"), |ui| {
        util::input(ui, &mut st.token, &s.t("dt-jwt-token-hint"), 4);
    });
    if st.token.trim().is_empty() {
        return;
    }
    let decoded = match decode(&st.token) {
        Ok(d) => d,
        Err(e) => {
            util::error(ui, theme, &e);
            return;
        }
    };
    form::section(ui, theme, &s.t("dt-jwt-header"), |ui| {
        util::output(
            ui,
            s,
            theme,
            "JSON",
            &serde_json::to_string_pretty(&decoded.header).unwrap_or_default(),
            4,
        );
    });
    form::section(ui, theme, &s.t("dt-jwt-payload"), |ui| {
        util::output(
            ui,
            s,
            theme,
            "JSON",
            &serde_json::to_string_pretty(&decoded.payload).unwrap_or_default(),
            8,
        );
        let claims = explain(&decoded.payload);
        if !claims.is_empty() {
            ui.add_space(6.0);
            egui::Grid::new("jwt-claims")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    for (k, text) in claims {
                        ui.label(egui::RichText::new(k).monospace().strong());
                        let bad = text.contains("EXPIRED") || text.contains("NOT YET VALID");
                        ui.colored_label(if bad { theme.error } else { theme.text }, text);
                        ui.end_row();
                    }
                });
        }
    });
    form::section(ui, theme, &s.t("dt-jwt-signature"), |ui| {
        match decoded.alg {
            Some(alg) => {
                ui.label(s.i18n.t_args("dt-jwt-alg-is", &[("alg", alg.name())]));
                key_field(st, ui, s, alg.is_hmac());
                if !st.key.trim().is_empty() {
                    match verify(&decoded, &st.key) {
                        Ok(true) => util::ok(ui, theme, &s.t("dt-jwt-valid")),
                        Ok(false) => util::error(ui, theme, &s.t("dt-jwt-invalid")),
                        Err(e) => util::error(ui, theme, &e),
                    }
                }
            }
            None => {
                form::hint(ui, theme, &s.t("dt-jwt-alg-unsupported"));
            }
        }
    });
}

fn encode_ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::section(ui, theme, &s.t("dt-jwt-encode"), |ui| {
        ui.horizontal(|ui| {
            ui.label(s.t("dt-jwt-algorithm"));
            egui::ComboBox::from_id_salt("jwt-alg")
                .selected_text(st.alg.name())
                .show_ui(ui, |ui| {
                    for a in Alg::ALL {
                        ui.selectable_value(&mut st.alg, a, a.name());
                    }
                });
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(s.t("dt-jwt-header"))
                .strong()
                .color(theme.text_muted),
        );
        util::input(ui, &mut st.header, "", 3);
        ui.label(
            egui::RichText::new(s.t("dt-jwt-payload"))
                .strong()
                .color(theme.text_muted),
        );
        util::input(ui, &mut st.payload, "", 6);
        ui.label(
            egui::RichText::new(if st.alg.is_hmac() {
                s.t("dt-jwt-secret")
            } else {
                s.t("dt-jwt-private-key")
            })
            .strong()
            .color(theme.text_muted),
        );
        let hmac = st.alg.is_hmac();
        key_field(st, ui, s, hmac);
        ui.add_space(6.0);
        if form::primary_button(ui, theme, &s.t("dt-jwt-sign")).clicked() {
            st.encoded = Some(encode(st.alg, &st.header, &st.payload, &st.key));
        }
        ui.add_space(6.0);
        util::result(ui, s, theme, &s.t("dt-jwt-token"), &st.encoded, 4);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs256_matches_the_jwt_io_example() {
        // The jwt.io sample: secret "your-256-bit-secret".
        let token = encode(
            Alg::HS256,
            r#"{"alg":"HS256","typ":"JWT"}"#,
            r#"{"sub":"1234567890","name":"John Doe","iat":1516239022}"#,
            "your-256-bit-secret",
        )
        .unwrap();
        assert_eq!(
            token,
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        );
        let d = decode(&token).unwrap();
        assert_eq!(d.alg, Some(Alg::HS256));
        assert!(verify(&d, "your-256-bit-secret").unwrap());
        assert!(!verify(&d, "wrong").unwrap());
        assert_eq!(d.payload["name"], "John Doe");
    }

    #[test]
    fn compact_json_keeps_strings_and_order() {
        assert_eq!(
            compact_json("{ \"b\": \"x y\", \n \"a\": [1, 2] }"),
            "{\"b\":\"x y\",\"a\":[1,2]}"
        );
        assert_eq!(compact_json("\"a \\\" b\""), "\"a \\\" b\"");
    }

    #[test]
    fn explain_marks_expired_tokens() {
        let payload = serde_json::json!({"exp": 1_000_000, "iss": "me"});
        let ex = explain(&payload);
        assert!(ex.iter().any(|(k, t)| k == "exp" && t.contains("EXPIRED")));
        assert!(ex.iter().any(|(k, _)| k == "iss"));
    }
}
