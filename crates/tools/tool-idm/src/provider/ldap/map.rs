//! The pure half of the LDAP provider (docs/tools/idm.md §4.12): search
//! filters, DN pieces, an entry read as the shared user and group shapes,
//! and a [`Change`] turned into LDAP modifications. No I/O here, so all of
//! it is unit-tested.
//!
//! A user's `raw` is written in Keycloak's field names (`username`,
//! `firstName`, `lastName`, `email`, `attributes`), so the user page's form
//! and its review work unchanged; [`user_mods`] maps an edit back.

use std::collections::{BTreeMap, HashMap};

use ldap3::{dn_escape, ldap_escape};
use serde_json::{Map, Value, json};

use crate::provider::{Change, DirEntry, Group, IdmError, Result, User, UserQuery, UserRow};

/// Every person, posix account or not, unless the profile says otherwise.
pub const DEFAULT_USER_FILTER: &str = "(|(objectClass=person)(objectClass=posixAccount))";
/// The three group shapes a directory holds.
pub const DEFAULT_GROUP_FILTER: &str =
    "(|(objectClass=groupOfNames)(objectClass=groupOfUniqueNames)(objectClass=posixGroup))";

/// The attributes the shared user fields come from; the rest of an entry
/// is its `attributes`.
const USER_FIELDS: [&str; 4] = ["uid", "givenName", "sn", "mail"];
/// Never shown and never edited as a plain attribute: the class list is
/// the entry's shape, and a password is never read back (§4.12).
const HIDDEN: [&str; 2] = ["objectClass", "userPassword"];
/// Read with every entry: the server's own facts.
pub const OPERATIONAL: [&str; 4] = [
    "createTimestamp",
    "modifyTimestamp",
    "entryUUID",
    "hasSubordinates",
];

/// An entry as read: its DN and attributes, names as the server spells
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub dn: String,
    pub attrs: HashMap<String, Vec<String>>,
}

impl Entry {
    /// The first value of `name`, matched without case, or "".
    pub fn first(&self, name: &str) -> String {
        self.values(name).first().cloned().unwrap_or_default()
    }

    /// Every value of `name`, matched without case.
    pub fn values(&self, name: &str) -> Vec<String> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.values("objectClass")
            .iter()
            .any(|c| c.eq_ignore_ascii_case(class))
    }
}

// ---------------------------------------------------------------- filters

/// A user search: the profile's filter, every free-text word in a name,
/// the username or the mail, and every `attr:value` term (§4.3).
pub fn user_filter(base: &str, q: &UserQuery) -> String {
    let base = if base.trim().is_empty() {
        DEFAULT_USER_FILTER
    } else {
        base.trim()
    };
    let mut parts = vec![wrap(base)];
    // A directory has no enabled flag: "Disabled only" finds no one.
    if q.enabled == Some(false) {
        parts.push("(!(objectClass=*))".into());
    }
    let (text, attrs) = q.terms();
    for word in text.split_whitespace() {
        let w = ldap_escape(word);
        parts.push(if q.exact {
            format!("(|(uid={w})(mail={w})(cn={w}))")
        } else {
            format!("(|(uid=*{w}*)(cn=*{w}*)(mail=*{w}*)(givenName=*{w}*)(sn=*{w}*))")
        });
    }
    for (k, v) in attrs {
        let v = ldap_escape(v.as_str());
        if is_attr_name(&k) {
            parts.push(format!("({k}={v})"));
        } else {
            parts.push(format!("(cn=*{v}*)"));
        }
    }
    and(parts)
}

/// A group search by name.
pub fn group_filter(base: &str, search: &str) -> String {
    let base = if base.trim().is_empty() {
        DEFAULT_GROUP_FILTER
    } else {
        base.trim()
    };
    let mut parts = vec![wrap(base)];
    for word in search.split_whitespace() {
        parts.push(format!("(cn=*{}*)", ldap_escape(word)));
    }
    and(parts)
}

