//! The typed confirmation's danger button does nothing until the name is
//! typed back exactly (FEAT-0116, ui.md: a typed confirmation for delete-
//! style verbs). Driven headlessly with egui_kittest.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_ui::dialog::{DialogResult, TypedConfirm, typed_confirm};

struct St {
    typed: String,
    result: DialogResult,
}

fn harness() -> Harness<'static, St> {
    let theme = hs_ui::theme::Theme::dark();
    Harness::new_ui_state(
        move |ui, st: &mut St| {
            let r = typed_confirm(
                ui.ctx(),
                &theme,
                "test",
                &TypedConfirm {
                    title: "Remove the User",
                    body: "Type the user's name to confirm.",
                    name: "alice",
                    ok_label: "Delete",
                    cancel_label: "Cancel",
                },
                &mut st.typed,
            );
            if r != DialogResult::Open {
                st.result = r;
            }
        },
        St {
            typed: String::new(),
            result: DialogResult::Open,
        },
    )
}

#[test]
fn ok_does_nothing_until_the_name_matches() {
    let mut h = harness();
    h.run();
    h.get_by_label_contains("Delete").click();
    h.run();
    assert!(h.state().result == DialogResult::Open, "empty: OK is off");

    h.state_mut().typed = "alic".into();
    h.run();
    h.get_by_label_contains("Delete").click();
    h.run();
    assert!(
        h.state().result == DialogResult::Open,
        "a prefix: OK is off"
    );

    h.state_mut().typed = "alice".into();
    h.run();
    h.get_by_label_contains("Delete").click();
    h.run();
    assert!(
        h.state().result == DialogResult::Ok,
        "the name: OK confirms"
    );
}
