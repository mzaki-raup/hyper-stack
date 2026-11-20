//! The LDAP provider (docs/tools/idm.md §4.12, ADR-033): any LDAP v3
//! directory - OpenLDAP, 389 DS / FreeIPA, OpenDJ - through `ldap3` on the
//! runtime. A realm is a naming context (a base DN from the Root DSE);
//! users and groups are the shared screens; roles, clients, sessions,
//! events and realm settings are switched off through [`Caps`], with the
//! reason. Active Directory mode (FEAT-0118) builds on this.

pub mod map;

use std::collections::{BTreeSet, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use ldap3::adapters::{Adapter, EntriesOnly, PagedResults};
use ldap3::exop::PasswordModify;
use ldap3::{LdapConnAsync, LdapConnSettings, LdapError, Mod, Scope, SearchEntry};
use secrecy::{ExposeSecret, SecretString};
use tokio_util::sync::CancellationToken;

use crate::provider::{
    AdminEvent, AdminEventQuery, Caps, Change, Client, ClientSessionStats, CredentialInfo,
    DirEntry, EventQuery, EventsConfig, ExportScope, Group, IdmError, IdmProvider, LoginEvent,
    LogoutAll, NewClient, NewRole, NewUser, ObjRef, Page, Paged, Realm, RealmSettings,
    RequiredAction, Result, Role, RoleMappings, RoleScope, Secret, Section, ServerInfo, Session,
    Subject, UnmanagedPolicy, User, UserQuery, UserRow,
};
use map::{Entry, ModOp};

/// The Password Modify extended operation (RFC 3062).
const PASSWORD_MODIFY_OID: &str = "1.3.6.1.4.1.4203.1.11.1";
/// What the screens say for the sections a directory does not have.
const NO_SUCH_SECTION: &str = "a directory has no roles, clients, sessions or events";

/// Everything the provider needs, resolved from the profile (§4.12).
#[derive(Clone)]
pub struct LdapConfig {
    /// `ldap://host:389` or `ldaps://host:636`.
    pub url: String,
    pub starttls: bool,
    pub tls_verify: bool,
    pub ca_file: String,
    /// Empty for an anonymous bind, which is read-only.
    pub bind_dn: String,
    pub password: Option<SecretString>,
    /// The naming context the tab opens on; empty for the server's first.
    pub default_base: String,
    /// Where users are searched and created; empty for the whole realm.
    pub users_base: String,
    pub groups_base: String,
    pub user_filter: String,
    pub group_filter: String,
    /// Entries a search asks for at a time (the paged-results size).
    pub page_size: u32,
    pub timeout: Duration,
}

impl std::fmt::Debug for LdapConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LdapConfig")
            .field("url", &self.url)
            .field("bind_dn", &self.bind_dn)
            .field("password", &self.password.as_ref().map(|_| "••••••••"))
            .finish_non_exhaustive()
    }
}

pub struct LdapProvider {
    cfg: LdapConfig,
    conn: tokio::sync::Mutex<Option<ldap3::Ldap>>,
    info: parking_lot::Mutex<ServerInfo>,
    contexts: parking_lot::Mutex<Vec<String>>,
    password_modify: AtomicBool,
}

impl LdapProvider {
    pub fn new(cfg: LdapConfig) -> std::result::Result<LdapProvider, String> {
        let url = cfg.url.trim();
        if !(url.starts_with("ldap://") || url.starts_with("ldaps://")) {
            return Err(format!("“{url}” is not an ldap:// or ldaps:// URL"));
        }
        Ok(LdapProvider {
            cfg,
            conn: tokio::sync::Mutex::new(None),
            info: parking_lot::Mutex::new(ServerInfo::default()),
            contexts: parking_lot::Mutex::new(Vec::new()),
            password_modify: AtomicBool::new(false),
        })
    }

    fn anonymous(&self) -> bool {
        self.cfg.bind_dn.trim().is_empty()
    }

    /// A bound handle: the open one, or a new connection.
    async fn handle(&self) -> Result<ldap3::Ldap> {
        let mut slot = self.conn.lock().await;
        if let Some(l) = slot.as_mut()
            && !l.is_closed()
        {
            return Ok(l.clone());
        }
        let l = self.open().await?;
        *slot = Some(l.clone());
        Ok(l)
    }

