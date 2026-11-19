//! The LDAP provider against a live directory (docs/tools/idm.md §4.12,
//! FEAT-0117). Skipped unless `HS_LDAP` names one:
//!
//! ```text
//! HS_LDAP=ldap://127.0.0.1:1389
//! HS_LDAP_PASSWORD=<cn=admin's password>      # bench/.env LDAP_ADMIN_PASSWORD
//! ```
//!
//! The bench's `hs-openldap` holds `dc=hyperstack,dc=test`, seeded from
//! `bench/seed/data/ldap/seed.ldif`: 11 people under `ou=people` (teams
//! `engineering` and `support`), groups `backend`, `frontend`, `engineering`
//! (which nests the first two), `support`, `admins` (groupOfUniqueNames)
//! and `developers` (posixGroup). Everything the test creates is named
//! `hstest-…` and removed at the end, and anything a crashed run left
//! behind is removed first.

use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use tool_idm::audit::AuditContext;
use tool_idm::guarded::{Guarded, WriteGate};
use tool_idm::provider::ldap::{LdapConfig, LdapProvider};
use tool_idm::provider::*;

const BASE: &str = "dc=hyperstack,dc=test";
const ADMIN: &str = "cn=admin,dc=hyperstack,dc=test";
const PEOPLE: &str = "ou=people,dc=hyperstack,dc=test";
const GROUPS: &str = "ou=groups,dc=hyperstack,dc=test";
/// A password the test sets and then binds with.
const NEW_PASSWORD: &str = "Hstest-Ldap-Pa55-Not-Kept";

type R<T> = std::result::Result<T, String>;

fn config(url: &str, password: &str) -> LdapConfig {
    LdapConfig {
        url: url.into(),
        starttls: false,
        tls_verify: true,
        ca_file: String::new(),
        bind_dn: ADMIN.into(),
        password: Some(SecretString::from(password.to_string())),
        default_base: String::new(),
        users_base: PEOPLE.into(),
        groups_base: GROUPS.into(),
        user_filter: String::new(),
        group_filter: String::new(),
        page_size: 5,
        timeout: Duration::from_secs(15),
    }
}

fn names<T>(rows: &[T], f: impl Fn(&T) -> &str) -> Vec<String> {
    rows.iter().map(|r| f(r).to_string()).collect()
}

/// Remove whatever an earlier run left.
async fn sweep(p: &LdapProvider) {
    let q = UserQuery {
        search: "hstest".into(),
        ..Default::default()
    };
    if let Ok(users) = p.users(BASE, &q, Page::of(200)).await {
        for u in users.rows {
            let _ = p.delete_user(BASE, &ObjRef::new(u.id, u.username)).await;
        }
    }
    if let Ok(groups) = p.groups(BASE, "hstest", Page::of(200)).await {
        for g in groups.rows {
            let _ = p.delete_group(BASE, &ObjRef::new(g.id, g.name)).await;
        }
    }
}

