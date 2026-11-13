//! Keycloak through its Admin REST API (docs/tools/idm.md §4, ADR-032):
//! `/admin/realms/…` with a bearer token from the realm's OpenID Connect
//! token endpoint, server-side paging everywhere, and every refusal made
//! specific (§8.4).

pub mod auth;
pub mod model;
pub mod version;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hs_core::service_auth::{Auth, TokenAs};
use hs_core::service_http::{Http, Tls, seg, status_error, transport_error};
use parking_lot::RwLock;
use reqwest::{Method, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use self::auth::{Form, Grant, SystemClock, TokenEndpoint, TokenResponse, Tokens};
use self::model::*;
use crate::mask;
use crate::provider::*;

/// Everything a Keycloak session needs, resolved from the profile.
#[derive(Debug, Clone)]
pub struct KeycloakConfig {
    /// The base URL, no `/admin` (with `/auth` for a pre-17 server).
    pub url: String,
    /// The realm the token is requested from.
    pub auth_realm: String,
    pub grant: Grant,
    pub tls: Tls,
    pub timeout: Duration,
}

/// What a call is, for its error: the kind of object (for "this user no
/// longer exists"), the realm-management role it needs (for a 403), and
/// whether it reads one item (a 404 means it is gone) or a list or
/// endpoint (a 404 means the server lacks it).
#[derive(Debug, Clone, Copy)]
struct Op {
    what: &'static str,
    needs: Option<&'static str>,
    item: bool,
}

const fn list(what: &'static str, needs: &'static str) -> Op {
    Op {
        what,
        needs: Some(needs),
        item: false,
    }
}

const fn item(what: &'static str, needs: &'static str) -> Op {
    Op {
        what,
        needs: Some(needs),
        item: true,
    }
}

const VIEW_USERS: &str = "view-users";
const MANAGE_USERS: &str = "manage-users";
const QUERY_GROUPS: &str = "query-groups";
const VIEW_REALM: &str = "view-realm";
const MANAGE_REALM: &str = "manage-realm";
const VIEW_CLIENTS: &str = "view-clients";
const MANAGE_CLIENTS: &str = "manage-clients";
const VIEW_EVENTS: &str = "view-events";

/// A query string, empty values left out, every value percent-encoded.
#[derive(Default)]
struct Q(Vec<(&'static str, String)>);

impl Q {
    fn s(mut self, k: &'static str, v: &str) -> Q {
        if !v.is_empty() {
            self.0.push((k, v.to_string()));
        }
        self
    }
    fn b(mut self, k: &'static str, v: bool) -> Q {
        self.0.push((k, v.to_string()));
        self
    }
    fn many(mut self, k: &'static str, vs: &[String]) -> Q {
        for v in vs.iter().filter(|v| !v.is_empty()) {
            self.0.push((k, v.clone()));
        }
        self
    }
    /// `first` and one row over `max`, so the answer says whether there
    /// is more ([`Paged::from_probe`]).
    fn page(mut self, page: Page) -> Q {
        self.0.push(("first", page.first.to_string()));
        self.0.push(("max", (page.max + 1).to_string()));
        self
    }
    fn on(self, path: &str) -> String {
        if self.0.is_empty() {
            return path.to_string();
        }
        let q: Vec<String> = self
            .0
            .iter()
            .map(|(k, v)| format!("{k}={}", seg(v)))
            .collect();
        format!("{path}?{}", q.join("&"))
    }
}

/// The Users list's query (§4.3): `search` for free text, or with Exact
/// the precise `username` (or `email` for a term with an `@`); `q` for
/// `attr:value` terms; `enabled`.
fn user_filters(q: &UserQuery) -> Q {
    let (text, attrs) = q.terms();
    let mut out = Q::default();
    if q.exact && !text.is_empty() {
        out = out
            .s(
                if text.contains('@') {
                    "email"
                } else {
                    "username"
                },
                &text,
            )
            .b("exact", true);
    } else {
        out = out.s("search", &text);
    }
    if !attrs.is_empty() {
        let terms: Vec<String> = attrs.iter().map(|(k, v)| format!("{k}:{v}")).collect();
        out = out.s("q", &terms.join(" "));
    }
    if let Some(e) = q.enabled {
        out = out.b("enabled", e);
    }
    out
}

fn date(d: &str) -> String {
    d.trim().to_string()
}

fn subject_base(who: &Subject) -> String {
    match who {
        Subject::User(id) => format!("/users/{}/role-mappings", seg(id)),
        Subject::Group(id) => format!("/groups/{}/role-mappings", seg(id)),
    }
}

fn roles_base(scope: &RoleScope) -> String {
    match scope {
        RoleScope::Realm => "/roles".into(),
        RoleScope::Client(c) => format!("/clients/{}/roles", seg(c)),
    }
}

/// The token endpoint over HTTP.
struct HttpTokens {
    http: Http,
    token_url: String,
    logout_url: String,
    grant: Grant,
    realm: String,
}

fn form_text(form: &Form) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::new());
    for (i, (k, v)) in form.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(k);
        out.push('=');
        out.push_str(&seg(v));
    }
    out
}

