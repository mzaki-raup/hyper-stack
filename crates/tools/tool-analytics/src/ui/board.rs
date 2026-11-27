//! A dashboard's page (docs/tools/analytics.md §4.5): its filters, tabs and
//! tiles on Metabase's 24-column grid. Each tile's question runs as a Job,
//! at most *Cards at Once* together, and only the shown tab's tiles run; a
//! tile has its own waiting, running and error states. Every read runs on
//! the runtime and lands in an inbox drained at frame start.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hs_core::Services;
use hs_core::jobs::JobId;
use hs_core::tool::{OpenRequest, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::TableState;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use super::question::{Done, chart_view, table_view};
use crate::grid::{self, Cell};
use crate::provider::{
    BiProvider, Content, DashCard, DashParam, Dashboard, Kind, Node, ParamKind, Result, Table,
};

/// The auto-refresh choices Metabase offers, in minutes (0 is off).
const REFRESH_MINUTES: [u32; 7] = [0, 1, 5, 10, 15, 30, 60];

enum Loaded<T> {
    Loading,
    Ready(T),
    Failed(String),
}

/// Where one tile stands.
enum Tile {
    Queued,
    Running(JobId),
    Done(Box<Done>),
    Failed(String),
}

#[derive(Default)]
struct Inbox {
    dash: Option<Result<Dashboard>>,
    /// A tile's answer, with the generation of the run it answers.
    tiles: Vec<(u64, String, Result<Table>)>,
    choices: Vec<(String, Vec<String>)>,
}

pub struct Board {
    node: Node,
    dash: Loaded<Arc<Dashboard>>,
    /// The filters as typed, and as the tiles last ran with them.
    filters: Vec<(String, String)>,
    applied: Vec<(String, String)>,
    /// What each text filter offers, once the server has said.
    choices: HashMap<String, Vec<String>>,
    /// The shown tab's id; `None` for a dashboard without tabs.
    tab: Option<String>,
    tiles: HashMap<String, Tile>,
    tables: HashMap<String, TableState>,
    queue: VecDeque<String>,
    /// Bumped when the tiles are rerun, so a late answer is dropped.
    generation: u64,
    refresh_minutes: u32,
    last_run: Instant,
    inbox: Arc<Mutex<Inbox>>,
}

impl Board {
    fn open(s: &Services, provider: &Arc<dyn BiProvider>, node: &Node) -> Board {
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let (p, ib, id) = (provider.clone(), inbox.clone(), node.id.clone());
        s.runtime.spawn(async move {
            let r = p.dashboard(&id).await;
            ib.lock().dash = Some(r);
        });
        Board {
            node: node.clone(),
            dash: Loaded::Loading,
            filters: Vec::new(),
            applied: Vec::new(),
            choices: HashMap::new(),
            tab: None,
            tiles: HashMap::new(),
            tables: HashMap::new(),
            queue: VecDeque::new(),
            generation: 0,
            refresh_minutes: 0,
            last_run: Instant::now(),
            inbox,
        }
    }

    /// The tiles of the shown tab that hold a question.
    fn visible<'a>(&self, d: &'a Dashboard) -> Vec<&'a DashCard> {
        d.cards
            .iter()
            .filter(|c| c.tab == self.tab)
            .collect::<Vec<_>>()
    }

    /// Queue every shown question tile that has not run yet.
    fn enqueue(&mut self) {
        let Loaded::Ready(d) = &self.dash else {
            return;
        };
        let wanted: Vec<String> = self
            .visible(d)
            .into_iter()
            .filter(|c| matches!(c.content, Content::Card { .. }))
            .map(|c| c.id.clone())
            .collect();
        for id in wanted {
            if !self.tiles.contains_key(&id) {
                self.tiles.insert(id.clone(), Tile::Queued);
                self.queue.push_back(id);
            }
        }
    }

    /// Start queued tiles while fewer than *Cards at Once* are running.
    fn pump(&mut self, s: &Services, provider: &Arc<dyn BiProvider>) {
        let Loaded::Ready(d) = &self.dash else {
            return;
        };
        let limit = s.settings.read().analytics.cards_at_once.max(1) as usize;
        let rows = s.settings.read().analytics.rows_per_run as usize;
        let mut running = self
            .tiles
            .values()
            .filter(|t| matches!(t, Tile::Running(_)))
            .count();
        while running < limit {
            let Some(id) = self.queue.pop_front() else {
                break;
            };
            let Some(card) = d.cards.iter().find(|c| c.id == id).cloned() else {
                continue;
            };
            let title = match &card.content {
                Content::Card { card, .. } => format!("Run {}", card.name),
                _ => continue,
            };
            let (p, dash, values, inbox, generate) = (
                provider.clone(),
                d.clone(),
                self.applied.clone(),
                self.inbox.clone(),
                self.generation,
            );
            let tile_id = id.clone();
            let handle = s.jobs.spawn(title, "analytics", move |h| async move {
                let token = h.cancel_token();
                tokio::select! {
                    _ = token.cancelled() => Err("Cancelled".to_string()),
                    r = p.run_dashcard(&dash, &card, &values, rows) => {
                        let outcome = r.as_ref().map(|_| ()).map_err(Clone::clone);
                        inbox.lock().tiles.push((generate, tile_id, r));
                        outcome
                    }
                }
            });
            self.tiles.insert(id, Tile::Running(handle.id()));
            running += 1;
        }
    }

    /// Stop what is running and forget what is queued.
    pub fn cancel_all(&mut self, s: &Services) {
        for t in self.tiles.values() {
            if let Tile::Running(id) = t {
                s.jobs.cancel(*id);
            }
        }
        self.queue.clear();
        self.generation += 1;
    }

    /// Run every shown tile again with the filters as applied.
    fn rerun(&mut self, s: &Services) {
        self.cancel_all(s);
        self.tiles.clear();
        self.last_run = Instant::now();
        self.enqueue();
    }

    fn drain(&mut self, s: &Services, provider: &Arc<dyn BiProvider>) {
        let (dash, tiles, choices) = {
            let mut g = self.inbox.lock();
            (
                g.dash.take(),
                std::mem::take(&mut g.tiles),
                std::mem::take(&mut g.choices),
            )
        };
        if let Some(r) = dash {
            match r {
                Ok(d) => {
                    self.filters = d
                        .params
                        .iter()
                        .map(|p| (p.id.clone(), p.default.clone()))
                        .collect();
                    self.applied = self.filters.clone();
                    self.tab = d.tabs.first().map(|t| t.id.clone());
                    // Text filters offer what the server lists.
                    for p in d.params.iter().filter(|p| p.kind == ParamKind::Text) {
                        let (pr, ib, did, pid) = (
                            provider.clone(),
                            self.inbox.clone(),
                            d.id.clone(),
                            p.id.clone(),
                        );
                        s.runtime.spawn(async move {
                            if let Ok(values) = pr.filter_values(&did, &pid).await {
                                ib.lock().choices.push((pid, values));
                            }
                        });
                    }
                    self.dash = Loaded::Ready(Arc::new(d));
                    self.last_run = Instant::now();
                    self.enqueue();
                }
                Err(e) => self.dash = Loaded::Failed(e),
            }
        }
        for (id, values) in choices {
            self.choices.insert(id, values);
        }
        for (generate, id, r) in tiles {
            if generate != self.generation {
                continue;
            }
            let Loaded::Ready(d) = &self.dash else {
                continue;
            };
            let card = d
                .cards
                .iter()
                .find(|c| c.id == id)
                .and_then(|c| match &c.content {
                    Content::Card { card, .. } => Some(card.clone()),
                    _ => None,
                });
            let state = match (r, card) {
                (Ok(table), Some(card)) => Tile::Done(Box::new(Done::new(&card, table))),
                (Err(e), _) => Tile::Failed(e),
                _ => continue,
            };
            self.tiles.insert(id, state);
        }
    }
}