    async fn open(&self) -> Result<ldap3::Ldap> {
        let tls = self.cfg.url.starts_with("ldaps://") || self.cfg.starttls;
        let mut settings = LdapConnSettings::new()
            .set_conn_timeout(self.cfg.timeout)
            .set_starttls(self.cfg.starttls && self.cfg.url.starts_with("ldap://"));
        if tls {
            let config = hs_core::tls::client_config(self.cfg.tls_verify, &self.cfg.ca_file)
                .map_err(IdmError::Network)?;
            settings = settings.set_config(config);
        }
        let (conn, mut ldap) = LdapConnAsync::with_settings(settings, self.cfg.url.trim())
            .await
            .map_err(|e| network(&e))?;
        ldap3::drive!(conn);
        if !self.anonymous() {
            let pw = self
                .cfg
                .password
                .as_ref()
                .map(|p| p.expose_secret().to_string())
                .unwrap_or_default();
            ldap.with_timeout(self.cfg.timeout)
                .simple_bind(self.cfg.bind_dn.trim(), &pw)
                .await
                .map_err(|e| network(&e))?
                .success()
                .map_err(|e| ldap_err(e, "entry"))?;
        }
        Ok(ldap)
    }

    /// Up to `limit` entries, asked for a page at a time; the search is
    /// abandoned once there are enough.
    async fn search(
        &self,
        base: &str,
        scope: Scope,
        filter: &str,
        attrs: &[&str],
        limit: usize,
    ) -> Result<Vec<Entry>> {
        let mut ldap = self.handle().await?;
        let size = self.cfg.page_size.clamp(1, 1000) as i32;
        let adapters: Vec<Box<dyn Adapter<_, _>>> = vec![
            Box::new(EntriesOnly::new()),
            Box::new(PagedResults::new(size)),
        ];
        let mut stream = ldap
            .with_timeout(self.cfg.timeout)
            .streaming_search_with(adapters, base, scope, filter, attrs.to_vec())
            .await
            .map_err(|e| ldap_err(e, "entry"))?;
        let mut out = Vec::new();
        while out.len() < limit {
            match stream.next().await.map_err(|e| ldap_err(e, "entry"))? {
                Some(re) => {
                    let se = SearchEntry::construct(re);
                    out.push(Entry {
                        dn: se.dn,
                        attrs: se.attrs,
                    });
                }
                None => break,
            }
        }
        if out.len() >= limit {
            let id = stream.ldap_handle().last_id();
            let _ = stream.ldap_handle().abandon(id).await;
        } else {
            let res = stream.finish().await;
            // 32: the base is gone; 4: the server's size limit, which still
            // gave us what it had.
            if res.rc != 0 && res.rc != 4 {
                return Err(ldap_err(LdapError::LdapResult { result: res }, "entry"));
            }
        }
        Ok(out)
    }

    /// One entry by DN, or `NotFound`.
    async fn read(&self, dn: &str, what: &'static str) -> Result<Entry> {
        let mut attrs = vec!["*"];
        attrs.extend(map::OPERATIONAL);
        self.search(dn, Scope::Base, "(objectClass=*)", &attrs, 1)
            .await
            .map_err(|e| match e {
                IdmError::NotFound { .. } => IdmError::NotFound { what },
                e => e,
            })?
            .into_iter()
            .next()
            .ok_or(IdmError::NotFound { what })
    }

    /// A page of a search: `page.first` skipped, one extra asked for so
    /// the status line knows there is more (§4.1). A directory does not
    /// count.
    async fn page(
        &self,
        base: &str,
        filter: &str,
        attrs: &[&str],
        page: Page,
    ) -> Result<Paged<Entry>> {
        let want = page.first as usize + page.max as usize + 1;
        let all = self
            .search(base, Scope::Subtree, filter, attrs, want)
            .await?;
        let rows: Vec<Entry> = all.into_iter().skip(page.first as usize).collect();
        Ok(Paged::from_probe(rows, page, None))
    }

    /// Where users live in `realm`: the profile's base when it is inside
    /// the realm, else the realm itself.
    fn users_base(&self, realm: &str) -> String {
        inside(&self.cfg.users_base, realm)
    }

    fn groups_base(&self, realm: &str) -> String {
        inside(&self.cfg.groups_base, realm)
    }

