//! Keycloak's tokens (docs/tools/idm.md §5.2): the three grants, the
//! access token held in memory only and renewed before it expires (a
//! third of its life left, at least 10 s), one renewal at a time, and the
//! token endpoint's refusals in words. The token's claims are read - not
//! verified - for permission hints and the audience check.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::Value;
use zeroize::Zeroizing;

use crate::provider::{IdmError, Result};

/// The client id the Admin Password kind uses when the profile names
/// none: Keycloak's public admin client.
pub const ADMIN_CLI: &str = "admin-cli";

/// The renewal floor: a token is renewed when less than this is left,
/// whatever its lifespan.
pub const RENEW_FLOOR: Duration = Duration::from_secs(10);

/// How the session gets its first token (§5.2's table).
#[derive(Clone)]
pub enum Grant {
    /// A confidential client's service account - the `api-key` kind.
    ClientCredentials {
        client_id: String,
        secret: SecretString,
    },
    /// An admin's username and password against `admin-cli` - the
    /// `password` kind.
    Password {
        client_id: String,
        username: String,
        password: SecretString,
    },
    /// An offline token obtained elsewhere, used as a refresh token - the
    /// `token` kind.
    OfflineToken {
        client_id: String,
        token: SecretString,
    },
}

/// Never prints a secret.
impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Grant::ClientCredentials { client_id, .. } => {
                write!(f, "ClientCredentials({client_id})")
            }
            Grant::Password {
                client_id,
                username,
                ..
            } => write!(f, "Password({username} via {client_id})"),
            Grant::OfflineToken { client_id, .. } => write!(f, "OfflineToken({client_id})"),
        }
    }
}

/// A form body. The values are zeroised when it is dropped.
pub type Form = Vec<(&'static str, Zeroizing<String>)>;

fn field(k: &'static str, v: &str) -> (&'static str, Zeroizing<String>) {
    (k, Zeroizing::new(v.to_string()))
}

impl Grant {
    pub fn client_id(&self) -> &str {
        match self {
            Grant::ClientCredentials { client_id, .. }
            | Grant::Password { client_id, .. }
            | Grant::OfflineToken { client_id, .. } => client_id,
        }
    }

    /// The first request's body.
    pub fn body(&self) -> Form {
        match self {
            Grant::ClientCredentials { client_id, secret } => vec![
                field("grant_type", "client_credentials"),
                field("client_id", client_id),
                field("client_secret", secret.expose_secret()),
            ],
            Grant::Password {
                client_id,
                username,
                password,
            } => vec![
                field("grant_type", "password"),
                field("client_id", client_id),
                field("username", username),
                field("password", password.expose_secret()),
            ],
            Grant::OfflineToken { client_id, token } => vec![
                field("grant_type", "refresh_token"),
                field("client_id", client_id),
                field("refresh_token", token.expose_secret()),
            ],
        }
    }

    /// A renewal with the refresh token the last answer gave.
    pub fn refresh_body(&self, refresh: &SecretString) -> Form {
        vec![
            field("grant_type", "refresh_token"),
            field("client_id", self.client_id()),
            field("refresh_token", refresh.expose_secret()),
        ]
    }

    fn what(&self) -> &'static str {
        match self {
            Grant::ClientCredentials { .. } => "client credentials",
            Grant::Password { .. } => "the password grant",
            Grant::OfflineToken { .. } => "an offline token",
        }
    }
}

/// The token endpoint's answer.
#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: u64,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// 0 for an offline token: it does not expire on a clock.
    #[serde(default)]
    pub refresh_expires_in: Option<u64>,
}

impl std::fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TokenResponse(expires_in: {})", self.expires_in)
    }
}

/// Where tokens come from: the realm's OpenID Connect endpoints, or a
/// fake in the tests.
#[async_trait]
pub trait TokenEndpoint: Send + Sync {
    async fn token(&self, form: &Form) -> Result<TokenResponse>;
    /// End the session the refresh token belongs to.
    async fn logout(&self, form: &Form) -> Result<()>;
}

/// A clock, so renewal timing can be tested without waiting.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// One access token, in memory only.
pub struct Token {
    pub access: SecretString,
    pub refresh: Option<SecretString>,
    pub obtained: Instant,
    pub lifetime: Duration,
    /// When the refresh token stops working; `None` for one with no clock
    /// (an offline token) or none at all.
    pub refresh_until: Option<Instant>,
    /// A 401 said the server no longer takes it.
    pub stale: bool,
}

