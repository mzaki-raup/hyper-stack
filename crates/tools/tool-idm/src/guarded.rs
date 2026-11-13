//! The safety layer between a tab and its provider (docs/tools/idm.md
//! §6): [`Guarded`] refuses every write on a read-only tab before any
//! request is made, and records every write it lets through in the
//! workspace's audit log, whatever the server answered.
//!
//! A tab is read-only when its profile says so, or when its environment
//! is protected (Production) and the tab has not been unlocked. Unlocking
//! is the owner's decision of 2026-11-08: a session toggle, behind a
//! confirmation naming the environment, as the Database tool does; it
//! lasts until the tab closes or reconnects ([`WriteGate::unlock`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;

use crate::audit::{AuditContext, AuditEntry, AuditLog};
use crate::provider::*;

/// Every method of [`IdmProvider`] that changes the server. A new write
/// is added here in the same change that adds it; the tests fail on a
/// trait method that is in neither this list nor [`READS`].
pub const WRITES: &[&str] = &[
    "create_user",
    "update_user",
    "set_user_enabled",
    "reset_password",
    "delete_user",
    "join_group",
    "leave_group",
    "logout_user",
    "end_session",
    "create_group",
    "update_group",
    "delete_group",
    "add_realm_roles",
    "remove_realm_roles",
    "add_client_roles",
    "remove_client_roles",
    "create_role",
    "update_role",
    "delete_role",
    "create_client",
    "update_client",
    "delete_client",
    "regenerate_secret",
    "logout_all",
];

/// Every method that only reads (including `connect`, `disconnect` and
/// `export`, which POSTs but changes nothing).
pub const READS: &[&str] = &[
    "connect",
    "info",
    "capabilities",
    "console_url",
    "disconnect",
    "realms",
    "realm_settings",
    "events_config",
    "users",
    "user_count",
    "user",
    "unmanaged_policy",
    "required_actions",
    "user_credentials",
    "user_groups",
    "user_sessions",
    "user_offline_sessions",
    "groups",
    "group_count",
    "group",
    "group_children",
    "group_members",
    "role_mappings",
    "realm_role_mappings",
    "available_realm_roles",
    "client_role_mappings",
    "available_client_roles",
    "roles",
    "role",
    "role_composites",
    "role_users",
    "role_groups",
    "clients",
    "client",
    "client_secret",
    "service_account_user",
    "client_sessions",
    "client_session_count",
    "client_session_stats",
    "login_events",
    "admin_events",
    "export",
];

/// Whether a tab may write, and why not. Shared by the tab (to draw
/// controls disabled with the reason, and to unlock) and the
/// [`Guarded`] provider (to refuse).
#[derive(Debug)]
pub struct WriteGate {
    profile_read_only: bool,
    protected: bool,
    env_name: String,
    unlocked: AtomicBool,
}

impl WriteGate {
    pub fn new(profile_read_only: bool, protected: bool, env_name: &str) -> Arc<WriteGate> {
        Arc::new(WriteGate {
            profile_read_only,
            protected,
            env_name: env_name.to_string(),
            unlocked: AtomicBool::new(false),
        })
    }

    /// A gate that never refuses (an unprotected, writable profile).
    pub fn open() -> Arc<WriteGate> {
        WriteGate::new(false, false, "")
    }

    pub fn protected(&self) -> bool {
        self.protected
    }

    pub fn env_name(&self) -> &str {
        &self.env_name
    }

    pub fn unlocked(&self) -> bool {
        self.unlocked.load(Ordering::SeqCst)
    }

    /// Why writes are refused, or `None` when they are allowed.
    pub fn refusal(&self) -> Option<String> {
        if self.profile_read_only {
            Some(
                "This connection is read-only: its profile says so. Change it in Settings → Identity."
                    .into(),
            )
        } else if self.protected && !self.unlocked() {
            let env = if self.env_name.is_empty() {
                "This environment"
            } else {
                &self.env_name
            };
            Some(format!(
                "{env} is read-only. Allow writes for this tab, or change it in Settings → Environments."
            ))
        } else {
            None
        }
    }

    pub fn read_only(&self) -> bool {
        self.refusal().is_some()
    }

    /// Whether the tab may offer Allow Writes: a protected environment,
    /// not a read-only profile, not unlocked yet.
    pub fn can_unlock(&self) -> bool {
        self.protected && !self.profile_read_only && !self.unlocked()
    }

    /// Allow writes for this tab's session, after the confirmation naming
    /// the environment. A read-only profile stays read-only.
    pub fn unlock(&self) -> bool {
        if self.profile_read_only {
            return false;
        }
        self.unlocked.store(true, Ordering::SeqCst);
        true
    }

