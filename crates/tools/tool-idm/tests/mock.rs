//! The Keycloak provider against a scripted HTTP server: the token
//! request's body, the 401-retry-once rule, the audience 401 of Keycloak
//! 26.8, 403 / 404 / 409 in words, the Keycloak 22 sub-group fallback and
//! read-merge-write keeping fields it did not edit. The answers follow
//! what the bench's Keycloak 26.8.2 returned for the same calls.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use hs_core::service_http::Tls;
use parking_lot::Mutex;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tool_idm::provider::keycloak::auth::Grant;
use tool_idm::provider::keycloak::{Keycloak, KeycloakConfig};
use tool_idm::provider::*;

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    auth: String,
    body: String,
}

type Handler = Arc<dyn Fn(&Seen, usize) -> (u16, String) + Send + Sync>;

/// A server that answers each request through `handler`, which also gets
/// how many requests came before it.
async fn serve(handler: Handler) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            let handler = handler.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head_end, length) = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                        let length = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (i + 4, length);
                    }
                };
                while buf.len() < head_end + length {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
                let mut first = head.lines().next().unwrap_or("").split(' ');
                let method = first.next().unwrap_or("").to_string();
                let path = first.next().unwrap_or("").to_string();
                let auth = head
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("authorization")
                            .then(|| v.trim().to_string())
                    })
                    .unwrap_or_default();
                let s = Seen {
                    method,
                    path,
                    auth,
                    body,
                };
                let n = {
                    let mut l = log.lock();
                    l.push(s.clone());
                    l.len() - 1
                };
                let (status, reply) = handler(&s, n);
                let location = if status == 201 {
                    format!("location: http://x{}/new-id-42\r\n", s.path)
                } else {
                    String::new()
                };
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n{location}content-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (format!("http://{addr}"), seen)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime")
}

