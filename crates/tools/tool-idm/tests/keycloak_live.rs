//! The Keycloak provider against a live server (docs/tools/idm.md §10).
//! Skipped unless `HS_KEYCLOAK` names one:
//!
//! ```text
//! HS_KEYCLOAK=http://127.0.0.1:8180
//! HS_KEYCLOAK_SECRET=<the hs-admin client's secret>
//! HS_KEYCLOAK_ADMIN=admin                      # optional: the master admin
//! HS_KEYCLOAK_ADMIN_PASSWORD=<its password>    # optional
//! ```
//!
//! The bench's `bench/.env` holds the values (`KEYCLOAK_HS_ADMIN_SECRET`,
//! `KEYCLOAK_ADMIN_USER`, `KEYCLOAK_ADMIN_PASSWORD`); they are never
//! written into the repository. The bench realm is `hs-demo` (25 users,
//! groups engineering > backend / frontend, realm roles viewer / editor /
//! admin, clients web-app, orders-api, hs-admin) and `hs-other`, which
//! hs-admin may not read.
//!
//! Everything the test creates is named `hstest-…` and removed at the
//! end, and anything a crashed run left behind is removed first.

use std::sync::Arc;
use std::time::Duration;

use hs_core::service_http::Tls;
use secrecy::SecretString;
use tool_idm::audit::{AuditContext, AuditLog};
use tool_idm::guarded::{Guarded, WriteGate};
use tool_idm::provider::keycloak::auth::Grant;
use tool_idm::provider::keycloak::{Keycloak, KeycloakConfig};
use tool_idm::provider::*;

const REALM: &str = "hs-demo";
const PREFIX: &str = "hstest-";
/// A temporary password the test sets; the data folder is searched for it.
const RESET_PASSWORD: &str = "Hstest-Temp-Pa55-Not-Kept";

type R<T> = std::result::Result<T, String>;

fn ck<T>(r: Result<T>, what: &str) -> R<T> {
    r.map_err(|e| format!("{what}: {e}"))
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime")
}

fn keycloak(url: &str, auth_realm: &str, grant: Grant) -> Keycloak {
    Keycloak::new(KeycloakConfig {
        url: url.into(),
        auth_realm: auth_realm.into(),
        grant,
        tls: Tls::default(),
        timeout: Duration::from_secs(15),
    })
    .expect("provider")
}

fn service_account(url: &str, secret: &str) -> Keycloak {
    keycloak(
        url,
        REALM,
        Grant::ClientCredentials {
            client_id: "hs-admin".into(),
            secret: SecretString::from(secret.to_string()),
        },
    )
}

fn admin(url: &str) -> Option<Keycloak> {
    let user = env("HS_KEYCLOAK_ADMIN")?;
    let password = env("HS_KEYCLOAK_ADMIN_PASSWORD")?;
    Some(keycloak(
        url,
        "master",
        Grant::Password {
            client_id: "admin-cli".into(),
            username: user,
            password: SecretString::from(password),
        },
    ))
}

/// Remove whatever an earlier run left: users, groups, clients and realm
/// roles named `hstest-…`.
async fn sweep(p: &dyn IdmProvider, roles_too: bool) {
    let page = Page { first: 0, max: 100 };
    let q = UserQuery {
        search: PREFIX.into(),
        ..Default::default()
    };
    if let Ok(users) = p.users(REALM, &q, page).await {
        for u in users.rows.iter().filter(|u| u.username.starts_with(PREFIX)) {
            let _ = p.delete_user(REALM, &u.as_ref()).await;
        }
    }
    if let Ok(groups) = p.groups(REALM, PREFIX, page).await {
        for g in groups.rows.iter().filter(|g| g.name.starts_with(PREFIX)) {
            let _ = p.delete_group(REALM, &g.as_ref()).await;
        }
    }
    if let Ok(clients) = p.clients(REALM, PREFIX, page).await {
        for c in clients
            .rows
            .iter()
            .filter(|c| c.client_id.starts_with(PREFIX))
        {
            let _ = p.delete_client(REALM, &c.as_ref()).await;
        }
    }
    if roles_too && let Ok(roles) = p.roles(REALM, &RoleScope::Realm, PREFIX, page).await {
        for r in roles.rows.iter().filter(|r| r.name.starts_with(PREFIX)) {
            let _ = p.delete_role(REALM, &r.as_ref()).await;
        }
    }
}

