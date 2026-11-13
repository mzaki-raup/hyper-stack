//! The provider model (docs/tools/idm.md §4.1, ADR-032): one trait every
//! identity product implements, and the shared types the screens see -
//! realm, user, group, role, client, session, event. The screens ask
//! [`Caps`], never the provider's name, so a later provider lights up
//! only what it has.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

pub mod keycloak;

pub type Result<T> = std::result::Result<T, IdmError>;

/// Why a call failed, in a shape the screens can act on (drop a vanished
/// row, name the field a 400 named) and in words through `Display`
/// (§8.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdmError {
    /// Nothing answered, the TLS handshake failed, or the server took too
    /// long; the words come from `service_http`.
    Network(String),
    /// The token endpoint refused the credential, already in words
    /// (`invalid_client`, a pending required action, …).
    Login(String),
    /// A 401 that one refresh and one retry did not cure.
    Unauthorized,
    /// A 401 because the token lacks the admin API's audience: Keycloak
    /// 26.8 checks it, and a client without the realm's `kc-admin-api`
    /// client scope gets 401 on every admin call whatever roles it holds.
    Audience {
        client_id: String,
    },
    /// A 403. `needs` is the realm-management role the call needs, where
    /// the provider knows it.
    Forbidden {
        realm: String,
        needs: Option<&'static str>,
    },
    /// A 404 on an item: it is gone. `what` is the kind, "user".
    NotFound {
        what: &'static str,
    },
    /// A 404 on an endpoint the server does not have: the feature is
    /// hidden for the session (§8.3).
    Unsupported(String),
    /// A 409, with the server's own message.
    Conflict(String),
    /// A 400, with the server's message and the field it named.
    Invalid {
        message: String,
        field: Option<String>,
    },
    /// Refused by [`crate::guarded::Guarded`] before any request was made.
    ReadOnly(String),
    Cancelled,
    Other(String),
}

impl std::fmt::Display for IdmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdmError::Network(e) | IdmError::Login(e) | IdmError::Other(e) => f.write_str(e),
            IdmError::Unauthorized => f.write_str(
                "the server refused the credentials (401): check the client secret or password, then Edit This Connection",
            ),
            IdmError::Audience { client_id } => write!(
                f,
                "the server refused the token (401): its audience lacks the admin API. In the admin console, open Clients → {client_id} → Client Scopes and add the realm's “kc-admin-api” client scope as Default (Keycloak 26.8 and later check it)"
            ),
            IdmError::Forbidden { realm, needs } => match needs {
                Some(role) => write!(
                    f,
                    "not allowed (403): this needs the realm-management role “{role}” in realm {realm}"
                ),
                None => write!(
                    f,
                    "not allowed (403): this account has no permission for that in realm {realm}"
                ),
            },
            IdmError::NotFound { what } => write!(f, "this {what} no longer exists"),
            IdmError::Unsupported(what) => {
                write!(f, "this server does not offer {what} (404)")
            }
            IdmError::Conflict(e) => write!(f, "conflict (409): {e}"),
            IdmError::Invalid { message, field } => match field {
                Some(field) => write!(f, "refused (400): {message} ({field})"),
                None => write!(f, "refused (400): {message}"),
            },
            IdmError::ReadOnly(e) => f.write_str(e),
            IdmError::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for IdmError {}

// ---------------------------------------------------------------- paging

/// One page of a list: `first` rows skipped, at most `max` returned
/// (§7: 50 by default, 20-200 from Settings → Identity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub first: u32,
    pub max: u32,
}

pub const PAGE_DEFAULT: u32 = 50;
pub const PAGE_MIN: u32 = 20;
pub const PAGE_MAX: u32 = 200;

impl Default for Page {
    fn default() -> Self {
        Page {
            first: 0,
            max: PAGE_DEFAULT,
        }
    }
}

impl Page {
    /// The first page of `max` rows, `max` kept inside 20-200.
    pub fn of(max: u32) -> Page {
        Page {
            first: 0,
            max: max.clamp(PAGE_MIN, PAGE_MAX),
        }
    }

    pub fn next(self) -> Page {
        Page {
            first: self.first.saturating_add(self.max),
            ..self
        }
    }

    /// The page before, or `None` on the first.
    pub fn previous(self) -> Option<Page> {
        (self.first > 0).then(|| Page {
            first: self.first.saturating_sub(self.max),
            ..self
        })
    }
}

/// A page of rows. `total` is the server's count where it gives one;
/// `more` says whether a next page exists either way, so a server that
/// counts and one that only says "there is more" draw the same status
/// line (§4.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Paged<T> {
    pub rows: Vec<T>,
    pub page: Page,
    pub total: Option<u64>,
    pub more: bool,
}

impl<T> Default for Paged<T> {
    fn default() -> Self {
        Paged {
            rows: Vec::new(),
            page: Page::default(),
            total: None,
            more: false,
        }
    }
}