    async fn modify(&self, dn: &str, mods: Vec<ModOp>) -> Result<()> {
        if mods.is_empty() {
            return Ok(());
        }
        let mods: Vec<Mod<String>> = mods
            .into_iter()
            .map(|m| match m {
                ModOp::Replace(a, v) => Mod::Replace(a, v.into_iter().collect()),
                ModOp::Delete(a) => Mod::Delete(a, HashSet::new()),
            })
            .collect();
        let mut ldap = self.handle().await?;
        ldap.with_timeout(self.cfg.timeout)
            .modify(dn, mods)
            .await
            .map_err(|e| network(&e))?
            .success()
            .map_err(|e| ldap_err(e, "entry"))?;
        Ok(())
    }

    async fn rename(&self, dn: &str, attr: &str, value: &str) -> Result<String> {
        let new_rdn = format!("{attr}={}", ldap3::dn_escape(value));
        let mut ldap = self.handle().await?;
        ldap.with_timeout(self.cfg.timeout)
            .modifydn(dn, &new_rdn, true, None)
            .await
            .map_err(|e| network(&e))?
            .success()
            .map_err(|e| ldap_err(e, "entry"))?;
        let parent = map::parent(dn);
        Ok(if parent.is_empty() {
            new_rdn
        } else {
            format!("{new_rdn},{parent}")
        })
    }

    /// A membership value added to or removed from a group, in the
    /// attribute that group keeps its members in.
    async fn membership(&self, group: &str, member: &Entry, add: bool) -> Result<()> {
        let g = self.read(group, "group").await?;
        let (attr, by_dn) = map::member_attr(&g);
        let value = if by_dn {
            member.dn.clone()
        } else {
            map::username(member)
        };
        let set: HashSet<String> = [value].into_iter().collect();
        let m = if add {
            Mod::Add(attr.to_string(), set)
        } else {
            Mod::Delete(attr.to_string(), set)
        };
        let mut ldap = self.handle().await?;
        ldap.with_timeout(self.cfg.timeout)
            .modify(group, vec![m])
            .await
            .map_err(|e| network(&e))?
            .success()
            .map_err(|e| ldap_err(e, "group"))?;
        Ok(())
    }

    /// Every group that lists `e`, so a deleted entry leaves no dangling
    /// member behind (servers without the referential-integrity overlay).
    async fn drop_from_groups(&self, realm: &str, e: &Entry) -> Result<()> {
        let filter = map::member_of_filter(&self.cfg.group_filter, &e.dn, &map::username(e));
        let groups = self
            .search(
                &self.groups_base(realm),
                Scope::Subtree,
                &filter,
                &["objectClass", "cn"],
                usize::MAX,
            )
            .await?;
        for g in groups {
            let _ = self.membership(&g.dn, e, false).await;
        }
        Ok(())
    }

    async fn delete(&self, dn: &str, what: &'static str) -> Result<()> {
        let mut ldap = self.handle().await?;
        ldap.with_timeout(self.cfg.timeout)
            .delete(dn)
            .await
            .map_err(|e| network(&e))?
            .success()
            .map_err(|e| ldap_err(e, what))?;
        Ok(())
    }

    async fn add(&self, dn: &str, attrs: Vec<(String, Vec<String>)>) -> Result<()> {
        let attrs: Vec<(String, HashSet<String>)> = attrs
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();
        let mut ldap = self.handle().await?;
        ldap.with_timeout(self.cfg.timeout)
            .add(dn, attrs)
            .await
            .map_err(|e| network(&e))?
            .success()
            .map_err(|e| ldap_err(e, "entry"))?;
        Ok(())
    }
}

/// `base` when it is inside `realm`, else `realm`.
fn inside(base: &str, realm: &str) -> String {
    let base = base.trim();
    if !base.is_empty() && map::is_under(base, realm) {
        base.to_string()
    } else {
        realm.to_string()
    }
}

/// A connection-level failure, in words.
fn network(e: &LdapError) -> IdmError {
    match e {
        LdapError::LdapResult { .. } => ldap_err_ref(e, "entry"),
        LdapError::Timeout { .. } => {
            IdmError::Network("the directory did not answer in time".into())
        }
        other => IdmError::Network(format!("could not reach the directory: {other}")),
    }
}

fn ldap_err(e: LdapError, what: &'static str) -> IdmError {
    ldap_err_ref(&e, what)
}

