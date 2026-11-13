//! The guard's decisions and the audit log's redaction (docs/tools/idm.md
//! §10): every write refused on a read-only tab before the provider sees
//! it, a source check that fails when a trait method is in neither list,
//! and audit lines that carry no secret, password or token.

use std::collections::BTreeSet;
use std::sync::Arc;

use secrecy::SecretString;

use super::tests_support::Fake;
use super::*;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
}

const PASSWORD: &str = "Temp-Pa55word-Never-Logged";

/// Calls every write once and returns the names it called, so the test
/// can check that list against [`WRITES`].
async fn every_write(g: &Guarded) -> Vec<(&'static str, Result<()>)> {
    let u = ObjRef::new("u1", "alice");
    let grp = ObjRef::new("g1", "/engineering");
    let cl = ObjRef::new("c1", "orders-api");
    let who = Subject::User("u1".into());
    let roles = [Role {
        name: "editor".into(),
        ..Default::default()
    }];
    let change = Change::default().set("email", "alice@example.test");
    let r = "hs-demo";
    vec![
        (
            "create_user",
            g.create_user(
                r,
                &NewUser {
                    username: "alice".into(),
                    ..Default::default()
                },
            )
            .await
            .map(|_| ()),
        ),
        ("update_user", g.update_user(r, &u, &change).await),
        ("set_user_enabled", g.set_user_enabled(r, &u, false).await),
        (
            "reset_password",
            g.reset_password(r, &u, SecretString::from(PASSWORD.to_string()), true)
                .await,
        ),
        ("delete_user", g.delete_user(r, &u).await),
        ("join_group", g.join_group(r, &u, &grp).await),
        ("leave_group", g.leave_group(r, &u, &grp).await),
        ("logout_user", g.logout_user(r, &u).await),
        (
            "end_session",
            g.end_session(r, &ObjRef::new("s1", "alice")).await,
        ),
        (
            "create_group",
            g.create_group(r, Some(&grp), "backend").await.map(|_| ()),
        ),
        ("update_group", g.update_group(r, &grp, &change).await),
        ("delete_group", g.delete_group(r, &grp).await),
        ("add_realm_roles", g.add_realm_roles(r, &who, &roles).await),
        (
            "remove_realm_roles",
            g.remove_realm_roles(r, &who, &roles).await,
        ),
        (
            "add_client_roles",
            g.add_client_roles(r, &who, &cl, &roles).await,
        ),
        (
            "remove_client_roles",
            g.remove_client_roles(r, &who, &cl, &roles).await,
        ),
        (
            "create_role",
            g.create_role(
                r,
                &RoleScope::Realm,
                &NewRole {
                    name: "auditor".into(),
                    description: String::new(),
                },
            )
            .await
            .map(|_| ()),
        ),
        (
            "update_role",
            g.update_role(r, &ObjRef::new("r1", "editor"), &change)
                .await,
        ),
        (
            "delete_role",
            g.delete_role(r, &ObjRef::new("r1", "editor")).await,
        ),
        (
            "create_client",
            g.create_client(
                r,
                &NewClient {
                    client_id: "hstest".into(),
                    ..Default::default()
                },
            )
            .await
            .map(|_| ()),
        ),
        ("update_client", g.update_client(r, &cl, &change).await),
        ("delete_client", g.delete_client(r, &cl).await),
        (
            "regenerate_secret",
            g.regenerate_secret(r, &cl).await.map(|_| ()),
        ),
        ("logout_all", g.logout_all(r).await.map(|_| ())),
    ]
}

fn guarded(gate: Arc<WriteGate>, fake: Arc<Fake>, audit: Option<Arc<AuditLog>>) -> Guarded {
    Guarded::new(
        fake,
        gate,
        audit,
        AuditContext {
            connection: "01JCONN".into(),
            server: "http://127.0.0.1:8180".into(),
        },
    )
}

