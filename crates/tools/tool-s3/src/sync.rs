//! Sync between a local folder and a prefix (docs/tools/s3.md §8): the
//! comparison is worked out here from two listings, so the dry run the
//! user confirms and the work the Job then does come from the same
//! function and cannot disagree.

use std::path::{Path, PathBuf};

use hs_core::objstore::s3::Object;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Local folder → prefix.
    Up,
    /// Prefix → local folder.
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Not on the other side.
    Added,
    /// There, but a different size.
    Changed,
    /// Only on the far side; removed when "delete extra" is on.
    Extra,
}

/// A side of the comparison: relative key and size.
type Side = Vec<(String, u64)>;

/// One difference, named by the key relative to the prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub rel: String,
    pub action: Action,
    /// Bytes to move (0 for a deletion).
    pub size: u64,
}

/// A local file found under the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFile {
    /// Path relative to the folder, with `/` separators.
    pub rel: String,
    pub size: u64,
    pub path: PathBuf,
}

/// Walk `root` into relative files, skipping anything unreadable. Hidden
/// files are included: a `.env` left out of a sync is a surprise.
pub fn walk_local(root: &Path) -> Vec<LocalFile> {
    fn go(root: &Path, dir: &Path, out: &mut Vec<LocalFile>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => go(root, &p, out),
                Ok(t) if t.is_file() => {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    if let Ok(rel) = p.strip_prefix(root) {
                        out.push(LocalFile {
                            rel: rel.to_string_lossy().replace('\\', "/"),
                            size,
                            path: p.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    go(root, root, &mut out);
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// The key of a remote object relative to `prefix`.
pub fn relative(key: &str, prefix: &str) -> Option<String> {
    let rest = key.strip_prefix(prefix)?;
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    (!rest.is_empty() && !rest.ends_with('/')).then(|| rest.to_string())
}

/// What the sync would do. Compared by size only: S3's ETag is an MD5
/// for a single-part upload and something else for a multipart one, and
/// a local mtime and a remote LastModified are not the same clock, so a
/// size comparison is the one answer that is never wrong in a way the
/// user cannot see (docs/tools/s3.md §8; checksum compare is not done).
pub fn plan(
    local: &[LocalFile],
    remote: &[Object],
    prefix: &str,
    direction: Direction,
    delete_extra: bool,
) -> Vec<Diff> {
    let remote: Side = remote
        .iter()
        .filter_map(|o| relative(&o.key, prefix).map(|r| (r, o.size)))
        .collect();
    let find_remote = |rel: &str| remote.iter().find(|(r, _)| r == rel).map(|(_, s)| *s);
    let find_local = |rel: &str| local.iter().find(|f| f.rel == rel).map(|f| f.size);
    let mut out = Vec::new();
    let (from, to): (Side, Side) = match direction {
        Direction::Up => (
            local.iter().map(|f| (f.rel.clone(), f.size)).collect(),
            remote.clone(),
        ),
        Direction::Down => (
            remote.clone(),
            local.iter().map(|f| (f.rel.clone(), f.size)).collect(),
        ),
    };
    for (rel, size) in &from {
        let other = match direction {
            Direction::Up => find_remote(rel),
            Direction::Down => find_local(rel),
        };
        match other {
            None => out.push(Diff {
                rel: rel.clone(),
                action: Action::Added,
                size: *size,
            }),
            Some(s) if s != *size => out.push(Diff {
                rel: rel.clone(),
                action: Action::Changed,
                size: *size,
            }),
            Some(_) => {}
        }
    }
    if delete_extra {
        for (rel, _) in &to {
            let here = match direction {
                Direction::Up => find_local(rel),
                Direction::Down => find_remote(rel),
            };
            if here.is_none() {
                out.push(Diff {
                    rel: rel.clone(),
                    action: Action::Extra,
                    size: 0,
                });
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// The key a relative path becomes under `prefix`.
pub fn key_for(prefix: &str, rel: &str) -> String {
    if prefix.is_empty() {
        rel.to_string()
    } else {
        format!("{}/{rel}", prefix.trim_end_matches('/'))
    }
}

/// Bytes the plan would move.
pub fn total_bytes(diffs: &[Diff]) -> u64 {
    diffs.iter().map(|d| d.size).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(key: &str, size: u64) -> Object {
        Object {
            key: key.into(),
            size,
            modified: String::new(),
            etag: String::new(),
            storage_class: String::new(),
        }
    }

    fn local(rel: &str, size: u64) -> LocalFile {
        LocalFile {
            rel: rel.into(),
            size,
            path: PathBuf::from(rel),
        }
    }

    #[test]
    fn keys_become_relative_to_the_prefix() {
        assert_eq!(relative("logs/a.txt", "logs").as_deref(), Some("a.txt"));
        assert_eq!(
            relative("logs/x/a.txt", "logs/").as_deref(),
            Some("x/a.txt")
        );
        assert_eq!(relative("logs/", "logs"), None, "the folder marker itself");
        assert_eq!(relative("other/a.txt", "logs"), None);
        assert_eq!(key_for("logs", "x/a.txt"), "logs/x/a.txt");
        assert_eq!(key_for("", "a.txt"), "a.txt");
    }

    #[test]
    fn an_upload_plan_adds_changes_and_optionally_removes() {
        let l = [local("a.txt", 10), local("b/c.txt", 20), local("same", 5)];
        let r = [
            obj("p/a.txt", 9),
            obj("p/same", 5),
            obj("p/gone.txt", 7),
            obj("other/x", 1),
        ];
        let plan_keep = plan(&l, &r, "p", Direction::Up, false);
        assert_eq!(
            plan_keep,
            vec![
                Diff {
                    rel: "a.txt".into(),
                    action: Action::Changed,
                    size: 10
                },
                Diff {
                    rel: "b/c.txt".into(),
                    action: Action::Added,
                    size: 20
                },
            ]
        );
        assert_eq!(total_bytes(&plan_keep), 30);
        let plan_del = plan(&l, &r, "p", Direction::Up, true);
        assert!(
            plan_del
                .iter()
                .any(|d| d.rel == "gone.txt" && d.action == Action::Extra)
        );
        assert_eq!(plan_del.len(), 3);
    }

    #[test]
    fn a_download_plan_is_the_mirror_image() {
        let l = [local("keep", 1), local("extra", 2)];
        let r = [obj("p/keep", 1), obj("p/new", 3)];
        let p = plan(&l, &r, "p", Direction::Down, true);
        assert_eq!(
            p,
            vec![
                Diff {
                    rel: "extra".into(),
                    action: Action::Extra,
                    size: 0
                },
                Diff {
                    rel: "new".into(),
                    action: Action::Added,
                    size: 3
                },
            ]
        );
        assert!(
            plan(&l, &r, "p", Direction::Down, false)
                .iter()
                .all(|d| d.action != Action::Extra)
        );
    }

    #[test]
    fn walking_a_folder_finds_every_file_with_forward_slashes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub/deep")).unwrap();
        std::fs::write(dir.path().join("top.txt"), b"abc").unwrap();
        std::fs::write(dir.path().join("sub/deep/x.bin"), b"12345").unwrap();
        std::fs::write(dir.path().join(".env"), b"K=V").unwrap();
        let files = walk_local(dir.path());
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec![".env", "sub/deep/x.bin", "top.txt"]);
        assert_eq!(files[1].size, 5);
        assert!(walk_local(&dir.path().join("missing")).is_empty());
    }
}
