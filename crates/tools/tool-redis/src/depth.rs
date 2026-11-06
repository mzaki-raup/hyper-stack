//! The depth-programme views (docs/10-depth-programme.md, phase A): the
//! Info **dashboard** with sparklines, the **Tools** tab (Slowlog,
//! Client List, Big Keys, Bulk Ops), **Pub/Sub** and **Streams**. Every
//! read goes through the runner; every write is guarded like the rest
//! of the tool (read-only profiles and protected environments refuse,
//! bulk operations preview first and ask).

use hs_core::tool::ToolContext;
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;

use crate::client::KeyType;
use crate::runner::{BulkAction, Cmd, Sample};
use crate::ui::{Popup, RedisTool};

/// What the new tabs remember between frames.
pub struct DepthState {
    pub bigkeys_pattern: String,
    pub bigkeys_top: usize,
    pub bulk_pattern: String,
    pub bulk_action: BulkAction,
    pub bulk_secs: String,
    pub bulk_to: String,
    pub sub_channels: String,
    pub sub_patterns: String,
    pub pub_channel: String,
    pub pub_message: String,
    pub stream_key: String,
    pub stream_count: usize,
    pub info_raw_open: bool,
    /// Which tools section is open.
    pub tool: ToolView,
    /// The Client List and Big Keys tables' sort, filter and selection.
    pub clients_table: TableState,
    pub bigkeys_table: TableState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolView {
    Slowlog,
    Clients,
    BigKeys,
    Bulk,
}

impl Default for DepthState {
    fn default() -> Self {
        Self {
            bigkeys_pattern: "*".into(),
            bigkeys_top: 50,
            bulk_pattern: String::new(),
            bulk_action: BulkAction::Expire,
            bulk_secs: "3600".into(),
            bulk_to: String::new(),
            sub_channels: String::new(),
            sub_patterns: String::new(),
            pub_channel: String::new(),
            pub_message: String::new(),
            stream_key: String::new(),
            stream_count: 50,
            info_raw_open: false,
            tool: ToolView::Slowlog,
            clients_table: TableState::default(),
            bigkeys_table: TableState::default(),
        }
    }
}

/// How often the Info dashboard re-reads INFO while it is on screen.
const INFO_EVERY: f64 = 2.0;

fn human_bytes(b: u64) -> String {
    const K: f64 = 1024.0;
    let b = b as f64;
    if b >= K * K * K {
        format!("{:.2} GB", b / (K * K * K))
    } else if b >= K * K {
        format!("{:.1} MB", b / (K * K))
    } else if b >= K {
        format!("{:.0} KB", b / K)
    } else {
        format!("{b:.0} B")
    }
}

/// `HH:MM:SS` in the local zone, for slowlog rows and messages.
fn local_time(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

fn human_secs(s: u64) -> String {
    let (d, h, m) = (s / 86_400, (s % 86_400) / 3600, (s % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m {}s", s % 60)
    }
}

/// A small line chart of the last samples' `pick` value.
fn sparkline(ui: &mut egui::Ui, theme: &Theme, samples: &[Sample], pick: impl Fn(&Sample) -> f64) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, theme.surface);
    if samples.len() < 2 {
        return;
    }
    let vals: Vec<f64> = samples.iter().map(&pick).collect();
    let max = vals.iter().cloned().fold(f64::MIN, f64::max).max(1e-9);
    let min = vals.iter().cloned().fold(f64::MAX, f64::min).min(0.0);
    let span = (max - min).max(1e-9);
    let n = vals.len();
    let pts: Vec<egui::Pos2> = vals
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = rect.left() + 4.0 + (rect.width() - 8.0) * i as f32 / (n - 1) as f32;
            let y = rect.bottom() - 4.0 - ((v - min) / span) as f32 * (rect.height() - 8.0);
            egui::pos2(x, y)
        })
        .collect();
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.5, theme.accent)));
}