/// The trait's method names, read from its source.
fn trait_methods() -> BTreeSet<String> {
    let src = include_str!("../provider.rs");
    let start = src
        .find("pub trait IdmProvider")
        .expect("the trait is in provider.rs");
    let body = &src[start..];
    let end = body.find("\n}\n").expect("the trait ends");
    body[..end]
        .split("fn ")
        .skip(1)
        .filter_map(|rest| {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

#[test]
fn every_trait_method_is_listed_as_a_read_or_a_write() {
    let methods = trait_methods();
    let listed: BTreeSet<String> = WRITES.iter().chain(READS).map(|s| s.to_string()).collect();
    let missing: Vec<_> = methods.difference(&listed).collect();
    assert!(
        missing.is_empty(),
        "{missing:?} must be added to WRITES (and guarded) or READS"
    );
    let stale: Vec<_> = listed.difference(&methods).collect();
    assert!(
        stale.is_empty(),
        "{stale:?} are listed but not in the trait"
    );
    assert!(
        WRITES.iter().all(|w| !READS.contains(w)),
        "a method is listed twice"
    );
}

#[test]
fn a_read_only_tab_refuses_every_write_before_the_provider_sees_it() {
    rt().block_on(async {
        let fake = Arc::new(Fake::default());
        let gate = WriteGate::new(false, true, "Production");
        let g = guarded(gate.clone(), fake.clone(), None);
        let results = every_write(&g).await;
        let called: BTreeSet<&str> = results.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            called,
            WRITES.iter().copied().collect::<BTreeSet<_>>(),
            "the test must call every listed write"
        );
        for (name, r) in &results {
            match r {
                Err(IdmError::ReadOnly(why)) => assert!(why.contains("Production"), "{why}"),
                other => panic!("{name} was not refused: {other:?}"),
            }
        }
        assert!(fake.hits.lock().is_empty(), "{:?}", fake.hits.lock());
        assert!(!g.capabilities().writes);
        // Reads still pass, including the client secret.
        assert!(
            g.users("hs-demo", &UserQuery::default(), Page::default())
                .await
                .is_ok()
        );
        assert!(g.client_secret("hs-demo", "c1").await.is_ok());
        // The session toggle opens writes for this tab, after which they
        // reach the provider.
        assert!(gate.can_unlock());
        assert!(gate.unlock());
        assert!(
            g.delete_user("hs-demo", &ObjRef::new("u1", "alice"))
                .await
                .is_ok()
        );
        assert!(fake.hits.lock().contains(&"delete_user"));
        gate.lock();
        assert!(
            g.delete_user("hs-demo", &ObjRef::new("u1", "alice"))
                .await
                .is_err()
        );
    });
}

#[test]
fn a_read_only_profile_cannot_be_unlocked() {
    let gate = WriteGate::new(true, true, "Production");
    assert!(!gate.can_unlock());
    assert!(!gate.unlock());
    assert!(gate.read_only());
    assert!(gate.refusal().is_some_and(|r| r.contains("profile")));
    assert!(WriteGate::open().refusal().is_none());
}

#[test]
fn audit_lines_name_fields_and_never_carry_a_secret() {
    rt().block_on(async {
        let dir = tempfile::tempdir().expect("dir");
        let log = Arc::new(AuditLog::in_workspace(dir.path()));
        let fake = Arc::new(Fake::default());
        let g = guarded(WriteGate::open(), fake.clone(), Some(log.clone()));
        let results = every_write(&g).await;
        assert!(results.iter().all(|(_, r)| r.is_ok()));
        // The password reached the provider and nowhere else.
        assert_eq!(*fake.password.lock(), PASSWORD);
        // A write the server refused is recorded too, with its words.
        let failing = Arc::new(Fake {
            fail: true,
            ..Default::default()
        });
        let g2 = guarded(WriteGate::open(), failing, Some(log.clone()));
        assert!(
            g2.reset_password(
                "hs-demo",
                &ObjRef::new("u1", "alice"),
                SecretString::from(PASSWORD.to_string()),
                false
            )
            .await
            .is_err()
        );
        let text = std::fs::read_to_string(dir.path().join("idm").join("audit.jsonl"))
            .expect("the audit file");
        assert_eq!(text.lines().count(), WRITES.len() + 1);
        for secret in [
            PASSWORD,
            "regenerated-client-secret-XYZ",
            "alice@example.test",
        ] {
            assert!(!text.contains(secret), "{secret} leaked into the audit log");
        }
        let entries = log.recent(100);
        let reset = entries
            .iter()
            .find(|e| e.verb == "reset-password" && e.outcome == "ok")
            .expect("the reset");
        assert_eq!(reset.fields, vec!["temporary"]);
        assert_eq!(reset.name, "alice");
        assert_eq!(reset.by, "hs-admin");
        assert_eq!(reset.connection, "01JCONN");
        let update = entries
            .iter()
            .find(|e| e.verb == "update" && e.object == "user");
        assert_eq!(update.expect("update").fields, vec!["email"]);
        assert!(entries[0].outcome.contains("409"), "{:?}", entries[0]);
        // A refused (read-only) write is not sent, so it is not recorded.
        let ro = guarded(
            WriteGate::new(true, false, ""),
            Arc::new(Fake::default()),
            Some(log.clone()),
        );
        assert!(ro.logout_all("hs-demo").await.is_err());
        assert_eq!(log.recent(100).len(), WRITES.len() + 1);
    });
}