/// An LDAP result code as the shared error (§8.4), the server's own
/// words kept where it gave some.
fn ldap_err_ref(e: &LdapError, what: &'static str) -> IdmError {
    let LdapError::LdapResult { result } = e else {
        return IdmError::Network(e.to_string());
    };
    let said = if result.text.trim().is_empty() {
        String::new()
    } else {
        format!(": {}", result.text.trim())
    };
    match result.rc {
        32 => IdmError::NotFound { what },
        49 => IdmError::Login(format!(
            "the directory refused the bind (49): check the bind DN and password{said}"
        )),
        50 => IdmError::Other(format!(
            "not allowed (50): the bind account may not do that{said}"
        )),
        53 => IdmError::Other(format!("the directory will not do that (53){said}")),
        68 => IdmError::Conflict(format!("an entry with that name exists{said}")),
        66 => IdmError::Invalid {
            message: format!("it has entries under it: move or delete them first{said}"),
            field: None,
        },
        16 | 17 | 18 | 19 | 20 | 21 | 64 | 65 | 67 => IdmError::Invalid {
            message: format!("the directory refused the change ({}){said}", result.rc),
            field: None,
        },
        rc => IdmError::Other(format!("the directory answered {rc}{said}")),
    }
}

fn not_here<T>() -> Result<T> {
    Err(IdmError::Other(NO_SUCH_SECTION.into()))
}

const USER_ATTRS: [&str; 8] = [
    "objectClass",
    "uid",
    "cn",
    "givenName",
    "sn",
    "mail",
    "createTimestamp",
    "entryUUID",
];

#[async_trait]
impl IdmProvider for LdapProvider {
    async fn connect(&self) -> Result<ServerInfo> {
        let dse = self
            .search(
                "",
                Scope::Base,
                "(objectClass=*)",
                &[
                    "namingContexts",
                    "defaultNamingContext",
                    "supportedExtension",
                    "vendorName",
                    "vendorVersion",
                    "objectClass",
                ],
                1,
            )
            .await?
            .into_iter()
            .next()
            .unwrap_or_default();
        let mut contexts = dse.values("namingContexts");
        let default = dse.first("defaultNamingContext");
        if !default.is_empty() {
            contexts.retain(|c| !c.eq_ignore_ascii_case(&default));
            contexts.insert(0, default);
        }
        let preferred = self.cfg.default_base.trim();
        if !preferred.is_empty() && !contexts.iter().any(|c| c.eq_ignore_ascii_case(preferred)) {
            contexts.insert(0, preferred.to_string());
        }
        *self.contexts.lock() = contexts.clone();
        self.password_modify.store(
            dse.values("supportedExtension")
                .iter()
                .any(|x| x == PASSWORD_MODIFY_OID),
            Ordering::Relaxed,
        );
        let product = match dse.first("vendorName") {
            v if !v.is_empty() => v,
            _ if dse.has_class("OpenLDAProotDSE") => "OpenLDAP".to_string(),
            _ => "LDAP Directory".to_string(),
        };
        let info = ServerInfo {
            product,
            version_text: dse.first("vendorVersion"),
            signed_in_as: if self.anonymous() {
                "anonymous".into()
            } else {
                self.cfg.bind_dn.trim().to_string()
            },
            auth_realm: contexts.first().cloned().unwrap_or_default(),
            ..ServerInfo::default()
        };
        *self.info.lock() = info.clone();
        Ok(info)
    }

    fn info(&self) -> ServerInfo {
        self.info.lock().clone()
    }

    fn capabilities(&self) -> Caps {
        let hidden: BTreeSet<Section> = [
            Section::RealmRoles,
            Section::ClientRoles,
            Section::Clients,
            Section::Sessions,
            Section::LoginEvents,
            Section::AdminEvents,
            Section::RealmSettings,
        ]
        .into_iter()
        .collect();
        Caps {
            user_profile: false,
            group_children: true,
            organizations: false,
            fgap_v2: false,
            // An anonymous bind reads only (§4.12).
            writes: !self.anonymous(),
            hidden,
            roles: None,
            auth_realm: self.info.lock().auth_realm.clone(),
            directory: true,
        }
    }

    fn console_url(&self, _realm: &str, _page: &str) -> String {
        String::new()
    }

    async fn disconnect(&self) {
        if let Some(mut l) = self.conn.lock().await.take() {
            let _ = l.unbind().await;
        }
    }

    async fn realms(&self) -> Result<Vec<Realm>> {
        Ok(self
            .contexts
            .lock()
            .iter()
            .map(|c| Realm {
                id: c.clone(),
                name: c.clone(),
                display_name: c.clone(),
                enabled: true,
            })
            .collect())
    }