impl<T> Paged<T> {
    /// Rows asked for with one row over the page (`page.max + 1`): the
    /// extra row only says there is more, and is dropped.
    pub fn from_probe(mut rows: Vec<T>, page: Page, total: Option<u64>) -> Paged<T> {
        let more = rows.len() > page.max as usize;
        rows.truncate(page.max as usize);
        Paged {
            rows,
            page,
            total,
            more,
        }
    }

    /// The status line's figures: rows `from`-`to` (1-based) of `total`.
    /// An empty page reads 0-0.
    pub fn span(&self) -> (u64, u64, Option<u64>) {
        if self.rows.is_empty() {
            return (0, 0, self.total);
        }
        let from = u64::from(self.page.first) + 1;
        (from, from + self.rows.len() as u64 - 1, self.total)
    }

    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Paged<U> {
        Paged {
            rows: self.rows.into_iter().map(f).collect(),
            page: self.page,
            total: self.total,
            more: self.more,
        }
    }
}

// ------------------------------------------------------- server and caps

/// A parsed server version (a Red Hat build's `26.0.6.redhat-00001`
/// reads as 26.0.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Where the server sits against the tested range (§8.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Support {
    /// Inside the tested range.
    Supported,
    /// Older than the range: reads only, best effort.
    TooOld,
    /// Newer than tested: everything offered, failures reported as usual.
    Newer,
    /// The server did not say (a realm-scoped admin is not shown the
    /// version).
    #[default]
    Unknown,
}

/// A server feature and whether it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    pub name: String,
    pub enabled: bool,
}

/// What the server said it is, for the header band.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {
    /// "Keycloak".
    pub product: String,
    /// As the server wrote it; empty when it did not say.
    pub version_text: String,
    pub version: Option<Version>,
    pub support: Support,
    pub features: Vec<Feature>,
    /// The login event types the server knows, for the events filter.
    pub event_types: Vec<String>,
    /// The client id or username the token was issued to.
    pub signed_in_as: String,
    /// The realm the token was requested from.
    pub auth_realm: String,
}

/// A section of the browse column (§3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Section {
    Users,
    Groups,
    RealmRoles,
    ClientRoles,
    Clients,
    Sessions,
    LoginEvents,
    AdminEvents,
    RealmSettings,
}

impl Section {
    pub const ALL: [Section; 9] = [
        Section::Users,
        Section::Groups,
        Section::RealmRoles,
        Section::ClientRoles,
        Section::Clients,
        Section::Sessions,
        Section::LoginEvents,
        Section::AdminEvents,
        Section::RealmSettings,
    ];

    /// The realm-management roles any one of which lets an account read
    /// the section.
    pub fn read_roles(self) -> &'static [&'static str] {
        match self {
            Section::Users => &["view-users", "query-users", "manage-users"],
            Section::Groups => &["query-groups", "view-users", "manage-users"],
            Section::RealmRoles | Section::RealmSettings => &["view-realm", "manage-realm"],
            Section::ClientRoles | Section::Clients => {
                &["view-clients", "query-clients", "manage-clients"]
            }
            Section::Sessions => &["view-realm", "view-users", "manage-users"],
            Section::LoginEvents | Section::AdminEvents => &["view-events", "manage-events"],
        }
    }
}

/// Whether a section or a verb can be offered, and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Yes,
    /// Drawn disabled with this reason as the tooltip, never left out.
    No(String),
}

/// What this server and this account can do (§4.1, §8.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Caps {
    /// The declarative user profile drives the user form (24+).
    pub user_profile: bool,
    /// Sub-groups page through `/groups/{id}/children` (23+); before
    /// that they come in the parent's `subGroups`.
    pub group_children: bool,
    /// Organizations (phase 3): 25 with the preview feature, 26 when on.
    pub organizations: bool,
    /// Fine-grained admin permissions v2 (26.2+ with the feature on).
    pub fgap_v2: bool,
    /// Writes are offered at all (not below the tested range).
    pub writes: bool,
    /// Sections a 404 hid for this session.
    pub hidden: BTreeSet<Section>,
    /// The roles the token carries, by the client that grants them
    /// (`realm-management`, or `{realm}-realm` in master), plus `"*"` for
    /// a master `admin`. `None`: the token carries no role claims (a
    /// lightweight token), so there are no hints and the server's 403 is
    /// the only authority.
    pub roles: Option<BTreeMap<String, BTreeSet<String>>>,
    /// The realm the token was issued in, for reading `roles`.
    pub auth_realm: String,
}

impl Caps {
    /// The realm-management roles the account holds in `realm`, or
    /// `None` when the token gives no hints.
    pub fn roles_in(&self, realm: &str) -> Option<BTreeSet<String>> {
        let roles = self.roles.as_ref()?;
        if roles.contains_key("*") {
            return None;
        }
        let client = if realm == self.auth_realm {
            "realm-management".to_string()
        } else {
            format!("{realm}-realm")
        };
        Some(roles.get(&client).cloned().unwrap_or_default())
    }

