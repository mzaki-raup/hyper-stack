//! The gateway's secure message flow: a request is signed with the sender's
//! RSA private key (`SHA256withRSA`) and encrypted with AES-256-GCM under
//! a shared key derived as `SHA-256(sharedKeyUuid)`; the receiver decrypts
//! with the shared key and verifies the signature with the sender's public
//! key. The response goes back the same way with the roles swapped.
//!
//! Flow 1 (device → server): `seal` with the device private key, `open`
//! with the device public key. Flow 2 (server → device): `seal` with the
//! target (server) private key, `open` with the target public key.

use hs_core::Services;
use hs_ui::form;
use hs_ui::theme::Theme;
use serde::{Deserialize, Serialize};

use crate::crypto::{
    self, SigAlg, aes_gcm_decrypt, aes_gcm_encrypt, b64_decode, b64_encode, key_from_uuid,
    random_bytes,
};
use crate::util;

/// Sign `plain` with `private_key`, encrypt it under the shared key.
/// Returns `(encrypted_msg, signature_str)`, both Base64.
pub fn seal(private_key: &str, shared_uuid: &str, plain: &str) -> Result<(String, String), String> {
    if shared_uuid.trim().is_empty() {
        return Err("the shared key (device UUID) is empty".into());
    }
    let signature = crypto::sign(SigAlg::RsaSha256, private_key, plain.as_bytes())?;
    let key = key_from_uuid(shared_uuid.trim());
    let mut iv = [0u8; crypto::GCM_IV_LEN];
    iv.copy_from_slice(&random_bytes(crypto::GCM_IV_LEN));
    let encrypted = aes_gcm_encrypt(&key, &iv, plain.as_bytes())?;
    Ok((b64_encode(&encrypted), b64_encode(&signature)))
}

