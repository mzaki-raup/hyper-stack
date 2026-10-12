//! The editor tab with several projects, driven through the `Tool` trait
//! with real services on a throwaway data folder.

use std::path::Path;
use std::sync::Arc;

use hs_core::paths::DataDir;
use hs_core::runtime::NoopWaker;
use hs_core::secrets::KdfParams;
use hs_core::tool::{OpenRequest, Tool, ToolContext, ToolFactory};
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

#[test]
fn several_projects_each_keep_their_files() {
    let tmp = std::env::temp_dir().join(format!("hs-editor-projects-{}", hs_core::new_id()));
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
    let mut actions = Vec::new();

    // A layout saved before several projects existed: one folder, one file.
    let legacy = json!({ "root": a, "path": a.join("a.rs"), "cursor": 3 });
    let mut t = factory.create(s, Some(legacy));
    assert_eq!(t.title(), "a.rs");
    let p = |p: &Path| p.to_string_lossy().to_string();
    assert_eq!(t.uri_affinity(&p(&a.join("a.rs"))), Some(0));
    assert_eq!(t.uri_affinity(&p(&a.join("sub/b.rs"))), Some(1));
    assert_eq!(t.uri_affinity(&p(&other.join("o.txt"))), Some(2));

    let mut open = |t: &mut Box<dyn Tool>, path: &Path, line: Option<u32>| {
        let mut cx = ToolContext {
            services: s,
            tab_id: 1,
            env_id: None,
            actions: &mut actions,
        };
        assert!(t.open(OpenRequest::File { uri: p(path), line }, &mut cx));
    };
    open(&mut t, &b, None); // a folder becomes a second project
    open(&mut t, &b.join("c.md"), None);
    open(&mut t, &other.join("o.txt"), None);
    open(&mut t, &a.join("sub/b.rs"), Some(2));
    assert_eq!(t.title(), "b.rs");
    assert_eq!(t.uri_affinity(&p(&b.join("c.md"))), Some(0));

    let state = t.save_state();
    assert_eq!(state["projects"].as_array().unwrap().len(), 3);
    assert_eq!(state["projects"][0]["root"], json!(a));
    assert_eq!(files(&state, 0), ["a.rs", "b.rs"]);
    assert_eq!(state["projects"][0]["active"], 1);
    // The cursor followed the requested line (line 2 starts at char 4).
    assert_eq!(state["projects"][0]["files"][1]["cursor"], 4);
    assert_eq!(state["projects"][1]["root"], json!(b));
    assert_eq!(files(&state, 1), ["c.md"]);
    assert!(state["projects"][2].get("root").is_none());
    assert_eq!(files(&state, 2), ["o.txt"]);
    assert_eq!(state["active"], 0);
    assert!(
        state.get("path").is_none(),
        "the old one-file fields are not written"
    );

    // The saved layout restores to the same layout.
    let t2 = factory.create(s, Some(state.clone()));
    assert_eq!(t2.save_state(), state);
    assert_eq!(t2.title(), "b.rs");

    // Next/previous file walk across the rows; closing a project keeps the rest.
    let mut cx = ToolContext {
        services: s,
        tab_id: 1,
        env_id: None,
        actions: &mut actions,
    };
    assert!(t.run_command("editor.next_file", &mut cx));
    assert_eq!(t.title(), "c.md");
    assert!(t.run_command("editor.prev_file", &mut cx));
    assert_eq!(t.title(), "b.rs");
    assert!(t.run_command("editor.close_project", &mut cx));
    let state = t.save_state();
    assert_eq!(state["projects"].as_array().unwrap().len(), 2);
    assert_eq!(files(&state, 0), ["c.md"]);
    assert_eq!(t.title(), "c.md");
    assert!(t.run_command("editor.close_file", &mut cx));
    assert_eq!(t.title(), "o.txt");
    assert!(t.run_command("editor.close_file", &mut cx));
    // Nothing left open: an empty untitled file, not a broken tab.
    assert_eq!(t.title(), "Untitled");
    assert!(!t.dirty());
    drop(boot);
    let _ = std::fs::remove_dir_all(&tmp);
}