    /// Whether the account may hold `role` in `realm`: `None` when the
    /// token gives no hints.
    pub fn may(&self, realm: &str, role: &str) -> Option<bool> {
        let held = self.roles_in(realm)?;
        // manage-x implies view-x in Keycloak's realm-management client.
        let implied = role
            .strip_prefix("view-")
            .or_else(|| role.strip_prefix("query-"))
            .map(|rest| format!("manage-{rest}"));
        Some(
            held.contains(role)
                || held.contains("realm-admin")
                || implied.is_some_and(|m| held.contains(&m)),
        )
    }

    /// Whether `section` can be shown in `realm`, with the reason it
    /// cannot.
    pub fn section(&self, realm: &str, section: Section) -> Availability {
        if self.hidden.contains(&section) {
            return Availability::No("this server does not offer it".into());
        }
        let roles = section.read_roles();
        match roles.iter().map(|r| self.may(realm, r)).collect::<Vec<_>>() {
            v if v.iter().any(|m| *m != Some(false)) => Availability::Yes,
            _ => Availability::No(format!(
                "this account lacks the realm-management role “{}”",
                roles[0]
            )),
        }
    }

    /// Whether a write that needs `role` can be offered in `realm`.
    pub fn write(&self, realm: &str, role: &str) -> Availability {
        if !self.writes {
            return Availability::No("this server is older than the supported range".into());
        }
        match self.may(realm, role) {
            Some(false) => Availability::No(format!(
                "this account lacks the realm-management role “{role}”"
            )),
            _ => Availability::Yes,
        }
    }
}

// ------------------------------------------------------------- objects

/// An existing object a write acts on: its id, and the name a person
/// knows it by, for the audit log and the messages (a username, a group
/// or role name, a client id).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjRef {
    pub id: String,
    pub name: String,
}

impl ObjRef {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> ObjRef {
        ObjRef {
            id: id.into(),
            name: name.into(),
        }
    }
}

/// A change to an existing object: the fields edited, by the server's
/// field name, with their new values. The provider reads the object
/// again, merges these into the **whole** representation and writes
/// that (§4.3: a partial body is never sent).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Change {
    pub fields: Map<String, Value>,
}

impl Change {
    pub fn set(mut self, field: &str, value: impl Into<Value>) -> Change {
        self.fields.insert(field.to_string(), value.into());
        self
    }

    /// The names of what changed, for the audit log - never the values.
    pub fn names(&self) -> Vec<String> {
        self.fields.keys().cloned().collect()
    }

    /// `current` with the edited fields put in, everything else kept.
    pub fn merged_into(&self, mut current: Value) -> Value {
        if let Value::Object(map) = &mut current {
            for (k, v) in &self.fields {
                map.insert(k.clone(), v.clone());
            }
        }
        current
    }

    /// The fields whose value differs from `current`, as (name, old,
    /// new), for the review before a save (§6.2). Values are the
    /// server's JSON; masking is the screen's.
    pub fn review(&self, current: &Value) -> Vec<(String, Value, Value)> {
        self.fields
            .iter()
            .filter_map(|(k, v)| {
                let old = current.get(k).cloned().unwrap_or(Value::Null);
                (old != *v).then(|| (k.clone(), old, v.clone()))
            })
            .collect()
    }
}

/// A secret the server gave back (a client secret). `Debug` prints no
/// value; the screens mask it until Show (§6.5).
#[derive(Clone)]
pub struct Secret(pub SecretString);

impl Secret {
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(••••••••)")
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}

// ---------------------------------------------------------------- realms

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Realm {
    pub id: String,
    /// The realm's name, what every path uses.
    pub name: String,
    pub display_name: String,
    pub enabled: bool,
}

/// The group a Realm Settings fact belongs to (§4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactGroup {
    General,
    Login,
    Tokens,
    Security,
    Events,
}

/// One read-only fact of Realm Settings: its group, the server's field
/// name (the screen's label key) and the value as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub group: FactGroup,
    pub field: &'static str,
    pub value: String,
}

/// What the realm stores of its events (`/events/config`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventsConfig {
    pub events_enabled: bool,
    /// Seconds; `None` keeps them for ever.
    pub events_expiration: Option<u64>,
    pub admin_events_enabled: bool,
    pub admin_events_details_enabled: bool,
    pub enabled_event_types: Vec<String>,
}

/// Realm Settings, read-only in phase 1.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RealmSettings {
    pub realm: Realm,
    pub facts: Vec<Fact>,
    /// `None` when the account may not read the events config.
    pub events: Option<EventsConfig>,
    /// The whole representation, for More Settings.
    pub raw: Value,
}

// ----------------------------------------------------------------- users

/// The Users list's filters (§4.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserQuery {
    /// Username, email, first and last name; an `attr:value` term is an
    /// attribute search.
    pub search: String,
    pub exact: bool,
    /// Any (`None`), enabled or disabled.
    pub enabled: Option<bool>,
}