#[async_trait]
impl TokenEndpoint for HttpTokens {
    async fn token(&self, form: &Form) -> Result<TokenResponse> {
        let body = form_text(form);
        let resp = self
            .http
            .request_as(Method::POST, &self.token_url, &Auth::None)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.as_bytes().to_vec())
            .send()
            .await
            .map_err(|e| IdmError::Network(transport_error(&e)))?;
        let status = resp.status();
        let text = Zeroizing::new(resp.text().await.unwrap_or_default());
        tracing::debug!("idm: POST token endpoint -> {}", status.as_u16());
        if !status.is_success() {
            return Err(auth::token_error(
                &self.grant,
                status.as_u16(),
                &text,
                &self.realm,
            ));
        }
        serde_json::from_str(&text)
            .map_err(|e| IdmError::Other(format!("the token endpoint's answer was not JSON: {e}")))
    }

    async fn logout(&self, form: &Form) -> Result<()> {
        let body = form_text(form);
        self.http
            .request_as(Method::POST, &self.logout_url, &Auth::None)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.as_bytes().to_vec())
            .send()
            .await
            .map_err(|e| IdmError::Network(transport_error(&e)))?;
        Ok(())
    }
}

/// A request body. A password's is kept in a buffer that is zeroised
/// when it is dropped (reqwest's own copy is beyond reach).
enum Body {
    Json(Value),
    Secret(Zeroizing<Vec<u8>>),
}

struct State {
    info: ServerInfo,
    caps: Caps,
}

/// One Keycloak server, one token, for one tab.
pub struct Keycloak {
    url: String,
    auth_realm: String,
    http: Http,
    tokens: Tokens,
    state: RwLock<State>,
}

impl Keycloak {
    pub fn new(cfg: KeycloakConfig) -> std::result::Result<Keycloak, String> {
        let url = cfg.url.trim().trim_end_matches('/').to_string();
        let http = Http::with_tls(&url, Auth::None, TokenAs::Bearer, cfg.timeout, &cfg.tls)?;
        let oidc = format!(
            "{url}/realms/{}/protocol/openid-connect",
            seg(&cfg.auth_realm)
        );
        let endpoint = Arc::new(HttpTokens {
            http: http.clone(),
            token_url: format!("{oidc}/token"),
            logout_url: format!("{oidc}/logout"),
            grant: cfg.grant.clone(),
            realm: cfg.auth_realm.clone(),
        });
        Ok(Keycloak {
            tokens: Tokens::new(endpoint, cfg.grant, Arc::new(SystemClock)),
            url,
            auth_realm: cfg.auth_realm,
            http,
            state: RwLock::new(State {
                info: ServerInfo {
                    product: "Keycloak".into(),
                    ..Default::default()
                },
                caps: Caps::default(),
            }),
        })
    }

    fn admin(&self, realm: &str, rest: &str) -> String {
        format!("/admin/realms/{}{rest}", seg(realm))
    }

    /// A 4xx or 5xx in the terms the screens act on (§8.4).
    async fn refusal(
        &self,
        status: StatusCode,
        body: &str,
        op: Op,
        realm: &str,
        token: &SecretString,
    ) -> IdmError {
        let (message, field) = error_words(body);
        match status.as_u16() {
            401 => {
                let claims = auth::claims(token.expose_secret()).unwrap_or(Value::Null);
                let checks_audience = self.state.read().info.version.is_none_or(|v| v.major >= 26);
                if checks_audience && !auth::has_admin_audience(&claims) {
                    IdmError::Audience {
                        client_id: self.tokens.grant().client_id().to_string(),
                    }
                } else {
                    IdmError::Unauthorized
                }
            }
            403 => IdmError::Forbidden {
                realm: realm.to_string(),
                needs: op.needs,
            },
            404 => {
                let gone =
                    op.item || message.contains("Could not find") || message.contains("not found");
                if gone && !message.contains("matching target resource") {
                    IdmError::NotFound { what: op.what }
                } else {
                    IdmError::Unsupported(op.what.to_string())
                }
            }
            409 => IdmError::Conflict(if message.is_empty() {
                "the object already exists".into()
            } else {
                message
            }),
            400 => IdmError::Invalid { message, field },
            _ => IdmError::Other(status_error(status, body)),
        }
    }