/// The reads of every phase-1 section on the bench realm.
async fn reads(p: &dyn IdmProvider) -> R<()> {
    let info = ck(p.connect().await, "connect")?;
    assert_eq!(info.product, "Keycloak");
    assert_eq!(info.signed_in_as, "hs-admin");
    assert!(
        !info.event_types.is_empty(),
        "the event types from serverinfo"
    );
    let caps = p.capabilities();
    assert!(caps.user_profile, "26 has the declarative user profile");
    assert!(caps.group_children);
    assert_eq!(caps.section(REALM, Section::Users), Availability::Yes);

    let realms = ck(p.realms().await, "realms")?;
    assert!(realms.iter().any(|r| r.name == REALM), "{realms:?}");
    let settings = ck(p.realm_settings(REALM).await, "realm settings")?;
    assert_eq!(settings.realm.name, REALM);
    assert!(
        settings
            .facts
            .iter()
            .any(|f| f.field == "accessTokenLifespan")
    );
    assert!(settings.events.as_ref().is_some_and(|e| e.events_enabled));

    // Users: a page of 20 with one more, the count, and the next page.
    let page = Page::of(20);
    let all = UserQuery::default();
    let first = ck(p.users(REALM, &all, page).await, "users")?;
    let total = ck(p.user_count(REALM, &all).await, "user count")?.unwrap_or(0);
    assert!(total >= 25, "the bench has 25 users, saw {total}");
    assert_eq!(first.rows.len(), 20);
    assert!(first.more);
    let second = ck(p.users(REALM, &all, page.next()).await, "users page 2")?;
    assert!(!second.rows.is_empty());
    assert!(second.rows.iter().all(|u| !first.rows.contains(u)));
    let alice = ck(
        p.users(
            REALM,
            &UserQuery {
                search: "alice.nguyen".into(),
                exact: true,
                enabled: None,
            },
            page,
        )
        .await,
        "exact search",
    )?;
    assert_eq!(alice.rows.len(), 1);
    let by_attr = ck(
        p.user_count(
            REALM,
            &UserQuery {
                search: "department:Engineering".into(),
                ..Default::default()
            },
        )
        .await,
        "attribute count",
    )?;
    assert!(by_attr.unwrap_or(0) > 0);
    let a = &alice.rows[0];
    let user = ck(p.user(REALM, &a.id).await, "user")?;
    assert!(
        user.profile
            .as_ref()
            .is_some_and(|pr| pr.manages("username"))
    );
    assert_eq!(user.attributes["department"], vec!["Engineering"]);
    ck(p.user_credentials(REALM, &a.id).await, "credentials")?;
    let groups = ck(p.user_groups(REALM, &a.id, page).await, "user groups")?;
    assert!(groups.rows.iter().any(|g| g.path == "/engineering/backend"));
    ck(p.user_sessions(REALM, &a.id).await, "user sessions")?;
    let maps = ck(
        p.role_mappings(REALM, &Subject::User(a.id.clone())).await,
        "role mappings",
    )?;
    assert!(!maps.realm.is_empty());
    let effective = ck(
        p.realm_role_mappings(REALM, &Subject::User(a.id.clone()), true)
            .await,
        "effective roles",
    )?;
    assert!(effective.len() >= maps.realm.len());
    ck(p.required_actions(REALM).await, "required actions")?;
    ck(p.unmanaged_policy(REALM).await, "unmanaged policy")?;

    // Groups: the top level with its count, then a level down.
    let top = ck(p.groups(REALM, "", page).await, "groups")?;
    assert_eq!(top.total, Some(top.rows.len() as u64));
    let eng = top
        .rows
        .iter()
        .find(|g| g.name == "engineering")
        .ok_or("no engineering group")?;
    assert_eq!(eng.sub_group_count, Some(2));
    let kids = ck(p.group_children(REALM, &eng.id, page).await, "children")?;
    assert_eq!(kids.rows.len(), 2);
    let found = ck(p.groups(REALM, "back", page).await, "group search")?;
    assert!(found.rows.iter().any(|g| g.path == "/engineering/backend"));
    ck(
        p.group_members(REALM, &kids.rows[0].id, page).await,
        "members",
    )?;

    // Roles and clients.
    let roles = ck(p.roles(REALM, &RoleScope::Realm, "", page).await, "roles")?;
    let power = roles
        .rows
        .iter()
        .find(|r| r.name == "power-user")
        .ok_or("no power-user role")?;
    assert!(power.composite);
    let parts = ck(p.role_composites(REALM, &power.id).await, "composites")?;
    assert!(!parts.is_empty());
    ck(
        p.role_users(REALM, &RoleScope::Realm, "editor", page).await,
        "role users",
    )?;
    let clients = ck(p.clients(REALM, "orders", page).await, "clients")?;
    let orders = clients
        .rows
        .iter()
        .find(|c| c.client_id == "orders-api")
        .ok_or("no orders-api")?;
    let client_roles = ck(
        p.roles(REALM, &RoleScope::Client(orders.id.clone()), "", page)
            .await,
        "client roles",
    )?;
    assert!(client_roles.rows.iter().any(|r| r.name == "orders:read"));
    let all_clients = ck(p.clients(REALM, "", Page::of(100)).await, "all clients")?;
    assert!(all_clients.rows.iter().any(|c| c.is_builtin(REALM)));

    // Sessions and events.
    let stats = ck(p.client_session_stats(REALM).await, "session stats")?;
    if let Some(s) = stats.iter().find(|s| s.active > 0) {
        let sessions = ck(
            p.client_sessions(REALM, &s.id, page).await,
            "client sessions",
        )?;
        assert!(!sessions.rows.is_empty());
    }
    let events = ck(
        p.login_events(REALM, &EventQuery::default(), page).await,
        "login events",
    )?;
    assert!(!events.rows.is_empty(), "the bench has login events");
    let typed = ck(
        p.login_events(
            REALM,
            &EventQuery {
                types: vec!["CLIENT_LOGIN".into()],
                ..Default::default()
            },
            page,
        )
        .await,
        "typed events",
    )?;
    assert!(typed.rows.iter().all(|e| e.kind == "CLIENT_LOGIN"));
    ck(
        p.admin_events(REALM, &AdminEventQuery::default(), page)
            .await,
        "admin events",
    )?;

    // Another realm this account has no rights in.
    match p.users("hs-other", &UserQuery::default(), page).await {
        Err(IdmError::Forbidden { realm, .. }) => assert_eq!(realm, "hs-other"),
        other => return Err(format!("hs-other should be refused: {other:?}")),
    }
    // The export, masked.
    let export = ck(
        p.export(REALM, ExportScope::default(), Default::default())
            .await,
        "export",
    );
    match export {
        Ok(text) => assert!(text.contains("orders-api")),
        // hs-admin has no manage-realm; the refusal names it.
        Err(e) => assert!(e.contains("manage-realm"), "{e}"),
    }
    Ok(())
}