impl UserQuery {
    /// The search split into free text and `attr:value` terms.
    pub fn terms(&self) -> (String, Vec<(String, String)>) {
        let mut text = Vec::new();
        let mut attrs = Vec::new();
        for word in self.search.split_whitespace() {
            match word.split_once(':') {
                Some((k, v)) if !k.is_empty() && !v.is_empty() && !v.starts_with("//") => {
                    attrs.push((k.to_string(), v.to_string()))
                }
                _ => text.push(word),
            }
        }
        (text.join(" "), attrs)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub enabled: bool,
    pub email_verified: bool,
    /// The federation provider's id when the user comes from LDAP or
    /// another provider.
    pub federation_link: Option<String>,
    /// Milliseconds since the epoch.
    pub created: Option<i64>,
}

impl UserRow {
    /// "First Last", or nothing.
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }

    pub fn as_ref(&self) -> ObjRef {
        ObjRef::new(&self.id, &self.username)
    }
}

/// One attribute of the declarative user profile (24+).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProfileAttribute {
    pub name: String,
    /// As the server gives it; `${username}` is a message key.
    pub display_name: String,
    pub required: bool,
    pub read_only: bool,
    pub multivalued: bool,
    pub group: Option<String>,
    /// The validators by name, as the server gave them, for the hints.
    pub validators: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileGroup {
    pub name: String,
    pub header: String,
    pub description: String,
}

/// `userProfileMetadata` of a user.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserProfile {
    pub attributes: Vec<ProfileAttribute>,
    pub groups: Vec<ProfileGroup>,
}

impl UserProfile {
    /// Whether `name` is managed by the profile (the rest are unmanaged
    /// attributes, §4.3.1).
    pub fn manages(&self, name: &str) -> bool {
        self.attributes.iter().any(|a| a.name == name)
    }
}

/// The realm's policy for attributes the profile does not manage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UnmanagedPolicy {
    /// Unset: unmanaged attributes are hidden.
    #[default]
    Off,
    Enabled,
    AdminEdit,
    AdminView,
}

impl UnmanagedPolicy {
    pub fn parse(s: &str) -> UnmanagedPolicy {
        match s {
            "ENABLED" => UnmanagedPolicy::Enabled,
            "ADMIN_EDIT" => UnmanagedPolicy::AdminEdit,
            "ADMIN_VIEW" => UnmanagedPolicy::AdminView,
            _ => UnmanagedPolicy::Off,
        }
    }

    /// Whether the Attributes tab shows unmanaged attributes, and whether
    /// it may edit them.
    pub fn shows(self) -> bool {
        self != UnmanagedPolicy::Off
    }
    pub fn edits(self) -> bool {
        matches!(self, UnmanagedPolicy::Enabled | UnmanagedPolicy::AdminEdit)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct User {
    pub row: UserRow,
    pub required_actions: Vec<String>,
    pub attributes: BTreeMap<String, Vec<String>>,
    /// The declarative profile (24+); `None` before.
    pub profile: Option<UserProfile>,
    /// The whole representation as read, for the review and the merge.
    pub raw: Value,
}

/// A new user (§4.3; owner, 2026-11-08: create in phase 1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewUser {
    pub username: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub enabled: bool,
    pub email_verified: bool,
    pub required_actions: Vec<String>,
}

/// A realm's required action, for the Required Actions pick list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequiredAction {
    pub alias: String,
    pub name: String,
    pub enabled: bool,
}

/// A credential's metadata - never its secret, which the server does not
/// return.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialInfo {
    pub id: String,
    /// password, otp, webauthn …
    pub kind: String,
    pub label: String,
    pub created: Option<i64>,
}

// ---------------------------------------------------------------- groups

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
    /// `/engineering/backend`.
    pub path: String,
    pub parent_id: Option<String>,
    /// Where the server gives it (23+).
    pub sub_group_count: Option<u64>,
    /// The children a 22 server puts in the parent.
    pub sub_groups: Vec<Group>,
    pub attributes: BTreeMap<String, Vec<String>>,
    pub raw: Value,
}

impl Group {
    pub fn as_ref(&self) -> ObjRef {
        ObjRef::new(&self.id, &self.path)
    }
}

// ----------------------------------------------------------------- roles

/// Which roles: the realm's, or one client's (by the client's internal
/// id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleScope {
    Realm,
    Client(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub description: String,
    pub composite: bool,
    pub client_role: bool,
    /// The realm's id, or the client's internal id.
    pub container_id: String,
    pub raw: Value,
}

impl Role {
    pub fn as_ref(&self) -> ObjRef {
        ObjRef::new(&self.id, &self.name)
    }

    /// Built-in roles are never offered for deletion (§4.5).
    pub fn is_builtin(&self, realm: &str, client_id: Option<&str>) -> bool {
        is_builtin_role(&self.name, realm, client_id)
    }
}