impl RedisTool {
    /// The Info tab: the figures as cards with their last five minutes,
    /// then the raw INFO sections folded away (docs/tools/redis.md §8).
    pub(crate) fn info_dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let now = ui.input(|i| i.time);
        let (samples, last) = {
            let sh = self.shared.lock();
            (
                sh.samples.iter().cloned().collect::<Vec<Sample>>(),
                sh.info_at,
            )
        };
        if now - last > INFO_EVERY && !self.shared.lock().busy {
            self.shared.lock().info_at = now;
            self.send(Cmd::RefreshInfo);
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(INFO_EVERY));
        let Some(cur) = samples.last().cloned() else {
            form::hint(ui, &theme, &s.t("redis-info-hint"));
            return false;
        };
        let hit_rate = if cur.hits + cur.misses > 0 {
            format!(
                "{:.1}%",
                100.0 * cur.hits as f64 / (cur.hits + cur.misses) as f64
            )
        } else {
            "–".into()
        };
        let frag = if cur.fragmentation > 0.0 {
            format!("{:.2}", cur.fragmentation)
        } else {
            "–".into()
        };
        let keys: u64 = self.shared.lock().keyspace.iter().map(|(_, n)| n).sum();
        hs_ui::dashboard::stats(
            ui,
            &theme,
            &[
                hs_ui::dashboard::Stat {
                    icon: ph::MEMORY,
                    value: &human_bytes(cur.used_memory),
                    label: &s.t("redis-dash-memory"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::USERS,
                    value: &cur.clients.to_string(),
                    label: &s.t("redis-dash-clients"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::LIGHTNING,
                    value: &cur.ops.to_string(),
                    label: &s.t("redis-dash-ops"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::TARGET,
                    value: &hit_rate,
                    label: &s.t("redis-dash-hit-rate"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::KEY,
                    value: &keys.to_string(),
                    label: &s.t("redis-dash-keys"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::CLOCK,
                    value: &human_secs(cur.uptime),
                    label: &s.t("redis-dash-uptime"),
                },
            ],
        );
        ui.add_space(8.0);
        let gap = ui.spacing().item_spacing.x;
        let w = (ui.available_width() - 2.0 * gap) / 3.0;
        ui.horizontal_top(|ui| {
            for (label, pick) in [
                (
                    s.t("redis-dash-ops"),
                    Box::new(|x: &Sample| x.ops as f64) as Box<dyn Fn(&Sample) -> f64>,
                ),
                (
                    s.t("redis-dash-memory"),
                    Box::new(|x: &Sample| x.used_memory as f64),
                ),
                (
                    s.t("redis-dash-clients"),
                    Box::new(|x: &Sample| x.clients as f64),
                ),
            ] {
                ui.vertical(|ui| {
                    ui.set_width(w);
                    ui.label(egui::RichText::new(label).small().color(theme.text_muted));
                    sparkline(ui, &theme, &samples, pick);
                });
            }
        });
        ui.add_space(8.0);
        egui::Grid::new("redis-dash-facts")
            .num_columns(4)
            .spacing([24.0, 4.0])
            .show(ui, |ui| {
                let muted = |t: String| egui::RichText::new(t).small().color(theme.text_muted);
                ui.label(muted(s.t("redis-dash-peak")));
                ui.label(
                    egui::RichText::new(human_bytes(cur.peak_memory))
                        .small()
                        .monospace(),
                );
                ui.label(muted(s.t("redis-dash-fragmentation")));
                ui.label(egui::RichText::new(frag).small().monospace());
                ui.end_row();
                ui.label(muted(s.t("redis-dash-role")));
                ui.label(egui::RichText::new(&cur.role).small().monospace());
                ui.label(muted(s.t("redis-dash-persistence")));
                ui.label(egui::RichText::new(&cur.persistence).small().monospace());
                ui.end_row();
                ui.label(muted(s.t("redis-dash-evicted")));
                ui.label(
                    egui::RichText::new(cur.evicted.to_string())
                        .small()
                        .monospace(),
                );
                ui.label(muted(s.t("redis-dash-expired")));
                ui.label(
                    egui::RichText::new(cur.expired.to_string())
                        .small()
                        .monospace(),
                );
                ui.end_row();
            });
        ui.add_space(8.0);
        true
    }

    /// Slowlog, Client List, Big Keys and Bulk Ops.
    pub(crate) fn tools_tab(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        ui.add_space(form::PANE_GAP);
        ui.horizontal(|ui| {
            for (v, icon, key) in [
                (
                    ToolView::Slowlog,
                    ph::CLOCK_COUNTDOWN,
                    "redis-tools-slowlog",
                ),
                (ToolView::Clients, ph::USERS_THREE, "redis-tools-clients"),
                (ToolView::BigKeys, ph::KEY, "redis-tools-bigkeys"),
                (ToolView::Bulk, ph::STACK, "redis-tools-bulk"),
            ] {
                if hs_ui::toolbar::tab(ui, &theme, self.depth.tool == v, icon, &s.t(key)).clicked()
                {
                    self.depth.tool = v;
                    match v {
                        ToolView::Slowlog => {
                            self.send(Cmd::Slowlog);
                        }
                        ToolView::Clients => {
                            self.send(Cmd::Clients);
                        }
                        _ => {}
                    }
                }
            }
        });
        ui.separator();
        ui.add_space(form::PANE_GAP);
        match self.depth.tool {
            ToolView::Slowlog => self.slowlog_view(ui, &theme, cx),
            ToolView::Clients => self.clients_view(ui, &theme, cx),
            ToolView::BigKeys => self.bigkeys_view(ui, &theme, cx),
            ToolView::Bulk => self.bulk_view(ui, &theme, cx),
        }
    }

    fn slowlog_view(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} {}", ph::ARROWS_CLOCKWISE, s.t("redis-reload")))
                .clicked()
            {
                self.send(Cmd::Slowlog);
            }
            form::hint(ui, theme, &s.t("redis-slowlog-hint"));
        });
        ui.add_space(form::PANE_GAP);
        let rows = self.shared.lock().slowlog.clone();
        if rows.is_empty() {
            form::hint(ui, theme, &s.t("redis-slowlog-none"));
            return;
        }
        egui::ScrollArea::both()
            .id_salt("redis-slowlog")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("redis-slowlog-grid")
                    .striped(true)
                    .num_columns(5)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for h in ["#", "When", "Duration", "Client", "Command"] {
                            ui.label(egui::RichText::new(h).strong().small());
                        }
                        ui.end_row();
                        for e in &rows {
                            ui.label(egui::RichText::new(e.id.to_string()).small().monospace());
                            ui.label(egui::RichText::new(local_time(e.at)).small());
                            ui.label(
                                egui::RichText::new(format!("{:.3} ms", e.micros as f64 / 1000.0))
                                    .small()
                                    .monospace(),
                            );
                            ui.label(egui::RichText::new(&e.client).small().monospace());
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&e.command).small().monospace(),
                                )
                                .truncate(),
                            );
                            ui.end_row();
                        }
                    });
            });
    }

    /// The clients as the table every activity's lists use: sortable,
    /// filterable, one row selected; Kill Client acts on that row
    /// (CR-0031, in place of a bare grid with a button per row).
    fn clients_view(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let rows = self.shared.lock().clients.clone();
        let blocked = self.read_only();
        let selected = self
            .depth
            .clients_table
            .selected
            .and_then(|i| rows.get(i))
            .map(|c| c.id.clone());
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} {}", ph::ARROWS_CLOCKWISE, s.t("redis-reload")))
                .clicked()
            {
                self.send(Cmd::Clients);
            }
            if ui
                .add_enabled(
                    !blocked && selected.is_some(),
                    egui::Button::new(format!("{} {}", ph::X_CIRCLE, s.t("redis-kill-client"))),
                )
                .clicked()
                && let Some(id) = selected.clone()
            {
                self.popup = Some(Popup::Kill { id });
            }
            form::hint(ui, theme, &s.t("redis-clients-hint"));
        });
        ui.add_space(form::PANE_GAP);
        if rows.is_empty() {
            form::hint(ui, theme, &s.t("redis-clients-none"));
            return;
        }
        let columns = [
            ColumnSpec::new(s.t("redis-col-id"), 70.0),
            ColumnSpec::new(s.t("redis-col-addr"), 160.0),
            ColumnSpec::new(s.t("redis-col-name"), 120.0),
            ColumnSpec::new(s.t("redis-col-age"), 70.0).numeric(),
            ColumnSpec::new(s.t("redis-col-idle"), 70.0).numeric(),
            ColumnSpec::new(s.t("redis-col-db"), 50.0).numeric(),
            ColumnSpec::new(s.t("redis-col-cmd"), 140.0),
        ];
        let cells: Vec<Vec<String>> = rows
            .iter()
            .map(|c| {
                vec![
                    c.id.clone(),
                    c.addr.clone(),
                    c.name.clone(),
                    c.age.clone(),
                    c.idle.clone(),
                    c.db.clone(),
                    c.cmd.clone(),
                ]
            })
            .collect();
        let r = ui
            .push_id("redis-clients", |ui| {
                DataTable {
                    theme,
                    columns: &columns,
                    rows: &cells,
                    row_height: hs_ui::table::ROW_HEIGHT,
                    show_filter: true,
                    copy_row_label: &s.t("redis-copy-row"),
                    built_in_menu: true,
                    row_icons: None,
                    empty_hint: None,
                }
                .show(ui, &mut self.depth.clients_table)
            })
            .inner;
        if let Some(i) = r.clicked {
            self.depth.clients_table.selected = Some(i);
        }
        if let Some(text) = r.copied {
            ui.ctx().copy_text(text);
        }
    }

    fn bigkeys_view(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let (running, scanned, found) = {
            let sh = self.shared.lock();
            (sh.bigkeys_running, sh.bigkeys_scanned, sh.bigkeys.clone())
        };
        hs_ui::form::centred_row(ui, |ui| {
            ui.label(s.t("redis-bigkeys-pattern"));
            ui.add(hs_ui::form::line_edit(&mut self.depth.bigkeys_pattern).desired_width(200.0));
            ui.label(s.t("redis-bigkeys-top"));
            ui.add(egui::DragValue::new(&mut self.depth.bigkeys_top).range(5..=500));
            if running {
                ui.spinner();
                ui.label(
                    s.i18n
                        .t_args("redis-bigkeys-scanning", &[("n", &scanned.to_string())]),
                );
                if ui.button(s.t("cancel")).clicked() {
                    self.shared.lock().bigkeys_stop = true;
                }
            } else if ui
                .button(format!(
                    "{} {}",
                    ph::MAGNIFYING_GLASS,
                    s.t("redis-bigkeys-run")
                ))
                .clicked()
            {
                self.send(Cmd::BigKeys {
                    pattern: self.depth.bigkeys_pattern.clone(),
                    top: self.depth.bigkeys_top,
                });
            }
        });
        form::hint(ui, theme, &s.t("redis-bigkeys-hint"));
        ui.add_space(form::PANE_GAP);
        if running {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(300));
        }
        if found.is_empty() {
            return;
        }
        // The same table as every other list; a click on a row opens
        // the key, as the hint above says (CR-0031).
        let columns = [
            ColumnSpec::new(s.t("redis-col-key"), 320.0),
            ColumnSpec::new(s.t("redis-col-size"), 90.0).numeric(),
            ColumnSpec::new(s.t("redis-col-type"), 80.0),
            ColumnSpec::new(s.t("redis-col-length"), 90.0).numeric(),
            ColumnSpec::new(s.t("redis-col-ttl"), 90.0),
        ];
        let cells: Vec<Vec<String>> = found
            .iter()
            .map(|k| {
                vec![
                    k.key.clone(),
                    human_bytes(k.bytes),
                    k.kind.clone(),
                    k.len.to_string(),
                    if k.ttl < 0 {
                        "∞".to_string()
                    } else {
                        human_secs(k.ttl as u64)
                    },
                ]
            })
            .collect();
        // Material icons on: every row is a key, so one picture serves
        // the whole list, as the Docker tables do.
        let icons: Vec<egui::ImageSource<'static>> =
            if s.settings.read().appearance.material_icons("redis") {
                hs_ui::fileicons::for_kind("key")
                    .map(|i| vec![i; cells.len()])
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
        let r = ui
            .push_id("redis-bigkeys", |ui| {
                DataTable {
                    theme,
                    columns: &columns,
                    rows: &cells,
                    row_height: hs_ui::table::ROW_HEIGHT,
                    show_filter: true,
                    copy_row_label: &s.t("redis-copy-row"),
                    built_in_menu: true,
                    row_icons: (!icons.is_empty()).then_some(icons.as_slice()),
                    empty_hint: None,
                }
                .show(ui, &mut self.depth.bigkeys_table)
            })
            .inner;
        if let Some(text) = r.copied {
            ui.ctx().copy_text(text);
        }
        if let Some(i) = r.clicked
            && let Some(k) = found.get(i)
        {
            self.depth.bigkeys_table.selected = Some(i);
            let key = k.key.clone();
            self.open_key(&key);
        }
    }

    fn bulk_view(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        form::hint(ui, theme, &s.t("redis-bulk-hint"));
        ui.add_space(form::PANE_GAP);
        ui.horizontal(|ui| {
            ui.label(s.t("redis-bulk-action"));
            for (a, icon, key) in [
                (BulkAction::Delete, ph::TRASH, "redis-bulk-delete"),
                (BulkAction::Expire, ph::CLOCK_COUNTDOWN, "redis-bulk-expire"),
                (BulkAction::Persist, ph::INFINITY, "redis-bulk-persist"),
                (
                    BulkAction::RenamePrefix,
                    ph::PENCIL_SIMPLE,
                    "redis-bulk-rename",
                ),
            ] {
                if hs_ui::toolbar::tab(ui, theme, self.depth.bulk_action == a, icon, &s.t(key))
                    .clicked()
                {
                    self.depth.bulk_action = a;
                }
            }
        });
        ui.add_space(form::PANE_GAP);
        hs_ui::form::centred_row(ui, |ui| {
            ui.label(s.t("redis-bulk-pattern"));
            ui.add(
                hs_ui::form::line_edit(&mut self.depth.bulk_pattern)
                    .hint_text("session:*")
                    .desired_width(240.0),
            );
            match self.depth.bulk_action {
                BulkAction::Expire => {
                    ui.label(s.t("redis-bulk-seconds"));
                    ui.add(hs_ui::form::line_edit(&mut self.depth.bulk_secs).desired_width(90.0));
                }
                BulkAction::RenamePrefix => {
                    ui.label(s.t("redis-bulk-to"));
                    ui.add(
                        hs_ui::form::line_edit(&mut self.depth.bulk_to)
                            .hint_text("archive:session:")
                            .desired_width(200.0),
                    );
                }
                _ => {}
            }
            let can = !self.depth.bulk_pattern.trim().is_empty() && !self.read_only();
            if ui
                .add_enabled(
                    can,
                    egui::Button::new(format!("{} {}", ph::EYE, s.t("redis-bulk-preview"))),
                )
                .clicked()
            {
                self.send(Cmd::BulkPreview {
                    pattern: self.depth.bulk_pattern.trim().to_string(),
                });
            }
        });
        ui.add_space(form::PANE_GAP);
        if self.read_only() {
            form::hint(ui, theme, &s.t("redis-refused-protected"));
        }
        let preview = self.shared.lock().bulk.clone();
        let Some(pv) = preview else {
            return;
        };
        if pv.pattern != self.depth.bulk_pattern.trim() {
            return;
        }
        ui.add_space(form::PANE_GAP);
        ui.label(
            egui::RichText::new(
                s.i18n
                    .t_args("redis-bulk-found", &[("n", &pv.keys.len().to_string())]),
            )
            .strong(),
        );
        ui.add_space(4.0);
        // The preview in a well, like every other scrolling list.
        form::scroll_well(ui, theme, "redis-bulk-preview", Some(220.0), |ui| {
            for k in pv.keys.iter().take(200) {
                ui.label(egui::RichText::new(k).small().monospace());
            }
        });
        ui.add_space(form::PANE_GAP);
        if !pv.keys.is_empty()
            && !pv.capped
            && ui
                .add(
                    egui::Button::new(format!("{} {}", ph::PLAY, s.t("redis-bulk-apply")))
                        .fill(theme.error),
                )
                .clicked()
        {
            let arg = match self.depth.bulk_action {
                BulkAction::Expire => self.depth.bulk_secs.trim().to_string(),
                BulkAction::RenamePrefix => self.depth.bulk_to.trim().to_string(),
                _ => String::new(),
            };
            self.popup = Some(Popup::Bulk {
                action: self.depth.bulk_action,
                keys: pv.keys.clone(),
                pattern: pv.pattern.clone(),
                arg,
                typed: String::new(),
            });
        }
        if pv.capped {
            form::hint(ui, theme, &s.t("redis-bulk-capped"));
        }
    }

    /// Subscribe to channels and patterns, publish, read what arrives.
    pub(crate) fn pubsub_tab(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let on = self.shared.lock().pubsub_on;
        ui.add_space(form::PANE_GAP);
        hs_ui::form::centred_row(ui, |ui| {
            ui.label(s.t("redis-pubsub-channels"));
            ui.add_enabled(
                !on,
                hs_ui::form::line_edit(&mut self.depth.sub_channels)
                    .hint_text("events, orders")
                    .desired_width(220.0),
            );
            ui.label(s.t("redis-pubsub-patterns"));
            ui.add_enabled(
                !on,
                hs_ui::form::line_edit(&mut self.depth.sub_patterns)
                    .hint_text("log:*")
                    .desired_width(160.0),
            );
            if on {
                if ui
                    .button(format!("{} {}", ph::STOP, s.t("redis-pubsub-unsubscribe")))
                    .clicked()
                {
                    self.send(Cmd::Unsubscribe);
                }
            } else {
                let can = !self.depth.sub_channels.trim().is_empty()
                    || !self.depth.sub_patterns.trim().is_empty();
                if ui
                    .add_enabled(
                        can,
                        egui::Button::new(format!(
                            "{} {}",
                            ph::PLAY,
                            s.t("redis-pubsub-subscribe")
                        )),
                    )
                    .clicked()
                {
                    let split = |t: &str| -> Vec<String> {
                        t.split(',')
                            .map(|x| x.trim().to_string())
                            .filter(|x| !x.is_empty())
                            .collect()
                    };
                    self.send(Cmd::Subscribe {
                        channels: split(&self.depth.sub_channels),
                        patterns: split(&self.depth.sub_patterns),
                    });
                }
            }
            if hs_ui::toolbar::btn(ui, &theme, ph::ERASER, &s.t("redis-pubsub-clear")).clicked() {
                self.shared.lock().pubsub_msgs.clear();
            }
        });
        ui.add_space(form::PANE_GAP);
        hs_ui::form::centred_row(ui, |ui| {
            ui.label(s.t("redis-pubsub-publish"));
            ui.add(
                hs_ui::form::line_edit(&mut self.depth.pub_channel)
                    .hint_text("channel")
                    .desired_width(160.0),
            );
            ui.add(
                hs_ui::form::line_edit(&mut self.depth.pub_message)
                    .hint_text("message")
                    .desired_width(320.0),
            );
            let can = !self.depth.pub_channel.trim().is_empty() && !self.read_only();
            if ui
                .add_enabled(
                    can,
                    egui::Button::new(format!(
                        "{} {}",
                        ph::PAPER_PLANE_TILT,
                        s.t("redis-pubsub-send")
                    )),
                )
                .clicked()
            {
                self.send(Cmd::Publish {
                    channel: self.depth.pub_channel.trim().to_string(),
                    message: self.depth.pub_message.clone(),
                });
            }
        });
        form::hint(ui, &theme, &s.t("redis-pubsub-hint"));
        ui.separator();
        ui.add_space(form::PANE_GAP);
        if on {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        let msgs = self.shared.lock().pubsub_msgs.clone();
        if msgs.is_empty() {
            form::hint(ui, &theme, &s.t("redis-pubsub-none"));
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("redis-pubsub-msgs")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for m in &msgs {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(local_time(m.at))
                                .small()
                                .color(theme.text_muted),
                        );
                        ui.label(
                            egui::RichText::new(&m.channel)
                                .small()
                                .strong()
                                .color(theme.accent),
                        );
                        ui.add(
                            egui::Label::new(egui::RichText::new(&m.payload).small().monospace())
                                .wrap(),
                        );
                    });
                }
            });
    }

    /// A stream's groups, consumers and entries (XINFO, XRANGE, XPENDING).
    pub(crate) fn streams_tab(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let streams: Vec<String> = self.shared.lock().tree.keys_with_kind(&KeyType::Stream);
        ui.add_space(form::PANE_GAP);
        hs_ui::form::centred_row(ui, |ui| {
            ui.label(s.t("redis-streams-key"));
            let shown = if self.depth.stream_key.is_empty() {
                s.t("redis-streams-pick")
            } else {
                self.depth.stream_key.clone()
            };
            let mut picked: Option<String> = None;
            egui::ComboBox::from_id_salt("redis-stream-key")
                .selected_text(shown)
                .width(320.0)
                .show_ui(ui, |ui| {
                    for k in &streams {
                        if ui
                            .selectable_label(self.depth.stream_key == *k, k)
                            .clicked()
                        {
                            picked = Some(k.clone());
                        }
                    }
                });
            ui.add(
                hs_ui::form::line_edit(&mut self.depth.stream_key)
                    .hint_text(s.t("redis-streams-typed"))
                    .desired_width(220.0),
            );
            ui.label(s.t("redis-streams-last"));
            ui.add(egui::DragValue::new(&mut self.depth.stream_count).range(10..=1000));
            if let Some(k) = picked {
                self.depth.stream_key = k.clone();
                self.send(Cmd::StreamInfo {
                    key: k,
                    count: self.depth.stream_count,
                });
            }
            let can = !self.depth.stream_key.trim().is_empty();
            if ui
                .add_enabled(
                    can,
                    egui::Button::new(format!("{} {}", ph::ARROWS_CLOCKWISE, s.t("redis-reload"))),
                )
                .clicked()
            {
                self.send(Cmd::StreamInfo {
                    key: self.depth.stream_key.trim().to_string(),
                    count: self.depth.stream_count,
                });
            }
        });
        form::hint(ui, &theme, &s.t("redis-streams-hint"));
        let view = self.shared.lock().stream.clone();
        let Some(v) = view else {
            return;
        };
        ui.separator();
        hs_ui::dashboard::stats(
            ui,
            &theme,
            &[
                hs_ui::dashboard::Stat {
                    icon: ph::LIST_NUMBERS,
                    value: &v.length.to_string(),
                    label: &s.t("redis-streams-length"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::USERS_THREE,
                    value: &v.groups.len().to_string(),
                    label: &s.t("redis-streams-groups"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::CLOCK,
                    value: &v.last_id,
                    label: &s.t("redis-streams-last-id"),
                },
            ],
        );
        ui.add_space(6.0);
        if !v.groups.is_empty() {
            egui::Grid::new("redis-stream-groups")
                .striped(true)
                .num_columns(4)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    for h in ["Group", "Consumers", "Pending", "Last delivered"] {
                        ui.label(egui::RichText::new(h).strong().small());
                    }
                    ui.end_row();
                    for g in &v.groups {
                        ui.label(egui::RichText::new(&g.name).small().monospace());
                        ui.label(egui::RichText::new(g.consumers.to_string()).small());
                        ui.label(egui::RichText::new(g.pending.to_string()).small());
                        ui.label(egui::RichText::new(&g.last_delivered).small().monospace());
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
        }
        egui::ScrollArea::both()
            .id_salt("redis-stream-entries")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("redis-stream-entries-grid")
                    .striped(true)
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for (id, fields) in &v.entries {
                            ui.label(
                                egui::RichText::new(id)
                                    .small()
                                    .monospace()
                                    .color(theme.text_muted),
                            );
                            let text = fields
                                .iter()
                                .map(|(k, val)| format!("{k}={val}"))
                                .collect::<Vec<_>>()
                                .join("  ");
                            ui.add(
                                egui::Label::new(egui::RichText::new(text).small().monospace())
                                    .wrap(),
                            );
                            ui.end_row();
                        }
                    });
            });
    }
}
