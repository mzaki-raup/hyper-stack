//! The workspace's **project list**: the folders the Editor activity
//! offers, in the order the user put them in.
//!
//! This is a list of *references*, never a copy and never an owner: an
//! entry points at a folder that exists on disk for its own reasons.
//! [`Projects::remove`] therefore drops the entry and nothing else - the
//! folder, its files and its repository are left exactly as they were
//! (owner's requirement, 2026-10-26). Nothing in this module deletes a
//! directory, and nothing in it should ever learn how.
//!
//! One `projects.toml` holds the whole list rather than a file per entry:
//! the order is the point, and a single array keeps it in one place. (A
//! file per profile is right for connections, whose names make the file
//! names - and whose file names, when hand-written, have bitten us.)

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage;

pub const SCHEMA: u32 = 1;

/// One saved project: a name, the folder it refers to, and what we have
/// worked out about it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProjectEntry {
    pub id: String,
    /// What the list shows. Seeded from the folder's own name, then the
    /// user's to change.
    pub name: String,
    pub root: PathBuf,
    /// The detected language id (`rust`, `java`, …) - cached so a list of
    /// twenty projects draws without touching the disk twenty times.
    /// Empty until a detection has run; `detect_language` refreshes it.
    pub language: String,
    /// Where this entry sits in the list. Written by dragging a row.
    pub order: i32,
    /// The identity commits from this project should carry. Applied to
    /// the repository's own config when saved, so it never leaks between
    /// projects the way a global `user.email` does.
    pub git_name: String,
    pub git_email: String,
}

impl Default for ProjectEntry {
    fn default() -> Self {
        Self {
            id: crate::new_id(),
            name: String::new(),
            root: PathBuf::new(),
            language: String::new(),
            order: 0,
            git_name: String::new(),
            git_email: String::new(),
        }
    }
}

impl ProjectEntry {
    /// A new entry for `root`, named after the folder and with its
    /// language detected.
    pub fn for_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string());
        let language = detect_language(&root).to_string();
        Self {
            name,
            root,
            language,
            ..Self::default()
        }
    }

    /// The name to show, falling back to the folder when it is blank so a
    /// row is never nameless.
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() {
            self.root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| self.root.to_string_lossy().to_string())
        } else {
            self.name.clone()
        }
    }
}

/// The file on disk: a schema and an array of entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct File {
    schema: u32,
    project: Vec<ProjectEntry>,
}

