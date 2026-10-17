//! Favourites and saved passwords, one set per workspace.
//!
//! Both live beside the workspace they belong to (`<workspace>/browser/`),
//! because that is what the owner asked for: a workspace is a project, and
//! the sites and logins that go with one project are not the ones that go
//! with another.
//!
//! Passwords keep only what is safe to read in the file — a name, a URL, an
//! icon, a username. **Everything secret is a `secret:<ulid>` reference into
//! the encrypted vault** (`docs/06-security.md`), so the workspace file can
//! be copied, committed or synced without carrying a password with it, and
//! revealing or filling one needs the vault unlocked.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where both files live for a workspace root.
pub fn dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("browser")
}

/// One saved site, or a folder of them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FavItem {
    Link(Favorite),
    Folder(FavFolder),
}

impl FavItem {
    pub fn id(&self) -> &str {
        match self {
            FavItem::Link(l) => &l.id,
            FavItem::Folder(f) => &f.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            FavItem::Link(l) => &l.name,
            FavItem::Folder(f) => &f.name,
        }
    }

    pub fn set_name(&mut self, name: &str) {
        match self {
            FavItem::Link(l) => l.name = name.to_string(),
            FavItem::Folder(f) => f.name = name.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Favorite {
    pub id: String,
    pub name: String,
    pub url: String,
    /// A custom icon the user chose, as a data URI. Empty means the site's
    /// own favicon is used, which is what happens for almost every entry.
    pub icon: String,
}

impl Default for Favorite {
    fn default() -> Self {
        Self {
            id: hs_core::new_id(),
            name: String::new(),
            url: String::new(),
            icon: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FavFolder {
    pub id: String,
    pub name: String,
    pub items: Vec<FavItem>,
}

impl Default for FavFolder {
    fn default() -> Self {
        Self {
            id: hs_core::new_id(),
            name: String::new(),
            items: Vec::new(),
        }
    }
}

/// A workspace's favourites.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Favorites {
    pub items: Vec<FavItem>,
}

impl Favorites {
    fn file(root: &Path) -> PathBuf {
        dir(root).join("favorites.json")
    }

    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(Self::file(root))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let f = Self::file(root);
        if let Some(parent) = f.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(f, text)
    }

    /// The items of `folder` (`None` = the root).
    pub fn items(&self, folder: Option<&str>) -> Option<&Vec<FavItem>> {
        fn walk<'a>(items: &'a [FavItem], id: &str) -> Option<&'a Vec<FavItem>> {
            for it in items {
                if let FavItem::Folder(f) = it {
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

    pub fn items_mut(&mut self, folder: Option<&str>) -> Option<&mut Vec<FavItem>> {
        fn walk<'a>(items: &'a mut [FavItem], id: &str) -> Option<&'a mut Vec<FavItem>> {
            for it in items.iter_mut() {
                if let FavItem::Folder(f) = it {
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

    /// Every folder as (id, display path), for a "where should this go?"
    /// picker. The root is not included; the caller offers it itself.
    pub fn folders(&self) -> Vec<(String, String)> {
        fn walk(items: &[FavItem], prefix: &str, out: &mut Vec<(String, String)>) {
            for it in items {
                if let FavItem::Folder(f) = it {
                    let path = if prefix.is_empty() {
                        f.name.clone()
                    } else {
                        format!("{prefix} / {}", f.name)
                    };
                    out.push((f.id.clone(), path.clone()));
                    walk(&f.items, &path, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.items, "", &mut out);
        out
    }

    /// Find a link anywhere in the tree.
    pub fn link(&self, id: &str) -> Option<&Favorite> {
        fn walk<'a>(items: &'a [FavItem], id: &str) -> Option<&'a Favorite> {
            for it in items {
                match it {
                    FavItem::Link(l) if l.id == id => return Some(l),
                    FavItem::Folder(f) => {
                        if let Some(l) = walk(&f.items, id) {
                            return Some(l);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        walk(&self.items, id)
    }

    pub fn link_mut(&mut self, id: &str) -> Option<&mut Favorite> {
        fn walk<'a>(items: &'a mut [FavItem], id: &str) -> Option<&'a mut Favorite> {
            for it in items.iter_mut() {
                match it {
                    FavItem::Link(l) if l.id == id => return Some(l),
                    FavItem::Folder(f) => {
                        if let Some(l) = walk(&mut f.items, id) {
                            return Some(l);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        walk(&mut self.items, id)
    }

    /// Which folder holds `id` (`None` = the root, or not found).
    pub fn parent_of(&self, id: &str) -> Option<String> {
        fn walk(items: &[FavItem], id: &str, here: Option<&str>) -> Option<Option<String>> {
            for it in items {
                if it.id() == id {
                    return Some(here.map(str::to_string));
                }
                if let FavItem::Folder(f) = it
                    && let Some(found) = walk(&f.items, id, Some(&f.id))
                {
                    return Some(found);
                }
            }
            None
        }
        walk(&self.items, id, None).flatten()
    }

    /// Take an item out of the tree, wherever it is.
    pub fn remove(&mut self, id: &str) -> Option<FavItem> {
        fn walk(items: &mut Vec<FavItem>, id: &str) -> Option<FavItem> {
            if let Some(i) = items.iter().position(|x| x.id() == id) {
                return Some(items.remove(i));
            }
            for it in items.iter_mut() {
                if let FavItem::Folder(f) = it
                    && let Some(found) = walk(&mut f.items, id)
                {
                    return Some(found);
                }
            }
            None
        }
        walk(&mut self.items, id)
    }

    /// Whether `folder` is `id` or sits inside it, so a folder is never
    /// dropped into its own branch and taken out of the tree with it.
    pub fn is_within(&self, folder: &str, id: &str) -> bool {
        if folder == id {
            return true;
        }
        let mut at = Some(folder.to_string());
        while let Some(f) = at {
            if f == id {
                return true;
            }
            at = self.parent_of(&f);
        }
        false
    }
}

/// One saved login. Everything secret is a vault reference, never a value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Login {
    pub id: String,
    pub name: String,
    pub url: String,
    /// A custom icon as a data URI; empty means the site's own favicon.
    pub icon: String,
    /// Readable: a username is not a secret, and seeing which account a
    /// row is for is the point of the list.
    pub username: String,
    /// `secret:<ulid>` into the vault, or empty.
    pub password: String,
    /// `secret:<ulid>` into the vault, or empty.
    pub passkey: String,
    /// `secret:<ulid>` into the vault, or empty.
    pub api_key: String,
}

impl Default for Login {
    fn default() -> Self {
        Self {
            id: hs_core::new_id(),
            name: String::new(),
            url: String::new(),
            icon: String::new(),
            username: String::new(),
            password: String::new(),
            passkey: String::new(),
            api_key: String::new(),
        }
    }
}

/// A workspace's saved logins.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Passwords {
    pub items: Vec<Login>,
}

impl Passwords {
    fn file(root: &Path) -> PathBuf {
        dir(root).join("passwords.json")
    }

    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(Self::file(root))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let f = Self::file(root);
        if let Some(parent) = f.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(f, text)
    }

    /// The logins saved for a page's host, best match first. Matching is on
    /// the host alone: a login for a site is a login for its pages, and the
    /// path a form happens to live at is not part of the identity.
    pub fn for_host(&self, host: &str) -> Vec<&Login> {
        let host = host.trim().to_lowercase();
        if host.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<&Login> = self
            .items
            .iter()
            .filter(|l| {
                host_of(&l.url).is_some_and(|h| h == host || host.ends_with(&format!(".{h}")))
            })
            .collect();
        // An exact host before a parent domain's entry.
        out.sort_by_key(|l| host_of(&l.url).is_none_or(|h| h != host));
        out
    }
}

/// The host part of a URL, lower-cased, without a leading `www.`.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url)
        .trim_start_matches('/');
    let host = rest
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .trim()
        .to_lowercase();
    if host.is_empty() {
        return None;
    }
    Some(host.strip_prefix("www.").unwrap_or(&host).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, items: Vec<FavItem>) -> FavItem {
        FavItem::Folder(FavFolder {
            name: name.into(),
            items,
            ..Default::default()
        })
    }

    fn link(name: &str, url: &str) -> FavItem {
        FavItem::Link(Favorite {
            name: name.into(),
            url: url.into(),
            ..Default::default()
        })
    }

    #[test]
    fn a_host_is_read_without_scheme_port_or_www() {
        assert_eq!(
            host_of("https://www.example.com/a/b?c"),
            Some("example.com".into())
        );
        assert_eq!(host_of("example.com:8443"), Some("example.com".into()));
        assert_eq!(
            host_of("http://user@git.zacx.net/x"),
            Some("git.zacx.net".into())
        );
        assert_eq!(host_of(""), None);
    }

    #[test]
    fn a_login_matches_its_site_and_that_sites_subdomains() {
        let p = Passwords {
            items: vec![
                Login {
                    name: "Forge".into(),
                    url: "https://git.zacx.net".into(),
                    ..Default::default()
                },
                Login {
                    name: "Elsewhere".into(),
                    url: "https://example.com".into(),
                    ..Default::default()
                },
            ],
        };
        let hit: Vec<&str> = p
            .for_host("git.zacx.net")
            .iter()
            .map(|l| l.name.as_str())
            .collect();
        assert_eq!(hit, vec!["Forge"]);
        assert!(p.for_host("unrelated.test").is_empty());
    }

    #[test]
    fn an_item_is_found_and_taken_out_of_a_nested_folder() {
        let inner = link("Deep", "https://deep.test");
        let inner_id = inner.id().to_string();
        let mut f = Favorites {
            items: vec![folder("Outer", vec![folder("Inner", vec![inner])])],
        };
        assert!(f.link(&inner_id).is_some());
        let parent = f.parent_of(&inner_id).expect("it is in a folder");
        assert_eq!(f.items(Some(&parent)).map(Vec::len), Some(1));
        assert!(f.remove(&inner_id).is_some());
        assert!(f.link(&inner_id).is_none());
    }

    #[test]
    fn a_folder_is_never_dropped_inside_its_own_branch() {
        let inner = folder("Inner", vec![]);
        let inner_id = inner.id().to_string();
        let outer = folder("Outer", vec![inner]);
        let outer_id = outer.id().to_string();
        let f = Favorites { items: vec![outer] };
        assert!(f.is_within(&inner_id, &outer_id), "inner is inside outer");
        assert!(
            !f.is_within(&outer_id, &inner_id),
            "outer is not inside inner"
        );
    }

    #[test]
    fn folders_are_listed_with_the_path_that_reaches_them() {
        let f = Favorites {
            items: vec![folder("Work", vec![folder("Infra", vec![])])],
        };
        let names: Vec<String> = f.folders().into_iter().map(|(_, p)| p).collect();
        assert_eq!(names, vec!["Work".to_string(), "Work / Infra".to_string()]);
    }
}