/// `offline_access`, `uma_authorization`, `default-roles-{realm}` and
/// every role of the `realm-management` client.
pub fn is_builtin_role(name: &str, realm: &str, client_id: Option<&str>) -> bool {
    match client_id {
        Some(c) => c == "realm-management" || c == "broker" || c == "account",
        None => {
            name == "offline_access"
                || name == "uma_authorization"
                || name == format!("default-roles-{realm}")
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewRole {
    pub name: String,
    pub description: String,
}

/// Whose role mappings: a user's or a group's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    User(String),
    Group(String),
}

/// One client's roles in a mapping.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClientRoles {
    /// The client's internal id.
    pub id: String,
    /// Its client id, `orders-api`.
    pub client: String,
    pub roles: Vec<Role>,
}

/// The roles assigned directly to a subject.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoleMappings {
    pub realm: Vec<Role>,
    pub clients: Vec<ClientRoles>,
}

// --------------------------------------------------------------- clients

/// How a client authenticates (§4.6's Access column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Confidential,
    Public,
    BearerOnly,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Client {
    /// The internal id every path uses.
    pub id: String,
    /// `web-app`.
    pub client_id: String,
    pub name: String,
    pub description: String,
    /// `openid-connect` or `saml`.
    pub protocol: String,
    pub enabled: bool,
    pub public_client: bool,
    pub bearer_only: bool,
    pub service_accounts: bool,
    pub raw: Value,
}

impl Client {
    pub fn access(&self) -> Access {
        if self.bearer_only {
            Access::BearerOnly
        } else if self.public_client {
            Access::Public
        } else {
            Access::Confidential
        }
    }

    pub fn as_ref(&self) -> ObjRef {
        ObjRef::new(&self.id, &self.client_id)
    }

    /// The realm's own clients open read-only and are never deleted
    /// (§4.6).
    pub fn is_builtin(&self, realm: &str) -> bool {
        is_builtin_client(&self.client_id, realm)
    }
}

/// `account`, `account-console`, `admin-cli`, `broker`,
/// `realm-management`, `security-admin-console`, and in master the
/// `{realm}-realm` clients.
pub fn is_builtin_client(client_id: &str, realm: &str) -> bool {
    matches!(
        client_id,
        "account"
            | "account-console"
            | "admin-cli"
            | "broker"
            | "realm-management"
            | "security-admin-console"
    ) || (realm == "master" && client_id.ends_with("-realm"))
}

/// A new client (owner, 2026-11-08: create in phase 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewClient {
    pub client_id: String,
    pub name: String,
    pub description: String,
    /// `openid-connect` or `saml`.
    pub protocol: String,
    pub public_client: bool,
    pub service_accounts: bool,
    pub standard_flow: bool,
    pub direct_access_grants: bool,
    pub redirect_uris: Vec<String>,
    pub web_origins: Vec<String>,
    pub enabled: bool,
}

impl Default for NewClient {
    fn default() -> Self {
        NewClient {
            client_id: String::new(),
            name: String::new(),
            description: String::new(),
            protocol: "openid-connect".into(),
            public_client: false,
            service_accounts: false,
            standard_flow: true,
            direct_access_grants: false,
            redirect_uris: Vec::new(),
            web_origins: Vec::new(),
            enabled: true,
        }
    }
}

// -------------------------------------------------------------- sessions

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub username: String,
    pub user_id: String,
    pub ip_address: String,
    /// Milliseconds since the epoch.
    pub started: i64,
    pub last_access: i64,
    /// Client id by the client's internal id.
    pub clients: BTreeMap<String, String>,
}

/// A client's session counts, from `/client-session-stats`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientSessionStats {
    pub id: String,
    pub client_id: String,
    pub active: u64,
    pub offline: u64,
}

/// What Log Out All Sessions reached (§4.7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogoutAll {
    /// Client URLs the server could not tell.
    pub failed: Vec<String>,
}

// ---------------------------------------------------------------- events

/// The Login Events filters (§4.8). Dates are `YYYY-MM-DD`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventQuery {
    pub types: Vec<String>,
    pub client: String,
    /// A user's id (the screen resolves a username first).
    pub user: String,
    pub ip: String,
    pub date_from: String,
    pub date_to: String,
}

/// The Admin Events filters (§4.8).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminEventQuery {
    pub operation_types: Vec<String>,
    pub resource_types: Vec<String>,
    pub resource_path: String,
    pub auth_realm: String,
    pub auth_client: String,
    pub auth_user: String,
    pub auth_ip: String,
    pub date_from: String,
    pub date_to: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoginEvent {
    /// Milliseconds since the epoch.
    pub time: i64,
    pub kind: String,
    pub client_id: String,
    pub user_id: String,
    pub session_id: String,
    pub ip_address: String,
    pub error: String,
    pub details: BTreeMap<String, String>,
}

