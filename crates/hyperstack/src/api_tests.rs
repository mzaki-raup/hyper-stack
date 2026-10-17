//! The API client's assistant actions, applied through the real tool on a
//! throwaway data folder: collections and requests land on disk exactly as
//! the sidebar would create them, and nothing is sent.

use std::sync::Arc;

use hs_core::paths::DataDir;
use hs_core::runtime::NoopWaker;
use hs_core::secrets::KdfParams;
use hs_core::tool::ToolContext;
use tool_api::model::{AuthKind, BodyKind};

#[test]
fn assistant_block_builds_a_collection_and_fills_the_open_request() {
    let tmp = std::env::temp_dir().join(format!("hs-api-assist-{}", hs_core::new_id()));
    let paths = DataDir::resolve(Some(&tmp)).unwrap();
    let boot = crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap();
    let s = &boot.services;
    let mut actions = Vec::new();
    let mut cx = ToolContext {
        services: s,
        tab_id: 1,
        env_id: None,
        actions: &mut actions,
        is_home: false,
    };
    let mut t = tool_api::ApiTool::new_in(s);
    assert!(t.collections().is_empty());

    let block = r##"{"actions":[
        {"op":"new_collection","name":"Orders"},
        {"op":"new_request","name":"Create order","method":"post","url":"{{host}}/orders","folder":"Write"},
        {"op":"set_header","key":"Content-Type","value":"application/json"},
        {"op":"set_param","key":"dry_run","value":"true","enabled":false},
        {"op":"set_body","kind":"json","text":"{\"sku\":\"A1\"}"},
        {"op":"set_auth","kind":"bearer","token":"{{token}}"},
        {"op":"set_script","phase":"post","code":"pm.test('ok', () => pm.response.to.have.status(201));"},
        {"op":"set_docs","text":"# Create order"},
        {"op":"set_variable","scope":"collection","key":"host","value":"https://api.example.test"},
        {"op":"fly_to_the_moon"},
        {"op":"save"}
    ]}"##;
    let rep = t.apply_assistant_block(block, &mut cx).unwrap_err();
    assert!(
        rep.contains("action 10"),
        "an unknown op is reported: {rep}"
    );

    // Without the bad action the whole block lands.
    let block = block.replace(r#"{"op":"fly_to_the_moon"},"#, "");
    let rep = t.apply_assistant_block(&block, &mut cx).unwrap();
    assert_eq!(rep.errors, Vec::<String>::new());
    assert_eq!(rep.applied, 10);
    assert_eq!(
        rep.focus_tab,
        Some(5),
        "the docs tab shows the last field change"
    );

    let r = t.request();
    assert_eq!(r.method, "POST");
    assert_eq!(r.url, "{{host}}/orders");
    assert_eq!(r.name, "Create order");
    assert_eq!(r.headers.len(), 1);
    assert_eq!(r.headers[0].value, "application/json");
    assert!(!r.params[0].enabled);
    assert_eq!(r.body.kind, BodyKind::Json);
    // A JSON body the assistant sets is laid out as it lands (2026-10-16).
    assert_eq!(
        r.body.text,
        "{
  \"sku\": \"A1\"
}"
    );
    assert_eq!(r.auth.kind, AuthKind::Bearer);
    assert_eq!(r.auth.token, "{{token}}");
    assert!(r.post_script.contains("pm.test"));
    assert_eq!(r.docs, "# Create order");

    let c = &t.collections()[0];
    assert_eq!(c.name, "Orders");
    assert_eq!(c.request_count(), 1);
    assert_eq!(c.variables[0].key, "host");
    let (saved, folders) = c
        .find(&r.id)
        .expect("the request is saved in the collection");
    assert_eq!(saved, r, "save wrote the edited request back");
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].name, "Write");
    // On disk, as the sidebar would have written it.
    let on_disk = std::fs::read_to_string(
        tmp.join("workspaces/default/api/collections")
            .join(format!("{}.json", c.id)),
    )
    .unwrap();
    assert!(on_disk.contains("Create order"));
    assert!(
        !on_disk.contains("fly_to_the_moon"),
        "the rejected block never touched disk"
    );

    // Applying the same block again (a double click, a re-run) changes
    // nothing: the collection and the request are reused, not twinned.
    let rep = t.apply_assistant_block(&block, &mut cx).unwrap();
    assert_eq!(rep.applied, 10);
    assert_eq!(t.collections().len(), 1);
    assert_eq!(t.collections()[0].request_count(), 1);

    // A second block edits the same open request by name, case-insensitively.
    let rep = t
        .apply_assistant_block(
            r#"[{"op":"open_request","name":"create ORDER"},{"op":"set_method","method":"put"},{"op":"remove_header","key":"content-type"}]"#,
            &mut cx,
        )
        .unwrap();
    assert_eq!(rep.applied, 3);
    assert_eq!(t.request().method, "PUT");
    assert!(t.request().headers.is_empty());

    // Nothing was sent: no history file appeared.
    assert!(!tmp.join("workspaces/default/api/history.jsonl").exists());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A request can be dragged anywhere in the sidebar: another folder,
