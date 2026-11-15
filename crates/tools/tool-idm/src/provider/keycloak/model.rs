//! Keycloak's JSON, read into the shared types. Every type takes
//! `#[serde(default)]` and ignores fields it does not know, so a newer
//! server still parses; the whole representation is kept in `raw` where a
//! save has to write it back (§4.3's read-merge-write).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::mask;
use crate::provider::*;

/// `v` as `T`, or the server's answer named as unreadable.
pub fn parse<T: DeserializeOwned>(v: Value, what: &str) -> Result<T> {
    serde_json::from_value(v)
        .map_err(|e| IdmError::Other(format!("the server's {what} could not be read: {e}")))
}

/// A list of `T`, each kept with its own JSON.
pub fn parse_list<T, K>(v: Value, what: &str, f: impl Fn(K, Value) -> T) -> Result<Vec<T>>
where
    K: DeserializeOwned,
{
    let Value::Array(items) = v else {
        return Err(IdmError::Other(format!(
            "the server's {what} was not a list"
        )));
    };
    items
        .into_iter()
        .map(|item| Ok(f(parse::<K>(item.clone(), what)?, item)))
        .collect()
}

/// Keycloak sends some numbers as strings (`client-session-stats`).
fn num(v: &Value) -> u64 {
    match v {
        Value::Number(n) => n.as_u64().unwrap_or(0),
        Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcRealm {
    pub id: String,
    pub realm: String,
    pub display_name: String,
    pub enabled: bool,
}

impl From<KcRealm> for Realm {
    fn from(r: KcRealm) -> Realm {
        Realm {
            id: r.id,
            name: r.realm,
            display_name: r.display_name,
            enabled: r.enabled,
        }
    }
}

/// Realm Settings' facts, in §4.2's groups. A field the server left out
/// is left out here too.
pub fn realm_facts(v: &Value) -> Vec<Fact> {
    use FactGroup::*;
    const FIELDS: &[(FactGroup, &str)] = &[
        (General, "realm"),
        (General, "displayName"),
        (General, "enabled"),
        (General, "sslRequired"),
        (General, "frontendUrl"),
        (Login, "registrationAllowed"),
        (Login, "rememberMe"),
        (Login, "loginWithEmailAllowed"),
        (Login, "verifyEmail"),
        (Login, "resetPasswordAllowed"),
        (Login, "editUsernameAllowed"),
        (Tokens, "accessTokenLifespan"),
        (Tokens, "ssoSessionIdleTimeout"),
        (Tokens, "ssoSessionMaxLifespan"),
        (Tokens, "offlineSessionIdleTimeout"),
        (Security, "bruteForceProtected"),
        (Security, "failureFactor"),
    ];
    FIELDS
        .iter()
        .filter_map(|(group, field)| {
            // frontendUrl lives under attributes.
            let value = v
                .get(*field)
                .or_else(|| v.get("attributes").and_then(|a| a.get(*field)))?;
            Some(Fact {
                group: *group,
                field,
                value: match value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                },
            })
        })
        .collect()
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcEventsConfig {
    pub events_enabled: bool,
    pub events_expiration: Option<u64>,
    pub admin_events_enabled: bool,
    pub admin_events_details_enabled: bool,
    pub enabled_event_types: Vec<String>,
}

impl From<KcEventsConfig> for EventsConfig {
    fn from(c: KcEventsConfig) -> EventsConfig {
        EventsConfig {
            events_enabled: c.events_enabled,
            events_expiration: c.events_expiration,
            admin_events_enabled: c.admin_events_enabled,
            admin_events_details_enabled: c.admin_events_details_enabled,
            enabled_event_types: c.enabled_event_types,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcUser {
    pub id: String,
    pub username: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub enabled: bool,
    pub email_verified: bool,
    pub federation_link: Option<String>,
    pub created_timestamp: Option<i64>,
    pub required_actions: Vec<String>,
    pub attributes: BTreeMap<String, Vec<String>>,
    pub user_profile_metadata: Option<KcProfile>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcProfile {
    pub attributes: Vec<KcProfileAttribute>,
    pub groups: Vec<KcProfileGroup>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcProfileAttribute {
    pub name: String,
    pub display_name: String,
    pub required: bool,
    pub read_only: bool,
    pub multivalued: bool,
    pub group: Option<String>,
    pub validators: serde_json::Map<String, Value>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcProfileGroup {
    pub name: String,
    pub display_header: String,
    pub display_description: String,
}

impl KcUser {
    pub fn row(&self) -> UserRow {
        UserRow {
            id: self.id.clone(),
            username: self.username.clone(),
            email: self.email.clone(),
            first_name: self.first_name.clone(),
            last_name: self.last_name.clone(),
            enabled: self.enabled,
            email_verified: self.email_verified,
            federation_link: self.federation_link.clone().filter(|f| !f.is_empty()),
            created: self.created_timestamp,
        }
    }

    pub fn into_user(self, raw: Value) -> User {
        let row = self.row();
        let profile = self.user_profile_metadata.map(|p| UserProfile {
            attributes: p
                .attributes
                .into_iter()
                .map(|a| ProfileAttribute {
                    name: a.name,
                    display_name: a.display_name,
                    required: a.required,
                    read_only: a.read_only,
                    multivalued: a.multivalued,
                    group: a.group,
                    validators: a.validators,
                })
                .collect(),
            groups: p
                .groups
                .into_iter()
                .map(|g| ProfileGroup {
                    name: g.name,
                    header: g.display_header,
                    description: g.display_description,
                })
                .collect(),
        });
        User {
            row,
            required_actions: self.required_actions,
            attributes: self.attributes,
            profile,
            raw,
        }
    }
}

pub fn user_row(u: KcUser, _: Value) -> UserRow {
    u.row()
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcRequiredAction {
    pub alias: String,
    pub name: String,
    pub enabled: bool,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcCredential {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub user_label: String,
    pub created_date: Option<i64>,
}

impl From<KcCredential> for CredentialInfo {
    fn from(c: KcCredential) -> CredentialInfo {
        CredentialInfo {
            id: c.id,
            kind: c.kind,
            label: c.user_label,
            created: c.created_date,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcGroup {
    pub id: String,
    pub name: String,
    pub path: String,
    pub parent_id: Option<String>,
    pub sub_group_count: Option<u64>,
    pub sub_groups: Vec<Value>,
    pub attributes: BTreeMap<String, Vec<String>>,
}

pub fn group(g: KcGroup, raw: Value) -> Group {
    let sub_groups = g
        .sub_groups
        .into_iter()
        .filter_map(|v| {
            let kg: KcGroup = serde_json::from_value(v.clone()).ok()?;
            Some(group(kg, v))
        })
        .collect();
    Group {
        id: g.id,
        name: g.name,
        path: g.path,
        parent_id: g.parent_id,
        sub_group_count: g.sub_group_count,
        sub_groups,
        attributes: g.attributes,
        raw,
    }
}

/// The groups of a search answer, flattened: Keycloak answers a search
/// with the matching groups inside their ancestors, and the list shows
/// the matches with their paths (§4.4).
pub fn flatten_matches(groups: Vec<Group>, search: &str, out: &mut Vec<Group>) {
    let needle = search.to_lowercase();
    for mut g in groups {
        let children = std::mem::take(&mut g.sub_groups);
        if g.name.to_lowercase().contains(&needle) {
            out.push(g);
        }
        flatten_matches(children, search, out);
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcRole {
    pub id: String,
    pub name: String,
    pub description: String,
    pub composite: bool,
    pub client_role: bool,
    pub container_id: String,
}

pub fn role(r: KcRole, raw: Value) -> Role {
    Role {
        id: r.id,
        name: r.name,
        description: r.description,
        composite: r.composite,
        client_role: r.client_role,
        container_id: r.container_id,
        raw,
    }
}

/// `GET …/role-mappings`.
pub fn role_mappings(v: &Value) -> RoleMappings {
    let roles = |v: Option<&Value>| -> Vec<Role> {
        v.and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let kr: KcRole = serde_json::from_value(r.clone()).ok()?;
                Some(role(kr, r.clone()))
            })
            .collect()
    };
    let clients = v
        .get("clientMappings")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(client, m)| ClientRoles {
            id: m
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            client: m
                .get("client")
                .and_then(Value::as_str)
                .unwrap_or(client)
                .to_string(),
            roles: roles(m.get("mappings")),
        })
        .collect();
    RoleMappings {
        realm: roles(v.get("realmMappings")),
        clients,
    }
}

/// A role as a mapping request's body carries it: the representation the
/// server gave, or its id and name.
pub fn role_body(r: &Role) -> Value {
    if r.raw.is_object() {
        r.raw.clone()
    } else {
        serde_json::json!({ "id": r.id, "name": r.name })
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcClient {
    pub id: String,
    pub client_id: String,
    pub name: String,
    pub description: String,
    pub protocol: String,
    pub enabled: bool,
    pub public_client: bool,
    pub bearer_only: bool,
    pub service_accounts_enabled: bool,
}

pub fn client(c: KcClient, raw: Value) -> Client {
    Client {
        id: c.id,
        client_id: c.client_id,
        name: c.name,
        description: c.description,
        protocol: if c.protocol.is_empty() {
            "openid-connect".into()
        } else {
            c.protocol
        },
        enabled: c.enabled,
        public_client: c.public_client,
        bearer_only: c.bearer_only,
        service_accounts: c.service_accounts_enabled,
        raw,
    }
}

pub fn new_client_body(c: &NewClient) -> Value {
    serde_json::json!({
        "clientId": c.client_id,
        "name": c.name,
        "description": c.description,
        "protocol": c.protocol,
        "enabled": c.enabled,
        "publicClient": c.public_client,
        "serviceAccountsEnabled": c.service_accounts,
        "standardFlowEnabled": c.standard_flow,
        "directAccessGrantsEnabled": c.direct_access_grants,
        "redirectUris": c.redirect_uris,
        "webOrigins": c.web_origins,
    })
}

pub fn new_user_body(u: &NewUser) -> Value {
    let mut v = serde_json::json!({
        "username": u.username,
        "enabled": u.enabled,
        "emailVerified": u.email_verified,
        "requiredActions": u.required_actions,
    });
    for (k, val) in [
        ("email", &u.email),
        ("firstName", &u.first_name),
        ("lastName", &u.last_name),
    ] {
        if !val.is_empty() {
            v[k] = Value::String(val.clone());
        }
    }
    v
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcSession {
    pub id: String,
    pub username: String,
    pub user_id: String,
    pub ip_address: String,
    pub start: i64,
    pub last_access: i64,
    pub clients: BTreeMap<String, String>,
}

pub fn session(s: KcSession, _: Value) -> Session {
    Session {
        id: s.id,
        username: s.username,
        user_id: s.user_id,
        ip_address: s.ip_address,
        started: s.start,
        last_access: s.last_access,
        clients: s.clients,
    }
}

/// `GET /client-session-stats`: the counts arrive as strings.
pub fn session_stats(v: &Value) -> Vec<ClientSessionStats> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|s| ClientSessionStats {
            id: s.get("id").and_then(Value::as_str).unwrap_or("").into(),
            client_id: s
                .get("clientId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            active: s.get("active").map(num).unwrap_or(0),
            offline: s.get("offline").map(num).unwrap_or(0),
        })
        .collect()
}

/// `POST /logout-all`'s GlobalRequestResult.
pub fn logout_all(v: &Value) -> LogoutAll {
    LogoutAll {
        failed: v
            .get("failedRequests")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
    }
}

/// `{"count": n}` or a bare number.
pub fn count(v: &Value) -> u64 {
    v.get("count").map(num).unwrap_or_else(|| num(v))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcEvent {
    pub time: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub client_id: String,
    pub user_id: String,
    pub session_id: String,
    pub ip_address: String,
    pub error: String,
    pub details: BTreeMap<String, Value>,
}

pub fn login_event(e: KcEvent, _: Value) -> LoginEvent {
    let details: BTreeMap<String, String> = e
        .details
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                match v {
                    Value::String(s) => s,
                    other => other.to_string(),
                },
            )
        })
        .collect();
    LoginEvent {
        time: e.time,
        kind: e.kind,
        client_id: e.client_id,
        user_id: e.user_id,
        session_id: e.session_id,
        ip_address: e.ip_address,
        error: e.error,
        details,
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcAdminEvent {
    pub time: i64,
    pub operation_type: String,
    pub resource_type: String,
    pub resource_path: String,
    pub auth_details: KcAuthDetails,
    pub representation: Option<String>,
    pub error: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KcAuthDetails {
    pub realm_id: String,
    pub client_id: String,
    pub user_id: String,
    pub ip_address: String,
}

pub fn admin_event(e: KcAdminEvent, _: Value) -> AdminEvent {
    AdminEvent {
        time: e.time,
        operation: e.operation_type,
        resource_type: e.resource_type,
        resource_path: e.resource_path,
        auth_realm: e.auth_details.realm_id,
        auth_client: e.auth_details.client_id,
        auth_user: e.auth_details.user_id,
        auth_ip: e.auth_details.ip_address,
        // Masked before anything shows it; a representation that is not
        // JSON is withheld rather than shown unchecked (§6.5).
        representation: e
            .representation
            .filter(|r| !r.trim().is_empty())
            .map(|r| mask::mask_text(&r).unwrap_or_else(|| mask::MASK.to_string())),
        error: e.error,
    }
}

/// A 4xx body's own words: `errorMessage` (and `field`), else
/// `error_description`, else `error`.
pub fn error_words(body: &str) -> (String, Option<String>) {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(String::from);
    let message = s("errorMessage")
        .or_else(|| s("error_description"))
        .or_else(|| s("error"))
        .unwrap_or_else(|| {
            let t = body.trim();
            if t.starts_with('<') {
                String::new()
            } else {
                t.chars().take(200).collect()
            }
        });
    (message, s("field"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_user_reads_with_its_profile_and_ignores_unknown_fields() {
        let raw = json!({
            "id": "u1", "username": "alice.nguyen", "firstName": "Alice", "lastName": "Nguyen",
            "email": "alice.nguyen@hs-demo.test", "emailVerified": true, "enabled": true,
            "attributes": {"department": ["Engineering"]}, "createdTimestamp": 1794118632240i64,
            "requiredActions": ["UPDATE_PASSWORD"], "someFieldFromKeycloak30": {"x": 1},
            "userProfileMetadata": {"attributes": [
                {"name": "username", "displayName": "${username}", "required": true, "readOnly": true,
                 "validators": {"length": {"min": 3}}, "multivalued": false}
            ], "groups": [{"name": "user-metadata", "displayHeader": "User metadata"}]}
        });
        let u = parse::<KcUser>(raw.clone(), "user").unwrap().into_user(raw);
        assert_eq!(u.row.full_name(), "Alice Nguyen");
        assert_eq!(u.required_actions, vec!["UPDATE_PASSWORD"]);
        let p = u.profile.expect("profile");
        assert!(p.manages("username") && !p.manages("department"));
        assert!(p.attributes[0].read_only);
        assert_eq!(p.groups[0].header, "User metadata");
        assert_eq!(u.raw["someFieldFromKeycloak30"]["x"], 1);
    }

    #[test]
    fn groups_read_in_the_22_and_the_23_shapes_and_searches_flatten() {
        // 23+: brief, with a count and no children.
        let v = json!([{"id": "g1", "name": "engineering", "path": "/engineering", "subGroupCount": 2, "subGroups": []}]);
        let gs = parse_list(v, "groups", group).unwrap();
        assert_eq!(gs[0].sub_group_count, Some(2));
        // 22: the children inside the parent.
        let v = json!([{"id": "g1", "name": "engineering", "path": "/engineering", "subGroups": [
            {"id": "g2", "name": "backend", "path": "/engineering/backend", "subGroups": []}
        ]}]);
        let gs = parse_list(v, "groups", group).unwrap();
        assert_eq!(gs[0].sub_groups[0].path, "/engineering/backend");
        let mut out = Vec::new();
        flatten_matches(gs, "back", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, "/engineering/backend");
    }

    #[test]
    fn role_mappings_read_realm_and_client_roles() {
        let v = json!({
            "realmMappings": [{"id": "r1", "name": "admin", "composite": false, "clientRole": false}],
            "clientMappings": {"orders-api": {"id": "c1", "client": "orders-api", "mappings": [
                {"id": "r2", "name": "orders:read", "clientRole": true}
            ]}}
        });
        let m = role_mappings(&v);
        assert_eq!(m.realm[0].name, "admin");
        assert_eq!(m.clients[0].client, "orders-api");
        assert_eq!(m.clients[0].roles[0].name, "orders:read");
        assert_eq!(role_body(&m.realm[0])["id"], "r1");
    }

    #[test]
    fn clients_sessions_and_events_read() {
        let v = json!([{"id": "c1", "clientId": "web-app", "publicClient": true, "enabled": true}]);
        let c = parse_list(v, "clients", client).unwrap();
        assert_eq!(c[0].access(), Access::Public);
        assert_eq!(c[0].protocol, "openid-connect");
        let stats = session_stats(
            &json!([{"offline": "0", "clientId": "web-app", "active": "8", "id": "c1"}]),
        );
        assert_eq!(stats[0].active, 8);
        assert_eq!(count(&json!({"count": 5})), 5);
        assert_eq!(count(&json!(7)), 7);
        let s = parse_list(
            json!([{"id": "s1", "username": "alice", "userId": "u1", "ipAddress": "127.0.0.1",
                     "start": 1, "lastAccess": 2, "clients": {"c1": "web-app"}}]),
            "sessions",
            session,
        )
        .unwrap();
        assert_eq!(s[0].clients["c1"], "web-app");
        let e = parse_list(
            json!([{"time": 1, "type": "LOGIN_ERROR", "clientId": "web-app", "error": "invalid_user_credentials",
                    "ipAddress": "10.0.0.1", "details": {"username": "alice", "auth_method": "openid-connect"}}]),
            "events",
            login_event,
        )
        .unwrap();
        assert_eq!(e[0].level(), "ERROR");
        assert_eq!(e[0].details["username"], "alice");
    }

    #[test]
    fn an_admin_event_representation_is_masked_before_anyone_sees_it() {
        let rep = r#"{"clientId":"orders-api","secret":"leaked-on-the-wire"}"#;
        let v = json!([{"time": 1, "operationType": "UPDATE", "resourceType": "CLIENT",
                        "resourcePath": "clients/c1", "authDetails": {"realmId": "m", "userId": "u"},
                        "representation": rep}]);
        let e = parse_list(v, "admin events", admin_event).unwrap();
        let shown = e[0].representation.clone().expect("representation");
        assert!(!shown.contains("leaked-on-the-wire"));
        assert!(shown.contains("orders-api"));
        let v = json!([{"representation": "not json at all"}]);
        let e = parse_list(v, "admin events", admin_event).unwrap();
        assert_eq!(e[0].representation.as_deref(), Some(mask::MASK));
    }

    #[test]
    fn realm_facts_follow_the_groups() {
        let v = json!({"realm": "hs-demo", "displayName": "HyperStack Demo", "enabled": true,
                       "accessTokenLifespan": 300, "bruteForceProtected": false,
                       "attributes": {"frontendUrl": "https://id.example.test"}});
        let facts = realm_facts(&v);
        assert!(
            facts
                .iter()
                .any(|f| f.field == "accessTokenLifespan" && f.value == "300")
        );
        assert!(
            facts
                .iter()
                .any(|f| f.field == "frontendUrl" && f.group == FactGroup::General)
        );
        assert!(
            !facts.iter().any(|f| f.field == "verifyEmail"),
            "absent stays absent"
        );
    }

    #[test]
    fn error_bodies_give_their_words_and_field() {
        assert_eq!(
            error_words(r#"{"errorMessage":"User exists with same username"}"#),
            ("User exists with same username".into(), None)
        );
        assert_eq!(
            error_words(
                r#"{"field":"email","errorMessage":"error-invalid-email","params":["email","x"]}"#
            ),
            ("error-invalid-email".into(), Some("email".into()))
        );
        assert_eq!(
            error_words(r#"{"error":"HTTP 403 Forbidden"}"#).0,
            "HTTP 403 Forbidden"
        );
        assert_eq!(error_words("<html></html>").0, "");
    }
}