impl LoginEvent {
    /// `ERROR` for a type ending in `_ERROR` or with an error set, `INFO`
    /// otherwise - the word the shared log-level colours paint (§4.8).
    pub fn level(&self) -> &'static str {
        if self.kind.ends_with("_ERROR") || !self.error.is_empty() {
            "ERROR"
        } else {
            "INFO"
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminEvent {
    pub time: i64,
    /// CREATE, UPDATE, DELETE, ACTION.
    pub operation: String,
    pub resource_type: String,
    pub resource_path: String,
    pub auth_realm: String,
    pub auth_client: String,
    pub auth_user: String,
    pub auth_ip: String,
    /// The representation the event carries, **already masked**
    /// (`crate::mask`), when the realm stores details.
    pub representation: Option<String>,
    pub error: String,
}

/// What a partial export includes (§4.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportScope {
    pub clients: bool,
    pub groups_and_roles: bool,
}

impl Default for ExportScope {
    fn default() -> Self {
        ExportScope {
            clients: true,
            groups_and_roles: true,
        }
    }
}

// ----------------------------------------------------------------- trait

/// One identity product. Every method that changes the server is listed
/// in [`crate::guarded::WRITES`] and refused there on a read-only tab; a
/// test reads this trait's source and fails when a method is in neither
/// list.
#[async_trait]
pub trait IdmProvider: Send + Sync {
    // server
    /// The token, then the server's facts (§4.2).
    async fn connect(&self) -> Result<ServerInfo>;
    fn info(&self) -> ServerInfo;
    fn capabilities(&self) -> Caps;
    /// The admin console's URL for a page (`#/{realm}/users/{id}/settings`),
    /// for Open in Admin Console.
    fn console_url(&self, realm: &str, page: &str) -> String;
    /// End the session on the server where the grant made one (the
    /// password kind's admin session).
    async fn disconnect(&self);

    // realms
    async fn realms(&self) -> Result<Vec<Realm>>;
    async fn realm_settings(&self, realm: &str) -> Result<RealmSettings>;
    async fn events_config(&self, realm: &str) -> Result<EventsConfig>;

    // users
    async fn users(&self, realm: &str, q: &UserQuery, page: Page) -> Result<Paged<UserRow>>;
    async fn user_count(&self, realm: &str, q: &UserQuery) -> Result<Option<u64>>;
    async fn user(&self, realm: &str, id: &str) -> Result<User>;
    async fn unmanaged_policy(&self, realm: &str) -> Result<UnmanagedPolicy>;
    async fn required_actions(&self, realm: &str) -> Result<Vec<RequiredAction>>;
    async fn user_credentials(&self, realm: &str, id: &str) -> Result<Vec<CredentialInfo>>;
    async fn user_groups(&self, realm: &str, id: &str, page: Page) -> Result<Paged<Group>>;
    async fn user_sessions(&self, realm: &str, id: &str) -> Result<Vec<Session>>;
    async fn user_offline_sessions(
        &self,
        realm: &str,
        id: &str,
        client: &str,
    ) -> Result<Vec<Session>>;
    /// Creates the user and returns its id.
    async fn create_user(&self, realm: &str, user: &NewUser) -> Result<String>;
    async fn update_user(&self, realm: &str, user: &ObjRef, change: &Change) -> Result<()>;
    async fn set_user_enabled(&self, realm: &str, user: &ObjRef, enabled: bool) -> Result<()>;
    async fn reset_password(
        &self,
        realm: &str,
        user: &ObjRef,
        password: SecretString,
        temporary: bool,
    ) -> Result<()>;
    async fn delete_user(&self, realm: &str, user: &ObjRef) -> Result<()>;
    async fn join_group(&self, realm: &str, user: &ObjRef, group: &ObjRef) -> Result<()>;
    async fn leave_group(&self, realm: &str, user: &ObjRef, group: &ObjRef) -> Result<()>;
    /// Ends every session of the user.
    async fn logout_user(&self, realm: &str, user: &ObjRef) -> Result<()>;
    async fn end_session(&self, realm: &str, session: &ObjRef) -> Result<()>;

    // groups
    async fn groups(&self, realm: &str, search: &str, page: Page) -> Result<Paged<Group>>;
    async fn group_count(&self, realm: &str, search: &str) -> Result<Option<u64>>;
    async fn group(&self, realm: &str, id: &str) -> Result<Group>;
    async fn group_children(&self, realm: &str, id: &str, page: Page) -> Result<Paged<Group>>;
    async fn group_members(&self, realm: &str, id: &str, page: Page) -> Result<Paged<UserRow>>;
    /// Creates a top-level group, or a sub-group of `parent`; returns its
    /// id.
    async fn create_group(
        &self,
        realm: &str,
        parent: Option<&ObjRef>,
        name: &str,
    ) -> Result<String>;
    async fn update_group(&self, realm: &str, group: &ObjRef, change: &Change) -> Result<()>;
    async fn delete_group(&self, realm: &str, group: &ObjRef) -> Result<()>;

