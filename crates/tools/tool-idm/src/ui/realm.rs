//! Realm Settings (§4.2), read-only in phase 1: the realm's facts in
//! groups, its event settings, and Open in Admin Console for the rest.

use hs_ui::form;
use hs_ui::icons::ph;

use super::common::{Cx, Load, ask, state_note, take};
use crate::provider::{FactGroup, IdmProvider, RealmSettings};

#[derive(Default)]
pub struct RealmView {
    settings: Load<RealmSettings>,
}

impl RealmView {
    pub fn reset(&mut self) {
        self.settings = Load::Idle;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        take(cx.session, "realm.settings", &mut self.settings);
        if self.settings.is_idle() {
            let realm = cx.realm.to_string();
            ask(
                cx.session,
                "realm.settings",
                &mut self.settings,
                move |p| async move { p.realm_settings(&realm).await },
            );
        }
        let theme = cx.theme;
        egui::ScrollArea::vertical()
            .id_salt("idm-realm-settings")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                form::pane(ui, |ui| {
                    form::page_title(
                        ui,
                        theme,
                        &cx.t("idm-section-realm-settings"),
                        Some(&cx.t("idm-realm-settings-sub")),
                    );
                    if form::button(ui, theme, ph::ARROW_SQUARE_OUT, &cx.t("idm-console")).clicked()
                    {
                        cx.out.console = Some("realm-settings".into());
                    }
                    ui.add_space(form::ROW_GAP);
                    if !state_note(ui, theme, cx.s, &self.settings) {
                        return;
                    }
                    let Some(rs) = self.settings.ready() else {
                        return;
                    };
                    for (group, key) in [
                        (FactGroup::General, "idm-facts-general"),
                        (FactGroup::Login, "idm-facts-login"),
                        (FactGroup::Tokens, "idm-facts-tokens"),
                        (FactGroup::Security, "idm-facts-security"),
                    ] {
                        let rows: Vec<(String, String)> = rs
                            .facts
                            .iter()
                            .filter(|f| f.group == group)
                            .map(|f| (fact_label(cx, f.field), fact_value(cx, f.field, &f.value)))
                            .collect();
                        if rows.is_empty() {
                            continue;
                        }
                        form::section(ui, theme, &cx.t(key), |ui| {
                            form::detail_table(
                                ui,
                                theme,
                                ("idm-facts", key),
                                rows.iter().map(|(a, b)| (a.as_str(), b.as_str())),
                            );
                        });
                    }
                    form::section(ui, theme, &cx.t("idm-facts-events"), |ui| {
                        match &rs.events {
                            None => form::hint(ui, theme, &cx.t("idm-events-config-refused")),
                            Some(e) => {
                                let keep = match e.events_expiration {
                                    Some(s) if s > 0 => duration(s),
                                    _ => cx.t("idm-forever"),
                                };
                                let rows = [
                                    (cx.t("idm-fact-eventsEnabled"), yes_no(cx, e.events_enabled)),
                                    (cx.t("idm-fact-eventsExpiration"), keep),
                                    (
                                        cx.t("idm-fact-adminEventsEnabled"),
                                        yes_no(cx, e.admin_events_enabled),
                                    ),
                                    (
                                        cx.t("idm-fact-adminEventsDetailsEnabled"),
                                        yes_no(cx, e.admin_events_details_enabled),
                                    ),
                                ];
                                form::detail_table(
                                    ui,
                                    theme,
                                    "idm-facts-events",
                                    rows.iter().map(|(a, b)| (a.as_str(), b.as_str())),
                                );
                            }
                        }
                    });
                });
            });
    }
}

fn fact_label(cx: &Cx<'_>, field: &str) -> String {
    cx.t(&format!("idm-fact-{field}"))
}

fn yes_no(cx: &Cx<'_>, on: bool) -> String {
    if on { cx.t("idm-yes") } else { cx.t("idm-no") }
}

/// A fact's value in words: yes / no, a lifespan in minutes or hours.
fn fact_value(cx: &Cx<'_>, field: &str, value: &str) -> String {
    match value {
        "true" => return cx.t("idm-yes"),
        "false" => return cx.t("idm-no"),
        _ => {}
    }
    let lifespan = matches!(
        field,
        "accessTokenLifespan"
            | "ssoSessionIdleTimeout"
            | "ssoSessionMaxLifespan"
            | "offlineSessionIdleTimeout"
    );
    match value.parse::<u64>() {
        Ok(s) if lifespan => duration(s),
        _ => value.to_string(),
    }
}

/// Seconds as "5 min", "10 h", "30 d" (whole units only; otherwise
/// seconds).
pub fn duration(secs: u64) -> String {
    match secs {
        0 => "0 s".into(),
        s if s % 86_400 == 0 => format!("{} d", s / 86_400),
        s if s % 3_600 == 0 => format!("{} h", s / 3_600),
        s if s % 60 == 0 => format!("{} min", s / 60),
        s => format!("{s} s"),
    }
}

#[cfg(test)]
mod tests {
    use super::duration;

    #[test]
    fn lifespans_read_in_their_largest_whole_unit() {
        assert_eq!(duration(60), "1 min");
        assert_eq!(duration(300), "5 min");
        assert_eq!(duration(36_000), "10 h");
        assert_eq!(duration(2_592_000), "30 d");
        assert_eq!(duration(90), "90 s");
    }
}
