//! Firebase tokens from a service account: a custom token for a uid
//! (offline), an OAuth 2.0 access token for the Google APIs (one request to
//! oauth2.googleapis.com) and an ID token by signing in with the custom
//! token (one request to identitytoolkit.googleapis.com). Network calls
//! happen only when their button is pressed.

use std::sync::Arc;

use hs_core::Services;
use hs_ui::form;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use crate::crypto::b64url_encode;
use crate::jwt::{self, Alg};
use crate::util;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServiceAccount {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
    pub token_uri: String,
}

pub fn parse_service_account(json: &str) -> Result<ServiceAccount, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let get = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("service account JSON has no \"{k}\""))
    };
    Ok(ServiceAccount {
        project_id: get("project_id").unwrap_or_default(),
        client_email: get("client_email")?,
        private_key: get("private_key")?,
        token_uri: get("token_uri")
            .unwrap_or_else(|_| "https://oauth2.googleapis.com/token".into()),
    })
}

const CUSTOM_TOKEN_AUD: &str =
    "https://identitytoolkit.googleapis.com/google.identity.identitytoolkit.v1.IdentityToolkit";

/// A Firebase custom token (RS256 JWT, at most one hour) for `uid`.
pub fn custom_token(
    sa: &ServiceAccount,
    uid: &str,
    claims: Option<&serde_json::Value>,
    ttl_secs: i64,
) -> Result<String, String> {
    if uid.trim().is_empty() {
        return Err("a uid is needed".into());
    }
    let now = chrono::Utc::now().timestamp();
    let mut payload = serde_json::json!({
        "iss": sa.client_email,
        "sub": sa.client_email,
        "aud": CUSTOM_TOKEN_AUD,
        "iat": now,
        "exp": now + ttl_secs.clamp(1, 3600),
        "uid": uid.trim(),
    });
    if let Some(c) = claims
        && c.as_object().is_some_and(|o| !o.is_empty())
    {
        payload["claims"] = c.clone();
    }
    jwt::encode(
        Alg::RS256,
        r#"{"alg":"RS256","typ":"JWT"}"#,
        &payload.to_string(),
        &sa.private_key,
    )
}

/// The signed assertion for the JWT bearer grant.
pub fn access_token_assertion(sa: &ServiceAccount, scopes: &str) -> Result<String, String> {
    let now = chrono::Utc::now().timestamp();
    let scope = scopes
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if scope.is_empty() {
        return Err("at least one scope is needed".into());
    }
    let payload = serde_json::json!({
        "iss": sa.client_email,
        "scope": scope,
        "aud": sa.token_uri,
        "iat": now,
        "exp": now + 3600,
    });
    jwt::encode(
        Alg::RS256,
        r#"{"alg":"RS256","typ":"JWT"}"#,
        &payload.to_string(),
        &sa.private_key,
    )
}

type Pending = Arc<Mutex<Option<Result<String, String>>>>;

pub struct State {
    pub service_json: String,
    pub uid: String,
    pub claims: String,
    pub ttl_minutes: i64,
    pub custom: Option<Result<String, String>>,
    pub scopes: String,
    pub access: Option<Result<String, String>>,
    pub access_pending: Option<Pending>,
    pub web_api_key: String,
    pub id_token: Option<Result<String, String>>,
    pub id_pending: Option<Pending>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            service_json: String::new(),
            uid: String::new(),
            claims: "{}".into(),
            ttl_minutes: 60,
            custom: None,
            scopes: "https://www.googleapis.com/auth/cloud-platform".into(),
            access: None,
            access_pending: None,
            web_api_key: String::new(),
            id_token: None,
            id_pending: None,
        }
    }
}

fn poll(slot: &mut Option<Pending>, out: &mut Option<Result<String, String>>) {
    let done = slot.as_ref().and_then(|p| p.lock().take());
    if let Some(r) = done {
        *out = Some(r);
        *slot = None;
    }
}

/// `application/x-www-form-urlencoded` body (reqwest's `form` feature is
/// not enabled in this workspace).
fn form_body(fields: &[(String, String)]) -> String {
    fn enc(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char)
                }
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn post_form(s: &Services, url: String, form: Vec<(String, String)>) -> Pending {
    let pending: Pending = Arc::new(Mutex::new(None));
    let out = pending.clone();
    let waker = s.runtime.waker();
    s.runtime.spawn(async move {
        let client = reqwest::Client::new();
        let result = async {
            let resp = client
                .post(&url)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(form_body(&form))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let status = resp.status();
            let text = resp.text().await.map_err(|e| e.to_string())?;
            if status.is_success() {
                Ok(util::pretty_json(&text))
            } else {
                Err(format!("{status}: {}", util::pretty_json(&text)))
            }
        }
        .await;
        *out.lock() = Some(result);
        waker.request_repaint();
    });
    pending
}