/// another collection's root, a folder of another collection. Only
/// reordering inside one parent used to work, because the move carried a
/// single collection id. Owner's report, 2026-10-18.
#[test]
fn a_request_moves_between_folders_and_between_collections() {
    let tmp = std::env::temp_dir().join(format!("hs-api-move-{}", hs_core::new_id()));
    let paths = DataDir::resolve(Some(&tmp)).unwrap();
    let boot = crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap();
    let s = &boot.services;
    let mut actions = Vec::new();
    let mut cx = ToolContext {
        services: s,
        tab_id: 1,
        env_id: None,
        actions: &mut actions,
        is_home: false,
    };
    let mut t = tool_api::ApiTool::new_in(s);
    t.apply_assistant_block(
        r#"[{"op":"new_collection","name":"Alpha"},
            {"op":"new_request","name":"Ping","method":"get","url":"/ping","folder":"Health"},
            {"op":"save"}]"#,
        &mut cx,
    )
    .unwrap();
    t.apply_assistant_block(
        r#"[{"op":"new_collection","name":"Beta"},{"op":"save"}]"#,
        &mut cx,
    )
    .unwrap();

    let alpha = t.collections()[0].id.clone();
    let beta = t.collections()[1].id.clone();
    let folder = match &t.collections()[0].items[0] {
        tool_api::model::Item::Folder(f) => f.id.clone(),
        other => panic!("expected the folder first, got {other:?}"),
    };
    let moved = t.collections()[0].requests()[0].1.id.clone();
    let alpha_requests = t.collections()[0].request_count();

    // Out of a folder, into the same collection's root.
    t.move_item(&alpha, Some(&folder), 0, &alpha, None, usize::MAX, &mut cx);
    assert!(
        t.collections()[0].find(&moved).unwrap().1.is_empty(),
        "no longer inside a folder"
    );
    assert_eq!(t.collections()[0].request_count(), alpha_requests);

    // Into another collection's root: it leaves the first file and lands
    // in the second.
    let at = t.collections()[0]
        .items
        .iter()
        .position(|i| i.id() == moved)
        .unwrap();
    t.move_item(&alpha, None, at, &beta, None, usize::MAX, &mut cx);
    assert_eq!(t.collections()[0].request_count(), alpha_requests - 1);
    assert_eq!(t.collections()[1].request_count(), 1);
    let dir = tmp.join("workspaces/default/api/collections");
    let a_disk = std::fs::read_to_string(dir.join(format!("{alpha}.json"))).unwrap();
    let b_disk = std::fs::read_to_string(dir.join(format!("{beta}.json"))).unwrap();
    assert!(!a_disk.contains("Ping"), "the source file was written back");
    assert!(b_disk.contains("Ping"), "the target file has it now");

    // Back into the first collection's folder.
    t.move_item(&beta, None, 0, &alpha, Some(&folder), usize::MAX, &mut cx);
    assert_eq!(t.collections()[1].request_count(), 0);
    let (_, folders) = t.collections()[0].find(&moved).expect("back in Alpha");
    assert_eq!(folders.len(), 1, "inside the folder it came from");
    assert_eq!(folders[0].id, folder);

    let _ = std::fs::remove_dir_all(&tmp);
}