/// Create, change and delete a throwaway user, group, client and client
/// role through the guarded provider, which writes the audit log.
async fn round_trip(p: &dyn IdmProvider) -> R<()> {
    let page = Page::of(20);
    // A user.
    let username = format!("{PREFIX}user");
    let id = ck(
        p.create_user(
            REALM,
            &NewUser {
                username: username.clone(),
                email: format!("{PREFIX}user@hs-demo.test"),
                first_name: "Hs".into(),
                last_name: "Test".into(),
                enabled: true,
                ..Default::default()
            },
        )
        .await,
        "create user",
    )?;
    let u = ObjRef::new(&id, &username);
    let fresh = ck(p.user(REALM, &id).await, "read user")?;
    let mut attrs = serde_json::Map::new();
    for (k, v) in &fresh.attributes {
        attrs.insert(k.clone(), serde_json::json!(v));
    }
    attrs.insert("department".into(), serde_json::json!(["QA"]));
    ck(
        p.update_user(
            REALM,
            &u,
            &Change::default()
                .set("firstName", "Edited")
                .set("attributes", serde_json::Value::Object(attrs)),
        )
        .await,
        "update user",
    )?;
    let back = ck(p.user(REALM, &id).await, "read back")?;
    assert_eq!(back.row.first_name, "Edited");
    assert_eq!(back.row.last_name, "Test", "a field not edited is kept");
    assert_eq!(
        back.attributes.get("department"),
        Some(&vec!["QA".to_string()])
    );
    ck(p.set_user_enabled(REALM, &u, false).await, "disable")?;
    assert!(!ck(p.user(REALM, &id).await, "read")?.row.enabled);
    assert_eq!(
        ck(p.user(REALM, &id).await, "read")?
            .attributes
            .get("department"),
        Some(&vec!["QA".to_string()]),
        "disabling keeps the attributes"
    );
    ck(p.set_user_enabled(REALM, &u, true).await, "enable")?;
    ck(
        p.reset_password(
            REALM,
            &u,
            SecretString::from(RESET_PASSWORD.to_string()),
            true,
        )
        .await,
        "reset password",
    )?;
    let creds = ck(p.user_credentials(REALM, &id).await, "credentials")?;
    assert!(creds.iter().any(|c| c.kind == "password"));
    assert!(
        ck(p.user(REALM, &id).await, "read")?
            .required_actions
            .contains(&"UPDATE_PASSWORD".to_string()),
        "a temporary password asks for a new one"
    );
    // A group, a sub-group, membership.
    let gname = format!("{PREFIX}group");
    let gid = ck(p.create_group(REALM, None, &gname).await, "create group")?;
    let g = ObjRef::new(&gid, format!("/{gname}"));
    let sub = ck(
        p.create_group(REALM, Some(&g), "child").await,
        "create sub-group",
    )?;
    let kids = ck(p.group_children(REALM, &gid, page).await, "children")?;
    assert!(kids.rows.iter().any(|k| k.id == sub));
    ck(
        p.update_group(
            REALM,
            &g,
            &Change::default().set("attributes", serde_json::json!({"costCentre": ["CC-999"]})),
        )
        .await,
        "update group",
    )?;
    assert_eq!(
        ck(p.group(REALM, &gid).await, "read group")?.attributes["costCentre"],
        vec!["CC-999"]
    );
    ck(p.join_group(REALM, &u, &g).await, "join")?;
    let members = ck(p.group_members(REALM, &gid, page).await, "members")?;
    assert!(members.rows.iter().any(|m| m.id == id));
    ck(p.leave_group(REALM, &u, &g).await, "leave")?;
    // A client, its secret, its role, a mapping of it.
    let cname = format!("{PREFIX}client");
    let cid = ck(
        p.create_client(
            REALM,
            &NewClient {
                client_id: cname.clone(),
                name: "HS Test".into(),
                service_accounts: true,
                redirect_uris: vec!["http://localhost:9/*".into()],
                ..Default::default()
            },
        )
        .await,
        "create client",
    )?;
    let c = ObjRef::new(&cid, &cname);
    ck(
        p.update_client(
            REALM,
            &c,
            &Change::default().set(
                "redirectUris",
                serde_json::json!(["http://localhost:9/*", "http://localhost:10/*"]),
            ),
        )
        .await,
        "update client",
    )?;
    let client = ck(p.client(REALM, &cid).await, "read client")?;
    assert_eq!(client.raw["redirectUris"].as_array().map(Vec::len), Some(2));
    assert_eq!(client.name, "HS Test", "a field not edited is kept");
    let before = ck(p.client_secret(REALM, &cid).await, "secret")?;
    let after = ck(p.regenerate_secret(REALM, &c).await, "regenerate")?;
    assert_ne!(before.expose(), after.expose());
    assert_eq!(
        ck(p.client_secret(REALM, &cid).await, "secret")?.expose(),
        after.expose()
    );
    let role = ck(
        p.create_role(
            REALM,
            &RoleScope::Client(cid.clone()),
            &NewRole {
                name: format!("{PREFIX}role"),
                description: "made by the live test".into(),
            },
        )
        .await,
        "create client role",
    )?;
    ck(
        p.update_role(
            REALM,
            &role.as_ref(),
            &Change::default().set("description", "edited"),
        )
        .await,
        "update role",
    )?;
    assert_eq!(
        ck(p.role(REALM, &role.id).await, "read role")?.description,
        "edited"
    );
    let who = Subject::User(id.clone());
    ck(
        p.add_client_roles(REALM, &who, &c, std::slice::from_ref(&role))
            .await,
        "assign client role",
    )?;
    let mapped = ck(
        p.client_role_mappings(REALM, &who, &cid, false).await,
        "client mappings",
    )?;
    assert!(mapped.iter().any(|r| r.id == role.id));
    ck(
        p.remove_client_roles(REALM, &who, &c, std::slice::from_ref(&role))
            .await,
        "unassign client role",
    )?;
    // A realm role mapping on the user (the bench's viewer role).
    let available = ck(
        p.available_realm_roles(REALM, &who).await,
        "available roles",
    )?;
    if let Some(viewer) = available.iter().find(|r| r.name == "viewer").cloned() {
        ck(
            p.add_realm_roles(REALM, &who, std::slice::from_ref(&viewer))
                .await,
            "assign realm role",
        )?;
        let now = ck(
            p.realm_role_mappings(REALM, &who, false).await,
            "realm mappings",
        )?;
        assert!(now.iter().any(|r| r.name == "viewer"));
        ck(
            p.remove_realm_roles(REALM, &who, &[viewer]).await,
            "unassign realm role",
        )?;
    }
    let sa = ck(p.service_account_user(REALM, &cid).await, "service account")?;
    assert!(sa.username.starts_with("service-account-"));
    ck(p.logout_user(REALM, &u).await, "log out")?;
    // And away again.
    ck(
        p.delete_role(REALM, &role.as_ref()).await,
        "delete client role",
    )?;
    ck(p.delete_client(REALM, &c).await, "delete client")?;
    ck(p.delete_group(REALM, &g).await, "delete group")?;
    ck(p.delete_user(REALM, &u).await, "delete user")?;
    match p.user(REALM, &id).await {
        Err(IdmError::NotFound { what: "user" }) => {}
        other => return Err(format!("the user should be gone: {other:?}")),
    }
    Ok(())
}