    /// Back to read-only (the tab reconnected, or the user locked it).
    pub fn lock(&self) {
        self.unlocked.store(false, Ordering::SeqCst);
    }
}

/// The provider every tab talks to: reads pass through; each write is
/// refused on a read-only tab before a request is made, and otherwise
/// sent and recorded in the audit log.
pub struct Guarded {
    inner: Arc<dyn IdmProvider>,
    gate: Arc<WriteGate>,
    audit: Option<Arc<AuditLog>>,
    ctx: AuditContext,
}

impl Guarded {
    pub fn new(
        inner: Arc<dyn IdmProvider>,
        gate: Arc<WriteGate>,
        audit: Option<Arc<AuditLog>>,
        ctx: AuditContext,
    ) -> Guarded {
        Guarded {
            inner,
            gate,
            audit,
            ctx,
        }
    }

    pub fn gate(&self) -> &Arc<WriteGate> {
        &self.gate
    }

    fn allow(&self) -> Result<()> {
        match self.gate.refusal() {
            Some(why) => Err(IdmError::ReadOnly(why)),
            None => Ok(()),
        }
    }

    /// One audit line for a write that was sent.
    #[allow(clippy::too_many_arguments)] // one line's worth of facts
    fn record<T>(
        &self,
        realm: &str,
        object: &str,
        id: &str,
        name: &str,
        verb: &str,
        fields: Vec<String>,
        r: &Result<T>,
    ) {
        let Some(log) = &self.audit else { return };
        let entry = AuditEntry {
            connection: self.ctx.connection.clone(),
            server: self.ctx.server.clone(),
            realm: realm.to_string(),
            object: object.to_string(),
            id: id.to_string(),
            name: name.to_string(),
            verb: verb.to_string(),
            fields,
            by: self.inner.info().signed_in_as,
            outcome: match r {
                Ok(_) => "ok".into(),
                Err(e) => e.to_string(),
            },
            ..Default::default()
        };
        if let Err(e) = log.append(entry) {
            // The write itself happened; the screen's toast reports it.
            tracing::warn!("idm: the audit log could not be written: {e}");
        }
    }

    fn record_obj<T>(
        &self,
        realm: &str,
        object: &str,
        o: &ObjRef,
        verb: &str,
        fields: Vec<String>,
        r: &Result<T>,
    ) {
        self.record(realm, object, &o.id, &o.name, verb, fields, r);
    }
}

fn subject_kind(who: &Subject) -> (&'static str, &str) {
    match who {
        Subject::User(id) => ("user", id),
        Subject::Group(id) => ("group", id),
    }
}

fn role_names(roles: &[Role]) -> Vec<String> {
    roles.iter().map(|r| r.name.clone()).collect()
}

#[async_trait]
impl IdmProvider for Guarded {
    async fn connect(&self) -> Result<ServerInfo> {
        self.inner.connect().await
    }
    fn info(&self) -> ServerInfo {
        self.inner.info()
    }
    fn capabilities(&self) -> Caps {
        let mut caps = self.inner.capabilities();
        // A read-only tab offers no write, whatever the server allows.
        caps.writes &= !self.gate.read_only();
        caps
    }
    fn console_url(&self, realm: &str, page: &str) -> String {
        self.inner.console_url(realm, page)
    }
    async fn disconnect(&self) {
        self.inner.disconnect().await
    }

    async fn realms(&self) -> Result<Vec<Realm>> {
        self.inner.realms().await
    }
    async fn realm_settings(&self, realm: &str) -> Result<RealmSettings> {
        self.inner.realm_settings(realm).await
    }
    async fn events_config(&self, realm: &str) -> Result<EventsConfig> {
        self.inner.events_config(realm).await
    }

