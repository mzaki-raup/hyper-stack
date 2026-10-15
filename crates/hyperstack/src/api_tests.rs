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
    assert_eq!(r.body.text, "{\"sku\":\"A1\"}");
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
