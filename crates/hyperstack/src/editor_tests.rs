//! The editor tab with several projects, driven through the `Tool` trait
//! with real services on a throwaway data folder.

use std::path::Path;
use std::sync::Arc;

use hs_core::paths::DataDir;
use hs_core::runtime::NoopWaker;
use hs_core::secrets::KdfParams;
use hs_core::tool::{OpenRequest, ToolContext, ToolFactory};
use serde_json::{Value, json};

fn files(state: &Value, project: usize) -> Vec<String> {
    state["projects"][project]["files"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|f| {
                    Path::new(f["path"].as_str().unwrap())
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn requests(actions: &[hs_core::tool::Action], new_tab: bool) -> Vec<String> {
    actions
        .iter()
        .filter_map(|a| match a {
            hs_core::tool::Action::OpenTool {
                factory_id,
                request: Some(OpenRequest::File { uri, .. }),
            } if factory_id == "editor" && !new_tab => Some(uri.clone()),
            hs_core::tool::Action::OpenToolNewTab {
                factory_id,
                request: Some(OpenRequest::File { uri, .. }),
            } if factory_id == "editor" && new_tab => Some(uri.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn one_tab_per_project_with_its_files_inside() {
    let tmp = std::env::temp_dir().join(format!("hs-editor-projects-{}", hs_core::new_id()));
    std::fs::create_dir_all(&tmp).unwrap();
    // The editor reports canonical paths; on the CI runner `%TEMP%` is
    // spelled `C:\WINDOWS\SystemTemp` while the volume says `C:\Windows`,
    // so expected paths are built from the canonical folder (without the
    // `\\?\` prefix `canonicalize` adds on Windows).
    let canonical = std::fs::canonicalize(&tmp).unwrap();
    let canonical = canonical.to_string_lossy().to_string();
    let tmp = std::path::PathBuf::from(
        canonical
            .strip_prefix(r"\\?\")
            .unwrap_or(&canonical)
            .to_string(),
    );
    let (a, b, other) = (tmp.join("alpha"), tmp.join("beta"), tmp.join("loose"));
    std::fs::create_dir_all(a.join("sub")).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(a.join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(a.join("sub/b.rs"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(b.join("c.md"), "# c\n").unwrap();
    std::fs::write(other.join("o.txt"), "o\n").unwrap();

    let paths = DataDir::resolve(Some(&tmp.join("data"))).unwrap();
    let boot = crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap();
    let s = &boot.services;
    let factory = tool_editor::EditorFactory;
    let p = |p: &Path| p.to_string_lossy().to_string();
    let file = |path: &Path, line: Option<u32>| OpenRequest::File { uri: p(path), line };
    let mut actions = Vec::new();
    let mut cx = ToolContext {
        services: s,
        tab_id: 1,
        env_id: None,
        actions: &mut actions,
        is_home: false,
    };

    // A layout saved before several projects existed: one folder, one
    // file. The window tab is the project, titled with its folder.
    let legacy = json!({ "root": a, "path": a.join("a.rs"), "cursor": 3 });
    let mut t = factory.create(s, Some(legacy));
    assert_eq!(t.title(), "alpha");
    // The shell routes to this tab: its files and folder, any file inside
    // it; not another project or a file elsewhere.
    assert_eq!(t.uri_affinity(&p(&a.join("a.rs"))), Some(0));
    assert_eq!(t.uri_affinity(&p(&a)), Some(0));
    assert_eq!(
        t.uri_affinity(&p(&a.join("sub/.."))),
        Some(0),
        "a normalized spelling of an existing project root has exact affinity"
    );
    #[cfg(windows)]
    assert_eq!(
        t.uri_affinity(&p(&a.join("sub\\.."))),
        Some(0),
        "Windows-style separator variants still identify the same project root"
    );
    assert_eq!(t.uri_affinity(&p(&a.join("sub/b.rs"))), Some(3));
    assert_eq!(t.uri_affinity(&p(&b)), None);
    assert_eq!(t.uri_affinity(&p(&other.join("o.txt"))), None);

    // A file of the project opens as another file tab inside it.
    assert!(t.open(file(&a.join("sub/b.rs"), Some(2)), &mut cx));
    assert_eq!(t.title(), "alpha");
    let state = t.save_state();
    assert_eq!(state["projects"].as_array().unwrap().len(), 1);
    assert_eq!(files(&state, 0), ["a.rs", "b.rs"]);
    // The cursor followed the requested line (line 2 starts at char 4).
    assert_eq!(state["projects"][0]["files"][1]["cursor"], 4);
    assert!(
        state.get("path").is_none(),
        "the old one-file fields are not written"
    );
    let t2 = factory.create(s, Some(state.clone()));
    assert_eq!(
        t2.save_state(),
        state,
        "the saved layout restores as it was"
    );

    // Another folder never joins this tab: it goes to a new one.
    assert!(t.open(file(&b, None), &mut cx));
    assert_eq!(requests(cx.actions, true), [p(&b)]);
    assert_eq!(t.save_state()["projects"].as_array().unwrap().len(), 1);

    // A fresh tab takes the folder and becomes that project.
    let mut tb = factory.create(s, None);
    assert_eq!(tb.uri_affinity(&p(&b)), Some(200));
    assert!(tb.open(file(&b, None), &mut cx));
    assert_eq!(tb.title(), "beta");
    assert!(tb.open(file(&b.join("c.md"), None), &mut cx));
    assert_eq!(files(&tb.save_state(), 0), ["c.md"]);

    // A file outside every project: an Other Files tab collects them.
    let mut to = factory.create(s, None);
    assert!(to.open(file(&other.join("o.txt"), None), &mut cx));
    assert_eq!(to.title(), "o.txt");
    assert_eq!(to.uri_affinity(&p(&tmp.join("x.txt"))), Some(200));
    assert_eq!(
        to.uri_affinity(&p(&b)),
        None,
        "a folder is not an Other File"
    );

    // Next/previous walk the file tabs; closing the last leaves Untitled.
    assert!(t.run_command("editor.next_file", &mut cx));
    assert!(t.run_command("editor.close_file", &mut cx));
    assert_eq!(files(&t.save_state(), 0).len(), 1);
    assert!(t.run_command("editor.close_file", &mut cx));
    assert!(files(&t.save_state(), 0).is_empty());
    assert!(!t.dirty());

    // A tab saved with several projects keeps one; the others move to
    // tabs of their own on its first frame (not saved back here).
    let multi = json!({
        "projects": [
            { "root": a, "files": [{ "path": a.join("a.rs") }] },
            { "root": b, "files": [{ "path": b.join("c.md") }] }
        ],
        "active": 1
    });
    let tm = factory.create(s, Some(multi));
    assert_eq!(tm.title(), "beta");
    assert_eq!(tm.save_state()["projects"].as_array().unwrap().len(), 1);

    // The home tab is a dashboard and answers for nothing. It used to
    // report the file it happened to hold, which meant the first file
    // opened anywhere landed in it - and the activity then had no
    // dashboard for the rest of the workspace's life, with no way back
    // (owner's report, 2026-10-27).
    let home = json!({ "root": a, "path": a.join("a.rs") });
    let mut th = factory.create(s, Some(home));
    assert_eq!(
        th.uri_affinity(&p(&a.join("a.rs"))),
        Some(0),
        "a working tab still answers for the file it holds"
    );
    th.set_home_tab(true, s);
    assert_eq!(
        th.current_uri(),
        None,
        "a dashboard shows nothing of its own"
    );
    assert_eq!(
        th.uri_affinity(&p(&a.join("a.rs"))),
        None,
        "and claims no file, so the shell gives the file a tab beside it"
    );
    assert_eq!(th.uri_affinity(&p(&a)), None);
    assert!(
        th.project_roots().is_empty(),
        "a dashboard holds no project, so removing one never closes it"
    );
    assert!(
        !th.draws_toolbar(),
        "and has nothing for a toolbar of editor verbs to act on"
    );
    assert!(
        !th.open(file(&a.join("a.rs"), None), &mut cx),
        "a request routed here directly is refused rather than filling the dashboard"
    );

    drop(boot);
    let _ = std::fs::remove_dir_all(&tmp);
}
