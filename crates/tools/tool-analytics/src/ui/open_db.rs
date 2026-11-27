//! "Open in Database": the server's SQL in a Database tab, on the saved
//! connection that is the same server when exactly one is (docs/tools/
//! analytics.md §4.6). The request carries the query and a connection id,
//! never a credential.

use hs_core::tool::{OpenRequest, ToastLevel, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;

use crate::open_db::matching;
use crate::provider::Database;

/// The button; a click opens a new Database tab holding `sql`.
pub fn button(
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    db: Option<&Database>,
    sql: &str,
) {
    let s = cx.services;
    let resp = form::button(ui, theme, ph::DATABASE, &s.t("analytics-sql-open-db"))
        .on_hover_text(s.t("analytics-sql-open-db-tip"));
    if !resp.clicked() {
        return;
    }
    let ids = db
        .map(|d| matching(d, s.connections.read().all()))
        .unwrap_or_default();
    let name = db.map(|d| d.name.as_str()).unwrap_or_default();
    match ids.len() {
        1 => {}
        0 => cx.toast(
            ToastLevel::Info,
            s.i18n.t_args("analytics-open-db-none", &[("db", name)]),
        ),
        _ => cx.toast(
            ToastLevel::Info,
            s.i18n.t_args("analytics-open-db-several", &[("db", name)]),
        ),
    }
    cx.open_new_tab(
        "db",
        Some(OpenRequest::Sql {
            connection: (ids.len() == 1).then(|| ids[0].clone()),
            sql: sql.to_string(),
        }),
    );
}