    // role mappings
    async fn role_mappings(&self, realm: &str, who: &Subject) -> Result<RoleMappings>;
    /// Realm roles assigned, `effective` adding the inherited ones.
    async fn realm_role_mappings(
        &self,
        realm: &str,
        who: &Subject,
        effective: bool,
    ) -> Result<Vec<Role>>;
    async fn available_realm_roles(&self, realm: &str, who: &Subject) -> Result<Vec<Role>>;
    async fn client_role_mappings(
        &self,
        realm: &str,
        who: &Subject,
        client: &str,
        effective: bool,
    ) -> Result<Vec<Role>>;
    async fn available_client_roles(
        &self,
        realm: &str,
        who: &Subject,
        client: &str,
    ) -> Result<Vec<Role>>;
    async fn add_realm_roles(&self, realm: &str, who: &Subject, roles: &[Role]) -> Result<()>;
    async fn remove_realm_roles(&self, realm: &str, who: &Subject, roles: &[Role]) -> Result<()>;
    async fn add_client_roles(
        &self,
        realm: &str,
        who: &Subject,
        client: &ObjRef,
        roles: &[Role],
    ) -> Result<()>;
    async fn remove_client_roles(
        &self,
        realm: &str,
        who: &Subject,
        client: &ObjRef,
        roles: &[Role],
    ) -> Result<()>;

    // roles
    async fn roles(
        &self,
        realm: &str,
        scope: &RoleScope,
        search: &str,
        page: Page,
    ) -> Result<Paged<Role>>;
    async fn role(&self, realm: &str, id: &str) -> Result<Role>;
    async fn role_composites(&self, realm: &str, id: &str) -> Result<Vec<Role>>;
    async fn role_users(
        &self,
        realm: &str,
        scope: &RoleScope,
        name: &str,
        page: Page,
    ) -> Result<Paged<UserRow>>;
    async fn role_groups(
        &self,
        realm: &str,
        scope: &RoleScope,
        name: &str,
        page: Page,
    ) -> Result<Paged<Group>>;
    /// Creates the role and returns it as the server stored it.
    async fn create_role(&self, realm: &str, scope: &RoleScope, role: &NewRole) -> Result<Role>;
    async fn update_role(&self, realm: &str, role: &ObjRef, change: &Change) -> Result<()>;
    async fn delete_role(&self, realm: &str, role: &ObjRef) -> Result<()>;

    // clients
    async fn clients(&self, realm: &str, search: &str, page: Page) -> Result<Paged<Client>>;
    async fn client(&self, realm: &str, id: &str) -> Result<Client>;
    async fn client_secret(&self, realm: &str, id: &str) -> Result<Secret>;
    async fn service_account_user(&self, realm: &str, id: &str) -> Result<UserRow>;
    async fn client_sessions(&self, realm: &str, id: &str, page: Page) -> Result<Paged<Session>>;
    async fn client_session_count(&self, realm: &str, id: &str) -> Result<u64>;
    /// Creates the client and returns its internal id.
    async fn create_client(&self, realm: &str, client: &NewClient) -> Result<String>;
    async fn update_client(&self, realm: &str, client: &ObjRef, change: &Change) -> Result<()>;
    async fn delete_client(&self, realm: &str, client: &ObjRef) -> Result<()>;
    async fn regenerate_secret(&self, realm: &str, client: &ObjRef) -> Result<Secret>;

    // sessions
    async fn client_session_stats(&self, realm: &str) -> Result<Vec<ClientSessionStats>>;
    async fn logout_all(&self, realm: &str) -> Result<LogoutAll>;

    // events
    async fn login_events(
        &self,
        realm: &str,
        q: &EventQuery,
        page: Page,
    ) -> Result<Paged<LoginEvent>>;
    async fn admin_events(
        &self,
        realm: &str,
        q: &AdminEventQuery,
        page: Page,
    ) -> Result<Paged<AdminEvent>>;

    // export
    /// The realm as JSON, **already masked** (§4.9, §6.5). Cancelling
    /// drops the request.
    async fn export(
        &self,
        realm: &str,
        scope: ExportScope,
        cancel: CancellationToken,
    ) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_steps_and_stays_in_range() {
        assert_eq!(Page::of(5).max, PAGE_MIN);
        assert_eq!(Page::of(1000).max, PAGE_MAX);
        let p = Page::of(50);
        assert_eq!(p.next(), Page { first: 50, max: 50 });
        assert_eq!(p.previous(), None);
        assert_eq!(p.next().previous(), Some(p));
    }

    #[test]
    fn the_extra_row_says_there_is_more_and_is_dropped() {
        let page = Page { first: 0, max: 2 };
        let p = Paged::from_probe(vec![1, 2, 3], page, None);
        assert_eq!(p.rows, vec![1, 2]);
        assert!(p.more);
        assert_eq!(p.span(), (1, 2, None));
        let last = Paged::from_probe(vec![5], page.next(), Some(5));
        assert!(!last.more);
        assert_eq!(last.span(), (3, 3, Some(5)));
        let empty: Paged<u8> = Paged::from_probe(vec![], page, Some(0));
        assert_eq!(empty.span(), (0, 0, Some(0)));
    }