    async fn users(&self, realm: &str, q: &UserQuery, page: Page) -> Result<Paged<UserRow>> {
        self.inner.users(realm, q, page).await
    }
    async fn user_count(&self, realm: &str, q: &UserQuery) -> Result<Option<u64>> {
        self.inner.user_count(realm, q).await
    }
    async fn user(&self, realm: &str, id: &str) -> Result<User> {
        self.inner.user(realm, id).await
    }
    async fn unmanaged_policy(&self, realm: &str) -> Result<UnmanagedPolicy> {
        self.inner.unmanaged_policy(realm).await
    }
    async fn required_actions(&self, realm: &str) -> Result<Vec<RequiredAction>> {
        self.inner.required_actions(realm).await
    }
    async fn user_credentials(&self, realm: &str, id: &str) -> Result<Vec<CredentialInfo>> {
        self.inner.user_credentials(realm, id).await
    }
    async fn user_groups(&self, realm: &str, id: &str, page: Page) -> Result<Paged<Group>> {
        self.inner.user_groups(realm, id, page).await
    }
    async fn user_sessions(&self, realm: &str, id: &str) -> Result<Vec<Session>> {
        self.inner.user_sessions(realm, id).await
    }
    async fn user_offline_sessions(
        &self,
        realm: &str,
        id: &str,
        client: &str,
    ) -> Result<Vec<Session>> {
        self.inner.user_offline_sessions(realm, id, client).await
    }
    async fn create_user(&self, realm: &str, user: &NewUser) -> Result<String> {
        self.allow()?;
        let r = self.inner.create_user(realm, user).await;
        let id = r.as_ref().map(String::as_str).unwrap_or_default();
        let mut fields = vec!["username".to_string()];
        for (name, set) in [
            ("email", !user.email.is_empty()),
            ("firstName", !user.first_name.is_empty()),
            ("lastName", !user.last_name.is_empty()),
            ("enabled", true),
            ("emailVerified", user.email_verified),
            ("requiredActions", !user.required_actions.is_empty()),
        ] {
            if set {
                fields.push(name.into());
            }
        }
        self.record(realm, "user", id, &user.username, "create", fields, &r);
        r
    }
    async fn update_user(&self, realm: &str, user: &ObjRef, change: &Change) -> Result<()> {
        self.allow()?;
        let r = self.inner.update_user(realm, user, change).await;
        self.record_obj(realm, "user", user, "update", change.names(), &r);
        r
    }
    async fn set_user_enabled(&self, realm: &str, user: &ObjRef, enabled: bool) -> Result<()> {
        self.allow()?;
        let r = self.inner.set_user_enabled(realm, user, enabled).await;
        let verb = if enabled { "enable" } else { "disable" };
        self.record_obj(realm, "user", user, verb, vec!["enabled".into()], &r);
        r
    }
    async fn reset_password(
        &self,
        realm: &str,
        user: &ObjRef,
        password: SecretString,
        temporary: bool,
    ) -> Result<()> {
        self.allow()?;
        let r = self
            .inner
            .reset_password(realm, user, password, temporary)
            .await;
        self.record_obj(
            realm,
            "user",
            user,
            "reset-password",
            vec!["temporary".into()],
            &r,
        );
        r
    }
    async fn delete_user(&self, realm: &str, user: &ObjRef) -> Result<()> {
        self.allow()?;
        let r = self.inner.delete_user(realm, user).await;
        self.record_obj(realm, "user", user, "delete", Vec::new(), &r);
        r
    }
    async fn join_group(&self, realm: &str, user: &ObjRef, group: &ObjRef) -> Result<()> {
        self.allow()?;
        let r = self.inner.join_group(realm, user, group).await;
        self.record_obj(
            realm,
            "user",
            user,
            "join-group",
            vec![group.name.clone()],
            &r,
        );
        r
    }
    async fn leave_group(&self, realm: &str, user: &ObjRef, group: &ObjRef) -> Result<()> {
        self.allow()?;
        let r = self.inner.leave_group(realm, user, group).await;
        self.record_obj(
            realm,
            "user",
            user,
            "leave-group",
            vec![group.name.clone()],
            &r,
        );
        r
    }
    async fn logout_user(&self, realm: &str, user: &ObjRef) -> Result<()> {
        self.allow()?;
        let r = self.inner.logout_user(realm, user).await;
        self.record_obj(realm, "user", user, "logout", Vec::new(), &r);
        r
    }
    async fn end_session(&self, realm: &str, session: &ObjRef) -> Result<()> {
        self.allow()?;
        let r = self.inner.end_session(realm, session).await;
        self.record_obj(realm, "session", session, "end-session", Vec::new(), &r);
        r
    }