    async fn realm_settings(&self, _realm: &str) -> Result<RealmSettings> {
        not_here()
    }

    async fn events_config(&self, _realm: &str) -> Result<EventsConfig> {
        not_here()
    }

    async fn users(&self, realm: &str, q: &UserQuery, page: Page) -> Result<Paged<UserRow>> {
        let filter = map::user_filter(&self.cfg.user_filter, q);
        let mut p = self
            .page(&self.users_base(realm), &filter, &USER_ATTRS, page)
            .await?
            .map(|e| map::user_row(&e));
        // A directory returns entries in its own order; a page reads by name.
        p.rows.sort_by_key(|r| r.username.to_lowercase());
        Ok(p)
    }

    async fn user_count(&self, _realm: &str, _q: &UserQuery) -> Result<Option<u64>> {
        Ok(None)
    }

    async fn user(&self, _realm: &str, id: &str) -> Result<User> {
        Ok(map::user(&self.read(id, "user").await?))
    }

    async fn unmanaged_policy(&self, _realm: &str) -> Result<UnmanagedPolicy> {
        // Every attribute of an entry is the admin's to edit.
        Ok(UnmanagedPolicy::AdminEdit)
    }

    async fn required_actions(&self, _realm: &str) -> Result<Vec<RequiredAction>> {
        Ok(Vec::new())
    }

    async fn user_credentials(&self, _realm: &str, _id: &str) -> Result<Vec<CredentialInfo>> {
        Ok(Vec::new())
    }

    async fn user_groups(&self, realm: &str, id: &str, page: Page) -> Result<Paged<Group>> {
        let e = self.read(id, "user").await?;
        let filter = map::member_of_filter(&self.cfg.group_filter, &e.dn, &map::username(&e));
        let mut p = self
            .page(&self.groups_base(realm), &filter, &["*"], page)
            .await?
            .map(|g| map::group(&g));
        p.rows.sort_by_key(|g| g.name.to_lowercase());
        Ok(p)
    }

    async fn user_sessions(&self, _realm: &str, _id: &str) -> Result<Vec<Session>> {
        Ok(Vec::new())
    }

    async fn user_offline_sessions(
        &self,
        _realm: &str,
        _id: &str,
        _client: &str,
    ) -> Result<Vec<Session>> {
        Ok(Vec::new())
    }

    async fn create_user(&self, realm: &str, user: &NewUser) -> Result<String> {
        let username = user.username.trim();
        if username.is_empty() {
            return Err(IdmError::Invalid {
                message: "a user needs a username".into(),
                field: Some("username".into()),
            });
        }
        let dn = map::child("uid", username, &self.users_base(realm));
        let attrs = map::new_user_attrs(
            username,
            user.first_name.trim(),
            user.last_name.trim(),
            user.email.trim(),
        );
        self.add(&dn, attrs).await?;
        Ok(dn)
    }

    async fn update_user(&self, _realm: &str, user: &ObjRef, change: &Change) -> Result<()> {
        let e = self.read(&user.id, "user").await?;
        let edit = map::user_mods(&e, change)?;
        let dn = match &edit.rename {
            Some(new) => self.rename(&e.dn, "uid", new).await?,
            None => e.dn.clone(),
        };
        // The rename already replaced uid.
        let mods = edit
            .mods
            .into_iter()
            .filter(|m| edit.rename.is_none() || !matches!(m, ModOp::Replace(a, _) if a == "uid"))
            .collect();
        self.modify(&dn, mods).await
    }

    async fn set_user_enabled(&self, _realm: &str, _user: &ObjRef, _enabled: bool) -> Result<()> {
        Err(IdmError::Other(
            "a plain directory has no enabled flag (Active Directory mode has one)".into(),
        ))
    }

    async fn reset_password(
        &self,
        _realm: &str,
        user: &ObjRef,
        password: SecretString,
        _temporary: bool,
    ) -> Result<()> {
        if self.password_modify.load(Ordering::Relaxed) {
            let mut ldap = self.handle().await?;
            ldap.with_timeout(self.cfg.timeout)
                .extended(PasswordModify {
                    user_id: Some(&user.id),
                    old_pass: None,
                    new_pass: Some(password.expose_secret()),
                })
                .await
                .map_err(|e| network(&e))?
                .success()
                .map_err(|e| ldap_err(e, "user"))?;
            return Ok(());
        }
        self.modify(
            &user.id,
            vec![ModOp::Replace(
                "userPassword".into(),
                vec![password.expose_secret().to_string()],
            )],
        )
        .await
    }

