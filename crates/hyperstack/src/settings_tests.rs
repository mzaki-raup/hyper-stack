//! Settings driven through egui_kittest: the Add jumps' focus and the Save
//! that opens a new profile in its activity (FEAT-0093).

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_core::Services;
use hs_core::connections::ConnectionProfile;
use hs_core::paths::DataDir;
use hs_core::runtime::NoopWaker;
use hs_core::secrets::KdfParams;
use hs_ui::theme::Theme;

use crate::settings_ui::{self, Page, SettingsEffect, SettingsState};

struct View<'s> {
    services: &'s Services,
    state: SettingsState,
    effects: Vec<SettingsEffect>,
    fonts_ready: bool,
}

fn boot(dir: &std::path::Path) -> crate::bootstrap::Boot {
    let paths = DataDir::resolve(Some(&dir.join("data"))).unwrap();
    crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap()
}

/// Tall enough that a connection form's Save row is on screen: a click
/// lands where the button is drawn, and off screen it lands on nothing.
fn harness<'s>(services: &'s Services, state: SettingsState) -> Harness<'s, View<'s>> {
    Harness::builder()
        .with_size(egui::vec2(1200.0, 2400.0))
        .build_ui_state(
            |ui, st: &mut View| {
                if !st.fonts_ready {
                    hs_ui::icons::install_fonts(ui.ctx());
                    st.fonts_ready = true;
                    return;
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    settings_ui::page(
                        ui,
                        &Theme::dark(),
                        st.services,
                        &mut st.state,
                        &mut st.effects,
                    );
                });
            },
            View {
                services,
                state,
                effects: Vec::new(),
                fonts_ready: false,
            },
        )
}

/// What an activity's Add leaves behind: the page for the kind, an empty
/// form of that kind, and the focus flag (as `settings:connection:new:`).
fn jump(kind: &str) -> SettingsState {
    let c = ConnectionProfile {
        name: "new".into(),
        kind: kind.into(),
        ..Default::default()
    };
    SettingsState {
        page: settings_ui::page_for_kind(kind),
        conn_selected: Some(c.id.clone()),
        conn_draft: Some((c, String::new())),
        focus_form: true,
        ..Default::default()
    }
}

#[test]
fn every_kind_jumps_to_the_page_that_holds_it() {
    for (kind, page) in [
        ("ssh", Page::Connections),
        ("db", Page::Connections),
        ("s3", Page::Connections),
        ("redis", Page::Connections),
        ("mongodb", Page::Connections),
        ("docker", Page::Containers),
        ("k8s", Page::Containers),
        ("rabbitmq", Page::Queue),
        ("kafka", Page::Queue),
        ("jenkins", Page::Automation),
        ("github-actions", Page::Automation),
        ("jira", Page::Tracker),
        ("redmine", Page::Tracker),
    ] {
        assert!(settings_ui::page_for_kind(kind) == page, "{kind}");
    }
}

#[test]
fn an_add_jump_focuses_the_empty_form() {
    let dir = tempfile::tempdir().unwrap();
    let boot = boot(dir.path());
    let mut h = harness(&boot.services, jump("redis"));
    h.run();
    assert!(!h.state().state.focus_form, "the form took the focus");
    assert!(
        h.ctx.memory(|m| m.focused().is_some()),
        "the name field has the keyboard"
    );
}

#[test]
fn a_page_whose_form_waits_for_a_kind_takes_the_focus_on_its_list() {
    let dir = tempfile::tempdir().unwrap();
    let boot = boot(dir.path());
    for page in [
        Page::Containers,
        Page::Queue,
        Page::Automation,
        Page::Tracker,
    ] {
        let state = SettingsState {
            page,
            focus_form: true,
            ..Default::default()
        };
        let mut h = harness(&boot.services, state);
        h.run();
        assert!(!h.state().state.focus_form, "{page:?} list took the focus");
        assert!(
            h.state().state.conn_draft.is_none(),
            "{page:?} opened no form"
        );
    }
}

#[test]
fn saving_a_new_profile_closes_the_form_and_opens_it_in_its_activity() {
    let dir = tempfile::tempdir().unwrap();
    let boot = boot(dir.path());
    let state = jump("redis");
    let id = state.conn_selected.clone().unwrap();
    let mut h = harness(&boot.services, state);
    h.run();
    h.get_by_label("Save").click();
    h.run();
    let st = h.state();
    assert!(st.state.conn_draft.is_none(), "the form closed");
    assert!(st.state.conn_selected.is_none());
    assert!(boot.services.connections.read().by_id(&id).is_some());
    assert!(
        st.effects.iter().any(|e| matches!(
            e,
            SettingsEffect::OpenConnection { tool, id: opened } if tool == "redis" && *opened == id
        )),
        "Save asked to open the new profile in Redis"
    );
}

#[test]
fn saving_an_existing_profile_keeps_the_form_and_opens_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let boot = boot(dir.path());
    let c = ConnectionProfile {
        name: "cache".into(),
        kind: "redis".into(),
        ..Default::default()
    };
    boot.services.connections.write().upsert(c.clone()).unwrap();
    let state = SettingsState {
        page: Page::Connections,
        conn_selected: Some(c.id.clone()),
        conn_draft: Some((c, String::new())),
        ..Default::default()
    };
    let mut h = harness(&boot.services, state);
    h.run();
    h.get_by_label("Save").click();
    h.run();
    let st = h.state();
    assert!(
        st.effects
            .iter()
            .any(|e| matches!(e, SettingsEffect::ConnectionsChanged)),
        "the edit was saved"
    );
    assert!(st.state.conn_draft.is_some(), "an edit stays on the form");
    assert!(
        !st.effects
            .iter()
            .any(|e| matches!(e, SettingsEffect::OpenConnection { .. }))
    );
}

#[test]
fn close_closes_the_connection_form() {
    let dir = tempfile::tempdir().unwrap();
    let boot = boot(dir.path());
    let mut h = harness(&boot.services, jump("ssh"));
    h.run();
    h.get_by_label("Close").click();
    h.run();
    assert!(h.state().state.conn_draft.is_none());
    assert!(h.state().state.conn_selected.is_none());
}
