//! The API client's data model (docs/tools/api-client.md §4, §7): requests,
//! folders and collections, plus the variable scopes. Collections are saved
//! as one JSON file each under `<workspace>/api/collections/<id>.json`;
//! globals in `<workspace>/api/globals.json`; history in
//! `<workspace>/api/history.jsonl`. Secrets never appear here: a value may
//! be a `secret:<id>` reference into the vault, resolved only when sending.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Kv {
    pub enabled: bool,
    pub key: String,
    pub value: String,
    pub description: String,
}

impl Default for Kv {
    fn default() -> Self {
        Self {
            enabled: true,
            key: String::new(),
            value: String::new(),
            description: String::new(),
        }
    }
}

impl Kv {
    pub fn new(key: &str, value: &str) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    #[default]
    None,
    Json,
    Text,
    Xml,
    Form,
    Multipart,
    Binary,
}

impl BodyKind {
    pub const ALL: [BodyKind; 7] = [
        BodyKind::None,
        BodyKind::Json,
        BodyKind::Text,
        BodyKind::Xml,
        BodyKind::Form,
        BodyKind::Multipart,
        BodyKind::Binary,
    ];
    pub fn label(self) -> &'static str {
        match self {
            BodyKind::None => "None",
            BodyKind::Json => "JSON",
            BodyKind::Text => "Text",
            BodyKind::Xml => "XML",
            BodyKind::Form => "Form",
            BodyKind::Multipart => "Multipart",
            BodyKind::Binary => "Binary File",
        }
    }
    pub fn content_type(self) -> Option<&'static str> {
        match self {
            BodyKind::Json => Some("application/json"),
            BodyKind::Text => Some("text/plain"),
            BodyKind::Xml => Some("application/xml"),
            BodyKind::Form => Some("application/x-www-form-urlencoded"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Body {
    pub kind: BodyKind,
    /// JSON, text or XML.
    pub text: String,
    /// Form and multipart fields. A multipart row whose `description` starts
    /// with `file:` sends the file at that path.
    pub form: Vec<Kv>,
    /// Binary: path of the file to send.
    pub file: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    #[default]
    Inherit,
    None,
    Basic,
    Bearer,
    ApiKey,
}

impl AuthKind {
    pub const ALL: [AuthKind; 5] = [
        AuthKind::Inherit,
        AuthKind::None,
        AuthKind::Basic,
        AuthKind::Bearer,
        AuthKind::ApiKey,
    ];
    pub fn label(self) -> &'static str {
        match self {
            AuthKind::Inherit => "Inherit",
            AuthKind::None => "No Auth",
            AuthKind::Basic => "Basic",
            AuthKind::Bearer => "Bearer Token",
            AuthKind::ApiKey => "API Key",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Auth {
    pub kind: AuthKind,
    pub username: String,
    pub password: String,
    pub token: String,
    pub key: String,
    pub value: String,
    /// `header` or `query` for API keys.
    pub add_to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Request {
    pub id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub params: Vec<Kv>,
    pub headers: Vec<Kv>,
    pub body: Body,
    pub auth: Auth,
    pub pre_script: String,
    pub post_script: String,
    pub docs: String,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            id: hs_core::new_id(),
            name: "New Request".into(),
            method: "GET".into(),
            url: String::new(),
            params: Vec::new(),
            headers: Vec::new(),
            body: Body::default(),
            auth: Auth::default(),
            pre_script: String::new(),
            post_script: String::new(),
            docs: String::new(),
        }
    }
}

pub const METHODS: [&str; 8] = [
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    Request(Request),
    Folder(Folder),
}

impl Item {
    pub fn id(&self) -> &str {
        match self {
            Item::Request(r) => &r.id,
            Item::Folder(f) => &f.id,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Item::Request(r) => &r.name,
            Item::Folder(f) => &f.name,
        }
    }

    pub fn set_name(&mut self, name: &str) {
        match self {
            Item::Request(r) => r.name = name.to_string(),
            Item::Folder(f) => f.name = name.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub items: Vec<Item>,
    pub auth: Auth,
    pub pre_script: String,
    pub post_script: String,
    pub docs: String,
}

impl Default for Folder {
    fn default() -> Self {
        Self {
            id: hs_core::new_id(),
            name: "New Folder".into(),
            items: Vec::new(),
            auth: Auth::default(),
            pre_script: String::new(),
            post_script: String::new(),
            docs: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Collection {
    pub schema: u32,
    pub id: String,
    pub name: String,
    /// Where this collection sits in the sidebar. Collections are read
    /// from a directory, which has no order of its own, so one is kept
    /// here; equal values fall back to the name.
    #[serde(default)]
    pub order: i32,
    pub items: Vec<Item>,
    pub variables: Vec<Kv>,
    pub auth: Auth,
    pub pre_script: String,
    pub post_script: String,
    /// JavaScript libraries evaluated before every script of this
    /// collection (helpers, a pasted pure-JS library).
    pub libraries: Vec<Library>,
    pub docs: String,
}

impl Default for Collection {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            id: hs_core::new_id(),
            name: "New Collection".into(),
            order: 0,
            items: Vec::new(),
            variables: Vec::new(),
            auth: Auth {
                kind: AuthKind::None,
                ..Default::default()
            },
            pre_script: String::new(),
            post_script: String::new(),
            libraries: Vec::new(),
            docs: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Library {
    pub name: String,
    pub enabled: bool,
    pub code: String,
}

/// Where a request sits: collection id and the folder ids down to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Location {
    pub collection: String,
    pub folders: Vec<String>,
}

impl Collection {
    pub fn path(dir: &Path, id: &str) -> PathBuf {
        dir.join(format!("{id}.json"))
    }

    pub fn load_all(dir: &Path) -> Vec<Collection> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                match hs_core::storage::read_json::<Collection>(&p) {
                    Ok(c) => out.push(c),
                    Err(e) => tracing::warn!("collection {}: {e}", p.display()),
                }
            }
        }
        out.sort_by(|a, b| {
            a.order
                .cmp(&b.order)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    pub fn save(&self, dir: &Path) -> hs_core::storage::Result<()> {
        std::fs::create_dir_all(dir).map_err(|e| hs_core::storage::StorageError::Io {
            path: dir.display().to_string(),
            source: e,
        })?;
        hs_core::storage::write_json(&Self::path(dir, &self.id), self)
    }

    /// Walk to a request by id, returning it with the folders above it.
    pub fn find(&self, id: &str) -> Option<(&Request, Vec<&Folder>)> {
        fn walk<'a>(
            items: &'a [Item],
            id: &str,
            stack: &mut Vec<&'a Folder>,
        ) -> Option<&'a Request> {
            for it in items {
                match it {
                    Item::Request(r) if r.id == id => return Some(r),
                    Item::Folder(f) => {
                        stack.push(f);
                        if let Some(r) = walk(&f.items, id, stack) {
                            return Some(r);
                        }
                        stack.pop();
                    }
                    _ => {}
                }
            }
            None
        }
        let mut stack = Vec::new();
        walk(&self.items, id, &mut stack).map(|r| (r, stack))
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Request> {
        fn walk<'a>(items: &'a mut [Item], id: &str) -> Option<&'a mut Request> {
            for it in items {
                match it {
                    Item::Request(r) if r.id == id => return Some(r),
                    Item::Folder(f) => {
                        if let Some(r) = walk(&mut f.items, id) {
                            return Some(r);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        walk(&mut self.items, id)
    }

    /// Items of the folder `folder` (`None` = the collection root), for
    /// reading - naming a new item against what is already beside it.
    pub fn items(&self, folder: Option<&str>) -> Option<&Vec<Item>> {
        fn walk<'a>(items: &'a [Item], id: &str) -> Option<&'a Vec<Item>> {
            for it in items.iter() {
                if let Item::Folder(f) = it {
                    if f.id == id {
                        return Some(&f.items);
                    }
                    if let Some(v) = walk(&f.items, id) {
                        return Some(v);
                    }
                }
            }
            None
        }
        match folder {
            None => Some(&self.items),
            Some(id) => walk(&self.items, id),
        }
    }

    /// Items of the folder `folder` (`None` = the collection root).
    pub fn items_mut(&mut self, folder: Option<&str>) -> Option<&mut Vec<Item>> {
        fn walk<'a>(items: &'a mut [Item], id: &str) -> Option<&'a mut Vec<Item>> {
            for it in items.iter_mut() {
                if let Item::Folder(f) = it {
                    if f.id == id {
                        return Some(&mut f.items);
                    }
                    if let Some(v) = walk(&mut f.items, id) {
                        return Some(v);
                    }
                }
            }
            None
        }
        match folder {
            None => Some(&mut self.items),
            Some(id) => walk(&mut self.items, id),
        }
    }

    /// Remove an item (request or folder) anywhere in the tree.
    pub fn remove(&mut self, id: &str) -> Option<Item> {
        fn walk(items: &mut Vec<Item>, id: &str) -> Option<Item> {
            if let Some(i) = items.iter().position(|it| it.id() == id) {
                return Some(items.remove(i));
            }
            for it in items.iter_mut() {
                if let Item::Folder(f) = it
                    && let Some(x) = walk(&mut f.items, id)
                {
                    return Some(x);
                }
            }
            None
        }
        walk(&mut self.items, id)
    }

    /// How many requests the tree holds (no allocation).
    pub fn request_count(&self) -> usize {
        fn walk(items: &[Item]) -> usize {
            items
                .iter()
                .map(|it| match it {
                    Item::Request(_) => 1,
                    Item::Folder(f) => walk(&f.items),
                })
                .sum()
        }
        walk(&self.items)
    }

    /// Every request in tree order with its folder path, for quick open.
    pub fn requests(&self) -> Vec<(Vec<String>, &Request)> {
        fn walk<'a>(
            items: &'a [Item],
            path: &mut Vec<String>,
            out: &mut Vec<(Vec<String>, &'a Request)>,
        ) {
            for it in items {
                match it {
                    Item::Request(r) => out.push((path.clone(), r)),
                    Item::Folder(f) => {
                        path.push(f.name.clone());
                        walk(&f.items, path, out);
                        path.pop();
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.items, &mut Vec::new(), &mut out);
        out
    }
}

/// `globals.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Globals {
    pub schema: u32,
    pub values: Vec<Kv>,
}

impl Globals {
    pub fn load(path: &Path) -> Self {
        if path.exists() {
            hs_core::storage::read_json(path).unwrap_or_default()
        } else {
            Self::default()
        }
    }
    pub fn save(&self, path: &Path) -> hs_core::storage::Result<()> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut g = self.clone();
        g.schema = SCHEMA;
        hs_core::storage::write_json(path, &g)
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|v| v.enabled && v.key == key)
            .map(|v| v.value.as_str())
    }
    pub fn set(&mut self, key: &str, value: &str) {
        match self.values.iter_mut().find(|v| v.key == key) {
            Some(v) => {
                v.value = value.to_string();
                v.enabled = true;
            }
            None => self.values.push(Kv::new(key, value)),
        }
    }
    pub fn unset(&mut self, key: &str) {
        self.values.retain(|v| v.key != key);
    }
}

/// One line of `history.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HistoryEntry {
    pub at: String,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub millis: u64,
    pub request: Request,
}

impl Default for HistoryEntry {
    fn default() -> Self {
        Self {
            at: String::new(),
            method: "GET".into(),
            url: String::new(),
            status: 0,
            millis: 0,
            request: Request::default(),
        }
    }
}

pub fn load_history(path: &Path, max: usize) -> Vec<HistoryEntry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out: Vec<HistoryEntry> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if out.len() > max {
        out.drain(..out.len() - max);
    }
    out
}

pub fn append_history(path: &Path, entry: &HistoryEntry, max: usize) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut entries = load_history(path, max.saturating_sub(1));
    entries.push(entry.clone());
    let text: String = entries
        .iter()
        .filter_map(|e| serde_json::to_string(e).ok())
        .map(|l| l + "\n")
        .collect();
    let _ = hs_core::storage::write_atomic(path, text.as_bytes());
}

/// The name a copy takes: the original with a space and a sequence number
/// on the end, counting up past whatever is already there. "List" becomes
/// "List 001", and copying "List 001" gives "List 002" rather than
/// "List 001 001". Used both when a request is duplicated and when one is
/// dragged somewhere a request of that name already lives. Owner's rule,
/// 2026-10-17.
pub fn next_copy_name(name: &str, taken: &[String]) -> String {
    let stem = stem_of(name);
    for n in 1..=999 {
        let candidate = format!("{stem} {n:03}");
        if !taken.iter().any(|t| t == &candidate) {
            return candidate;
        }
    }
    // Past 999 the sequence is no help; fall back to something unique.
    format!("{stem} {}", crate::model::short_id())
}

/// The name with any sequence number this function added taken back off.
/// Only " " plus three or more digits counts, so "HTTP 2" and "Step 42"
/// keep their numbers and get a sequence of their own.
fn stem_of(name: &str) -> &str {
    let digits = name.chars().rev().take_while(char::is_ascii_digit).count();
    if digits < 3 {
        return name;
    }
    let head = &name[..name.len() - digits];
    match head.strip_suffix(' ') {
        Some(stem) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// The name to give `name` in a list that already holds `taken`: itself
/// when it is free, else the next numbered form.
pub fn free_name(name: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == name) {
        return name.to_string();
    }
    next_copy_name(name, taken)
}

/// A short unique tail, for the case a sequence cannot be found.
fn short_id() -> String {
    hs_core::new_id().chars().rev().take(6).collect()
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_copy_counts_up_from_the_original_name() {
        assert_eq!(next_copy_name("List", &[]), "List 001");
        assert_eq!(
            next_copy_name("List", &["List 001".to_string()]),
            "List 002"
        );
        // Copying a copy continues the sequence rather than nesting.
        assert_eq!(
            next_copy_name("List 001", &["List 001".to_string()]),
            "List 002"
        );
        assert_eq!(
            next_copy_name("List", &["List 001".to_string(), "List 002".to_string()]),
            "List 003"
        );
    }

    #[test]
    fn a_number_that_is_part_of_the_name_is_kept() {
        // A sequence is a space and three or more digits; anything else
        // belongs to the name and gets a sequence of its own.
        assert_eq!(next_copy_name("HTTP2", &[]), "HTTP2 001");
        assert_eq!(next_copy_name("v10", &[]), "v10 001");
        assert_eq!(next_copy_name("Step 42", &[]), "Step 42 001");
        assert_eq!(next_copy_name("Report2024", &[]), "Report2024 001");
        assert_eq!(next_copy_name("Step 007", &[]), "Step 001");
    }

    #[test]
    fn a_name_nothing_else_uses_is_left_alone() {
        assert_eq!(free_name("List", &["Create".to_string()]), "List");
        assert_eq!(free_name("List", &["List".to_string()]), "List 001");
        assert_eq!(
            free_name("List", &["List".to_string(), "List 001".to_string()]),
            "List 002"
        );
    }

    use super::*;

    #[test]
    fn collection_tree_find_remove_and_round_trip() {
        let mut c = Collection {
            name: "shop".into(),
            ..Default::default()
        };
        let mut f = Folder {
            name: "orders".into(),
            ..Default::default()
        };
        let r = Request {
            name: "create".into(),
            method: "POST".into(),
            ..Default::default()
        };
        let rid = r.id.clone();
        let fid = f.id.clone();
        f.items.push(Item::Request(r));
        c.items.push(Item::Folder(f));
        c.items.push(Item::Request(Request::default()));
        let (found, folders) = c.find(&rid).unwrap();
        assert_eq!(found.name, "create");
        assert_eq!(folders[0].id, fid);
        assert_eq!(c.requests().len(), 2);
        assert_eq!(c.requests()[0].0, vec!["orders".to_string()]);
        c.find_mut(&rid).unwrap().url = "https://x".into();
        let tmp = tempfile::tempdir().unwrap();
        c.save(tmp.path()).unwrap();
        let back = Collection::load_all(tmp.path());
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].find(&rid).unwrap().0.url, "https://x");
        assert!(c.remove(&fid).is_some());
        assert!(c.find(&rid).is_none());
    }

    #[test]
    fn globals_and_history() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("globals.json");
        let mut g = Globals::default();
        g.set("token", "abc");
        g.set("token", "def");
        g.save(&p).unwrap();
        assert_eq!(Globals::load(&p).get("token"), Some("def"));
        g.unset("token");
        assert!(g.get("token").is_none());
        let h = tmp.path().join("history.jsonl");
        for i in 0..5 {
            append_history(
                &h,
                &HistoryEntry {
                    url: format!("u{i}"),
                    ..Default::default()
                },
                3,
            );
        }
        let back = load_history(&h, 10);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].url, "u2");
    }
}
