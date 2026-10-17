//! hs-git against a real repository built with the installed git: status in
//! every state the editor shows, file diffs, history and branches.

use std::path::Path;
use std::process::Command;

use hs_git::{Kind, LineKind, Side};

fn run(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn a_repository_in_every_state() {
    if hs_git::version().is_err() {
        eprintln!("git not installed; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    run(root, &["init", "-q", "-b", "main"]);
    run(root, &["config", "commit.gpgsign", "false"]);

    // No commits yet.
    let st = hs_git::status(root).unwrap();
    assert_eq!(st.branch.as_deref(), Some("main"));
    assert_eq!(st.head, None);
    assert!(hs_git::log(root, 0, 10).unwrap().is_empty());

    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(root.join("old.md"), "# doc\n\nsome text here\nmore text\n").unwrap();
    run(root, &["add", "."]);
    run(root, &["commit", "-q", "-m", "First commit"]);
    run(root, &["branch", "feature"]);

    // Unstaged edit, staged new file, rename, untracked file in a folder.
    std::fs::write(root.join("src/a.rs"), "one\n2\nthree\nfour\n").unwrap();
    std::fs::write(root.join("b.txt"), "new\n").unwrap();
    run(root, &["add", "b.txt"]);
    run(root, &["mv", "old.md", "new.md"]);
    std::fs::create_dir_all(root.join("notes")).unwrap();
    std::fs::write(root.join("notes/todo.txt"), "a\nb\n").unwrap();

    assert_eq!(
        hs_git::discover(&root.join("src/a.rs")).as_deref(),
        Some(root)
    );
    let st = hs_git::status(root).unwrap();
    assert!(st.head.is_some());
    let get = |p: &str| {
        st.files
            .iter()
            .find(|f| f.path == p)
            .unwrap_or_else(|| panic!("{p}"))
    };
    assert!(get("src/a.rs").unstaged() && !get("src/a.rs").staged());
    assert!(get("b.txt").staged());
    assert_eq!(get("new.md").orig_path.as_deref(), Some("old.md"));
    assert_eq!(get("new.md").index, 'R');
    assert_eq!(get("notes/todo.txt").kind, Kind::Untracked);

    let d = hs_git::file_diff(root, "src/a.rs", Side::Unstaged).unwrap();
    assert_eq!((d.added(), d.removed()), (2, 1));
    let added: Vec<&str> = d.hunks[0]
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Added)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(added, ["2", "four"]);
    let d = hs_git::file_diff(root, "b.txt", Side::Staged).unwrap();
    assert_eq!(d.added(), 1);
    assert!(d.notes.iter().any(|n| n.starts_with("new file")));
    let d = hs_git::file_diff(root, "notes/todo.txt", Side::Untracked).unwrap();
    assert_eq!(d.added(), 2);
    assert_eq!(
        hs_git::head_text(root, "src/a.rs").unwrap().as_deref(),
        Some("one\ntwo\nthree\n")
    );
    assert_eq!(hs_git::head_text(root, "b.txt").unwrap(), None);
    // Nothing staged for a file changed only in the work tree.
    assert!(
        hs_git::file_diff(root, "src/a.rs", Side::Staged)
            .unwrap()
            .hunks
            .is_empty()
    );

    run(root, &["add", "-A"]);
    run(root, &["commit", "-q", "-m", "Second\n\nWith a body."]);
    let log = hs_git::log(root, 0, 10).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].subject, "Second");
    assert!(log[0].refs.iter().any(|r| r == "HEAD -> main"));
    assert!(log[1].parents.is_empty());
    assert_eq!(hs_git::log(root, 1, 10).unwrap().len(), 1);

    let parent = log[0].parents.first().map(String::as_str);
    let det = hs_git::commit_details(root, &log[0].id, parent).unwrap();
    assert_eq!(det.message, "Second\n\nWith a body.");
    let names: Vec<(char, &str)> = det
        .files
        .iter()
        .map(|f| (f.status, f.path.as_str()))
        .collect();
    assert!(names.contains(&('R', "new.md")), "{names:?}");
    assert!(names.contains(&('M', "src/a.rs")));
    let renamed = det.files.iter().find(|f| f.path == "new.md").unwrap();
    assert_eq!(renamed.orig_path.as_deref(), Some("old.md"));
    let f = det.files.iter().find(|f| f.path == "src/a.rs").unwrap();
    let d = hs_git::commit_file_diff(root, &log[0].id, parent, f).unwrap();
    assert_eq!((d.added(), d.removed()), (2, 1));
    // The first commit added everything.
    let first = hs_git::commit_details(root, &log[1].id, None).unwrap();
    assert!(first.files.iter().all(|f| f.status == 'A'));
    let d = hs_git::commit_file_diff(root, &log[1].id, None, &first.files[0]).unwrap();
    assert!(d.added() > 0);

    let b = hs_git::branches(root).unwrap();
    let names: Vec<&str> = b.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"main") && names.contains(&"feature"));
    assert!(b.iter().find(|b| b.name == "main").unwrap().current);
}