/// Stop a dashboard's runs when something else is picked.
pub fn leave(slot: &mut Option<Board>, s: &Services, selected: Option<&Node>) {
    let Some(b) = slot.as_mut() else {
        return;
    };
    let still_here = selected.is_some_and(|n| n.kind == Kind::Dashboard && n.id == b.node.id);
    if !still_here {
        b.cancel_all(s);
        // Whatever was waiting must run again when the dashboard is shown.
        b.tiles
            .retain(|_, t| matches!(t, Tile::Done(_) | Tile::Failed(_)));
    }
}

/// Draw the page for the dashboard `node`. Returns the question whose
/// title was clicked, for the caller to show.
pub fn page(
    slot: &mut Option<Board>,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    provider: &Arc<dyn BiProvider>,
    node: &Node,
) -> Option<Node> {
    if !slot
        .as_ref()
        .is_some_and(|b| b.node.kind == node.kind && b.node.id == node.id)
    {
        if let Some(old) = slot.as_mut() {
            old.cancel_all(cx.services);
        }
        *slot = Some(Board::open(cx.services, provider, node));
    }
    let b = slot.as_mut()?;
    let s = cx.services;
    b.drain(s, provider);
    b.enqueue();
    b.pump(s, provider);
    let url = provider.item_url(node.kind, &node.id);
    form::page_title(
        ui,
        theme,
        &node.name,
        Some(&s.t("analytics-kind-dashboard")),
    );
    let dash = match &b.dash {
        Loaded::Loading => {
            form::centred_row(ui, |ui| {
                ui.spinner();
                ui.label(s.t("analytics-loading"));
            });
            return None;
        }
        Loaded::Failed(e) => {
            form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e));
            return None;
        }
        Loaded::Ready(d) => d.clone(),
    };
    toolbar(b, ui, cx, theme, &url);
    ui.add_space(form::ROW_GAP);
    filters(b, ui, cx, theme, &dash);
    tabs(b, ui, cx, theme, &dash);
    // Auto-refresh: only while this page is drawn.
    if b.refresh_minutes > 0 {
        let every = Duration::from_secs(u64::from(b.refresh_minutes) * 60);
        if b.last_run.elapsed() >= every {
            b.rerun(s);
        }
        ui.ctx().request_repaint_after(
            every
                .saturating_sub(b.last_run.elapsed())
                .max(Duration::from_secs(1)),
        );
    }
    grid(b, ui, cx, theme, &dash)
}

