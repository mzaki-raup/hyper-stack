//! Restoring a saved layout: which tab is an activity's home tab is
//! remembered, and an activity that has none gets a new one rather than
//! turning a saved note or connection into a dashboard.

use std::collections::HashMap;
use std::sync::Arc;

use hs_core::paths::DataDir;
use hs_core::runtime::NoopWaker;
use hs_core::secrets::KdfParams;
use hs_core::tool::ToolFactory;
use hs_core::workspace::{Layout, LayoutTab};

use crate::tabspace::TabSpace;
use crate::tools::scratchpad::ScratchpadFactory;
use tool_graphics::{GraphicsFactory, GraphicsHub};

fn note(id: u64, text: &str, home: bool) -> LayoutTab {
    LayoutTab {
        id,
        tool: "scratchpad".into(),
        pinned: false,
        colour: None,
        home,
        state: serde_json::json!({ "text": text }),
    }
}

#[test]
fn an_old_layout_keeps_its_notes_and_gains_a_home_tab() {
    let tmp = std::env::temp_dir().join(format!("hs-tabspace-{}", hs_core::new_id()));
    let paths = DataDir::resolve(Some(&tmp)).unwrap();
    let boot = crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap();
    let s = &boot.services;
    let factory = ScratchpadFactory;
    let mut factories: HashMap<String, &dyn ToolFactory> = HashMap::new();
    factories.insert("scratchpad".into(), &factory);

    // Written before the activity had a home tab: no tab carries the flag.
    let layout = Layout {
        tabs: vec![note(1, "# first", false), note(2, "# second", false)],
        active: Some(2),
        ..Default::default()
    };
    let mut tabs = TabSpace::new();
    tabs.restore(&layout, &factories, s);

    assert_eq!(
        tabs.tabs.len(),
        3,
        "a home tab is added, none is taken over"
    );
    assert!(tabs.tabs[0].home);
    assert_eq!(tabs.tabs[0].tool.title(), "Scratchpad");
    assert!(!tabs.tabs[1].home && !tabs.tabs[2].home);
    assert_eq!(tabs.tabs[1].tool.title(), "first");
    assert_eq!(tabs.tabs[2].tool.title(), "second");

    // Saved and restored again, the same tab is the home tab and no second
    // one appears.
    let saved = tabs.to_layout(None);
    assert_eq!(saved.tabs.iter().filter(|t| t.home).count(), 1);
    let mut again = TabSpace::new();
    again.restore(&saved, &factories, s);
    assert_eq!(again.tabs.len(), 3);
    assert!(again.tabs[0].home);
    assert_eq!(again.tabs[1].tool.title(), "first");

    // Pinning puts a tab directly after the permanent one, whatever its
    // place was, and unpinning sends it back behind the pinned ones.
    again.set_pinned(2, true);
    let order: Vec<String> = again
        .tabs
        .iter()
        .map(|t| t.tool.title().to_string())
        .collect();
    assert_eq!(order, ["Scratchpad", "second", "first"]);
    again.set_pinned(1, true);
    let order: Vec<String> = again
        .tabs
        .iter()
        .map(|t| t.tool.title().to_string())
        .collect();
    assert_eq!(
        order,
        ["Scratchpad", "second", "first"],
        "pinned tabs keep their order"
    );
    again.set_pinned(2, false);
    let order: Vec<String> = again
        .tabs
        .iter()
        .map(|t| t.tool.title().to_string())
        .collect();
    assert_eq!(order, ["Scratchpad", "first", "second"]);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn an_old_graphics_image_home_gains_a_dashboard() {
    let tmp = std::env::temp_dir().join(format!("hs-gfx-home-{}", hs_core::new_id()));
    let paths = DataDir::resolve(Some(&tmp)).unwrap();
    let boot = crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap();
    let s = &boot.services;
    let hub = GraphicsHub::shared(s);
    let factory = GraphicsFactory { hub };
    let mut factories: HashMap<String, &dyn ToolFactory> = HashMap::new();
    factories.insert("graphics".into(), &factory);

    // A picture on disk, so the restored tab really holds a document.
    let image_path = tmp.join("old.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
        .save(&image_path)
        .unwrap();
    let old = LayoutTab {
        id: 1,
        tool: "graphics".into(),
        pinned: false,
        colour: None,
        home: true,
        state: serde_json::json!({
            "kind": "image",
            "path": image_path.to_string_lossy(),
        }),
    };
    let layout = Layout {
        tabs: vec![old],
        active: Some(1),
        ..Default::default()
    };
    let mut tabs = TabSpace::new();
    tabs.restore(&layout, &factories, s);

    assert_eq!(
        tabs.tabs.len(),
        2,
        "the old image stays, a dashboard is added"
    );
    assert!(tabs.tabs[0].home);
    assert!(tabs.tabs[0].tool.home_is_dashboard());
    assert_eq!(tabs.tabs[0].tool.title(), "Graphics");
    assert!(!tabs.tabs[1].home, "the image is no longer the home tab");
    assert_eq!(tabs.tabs[1].tool.title(), "old.png");

    let _ = std::fs::remove_dir_all(&tmp);
}