impl Token {
    fn from_response(r: TokenResponse, now: Instant) -> Token {
        let refresh_until = match (r.refresh_token.is_some(), r.refresh_expires_in) {
            (true, Some(s)) if s > 0 => Some(now + Duration::from_secs(s)),
            _ => None,
        };
        Token {
            access: SecretString::from(r.access_token),
            refresh: r.refresh_token.map(SecretString::from),
            obtained: now,
            lifetime: Duration::from_secs(r.expires_in),
            refresh_until,
            stale: false,
        }
    }

    /// When it is renewed: once less than a third of its life, and at
    /// least [`RENEW_FLOOR`], is left.
    pub fn renew_at(&self) -> Instant {
        let margin = (self.lifetime / 3).max(RENEW_FLOOR);
        self.obtained + self.lifetime.saturating_sub(margin)
    }

    pub fn needs_renewal(&self, now: Instant) -> bool {
        self.stale || now >= self.renew_at()
    }

    fn refresh_usable(&self, now: Instant) -> Option<&SecretString> {
        let r = self.refresh.as_ref()?;
        match self.refresh_until {
            Some(until) if now + Duration::from_secs(5) >= until => None,
            _ => Some(r),
        }
    }
}

/// The token endpoint's refusal in words (§5.2).
pub fn token_error(grant: &Grant, status: u16, body: &str, realm: &str) -> IdmError {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let error = v.get("error").and_then(Value::as_str).unwrap_or("");
    let desc = v
        .get("error_description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let words = match (error, grant) {
        _ if status == 404 => format!(
            "no realm “{realm}” at this URL: check the auth realm, and add /auth to the base URL for a server older than Keycloak 17"
        ),
        // Keycloak 26 answers a wrong secret with unauthorized_client and
        // this description, older servers with invalid_client.
        ("invalid_client", _) => "the client id or secret is wrong".into(),
        (_, _) if desc.contains("Invalid client credentials") => {
            "the client id or secret is wrong".into()
        }
        ("unauthorized_client", Grant::ClientCredentials { .. }) => {
            "this client may not use client credentials: turn on Service Accounts for it (Clients → the client → Settings → Capability config)".into()
        }
        ("unauthorized_client", Grant::Password { .. }) => {
            "this client may not use the password grant: turn on Direct Access Grants for it".into()
        }
        ("invalid_grant", _) if desc.contains("not fully set up") => {
            "the admin has a required action pending (an OTP or a password change): sign in to the admin console once, or use a service-account client".into()
        }
        ("invalid_grant", Grant::Password { .. }) => "wrong username or password".into(),
        ("invalid_grant", Grant::OfflineToken { .. }) => {
            "the offline token has expired or was revoked: get a new one".into()
        }
        ("invalid_scope", _) => format!("the server refused the scope: {desc}"),
        _ if !desc.is_empty() => format!("the server refused {}: {desc}", grant.what()),
        _ if !error.is_empty() => format!("the server refused {}: {error}", grant.what()),
        _ => format!(
            "the token endpoint answered {status} to {}",
            grant.what()
        ),
    };
    IdmError::Login(words)
}

/// The session's tokens. One renewal at a time: concurrent callers wait
/// on the same lock and find the fresh token when they get it.
pub struct Tokens {
    endpoint: Arc<dyn TokenEndpoint>,
    grant: Grant,
    clock: Arc<dyn Clock>,
    current: tokio::sync::Mutex<Option<Token>>,
}

impl Tokens {
    pub fn new(endpoint: Arc<dyn TokenEndpoint>, grant: Grant, clock: Arc<dyn Clock>) -> Tokens {
        Tokens {
            endpoint,
            grant,
            clock,
            current: tokio::sync::Mutex::new(None),
        }
    }

    pub fn grant(&self) -> &Grant {
        &self.grant
    }

    /// A token good for the next request, renewed first when it is due.
    pub async fn bearer(&self) -> Result<SecretString> {
        let mut cur = self.current.lock().await;
        let now = self.clock.now();
        if let Some(t) = cur.as_ref()
            && !t.needs_renewal(now)
        {
            return Ok(t.access.clone());
        }
        let next = self.obtain(cur.as_ref(), now).await?;
        let access = next.access.clone();
        *cur = Some(next);
        Ok(access)
    }

    async fn obtain(&self, prev: Option<&Token>, now: Instant) -> Result<Token> {
        if let Some(refresh) = prev.and_then(|t| t.refresh_usable(now)) {
            match self.endpoint.token(&self.grant.refresh_body(refresh)).await {
                Ok(r) => return Ok(Token::from_response(r, self.clock.now())),
                // A refresh token that stopped working: the password kind
                // signs in again; the offline kind has nothing else.
                Err(e) if !matches!(self.grant, Grant::Password { .. }) => return Err(e),
                Err(_) => {}
            }
        }
        let r = self.endpoint.token(&self.grant.body()).await?;
        Ok(Token::from_response(r, self.clock.now()))
    }

