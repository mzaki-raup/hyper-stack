//! The Graphics image editor's settings rows, drawn through the `Tool`
//! trait with real services on a throwaway data folder.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_core::paths::DataDir;
use hs_core::runtime::NoopWaker;
use hs_core::secrets::KdfParams;
use hs_core::tool::{Action, ToolContext, ToolFactory};
use tool_graphics::{GraphicsFactory, GraphicsHub};

/// Each Transform row's label and its Apply button sit on the middle line
/// of the row's fields, and the button is a field's height. The label was
/// a plain label at its own height in a field-height row, so it sat high
/// beside W / H, and Apply was a small button, shorter than the fields
/// (BUG-0058).
#[test]
fn transform_rows_sit_on_one_middle_line() {
    let tmp = std::env::temp_dir().join(format!("hs-gfx-rows-{}", hs_core::new_id()));
    let paths = DataDir::resolve(Some(&tmp.join("data"))).unwrap();
    let boot = crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap();
    let s = &boot.services;
    let image_path = tmp.join("pic.png");
    image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 120, 255, 255]))
        .save(&image_path)
        .unwrap();
    let factory = GraphicsFactory {
        hub: GraphicsHub::shared(s),
    };
    let mut tool = factory.create(
        s,
        Some(serde_json::json!({
            "kind": "image",
            "path": image_path.to_string_lossy(),
        })),
    );
    let mut fonts_ready = false;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 2000.0))
        .build_ui(|ui| {
            if !fonts_ready {
                // The tools draw with the named families the app installs
                // at startup; a fresh context has none.
                hs_ui::icons::install_fonts(ui.ctx());
                fonts_ready = true;
                return;
            }
            let mut actions: Vec<Action> = Vec::new();
            let mut cx = ToolContext {
                services: s,
                tab_id: 1,
                env_id: None,
                actions: &mut actions,
                is_home: false,
            };
            tool.ui(ui, &mut cx);
        });
    h.run_steps(10);

    // The W / H, scale and margin boxes are drag values (spin buttons);
    // a few are text fields.
    let fields: Vec<egui::Rect> = h
        .query_all_by_role(egui::accesskit::Role::SpinButton)
        .chain(h.query_all_by_role(egui::accesskit::Role::TextInput))
        .map(|n| n.rect())
        .collect();
    let applies: Vec<egui::Rect> = h
        .query_all_by_label_contains("Apply")
        .map(|n| n.rect())
        .collect();
    let mut checked = 0;
    // Margins has no button of its own: Extend Canvas applies from Fill.
    for (name, has_apply) in [
        ("Size", true),
        ("Scale", true),
        ("Margins", false),
        ("Area", true),
    ] {
        let label = h
            .query_all_by_label(name)
            .map(|n| n.rect())
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .unwrap_or_else(|| panic!("no {name:?} row is drawn"));
        let y = label.center().y;
        let field = fields
            .iter()
            .filter(|f| f.left() > label.right() && (f.center().y - y).abs() < 24.0)
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .unwrap_or_else(|| panic!("no field beside {name:?}"));
        assert!(
            (field.center().y - y).abs() < 1.5,
            "{name:?}: the label's middle is y={y}, the field's y={}",
            field.center().y
        );
        let apply = applies.iter().find(|a| (a.center().y - y).abs() < 24.0);
        assert_eq!(apply.is_some(), has_apply, "{name:?}: an Apply button");
        checked += 1;
        let Some(apply) = apply else { continue };
        assert!(
            (apply.center().y - y).abs() < 1.5,
            "{name:?}: Apply's middle is y={}, the row's y={y}",
            apply.center().y
        );
        assert!(
            (apply.height() - field.height()).abs() < 1.5,
            "{name:?}: Apply is {} tall, the field {}",
            apply.height(),
            field.height()
        );
    }
    assert_eq!(checked, 4);
    drop(h);
    drop(boot);
    let _ = std::fs::remove_dir_all(&tmp);
}
