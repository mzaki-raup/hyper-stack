//! A dashboard's Start Here row on the light theme: the primary button's
//! label is white on the accent, not the theme's dark text (CR-0045).
//! Read back from what egui painted, so it checks what reaches the screen.

use egui_kittest::Harness;
use hs_ui::egui;

/// The colour each painted text run was given, with its text.
fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Color32)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Color32)>) {
        match shape {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            egui::Shape::Text(t) => {
                // A button's label is one run; its colour is the first
                // section's unless the shape overrides it.
                if let Some(section) = t.galley.job.sections.first() {
                    let colour = t.override_text_color.unwrap_or(section.format.color);
                    out.push((t.galley.text().to_string(), colour));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in shapes {
        walk(&c.shape, &mut out);
    }
    out
}

#[test]
fn the_primary_start_here_button_is_white_on_the_accent_in_light_mode() {
    let theme = hs_ui::theme::Theme::light();
    let labels = vec!["Add Note".to_string(), "Add Folder".to_string()];
    let mut harness = Harness::new_ui(|ui| {
        hs_ui::dashboard::actions(ui, &theme, &labels);
    });
    harness.run();
    let texts = painted_text(&harness.output().shapes);
    let primary = texts
        .iter()
        .find(|(t, _)| t == "Add Note")
        .expect("the primary label is painted");
    assert_eq!(primary.1, egui::Color32::WHITE, "white on the accent");
    let other = texts
        .iter()
        .find(|(t, _)| t == "Add Folder")
        .expect("the second label is painted");
    assert_ne!(
        other.1,
        egui::Color32::WHITE,
        "an ordinary button keeps the theme's text colour"
    );
}