    async fn groups(&self, realm: &str, search: &str, page: Page) -> Result<Paged<Group>> {
        self.inner.groups(realm, search, page).await
    }
    async fn group_count(&self, realm: &str, search: &str) -> Result<Option<u64>> {
        self.inner.group_count(realm, search).await
    }
    async fn group(&self, realm: &str, id: &str) -> Result<Group> {
        self.inner.group(realm, id).await
    }
    async fn group_children(&self, realm: &str, id: &str, page: Page) -> Result<Paged<Group>> {
        self.inner.group_children(realm, id, page).await
    }
    async fn group_members(&self, realm: &str, id: &str, page: Page) -> Result<Paged<UserRow>> {
        self.inner.group_members(realm, id, page).await
    }
    async fn create_group(
        &self,
        realm: &str,
        parent: Option<&ObjRef>,
        name: &str,
    ) -> Result<String> {
        self.allow()?;
        let r = self.inner.create_group(realm, parent, name).await;
        let id = r.as_ref().map(String::as_str).unwrap_or_default();
        let label = match parent {
            Some(p) => format!("{}/{name}", p.name.trim_end_matches('/')),
            None => format!("/{name}"),
        };
        self.record(
            realm,
            "group",
            id,
            &label,
            "create",
            vec!["name".into()],
            &r,
        );
        r
    }
    async fn update_group(&self, realm: &str, group: &ObjRef, change: &Change) -> Result<()> {
        self.allow()?;
        let r = self.inner.update_group(realm, group, change).await;
        self.record_obj(realm, "group", group, "update", change.names(), &r);
        r
    }
    async fn delete_group(&self, realm: &str, group: &ObjRef) -> Result<()> {
        self.allow()?;
        let r = self.inner.delete_group(realm, group).await;
        self.record_obj(realm, "group", group, "delete", Vec::new(), &r);
        r
    }

    async fn role_mappings(&self, realm: &str, who: &Subject) -> Result<RoleMappings> {
        self.inner.role_mappings(realm, who).await
    }
    async fn realm_role_mappings(
        &self,
        realm: &str,
        who: &Subject,
        effective: bool,
    ) -> Result<Vec<Role>> {
        self.inner.realm_role_mappings(realm, who, effective).await
    }
    async fn available_realm_roles(&self, realm: &str, who: &Subject) -> Result<Vec<Role>> {
        self.inner.available_realm_roles(realm, who).await
    }
    async fn client_role_mappings(
        &self,
        realm: &str,
        who: &Subject,
        client: &str,
        effective: bool,
    ) -> Result<Vec<Role>> {
        self.inner
            .client_role_mappings(realm, who, client, effective)
            .await
    }
    async fn available_client_roles(
        &self,
        realm: &str,
        who: &Subject,
        client: &str,
    ) -> Result<Vec<Role>> {
        self.inner.available_client_roles(realm, who, client).await
    }
    async fn add_realm_roles(&self, realm: &str, who: &Subject, roles: &[Role]) -> Result<()> {
        self.allow()?;
        let r = self.inner.add_realm_roles(realm, who, roles).await;
        let (kind, id) = subject_kind(who);
        self.record(realm, kind, id, "", "assign-roles", role_names(roles), &r);
        r
    }
    async fn remove_realm_roles(&self, realm: &str, who: &Subject, roles: &[Role]) -> Result<()> {
        self.allow()?;
        let r = self.inner.remove_realm_roles(realm, who, roles).await;
        let (kind, id) = subject_kind(who);
        self.record(realm, kind, id, "", "unassign-roles", role_names(roles), &r);
        r
    }
    async fn add_client_roles(
        &self,
        realm: &str,
        who: &Subject,
        client: &ObjRef,
        roles: &[Role],
    ) -> Result<()> {
        self.allow()?;
        let r = self.inner.add_client_roles(realm, who, client, roles).await;
        let (kind, id) = subject_kind(who);
        let names = roles
            .iter()
            .map(|r| format!("{}:{}", client.name, r.name))
            .collect();
        self.record(realm, kind, id, "", "assign-roles", names, &r);
        r
    }
    async fn remove_client_roles(
        &self,
        realm: &str,
        who: &Subject,
        client: &ObjRef,
        roles: &[Role],
    ) -> Result<()> {
        self.allow()?;
        let r = self
            .inner
            .remove_client_roles(realm, who, client, roles)
            .await;
        let (kind, id) = subject_kind(who);
        let names = roles
            .iter()
            .map(|r| format!("{}:{}", client.name, r.name))
            .collect();
        self.record(realm, kind, id, "", "unassign-roles", names, &r);
        r
    }