impl Default for File {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            project: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Projects {
    path: PathBuf,
    items: Vec<ProjectEntry>,
}

impl Projects {
    /// Read `<workspace>/projects.toml`. A missing or unreadable file is
    /// an empty list, not an error: a workspace that has never had a
    /// project is the ordinary case.
    pub fn load(path: &Path) -> Self {
        let mut items = match storage::read_toml::<File>(path) {
            Ok(f) if f.schema <= SCHEMA => f.project,
            Ok(f) => {
                tracing::warn!(
                    "projects.toml is schema {} and this build knows {SCHEMA}; ignoring it",
                    f.schema
                );
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        // An entry with no cached language gets one now: a list written
        // by an older build, or by hand, would otherwise draw plain
        // folders for ever. Only the blanks are detected, so this is a
        // few `stat`s on the first open and nothing afterwards; Save in
        // Settings re-detects a project whose manifest has changed.
        for p in &mut items {
            if p.language.is_empty() {
                p.language = detect_language(&p.root).to_string();
            }
        }
        items.sort_by_key(|p| (p.order, p.name.to_lowercase()));
        Self {
            path: path.to_path_buf(),
            items,
        }
    }

    pub fn all(&self) -> &[ProjectEntry] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn by_id(&self, id: &str) -> Option<&ProjectEntry> {
        self.items.iter().find(|p| p.id == id)
    }

    /// The entry whose folder this is. Compared on the normalised path, so
    /// `C:\p` and `C:\p\` are the same project.
    pub fn by_root(&self, root: &Path) -> Option<&ProjectEntry> {
        let want = normalise(root);
        self.items.iter().find(|p| normalise(&p.root) == want)
    }

    /// Add or replace an entry, then write the list.
    pub fn upsert(&mut self, mut entry: ProjectEntry) -> storage::Result<()> {
        if entry.order == 0 {
            entry.order = self.items.iter().map(|p| p.order).max().unwrap_or(0) + 1;
        }
        self.items.retain(|p| p.id != entry.id);
        self.items.push(entry);
        self.items.sort_by_key(|p| (p.order, p.name.to_lowercase()));
        self.save()
    }

    /// Drop an entry from the list.
    ///
    /// **This never touches the folder it points at.** The project list
    /// holds references; removing one is the list forgetting a folder, not
    /// the folder going away (owner's requirement, 2026-10-26).
    pub fn remove(&mut self, id: &str) -> storage::Result<()> {
        self.items.retain(|p| p.id != id);
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
        for (i, p) in self.items.iter_mut().enumerate() {
            p.order = i as i32 + 1;
        }
        self.save()
    }

    fn save(&self) -> storage::Result<()> {
        storage::write_toml(
            &self.path,
            &File {
                schema: SCHEMA,
                project: self.items.clone(),
            },
        )
    }
}

/// Compare paths without caring about a trailing separator or the
/// `\\?\` prefix `canonicalize` adds on Windows.
///
/// Public because the shell compares a tab's open file against a project
/// root with it, and two different spellings of one path are how "close
/// the tabs of this project" quietly misses half of them.
pub fn normalise(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s).to_string();
    s.trim_end_matches('/').to_lowercase()
}

/// Every language the app knows by name, as `(id, display name)`.
///
/// The ids are what [`detect_language`] returns and what
/// `hs_ui::icons::language_icon` draws; the names are what a person
/// picks from. The list is here rather than in the Editor because the
/// Settings form and the Editor both read it, and a project whose
/// language was chosen by hand must mean the same thing in both.
///
/// Ordered as a reader would look for them: the compiled languages, the
/// scripted ones, then the markup and configuration formats.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("rust", "Rust"),
    ("go", "Go"),
    ("c", "C"),
    ("cpp", "C++"),
    ("csharp", "C#"),
    ("java", "Java"),
    ("kotlin", "Kotlin"),
    ("scala", "Scala"),
    ("swift", "Swift"),
    ("zig", "Zig"),
    ("haskell", "Haskell"),
    ("elixir", "Elixir"),
    ("dart", "Dart"),
    ("python", "Python"),
    ("ruby", "Ruby"),
    ("php", "PHP"),
    ("perl", "Perl"),
    ("lua", "Lua"),
    ("javascript", "JavaScript"),
    ("typescript", "TypeScript"),
    ("node", "Node.js"),
    ("shell", "Shell"),
    ("terraform", "Terraform"),
    ("html", "HTML"),
    ("css", "CSS"),
    ("markdown", "Markdown"),
];

/// The display name for a language id, or `""` when it is not one of
/// [`LANGUAGES`] - an empty answer is better than inventing a name.
pub fn language_name(id: &str) -> &'static str {
    LANGUAGES
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, n)| *n)
        .unwrap_or("")
}