    /// A 401 on `used`: the next [`Tokens::bearer`] renews, unless another
    /// request already did.
    pub async fn invalidate(&self, used: &SecretString) {
        let mut cur = self.current.lock().await;
        if let Some(t) = cur.as_mut()
            && t.access.expose_secret() == used.expose_secret()
        {
            t.stale = true;
        }
    }

    /// The current access token's claims, if there is one.
    pub async fn claims(&self) -> Option<Value> {
        let cur = self.current.lock().await;
        claims(cur.as_ref()?.access.expose_secret())
    }

    /// End the server-side session the password kind made, and forget
    /// the tokens.
    pub async fn logout(&self) {
        let token = self.current.lock().await.take();
        if let (Grant::Password { .. }, Some(t)) = (&self.grant, token)
            && let Some(refresh) = t.refresh.as_ref()
        {
            let form = vec![
                field("client_id", self.grant.client_id()),
                field("refresh_token", refresh.expose_secret()),
            ];
            if let Err(e) = self.endpoint.logout(&form).await {
                tracing::debug!("idm: the admin session could not be ended: {e}");
            }
        }
    }
}

/// A JWT's claims, decoded but **not verified**: hints only.
pub fn claims(jwt: &str) -> Option<Value> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The roles a token carries, by the client that grants them, plus `"*"`
/// for master's `admin` realm role. `None` when the token has no role
/// claims at all (a lightweight access token): no hints.
pub fn role_hints(claims: &Value) -> Option<BTreeMap<String, BTreeSet<String>>> {
    let resource = claims.get("resource_access").and_then(Value::as_object);
    let realm_roles = claims
        .get("realm_access")
        .and_then(|r| r.get("roles"))
        .and_then(Value::as_array);
    if resource.is_none() && realm_roles.is_none() {
        return None;
    }
    let mut out = BTreeMap::new();
    for (client, v) in resource.into_iter().flatten() {
        let roles = v
            .get("roles")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect();
        out.insert(client.clone(), roles);
    }
    if realm_roles
        .into_iter()
        .flatten()
        .any(|r| r.as_str() == Some("admin"))
    {
        out.insert("*".into(), BTreeSet::new());
    }
    Some(out)
}