/// The write side against the same kind of fixture: stage, unstage,
/// discard, commit, stash, branches and remotes, each checked through the
/// read side afterwards.
#[test]
fn the_write_side_round_trips_through_status() {
    if hs_git::version().is_err() {
        eprintln!("git not installed; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    run(root, &["init", "-q", "-b", "main"]);
    run(root, &["config", "commit.gpgsign", "false"]);
    run(root, &["config", "user.name", "Ada"]);
    run(root, &["config", "user.email", "ada@example.com"]);
    // A Windows git with core.autocrlf=true would hand back CRLF.
    run(root, &["config", "core.autocrlf", "false"]);
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    std::fs::write(root.join("b.txt"), "two\n").unwrap();

    // Stage one, leave one untracked.
    hs_git::write::stage(root, &["a.txt"]).unwrap();
    let st = hs_git::status(root).unwrap();
    let a = st.files.iter().find(|f| f.path == "a.txt").unwrap();
    assert!(a.staged() && !a.unstaged());
    assert_eq!(
        st.files.iter().find(|f| f.path == "b.txt").unwrap().kind,
        Kind::Untracked
    );

    // Unstage it again, then stage everything and commit.
    hs_git::write::unstage(root, &["a.txt"]).unwrap();
    assert_eq!(
        hs_git::status(root)
            .unwrap()
            .files
            .iter()
            .filter(|f| f.staged())
            .count(),
        0
    );
    hs_git::write::stage_all(root).unwrap();
    hs_git::write::commit(root, "First", &hs_git::CommitOptions::default()).unwrap();
    assert!(hs_git::status(root).unwrap().files.is_empty());
    assert_eq!(hs_git::log(root, 0, 10).unwrap()[0].subject, "First");

    // Amend with sign-off.
    hs_git::write::commit(
        root,
        "First, amended",
        &hs_git::CommitOptions {
            amend: true,
            signoff: true,
        },
    )
    .unwrap();
    let log = hs_git::log(root, 0, 10).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].subject, "First, amended");

    // Discard a tracked change; remove an untracked file.
    std::fs::write(root.join("a.txt"), "changed\n").unwrap();
    std::fs::write(root.join("junk.tmp"), "x").unwrap();
    hs_git::write::discard(root, &["a.txt"]).unwrap();
    hs_git::write::remove_untracked(root, &["junk.tmp"]).unwrap();
    assert!(hs_git::status(root).unwrap().files.is_empty());
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );

    // Stash and pop.
    std::fs::write(root.join("a.txt"), "stashed\n").unwrap();
    hs_git::write::stash_push(root, "keep this", false).unwrap();
    assert!(hs_git::status(root).unwrap().files.is_empty());
    let stashes = hs_git::write::stash_list(root).unwrap();
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].message.contains("keep this"));
    hs_git::write::stash_pop(root, &stashes[0].id).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "stashed\n"
    );
    assert!(hs_git::write::stash_list(root).unwrap().is_empty());
    hs_git::write::discard(root, &["a.txt"]).unwrap();

    // Branches: create, switch back, delete.
    hs_git::write::create_branch(root, "feature", None).unwrap();
    assert_eq!(
        hs_git::status(root).unwrap().branch.as_deref(),
        Some("feature")
    );
    hs_git::write::checkout(root, "main").unwrap();
    hs_git::write::delete_branch(root, "feature", false).unwrap();
    assert!(
        hs_git::branches(root)
            .unwrap()
            .iter()
            .all(|b| b.name != "feature")
    );

    // Remotes, and the guard over them.
    run(
        root,
        &[
            "remote",
            "add",
            "origin",
            "https://example.test/team/prod-api.git",
        ],
    );
    let remotes = hs_git::write::remotes(root).unwrap();
    assert_eq!(remotes[0].name, "origin");
    assert!(hs_git::write::is_protected(
        &["prod".into()],
        &remotes[0].url,
        "main"
    ));
}