/// The audience gotcha (Keycloak 26.8): a client without the realm's
/// `kc-admin-api` client scope is refused every admin call, and the
/// provider says what to add.
async fn audience(p: &dyn IdmProvider, url: &str) -> R<()> {
    let cname = format!("{PREFIX}no-audience");
    let cid = ck(
        p.create_client(
            REALM,
            &NewClient {
                client_id: cname.clone(),
                service_accounts: true,
                standard_flow: false,
                ..Default::default()
            },
        )
        .await,
        "create audience client",
    )?;
    let secret = ck(p.client_secret(REALM, &cid).await, "audience secret")?;
    let bare = keycloak(
        url,
        REALM,
        Grant::ClientCredentials {
            client_id: cname.clone(),
            secret: SecretString::from(secret.expose().to_string()),
        },
    );
    let r = bare.connect().await;
    let users = bare
        .users(REALM, &UserQuery::default(), Page::default())
        .await;
    let _ = p.delete_client(REALM, &ObjRef::new(&cid, &cname)).await;
    let expected = IdmError::Audience {
        client_id: cname.clone(),
    };
    if r.as_ref().err() != Some(&expected) && users.as_ref().err() != Some(&expected) {
        return Err(format!(
            "a client without kc-admin-api should get the audience error: {r:?} / {users:?}"
        ));
    }
    Ok(())
}