    async fn delete_user(&self, realm: &str, user: &ObjRef) -> Result<()> {
        let e = self.read(&user.id, "user").await?;
        self.delete(&e.dn, "user").await?;
        self.drop_from_groups(realm, &e).await
    }

    async fn join_group(&self, _realm: &str, user: &ObjRef, group: &ObjRef) -> Result<()> {
        let e = self.read(&user.id, "user").await?;
        self.membership(&group.id, &e, true).await
    }

    async fn leave_group(&self, _realm: &str, user: &ObjRef, group: &ObjRef) -> Result<()> {
        let e = self.read(&user.id, "user").await?;
        self.membership(&group.id, &e, false).await
    }

    async fn logout_user(&self, _realm: &str, _user: &ObjRef) -> Result<()> {
        not_here()
    }

    async fn end_session(&self, _realm: &str, _session: &ObjRef) -> Result<()> {
        not_here()
    }

    async fn groups(&self, realm: &str, search: &str, page: Page) -> Result<Paged<Group>> {
        let filter = map::group_filter(&self.cfg.group_filter, search);
        let mut p = self
            .page(&self.groups_base(realm), &filter, &["*"], page)
            .await?
            .map(|g| map::group(&g));
        p.rows.sort_by_key(|g| g.name.to_lowercase());
        Ok(p)
    }

    async fn group_count(&self, _realm: &str, _search: &str) -> Result<Option<u64>> {
        Ok(None)
    }

    async fn group(&self, _realm: &str, id: &str) -> Result<Group> {
        Ok(map::group(&self.read(id, "group").await?))
    }

    /// The groups a group lists as members: a directory's nesting.
    async fn group_children(&self, _realm: &str, id: &str, page: Page) -> Result<Paged<Group>> {
        let g = self.read(id, "group").await?;
        let (attr, by_dn) = map::member_attr(&g);
        if !by_dn {
            return Ok(Paged::default());
        }
        let mut found = Vec::new();
        for dn in g.values(attr) {
            if let Ok(e) = self.read(&dn, "group").await
                && (e.has_class("groupOfNames")
                    || e.has_class("groupOfUniqueNames")
                    || e.has_class("posixGroup"))
            {
                let mut child = map::group(&e);
                child.parent_id = Some(g.dn.clone());
                found.push(child);
            }
        }
        found.sort_by_key(|g| g.name.to_lowercase());
        let rows: Vec<Group> = found.into_iter().skip(page.first as usize).collect();
        Ok(Paged::from_probe(rows, page, None))
    }

    async fn group_members(&self, realm: &str, id: &str, page: Page) -> Result<Paged<UserRow>> {
        let g = self.read(id, "group").await?;
        let (attr, by_dn) = map::member_attr(&g);
        let mut values = g.values(attr);
        values.sort_by_key(|v| v.to_lowercase());
        let wanted: Vec<String> = values
            .into_iter()
            .skip(page.first as usize)
            .take(page.max as usize + 1)
            .collect();
        let mut rows = Vec::new();
        for v in wanted {
            let entry = if by_dn {
                self.read(&v, "user").await.ok()
            } else {
                let f = format!("(uid={})", ldap3::ldap_escape(v.as_str()));
                self.search(&self.users_base(realm), Scope::Subtree, &f, &USER_ATTRS, 1)
                    .await?
                    .into_iter()
                    .next()
            };
            // A member that is a group is a child, not a member row.
            if let Some(e) =
                entry.filter(|e| !e.has_class("groupOfNames") && !e.has_class("groupOfUniqueNames"))
            {
                rows.push(map::user_row(&e));
            }
        }
        Ok(Paged::from_probe(rows, page, None))
    }

