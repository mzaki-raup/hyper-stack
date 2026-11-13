//! A provider that records which methods reached it, for the guard's and
//! the runner's tests.

use async_trait::async_trait;
use parking_lot::Mutex;
use secrecy::{ExposeSecret, SecretString};
use tokio_util::sync::CancellationToken;

use crate::provider::*;

/// Records which methods reached it; answers defaults, or an error when
/// `fail` is set.
#[derive(Default)]
pub(crate) struct Fake {
    pub hits: Mutex<Vec<&'static str>>,
    pub fail: bool,
    /// The last password a reset carried, to prove it reached the
    /// provider (and only the provider).
    pub password: Mutex<String>,
}

impl Fake {
    fn hit<T: Default>(&self, name: &'static str) -> Result<T> {
        self.hits.lock().push(name);
        if self.fail {
            Err(IdmError::Conflict("User exists with same username".into()))
        } else {
            Ok(T::default())
        }
    }
    fn secret(&self, name: &'static str) -> Result<Secret> {
        self.hits.lock().push(name);
        Ok(Secret(SecretString::from(
            "regenerated-client-secret-XYZ".to_string(),
        )))
    }
}

#[async_trait]
impl IdmProvider for Fake {
    async fn connect(&self) -> Result<ServerInfo> {
        self.hit("connect")
    }
    fn info(&self) -> ServerInfo {
        ServerInfo {
            signed_in_as: "hs-admin".into(),
            ..Default::default()
        }
    }
    fn capabilities(&self) -> Caps {
        Caps {
            writes: true,
            ..Default::default()
        }
    }
    fn console_url(&self, _: &str, _: &str) -> String {
        String::new()
    }
    async fn disconnect(&self) {
        self.hits.lock().push("disconnect");
    }
    async fn realms(&self) -> Result<Vec<Realm>> {
        self.hit("realms")
    }
    async fn realm_settings(&self, _: &str) -> Result<RealmSettings> {
        self.hit("realm_settings")
    }
    async fn events_config(&self, _: &str) -> Result<EventsConfig> {
        self.hit("events_config")
    }
    async fn users(&self, _: &str, _: &UserQuery, _: Page) -> Result<Paged<UserRow>> {
        self.hit("users")
    }
    async fn user_count(&self, _: &str, _: &UserQuery) -> Result<Option<u64>> {
        self.hit("user_count")
    }
    async fn user(&self, _: &str, _: &str) -> Result<User> {
        self.hit("user")
    }
    async fn unmanaged_policy(&self, _: &str) -> Result<UnmanagedPolicy> {
        self.hit("unmanaged_policy")
    }
    async fn required_actions(&self, _: &str) -> Result<Vec<RequiredAction>> {
        self.hit("required_actions")
    }
    async fn user_credentials(&self, _: &str, _: &str) -> Result<Vec<CredentialInfo>> {
        self.hit("user_credentials")
    }
    async fn user_groups(&self, _: &str, _: &str, _: Page) -> Result<Paged<Group>> {
        self.hit("user_groups")
    }
    async fn user_sessions(&self, _: &str, _: &str) -> Result<Vec<Session>> {
        self.hit("user_sessions")
    }
    async fn user_offline_sessions(&self, _: &str, _: &str, _: &str) -> Result<Vec<Session>> {
        self.hit("user_offline_sessions")
    }
    async fn create_user(&self, _: &str, _: &NewUser) -> Result<String> {
        self.hit("create_user")
    }
    async fn update_user(&self, _: &str, _: &ObjRef, _: &Change) -> Result<()> {
        self.hit("update_user")
    }
    async fn set_user_enabled(&self, _: &str, _: &ObjRef, _: bool) -> Result<()> {
        self.hit("set_user_enabled")
    }
    async fn reset_password(&self, _: &str, _: &ObjRef, pw: SecretString, _: bool) -> Result<()> {
        *self.password.lock() = pw.expose_secret().to_string();
        self.hit("reset_password")
    }
    async fn delete_user(&self, _: &str, _: &ObjRef) -> Result<()> {
        self.hit("delete_user")
    }
    async fn join_group(&self, _: &str, _: &ObjRef, _: &ObjRef) -> Result<()> {
        self.hit("join_group")
    }
    async fn leave_group(&self, _: &str, _: &ObjRef, _: &ObjRef) -> Result<()> {
        self.hit("leave_group")
    }
    async fn logout_user(&self, _: &str, _: &ObjRef) -> Result<()> {
        self.hit("logout_user")
    }
    async fn end_session(&self, _: &str, _: &ObjRef) -> Result<()> {
        self.hit("end_session")
    }
    async fn groups(&self, _: &str, _: &str, _: Page) -> Result<Paged<Group>> {
        self.hit("groups")
    }
    async fn group_count(&self, _: &str, _: &str) -> Result<Option<u64>> {
        self.hit("group_count")
    }
    async fn group(&self, _: &str, _: &str) -> Result<Group> {
        self.hit("group")
    }
    async fn group_children(&self, _: &str, _: &str, _: Page) -> Result<Paged<Group>> {
        self.hit("group_children")
    }
    async fn group_members(&self, _: &str, _: &str, _: Page) -> Result<Paged<UserRow>> {
        self.hit("group_members")
    }
    async fn create_group(&self, _: &str, _: Option<&ObjRef>, _: &str) -> Result<String> {
        self.hit("create_group")
    }
    async fn update_group(&self, _: &str, _: &ObjRef, _: &Change) -> Result<()> {
        self.hit("update_group")
    }
    async fn delete_group(&self, _: &str, _: &ObjRef) -> Result<()> {
        self.hit("delete_group")
    }
    async fn role_mappings(&self, _: &str, _: &Subject) -> Result<RoleMappings> {
        self.hit("role_mappings")
    }
    async fn realm_role_mappings(&self, _: &str, _: &Subject, _: bool) -> Result<Vec<Role>> {
        self.hit("realm_role_mappings")
    }
    async fn available_realm_roles(&self, _: &str, _: &Subject) -> Result<Vec<Role>> {
        self.hit("available_realm_roles")
    }
    async fn client_role_mappings(
        &self,
        _: &str,
        _: &Subject,
        _: &str,
        _: bool,
    ) -> Result<Vec<Role>> {
        self.hit("client_role_mappings")
    }
    async fn available_client_roles(&self, _: &str, _: &Subject, _: &str) -> Result<Vec<Role>> {
        self.hit("available_client_roles")
    }
    async fn add_realm_roles(&self, _: &str, _: &Subject, _: &[Role]) -> Result<()> {
        self.hit("add_realm_roles")
    }
    async fn remove_realm_roles(&self, _: &str, _: &Subject, _: &[Role]) -> Result<()> {
        self.hit("remove_realm_roles")
    }
    async fn add_client_roles(&self, _: &str, _: &Subject, _: &ObjRef, _: &[Role]) -> Result<()> {
        self.hit("add_client_roles")
    }
    async fn remove_client_roles(
        &self,
        _: &str,
        _: &Subject,
        _: &ObjRef,
        _: &[Role],
    ) -> Result<()> {
        self.hit("remove_client_roles")
    }
    async fn roles(&self, _: &str, _: &RoleScope, _: &str, _: Page) -> Result<Paged<Role>> {
        self.hit("roles")
    }
    async fn role(&self, _: &str, _: &str) -> Result<Role> {
        self.hit("role")
    }
    async fn role_composites(&self, _: &str, _: &str) -> Result<Vec<Role>> {
        self.hit("role_composites")
    }
    async fn role_users(&self, _: &str, _: &RoleScope, _: &str, _: Page) -> Result<Paged<UserRow>> {
        self.hit("role_users")
    }
    async fn role_groups(&self, _: &str, _: &RoleScope, _: &str, _: Page) -> Result<Paged<Group>> {
        self.hit("role_groups")
    }
    async fn create_role(&self, _: &str, _: &RoleScope, _: &NewRole) -> Result<Role> {
        self.hit("create_role")
    }
    async fn update_role(&self, _: &str, _: &ObjRef, _: &Change) -> Result<()> {
        self.hit("update_role")
    }
    async fn delete_role(&self, _: &str, _: &ObjRef) -> Result<()> {
        self.hit("delete_role")
    }
    async fn clients(&self, _: &str, _: &str, _: Page) -> Result<Paged<Client>> {
        self.hit("clients")
    }
    async fn client(&self, _: &str, _: &str) -> Result<Client> {
        self.hit("client")
    }
    async fn client_secret(&self, _: &str, _: &str) -> Result<Secret> {
        self.secret("client_secret")
    }
    async fn service_account_user(&self, _: &str, _: &str) -> Result<UserRow> {
        self.hit("service_account_user")
    }
    async fn client_sessions(&self, _: &str, _: &str, _: Page) -> Result<Paged<Session>> {
        self.hit("client_sessions")
    }
    async fn client_session_count(&self, _: &str, _: &str) -> Result<u64> {
        self.hit("client_session_count")
    }
    async fn create_client(&self, _: &str, _: &NewClient) -> Result<String> {
        self.hit("create_client")
    }
    async fn update_client(&self, _: &str, _: &ObjRef, _: &Change) -> Result<()> {
        self.hit("update_client")
    }
    async fn delete_client(&self, _: &str, _: &ObjRef) -> Result<()> {
        self.hit("delete_client")
    }
    async fn regenerate_secret(&self, _: &str, _: &ObjRef) -> Result<Secret> {
        self.secret("regenerate_secret")
    }
    async fn client_session_stats(&self, _: &str) -> Result<Vec<ClientSessionStats>> {
        self.hit("client_session_stats")
    }
    async fn logout_all(&self, _: &str) -> Result<LogoutAll> {
        self.hit("logout_all")
    }
    async fn login_events(&self, _: &str, _: &EventQuery, _: Page) -> Result<Paged<LoginEvent>> {
        self.hit("login_events")
    }
    async fn admin_events(
        &self,
        _: &str,
        _: &AdminEventQuery,
        _: Page,
    ) -> Result<Paged<AdminEvent>> {
        self.hit("admin_events")
    }
    async fn export(&self, _: &str, _: ExportScope, _: CancellationToken) -> Result<String> {
        self.hit("export")
    }
}