/// The groups that list `dn` (or, for a posix group, `uid`) as a member.
pub fn member_of_filter(base: &str, dn: &str, uid: &str) -> String {
    let base = if base.trim().is_empty() {
        DEFAULT_GROUP_FILTER
    } else {
        base.trim()
    };
    let d = ldap_escape(dn);
    let mut any = format!("(member={d})(uniqueMember={d})");
    if !uid.is_empty() {
        any.push_str(&format!("(memberUid={})", ldap_escape(uid)));
    }
    format!("(&{}(|{any}))", wrap(base))
}

fn wrap(f: &str) -> String {
    if f.starts_with('(') {
        f.to_string()
    } else {
        format!("({f})")
    }
}

fn and(parts: Vec<String>) -> String {
    if parts.len() == 1 {
        parts.into_iter().next().unwrap_or_default()
    } else {
        format!("(&{})", parts.concat())
    }
}

/// An attribute description: a name, or an OID.
pub fn is_attr_name(s: &str) -> bool {
    let mut c = s.chars();
    match c.next() {
        Some(f) if f.is_ascii_alphabetic() => c.all(|c| c.is_ascii_alphanumeric() || c == '-'),
        Some(f) if f.is_ascii_digit() => s.chars().all(|c| c.is_ascii_digit() || c == '.'),
        _ => false,
    }
}

// --------------------------------------------------------------------- DN

