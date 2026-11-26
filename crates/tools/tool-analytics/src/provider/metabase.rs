//! Metabase through its REST API (docs/tools/analytics.md §5, §8.3): an
//! API key in `x-api-key`, or a session from `POST /api/session` sent as
//! `X-Metabase-Session`. The session is kept in memory only and ended with
//! `DELETE /api/session` when the tab closes.

use async_trait::async_trait;
use hs_core::service_auth::{Auth, TokenAs};
use hs_core::service_http::{Http, s};
use parking_lot::RwLock;
use reqwest::Method;
use serde_json::{Value, json};

use super::{API_KEYS_FROM_MINOR, BiProvider, Caps, Result, ServerInfo, Support, Version};
use crate::profile::MetabaseConfig;

const SESSION_HEADER: &str = "X-Metabase-Session";

pub struct Metabase {
    cfg: MetabaseConfig,
    http: Http,
    /// The session id a username and password gave, while one is open.
    session: RwLock<Option<String>>,
}

impl Metabase {
    pub fn new(cfg: MetabaseConfig) -> Result<Metabase> {
        // The client carries no credential of its own: each request is
        // handed the current one (`request_as`), so a renewed session
        // needs no second client.
        let http = Http::with_tls(&cfg.url, Auth::None, TokenAs::Bearer, cfg.timeout, &cfg.tls)?;
        Ok(Metabase {
            cfg,
            http,
            session: RwLock::new(None),
        })
    }

    /// What goes on the next request.
    fn auth(&self) -> Auth {
        match &self.cfg.auth {
            Auth::ApiKey { .. } => self.cfg.auth.clone(),
            Auth::Password { .. } => match self.session.read().clone() {
                Some(id) => Auth::ApiKey {
                    header: SESSION_HEADER.into(),
                    key: id,
                },
                None => Auth::None,
            },
            _ => Auth::None,
        }
    }

    /// `GET path` as the signed-in account.
    pub async fn get(&self, path: &str) -> Result<Value> {
        self.http
            .json(self.http.request_as(Method::GET, path, &self.auth()))
            .await
    }

    async fn sign_in(&self) -> Result<()> {
        let Auth::Password { user, password } = &self.cfg.auth else {
            return Ok(());
        };
        let rb = self
            .http
            .request_as(Method::POST, "/api/session", &Auth::None)
            .json(&json!({ "username": user, "password": password }));
        let v = self.http.json(rb).await.map_err(|e| {
            format!(
                "Metabase refused the sign-in: {e}. A server that signs people in through single sign-on takes an API key instead."
            )
        })?;
        let id = s(&v, "id");
        if id.is_empty() {
            return Err("Metabase signed in but gave no session.".into());
        }
        *self.session.write() = Some(id);
        Ok(())
    }
}

#[async_trait]
impl BiProvider for Metabase {
    async fn connect(&self) -> Result<ServerInfo> {
        // The version is public: read it before signing in, so an API key
        // sent to a server too old for one is said in words, not as a 401.
        let props = self.http.get_json("/api/session/properties").await?;
        let version_text = props
            .pointer("/version/tag")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let version = Version::parse(&version_text);
        if matches!(self.cfg.auth, Auth::ApiKey { .. })
            && version.is_some_and(|v| v.minor < API_KEYS_FROM_MINOR)
        {
            return Err(format!(
                "This Metabase ({version_text}) is older than 0.{API_KEYS_FROM_MINOR}, which is the first to take an API key. Use a username and password."
            ));
        }
        self.sign_in().await?;
        let me = self.get("/api/user/current").await?;
        let name = [s(&me, "common_name"), s(&me, "email")]
            .into_iter()
            .find(|n| !n.is_empty())
            .unwrap_or_default();
        Ok(ServerInfo {
            product: "Metabase".into(),
            version_text,
            support: version.map(Version::support).unwrap_or(Support::Unknown),
            version,
            signed_in_as: name,
        })
    }

    fn capabilities(&self) -> Caps {
        // Browsing and SQL arrive with FEAT-0127 and FEAT-0130.
        Caps::default()
    }

    fn web_url(&self) -> String {
        self.cfg.url.clone()
    }

    async fn disconnect(&self) {
        let Some(id) = self.session.write().take() else {
            return;
        };
        let rb = self.http.request_as(
            Method::DELETE,
            &format!(
                "/api/session?metabase-session-id={}",
                hs_core::service_http::seg(&id)
            ),
            &Auth::ApiKey {
                header: SESSION_HEADER.into(),
                key: id.clone(),
            },
        );
        // Best effort: the session expires on its own if this fails.
        let _ = self.http.send(rb).await;
    }
}