    #[test]
    fn a_search_splits_into_text_and_attribute_terms() {
        let q = UserQuery {
            search: "ali department:Engineering lee".into(),
            ..Default::default()
        };
        assert_eq!(
            q.terms(),
            (
                "ali lee".to_string(),
                vec![("department".to_string(), "Engineering".to_string())]
            )
        );
        // A URL-ish word is text, not an attribute.
        let q = UserQuery {
            search: "http://x".into(),
            ..Default::default()
        };
        assert_eq!(q.terms().0, "http://x");
    }

    #[test]
    fn a_change_merges_into_the_whole_representation_and_reviews_what_differs() {
        let current = serde_json::json!({
            "id": "1", "username": "alice", "email": "a@x", "attributes": {"dept": ["Eng"]},
            "unknownNewField": 7
        });
        let change = Change::default()
            .set("email", "alice@x")
            .set("username", "alice");
        let merged = change.merged_into(current.clone());
        assert_eq!(merged["email"], "alice@x");
        // Everything not edited is kept, unknown fields included.
        assert_eq!(merged["attributes"]["dept"][0], "Eng");
        assert_eq!(merged["unknownNewField"], 7);
        let review = change.review(&current);
        assert_eq!(review.len(), 1, "an unchanged field is not reviewed");
        assert_eq!(review[0].0, "email");
        assert_eq!(change.names(), vec!["email", "username"]);
    }

    #[test]
    fn role_hints_gate_sections_and_writes() {
        let mut roles = BTreeMap::new();
        roles.insert(
            "realm-management".to_string(),
            ["view-users", "manage-clients"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        let caps = Caps {
            writes: true,
            roles: Some(roles),
            auth_realm: "hs-demo".into(),
            ..Default::default()
        };
        assert_eq!(caps.section("hs-demo", Section::Users), Availability::Yes);
        // manage-clients implies view-clients.
        assert_eq!(caps.section("hs-demo", Section::Clients), Availability::Yes);
        assert!(matches!(
            caps.section("hs-demo", Section::LoginEvents),
            Availability::No(_)
        ));
        assert!(matches!(
            caps.write("hs-demo", "manage-users"),
            Availability::No(_)
        ));
        // Another realm's roles live on its {realm}-realm client in master.
        assert_eq!(caps.may("hs-other", "view-users"), Some(false));
        // A token with no role claims gives no hints at all.
        let none = Caps {
            writes: true,
            ..Default::default()
        };
        assert_eq!(none.may("hs-demo", "manage-users"), None);
        assert_eq!(
            none.section("hs-demo", Section::AdminEvents),
            Availability::Yes
        );
        let mut hidden = none.clone();
        hidden.hidden.insert(Section::AdminEvents);
        assert!(matches!(
            hidden.section("hs-demo", Section::AdminEvents),
            Availability::No(_)
        ));
    }

    #[test]
    fn built_in_clients_and_roles_are_recognised() {
        assert!(is_builtin_client("admin-cli", "hs-demo"));
        assert!(is_builtin_client("hs-demo-realm", "master"));
        assert!(!is_builtin_client("hs-demo-realm", "hs-demo"));
        assert!(!is_builtin_client("web-app", "hs-demo"));
        assert!(is_builtin_role("default-roles-hs-demo", "hs-demo", None));
        assert!(is_builtin_role("offline_access", "hs-demo", None));
        assert!(!is_builtin_role("editor", "hs-demo", None));
        assert!(is_builtin_role(
            "view-users",
            "hs-demo",
            Some("realm-management")
        ));
        assert!(!is_builtin_role(
            "orders:read",
            "hs-demo",
            Some("orders-api")
        ));
    }

    #[test]
    fn a_login_event_level_is_error_or_info() {
        let mut e = LoginEvent {
            kind: "LOGIN_ERROR".into(),
            ..Default::default()
        };
        assert_eq!(e.level(), "ERROR");
        e.kind = "LOGIN".into();
        assert_eq!(e.level(), "INFO");
        e.error = "invalid_user_credentials".into();
        assert_eq!(e.level(), "ERROR");
    }

    #[test]
    fn a_secret_never_prints() {
        let s = Secret(SecretString::from("s3cr3t".to_string()));
        assert!(!format!("{s:?}").contains("s3cr3t"));
        assert_eq!(s.expose(), "s3cr3t");
    }

    #[test]
    fn the_audience_error_tells_the_admin_what_to_add() {
        let e = IdmError::Audience {
            client_id: "hs-admin".into(),
        }
        .to_string();
        assert!(e.contains("kc-admin-api"), "{e}");
        assert!(e.contains("hs-admin"), "{e}");
        let f = IdmError::Forbidden {
            realm: "hs-other".into(),
            needs: None,
        }
        .to_string();
        assert!(f.contains("no permission") && f.contains("hs-other"), "{f}");
    }
}