fn toolbar(b: &mut Board, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme, url: &str) {
    let s = cx.services;
    form::centred_row_wrapped(ui, |ui| {
        if form::primary_button(
            ui,
            theme,
            &format!("{} {}", ph::ARROWS_CLOCKWISE, s.t("analytics-refresh")),
        )
        .clicked()
        {
            b.rerun(s);
        }
        if form::button(
            ui,
            theme,
            ph::ARROW_SQUARE_OUT,
            &s.t("analytics-open-browser"),
        )
        .clicked()
        {
            cx.open(
                "browser",
                Some(OpenRequest::Url {
                    url: url.to_string(),
                }),
            );
        }
        if form::button(ui, theme, ph::COPY, &s.t("analytics-copy-link")).clicked() {
            ui.ctx().copy_text(url.to_string());
        }
        ui.label(s.t("analytics-auto-refresh"));
        let label = |m: u32| {
            if m == 0 {
                s.t("analytics-refresh-off")
            } else {
                s.i18n
                    .t_args("analytics-refresh-minutes", &[("n", &m.to_string())])
            }
        };
        egui::ComboBox::from_id_salt("analytics-refresh-interval")
            .selected_text(label(b.refresh_minutes))
            .show_ui(ui, |ui| {
                for m in REFRESH_MINUTES {
                    if form::selectable_value(ui, &mut b.refresh_minutes, m, label(m)).clicked() {
                        b.last_run = Instant::now();
                    }
                }
            });
    });
}