fn jwt(claims: Value) -> String {
    let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
    format!(
        "{}.{}.sig",
        enc(br#"{"alg":"RS256"}"#),
        enc(claims.to_string().as_bytes())
    )
}

/// A token answer; `n` numbers the token, `admin_scope` gives it the
/// `kc-admin-api` scope Keycloak 26.8 checks.
fn token(n: usize, admin_scope: bool) -> String {
    let scope = if admin_scope {
        "kc-admin-api profile email"
    } else {
        "profile email"
    };
    let access = jwt(json!({
        "n": n,
        "scope": scope,
        "aud": "account",
        "azp": "hs-admin", "client_id": "hs-admin",
        "preferred_username": "service-account-hs-admin",
        "resource_access": {"realm-management": {"roles": ["view-users", "manage-users"]}}
    }));
    json!({"access_token": access, "expires_in": 300, "token_type": "Bearer"}).to_string()
}

fn provider(url: &str) -> Keycloak {
    Keycloak::new(KeycloakConfig {
        url: url.into(),
        auth_realm: "hs-demo".into(),
        grant: Grant::ClientCredentials {
            client_id: "hs-admin".into(),
            secret: SecretString::from("the-client-secret".to_string()),
        },
        tls: Tls::default(),
        timeout: Duration::from_secs(5),
    })
    .expect("provider")
}

const SERVERINFO: &str = r#"{"features":[{"name":"ORGANIZATION","enabled":true}],"enums":{"eventType":["LOGIN","LOGIN_ERROR"]}}"#;

#[test]
fn connect_sends_the_grant_reads_server_info_and_hints() {
    runtime().block_on(async {
        let (url, seen) = serve(Arc::new(|s: &Seen, _| {
            if s.path.ends_with("/token") {
                (200, token(1, true))
            } else if s.path == "/admin/serverinfo" {
                (200, SERVERINFO.into())
            } else {
                (404, "{}".into())
            }
        }))
        .await;
        let kc = provider(&url);
        let info = kc.connect().await.expect("connect");
        assert_eq!(info.signed_in_as, "hs-admin");
        // A realm-scoped account is not shown the version.
        assert_eq!(info.support, Support::Unknown);
        assert_eq!(info.event_types, vec!["LOGIN", "LOGIN_ERROR"]);
        let caps = kc.capabilities();
        assert!(caps.user_profile && caps.organizations);
        assert_eq!(caps.may("hs-demo", "manage-users"), Some(true));
        assert_eq!(caps.may("hs-demo", "view-events"), Some(false));
        let seen = seen.lock().clone();
        assert_eq!(
            seen[0].path,
            "/realms/hs-demo/protocol/openid-connect/token"
        );
        assert_eq!(
            seen[0].body,
            "grant_type=client_credentials&client_id=hs-admin&client_secret=the-client-secret"
        );
        assert!(seen[1].auth.starts_with("Bearer "), "{:?}", seen[1]);
    });
}

#[test]
fn a_401_renews_the_token_and_retries_once() {
    runtime().block_on(async {
        let admin_calls = Arc::new(Mutex::new(0usize));
        let calls = admin_calls.clone();
        let (url, seen) = serve(Arc::new(move |s: &Seen, n| {
            if s.path.ends_with("/token") {
                return (200, token(n, true));
            }
            // The first call's token is refused, as a revoked one is.
            let mut c = calls.lock();
            *c += 1;
            if *c == 1 {
                (401, r#"{"error":"HTTP 401 Unauthorized"}"#.into())
            } else {
                (200, "25".into())
            }
        }))
        .await;
        let kc = provider(&url);
        let n = kc
            .user_count("hs-demo", &UserQuery::default())
            .await
            .expect("count after one retry");
        assert_eq!(n, Some(25));
        let seen = seen.lock().clone();
        let tokens_asked = seen.iter().filter(|s| s.path.ends_with("/token")).count();
        let calls = seen
            .iter()
            .filter(|s| s.path.contains("/users/count"))
            .count();
        assert_eq!((tokens_asked, calls), (2, 2), "{seen:#?}");
    });
}

#[test]
fn a_second_401_without_the_admin_audience_names_the_missing_scope() {
    runtime().block_on(async {
        let (url, seen) = serve(Arc::new(|s: &Seen, n| {
            if s.path.ends_with("/token") {
                (200, token(n, false))
            } else {
                (401, String::new())
            }
        }))
        .await;
        let kc = provider(&url);
        let e = kc.connect().await.unwrap_err();
        assert_eq!(
            e,
            IdmError::Audience {
                client_id: "hs-admin".into()
            }
        );
        assert!(e.to_string().contains("kc-admin-api"));
        // Exactly one retry.
        let admin_calls = seen
            .lock()
            .iter()
            .filter(|s| s.path == "/admin/serverinfo")
            .count();
        assert_eq!(admin_calls, 2);
        // With the scope, a stubborn 401 is the credentials.
        let (url, _) = serve(Arc::new(|s: &Seen, n| {
            if s.path.ends_with("/token") {
                (200, token(n, true))
            } else {
                (401, String::new())
            }
        }))
        .await;
        assert_eq!(
            provider(&url).connect().await.unwrap_err(),
            IdmError::Unauthorized
        );
    });
}

#[test]
fn a_refused_token_request_is_said_in_words() {
    runtime().block_on(async {
        let (url, _) = serve(Arc::new(|_: &Seen, _| {
            (
                401,
                r#"{"error":"unauthorized_client","error_description":"Invalid client or Invalid client credentials"}"#.into(),
            )
        }))
        .await;
        let e = provider(&url).connect().await.unwrap_err();
        assert_eq!(e, IdmError::Login("the client id or secret is wrong".into()));
    });
}

#[test]
fn refusals_name_the_role_the_object_and_the_conflict() {
    runtime().block_on(async {
        let (url, _) = serve(Arc::new(|s: &Seen, n| {
            let p = s.path.as_str();
            if p.ends_with("/token") {
                (200, token(n, true))
            } else if p.starts_with("/admin/realms/hs-other/") {
                (403, r#"{"error":"HTTP 403 Forbidden"}"#.into())
            } else if p.starts_with("/admin/realms/hs-demo/users/gone") {
                (404, r#"{"error":"User not found"}"#.into())
            } else if p == "/admin/realms/hs-demo/users" && s.method == "POST" {
                (409, r#"{"errorMessage":"User exists with same username"}"#.into())
            } else if p.starts_with("/admin/realms/hs-demo/users/u1") && s.method == "GET" {
                (200, r#"{"id":"u1","username":"alice"}"#.into())
            } else if p.starts_with("/admin/realms/hs-demo/users/u1") && s.method == "PUT" {
                (
                    400,
                    r#"{"field":"email","errorMessage":"error-invalid-email","params":["email","x"]}"#.into(),
                )
            } else {
                (404, "{}".into())
            }
        }))
        .await;
        let kc = provider(&url);
        let e = kc
            .users("hs-other", &UserQuery::default(), Page::default())
            .await
            .unwrap_err();
        assert_eq!(
            e,
            IdmError::Forbidden {
                realm: "hs-other".into(),
                needs: Some("view-users")
            }
        );
        assert!(e.to_string().contains("view-users"));
        assert_eq!(
            kc.user("hs-demo", "gone").await.unwrap_err(),
            IdmError::NotFound { what: "user" }
        );
        let e = kc
            .create_user(
                "hs-demo",
                &NewUser {
                    username: "alice".into(),
                    enabled: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert_eq!(e, IdmError::Conflict("User exists with same username".into()));
        let e = kc
            .update_user(
                "hs-demo",
                &ObjRef::new("u1", "alice"),
                &Change::default().set("email", "x"),
            )
            .await
            .unwrap_err();
        assert_eq!(
            e,
            IdmError::Invalid {
                message: "error-invalid-email".into(),
                field: Some("email".into())
            }
        );
    });
}

#[test]
fn a_save_writes_the_whole_representation_back() {
    runtime().block_on(async {
        let current = json!({
            "id": "u1", "username": "alice", "email": "old@x", "enabled": true,
            "attributes": {"department": ["Engineering"], "employeeId": ["E1001"]},
            "fieldFromANewerKeycloak": {"kept": true}
        });
        let reply = current.to_string();
        let (url, seen) = serve(Arc::new(move |s: &Seen, n| {
            if s.path.ends_with("/token") {
                (200, token(n, true))
            } else if s.method == "GET" {
                (200, reply.clone())
            } else {
                (204, String::new())
            }
        }))
        .await;
        let kc = provider(&url);
        kc.update_user(
            "hs-demo",
            &ObjRef::new("u1", "alice"),
            &Change::default().set("email", "alice@x"),
        )
        .await
        .expect("save");
        kc.set_user_enabled("hs-demo", &ObjRef::new("u1", "alice"), false)
            .await
            .expect("disable");
        let seen = seen.lock().clone();
        let puts: Vec<Value> = seen
            .iter()
            .filter(|s| s.method == "PUT")
            .map(|s| serde_json::from_str(&s.body).expect("json"))
            .collect();
        assert_eq!(puts.len(), 2);
        assert_eq!(puts[0]["email"], "alice@x");
        // Attributes and unknown fields survive, as Keycloak 24+ needs.
        assert_eq!(puts[0]["attributes"]["employeeId"][0], "E1001");
        assert_eq!(puts[0]["fieldFromANewerKeycloak"]["kept"], true);
        assert_eq!(puts[1]["enabled"], false);
        assert_eq!(puts[1]["email"], "old@x", "only the edited field changes");
    });
}

#[test]
fn a_created_object_returns_the_id_its_location_names() {
    runtime().block_on(async {
        let (url, seen) = serve(Arc::new(|s: &Seen, n| {
            if s.path.ends_with("/token") {
                (200, token(n, true))
            } else if s.method == "POST" {
                (201, String::new())
            } else {
                (404, "{}".into())
            }
        }))
        .await;
        let kc = provider(&url);
        let id = kc
            .create_user(
                "hs-demo",
                &NewUser {
                    username: "hstest-mock".into(),
                    email: "m@x".into(),
                    enabled: true,
                    ..Default::default()
                },
            )
            .await
            .expect("create");
        assert_eq!(id, "new-id-42");
        let post = seen
            .lock()
            .iter()
            .find(|s| s.method == "POST" && s.path.ends_with("/users"))
            .cloned()
            .expect("post");
        let body: Value = serde_json::from_str(&post.body).expect("json");
        assert_eq!(body["username"], "hstest-mock");
        assert!(body.get("firstName").is_none(), "empty fields are left out");
    });
}

#[test]
fn a_keycloak_22_server_gives_sub_groups_in_the_parent() {
    runtime().block_on(async {
        let (url, seen) = serve(Arc::new(|s: &Seen, n| {
            let p = s.path.as_str();
            if p.ends_with("/token") {
                (200, token(n, true))
            } else if p.contains("/children") {
                // No such endpoint before 23.
                (
                    404,
                    r#"{"error":"Unable to find matching target resource method"}"#.into(),
                )
            } else if p.starts_with("/admin/realms/hs-demo/groups/g1") {
                (
                    200,
                    json!({"id": "g1", "name": "engineering", "path": "/engineering", "subGroups": [
                        {"id": "g2", "name": "backend", "path": "/engineering/backend"},
                        {"id": "g3", "name": "frontend", "path": "/engineering/frontend"}
                    ]})
                    .to_string(),
                )
            } else {
                (404, "{}".into())
            }
        }))
        .await;
        let kc = provider(&url);
        // Server info refused: the version is unknown, so /children is
        // tried first.
        kc.connect().await.expect("connect");
        assert!(kc.capabilities().group_children);
        let page = Page { first: 0, max: 20 };
        let kids = kc
            .group_children("hs-demo", "g1", page)
            .await
            .expect("children");
        assert_eq!(kids.rows.len(), 2);
        assert_eq!(kids.total, Some(2));
        assert!(!kids.more);
        assert!(!kc.capabilities().group_children, "hidden for the session");
        // The next call goes straight to the parent.
        kc.group_children("hs-demo", "g1", page)
            .await
            .expect("children");
        let tries = seen
            .lock()
            .iter()
            .filter(|s| s.path.contains("/children"))
            .count();
        assert_eq!(tries, 1);
    });
}

#[test]
fn an_export_is_masked_and_a_cancel_stops_it() {
    runtime().block_on(async {
        let (url, _) = serve(Arc::new(|s: &Seen, n| {
            if s.path.ends_with("/token") {
                (200, token(n, true))
            } else if s.path.contains("partial-export") {
                (
                    200,
                    json!({"realm": "hs-demo", "clients": [{"clientId": "orders-api", "secret": "planted-unmasked"}]}).to_string(),
                )
            } else {
                (404, "{}".into())
            }
        }))
        .await;
        let kc = provider(&url);
        let text = kc
            .export("hs-demo", ExportScope::default(), Default::default())
            .await
            .expect("export");
        assert!(text.contains("orders-api"));
        assert!(!text.contains("planted-unmasked"));
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        assert_eq!(
            kc.export("hs-demo", ExportScope::default(), cancel).await.unwrap_err(),
            IdmError::Cancelled
        );
    });
}