    /// Send with the session's token; a 401 renews it and retries once.
    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&Body>,
        op: Op,
        realm: &str,
    ) -> Result<reqwest::Response> {
        let mut retried = false;
        loop {
            let token = self.tokens.bearer().await?;
            let auth = Auth::Token {
                user: String::new(),
                token: token.expose_secret().to_string(),
            };
            let mut rb = self.http.request_as(method.clone(), path, &auth);
            rb = match body {
                Some(Body::Json(v)) => rb.json(v),
                Some(Body::Secret(bytes)) => rb
                    .header("Content-Type", "application/json")
                    .body(bytes.to_vec()),
                None => rb,
            };
            let resp = rb
                .send()
                .await
                .map_err(|e| IdmError::Network(transport_error(&e)))?;
            let status = resp.status();
            // The path carries ids, never names or secrets; the query is
            // left out because a search can hold a name.
            tracing::debug!(
                "idm: {method} {} -> {}",
                path.split('?').next().unwrap_or(path),
                status.as_u16()
            );
            if status.is_success() {
                return Ok(resp);
            }
            if status == StatusCode::UNAUTHORIZED && !retried {
                retried = true;
                self.tokens.invalidate(&token).await;
                continue;
            }
            let text = resp.text().await.unwrap_or_default();
            return Err(self.refusal(status, &text, op, realm, &token).await);
        }
    }

    async fn json(&self, resp: reqwest::Response) -> Result<Value> {
        let text = resp
            .text()
            .await
            .map_err(|e| IdmError::Network(e.to_string()))?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|e| IdmError::Other(format!("the server's answer was not JSON: {e}")))
    }

    async fn get(&self, realm: &str, rest: &str, op: Op) -> Result<Value> {
        let resp = self
            .send(Method::GET, &self.admin(realm, rest), None, op, realm)
            .await?;
        self.json(resp).await
    }

    async fn write(
        &self,
        method: Method,
        realm: &str,
        rest: &str,
        body: Option<Value>,
        op: Op,
    ) -> Result<reqwest::Response> {
        let body = body.map(Body::Json);
        self.send(method, &self.admin(realm, rest), body.as_ref(), op, realm)
            .await
    }

    /// Read the whole representation, merge the change, write it back.
    async fn read_merge_write(
        &self,
        realm: &str,
        rest: &str,
        change: &Change,
        op: Op,
    ) -> Result<()> {
        let current = self.get(realm, rest, op).await?;
        let merged = change.merged_into(current);
        self.write(Method::PUT, realm, rest, Some(merged), op)
            .await?;
        Ok(())
    }

    /// The id a `201 Created` names in its `Location`, else the body's.
    async fn created_id(&self, resp: reqwest::Response) -> Result<String> {
        let from_location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|l| l.trim_end_matches('/').rsplit('/').next())
            .map(String::from);
        if let Some(id) = from_location.filter(|s| !s.is_empty()) {
            return Ok(id);
        }
        let v = self.json(resp).await?;
        Ok(v.get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    async fn paged<T, K>(
        &self,
        realm: &str,
        path: String,
        page: Page,
        op: Op,
        f: impl Fn(K, Value) -> T,
        total: Option<u64>,
    ) -> Result<Paged<T>>
    where
        K: serde::de::DeserializeOwned,
    {
        let v = self.get(realm, &path, op).await?;
        Ok(Paged::from_probe(parse_list(v, op.what, f)?, page, total))
    }

    fn roles_of(v: Value, what: &str) -> Result<Vec<Role>> {
        if v.is_null() {
            return Ok(Vec::new());
        }
        parse_list(v, what, role)
    }
}