#[test]
fn ldap_live_reads_and_writes_users_and_groups() -> R<()> {
    let (Ok(url), Ok(password)) = (std::env::var("HS_LDAP"), std::env::var("HS_LDAP_PASSWORD"))
    else {
        eprintln!("HS_LDAP / HS_LDAP_PASSWORD not set - skipped");
        return Ok(());
    };
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let p = LdapProvider::new(config(&url, &password))?;
        let info = p.connect().await.map_err(|e| e.to_string())?;
        assert_eq!(info.product, "OpenLDAP");
        assert_eq!(info.signed_in_as, ADMIN);
        let realms = p.realms().await.map_err(|e| e.to_string())?;
        assert!(realms.iter().any(|r| r.name == BASE), "{realms:?}");
        let caps = p.capabilities();
        assert!(caps.writes);
        assert!(matches!(
            caps.section(BASE, Section::Clients),
            Availability::No(_)
        ));
        assert!(matches!(
            caps.section(BASE, Section::Users),
            Availability::Yes
        ));
        sweep(&p).await;

        // Paging: 11 people (ci-bot is under ou=service, outside the base).
        let first = p
            .users(BASE, &UserQuery::default(), Page { first: 0, max: 5 })
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(first.rows.len(), 5);
        assert!(first.more);
        let last = p
            .users(BASE, &UserQuery::default(), Page { first: 10, max: 5 })
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(
            last.rows.len(),
            1,
            "{:?}",
            names(&last.rows, |r| &r.username)
        );
        assert!(!last.more);

        // Search: free text, and an attribute term.
        let ada = p
            .users(
                BASE,
                &UserQuery {
                    search: "lovelace".into(),
                    ..Default::default()
                },
                Page::of(50),
            )
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(names(&ada.rows, |r| &r.username), ["ada"]);
        let leads = p
            .users(
                BASE,
                &UserQuery {
                    search: "title:Engineer".into(),
                    ..Default::default()
                },
                Page::of(50),
            )
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(names(&leads.rows, |r| &r.username), ["alan"]);
        let ada_id = ada.rows[0].id.clone();
        let user = p.user(BASE, &ada_id).await.map_err(|e| e.to_string())?;
        assert_eq!(user.row.first_name, "Ada");
        assert!(user.attributes.contains_key("title"));
        assert!(
            !user.raw.to_string().contains("SSHA"),
            "a password is never read back"
        );

        // Membership both ways: groupOfNames by DN, posixGroup by uid.
        let of_ada = p
            .user_groups(BASE, &ada_id, Page::of(50))
            .await
            .map_err(|e| e.to_string())?;
        let of_ada = names(&of_ada.rows, |g| &g.name);
        assert!(of_ada.contains(&"backend".to_string()), "{of_ada:?}");
        assert!(of_ada.contains(&"developers".to_string()), "{of_ada:?}");
        let groups = p
            .groups(BASE, "", Page::of(50))
            .await
            .map_err(|e| e.to_string())?;
        let eng = groups
            .rows
            .iter()
            .find(|g| g.name == "engineering")
            .ok_or("no engineering group")?
            .id
            .clone();
        let kids = p
            .group_children(BASE, &eng, Page::of(50))
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(names(&kids.rows, |g| &g.name), ["backend", "frontend"]);
        let members = p
            .group_members(BASE, &eng, Page::of(50))
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(
            names(&members.rows, |u| &u.username),
            ["grace"],
            "groups are children, not members"
        );

        // A round trip of writes.
        let new_id = p
            .create_user(
                BASE,
                &NewUser {
                    username: "hstest-user".into(),
                    first_name: "Test".into(),
                    last_name: "User".into(),
                    email: "hstest@hyperstack.test".into(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| e.to_string())?;
        assert!(new_id.starts_with("uid=hstest-user,"), "{new_id}");
        let me = ObjRef::new(new_id.clone(), "hstest-user");
        p.update_user(
            BASE,
            &me,
            &Change::default().set("firstName", "Tested").set(
                "attributes",
                serde_json::json!({"cn": ["Test User"], "title": ["Probe"]}),
            ),
        )
        .await
        .map_err(|e| e.to_string())?;
        let read = p.user(BASE, &new_id).await.map_err(|e| e.to_string())?;
        assert_eq!(read.row.first_name, "Tested");
        assert_eq!(
            read.attributes.get("title"),
            Some(&vec!["Probe".to_string()])
        );
        // A rename moves the entry to its new RDN.
        p.update_user(
            BASE,
            &me,
            &Change::default().set("username", "hstest-renamed"),
        )
        .await
        .map_err(|e| e.to_string())?;
        let renamed_id = format!("uid=hstest-renamed,{PEOPLE}");
        let renamed = p.user(BASE, &renamed_id).await.map_err(|e| e.to_string())?;
        assert_eq!(renamed.row.username, "hstest-renamed");
        assert!(matches!(
            p.user(BASE, &new_id).await,
            Err(IdmError::NotFound { .. })
        ));
        let me = ObjRef::new(renamed_id.clone(), "hstest-renamed");
        // A Keycloak-only field is refused, not dropped.
        assert!(
            p.update_user(BASE, &me, &Change::default().set("enabled", false))
                .await
                .is_err()
        );

        // The new password binds.
        p.reset_password(
            BASE,
            &me,
            SecretString::from(NEW_PASSWORD.to_string()),
            false,
        )
        .await
        .map_err(|e| e.to_string())?;
        let mut as_user = config(&url, NEW_PASSWORD);
        as_user.bind_dn = renamed_id.clone();
        let them = LdapProvider::new(as_user)?;
        them.connect()
            .await
            .map_err(|e| format!("the new password does not bind: {e}"))?;
        them.disconnect().await;

        // A group, joined and left; deleting the user cleans up after it.
        let gid = p
            .create_group(BASE, None, "hstest-group")
            .await
            .map_err(|e| e.to_string())?;
        let group = ObjRef::new(gid.clone(), "hstest-group");
        p.join_group(BASE, &me, &group)
            .await
            .map_err(|e| e.to_string())?;
        let mine = p
            .user_groups(BASE, &renamed_id, Page::of(50))
            .await
            .map_err(|e| e.to_string())?;
        assert!(names(&mine.rows, |g| &g.name).contains(&"hstest-group".to_string()));
        p.leave_group(BASE, &me, &group)
            .await
            .map_err(|e| e.to_string())?;
        p.join_group(BASE, &me, &group)
            .await
            .map_err(|e| e.to_string())?;
        p.delete_user(BASE, &me).await.map_err(|e| e.to_string())?;
        let left = p.group(BASE, &gid).await.map_err(|e| e.to_string())?;
        assert!(
            !left.raw.to_string().contains("hstest-renamed"),
            "the deleted user's membership went with it"
        );
        p.delete_group(BASE, &group)
            .await
            .map_err(|e| e.to_string())?;
        assert!(matches!(
            p.group(BASE, &gid).await,
            Err(IdmError::NotFound { .. })
        ));

        // The Directory section (part two): the tree, an entry, an edit,
        // a move and a delete.
        let top = p
            .dir_children(BASE, BASE, Page::of(50))
            .await
            .map_err(|e| e.to_string())?;
        let tops: Vec<String> = top.rows.iter().map(|e| e.rdn.clone()).collect();
        assert!(tops.contains(&"ou=people".to_string()), "{tops:?}");
        assert!(
            top.rows.iter().all(|e| e.has_children),
            "every top OU holds entries"
        );
        let people = p
            .dir_children(BASE, PEOPLE, Page::of(50))
            .await
            .map_err(|e| e.to_string())?;
        // Containers first: engineering and support before tim.
        let order: Vec<String> = people.rows.iter().map(|e| e.rdn.clone()).collect();
        assert_eq!(
            order.last().map(String::as_str),
            Some("uid=tim"),
            "{order:?}"
        );
        let probe_id = p
            .create_user(
                BASE,
                &NewUser {
                    username: "hstest-dir".into(),
                    last_name: "Dir".into(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| e.to_string())?;
        let probe = ObjRef::new(probe_id.clone(), "uid=hstest-dir");
        let mut entry = p
            .dir_entry(BASE, &probe_id)
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(entry.kind(), DirKind::Person);
        assert!(!entry.has_children);
        assert!(
            !entry.attributes.contains_key("objectClass")
                && !entry.attributes.contains_key("userPassword")
        );
        entry
            .attributes
            .insert("description".into(), vec!["probe".into()]);
        p.dir_modify(BASE, &probe, &entry.attributes)
            .await
            .map_err(|e| e.to_string())?;
        let read = p
            .dir_entry(BASE, &probe_id)
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(
            read.attributes.get("description"),
            Some(&vec!["probe".to_string()])
        );
        // Renaming through the editor is refused: the RDN keeps its value.
        let mut renamed = read.attributes.clone();
        renamed.insert("uid".into(), vec!["hstest-other".into()]);
        assert!(p.dir_modify(BASE, &probe, &renamed).await.is_err());
        let support = format!("ou=support,{PEOPLE}");
        let moved = p
            .dir_move(BASE, &probe, &support)
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(moved, format!("uid=hstest-dir,{support}"));
        assert!(p.dir_entry(BASE, &moved).await.is_ok());
        // An entry with children is not deleted; a move under itself is refused.
        assert!(
            p.dir_delete(BASE, &ObjRef::new(PEOPLE, "ou=people"))
                .await
                .is_err()
        );
        assert!(
            p.dir_move(BASE, &ObjRef::new(PEOPLE, "ou=people"), &support)
                .await
                .is_err()
        );
        p.dir_delete(BASE, &ObjRef::new(moved.clone(), "uid=hstest-dir"))
            .await
            .map_err(|e| e.to_string())?;
        assert!(matches!(
            p.dir_entry(BASE, &moved).await,
            Err(IdmError::NotFound { .. })
        ));

        // A read-only tab refuses before anything is sent.
        let guarded = Guarded::new(
            Arc::new(LdapProvider::new(config(&url, &password))?),
            WriteGate::new(true, false, ""),
            None,
            AuditContext {
                connection: "test".into(),
                server: url.clone(),
            },
        );
        let refused = guarded
            .create_user(
                BASE,
                &NewUser {
                    username: "hstest-never".into(),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(refused, Err(IdmError::ReadOnly(_))), "{refused:?}");
        sweep(&p).await;
        p.disconnect().await;
        Ok(())
    })
}
