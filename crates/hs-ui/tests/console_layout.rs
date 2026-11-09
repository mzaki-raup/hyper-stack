//! The layout the Editor's debug console uses: an input row at the bottom
//! of a panel, laid out bottom-up, and a scroll area of output above it
//! that sticks to the bottom (`tool_editor::debug::console_ui`).
//!
//! An `egui::ScrollArea` does not shrink below `min_scrolled_height`
//! (64 px by default). In a short panel the output was given less than
//! that, took 64 px anyway and spilled down over the input row: the last
//! line was drawn under the field (BUG-0055). The console sets the
//! minimum to 0; this test pins down why.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// Draws the console's layout in a bottom panel `panel` px tall with `n`
/// lines of output, and returns (the field's top, the last line's bottom).
fn measure(panel: f32, n: usize, min_scrolled: Option<f32>) -> (f32, f32) {
    let mut input = String::new();
    let mut h = Harness::builder()
        .with_size(egui::vec2(600.0, 500.0))
        .build_ui(move |ui| {
            egui::Panel::bottom("console")
                .exact_size(panel)
                .show(ui, |ui| {
                    ui.label("Console");
                    ui.separator();
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        hs_ui::form::centred_row(ui, |ui| {
                            ui.label(">");
                            ui.add(hs_ui::form::line_edit(&mut input).desired_width(f32::INFINITY));
                        });
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            let mut area = egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .stick_to_bottom(true);
                            if let Some(m) = min_scrolled {
                                area = area.min_scrolled_height(m);
                            }
                            area.show(ui, |ui| {
                                for i in 0..n {
                                    ui.add(egui::Label::new(format!("line {i}")).wrap());
                                }
                            });
                        });
                    });
                });
        });
    h.run_steps(5);
    let field = h.get_by_role(egui::accesskit::Role::TextInput).rect();
    let last = h.get_by_label(&format!("line {}", n - 1)).rect();
    (field.top(), last.bottom())
}

#[test]
fn a_short_console_keeps_its_output_above_the_field() {
    for n in [3, 30] {
        let (field, last) = measure(120.0, n, Some(0.0));
        assert!(
            last <= field,
            "{n} lines: the last line ends at y={last}, under the field at y={field}"
        );
    }
}

/// Without the minimum set to 0 the output spills over the field - the
/// defect, kept as a test so a change in egui's default is noticed.
#[test]
fn the_default_minimum_spills_over_the_field() {
    let (field, last) = measure(120.0, 30, None);
    assert!(
        last > field,
        "egui's default no longer spills (last line y={last}, field y={field})"
    );
}