    async fn roles(
        &self,
        realm: &str,
        scope: &RoleScope,
        search: &str,
        page: Page,
    ) -> Result<Paged<Role>> {
        self.inner.roles(realm, scope, search, page).await
    }
    async fn role(&self, realm: &str, id: &str) -> Result<Role> {
        self.inner.role(realm, id).await
    }
    async fn role_composites(&self, realm: &str, id: &str) -> Result<Vec<Role>> {
        self.inner.role_composites(realm, id).await
    }
    async fn role_users(
        &self,
        realm: &str,
        scope: &RoleScope,
        name: &str,
        page: Page,
    ) -> Result<Paged<UserRow>> {
        self.inner.role_users(realm, scope, name, page).await
    }
    async fn role_groups(
        &self,
        realm: &str,
        scope: &RoleScope,
        name: &str,
        page: Page,
    ) -> Result<Paged<Group>> {
        self.inner.role_groups(realm, scope, name, page).await
    }
    async fn create_role(&self, realm: &str, scope: &RoleScope, role: &NewRole) -> Result<Role> {
        self.allow()?;
        let r = self.inner.create_role(realm, scope, role).await;
        let id = r.as_ref().map(|r| r.id.as_str()).unwrap_or_default();
        let mut fields = vec!["name".to_string()];
        if !role.description.is_empty() {
            fields.push("description".into());
        }
        self.record(realm, "role", id, &role.name, "create", fields, &r);
        r
    }
    async fn update_role(&self, realm: &str, role: &ObjRef, change: &Change) -> Result<()> {
        self.allow()?;
        let r = self.inner.update_role(realm, role, change).await;
        self.record_obj(realm, "role", role, "update", change.names(), &r);
        r
    }
    async fn delete_role(&self, realm: &str, role: &ObjRef) -> Result<()> {
        self.allow()?;
        let r = self.inner.delete_role(realm, role).await;
        self.record_obj(realm, "role", role, "delete", Vec::new(), &r);
        r
    }

    async fn clients(&self, realm: &str, search: &str, page: Page) -> Result<Paged<Client>> {
        self.inner.clients(realm, search, page).await
    }
    async fn client(&self, realm: &str, id: &str) -> Result<Client> {
        self.inner.client(realm, id).await
    }
    async fn client_secret(&self, realm: &str, id: &str) -> Result<Secret> {
        self.inner.client_secret(realm, id).await
    }
    async fn service_account_user(&self, realm: &str, id: &str) -> Result<UserRow> {
        self.inner.service_account_user(realm, id).await
    }
    async fn client_sessions(&self, realm: &str, id: &str, page: Page) -> Result<Paged<Session>> {
        self.inner.client_sessions(realm, id, page).await
    }
    async fn client_session_count(&self, realm: &str, id: &str) -> Result<u64> {
        self.inner.client_session_count(realm, id).await
    }
    async fn create_client(&self, realm: &str, client: &NewClient) -> Result<String> {
        self.allow()?;
        let r = self.inner.create_client(realm, client).await;
        let id = r.as_ref().map(String::as_str).unwrap_or_default();
        self.record(
            realm,
            "client",
            id,
            &client.client_id,
            "create",
            vec!["clientId".into(), "protocol".into(), "publicClient".into()],
            &r,
        );
        r
    }
    async fn update_client(&self, realm: &str, client: &ObjRef, change: &Change) -> Result<()> {
        self.allow()?;
        let r = self.inner.update_client(realm, client, change).await;
        self.record_obj(realm, "client", client, "update", change.names(), &r);
        r
    }
    async fn delete_client(&self, realm: &str, client: &ObjRef) -> Result<()> {
        self.allow()?;
        let r = self.inner.delete_client(realm, client).await;
        self.record_obj(realm, "client", client, "delete", Vec::new(), &r);
        r
    }
    async fn regenerate_secret(&self, realm: &str, client: &ObjRef) -> Result<Secret> {
        self.allow()?;
        let r = self.inner.regenerate_secret(realm, client).await;
        self.record_obj(
            realm,
            "client",
            client,
            "regenerate-secret",
            vec!["secret".into()],
            &r,
        );
        r
    }

    async fn client_session_stats(&self, realm: &str) -> Result<Vec<ClientSessionStats>> {
        self.inner.client_session_stats(realm).await
    }
    async fn logout_all(&self, realm: &str) -> Result<LogoutAll> {
        self.allow()?;
        let r = self.inner.logout_all(realm).await;
        self.record(realm, "realm", "", realm, "logout-all", Vec::new(), &r);
        r
    }

    async fn login_events(
        &self,
        realm: &str,
        q: &EventQuery,
        page: Page,
    ) -> Result<Paged<LoginEvent>> {
        self.inner.login_events(realm, q, page).await
    }
    async fn admin_events(
        &self,
        realm: &str,
        q: &AdminEventQuery,
        page: Page,
    ) -> Result<Paged<AdminEvent>> {
        self.inner.admin_events(realm, q, page).await
    }

    async fn export(
        &self,
        realm: &str,
        scope: ExportScope,
        cancel: CancellationToken,
    ) -> Result<String> {
        self.inner.export(realm, scope, cancel).await
    }
}

#[cfg(test)]
pub(crate) mod tests_support;

#[cfg(test)]
mod tests;