/// Realm roles need manage-realm, which hs-admin lacks: through the
/// master admin when it is given.
async fn realm_role_round_trip(p: &dyn IdmProvider) -> R<()> {
    let name = format!("{PREFIX}realm-role");
    let role = ck(
        p.create_role(
            REALM,
            &RoleScope::Realm,
            &NewRole {
                name: name.clone(),
                description: String::new(),
            },
        )
        .await,
        "create realm role",
    )?;
    ck(
        p.update_role(
            REALM,
            &role.as_ref(),
            &Change::default().set("description", "edited by the live test"),
        )
        .await,
        "update realm role",
    )?;
    assert_eq!(
        ck(p.role(REALM, &role.id).await, "read realm role")?.description,
        "edited by the live test"
    );
    ck(
        p.delete_role(REALM, &role.as_ref()).await,
        "delete realm role",
    )?;
    Ok(())
}

/// The Offline Token kind: an offline token taken with the admin's
/// password (as an admin with OTP would take it in a browser), used as
/// the profile's secret, then revoked.
async fn offline(url: &str) -> R<()> {
    let (Some(user), Some(password)) =
        (env("HS_KEYCLOAK_ADMIN"), env("HS_KEYCLOAK_ADMIN_PASSWORD"))
    else {
        return Ok(());
    };
    let oidc = format!("{url}/realms/master/protocol/openid-connect");
    let http = reqwest::Client::new();
    let answer: serde_json::Value = http
        .post(format!("{oidc}/token"))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "admin-cli"),
            ("username", user.as_str()),
            ("password", password.as_str()),
            ("scope", "offline_access"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let token = answer["refresh_token"]
        .as_str()
        .ok_or("no offline token")?
        .to_string();
    let p = keycloak(
        url,
        "master",
        Grant::OfflineToken {
            client_id: "admin-cli".into(),
            token: SecretString::from(token.clone()),
        },
    );
    let info = ck(p.connect().await, "offline connect")?;
    // A refreshed offline token may carry no profile claims; it then
    // reads as the client it was issued through.
    assert!(
        info.signed_in_as == user || info.signed_in_as == "admin-cli",
        "{}",
        info.signed_in_as
    );
    let realms = ck(p.realms().await, "offline realms")?;
    assert!(realms.iter().any(|r| r.name == REALM));
    // Revoke it, so offline sessions do not pile up on the bench.
    let _ = http
        .post(format!("{oidc}/logout"))
        .form(&[
            ("client_id", "admin-cli"),
            ("refresh_token", token.as_str()),
        ])
        .send()
        .await;
    Ok(())
}

