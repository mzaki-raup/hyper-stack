//! One server's tab (§3.3): the header band is the shell's; this is the
//! toolbar, the browse column and the item page.

use hs_core::tool::{Action, CommandDescriptor, OpenRequest, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

use super::AnalyticsTool;
use crate::provider::Support;
use crate::session::Status;

/// The tab's body.
pub fn body(tool: &mut AnalyticsTool, ui: &mut egui::Ui, cx: &mut ToolContext) {
    let s = cx.services;
    let theme = Theme::from_settings(&s.settings.read().appearance);
    if tool.state.conn_id.is_none() {
        form::pane(ui, |ui| {
            form::note(ui, &theme, ph::PLUGS_CONNECTED, &s.t("analytics-pick-hint"))
        });
        return;
    }
    if let Some(e) = tool.conn_error.clone() {
        form::pane(ui, |ui| {
            form::note(ui, &theme, ph::WARNING, &hs_core::fmt::sentence(&e));
            ui.add_space(form::ROW_GAP);
            if form::button(ui, &theme, ph::ARROWS_CLOCKWISE, &s.t("analytics-retry")).clicked() {
                tool.connect(s);
            }
        });
        return;
    }
    let Some(status) = tool.session.as_ref().map(|x| x.status()) else {
        return;
    };
    match status {
        Status::Connecting => {
            form::pane(ui, |ui| {
                form::centred_row(ui, |ui| {
                    ui.spinner();
                    ui.label(
                        s.i18n
                            .t_args("analytics-connecting", &[("server", &tool.address)]),
                    );
                });
            });
            return;
        }
        Status::Failed(e) => {
            form::pane(ui, |ui| {
                form::note(ui, &theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                ui.add_space(form::ROW_GAP);
                form::centred_row(ui, |ui| {
                    if form::button(ui, &theme, ph::ARROWS_CLOCKWISE, &s.t("analytics-retry"))
                        .clicked()
                    {
                        tool.connect(s);
                    }
                    if form::button(
                        ui,
                        &theme,
                        ph::PENCIL_SIMPLE,
                        &s.t("analytics-edit-connection"),
                    )
                    .clicked()
                        && let Some(id) = &tool.state.conn_id
                    {
                        cx.actions.push(Action::RunCommand {
                            id: format!("settings:connection:{id}"),
                        });
                    }
                });
            });
            return;
        }
        Status::Ready => {}
    }
    let Some(provider) = tool.session.as_ref().map(|x| x.provider().clone()) else {
        return;
    };
    // The browse column (§4.2, §3.3): the collection tree and Search; the
    // databases arrive with FEAT-0130 and are drawn disabled until then.
    if tool.state.browse {
        egui::Panel::left(ui.id().with("analytics-browse"))
            .resizable(true)
            .default_size(280.0)
            .size_range(220.0..=520.0)
            .show(ui, |ui| {
                form::pane(ui, |ui| {
                    form::subsection(ui, &theme, &s.t("analytics-browse"));
                    ui.add_space(form::PANE_GAP);
                    if provider.capabilities().browse {
                        tool.browse.show(ui, s, &theme, &provider);
                    }
                    ui.add_space(form::ROW_GAP);
                    ui.add_enabled_ui(false, |ui| {
                        form::selectable_label(
                            ui,
                            false,
                            format!("{}  {}", ph::DATABASE, s.t("analytics-section-databases")),
                        )
                    })
                    .inner
                    .tip_disabled(s.t("analytics-section-soon"));
                });
            });
    }
    let Some(info) = tool.session.as_ref().map(|x| x.info()) else {
        return;
    };
    form::pane(ui, |ui| match tool.browse.selected.clone() {
        Some(node) => item_page(ui, cx, &theme, &provider, &node),
        None => server_facts(ui, s, &theme, tool, &info),
    });
}

/// The server's facts, shown while no item is picked.
fn server_facts(
    ui: &mut egui::Ui,
    s: &hs_core::Services,
    theme: &Theme,
    tool: &AnalyticsTool,
    info: &crate::provider::ServerInfo,
) {
    form::subsection(ui, theme, &s.t("analytics-facts"));
    ui.add_space(form::PANE_GAP);
    let support = match info.support {
        Support::Supported => s.t("analytics-support-ok"),
        Support::TooOld => s.t("analytics-support-old"),
        Support::Newer => s.t("analytics-support-newer"),
        Support::Unknown => s.t("analytics-support-unknown"),
    };
    let facts = [
        (s.t("analytics-fact-product"), info.product.clone()),
        (s.t("analytics-fact-version"), info.version_text.clone()),
        (s.t("analytics-fact-signed-in"), info.signed_in_as.clone()),
        (s.t("analytics-fact-support"), support),
        (s.t("analytics-fact-url"), tool.url.clone()),
    ];
    form::detail_table(
        ui,
        theme,
        "analytics-facts",
        facts.iter().map(|(k, v)| (k.as_str(), v.as_str())),
    );
}

/// The picked item (§4.7): its facts, Open in Browser and Copy Link. The
/// question, dashboard and collection pages arrive with FEAT-0128 to
/// FEAT-0129; until then every kind shows this.
fn item_page(
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    provider: &std::sync::Arc<dyn crate::provider::BiProvider>,
    node: &crate::provider::Node,
) {
    let s = cx.services;
    let url = provider.item_url(node.kind, &node.id);
    form::page_title(
        ui,
        theme,
        &node.name,
        Some(&s.t(&format!("analytics-kind-{}", node.kind.tag()))),
    );
    form::centred_row_wrapped(ui, |ui| {
        if form::button(
            ui,
            theme,
            ph::ARROW_SQUARE_OUT,
            &s.t("analytics-open-browser"),
        )
        .clicked()
        {
            cx.open("browser", Some(OpenRequest::Url { url: url.clone() }));
        }
        if form::button(ui, theme, ph::COPY, &s.t("analytics-copy-link")).clicked() {
            ui.ctx().copy_text(url.clone());
        }
    });
    ui.add_space(form::ROW_GAP);
    let mut facts = vec![
        (
            s.t("analytics-fact-kind"),
            s.t(&format!("analytics-kind-{}", node.kind.tag())),
        ),
        (s.t("analytics-fact-id"), node.id.clone()),
    ];
    if !node.collection.is_empty() {
        facts.push((s.t("analytics-fact-collection"), node.collection.clone()));
    }
    if !node.editor.is_empty() {
        facts.push((s.t("analytics-fact-edited-by"), node.editor.clone()));
    }
    if !node.edited.is_empty() {
        facts.push((s.t("analytics-fact-edited"), node.edited.clone()));
    }
    if !node.description.is_empty() {
        facts.push((s.t("analytics-fact-description"), node.description.clone()));
    }
    form::detail_table(
        ui,
        theme,
        "analytics-item-facts",
        facts.iter().map(|(k, v)| (k.as_str(), v.as_str())),
    );
}

/// The review-only assistant (ADR-019): told about the server, never able
/// to write anything.
pub fn assistant_column(
    tool: &mut AnalyticsTool,
    outer: &mut egui::Ui,
    panel: Option<egui::Rect>,
    cx: &mut ToolContext,
) {
    if !tool.assistant.open {
        return;
    }
    let s = cx.services;
    let theme = Theme::from_settings(&s.settings.read().appearance);
    let info = tool.session.as_ref().map(|x| x.info()).unwrap_or_default();
    let context = format!(
        "BI server: {} ({}), {} {}",
        tool.conn_name,
        hs_core::connections::kind_label(&tool.kind),
        info.product,
        info.version_text,
    );
    tool.assistant.set_context("Analytics", context);
    let Some(panel) = panel else {
        return;
    };
    let mut child = outer.new_child(
        egui::UiBuilder::new()
            .max_rect(panel)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(panel.intersect(outer.clip_rect()));
    egui::Frame::new()
        .fill(theme.panel_bg)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .show(&mut child, |ui| {
            ui.set_min_width(370.0);
            // Review-only: nothing it writes is inserted anywhere.
            tool.assistant.can_insert = false;
            tool.assistant.ui(ui, s, &theme, "Analytics");
        });
}

/// The main toolbar (§3.3): Browse first, Refresh, Open in Browser.
pub fn toolbar(tool: &mut AnalyticsTool, ui: &mut egui::Ui, cx: &mut ToolContext) {
    let s = cx.services;
    let theme = Theme::from_settings(&s.settings.read().appearance);
    let ready = tool
        .session
        .as_ref()
        .is_some_and(|x| x.status() == Status::Ready);
    hs_ui::toolbar::toggle(
        ui,
        &theme,
        &mut tool.state.browse,
        ph::SIDEBAR_SIMPLE,
        &s.t("analytics-browse"),
    )
    .tip(s.t("analytics-browse-tip"));
    let was_searching = tool.browse.searching;
    hs_ui::toolbar::toggle(
        ui,
        &theme,
        &mut tool.browse.searching,
        ph::MAGNIFYING_GLASS,
        &s.t("analytics-search"),
    )
    .tip(s.t("analytics-search-tip"));
    if !was_searching && tool.browse.searching {
        tool.state.browse = true;
        tool.browse.open_search();
    }
    if hs_ui::toolbar::btn_enabled(
        ui,
        &theme,
        ready || tool.conn_error.is_some(),
        ph::ARROWS_CLOCKWISE,
        &s.t("analytics-refresh"),
    )
    .tip(s.t("analytics-refresh-tip"))
    .clicked()
    {
        run_command(tool, "analytics.refresh", cx);
    }
    if hs_ui::toolbar::btn_enabled(
        ui,
        &theme,
        !tool.url.is_empty(),
        ph::ARROW_SQUARE_OUT,
        &s.t("analytics-open-browser"),
    )
    .tip(s.t("analytics-open-browser-tip"))
    .clicked()
    {
        run_command(tool, "analytics.browser", cx);
    }
}

pub fn commands() -> Vec<CommandDescriptor> {
    let c = |id: &str, title: &str, key: Option<&str>| CommandDescriptor {
        id: id.into(),
        title: title.into(),
        shortcut_action: key.map(String::from),
    };
    vec![
        c("analytics.refresh", "Analytics: Refresh", Some("run.alt")),
        c(
            "analytics.search",
            "Analytics: Search…",
            Some("analytics.search"),
        ),
        c(
            "analytics.browse",
            "Analytics: Show or Hide Browse",
            Some("analytics.browse"),
        ),
        c("analytics.browser", "Analytics: Open in Browser", None),
        c("analytics.add", "Analytics: Add BI Server", None),
        c("analytics.ai", "Analytics: AI Assistant", None),
    ]
}

pub fn run_command(tool: &mut AnalyticsTool, id: &str, cx: &mut ToolContext) -> bool {
    match id {
        "analytics.refresh" | "run.alt" => {
            tool.connect(cx.services);
            true
        }
        "analytics.search" => {
            tool.state.browse = true;
            tool.browse.open_search();
            true
        }
        "analytics.browse" => {
            tool.state.browse = !tool.state.browse;
            true
        }
        "analytics.browser" => {
            if !tool.url.is_empty() {
                let url = tool
                    .session
                    .as_ref()
                    .map(|x| x.provider().web_url())
                    .filter(|u| !u.is_empty())
                    .unwrap_or_else(|| tool.url.clone());
                cx.open("browser", Some(OpenRequest::Url { url }));
            }
            true
        }
        "analytics.add" => {
            cx.actions.push(Action::RunCommand {
                id: "settings:analytics:new".into(),
            });
            true
        }
        "analytics.ai" => {
            tool.assistant.toggle("Analytics");
            true
        }
        _ => false,
    }
}