    async fn create_group(
        &self,
        realm: &str,
        parent: Option<&ObjRef>,
        name: &str,
    ) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(IdmError::Invalid {
                message: "a group needs a name".into(),
                field: Some("name".into()),
            });
        }
        let dn = map::child("cn", name, &self.groups_base(realm));
        // groupOfNames must have a member: the bind account holds the place
        // until the first real one joins.
        let attrs = vec![
            (
                "objectClass".to_string(),
                vec!["top".into(), "groupOfNames".into()],
            ),
            ("cn".into(), vec![name.into()]),
            ("member".into(), vec![self.cfg.bind_dn.trim().to_string()]),
        ];
        self.add(&dn, attrs).await?;
        if let Some(p) = parent {
            let child = Entry {
                dn: dn.clone(),
                ..Entry::default()
            };
            self.membership(&p.id, &child, true).await?;
        }
        Ok(dn)
    }

    async fn update_group(&self, _realm: &str, group: &ObjRef, change: &Change) -> Result<()> {
        let e = self.read(&group.id, "group").await?;
        let edit = map::group_mods(&e, change)?;
        let dn = match &edit.rename {
            Some(new) => self.rename(&e.dn, "cn", new).await?,
            None => e.dn.clone(),
        };
        self.modify(&dn, edit.mods).await
    }

    async fn delete_group(&self, realm: &str, group: &ObjRef) -> Result<()> {
        let e = self.read(&group.id, "group").await?;
        self.delete(&e.dn, "group").await?;
        self.drop_from_groups(realm, &e).await
    }

    async fn role_mappings(&self, _realm: &str, _who: &Subject) -> Result<RoleMappings> {
        not_here()
    }

    async fn realm_role_mappings(
        &self,
        _realm: &str,
        _who: &Subject,
        _effective: bool,
    ) -> Result<Vec<Role>> {
        not_here()
    }

    async fn available_realm_roles(&self, _realm: &str, _who: &Subject) -> Result<Vec<Role>> {
        not_here()
    }

    async fn client_role_mappings(
        &self,
        _realm: &str,
        _who: &Subject,
        _client: &str,
        _effective: bool,
    ) -> Result<Vec<Role>> {
        not_here()
    }

    async fn available_client_roles(
        &self,
        _realm: &str,
        _who: &Subject,
        _client: &str,
    ) -> Result<Vec<Role>> {
        not_here()
    }

    async fn add_realm_roles(&self, _realm: &str, _who: &Subject, _roles: &[Role]) -> Result<()> {
        not_here()
    }

    async fn remove_realm_roles(
        &self,
        _realm: &str,
        _who: &Subject,
        _roles: &[Role],
    ) -> Result<()> {
        not_here()
    }

    async fn add_client_roles(
        &self,
        _realm: &str,
        _who: &Subject,
        _client: &ObjRef,
        _roles: &[Role],
    ) -> Result<()> {
        not_here()
    }

    async fn remove_client_roles(
        &self,
        _realm: &str,
        _who: &Subject,
        _client: &ObjRef,
        _roles: &[Role],
    ) -> Result<()> {
        not_here()
    }

    async fn roles(
        &self,
        _realm: &str,
        _scope: &RoleScope,
        _search: &str,
        _page: Page,
    ) -> Result<Paged<Role>> {
        not_here()
    }

    async fn role(&self, _realm: &str, _id: &str) -> Result<Role> {
        not_here()
    }

    async fn role_composites(&self, _realm: &str, _id: &str) -> Result<Vec<Role>> {
        not_here()
    }

    async fn role_users(
        &self,
        _realm: &str,
        _scope: &RoleScope,
        _name: &str,
        _page: Page,
    ) -> Result<Paged<UserRow>> {
        not_here()
    }

    async fn role_groups(
        &self,
        _realm: &str,
        _scope: &RoleScope,
        _name: &str,
        _page: Page,
    ) -> Result<Paged<Group>> {
        not_here()
    }

    async fn create_role(&self, _realm: &str, _scope: &RoleScope, _role: &NewRole) -> Result<Role> {
        not_here()
    }

    async fn update_role(&self, _realm: &str, _role: &ObjRef, _change: &Change) -> Result<()> {
        not_here()
    }

    async fn delete_role(&self, _realm: &str, _role: &ObjRef) -> Result<()> {
        not_here()
    }

    async fn clients(&self, _realm: &str, _search: &str, _page: Page) -> Result<Paged<Client>> {
        not_here()
    }

    async fn client(&self, _realm: &str, _id: &str) -> Result<Client> {
        not_here()
    }

    async fn client_secret(&self, _realm: &str, _id: &str) -> Result<Secret> {
        not_here()
    }

    async fn service_account_user(&self, _realm: &str, _id: &str) -> Result<UserRow> {
        not_here()
    }

    async fn client_sessions(
        &self,
        _realm: &str,
        _id: &str,
        _page: Page,
    ) -> Result<Paged<Session>> {
        not_here()
    }

    async fn client_session_count(&self, _realm: &str, _id: &str) -> Result<u64> {
        not_here()
    }

    async fn create_client(&self, _realm: &str, _client: &NewClient) -> Result<String> {
        not_here()
    }

    async fn update_client(&self, _realm: &str, _client: &ObjRef, _change: &Change) -> Result<()> {
        not_here()
    }

    async fn delete_client(&self, _realm: &str, _client: &ObjRef) -> Result<()> {
        not_here()
    }

    async fn regenerate_secret(&self, _realm: &str, _client: &ObjRef) -> Result<Secret> {
        not_here()
    }

    async fn client_session_stats(&self, _realm: &str) -> Result<Vec<ClientSessionStats>> {
        not_here()
    }

    async fn logout_all(&self, _realm: &str) -> Result<LogoutAll> {
        not_here()
    }

    async fn login_events(
        &self,
        _realm: &str,
        _q: &EventQuery,
        _page: Page,
    ) -> Result<Paged<LoginEvent>> {
        not_here()
    }

    async fn admin_events(
        &self,
        _realm: &str,
        _q: &AdminEventQuery,
        _page: Page,
    ) -> Result<Paged<AdminEvent>> {
        not_here()
    }

    async fn export(
        &self,
        _realm: &str,
        _scope: ExportScope,
        _cancel: CancellationToken,
    ) -> Result<String> {
        Err(IdmError::Other(
            "a directory is exported as LDIF by its own tools (slapcat, ldapsearch)".into(),
        ))
    }

    async fn dir_children(&self, _realm: &str, dn: &str, page: Page) -> Result<Paged<DirEntry>> {
        let want = page.first as usize + page.max as usize + 1;
        let mut all: Vec<DirEntry> = self
            .search(
                dn,
                Scope::OneLevel,
                "(objectClass=*)",
                &["objectClass", "hasSubordinates"],
                want,
            )
            .await?
            .iter()
            .map(map::dir_entry)
            .collect();
        // Containers first, then by name, as every tree in the app reads.
        all.sort_by_key(|e| (!e.has_children, e.rdn.to_lowercase()));
        let rows: Vec<DirEntry> = all.into_iter().skip(page.first as usize).collect();
        Ok(Paged::from_probe(rows, page, None))
    }

    async fn dir_entry(&self, _realm: &str, dn: &str) -> Result<DirEntry> {
        Ok(map::dir_entry(&self.read(dn, "entry").await?))
    }

    async fn dir_modify(
        &self,
        _realm: &str,
        entry: &ObjRef,
        attributes: &std::collections::BTreeMap<String, Vec<String>>,
    ) -> Result<()> {
        let e = self.read(&entry.id, "entry").await?;
        let mods = map::dir_mods(&e, attributes)?;
        self.modify(&e.dn, mods).await
    }

    async fn dir_move(&self, _realm: &str, entry: &ObjRef, parent: &str) -> Result<String> {
        let parent = parent.trim();
        let e = self.read(&entry.id, "entry").await?;
        if map::is_under(parent, &e.dn) {
            return Err(IdmError::Invalid {
                message: "an entry cannot move under itself".into(),
                field: None,
            });
        }
        if self.read(parent, "entry").await.is_err() {
            return Err(IdmError::Invalid {
                message: format!("“{parent}” is not an entry of this directory"),
                field: None,
            });
        }
        let rdn = map::components(&e.dn)
            .into_iter()
            .next()
            .unwrap_or_default();
        let mut ldap = self.handle().await?;
        ldap.with_timeout(self.cfg.timeout)
            .modifydn(&e.dn, &rdn, true, Some(parent))
            .await
            .map_err(|e| network(&e))?
            .success()
            .map_err(|e| ldap_err(e, "entry"))?;
        Ok(format!("{rdn},{parent}"))
    }

    async fn dir_delete(&self, realm: &str, entry: &ObjRef) -> Result<()> {
        let e = self.read(&entry.id, "entry").await?;
        if e.first("hasSubordinates").eq_ignore_ascii_case("TRUE") {
            return Err(IdmError::Invalid {
                message: "it has entries under it: move or delete them first".into(),
                field: None,
            });
        }
        self.delete(&e.dn, "entry").await?;
        self.drop_from_groups(realm, &e).await
    }
}