fn post_json(s: &Services, url: String, body: serde_json::Value) -> Pending {
    let pending: Pending = Arc::new(Mutex::new(None));
    let out = pending.clone();
    let waker = s.runtime.waker();
    s.runtime.spawn(async move {
        let client = reqwest::Client::new();
        let result = async {
            let resp = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let status = resp.status();
            let text = resp.text().await.map_err(|e| e.to_string())?;
            if status.is_success() {
                Ok(util::pretty_json(&text))
            } else {
                Err(format!("{status}: {}", util::pretty_json(&text)))
            }
        }
        .await;
        *out.lock() = Some(result);
        waker.request_repaint();
    });
    pending
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    poll(&mut st.access_pending, &mut st.access);
    poll(&mut st.id_pending, &mut st.id_token);
    form::page_title(
        ui,
        theme,
        &s.t("dt-firebase"),
        Some(&s.t("dt-firebase-sub")),
    );
    form::section(ui, theme, &s.t("dt-firebase-account"), |ui| {
        util::input(
            ui,
            &mut st.service_json,
            &s.t("dt-firebase-account-hint"),
            5,
        );
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::FOLDER_OPEN,
                s.t("dt-firebase-load")
            ))
            .clicked()
            && let Some(p) = rfd::FileDialog::new()
                .add_filter("JSON", &["json"])
                .pick_file()
            && let Ok(text) = std::fs::read_to_string(p)
        {
            st.service_json = text;
        }
        match parse_service_account(&st.service_json) {
            Ok(sa) => {
                ui.label(
                    egui::RichText::new(format!("{} · {}", sa.project_id, sa.client_email))
                        .color(theme.text_muted),
                );
            }
            Err(e) if !st.service_json.trim().is_empty() => util::error(ui, theme, &e),
            Err(_) => {}
        }
        form::hint(ui, theme, &s.t("dt-firebase-privacy"));
    });
    let sa = parse_service_account(&st.service_json).ok();
    form::section(ui, theme, &s.t("dt-firebase-custom"), |ui| {
        form::field_grid(ui, "fb-custom", |ui| {
            form::label(ui, "uid");
            form::text_field(ui, &mut st.uid, "");
            ui.end_row();
            form::label(ui, &s.t("dt-firebase-claims"));
            util::input(ui, &mut st.claims, "{}", 2);
            ui.end_row();
            form::label(ui, &s.t("dt-firebase-ttl"));
            ui.add(
                egui::DragValue::new(&mut st.ttl_minutes)
                    .range(1..=60)
                    .suffix(" min"),
            );
            ui.end_row();
        });
        if ui
            .add_enabled(
                sa.is_some(),
                egui::Button::new(s.t("dt-firebase-make-custom")),
            )
            .clicked()
            && let Some(sa) = &sa
        {
            let claims = serde_json::from_str::<serde_json::Value>(&st.claims).ok();
            st.custom = Some(custom_token(
                sa,
                &st.uid,
                claims.as_ref(),
                st.ttl_minutes * 60,
            ));
        }
        util::result(
            ui,
            s,
            theme,
            &s.t("dt-firebase-custom-token"),
            &st.custom,
            4,
        );
    });
    form::section(ui, theme, &s.t("dt-firebase-access"), |ui| {
        form::field_grid(ui, "fb-access", |ui| {
            form::label(ui, &s.t("dt-firebase-scopes"));
            form::text_field(ui, &mut st.scopes, "");
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    sa.is_some() && st.access_pending.is_none(),
                    egui::Button::new(s.t("dt-firebase-get-access")),
                )
                .on_hover_text(s.t("dt-firebase-network-tip"))
                .clicked()
                && let Some(sa) = &sa
            {
                match access_token_assertion(sa, &st.scopes) {
                    Ok(assertion) => {
                        st.access = None;
                        st.access_pending = Some(post_form(
                            s,
                            sa.token_uri.clone(),
                            vec![
                                (
                                    "grant_type".into(),
                                    "urn:ietf:params:oauth:grant-type:jwt-bearer".into(),
                                ),
                                ("assertion".into(), assertion),
                            ],
                        ));
                    }
                    Err(e) => st.access = Some(Err(e)),
                }
            }
            if st.access_pending.is_some() {
                ui.spinner();
            }
        });
        util::result(
            ui,
            s,
            theme,
            &s.t("dt-firebase-access-token"),
            &st.access,
            5,
        );
    });
    form::section(ui, theme, &s.t("dt-firebase-id"), |ui| {
        form::field_grid(ui, "fb-id", |ui| {
            form::label(ui, &s.t("dt-firebase-web-key"));
            form::secret_field(ui, &mut st.web_api_key, "", 320.0);
            ui.end_row();
        });
        let custom = st.custom.as_ref().and_then(|c| c.as_ref().ok().cloned());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    custom.is_some() && !st.web_api_key.trim().is_empty() && st.id_pending.is_none(),
                    egui::Button::new(s.t("dt-firebase-get-id")),
                )
                .on_hover_text(s.t("dt-firebase-network-tip"))
                .clicked()
                && let Some(token) = custom
            {
                st.id_token = None;
                st.id_pending = Some(post_json(
                    s,
                    format!(
                        "https://identitytoolkit.googleapis.com/v1/accounts:signInWithCustomToken?key={}",
                        st.web_api_key.trim()
                    ),
                    serde_json::json!({ "token": token, "returnSecureToken": true }),
                ));
            }
            if st.id_pending.is_some() {
                ui.spinner();
            }
        });
        form::hint(ui, theme, &s.t("dt-firebase-id-hint"));
        util::result(ui, s, theme, &s.t("dt-firebase-id-token"), &st.id_token, 6);
    });
    let _ = b64url_encode;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_account_parsing() {
        let sa =
            parse_service_account(r#"{"project_id":"p","client_email":"a@b","private_key":"k"}"#)
                .unwrap();
        assert_eq!(sa.token_uri, "https://oauth2.googleapis.com/token");
        assert!(parse_service_account("{}").is_err());
        assert!(custom_token(&sa, "", None, 60).is_err());
    }
}