/// Decrypt `encrypted_msg` under the shared key and verify `signature_str`
/// with `public_key`. Returns the plaintext and whether the signature held.
pub fn open(
    public_key: &str,
    shared_uuid: &str,
    encrypted_msg: &str,
    signature_str: &str,
) -> Result<(String, bool), String> {
    if shared_uuid.trim().is_empty() {
        return Err("the shared key (device UUID) is empty".into());
    }
    let key = key_from_uuid(shared_uuid.trim());
    let plain = aes_gcm_decrypt(&key, &b64_decode(encrypted_msg)?)?;
    let plain =
        String::from_utf8(plain).map_err(|_| "decrypted bytes are not UTF-8".to_string())?;
    let valid = if signature_str.trim().is_empty() {
        false
    } else {
        crypto::verify(
            SigAlg::RsaSha256,
            public_key,
            plain.as_bytes(),
            &b64_decode(signature_str)?,
        )?
    };
    Ok((plain, valid))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Flow {
    #[default]
    Request,
    Response,
}

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct State {
    pub device_private: String,
    pub device_public: String,
    pub target_private: String,
    pub target_public: String,
    pub device_id: String,
    pub shared_key: String,
    pub flow: Flow,
    pub plain: String,
    #[serde(skip)]
    pub sealed: Option<Result<String, String>>,
    pub payload_in: String,
    #[serde(skip)]
    pub opened: Option<Result<String, String>>,
}

impl State {
    pub fn for_save(&self) -> State {
        let mut c = self.clone();
        c.device_private = crate::keep_reference(&c.device_private);
        c.target_private = crate::keep_reference(&c.target_private);
        c.shared_key = crate::keep_reference(&c.shared_key);
        c
    }
}

fn payload_json(device_id: &str, plain: &str, encrypted: &str, signature: &str) -> String {
    let uri = serde_json::from_str::<serde_json::Value>(plain)
        .ok()
        .and_then(|v| v.get("uri").and_then(|u| u.as_str()).map(str::to_string));
    let mut obj = serde_json::Map::new();
    obj.insert("deviceId".into(), device_id.into());
    if let Some(uri) = uri {
        obj.insert("bucketUri".into(), uri.into());
    }
    obj.insert("encryptedMsg".into(), encrypted.into());
    obj.insert("signatureStr".into(), signature.into());
    serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap_or_default()
}

/// Pull `encryptedMsg` / `signatureStr` out of a pasted payload, or treat
/// the text as the encrypted message alone.
fn parse_payload(text: &str) -> (String, String) {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        return (get("encryptedMsg"), get("signatureStr"));
    }
    (text.trim().to_string(), String::new())
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::page_title(ui, theme, &s.t("dt-pki"), Some(&s.t("dt-pki-sub")));
    form::section(ui, theme, &s.t("dt-pki-keys"), |ui| {
        form::hint(ui, theme, &s.t("dt-pki-keys-hint"));
        form::field_grid(ui, "pki-ids", |ui| {
            form::label(ui, &s.t("dt-pki-device-id"));
            util::line(ui, s, theme, &mut st.device_id, "");
            ui.end_row();
            form::label(ui, &s.t("dt-pki-shared-key"));
            util::secret_line(
                ui,
                s,
                theme,
                &mut st.shared_key,
                &s.t("dt-pki-shared-key-hint"),
            );
            ui.end_row();
            form::label(ui, &s.t("dt-pki-derived-key"));
            if st.shared_key.trim().is_empty() {
                ui.label("—");
            } else {
                match util::expand(s, st.shared_key.trim()) {
                    Ok(v) => {
                        ui.monospace(b64_encode(&key_from_uuid(v.trim())));
                    }
                    Err(e) => {
                        ui.colored_label(theme.error, e);
                    }
                }
            }
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.columns(2, |cols| {
            cols[0].label(egui::RichText::new(s.t("dt-pki-device-private")).strong());
            util::secret_input(
                &mut cols[0],
                s,
                theme,
                &mut st.device_private,
                "PKCS#8 / PEM / Base64",
                4,
            );
            cols[0].label(egui::RichText::new(s.t("dt-pki-device-public")).strong());
            util::input(
                &mut cols[0],
                s,
                theme,
                &mut st.device_public,
                "X.509 / PEM / Base64",
                4,
            );
            cols[1].label(egui::RichText::new(s.t("dt-pki-target-private")).strong());
            util::secret_input(
                &mut cols[1],
                s,
                theme,
                &mut st.target_private,
                "PKCS#8 / PEM / Base64",
                4,
            );
            cols[1].label(egui::RichText::new(s.t("dt-pki-target-public")).strong());
            util::input(
                &mut cols[1],
                s,
                theme,
                &mut st.target_public,
                "X.509 / PEM / Base64",
                4,
            );
        });
    });
    ui.horizontal(|ui| {
        ui.selectable_value(&mut st.flow, Flow::Request, s.t("dt-pki-flow-request"));
        ui.selectable_value(&mut st.flow, Flow::Response, s.t("dt-pki-flow-response"));
    });
    let (seal_key, open_key, seal_title, open_title) = match st.flow {
        Flow::Request => (
            st.device_private.clone(),
            st.device_public.clone(),
            s.t("dt-pki-seal-request"),
            s.t("dt-pki-open-request"),
        ),
        Flow::Response => (
            st.target_private.clone(),
            st.target_public.clone(),
            s.t("dt-pki-seal-response"),
            s.t("dt-pki-open-response"),
        ),
    };
    form::section(ui, theme, &seal_title, |ui| {
        util::input(ui, s, theme, &mut st.plain, &s.t("dt-pki-plain-hint"), 5);
        if form::primary_button(ui, theme, &s.t("dt-pki-seal")).clicked() {
            st.sealed = Some(
                util::expand(s, &seal_key)
                    .and_then(|k| Ok((k, util::expand(s, &st.shared_key)?)))
                    .and_then(|(k, shared)| {
                        let plain = util::expand(s, &st.plain)?;
                        let device = util::expand(s, &st.device_id)?;
                        seal(&k, &shared, &plain)
                            .map(|(enc, sig)| payload_json(&device, &plain, &enc, &sig))
                    }),
            );
        }
        util::result(ui, s, theme, &s.t("dt-pki-sealed"), &st.sealed, 8);
    });
    form::section(ui, theme, &open_title, |ui| {
        util::input(
            ui,
            s,
            theme,
            &mut st.payload_in,
            &s.t("dt-pki-payload-hint"),
            5,
        );
        if form::primary_button(ui, theme, &s.t("dt-pki-open")).clicked() {
            let (enc, sig) = parse_payload(&st.payload_in);
            let open_key = util::expand(s, &open_key).unwrap_or_default();
            let shared = util::expand(s, &st.shared_key).unwrap_or_default();
            st.opened = Some(open(&open_key, &shared, &enc, &sig).map(|(plain, valid)| {
                let mut obj = serde_json::Map::new();
                obj.insert(
                    "decryptedMsg".into(),
                    serde_json::from_str(&plain).unwrap_or(serde_json::Value::String(plain)),
                );
                obj.insert("signatureValid".into(), valid.into());
                serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap_or_default()
            }));
        }
        if let Some(Ok(text)) = &st.opened {
            if text.contains("\"signatureValid\": true") {
                util::ok(ui, theme, &s.t("dt-pki-signature-ok"));
            } else {
                util::error(ui, theme, &s.t("dt-pki-signature-bad"));
            }
        }
        util::result(ui, s, theme, &s.t("dt-pki-opened"), &st.opened, 8);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_parsing_and_shape() {
        let p = payload_json("dev-1", r#"{"uri":"/v1/x","method":"GET"}"#, "ENC", "SIG");
        let v: serde_json::Value = serde_json::from_str(&p).unwrap();
        assert_eq!(v["bucketUri"], "/v1/x");
        assert_eq!(v["deviceId"], "dev-1");
        assert_eq!(parse_payload(&p), ("ENC".into(), "SIG".into()));
        assert_eq!(parse_payload("raw"), ("raw".into(), String::new()));
    }

    #[test]
    fn open_without_a_signature_decrypts_but_reports_invalid() {
        // Encrypt directly (no RSA key needed) and open with an empty signature.
        let key = key_from_uuid("uuid-1");
        let iv = [1u8; 12];
        let enc = b64_encode(&aes_gcm_encrypt(&key, &iv, b"{\"ok\":true}").unwrap());
        let (plain, valid) = open("", "uuid-1", &enc, "").unwrap();
        assert_eq!(plain, "{\"ok\":true}");
        assert!(!valid);
        assert!(open("", "other-uuid", &enc, "").is_err());
    }
}
