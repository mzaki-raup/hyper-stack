//! The workspace's **repository list**: the folders the Git activity
//! offers, in the order the user put them in (docs/tools/git-activity.md
//! §2.1).
//!
//! Like [`crate::projects`], this is a list of *references*, never an
//! owner: an entry points at a folder that exists on disk for its own
//! reasons. [`GitRepos::remove`] drops the entry and nothing else - the
//! folder, its files and its `.git` are left exactly as they were
//! (owner's requirement, 2026-10-29). Nothing in this module deletes a
//! directory, and nothing in it should ever learn how.
//!
//! One `git-repos.toml` holds the whole list: the order is the point.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage;

pub const SCHEMA: u32 = 1;

/// Where a repository came from. It decides which fields the form asks
/// for and whether Save has to clone before it records.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// A folder on this machine that already is a repository.
    #[default]
    Local,
    /// A bare repository or `file://` path on this machine, cloned into
    /// `path`.
    Localhost,
    /// A project on a host (GitHub, GitLab, Forgejo, …), cloned into
    /// `path` with a saved credential.
    Remote,
}

impl Source {
    pub fn all() -> [Source; 3] {
        [Source::Local, Source::Localhost, Source::Remote]
    }
    pub fn label_key(self) -> &'static str {
        match self {
            Source::Local => "git-source-local",
            Source::Localhost => "git-source-localhost",
            Source::Remote => "git-source-remote",
        }
    }
}

/// One saved repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RepoEntry {
    pub id: String,
    /// What the list shows. Seeded from the folder's own name (or the
    /// host project's), then the user's to change.
    pub name: String,
    /// A glyph key from the icon picker; empty draws the language icon,
    /// or the branch glyph when the language is unknown.
    pub icon: String,
    pub source: Source,
    /// The host the project was fetched from (`https://github.com`),
    /// when it came from one.
    pub host_url: String,
    /// The host's project path (`owner/name`), when it came from a host.
    pub project: String,
    /// The clone URL. Empty for a local folder, where `origin` answers.
    pub url: String,
    /// The folder on disk.
    pub path: PathBuf,
    /// The id of a global Git credential (`crate::git_hosts`), or empty.
    pub credential: String,
    /// Where this entry sits in the list. Written by dragging a row.
    pub order: i32,
    /// The detected language id, cached so the list draws without
    /// touching the disk; `detect_language` refreshes it.
    pub language: String,
}

impl Default for RepoEntry {
    fn default() -> Self {
        Self {
            id: crate::new_id(),
            name: String::new(),
            icon: String::new(),
            source: Source::Local,
            host_url: String::new(),
            project: String::new(),
            url: String::new(),
            path: PathBuf::new(),
            credential: String::new(),
            order: 0,
            language: String::new(),
        }
    }
}

impl RepoEntry {
    /// A new local entry for `path`, named after the folder and with its
    /// language detected.
    pub fn for_path(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let name = folder_name(&path);
        let language = crate::projects::detect_language(&path).to_string();
        Self {
            name,
            path,
            language,
            ..Self::default()
        }
    }

    /// The name to show, falling back to the folder so a row is never
    /// nameless.
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() {
            folder_name(&self.path)
        } else {
            self.name.clone()
        }
    }

    /// Whether the folder holds a repository right now (a `.git` entry,
    /// file or directory - a linked worktree has the file).
    pub fn is_cloned(&self) -> bool {
        !self.path.as_os_str().is_empty() && self.path.join(".git").exists()
    }

    /// What is missing before this entry can be saved, as locale keys;
    /// empty means Save may go ahead (docs/tools/git-activity.md §3).
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.name.trim().is_empty() {
            out.push("git-missing-name");
        }
        if self.path.as_os_str().is_empty() {
            out.push("git-missing-path");
        }
        if self.source != Source::Local && self.url.trim().is_empty() {
            out.push("git-missing-url");
        }
        out
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// The file on disk: a schema and an array of entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct File {
    schema: u32,
    repo: Vec<RepoEntry>,
}

