//! A Tableau view's page (docs/tools/analytics.md §4.8): its picture and its
//! data, with the filter fields the view takes as `vf_<field>=<value>`, saves
//! as PNG, CSV or PDF, and Open Interactive, which leads to the Browser: the
//! interactive viz is Tableau's own to draw. Every read is a job that can be
//! cancelled; a late answer to a cancelled or replaced run is dropped.

use std::sync::Arc;

use hs_core::jobs::JobId;
use hs_core::tool::{OpenRequest, ToastLevel, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::TableState;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use super::question::{Done, table_view};
use crate::provider::{BiProvider, Card, Kind, Node, Result, Table, ViewFile, ViewInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Picture,
    Data,
}

/// A read that ends in a value, a failure, or nothing yet.
enum Load<T> {
    Idle,
    Running(JobId),
    Ready(T),
    Failed(String),
}

#[derive(Default)]
struct Inbox {
    info: Option<Result<ViewInfo>>,
    /// Answers carry the number of the run they answer.
    image: Option<(u64, Result<Vec<u8>>)>,
    data: Option<(u64, Result<Table>)>,
    save: Option<(ViewFile, Result<Vec<u8>>)>,
}

pub struct ViewPage {
    node: Node,
    info: Option<Result<ViewInfo>>,
    /// What is typed: `Region=East; Segment=Retail`.
    filters: String,
    tab: Tab,
    run_no: u64,
    /// The picture's width in pixels, fixed when the page opens.
    width: u32,
    image: Load<(String, Arc<[u8]>)>,
    data: Load<Done>,
    table: TableState,
    saving: bool,
    inbox: Arc<Mutex<Inbox>>,
}

/// `Region=East; Segment=Retail` as `(field, value)` pairs; a part without
/// an `=` or with nothing on either side is left out.
pub fn parse_filters(text: &str) -> Vec<(String, String)> {
    text.split([';', '\n'])
        .filter_map(|part| {
            let (f, v) = part.split_once('=')?;
            let (f, v) = (f.trim(), v.trim());
            (!f.is_empty() && !v.is_empty()).then(|| (f.to_string(), v.to_string()))
        })
        .collect()
}

/// A stand-in card, so the data's table is made ready the way a question's is.
fn card_for(node: &Node) -> Card {
    Card {
        id: node.id.clone(),
        kind: Kind::Question,
        name: node.name.clone(),
        description: String::new(),
        display: "table".into(),
        native_sql: None,
        database: String::new(),
        params: Vec::new(),
        viz: serde_json::Value::Null,
    }
}

/// A file name from a view's name: no path characters.
fn file_stem(name: &str) -> String {
    let t: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let t = t.trim().to_string();
    if t.is_empty() { "view".into() } else { t }
}

impl ViewPage {
    fn open(
        s: &hs_core::Services,
        provider: &Arc<dyn BiProvider>,
        node: &Node,
        width: u32,
    ) -> Self {
        let page = ViewPage {
            node: node.clone(),
            info: None,
            filters: String::new(),
            tab: Tab::Picture,
            run_no: 0,
            width,
            image: Load::Idle,
            data: Load::Idle,
            table: TableState::default(),
            saving: false,
            inbox: Arc::new(Mutex::new(Inbox::default())),
        };
        let (p, id, inbox) = (provider.clone(), node.id.clone(), page.inbox.clone());
        s.runtime.spawn(async move {
            let r = p.view(&id).await;
            inbox.lock().info = Some(r);
        });
        page
    }

    /// Read the picture and the data again with the filters as typed.
    fn run(&mut self, s: &hs_core::Services, provider: &Arc<dyn BiProvider>) {
        self.cancel(s);
        let no = self.run_no;
        let values = parse_filters(&self.filters);
        let limit = s.settings.read().analytics.rows_per_run as usize;
        let (p, id, inbox, width, v) = (
            provider.clone(),
            self.node.id.clone(),
            self.inbox.clone(),
            self.width,
            values.clone(),
        );
        let picture = s.jobs.spawn(
            format!("Picture of {}", self.node.name),
            "analytics",
            move |h| async move {
                let token = h.cancel_token();
                tokio::select! {
                    _ = token.cancelled() => Err("Cancelled".to_string()),
                    r = p.view_file(&id, ViewFile::Image, width, &v) => {
                        let outcome = r.as_ref().map(|_| ()).map_err(Clone::clone);
                        inbox.lock().image = Some((no, r));
                        outcome
                    }
                }
            },
        );
        let (p, id, inbox) = (provider.clone(), self.node.id.clone(), self.inbox.clone());
        if provider.capabilities().view_data {
            let data = s.jobs.spawn(
                format!("Data of {}", self.node.name),
                "analytics",
                move |h| async move {
                    let token = h.cancel_token();
                    tokio::select! {
                        _ = token.cancelled() => Err("Cancelled".to_string()),
                        r = p.view_data(&id, &values, limit) => {
                            let outcome = r.as_ref().map(|_| ()).map_err(Clone::clone);
                            inbox.lock().data = Some((no, r));
                            outcome
                        }
                    }
                },
            );
            self.data = Load::Running(data.id());
        }
        self.image = Load::Running(picture.id());
    }

    fn cancel(&mut self, s: &hs_core::Services) {
        if let Load::Running(id) = self.image {
            s.jobs.cancel(id);
            self.image = Load::Idle;
        }
        if let Load::Running(id) = self.data {
            s.jobs.cancel(id);
            self.data = Load::Idle;
        }
        // A late answer to the cancelled run is dropped.
        self.run_no += 1;
    }

    fn busy(&self) -> bool {
        matches!(self.image, Load::Running(_)) || matches!(self.data, Load::Running(_))
    }

    fn save(&mut self, s: &hs_core::Services, provider: &Arc<dyn BiProvider>, file: ViewFile) {
        self.saving = true;
        let (p, id, inbox, width, values) = (
            provider.clone(),
            self.node.id.clone(),
            self.inbox.clone(),
            self.width,
            parse_filters(&self.filters),
        );
        s.runtime.spawn(async move {
            let r = p.view_file(&id, file, width, &values).await;
            inbox.lock().save = Some((file, r));
        });
    }

    fn drain(&mut self, cx: &mut ToolContext, provider: &Arc<dyn BiProvider>) {
        let (info, image, data, save) = {
            let mut g = self.inbox.lock();
            (g.info.take(), g.image.take(), g.data.take(), g.save.take())
        };
        if let Some(r) = info {
            self.info = Some(r);
            self.run(cx.services, provider);
        }
        if let Some((no, r)) = image
            && no == self.run_no
        {
            self.image = match r {
                Ok(bytes) => Load::Ready((
                    format!("bytes://analytics/view/{}/{no}.png", self.node.id),
                    bytes.into(),
                )),
                Err(e) => Load::Failed(e),
            };
        }
        if let Some((no, r)) = data
            && no == self.run_no
        {
            self.table = TableState::default();
            self.data = match r {
                Ok(t) => Load::Ready(Done::new(&card_for(&self.node), t)),
                Err(e) => Load::Failed(e),
            };
        }
        if let Some((file, r)) = save {
            self.saving = false;
            let name = format!("{}.{}", file_stem(&self.node.name), file.ext());
            match r {
                Ok(bytes) => match hs_core::download::save_bytes(&name, &bytes) {
                    Ok(Some(path)) => cx.toast(
                        ToastLevel::Info,
                        cx.services.i18n.t_args(
                            "analytics-exported",
                            &[("path", &path.display().to_string())],
                        ),
                    ),
                    Ok(None) => {}
                    Err(e) => cx.toast(ToastLevel::Error, e),
                },
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
    }
}

/// Draw the page for the view `node`, opening it first if it is not shown.
pub fn page(
    slot: &mut Option<ViewPage>,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    provider: &Arc<dyn BiProvider>,
    node: &Node,
) {
    if !slot.as_ref().is_some_and(|v| v.node.id == node.id) {
        let width = (ui.available_width() * ui.ctx().pixels_per_point()).clamp(300.0, 2000.0);
        *slot = Some(ViewPage::open(cx.services, provider, node, width as u32));
    }
    let Some(v) = slot.as_mut() else {
        return;
    };
    v.drain(cx, provider);
    let s = cx.services;
    let url = provider.item_url(node.kind, &node.id);
    let caps = provider.capabilities();
    form::page_title(
        ui,
        theme,
        &node.name,
        Some(&s.t(&format!("analytics-kind-{}", node.kind.tag()))),
    );
    match &v.info {
        None => {
            form::centred_row(ui, |ui| {
                ui.spinner();
                ui.label(s.t("analytics-loading"));
            });
            return;
        }
        Some(Err(e)) => {
            form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e));
            return;
        }
        Some(Ok(info)) => {
            let line = [
                info.workbook.as_str(),
                info.project.as_str(),
                info.updated.as_str(),
            ]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
            if !line.is_empty() {
                form::hint(ui, theme, &line);
            }
        }
    }
    ui.add_space(form::PANE_GAP);
    form::centred_row_wrapped(ui, |ui| {
        if v.busy() {
            if form::button(ui, theme, ph::X, &s.t("analytics-cancel")).clicked() {
                v.cancel(s);
            }
            ui.spinner();
        } else if form::primary_button(
            ui,
            theme,
            &format!("{} {}", ph::PLAY, s.t("analytics-view-apply")),
        )
        .on_hover_text(s.t("analytics-view-apply-tip"))
        .clicked()
        {
            v.run(s, provider);
        }
        if form::button(
            ui,
            theme,
            ph::ARROW_SQUARE_OUT,
            &s.t("analytics-view-interactive"),
        )
        .on_hover_text(s.t("analytics-view-interactive-tip"))
        .clicked()
        {
            cx.open("browser", Some(OpenRequest::Url { url: url.clone() }));
        }
        if form::button(ui, theme, ph::COPY, &s.t("analytics-copy-link")).clicked() {
            ui.ctx().copy_text(url.clone());
        }
    });
    ui.add_space(form::ROW_GAP);
    if caps.view_filters {
        form::centred_row_wrapped(ui, |ui| {
            ui.label(s.t("analytics-view-filters"));
            let w = ui
                .available_width()
                .clamp(form::FIELD_MIN_WIDTH, form::FIELD_MAX_WIDTH);
            let r =
                form::text_field_width(ui, &mut v.filters, &s.t("analytics-view-filters-hint"), w);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !v.busy() {
                v.run(s, provider);
            }
        });
        ui.add_space(form::ROW_GAP);
    }
    form::centred_row_wrapped(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        for (file, icon, key) in [
            (ViewFile::Image, ph::IMAGE, "analytics-view-save-picture"),
            (ViewFile::Csv, ph::FILE_CSV, "analytics-view-save-data"),
            (ViewFile::Pdf, ph::FILE_PDF, "analytics-view-save-pdf"),
        ]
        .into_iter()
        .filter(|(f, ..)| caps.view_files.contains(f))
        {
            ui.add_enabled_ui(!v.saving, |ui| {
                if form::button(ui, theme, icon, &s.t(key)).clicked() {
                    v.save(s, provider, file);
                }
            });
        }
    });
    ui.add_space(form::PANE_GAP);
    let labels = [s.t("analytics-view-picture"), s.t("analytics-view-data")];
    let names: Vec<&str> = labels.iter().map(String::as_str).collect();
    let none: [Option<egui::ImageSource<'static>>; 2] = [None, None];
    let glyphs = [
        Some((ph::IMAGE, theme.text_muted)),
        Some((ph::TABLE, theme.text_muted)),
    ];
    let current = usize::from(v.tab == Tab::Data);
    if caps.view_data {
        ui.allocate_ui(egui::vec2(ui.available_width().min(320.0), 30.0), |ui| {
            if let Some(i) = hs_ui::tabs::segmented(ui, theme, &names, &none, &glyphs, current) {
                v.tab = [Tab::Picture, Tab::Data][i];
            }
        });
        ui.add_space(form::PANE_GAP);
    } else {
        v.tab = Tab::Picture;
    }
    match v.tab {
        Tab::Picture => match &v.image {
            Load::Running(_) | Load::Idle => {
                form::centred_row(ui, |ui| {
                    ui.spinner();
                    ui.label(s.t("analytics-running"));
                });
            }
            Load::Failed(e) => form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e)),
            Load::Ready((uri, bytes)) => {
                egui::ScrollArea::both()
                    .id_salt("analytics-view-picture")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Image::from_bytes(
                                uri.clone(),
                                egui::load::Bytes::from(bytes.clone()),
                            )
                            .max_width(ui.available_width())
                            .maintain_aspect_ratio(true),
                        );
                    });
            }
        },
        Tab::Data => match &v.data {
            Load::Running(_) | Load::Idle => {
                form::centred_row(ui, |ui| {
                    ui.spinner();
                    ui.label(s.t("analytics-running"));
                });
            }
            Load::Failed(e) => form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e)),
            Load::Ready(done) => {
                let status = s.i18n.t_args(
                    "analytics-result-status",
                    &[
                        ("rows", &done.table.rows.len().to_string()),
                        ("ms", &done.table.millis.to_string()),
                    ],
                );
                ui.label(egui::RichText::new(status).color(theme.text_muted));
                if done.table.truncated {
                    ui.label(
                        egui::RichText::new(s.i18n.t_args(
                            "analytics-truncated",
                            &[("rows", &done.table.rows.len().to_string())],
                        ))
                        .color(theme.warning),
                    );
                }
                ui.add_space(form::PANE_GAP);
                table_view(ui, cx, theme, done, &mut v.table);
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_typed_as_field_equals_value_separated_by_semicolons() {
        assert_eq!(
            parse_filters("Region=East; Segment = Retail\nYear=2026"),
            [
                ("Region".to_string(), "East".to_string()),
                ("Segment".to_string(), "Retail".to_string()),
                ("Year".to_string(), "2026".to_string()),
            ]
        );
        assert!(parse_filters("").is_empty());
        assert!(parse_filters("Region; =East; Year=").is_empty());
        assert_eq!(
            parse_filters("Note=a=b"),
            [("Note".to_string(), "a=b".to_string())],
            "the first = splits"
        );
    }

    #[test]
    fn a_file_name_has_no_path_characters() {
        assert_eq!(file_stem("By Month"), "By Month");
        assert_eq!(file_stem("a/b:c"), "a_b_c");
        assert_eq!(file_stem("  "), "view");
    }
}