/// What language a folder is, from the manifest it carries.
///
/// The order matters where a repository has more than one: a Rust
/// workspace with a `package.json` for its web assets is a Rust project,
/// so the build manifests are tested before the looser signals. Returns
/// `""` when nothing is recognised, which the list draws as a plain
/// folder rather than guessing.
pub fn detect_language(root: &Path) -> &'static str {
    const MANIFESTS: &[(&str, &str)] = &[
        ("Cargo.toml", "rust"),
        ("go.mod", "go"),
        ("pom.xml", "java"),
        ("build.gradle", "java"),
        ("build.gradle.kts", "kotlin"),
        ("pubspec.yaml", "dart"),
        ("mix.exs", "elixir"),
        ("composer.json", "php"),
        ("Gemfile", "ruby"),
        ("pyproject.toml", "python"),
        ("requirements.txt", "python"),
        ("setup.py", "python"),
        ("build.zig", "zig"),
        ("stack.yaml", "haskell"),
        ("cabal.project", "haskell"),
        ("build.sbt", "scala"),
        ("CMakeLists.txt", "cpp"),
    ];
    for (file, lang) in MANIFESTS {
        if root.join(file).is_file() {
            return lang;
        }
    }
    // A `package.json` is JavaScript unless a TypeScript config sits
    // beside it, which is the honest reading of a Node project.
    if root.join("package.json").is_file() {
        return if root.join("tsconfig.json").is_file() {
            "typescript"
        } else {
            "javascript"
        };
    }
    if root.join("tsconfig.json").is_file() {
        return "typescript";
    }
    // .NET names its manifest after the project, so it has to be looked
    // for by extension rather than by name.
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            match p.extension().and_then(|x| x.to_str()) {
                Some("csproj") | Some("sln") | Some("fsproj") => return "csharp",
                _ => {}
            }
        }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), "").unwrap();
    }

    /// Every language the picker offers has to have an icon and a name,
    /// or a chosen row draws as a plain folder with a blank label - and
    /// every id detection can return has to be in the list, or the
    /// picker cannot show what was detected.
    #[test]
    fn every_offered_language_has_a_name_and_every_detected_one_is_offered() {
        for (id, name) in LANGUAGES {
            assert!(!name.is_empty(), "{id} has no name");
            assert_eq!(language_name(id), *name);
        }
        assert_eq!(language_name("nonsense"), "");
        assert_eq!(language_name(""), "");
        for detected in [
            "rust",
            "go",
            "java",
            "kotlin",
            "dart",
            "elixir",
            "php",
            "ruby",
            "python",
            "zig",
            "haskell",
            "scala",
            "cpp",
            "javascript",
            "typescript",
            "csharp",
        ] {
            assert!(
                LANGUAGES.iter().any(|(k, _)| *k == detected),
                "{detected} can be detected but cannot be picked"
            );
        }
    }

    #[test]
    fn a_folder_is_read_from_the_manifest_it_carries() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(detect_language(tmp.path()), "");
        touch(tmp.path(), "package.json");
        assert_eq!(detect_language(tmp.path()), "javascript");
        touch(tmp.path(), "tsconfig.json");
        assert_eq!(detect_language(tmp.path()), "typescript");
        // A build manifest outranks the looser Node signals: a Rust
        // workspace with web assets beside it is a Rust project.
        touch(tmp.path(), "Cargo.toml");
        assert_eq!(detect_language(tmp.path()), "rust");
    }

    #[test]
    fn removing_an_entry_leaves_the_folder_alone() {
        // The whole point of the list: it refers to folders, it does not
        // own them. If this test ever fails, something has learned to
        // delete a user's source tree.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("my-app");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();

        let mut projects = Projects::load(&tmp.path().join("projects.toml"));
        let entry = ProjectEntry::for_root(&root);
        let id = entry.id.clone();
        projects.upsert(entry).unwrap();
        assert_eq!(projects.all().len(), 1);

        projects.remove(&id).unwrap();
        assert!(projects.all().is_empty(), "the entry is gone from the list");
        assert!(root.is_dir(), "the folder is untouched");
        assert!(root.join("main.rs").is_file(), "its files are untouched");
    }

    /// A list written before the language was ever detected - or by
    /// hand - still draws the right icons.
    #[test]
    fn a_blank_language_is_worked_out_when_the_list_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("svc");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("go.mod"),
            "module svc
",
        )
        .unwrap();
        let path = tmp.path().join("projects.toml");
        std::fs::write(
            &path,
            format!(
                "schema = 1

[[project]]
id = \"a\"
name = \"Svc\"
root = '{}'
language = \"\"
order = 1
",
                root.display()
            ),
        )
        .unwrap();
        let projects = Projects::load(&path);
        assert_eq!(projects.all()[0].language, "go");
    }

    #[test]
    fn the_list_keeps_the_order_it_was_dragged_into() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("projects.toml");
        let mut projects = Projects::load(&path);
        for name in ["alpha", "beta", "gamma"] {
            let mut e = ProjectEntry::for_root(tmp.path().join(name));
            e.name = name.to_string();
            projects.upsert(e).unwrap();
        }
        let names = |p: &Projects| {
            p.all()
                .iter()
                .map(|e| e.name.clone())
                .collect::<Vec<_>>()
                .join(",")
        };
        assert_eq!(names(&projects), "alpha,beta,gamma");
        projects.reorder(2, 0).unwrap();
        assert_eq!(names(&projects), "gamma,alpha,beta");
        // And it survives a reload, which is what the order is for.
        assert_eq!(names(&Projects::load(&path)), "gamma,alpha,beta");
    }

    #[test]
    fn a_folder_is_found_whatever_the_separator_or_trailing_slash() {
        let tmp = tempfile::tempdir().unwrap();
        let mut projects = Projects::load(&tmp.path().join("projects.toml"));
        let root = tmp.path().join("app");
        projects.upsert(ProjectEntry::for_root(&root)).unwrap();
        assert!(projects.by_root(&root).is_some());
        assert!(
            projects
                .by_root(&PathBuf::from(format!("{}/", root.display())))
                .is_some()
        );
    }
}