#[test]
fn keycloak_live() {
    let (Some(url), Some(secret)) = (env("HS_KEYCLOAK"), env("HS_KEYCLOAK_SECRET")) else {
        eprintln!("skipped: set HS_KEYCLOAK and HS_KEYCLOAK_SECRET to run against a server");
        return;
    };
    let data = tempfile::tempdir().expect("data folder");
    runtime().block_on(async {
        let log = Arc::new(AuditLog::in_workspace(data.path()));
        let guarded = Guarded::new(
            Arc::new(service_account(&url, &secret)),
            WriteGate::open(),
            Some(log.clone()),
            AuditContext {
                connection: "live-test".into(),
                server: url.clone(),
            },
        );
        let read = reads(&guarded).await;
        sweep(&guarded, false).await;
        let trip = round_trip(&guarded).await;
        let aud = audience(&guarded, &url).await;
        // hs-admin may not create realm roles; the refusal names the role.
        let refused = guarded
            .create_role(
                REALM,
                &RoleScope::Realm,
                &NewRole {
                    name: format!("{PREFIX}refused"),
                    description: String::new(),
                },
            )
            .await;
        sweep(&guarded, false).await;

        let mut admin_trip = Ok(());
        if let Some(a) = admin(&url) {
            let a = Guarded::new(
                Arc::new(a),
                WriteGate::open(),
                Some(log.clone()),
                AuditContext {
                    connection: "live-test-admin".into(),
                    server: url.clone(),
                },
            );
            admin_trip = async {
                let info = ck(a.connect().await, "admin connect")?;
                assert!(info.version.is_some(), "a master admin sees the version");
                assert_eq!(info.support, Support::Supported);
                let realms = ck(a.realms().await, "admin realms")?;
                assert!(realms.iter().any(|r| r.name == "hs-other"));
                sweep(&a, true).await;
                let r = realm_role_round_trip(&a).await;
                sweep(&a, true).await;
                let export = ck(
                    a.export(REALM, ExportScope::default(), Default::default())
                        .await,
                    "admin export",
                )?;
                assert!(export.contains("orders-api"));
                assert!(
                    !export.contains(&secret),
                    "the export shows the hs-admin secret"
                );
                a.disconnect().await;
                r?;
                offline(&url).await
            }
            .await;
        } else {
            eprintln!("the admin-password part skipped: set HS_KEYCLOAK_ADMIN and HS_KEYCLOAK_ADMIN_PASSWORD");
        }

        read.expect("reads");
        trip.expect("round trip");
        aud.expect("audience");
        match refused {
            Err(IdmError::Forbidden { needs, .. }) => assert_eq!(needs, Some("manage-realm")),
            other => panic!("hs-admin should be refused a realm role: {other:?}"),
        }
        admin_trip.expect("admin round trip");

        // The audit log names what changed and nothing secret.
        let entries = log.recent(500);
        assert!(entries.iter().any(|e| e.verb == "reset-password" && e.outcome == "ok"));
        assert!(entries.iter().any(|e| e.verb == "regenerate-secret"));
        assert!(
            entries
                .iter()
                .filter(|e| e.connection == "live-test")
                .all(|e| e.by == "hs-admin"),
            "every line says who wrote it"
        );
    });
    // Nothing secret anywhere in the data folder.
    let mut text = String::new();
    for entry in walk(data.path()) {
        text.push_str(&std::fs::read_to_string(entry).unwrap_or_default());
    }
    assert!(!text.is_empty(), "the audit log was written");
    for needle in [secret.as_str(), RESET_PASSWORD] {
        assert!(
            !text.contains(needle),
            "a secret leaked into the data folder"
        );
    }
    if let Some(pw) = env("HS_KEYCLOAK_ADMIN_PASSWORD") {
        assert!(!text.contains(&pw), "the admin password leaked");
    }
    assert!(!text.contains("eyJ"), "a token leaked into the data folder");
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn keycloak_live_wrong_secret_is_said_in_words() {
    let Some(url) = env("HS_KEYCLOAK") else {
        return;
    };
    runtime().block_on(async {
        let e = service_account(&url, "certainly-not-the-secret")
            .connect()
            .await
            .unwrap_err();
        assert_eq!(
            e,
            IdmError::Login("the client id or secret is wrong".into())
        );
        let nothing = keycloak(
            "http://127.0.0.1:9",
            REALM,
            Grant::ClientCredentials {
                client_id: "hs-admin".into(),
                secret: SecretString::from("x".to_string()),
            },
        );
        match nothing.connect().await {
            Err(IdmError::Network(w)) => assert!(w.contains("nothing answered"), "{w}"),
            other => panic!("a stopped server should be a network error: {other:?}"),
        }
        if let (Some(user), Some(_)) = (env("HS_KEYCLOAK_ADMIN"), env("HS_KEYCLOAK_ADMIN_PASSWORD"))
        {
            let wrong = keycloak(
                &url,
                "master",
                Grant::Password {
                    client_id: "admin-cli".into(),
                    username: user,
                    password: SecretString::from("wrong-password".to_string()),
                },
            );
            assert_eq!(
                wrong.connect().await.unwrap_err(),
                IdmError::Login("wrong username or password".into())
            );
        }
    });
}