fn filters(
    b: &mut Board,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    dash: &Dashboard,
) {
    let s = cx.services;
    if dash.params.is_empty() {
        return;
    }
    let months: Vec<String> = (1..=12).map(|m| s.t(&format!("date-month-{m}"))).collect();
    let weekdays: Vec<String> = (1..=7).map(|d| s.t(&format!("date-day-{d}"))).collect();
    let labels = hs_ui::datefield::DateLabels {
        open: &s.t("date-open"),
        today: &s.t("date-today"),
        clear: &s.t("date-clear"),
        months: &months,
        weekdays: &weekdays,
    };
    let params: Vec<&DashParam> = dash.params.iter().collect();
    form::field_grid(ui, "analytics-filters", |ui| {
        for p in params {
            let Some(slot) = b.filters.iter_mut().find(|(id, _)| *id == p.id) else {
                continue;
            };
            form::label(ui, &p.name);
            match p.kind {
                ParamKind::Date => {
                    hs_ui::datefield::date_field(ui, theme, &p.id, &mut slot.1, &labels);
                }
                ParamKind::Number => {
                    form::text_field_width(
                        ui,
                        &mut slot.1,
                        &s.t("analytics-param-number"),
                        form::FIELD_MIN_WIDTH,
                    );
                }
                ParamKind::Text => {
                    let options = b.choices.get(&p.id).map(Vec::as_slice).unwrap_or_default();
                    form::pick_field_width(
                        ui,
                        theme,
                        &mut slot.1,
                        options,
                        &s.t("analytics-param-text"),
                        form::FIELD_MIN_WIDTH,
                    );
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(form::PANE_GAP);
    let changed = b.filters != b.applied;
    form::centred_row(ui, |ui| {
        if form::primary_button(
            ui,
            theme,
            &format!("{} {}", ph::CHECK, s.t("analytics-apply")),
        )
        .clicked()
        {
            b.applied = b.filters.clone();
            b.rerun(s);
        }
        if changed {
            ui.label(egui::RichText::new(s.t("analytics-filters-changed")).color(theme.text_muted));
        }
    });
    ui.add_space(form::ROW_GAP);
}

fn tabs(b: &mut Board, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme, dash: &Dashboard) {
    let _ = cx;
    if dash.tabs.len() < 2 {
        return;
    }
    let names: Vec<&str> = dash.tabs.iter().map(|t| t.name.as_str()).collect();
    let none: Vec<Option<egui::ImageSource<'static>>> = vec![None; names.len()];
    let glyphs: Vec<Option<(&str, egui::Color32)>> = vec![None; names.len()];
    let current = dash
        .tabs
        .iter()
        .position(|t| Some(&t.id) == b.tab.as_ref())
        .unwrap_or(0);
    let width = (names.len() as f32 * 150.0).min(ui.available_width());
    ui.allocate_ui(egui::vec2(width, 30.0), |ui| {
        if let Some(i) = hs_ui::tabs::segmented(ui, theme, &names, &none, &glyphs, current) {
            b.tab = Some(dash.tabs[i].id.clone());
            b.enqueue();
        }
    });
    ui.add_space(form::PANE_GAP);
}

fn grid(
    b: &mut Board,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    dash: &Dashboard,
) -> Option<Node> {
    let cards: Vec<&DashCard> = b.visible(dash);
    let cells: Vec<Cell> = cards
        .iter()
        .map(|c| Cell {
            row: c.row,
            col: c.col,
            size_x: c.size_x,
            size_y: c.size_y,
        })
        .collect();
    let mut clicked = None;
    // Each tab keeps its own scroll position.
    let salt = ("analytics-board", b.tab.clone());
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width();
            let (places, height) = grid::place(&cells, width);
            let (area, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            for p in places {
                let card = cards[p.index];
                let rect = egui::Rect::from_min_size(
                    area.min + egui::vec2(p.x, p.y),
                    egui::vec2(p.w, p.h),
                );
                let table = b.tables.entry(card.id.clone()).or_default();
                if let Some(n) = tile(ui, cx, theme, rect, card, b.tiles.get(&card.id), table) {
                    clicked = Some(n);
                }
            }
        });
    clicked
}

/// One tile: a frame, its title, and its body for the state it is in.
fn tile(
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    rect: egui::Rect,
    card: &DashCard,
    state: Option<&Tile>,
    table: &mut TableState,
) -> Option<Node> {
    let s = cx.services;
    let mut clicked = None;
    // Tiles are placed by hand, not by the cursor, so each needs its own id
    // (its charts' plots take theirs from it).
    let id = ("analytics-tile", card.id.clone());
    ui.scope_builder(egui::UiBuilder::new().id_salt(id).max_rect(rect), |ui| {
        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        egui::Frame::new()
            .fill(theme.panel_bg)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .corner_radius(6)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.set_width(rect.width() - 18.0);
                ui.set_height(rect.height() - 18.0);
                match &card.content {
                    Content::Card { card: q, .. } => {
                        let title = egui::RichText::new(&q.name).strong();
                        let r = ui
                            .add(
                                egui::Label::new(title)
                                    .sense(egui::Sense::click())
                                    .truncate(),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if r.clicked() {
                            clicked = Some(Node {
                                kind: q.kind,
                                id: q.id.clone(),
                                name: q.name.clone(),
                                description: q.description.clone(),
                                collection: String::new(),
                                edited: String::new(),
                                editor: String::new(),
                            });
                        }
                        ui.add_space(4.0);
                        match state {
                            None | Some(Tile::Queued) => {
                                waiting(ui, &s.t("analytics-tile-waiting"))
                            }
                            Some(Tile::Running(_)) => waiting(ui, &s.t("analytics-running")),
                            Some(Tile::Failed(e)) => {
                                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e));
                            }
                            Some(Tile::Done(done)) => {
                                if done.plan.is_chart() {
                                    chart_view(ui, cx, theme, done);
                                } else {
                                    table_view(ui, cx, theme, done, table);
                                }
                            }
                        }
                    }
                    Content::Text { text, heading } => {
                        if *heading {
                            ui.label(egui::RichText::new(text).size(22.0).strong());
                        } else {
                            hs_ui::ai_assistant::markdown_text(ui, text, theme);
                        }
                    }
                    Content::Link { title, url } => {
                        ui.label(egui::RichText::new(title).strong());
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
                    }
                }
            });
    });
    clicked
}

fn waiting(ui: &mut egui::Ui, text: &str) {
    form::centred_row(ui, |ui| {
        ui.spinner();
        ui.label(text);
    });
}