impl Default for File {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            repo: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct GitRepos {
    path: PathBuf,
    items: Vec<RepoEntry>,
}

impl GitRepos {
    /// Read `<workspace>/git-repos.toml`. A missing or unreadable file is
    /// an empty list, not an error.
    pub fn load(path: &Path) -> Self {
        let mut items = match storage::read_toml::<File>(path) {
            Ok(f) if f.schema <= SCHEMA => f.repo,
            Ok(f) => {
                tracing::warn!(
                    "git-repos.toml is schema {} and this build knows {SCHEMA}; ignoring it",
                    f.schema
                );
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        for r in &mut items {
            if r.language.is_empty() && r.is_cloned() {
                r.language = crate::projects::detect_language(&r.path).to_string();
            }
        }
        items.sort_by_key(|r| (r.order, r.name.to_lowercase()));
        Self {
            path: path.to_path_buf(),
            items,
        }
    }

    pub fn all(&self) -> &[RepoEntry] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn by_id(&self, id: &str) -> Option<&RepoEntry> {
        self.items.iter().find(|r| r.id == id)
    }

    /// The entry whose folder this is, compared on the normalised path.
    pub fn by_path(&self, path: &Path) -> Option<&RepoEntry> {
        let want = crate::projects::normalise(path);
        self.items
            .iter()
            .find(|r| crate::projects::normalise(&r.path) == want)
    }

    /// Add or replace an entry, then write the list.
    pub fn upsert(&mut self, mut entry: RepoEntry) -> storage::Result<()> {
        if entry.order == 0 {
            entry.order = self.items.iter().map(|r| r.order).max().unwrap_or(0) + 1;
        }
        self.items.retain(|r| r.id != entry.id);
        self.items.push(entry);
        self.items.sort_by_key(|r| (r.order, r.name.to_lowercase()));
        self.save()
    }

    /// Drop an entry from the list.
    ///
    /// **This never touches the folder it points at.**
    pub fn remove(&mut self, id: &str) -> storage::Result<()> {
        self.items.retain(|r| r.id != id);
        self.save()
    }

    /// Move row `from` to boundary `to`, renumber, and write.
    pub fn reorder(&mut self, from: usize, to: usize) -> storage::Result<()> {
        if from >= self.items.len() || from == to {
            return Ok(());
        }
        let entry = self.items.remove(from);
        let to = to.min(self.items.len() + 1);
        let to = if to > from { to - 1 } else { to };
        self.items.insert(to.min(self.items.len()), entry);
        for (i, r) in self.items.iter_mut().enumerate() {
            r.order = i as i32 + 1;
        }
        self.save()
    }

    fn save(&self) -> storage::Result<()> {
        storage::write_toml(
            &self.path,
            &File {
                schema: SCHEMA,
                repo: self.items.clone(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_round_trips_and_keeps_its_order() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("git-repos.toml");
        let mut repos = GitRepos::load(&file);
        assert!(repos.is_empty());
        let a = RepoEntry {
            name: "alpha".into(),
            path: dir.path().join("alpha"),
            ..Default::default()
        };
        let b = RepoEntry {
            name: "beta".into(),
            path: dir.path().join("beta"),
            source: Source::Remote,
            url: "https://example.test/o/beta.git".into(),
            ..Default::default()
        };
        repos.upsert(a.clone()).unwrap();
        repos.upsert(b.clone()).unwrap();
        repos.reorder(1, 0).unwrap();
        let again = GitRepos::load(&file);
        let names: Vec<_> = again.all().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["beta", "alpha"]);
        assert_eq!(again.by_id(&b.id).unwrap().source, Source::Remote);
        assert!(again.by_path(&dir.path().join("ALPHA")).is_some() || cfg!(not(windows)));
    }

    /// Removing forgets the entry and leaves the folder alone - the one
    /// promise this module exists to keep.
    #[test]
    fn remove_never_touches_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("keep-me");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join("README.md"), "hello").unwrap();
        let file = dir.path().join("git-repos.toml");
        let mut repos = GitRepos::load(&file);
        let e = RepoEntry::for_path(&repo);
        assert!(e.is_cloned());
        repos.upsert(e.clone()).unwrap();
        repos.remove(&e.id).unwrap();
        assert!(repos.is_empty());
        assert!(repo.join("README.md").is_file());
        assert!(repo.join(".git").is_dir());
    }

    #[test]
    fn save_knows_what_is_missing() {
        let mut e = RepoEntry::default();
        assert_eq!(e.missing(), ["git-missing-name", "git-missing-path"]);
        e.name = "x".into();
        e.path = PathBuf::from("x");
        assert!(e.missing().is_empty());
        e.source = Source::Remote;
        assert_eq!(e.missing(), ["git-missing-url"]);
    }
}