/// Whether the token is meant for the admin API: the `kc-admin-api`
/// scope, or an audience that is an admin URL. Keycloak 26.8 refuses
/// (401) an admin call with a token that is neither.
pub fn has_admin_audience(claims: &Value) -> bool {
    let scoped = claims
        .get("scope")
        .and_then(Value::as_str)
        .is_some_and(|s| s.split_whitespace().any(|x| x == "kc-admin-api"));
    let aud = match claims.get("aud") {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    scoped || aud.iter().any(|a| a.contains("/admin/realms/"))
}

/// Who the token was issued to: a service account's client id, else
/// the username.
pub fn subject(claims: &Value) -> String {
    let s = |k: &str| {
        claims
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let user = s("preferred_username");
    if user.starts_with("service-account-") || user.is_empty() {
        let c = s("client_id");
        if c.is_empty() { s("azp") } else { c }
    } else {
        user
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FakeClock {
        base: Instant,
        secs: AtomicU64,
    }
    impl FakeClock {
        fn new() -> Arc<FakeClock> {
            Arc::new(FakeClock {
                base: Instant::now(),
                secs: AtomicU64::new(0),
            })
        }
        fn advance(&self, s: u64) {
            self.secs.fetch_add(s, Ordering::SeqCst);
        }
    }
    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.base + Duration::from_secs(self.secs.load(Ordering::SeqCst))
        }
    }

    /// Hands out numbered tokens and remembers each form's grant type.
    #[derive(Default)]
    struct FakeEndpoint {
        calls: Mutex<Vec<String>>,
        expires_in: u64,
        refresh: bool,
        refuse_refresh: bool,
        logouts: Mutex<usize>,
    }

    #[async_trait]
    impl TokenEndpoint for FakeEndpoint {
        async fn token(&self, form: &Form) -> Result<TokenResponse> {
            // Long enough for a racing caller to arrive while this waits.
            tokio::time::sleep(Duration::from_millis(20)).await;
            let grant = form
                .iter()
                .find(|(k, _)| *k == "grant_type")
                .map(|(_, v)| v.to_string())
                .unwrap_or_default();
            let n = {
                let mut c = self.calls.lock();
                c.push(grant.clone());
                c.len()
            };
            if grant == "refresh_token" && self.refuse_refresh {
                return Err(IdmError::Login("expired".into()));
            }
            Ok(TokenResponse {
                access_token: format!("access-{n}"),
                expires_in: self.expires_in,
                refresh_token: self.refresh.then(|| format!("refresh-{n}")),
                refresh_expires_in: Some(1800),
            })
        }
        async fn logout(&self, _: &Form) -> Result<()> {
            *self.logouts.lock() += 1;
            Ok(())
        }
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn password() -> Grant {
        Grant::Password {
            client_id: ADMIN_CLI.into(),
            username: "admin".into(),
            password: SecretString::from("pw".to_string()),
        }
    }

    #[test]
    fn each_grant_sends_its_own_body() {
        let pairs = |f: Form| {
            f.iter()
                .map(|(k, v)| format!("{k}={}", v.as_str()))
                .collect::<Vec<_>>()
                .join("&")
        };
        let cc = Grant::ClientCredentials {
            client_id: "hs-admin".into(),
            secret: SecretString::from("s".to_string()),
        };
        assert_eq!(
            pairs(cc.body()),
            "grant_type=client_credentials&client_id=hs-admin&client_secret=s"
        );
        assert_eq!(
            pairs(password().body()),
            "grant_type=password&client_id=admin-cli&username=admin&password=pw"
        );
        let off = Grant::OfflineToken {
            client_id: "admin-cli".into(),
            token: SecretString::from("off".to_string()),
        };
        assert_eq!(
            pairs(off.body()),
            "grant_type=refresh_token&client_id=admin-cli&refresh_token=off"
        );
        assert!(!format!("{cc:?}{:?}{off:?}", password()).contains("pw"));
    }

    #[test]
    fn a_token_renews_at_a_third_of_its_life_with_a_ten_second_floor() {
        let now = Instant::now();
        let t = |life: u64| Token {
            access: SecretString::from("a".to_string()),
            refresh: None,
            obtained: now,
            lifetime: Duration::from_secs(life),
            refresh_until: None,
            stale: false,
        };
        // 300 s: renewed with 100 s left, at 200 s.
        assert_eq!(t(300).renew_at(), now + Duration::from_secs(200));
        // Keycloak's default 60 s: a third is 20 s.
        assert_eq!(t(60).renew_at(), now + Duration::from_secs(40));
        // 15 s: a third is 5 s, under the floor, so 10 s are kept.
        assert_eq!(t(15).renew_at(), now + Duration::from_secs(5));
        assert!(!t(60).needs_renewal(now + Duration::from_secs(39)));
        assert!(t(60).needs_renewal(now + Duration::from_secs(40)));
    }

    #[test]
    fn one_renewal_serves_concurrent_requests_and_a_due_token_is_renewed() {
        rt().block_on(async {
            let clock = FakeClock::new();
            let ep = Arc::new(FakeEndpoint {
                expires_in: 60,
                ..Default::default()
            });
            let tokens = Arc::new(Tokens::new(
                ep.clone(),
                Grant::ClientCredentials {
                    client_id: "hs-admin".into(),
                    secret: SecretString::from("s".to_string()),
                },
                clock.clone(),
            ));
            let mut handles = Vec::new();
            for _ in 0..8 {
                let t = tokens.clone();
                handles.push(tokio::spawn(async move { t.bearer().await }));
            }
            for h in handles {
                let got = h.await.expect("task").expect("token");
                assert_eq!(got.expose_secret(), "access-1");
            }
            assert_eq!(ep.calls.lock().len(), 1, "one token request for all eight");
            clock.advance(39);
            assert_eq!(tokens.bearer().await.unwrap().expose_secret(), "access-1");
            clock.advance(1);
            // Client credentials have no refresh token: a new grant.
            assert_eq!(tokens.bearer().await.unwrap().expose_secret(), "access-2");
            assert_eq!(
                *ep.calls.lock(),
                vec!["client_credentials", "client_credentials"]
            );
        });
    }

    #[test]
    fn the_password_kind_refreshes_and_signs_in_again_when_the_refresh_fails() {
        rt().block_on(async {
            let clock = FakeClock::new();
            let ep = Arc::new(FakeEndpoint {
                expires_in: 60,
                refresh: true,
                ..Default::default()
            });
            let tokens = Tokens::new(ep.clone(), password(), clock.clone());
            tokens.bearer().await.unwrap();
            clock.advance(45);
            tokens.bearer().await.unwrap();
            assert_eq!(*ep.calls.lock(), vec!["password", "refresh_token"]);
            let refusing = Arc::new(FakeEndpoint {
                expires_in: 60,
                refresh: true,
                refuse_refresh: true,
                ..Default::default()
            });
            let tokens = Tokens::new(refusing.clone(), password(), clock.clone());
            tokens.bearer().await.unwrap();
            clock.advance(45);
            assert_eq!(tokens.bearer().await.unwrap().expose_secret(), "access-3");
            assert_eq!(
                *refusing.calls.lock(),
                vec!["password", "refresh_token", "password"]
            );
            // Closing the tab ends the admin session on the server.
            tokens.logout().await;
            assert_eq!(*refusing.logouts.lock(), 1);
        });
    }

    #[test]
    fn a_401_marks_the_token_stale_once() {
        rt().block_on(async {
            let clock = FakeClock::new();
            let ep = Arc::new(FakeEndpoint {
                expires_in: 300,
                ..Default::default()
            });
            let tokens = Tokens::new(ep.clone(), password(), clock);
            let first = tokens.bearer().await.unwrap();
            tokens.invalidate(&first).await;
            let second = tokens.bearer().await.unwrap();
            assert_eq!(second.expose_secret(), "access-2");
            // An old token's 401 does not throw away the new one.
            tokens.invalidate(&first).await;
            assert_eq!(tokens.bearer().await.unwrap().expose_secret(), "access-2");
        });
    }

    #[test]
    fn token_endpoint_refusals_are_said_in_words() {
        let cc = Grant::ClientCredentials {
            client_id: "hs-admin".into(),
            secret: SecretString::from("s".to_string()),
        };
        let words =
            |g: &Grant, status, body: &str| token_error(g, status, body, "hs-demo").to_string();
        assert_eq!(
            words(
                &cc,
                401,
                r#"{"error":"unauthorized_client","error_description":"Invalid client or Invalid client credentials"}"#
            ),
            "the client id or secret is wrong"
        );
        assert_eq!(
            words(&cc, 401, r#"{"error":"invalid_client"}"#),
            "the client id or secret is wrong"
        );
        assert!(
            words(&cc, 401, r#"{"error":"unauthorized_client","error_description":"Client not enabled to retrieve service account"}"#)
                .contains("Service Accounts")
        );
        assert_eq!(
            words(
                &password(),
                400,
                r#"{"error":"invalid_grant","error_description":"Invalid user credentials"}"#
            ),
            "wrong username or password"
        );
        assert!(
            words(
                &password(),
                400,
                r#"{"error":"invalid_grant","error_description":"Account is not fully set up"}"#
            )
            .contains("required action")
        );
        assert!(
            words(&cc, 404, r#"{"error":"Realm does not exist"}"#).contains("no realm “hs-demo”")
        );
        assert!(words(&cc, 500, "").contains("500"));
    }

    fn jwt(claims: &Value) -> String {
        let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.sig",
            enc(br#"{"alg":"RS256"}"#),
            enc(claims.to_string().as_bytes())
        )
    }

    #[test]
    fn claims_give_role_hints_the_audience_and_the_subject() {
        let c = serde_json::json!({
            "aud": ["http://127.0.0.1:8180/admin/realms/hs-demo", "realm-management"],
            "scope": "kc-admin-api profile email",
            "azp": "hs-admin", "client_id": "hs-admin",
            "preferred_username": "service-account-hs-admin",
            "resource_access": {"realm-management": {"roles": ["view-users", "manage-users"]}}
        });
        let back = claims(&jwt(&c)).expect("claims");
        let hints = role_hints(&back).expect("hints");
        assert!(hints["realm-management"].contains("manage-users"));
        assert!(has_admin_audience(&back));
        assert_eq!(subject(&back), "hs-admin");
        // A token without the scope or an admin audience: Keycloak 26.8
        // answers every admin call with 401.
        let bare = serde_json::json!({"aud": "account", "scope": "profile email", "preferred_username": "ops"});
        assert!(!has_admin_audience(&bare));
        assert_eq!(subject(&bare), "ops");
        // A lightweight token carries no role claims: no hints at all.
        assert_eq!(role_hints(&bare), None);
        // Master's admin role means everything.
        let admin = serde_json::json!({"realm_access": {"roles": ["admin"]}});
        assert!(role_hints(&admin).expect("hints").contains_key("*"));
        assert_eq!(claims("not-a-jwt"), None);
    }
}