/// A DN's components, split on unescaped commas.
pub fn components(dn: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for ch in dn.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
        } else if ch == '\\' {
            cur.push(ch);
            escaped = true;
        } else if ch == ',' {
            out.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// The first component as (attribute, value), the value unescaped.
pub fn rdn(dn: &str) -> (String, String) {
    let first = components(dn).into_iter().next().unwrap_or_default();
    match first.split_once('=') {
        Some((a, v)) => (a.trim().to_string(), unescape(v.trim())),
        None => (String::new(), first),
    }
}

/// The DN one level up, or "" at the top.
pub fn parent(dn: &str) -> String {
    components(dn)
        .into_iter()
        .skip(1)
        .collect::<Vec<_>>()
        .join(",")
}

/// Whether `dn` is `base` or under it, compared without case or spaces
/// around the commas.
pub fn is_under(dn: &str, base: &str) -> bool {
    let norm = |s: &str| components(s).join(",").to_ascii_lowercase();
    let (d, b) = (norm(dn), norm(base));
    !b.is_empty() && (d == b || d.ends_with(&format!(",{b}")))
}

/// A new entry's DN: `attr=value` (the value escaped) under `parent`.
pub fn child(attr: &str, value: &str, parent: &str) -> String {
    format!("{attr}={},{parent}", dn_escape(value))
}

/// An RDN value with its escapes undone: `\,` and the like, and `\c3\a9`
/// hex pairs, which are UTF-8 bytes (RFC 4514).
fn unescape(v: &str) -> String {
    let mut bytes = Vec::with_capacity(v.len());
    let mut chars = v.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        let a = chars.next();
        let b = chars.peek().copied();
        match (a, b) {
            (Some(h1), Some(h2)) if h1.is_ascii_hexdigit() && h2.is_ascii_hexdigit() => {
                chars.next();
                bytes.push(u8::from_str_radix(&format!("{h1}{h2}"), 16).unwrap_or(b'?'));
            }
            (Some(x), _) => {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(x.encode_utf8(&mut buf).as_bytes());
            }
            (None, _) => {}
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

// ---------------------------------------------------------------- entries

/// `20261119083000Z` (or with a fraction) as milliseconds since the epoch.
pub fn generalized_time(s: &str) -> Option<i64> {
    let digits: String = s.chars().take(14).collect();
    let t = chrono::NaiveDateTime::parse_from_str(&digits, "%Y%m%d%H%M%S").ok()?;
    Some(t.and_utc().timestamp_millis())
}

/// The username an entry is known by: `uid`, else its RDN's value.
pub fn username(e: &Entry) -> String {
    let uid = e.first("uid");
    if uid.is_empty() { rdn(&e.dn).1 } else { uid }
}

/// The Users list's row. A directory has no enabled flag, so every entry
/// reads enabled.
pub fn user_row(e: &Entry) -> UserRow {
    UserRow {
        id: e.dn.clone(),
        username: username(e),
        email: e.first("mail"),
        first_name: e.first("givenName"),
        last_name: e.first("sn"),
        enabled: true,
        email_verified: false,
        federation_link: None,
        created: generalized_time(&e.first("createTimestamp")),
    }
}

/// The entry's plain attributes: everything but the shared fields, the
/// hidden ones and the server's operational facts, sorted by name.
pub fn plain_attributes(e: &Entry) -> BTreeMap<String, Vec<String>> {
    e.attrs
        .iter()
        .filter(|(k, _)| {
            !USER_FIELDS
                .iter()
                .chain(HIDDEN.iter())
                .chain(OPERATIONAL.iter())
                .any(|f| f.eq_ignore_ascii_case(k))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// A user, its `raw` in Keycloak's field names.
pub fn user(e: &Entry) -> User {
    let row = user_row(e);
    let attributes = plain_attributes(e);
    let raw = json!({
        "id": row.id,
        "username": row.username,
        "email": row.email,
        "firstName": row.first_name,
        "lastName": row.last_name,
        "enabled": true,
        "emailVerified": false,
        "requiredActions": [],
        "attributes": attributes,
    });
    User {
        row,
        required_actions: Vec::new(),
        attributes,
        profile: None,
        raw,
    }
}

/// The member attribute a group keeps its members in, and whether the
/// values are DNs (`memberUid` holds usernames).
pub fn member_attr(e: &Entry) -> (&'static str, bool) {
    if e.has_class("groupOfUniqueNames") {
        ("uniqueMember", true)
    } else if e.has_class("posixGroup") && !e.has_class("groupOfNames") {
        ("memberUid", false)
    } else {
        ("member", true)
    }
}

/// A group. Its children are the member values that are groups, which
/// the caller knows only by reading them; `sub_group_count` is left to it.
pub fn group(e: &Entry) -> Group {
    let mut attributes = plain_attributes(e);
    for a in ["cn", "member", "uniqueMember", "memberUid"] {
        attributes.retain(|k, _| !k.eq_ignore_ascii_case(a));
    }
    let name = {
        let cn = e.first("cn");
        if cn.is_empty() { rdn(&e.dn).1 } else { cn }
    };
    // The path is what a list shows: the name, as Keycloak's `/name`
    // reads; the DN stays the id.
    let path = format!("/{name}");
    let raw = json!({
        "id": e.dn,
        "name": name,
        "path": path,
        "attributes": attributes,
    });
    Group {
        id: e.dn.clone(),
        name,
        path,
        parent_id: None,
        sub_group_count: None,
        sub_groups: Vec::new(),
        attributes,
        raw,
    }
}

// ---------------------------------------------------------------- changes

/// One LDAP modification, in our own words; the provider turns it into
/// `ldap3::Mod`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModOp {
    Replace(String, Vec<String>),
    Delete(String),
}

/// What a user edit asks of the directory: a new RDN value when the
/// username is the RDN and it changed, and the modifications.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserEdit {
    pub rename: Option<String>,
    pub mods: Vec<ModOp>,
}

/// A user page's [`Change`] turned into modifications of `e`. A field a
/// directory does not have (enabled, verified e-mail, required actions)
/// is refused in words when it changed, never dropped quietly.
pub fn user_mods(e: &Entry, change: &Change) -> Result<UserEdit> {
    let mut edit = UserEdit::default();
    let current = user(e).raw;
    for (field, value) in &change.fields {
        if current.get(field) == Some(value) {
            continue;
        }
        match field.as_str() {
            "username" => {
                let new = text(value);
                if new.is_empty() {
                    return Err(invalid("the username cannot be empty", "username"));
                }
                if rdn(&e.dn).0.eq_ignore_ascii_case("uid") {
                    edit.rename = Some(new.clone());
                }
                edit.mods.push(ModOp::Replace("uid".into(), vec![new]));
            }
            "firstName" => edit.mods.push(set_or_delete("givenName", text(value))),
            "lastName" => {
                let new = text(value);
                if new.is_empty() && e.has_class("person") {
                    return Err(invalid(
                        "a person entry must keep its surname (sn)",
                        "lastName",
                    ));
                }
                edit.mods.push(set_or_delete("sn", new));
            }
            "email" => edit.mods.push(set_or_delete("mail", text(value))),
            "attributes" => edit.mods.extend(attribute_mods(e, value)?),
            "enabled" | "emailVerified" | "requiredActions" => {
                return Err(IdmError::Other(format!(
                    "a directory has no “{field}”: it is a Keycloak setting"
                )));
            }
            other => {
                return Err(invalid(
                    &format!("“{other}” is not a directory field"),
                    other,
                ));
            }
        }
    }
    Ok(edit)
}

/// A group page's [`Change`]: a new name renames the entry (its `cn`
/// RDN), attributes become modifications.
pub fn group_mods(e: &Entry, change: &Change) -> Result<UserEdit> {
    let mut edit = UserEdit::default();
    let current = group(e).raw;
    for (field, value) in &change.fields {
        if current.get(field) == Some(value) {
            continue;
        }
        match field.as_str() {
            "name" => {
                let new = text(value);
                if new.is_empty() {
                    return Err(invalid("the name cannot be empty", "name"));
                }
                edit.rename = Some(new);
            }
            "attributes" => edit.mods.extend(attribute_mods(e, value)?),
            other => {
                return Err(invalid(
                    &format!("“{other}” is not a directory field"),
                    other,
                ));
            }
        }
    }
    Ok(edit)
}

/// The plain attributes edited: changed ones replaced, removed ones
/// deleted, the hidden and shared ones left alone.
fn attribute_mods(e: &Entry, value: &Value) -> Result<Vec<ModOp>> {
    let Value::Object(new) = value else {
        return Err(invalid("attributes are a map of lists", "attributes"));
    };
    let after: BTreeMap<String, Vec<String>> =
        new.iter().map(|(k, v)| (k.clone(), list(v))).collect();
    mods_between(&plain_attributes(e), &after)
}

/// The modifications that turn `before` into `after`: changed attributes
/// replaced, missing ones deleted, the hidden ones never touched.
fn mods_between(
    before: &BTreeMap<String, Vec<String>>,
    after: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<ModOp>> {
    let new = after;
    let mut mods = Vec::new();
    for (name, values) in new {
        if !is_attr_name(name) {
            return Err(invalid(
                &format!("“{name}” is not an attribute name"),
                "attributes",
            ));
        }
        if HIDDEN.iter().any(|h| h.eq_ignore_ascii_case(name)) {
            continue;
        }
        let values: Vec<String> = values.iter().filter(|v| !v.is_empty()).cloned().collect();
        let old = before
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone());
        if old.as_ref() == Some(&values) {
            continue;
        }
        mods.push(if values.is_empty() {
            ModOp::Delete(name.clone())
        } else {
            ModOp::Replace(name.clone(), values)
        });
    }
    for name in before.keys() {
        if !new.keys().any(|k| k.eq_ignore_ascii_case(name)) {
            mods.push(ModOp::Delete(name.clone()));
        }
    }
    Ok(mods)
}

fn set_or_delete(attr: &str, value: String) -> ModOp {
    if value.is_empty() {
        ModOp::Delete(attr.into())
    } else {
        ModOp::Replace(attr.into(), vec![value])
    }
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().trim().to_string()
}

fn list(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .collect(),
        Value::String(s) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn invalid(message: &str, field: &str) -> IdmError {
    IdmError::Invalid {
        message: message.to_string(),
        field: Some(field.to_string()),
    }
}

/// The attributes a new user entry is created with.
pub fn new_user_attrs(
    username: &str,
    first: &str,
    last: &str,
    email: &str,
) -> Vec<(String, Vec<String>)> {
    let cn = format!("{first} {last}").trim().to_string();
    let mut a = vec![
        (
            "objectClass".to_string(),
            vec![
                "top".into(),
                "person".into(),
                "organizationalPerson".into(),
                "inetOrgPerson".into(),
            ],
        ),
        ("uid".into(), vec![username.into()]),
        (
            "cn".into(),
            vec![if cn.is_empty() { username.into() } else { cn }],
        ),
        (
            "sn".into(),
            vec![if last.is_empty() {
                username.into()
            } else {
                last.into()
            }],
        ),
    ];
    if !first.is_empty() {
        a.push(("givenName".into(), vec![first.into()]));
    }
    if !email.is_empty() {
        a.push(("mail".into(), vec![email.into()]));
    }
    a
}

/// The Keycloak-shaped JSON of a map, for a raw that is not a user.
pub fn as_map(attrs: &BTreeMap<String, Vec<String>>) -> Map<String, Value> {
    attrs.iter().map(|(k, v)| (k.clone(), json!(v))).collect()
}

// -------------------------------------------------------------- directory

/// An entry as the Directory section shows it (§4.12): every attribute
/// but the class list, the password and the server's facts.
pub fn dir_entry(e: &Entry) -> DirEntry {
    let attributes = e
        .attrs
        .iter()
        .filter(|(k, _)| {
            !HIDDEN
                .iter()
                .chain(OPERATIONAL.iter())
                .any(|f| f.eq_ignore_ascii_case(k))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    DirEntry {
        dn: e.dn.clone(),
        rdn: components(&e.dn).into_iter().next().unwrap_or_default(),
        classes: e.values("objectClass"),
        attributes,
        // A server that does not say reads as "may have some".
        has_children: !e.first("hasSubordinates").eq_ignore_ascii_case("FALSE"),
    }
}

/// The Directory editor's whole attribute set against what `e` holds. The
/// RDN's attribute keeps its RDN value: a rename is a move, not an edit.
pub fn dir_mods(e: &Entry, after: &BTreeMap<String, Vec<String>>) -> Result<Vec<ModOp>> {
    let (rdn_attr, rdn_value) = rdn(&e.dn);
    if let Some((_, values)) = after
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(&rdn_attr))
        && !values.iter().any(|v| v == &rdn_value)
    {
        return Err(invalid(
            &format!("{rdn_attr} must keep “{rdn_value}”: it names the entry"),
            &rdn_attr,
        ));
    }
    if !after.keys().any(|k| k.eq_ignore_ascii_case(&rdn_attr)) && !rdn_attr.is_empty() {
        return Err(invalid(
            &format!("{rdn_attr} names the entry and cannot be removed"),
            &rdn_attr,
        ));
    }
    mods_between(&dir_entry(e).attributes, after)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(dn: &str, attrs: &[(&str, &[&str])]) -> Entry {
        Entry {
            dn: dn.into(),
            attrs: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
                .collect(),
        }
    }

    fn ada() -> Entry {
        entry(
            "uid=ada,ou=engineering,ou=people,dc=hyperstack,dc=test",
            &[
                ("objectClass", &["top", "person", "inetOrgPerson"]),
                ("uid", &["ada"]),
                ("cn", &["Ada Lovelace"]),
                ("givenName", &["Ada"]),
                ("sn", &["Lovelace"]),
                ("mail", &["ada@hyperstack.test"]),
                ("title", &["Principal Engineer"]),
                ("userPassword", &["{SSHA}x"]),
                ("createTimestamp", &["20261119083000Z"]),
            ],
        )
    }

    #[test]
    fn a_search_escapes_its_words_and_keeps_attribute_terms() {
        let q = UserQuery {
            search: "a*d title:Engineer".into(),
            ..Default::default()
        };
        let f = user_filter("", &q);
        assert!(f.starts_with("(&(|(objectClass=person)"), "{f}");
        assert!(f.contains("(uid=*a\\2ad*)"), "{f}");
        assert!(f.contains("(title=Engineer)"), "{f}");
        // A filter the profile gives without parentheses is wrapped.
        assert_eq!(
            user_filter("objectClass=inetOrgPerson", &UserQuery::default()),
            "(objectClass=inetOrgPerson)"
        );
        // Disabled only finds no one: a directory has no enabled flag.
        let off = user_filter(
            "",
            &UserQuery {
                enabled: Some(false),
                ..Default::default()
            },
        );
        assert!(off.contains("(!(objectClass=*))"), "{off}");
        // An injection attempt stays a value.
        let bad = user_filter(
            "",
            &UserQuery {
                search: "*)(uid=*".into(),
                ..Default::default()
            },
        );
        assert!(!bad.contains(")(uid=*)"), "{bad}");
    }

    #[test]
    fn membership_is_searched_by_dn_and_by_posix_username() {
        let f = member_of_filter("", "uid=a,dc=x", "a");
        assert!(
            f.contains("(member=uid=a,dc=x)(uniqueMember=uid=a,dc=x)(memberUid=a)"),
            "{f}"
        );
    }

    #[test]
    fn a_dn_splits_on_unescaped_commas_and_compares_without_case() {
        let dn = r"cn=Smith\, John,ou=People,DC=example,DC=org";
        assert_eq!(components(dn).len(), 4);
        assert_eq!(rdn(dn), ("cn".into(), "Smith, John".into()));
        assert_eq!(parent(dn), "ou=People,DC=example,DC=org");
        assert!(is_under(dn, "dc=example, dc=org"));
        assert!(is_under("dc=example,dc=org", "dc=example,dc=org"));
        assert!(!is_under("dc=badexample,dc=org", "dc=example,dc=org"));
        // ldap3 escapes as hex pairs; either spelling reads back the same.
        let made = child("uid", "o,neil", "ou=p");
        assert_eq!(components(&made).len(), 2);
        assert_eq!(rdn(&made).1, "o,neil");
        assert_eq!(rdn(r"cn=caf\c3\a9,dc=x").1, "café");
    }

    #[test]
    fn an_entry_reads_as_a_user_without_its_password() {
        let u = user(&ada());
        assert_eq!(u.row.username, "ada");
        assert_eq!(u.row.first_name, "Ada");
        assert_eq!(u.row.created, Some(1_795_077_000_000));
        assert!(u.attributes.contains_key("title"));
        assert!(u.attributes.contains_key("cn"));
        assert!(
            !u.attributes
                .keys()
                .any(|k| k == "userPassword" || k == "objectClass" || k == "uid")
        );
        assert_eq!(u.raw["firstName"], "Ada");
        assert!(!u.raw.to_string().contains("SSHA"));
    }

    #[test]
    fn an_edit_becomes_modifications_and_a_rename() {
        let e = ada();
        let change = Change::default()
            .set("firstName", "Augusta")
            .set("email", "")
            .set("username", "augusta")
            .set(
                "attributes",
                json!({"cn": ["Ada Lovelace"], "telephoneNumber": ["+60 3"]}),
            );
        let edit = user_mods(&e, &change).unwrap();
        assert_eq!(edit.rename.as_deref(), Some("augusta"));
        assert!(
            edit.mods
                .contains(&ModOp::Replace("givenName".into(), vec!["Augusta".into()]))
        );
        assert!(edit.mods.contains(&ModOp::Delete("mail".into())));
        assert!(edit.mods.contains(&ModOp::Replace(
            "telephoneNumber".into(),
            vec!["+60 3".into()]
        )));
        // title was dropped from the map: deleted. cn kept: untouched.
        assert!(edit.mods.contains(&ModOp::Delete("title".into())));
        assert!(
            !edit
                .mods
                .iter()
                .any(|m| matches!(m, ModOp::Replace(a, _) | ModOp::Delete(a) if a == "cn"))
        );
        // An unchanged field is not written at all.
        let same = user_mods(&e, &Change::default().set("firstName", "Ada")).unwrap();
        assert!(same.mods.is_empty() && same.rename.is_none());
    }

    #[test]
    fn a_keycloak_only_field_is_refused_and_a_surname_kept() {
        let e = ada();
        let off = user_mods(&e, &Change::default().set("enabled", false)).unwrap_err();
        assert!(off.to_string().contains("enabled"), "{off}");
        // Unchanged, it passes: the form sends it with every save.
        assert!(user_mods(&e, &Change::default().set("enabled", true)).is_ok());
        let sn = user_mods(&e, &Change::default().set("lastName", "")).unwrap_err();
        assert!(sn.to_string().contains("sn"), "{sn}");
        let pw = user_mods(
            &e,
            &Change::default().set("attributes", json!({"userPassword": ["x"]})),
        )
        .unwrap();
        assert!(
            !pw.mods
                .iter()
                .any(|m| matches!(m, ModOp::Replace(a, _) if a == "userPassword"))
        );
    }

    #[test]
    fn a_group_knows_where_its_members_are_and_renames_by_name() {
        let g = entry(
            "cn=backend,ou=groups,dc=x",
            &[
                ("objectClass", &["groupOfNames"]),
                ("cn", &["backend"]),
                ("member", &["uid=a,dc=x"]),
                ("description", &["APIs"]),
            ],
        );
        assert_eq!(member_attr(&g), ("member", true));
        let shown = group(&g);
        assert_eq!(shown.name, "backend");
        assert!(shown.attributes.contains_key("description"));
        assert!(!shown.attributes.contains_key("member"));
        let posix = entry(
            "cn=dev,dc=x",
            &[("objectClass", &["posixGroup"]), ("memberUid", &["a"])],
        );
        assert_eq!(member_attr(&posix), ("memberUid", false));
        let unique = entry("cn=adm,dc=x", &[("objectClass", &["groupOfUniqueNames"])]);
        assert_eq!(member_attr(&unique), ("uniqueMember", true));
        let edit = group_mods(&g, &Change::default().set("name", "services")).unwrap();
        assert_eq!(edit.rename.as_deref(), Some("services"));
    }

    #[test]
    fn a_new_user_has_the_classes_and_the_must_attributes() {
        let a = new_user_attrs("grace", "Grace", "", "");
        let get = |n: &str| a.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        assert_eq!(get("cn"), Some(vec!["Grace".to_string()]));
        assert_eq!(get("sn"), Some(vec!["grace".to_string()]));
        assert!(get("mail").is_none());
        assert!(
            get("objectClass")
                .unwrap()
                .contains(&"inetOrgPerson".to_string())
        );
    }

    #[test]
    fn a_directory_entry_hides_its_facts_and_guards_its_name() {
        let ou = entry(
            "ou=people,dc=x",
            &[
                ("objectClass", &["organizationalUnit"]),
                ("ou", &["people"]),
                ("description", &["Everyone"]),
                ("hasSubordinates", &["TRUE"]),
                ("entryUUID", &["u"]),
            ],
        );
        let d = dir_entry(&ou);
        assert_eq!(d.rdn, "ou=people");
        assert!(d.has_children);
        assert_eq!(d.kind(), crate::provider::DirKind::Container);
        assert_eq!(
            d.attributes.keys().collect::<Vec<_>>(),
            ["description", "ou"]
        );
        let leaf = entry(
            "cn=a,dc=x",
            &[
                ("objectClass", &["person"]),
                ("hasSubordinates", &["FALSE"]),
            ],
        );
        assert!(!dir_entry(&leaf).has_children);
        // An edit: description changed, a new attribute, the RDN kept.
        let mut after = d.attributes.clone();
        after.insert("description".into(), vec!["All people".into()]);
        after.insert("seeAlso".into(), vec!["cn=a,dc=x".into()]);
        let mods = dir_mods(&ou, &after).unwrap();
        assert!(mods.contains(&ModOp::Replace(
            "description".into(),
            vec!["All people".into()]
        )));
        assert!(mods.contains(&ModOp::Replace("seeAlso".into(), vec!["cn=a,dc=x".into()])));
        assert_eq!(mods.len(), 2);
        // The RDN's value cannot go, nor the attribute.
        after.insert("ou".into(), vec!["staff".into()]);
        assert!(dir_mods(&ou, &after).is_err());
        after.remove("ou");
        assert!(dir_mods(&ou, &after).is_err());
    }
}
