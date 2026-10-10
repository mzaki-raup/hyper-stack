//! Headless interaction test of the editor view with egui_kittest: typing,
//! select all, undo, and the right-click copy convention.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_editor::view::{EditorResponse, EditorTheme, EditorView};
use hs_editor::{Buffer, Cursors, languages};

struct State {
    view: EditorView,
    buf: Buffer,
    cursors: Cursors,
    last: EditorResponse,
    /// `EditorResponse::copied` is set on the click frame only; keep it until read.
    copied: Option<String>,
}

fn harness<'a>() -> Harness<'a, State> {
    let theme = EditorTheme::dark();
    let state = State {
        view: EditorView::new(
            egui::Id::new("test-editor"),
            languages::detect("a.rs"),
            true,
            13.0,
        ),
        buf: Buffer::from_text(""),
        cursors: Cursors::default(),
        last: EditorResponse::default(),
        copied: None,
    };
    Harness::new_ui_state(
        move |ui, state: &mut State| {
            ui.set_width(600.0);
            ui.set_height(300.0);
            state.last = state
                .view
                .show(ui, &mut state.buf, &mut state.cursors, &theme);
            if let Some(c) = state.last.copied.clone() {
                state.copied = Some(c);
            }
        },
        state,
    )
}

#[test]
fn typing_select_all_right_click_copy_and_undo() {
    let mut harness = harness();
    harness.run();

    let editor = harness.get_by_label("editor");
    editor.click();
    harness.run();
    assert!(
        harness.state().last.focused,
        "clicking the editor focuses it"
    );

    // One Text event per keystroke, as a keyboard produces them: "(" auto-closes,
    // ")" overtypes the closer, "{" auto-closes and leaves the cursor between.
    for piece in ["fn main", "(", ")", " ", "{"] {
        harness.get_by_label("editor").type_text(piece);
        harness.run();
    }
    assert_eq!(harness.state().buf.text(), "fn main() {}");
    assert_eq!(harness.state().cursors.primary().head, 11);

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    assert_eq!(harness.state().cursors.primary().range(), (0, 12));

    // Right-click with a selection copies it and collapses the selection.
    harness.get_by_label("editor").click_secondary();
    harness.run();
    assert_eq!(harness.state().copied.as_deref(), Some("fn main() {}"));
    assert!(!harness.state().cursors.has_selection());
    harness.state_mut().copied = None;

    // Right-click with nothing selected asks for a paste instead of copying.
    harness.get_by_label("editor").click_secondary();
    harness.run();
    assert_eq!(harness.state().copied, None);

    // Undo removes the typed text.
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(harness.state().buf.text(), "");
}
