//! The Identity activity's screens driven through egui_kittest (FEAT-0109):
//! the dashboard's empty state, and Settings → Identity showing only the
//! fields of the chosen sign-in kind.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_core::Services;
use hs_core::connections::IDM_KINDS;
use hs_core::paths::DataDir;
use hs_core::runtime::NoopWaker;
use hs_core::secrets::KdfParams;
use hs_core::tool::{Action, Tool, ToolContext, ToolFactory};
use hs_ui::theme::Theme;

use crate::settings_ui::{self, Page, SettingsEffect, SettingsState};

fn boot(dir: &std::path::Path) -> crate::bootstrap::Boot {
    let paths = DataDir::resolve(Some(&dir.join("data"))).unwrap();
    crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap()
}

struct Home<'s> {
    tool: Box<dyn Tool>,
    services: &'s Services,
    fonts_ready: bool,
}

#[test]
fn the_dashboard_says_there_are_no_servers_yet() {
    let dir = tempfile::tempdir().unwrap();
    let boot = boot(dir.path());
    let services = &boot.services;
    let tool = tool_idm::IdmFactory.create(services, None);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, st: &mut Home| {
                if !st.fonts_ready {
                    hs_ui::icons::install_fonts(ui.ctx());
                    st.fonts_ready = true;
                    return;
                }
                let mut actions: Vec<Action> = Vec::new();
                let mut cx = ToolContext {
                    services: st.services,
                    tab_id: 1,
                    env_id: None,
                    actions: &mut actions,
                    is_home: true,
                };
                st.tool.ui(ui, &mut cx);
            },
            Home {
                tool,
                services,
                fonts_ready: false,
            },
        );
    h.run();
    let none = services.t("idm-none");
    assert!(h.query_by_label_contains(&none).is_some(), "{none}");
    assert!(
        h.query_by_label_contains(&services.t("idm-recent-empty"))
            .is_some(),
        "Recent Changes says it is empty"
    );
}

struct Form<'s> {
    services: &'s Services,
    state: SettingsState,
    effects: Vec<SettingsEffect>,
    fonts_ready: bool,
}

/// Settings → Identity with a new server's form open, signing in as `auth`.
fn form<'s>(services: &'s Services, auth: &str) -> Harness<'s, Form<'s>> {
    let mut state = SettingsState {
        page: Page::Identity,
        ..Default::default()
    };
    state.open_kind_draft(IDM_KINDS[0]);
    let (draft, _) = state.conn_draft.as_mut().expect("a form is open");
    let mut f = tool_idm::profile::IdmFields::from_table(&draft.kind, &draft.fields);
    f.credential.auth = auth.into();
    f.write_into(&mut draft.fields);
    Harness::builder()
        .with_size(egui::vec2(1400.0, 2000.0))
        .build_ui_state(
            |ui, st: &mut Form| {
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
            Form {
                services,
                state,
                effects: Vec::new(),
                fonts_ready: false,
            },
        )
}

#[test]
fn the_identity_form_shows_only_the_chosen_sign_in_fields() {
    let dir = tempfile::tempdir().unwrap();
    let boot = boot(dir.path());
    let s = &boot.services;
    let (user, secret, token) = (
        s.t("conn-queue-user"),
        s.t("conn-idm-client-secret"),
        s.t("conn-idm-offline-token"),
    );
    for (auth, shown, hidden) in [
        ("api-key", vec![&secret], vec![&user, &token]),
        ("password", vec![&user], vec![&secret, &token]),
        ("token", vec![&token], vec![&user, &secret]),
    ] {
        let mut h = form(s, auth);
        h.run();
        for l in shown {
            assert!(h.query_by_label(l).is_some(), "{auth}: {l} is shown");
        }
        for l in hidden {
            assert!(h.query_by_label(l).is_none(), "{auth}: {l} is not");
        }
    }
}