#[async_trait]
impl IdmProvider for Keycloak {
    async fn connect(&self) -> Result<ServerInfo> {
        // The token first, so a refused credential is reported as such.
        self.tokens.bearer().await?;
        let claims = self.tokens.claims().await.unwrap_or(Value::Null);
        let resp = self
            .send(
                Method::GET,
                "/admin/serverinfo",
                None,
                list("server info", VIEW_REALM),
                &self.auth_realm,
            )
            .await;
        let si = match resp {
            Ok(r) => self.json(r).await?,
            // These say the session cannot work at all.
            Err(
                e @ (IdmError::Network(_)
                | IdmError::Login(_)
                | IdmError::Audience { .. }
                | IdmError::Unauthorized),
            ) => return Err(e),
            // A realm-scoped admin may be refused server info: the
            // version is unknown and the tab says so (§8.3).
            Err(e) => {
                tracing::info!("idm: server info refused ({e}); the version is unknown");
                Value::Null
            }
        };
        let version_text = si
            .pointer("/systemInfo/version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let parsed = version::parse_version(&version_text);
        let (features, listed) = version::features(&si);
        let enum_list = |k: &str| -> Vec<String> {
            si.pointer(&format!("/enums/{k}"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        };
        let mut caps = version::caps_for(parsed, &features, listed);
        caps.roles = auth::role_hints(&claims);
        caps.auth_realm = self.auth_realm.clone();
        let info = ServerInfo {
            product: "Keycloak".into(),
            version_text,
            version: parsed,
            support: version::support(parsed),
            features,
            event_types: enum_list("eventType"),
            signed_in_as: auth::subject(&claims),
            auth_realm: self.auth_realm.clone(),
        };
        let mut st = self.state.write();
        st.info = info.clone();
        st.caps = caps;
        Ok(info)
    }

    fn info(&self) -> ServerInfo {
        self.state.read().info.clone()
    }

    fn capabilities(&self) -> Caps {
        self.state.read().caps.clone()
    }

    fn console_url(&self, realm: &str, page: &str) -> String {
        format!(
            "{}/admin/{}/console/#/{}/{}",
            self.url,
            seg(&self.auth_realm),
            seg(realm),
            page.trim_start_matches('/')
        )
    }

    async fn disconnect(&self) {
        self.tokens.logout().await;
    }

    async fn realms(&self) -> Result<Vec<Realm>> {
        let resp = self
            .send(
                Method::GET,
                "/admin/realms?briefRepresentation=true",
                None,
                list("realms", VIEW_REALM),
                &self.auth_realm,
            )
            .await?;
        let v = self.json(resp).await?;
        parse_list(v, "realms", |r: KcRealm, _| Realm::from(r))
    }

    async fn realm_settings(&self, realm: &str) -> Result<RealmSettings> {
        let raw = self.get(realm, "", item("realm", VIEW_REALM)).await?;
        let r: KcRealm = parse(raw.clone(), "realm")?;
        let events = self.events_config(realm).await.ok();
        let mut shown = raw.clone();
        mask::mask(&mut shown);
        Ok(RealmSettings {
            realm: r.into(),
            facts: realm_facts(&raw),
            events,
            raw: shown,
        })
    }

    async fn events_config(&self, realm: &str) -> Result<EventsConfig> {
        let v = self
            .get(realm, "/events/config", list("event settings", VIEW_EVENTS))
            .await?;
        Ok(parse::<KcEventsConfig>(v, "event settings")?.into())
    }

    async fn users(&self, realm: &str, q: &UserQuery, page: Page) -> Result<Paged<UserRow>> {
        let path = user_filters(q)
            .b("briefRepresentation", true)
            .page(page)
            .on("/users");
        self.paged(realm, path, page, list("users", VIEW_USERS), user_row, None)
            .await
    }

    async fn user_count(&self, realm: &str, q: &UserQuery) -> Result<Option<u64>> {
        let path = user_filters(q).on("/users/count");
        let v = self.get(realm, &path, list("users", VIEW_USERS)).await?;
        Ok(Some(count(&v)))
    }

    async fn user(&self, realm: &str, id: &str) -> Result<User> {
        let profile = self.capabilities().user_profile;
        let path = Q::default()
            .s("userProfileMetadata", if profile { "true" } else { "" })
            .on(&format!("/users/{}", seg(id)));
        let raw = self.get(realm, &path, item("user", VIEW_USERS)).await?;
        let u: KcUser = parse(raw.clone(), "user")?;
        Ok(u.into_user(raw))
    }

    async fn unmanaged_policy(&self, realm: &str) -> Result<UnmanagedPolicy> {
        if !self.capabilities().user_profile {
            return Ok(UnmanagedPolicy::Off);
        }
        match self
            .get(
                realm,
                "/users/profile",
                list("the user profile", VIEW_REALM),
            )
            .await
        {
            Ok(v) => Ok(UnmanagedPolicy::parse(
                v.get("unmanagedAttributePolicy")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )),
            Err(IdmError::Unsupported(_)) => Ok(UnmanagedPolicy::Off),
            Err(e) => Err(e),
        }
    }

    async fn required_actions(&self, realm: &str) -> Result<Vec<RequiredAction>> {
        let v = self
            .get(
                realm,
                "/authentication/required-actions",
                list("required actions", VIEW_REALM),
            )
            .await?;
        parse_list(v, "required actions", |a: KcRequiredAction, _| {
            RequiredAction {
                alias: a.alias,
                name: a.name,
                enabled: a.enabled,
            }
        })
    }

    async fn user_credentials(&self, realm: &str, id: &str) -> Result<Vec<CredentialInfo>> {
        let v = self
            .get(
                realm,
                &format!("/users/{}/credentials", seg(id)),
                item("user", VIEW_USERS),
            )
            .await?;
        parse_list(v, "credentials", |c: KcCredential, _| c.into())
    }

    async fn user_groups(&self, realm: &str, id: &str, page: Page) -> Result<Paged<Group>> {
        let path = Q::default()
            .b("briefRepresentation", true)
            .page(page)
            .on(&format!("/users/{}/groups", seg(id)));
        self.paged(realm, path, page, item("user", VIEW_USERS), group, None)
            .await
    }

    async fn user_sessions(&self, realm: &str, id: &str) -> Result<Vec<Session>> {
        let v = self
            .get(
                realm,
                &format!("/users/{}/sessions", seg(id)),
                item("user", VIEW_USERS),
            )
            .await?;
        parse_list(v, "sessions", session)
    }

    async fn user_offline_sessions(
        &self,
        realm: &str,
        id: &str,
        client: &str,
    ) -> Result<Vec<Session>> {
        let v = self
            .get(
                realm,
                &format!("/users/{}/offline-sessions/{}", seg(id), seg(client)),
                item("user", VIEW_USERS),
            )
            .await?;
        parse_list(v, "sessions", session)
    }

    async fn create_user(&self, realm: &str, user: &NewUser) -> Result<String> {
        let resp = self
            .write(
                Method::POST,
                realm,
                "/users",
                Some(new_user_body(user)),
                list("user", MANAGE_USERS),
            )
            .await?;
        self.created_id(resp).await
    }

    async fn update_user(&self, realm: &str, user: &ObjRef, change: &Change) -> Result<()> {
        self.read_merge_write(
            realm,
            &format!("/users/{}", seg(&user.id)),
            change,
            item("user", MANAGE_USERS),
        )
        .await
    }

    async fn set_user_enabled(&self, realm: &str, user: &ObjRef, enabled: bool) -> Result<()> {
        self.update_user(realm, user, &Change::default().set("enabled", enabled))
            .await
    }

    async fn reset_password(
        &self,
        realm: &str,
        user: &ObjRef,
        password: SecretString,
        temporary: bool,
    ) -> Result<()> {
        let value = Zeroizing::new(
            serde_json::to_string(password.expose_secret())
                .map_err(|e| IdmError::Other(e.to_string()))?,
        );
        let body = Zeroizing::new(
            format!(
                r#"{{"type":"password","temporary":{temporary},"value":{}}}"#,
                *value
            )
            .into_bytes(),
        );
        drop(password);
        let path = self.admin(realm, &format!("/users/{}/reset-password", seg(&user.id)));
        self.send(
            Method::PUT,
            &path,
            Some(&Body::Secret(body)),
            item("user", MANAGE_USERS),
            realm,
        )
        .await?;
        Ok(())
    }

    async fn delete_user(&self, realm: &str, user: &ObjRef) -> Result<()> {
        self.write(
            Method::DELETE,
            realm,
            &format!("/users/{}", seg(&user.id)),
            None,
            item("user", MANAGE_USERS),
        )
        .await?;
        Ok(())
    }

    async fn join_group(&self, realm: &str, user: &ObjRef, group: &ObjRef) -> Result<()> {
        self.write(
            Method::PUT,
            realm,
            &format!("/users/{}/groups/{}", seg(&user.id), seg(&group.id)),
            None,
            item("user", MANAGE_USERS),
        )
        .await?;
        Ok(())
    }

    async fn leave_group(&self, realm: &str, user: &ObjRef, group: &ObjRef) -> Result<()> {
        self.write(
            Method::DELETE,
            realm,
            &format!("/users/{}/groups/{}", seg(&user.id), seg(&group.id)),
            None,
            item("user", MANAGE_USERS),
        )
        .await?;
        Ok(())
    }

    async fn logout_user(&self, realm: &str, user: &ObjRef) -> Result<()> {
        self.write(
            Method::POST,
            realm,
            &format!("/users/{}/logout", seg(&user.id)),
            None,
            item("user", MANAGE_USERS),
        )
        .await?;
        Ok(())
    }

    async fn end_session(&self, realm: &str, session: &ObjRef) -> Result<()> {
        self.write(
            Method::DELETE,
            realm,
            &format!("/sessions/{}", seg(&session.id)),
            None,
            item("session", MANAGE_USERS),
        )
        .await?;
        Ok(())
    }

    async fn groups(&self, realm: &str, search: &str, page: Page) -> Result<Paged<Group>> {
        let total = self.group_count(realm, search).await.ok().flatten();
        let path = Q::default()
            .s("search", search)
            .b("briefRepresentation", true)
            .page(page)
            .on("/groups");
        let mut p = self
            .paged(
                realm,
                path,
                page,
                list("groups", QUERY_GROUPS),
                group,
                total,
            )
            .await?;
        if !search.trim().is_empty() {
            let mut flat = Vec::new();
            flatten_matches(std::mem::take(&mut p.rows), search.trim(), &mut flat);
            p.rows = flat;
        }
        Ok(p)
    }

    async fn group_count(&self, realm: &str, search: &str) -> Result<Option<u64>> {
        let q = if search.trim().is_empty() {
            Q::default().b("top", true)
        } else {
            Q::default().s("search", search.trim())
        };
        let v = self
            .get(realm, &q.on("/groups/count"), list("groups", QUERY_GROUPS))
            .await?;
        Ok(Some(count(&v)))
    }

    async fn group(&self, realm: &str, id: &str) -> Result<Group> {
        let raw = self
            .get(
                realm,
                &format!("/groups/{}", seg(id)),
                item("group", QUERY_GROUPS),
            )
            .await?;
        let g: KcGroup = parse(raw.clone(), "group")?;
        Ok(group(g, raw))
    }

    async fn group_children(&self, realm: &str, id: &str, page: Page) -> Result<Paged<Group>> {
        if self.capabilities().group_children {
            let path = Q::default()
                .b("briefRepresentation", true)
                .page(page)
                .on(&format!("/groups/{}/children", seg(id)));
            match self
                .paged(
                    realm,
                    path,
                    page,
                    list("sub-groups", QUERY_GROUPS),
                    group,
                    None,
                )
                .await
            {
                Err(IdmError::Unsupported(_)) => {
                    // Keycloak 22: children come in the parent (§8.3).
                    self.state.write().caps.group_children = false;
                }
                other => return other,
            }
        }
        let parent = self.group(realm, id).await?;
        let total = parent.sub_groups.len() as u64;
        let rows: Vec<Group> = parent
            .sub_groups
            .into_iter()
            .skip(page.first as usize)
            .take(page.max as usize + 1)
            .collect();
        Ok(Paged::from_probe(rows, page, Some(total)))
    }

    async fn group_members(&self, realm: &str, id: &str, page: Page) -> Result<Paged<UserRow>> {
        let path = Q::default()
            .b("briefRepresentation", true)
            .page(page)
            .on(&format!("/groups/{}/members", seg(id)));
        self.paged(realm, path, page, item("group", VIEW_USERS), user_row, None)
            .await
    }

    async fn create_group(
        &self,
        realm: &str,
        parent: Option<&ObjRef>,
        name: &str,
    ) -> Result<String> {
        let path = match parent {
            Some(p) => format!("/groups/{}/children", seg(&p.id)),
            None => "/groups".into(),
        };
        let resp = self
            .write(
                Method::POST,
                realm,
                &path,
                Some(json!({ "name": name })),
                list("group", MANAGE_USERS),
            )
            .await?;
        self.created_id(resp).await
    }

    async fn update_group(&self, realm: &str, group: &ObjRef, change: &Change) -> Result<()> {
        self.read_merge_write(
            realm,
            &format!("/groups/{}", seg(&group.id)),
            change,
            item("group", MANAGE_USERS),
        )
        .await
    }

    async fn delete_group(&self, realm: &str, group: &ObjRef) -> Result<()> {
        self.write(
            Method::DELETE,
            realm,
            &format!("/groups/{}", seg(&group.id)),
            None,
            item("group", MANAGE_USERS),
        )
        .await?;
        Ok(())
    }

    async fn role_mappings(&self, realm: &str, who: &Subject) -> Result<RoleMappings> {
        let v = self
            .get(realm, &subject_base(who), item("role mapping", VIEW_USERS))
            .await?;
        Ok(model::role_mappings(&v))
    }

    async fn realm_role_mappings(
        &self,
        realm: &str,
        who: &Subject,
        effective: bool,
    ) -> Result<Vec<Role>> {
        let rest = if effective {
            "/realm/composite"
        } else {
            "/realm"
        };
        let v = self
            .get(
                realm,
                &format!("{}{rest}", subject_base(who)),
                item("role mapping", VIEW_USERS),
            )
            .await?;
        Keycloak::roles_of(v, "roles")
    }

    async fn available_realm_roles(&self, realm: &str, who: &Subject) -> Result<Vec<Role>> {
        let v = self
            .get(
                realm,
                &format!("{}/realm/available", subject_base(who)),
                item("role mapping", VIEW_USERS),
            )
            .await?;
        Keycloak::roles_of(v, "roles")
    }

    async fn client_role_mappings(
        &self,
        realm: &str,
        who: &Subject,
        client: &str,
        effective: bool,
    ) -> Result<Vec<Role>> {
        let rest = if effective { "/composite" } else { "" };
        let v = self
            .get(
                realm,
                &format!("{}/clients/{}{rest}", subject_base(who), seg(client)),
                item("role mapping", VIEW_USERS),
            )
            .await?;
        Keycloak::roles_of(v, "roles")
    }

    async fn available_client_roles(
        &self,
        realm: &str,
        who: &Subject,
        client: &str,
    ) -> Result<Vec<Role>> {
        let v = self
            .get(
                realm,
                &format!("{}/clients/{}/available", subject_base(who), seg(client)),
                item("role mapping", VIEW_USERS),
            )
            .await?;
        Keycloak::roles_of(v, "roles")
    }

    async fn add_realm_roles(&self, realm: &str, who: &Subject, roles: &[Role]) -> Result<()> {
        let body = Value::Array(roles.iter().map(role_body).collect());
        self.write(
            Method::POST,
            realm,
            &format!("{}/realm", subject_base(who)),
            Some(body),
            item("role mapping", MANAGE_USERS),
        )
        .await?;
        Ok(())
    }

    async fn remove_realm_roles(&self, realm: &str, who: &Subject, roles: &[Role]) -> Result<()> {
        let body = Value::Array(roles.iter().map(role_body).collect());
        self.write(
            Method::DELETE,
            realm,
            &format!("{}/realm", subject_base(who)),
            Some(body),
            item("role mapping", MANAGE_USERS),
        )
        .await?;
        Ok(())
    }

    async fn add_client_roles(
        &self,
        realm: &str,
        who: &Subject,
        client: &ObjRef,
        roles: &[Role],
    ) -> Result<()> {
        let body = Value::Array(roles.iter().map(role_body).collect());
        self.write(
            Method::POST,
            realm,
            &format!("{}/clients/{}", subject_base(who), seg(&client.id)),
            Some(body),
            item("role mapping", MANAGE_USERS),
        )
        .await?;
        Ok(())
    }

    async fn remove_client_roles(
        &self,
        realm: &str,
        who: &Subject,
        client: &ObjRef,
        roles: &[Role],
    ) -> Result<()> {
        let body = Value::Array(roles.iter().map(role_body).collect());
        self.write(
            Method::DELETE,
            realm,
            &format!("{}/clients/{}", subject_base(who), seg(&client.id)),
            Some(body),
            item("role mapping", MANAGE_USERS),
        )
        .await?;
        Ok(())
    }

    async fn roles(
        &self,
        realm: &str,
        scope: &RoleScope,
        search: &str,
        page: Page,
    ) -> Result<Paged<Role>> {
        let path = Q::default()
            .s("search", search.trim())
            .b("briefRepresentation", true)
            .page(page)
            .on(&roles_base(scope));
        let op = match scope {
            RoleScope::Realm => list("roles", VIEW_REALM),
            RoleScope::Client(_) => item("client", VIEW_CLIENTS),
        };
        self.paged(realm, path, page, op, role, None).await
    }

    async fn role(&self, realm: &str, id: &str) -> Result<Role> {
        let raw = self
            .get(
                realm,
                &format!("/roles-by-id/{}", seg(id)),
                item("role", VIEW_REALM),
            )
            .await?;
        let r: KcRole = parse(raw.clone(), "role")?;
        Ok(role(r, raw))
    }

    async fn role_composites(&self, realm: &str, id: &str) -> Result<Vec<Role>> {
        let v = self
            .get(
                realm,
                &format!("/roles-by-id/{}/composites", seg(id)),
                item("role", VIEW_REALM),
            )
            .await?;
        Keycloak::roles_of(v, "roles")
    }

    async fn role_users(
        &self,
        realm: &str,
        scope: &RoleScope,
        name: &str,
        page: Page,
    ) -> Result<Paged<UserRow>> {
        let path =
            Q::default()
                .page(page)
                .on(&format!("{}/{}/users", roles_base(scope), seg(name)));
        self.paged(realm, path, page, item("role", VIEW_USERS), user_row, None)
            .await
    }

    async fn role_groups(
        &self,
        realm: &str,
        scope: &RoleScope,
        name: &str,
        page: Page,
    ) -> Result<Paged<Group>> {
        let path = Q::default()
            .b("briefRepresentation", true)
            .page(page)
            .on(&format!("{}/{}/groups", roles_base(scope), seg(name)));
        self.paged(realm, path, page, item("role", VIEW_USERS), group, None)
            .await
    }

    async fn create_role(&self, realm: &str, scope: &RoleScope, new: &NewRole) -> Result<Role> {
        let base = roles_base(scope);
        let mut body = json!({ "name": new.name });
        if !new.description.is_empty() {
            body["description"] = Value::String(new.description.clone());
        }
        let needs = match scope {
            RoleScope::Realm => MANAGE_REALM,
            RoleScope::Client(_) => MANAGE_CLIENTS,
        };
        self.write(Method::POST, realm, &base, Some(body), list("role", needs))
            .await?;
        let raw = self
            .get(
                realm,
                &format!("{base}/{}", seg(&new.name)),
                item("role", VIEW_REALM),
            )
            .await?;
        let r: KcRole = parse(raw.clone(), "role")?;
        Ok(role(r, raw))
    }

    async fn update_role(&self, realm: &str, r: &ObjRef, change: &Change) -> Result<()> {
        self.read_merge_write(
            realm,
            &format!("/roles-by-id/{}", seg(&r.id)),
            change,
            item("role", MANAGE_REALM),
        )
        .await
    }

    async fn delete_role(&self, realm: &str, r: &ObjRef) -> Result<()> {
        self.write(
            Method::DELETE,
            realm,
            &format!("/roles-by-id/{}", seg(&r.id)),
            None,
            item("role", MANAGE_REALM),
        )
        .await?;
        Ok(())
    }

    async fn clients(&self, realm: &str, search: &str, page: Page) -> Result<Paged<Client>> {
        let search = search.trim();
        let mut q = Q::default();
        if !search.is_empty() {
            q = q.s("clientId", search).b("search", true);
        }
        let path = q.page(page).on("/clients");
        self.paged(
            realm,
            path,
            page,
            list("clients", VIEW_CLIENTS),
            client,
            None,
        )
        .await
    }

    async fn client(&self, realm: &str, id: &str) -> Result<Client> {
        let raw = self
            .get(
                realm,
                &format!("/clients/{}", seg(id)),
                item("client", VIEW_CLIENTS),
            )
            .await?;
        let c: KcClient = parse(raw.clone(), "client")?;
        Ok(client(c, raw))
    }

    async fn client_secret(&self, realm: &str, id: &str) -> Result<Secret> {
        let v = self
            .get(
                realm,
                &format!("/clients/{}/client-secret", seg(id)),
                item("client", VIEW_CLIENTS),
            )
            .await?;
        let value = v.get("value").and_then(Value::as_str).unwrap_or_default();
        Ok(Secret(SecretString::from(value.to_string())))
    }

    async fn service_account_user(&self, realm: &str, id: &str) -> Result<UserRow> {
        let v = self
            .get(
                realm,
                &format!("/clients/{}/service-account-user", seg(id)),
                item("client", VIEW_CLIENTS),
            )
            .await?;
        Ok(parse::<KcUser>(v, "service account")?.row())
    }

    async fn client_sessions(&self, realm: &str, id: &str, page: Page) -> Result<Paged<Session>> {
        let total = self.client_session_count(realm, id).await.ok();
        let path = Q::default()
            .page(page)
            .on(&format!("/clients/{}/user-sessions", seg(id)));
        self.paged(
            realm,
            path,
            page,
            item("client", VIEW_USERS),
            session,
            total,
        )
        .await
    }

    async fn client_session_count(&self, realm: &str, id: &str) -> Result<u64> {
        let v = self
            .get(
                realm,
                &format!("/clients/{}/session-count", seg(id)),
                item("client", VIEW_CLIENTS),
            )
            .await?;
        Ok(count(&v))
    }

    async fn create_client(&self, realm: &str, c: &NewClient) -> Result<String> {
        let resp = self
            .write(
                Method::POST,
                realm,
                "/clients",
                Some(new_client_body(c)),
                list("client", MANAGE_CLIENTS),
            )
            .await?;
        self.created_id(resp).await
    }

    async fn update_client(&self, realm: &str, c: &ObjRef, change: &Change) -> Result<()> {
        self.read_merge_write(
            realm,
            &format!("/clients/{}", seg(&c.id)),
            change,
            item("client", MANAGE_CLIENTS),
        )
        .await
    }

    async fn delete_client(&self, realm: &str, c: &ObjRef) -> Result<()> {
        self.write(
            Method::DELETE,
            realm,
            &format!("/clients/{}", seg(&c.id)),
            None,
            item("client", MANAGE_CLIENTS),
        )
        .await?;
        Ok(())
    }

    async fn regenerate_secret(&self, realm: &str, c: &ObjRef) -> Result<Secret> {
        let resp = self
            .write(
                Method::POST,
                realm,
                &format!("/clients/{}/client-secret", seg(&c.id)),
                None,
                item("client", MANAGE_CLIENTS),
            )
            .await?;
        let v = self.json(resp).await?;
        let value = v.get("value").and_then(Value::as_str).unwrap_or_default();
        Ok(Secret(SecretString::from(value.to_string())))
    }

    async fn client_session_stats(&self, realm: &str) -> Result<Vec<ClientSessionStats>> {
        let v = self
            .get(
                realm,
                "/client-session-stats",
                list("session statistics", VIEW_REALM),
            )
            .await?;
        Ok(session_stats(&v))
    }

    async fn logout_all(&self, realm: &str) -> Result<LogoutAll> {
        let resp = self
            .write(
                Method::POST,
                realm,
                "/logout-all",
                None,
                list("log out all", MANAGE_USERS),
            )
            .await?;
        Ok(model::logout_all(&self.json(resp).await?))
    }

    async fn login_events(
        &self,
        realm: &str,
        q: &EventQuery,
        page: Page,
    ) -> Result<Paged<LoginEvent>> {
        let path = Q::default()
            .many("type", &q.types)
            .s("client", &q.client)
            .s("user", &q.user)
            .s("ipAddress", &q.ip)
            .s("dateFrom", &date(&q.date_from))
            .s("dateTo", &date(&q.date_to))
            .page(page)
            .on("/events");
        self.paged(
            realm,
            path,
            page,
            list("login events", VIEW_EVENTS),
            login_event,
            None,
        )
        .await
    }

    async fn admin_events(
        &self,
        realm: &str,
        q: &AdminEventQuery,
        page: Page,
    ) -> Result<Paged<AdminEvent>> {
        let path = Q::default()
            .many("operationTypes", &q.operation_types)
            .many("resourceTypes", &q.resource_types)
            .s("resourcePath", &q.resource_path)
            .s("authRealm", &q.auth_realm)
            .s("authClient", &q.auth_client)
            .s("authUser", &q.auth_user)
            .s("authIpAddress", &q.auth_ip)
            .s("dateFrom", &date(&q.date_from))
            .s("dateTo", &date(&q.date_to))
            .page(page)
            .on("/admin-events");
        self.paged(
            realm,
            path,
            page,
            list("admin events", VIEW_EVENTS),
            admin_event,
            None,
        )
        .await
    }

    async fn export(
        &self,
        realm: &str,
        scope: ExportScope,
        cancel: CancellationToken,
    ) -> Result<String> {
        let path = Q::default()
            .b("exportClients", scope.clients)
            .b("exportGroupsAndRoles", scope.groups_and_roles)
            .on("/partial-export");
        let work = async {
            let resp = self
                .write(
                    Method::POST,
                    realm,
                    &path,
                    None,
                    list("export", MANAGE_REALM),
                )
                .await?;
            self.json(resp).await
        };
        // Cancel drops the request future, so the work actually stops.
        let mut v = tokio::select! {
            _ = cancel.cancelled() => return Err(IdmError::Cancelled),
            v = work => v?,
        };
        mask::mask(&mut v);
        serde_json::to_string_pretty(&v).map_err(|e| IdmError::Other(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_queries_build_search_exact_attribute_and_enabled_terms() {
        let q = |search: &str, exact, enabled| {
            user_filters(&UserQuery {
                search: search.into(),
                exact,
                enabled,
            })
            .page(Page { first: 50, max: 50 })
            .on("/users")
        };
        assert_eq!(q("ali", false, None), "/users?search=ali&first=50&max=51");
        assert_eq!(
            q("alice.nguyen", true, Some(true)),
            "/users?username=alice.nguyen&exact=true&enabled=true&first=50&max=51"
        );
        assert_eq!(
            q("a@x.test", true, None),
            "/users?email=a%40x.test&exact=true&first=50&max=51"
        );
        assert_eq!(
            q("department:Engineering", false, Some(false)),
            "/users?q=department%3AEngineering&enabled=false&first=50&max=51"
        );
        assert_eq!(Q::default().on("/users/count"), "/users/count");
    }

    #[test]
    fn event_queries_repeat_their_multi_valued_filters() {
        let path = Q::default()
            .many("type", &["LOGIN".into(), "LOGIN_ERROR".into()])
            .s("dateFrom", "2026-11-01")
            .s("client", "")
            .page(Page { first: 0, max: 20 })
            .on("/events");
        assert_eq!(
            path,
            "/events?type=LOGIN&type=LOGIN_ERROR&dateFrom=2026-11-01&first=0&max=21"
        );
    }

    #[test]
    fn role_mapping_paths_follow_the_subject_and_scope() {
        assert_eq!(
            subject_base(&Subject::User("u 1".into())),
            "/users/u%201/role-mappings"
        );
        assert_eq!(
            subject_base(&Subject::Group("g1".into())),
            "/groups/g1/role-mappings"
        );
        assert_eq!(roles_base(&RoleScope::Realm), "/roles");
        assert_eq!(
            roles_base(&RoleScope::Client("c1".into())),
            "/clients/c1/roles"
        );
    }

    #[test]
    fn the_console_url_names_the_auth_realm_and_the_page() {
        let kc = Keycloak::new(KeycloakConfig {
            url: "http://127.0.0.1:8180/".into(),
            auth_realm: "master".into(),
            grant: Grant::ClientCredentials {
                client_id: "hs-admin".into(),
                secret: SecretString::from("s".to_string()),
            },
            tls: Tls::default(),
            timeout: Duration::from_secs(5),
        })
        .expect("provider");
        assert_eq!(
            kc.console_url("hs-demo", "/users/u1/settings"),
            "http://127.0.0.1:8180/admin/master/console/#/hs-demo/users/u1/settings"
        );
        assert_eq!(
            kc.admin("hs demo", "/users"),
            "/admin/realms/hs%20demo/users"
        );
    }
}
