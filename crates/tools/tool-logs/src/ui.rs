//! The tab (docs/tools/logs.md §3): a sidebar of saved sources and filter
//! presets, a header with the source and its follow state, a row of filter
//! chips, the virtualised record list, a status line and a record detail
//! pane. Everything the list shows comes from [`crate::engine::Shared`].

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, ToastLevel, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::engine::{self, Runner, Shared, Status};
use crate::filter::{Chip, ChipKind, Filter, Highlight, Highlights, Preset};
use crate::parse::{Format, Level, Record};
use crate::source::{SavedSource, SourceSpec};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "logs",
    title_key: "tool-logs",
    icon: ph::SCROLL,
    category: ToolCategory::Infra,
    accepts: &[Accept::LocalFile, Accept::Text],
};

/// The MIME a source spec travels under between tabs (sidebar → new tab).
pub const SPEC_MIME: &str = "application/x-hyperstack-log-source+json";

pub struct LogsFactory;

impl ToolFactory for LogsFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let mut t = LogsTool::new(services, state);
        if t.state.spec.is_some() {
            t.start(services);
        }
        Box::new(t)
    }
}

/// What the workspace restores. Never a secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub spec: Option<SourceSpec>,
    pub format: Format,
    pub tail_lines: u32,
    pub chips: Vec<Chip>,
    pub follow: bool,
    /// Columns for structured formats; empty = time, level, message.
    pub pinned: Vec<String>,
    /// Pasted text, for a text source.
    pub text: Option<String>,
    pub highlights: Vec<Highlight>,
    /// (line number, note).
    pub bookmarks: Vec<(u64, String)>,
    pub local_time: bool,
    pub detail_open: bool,
    /// Mask secrets on the way out of the tab (docs/tools/logs.md §15.4).
    /// On by default, and what is on screen matches what would leave.
    #[serde(default = "yes")]
    pub mask: bool,
    /// Fold consecutive repeats into one row with a count (§15.5). Off by
    /// default: a log read in order is what people expect to see first.
    #[serde(default)]
    pub collapse: bool,
}

fn yes() -> bool {
    true
}

impl Default for State {
    fn default() -> Self {
        Self {
            spec: None,
            format: Format::Auto,
            tail_lines: 2000,
            chips: Vec::new(),
            follow: true,
            pinned: Vec::new(),
            text: None,
            highlights: default_highlights(),
            bookmarks: Vec::new(),
            local_time: true,
            detail_open: true,
            mask: true,
            collapse: false,
        }
    }
}

fn default_highlights() -> Vec<Highlight> {
    vec![Highlight {
        pattern: r"duration:\s*\d{3,}(?:\.\d+)?\s*ms".into(),
        colour: [0xfb, 0x92, 0x3c],
        bold: true,
        enabled: true,
    }]
}

/// The Add Source form, which doubles as Edit Source: the fields are the
/// same ones, and `editing` carries the name the source had so the old
/// file can be removed when it is renamed (owner's request, 2026-10-23).
#[derive(Default)]
struct AddSource {
    kind: usize,
    name: String,
    path: String,
    conn_id: String,
    command: String,
    format: Format,
    tail_lines: u32,
    pasted: String,
    save: bool,
    /// The saved source being edited, by the name it had.
    editing: Option<String>,
    /// Its place in the list, kept across an edit.
    order: u32,
}

impl AddSource {
    /// The form filled in from a running service, with its log already
    /// pointed at: a file when the server writes one here, otherwise the
    /// command that follows it (docs/tools/logs.md §16).
    fn from_service(svc: &crate::services::Service, order: u32) -> Self {
        let mut a = Self {
            name: svc.name.clone(),
            format: Format::Auto,
            tail_lines: 2000,
            save: true,
            order,
            ..Default::default()
        };
        match (svc.files.first(), svc.command.as_deref()) {
            (Some(path), _) => {
                a.kind = 0;
                a.path = path.clone();
            }
            (None, Some(command)) => {
                a.kind = 2;
                a.command = command.to_string();
            }
            // Nothing found: the form opens on the file picker so the
            // user can say where it writes.
            (None, None) => a.kind = 0,
        }
        a
    }

    /// The form filled in from a saved source.
    fn editing(src: &SavedSource) -> Self {
        let (kind, path, conn_id, command) = match &src.spec {
            SourceSpec::LocalFile { path } => (0, path.clone(), String::new(), String::new()),
            SourceSpec::SshFile { conn_id, path } => {
                (1, path.clone(), conn_id.clone(), String::new())
            }
            SourceSpec::Command { conn_id, command } => (
                2,
                String::new(),
                conn_id.clone().unwrap_or_default(),
                command.clone(),
            ),
            SourceSpec::Text { .. } => (3, String::new(), String::new(), String::new()),
        };
        Self {
            kind,
            name: src.name.clone(),
            path,
            conn_id,
            command,
            format: src.format,
            tail_lines: src.tail_lines,
            pasted: String::new(),
            save: true,
            editing: Some(src.name.clone()),
            order: src.order,
        }
    }
}

struct NewChip {
    kind: usize,
    text: String,
    field: String,
    level: Level,
    negate: bool,
}

impl Default for NewChip {
    fn default() -> Self {
        Self {
            kind: 0,
            text: String::new(),
            field: String::new(),
            level: Level::Warn,
            negate: false,
        }
    }
}

#[derive(Default)]
struct FindState {
    open: bool,
    text: String,
    hits: Vec<usize>,
    at: usize,
    for_version: u64,
    for_text: String,
}

/// `path:line` tokens in a record, for "open in the editor".
static PATH_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"((?:[A-Za-z]:[\\/]|/|\./|[\w.-]+/)?[\w./\\-]+\.(?:rs|py|js|ts|tsx|jsx|go|java|kt|cs|rb|php|c|cc|cpp|h|hpp|sql|toml|yaml|yml|json))"?(?::|\(|, line )(\d+)"#).unwrap()
});

pub struct LogsTool {
    state: State,
    shared: Arc<Mutex<Shared>>,
    runner: Option<Runner>,
    filter: Filter,
    highlights: Highlights,
    /// Indices into `Shared::records` that pass the chips.
    filtered: Vec<usize>,
    filtered_upto: usize,
    filtered_dropped: u64,
    chips_version: u64,
    chips_seen: u64,
    selected: Option<usize>,
    find: FindState,
    new_chip: Option<NewChip>,
    jump: Option<String>,
    add: Option<AddSource>,
    saved: Vec<SavedSource>,
    presets: Vec<Preset>,
    preset_name: Option<String>,
    paused: bool,
    /// Records that arrived while the view was not at the bottom.
    unseen: usize,
    seen_len: usize,
    at_bottom: bool,
    scroll_to: Option<usize>,
    conn_name: String,
    env_id: Option<String>,
    is_home: bool,
    logs_dir: PathBuf,
    /// Compiled once per change, never per record.
    redactor: crate::redact::Redactor,
    /// What is running here, and the scan that finds it. The scan is a
    /// Job: it shells out to docker, systemctl and kubectl, which must
    /// never happen on the UI thread.
    services: Vec<crate::services::Service>,
    services_scan: Option<std::sync::mpsc::Receiver<Vec<crate::services::Service>>>,
    services_scanned: bool,
    /// Review-only assistant (ADR-019, docs/tools/logs.md §15.1). It is
    /// told the selected record and its neighbours, after redaction.
    assistant: hs_ui::ai_assistant::AssistantPanel,
    /// The rows the list draws: one per record, or one per folded run
    /// (§15.5). Rebuilt when the filtered set or the toggle changes.
    rows: Vec<crate::shape::Row>,
    /// How far `rows` was built, so appends extend rather than rebuild.
    rows_upto: usize,
    rows_collapse: bool,
    /// Runs the user has opened; keyed by the run's first position.
    expanded: std::collections::HashSet<usize>,
    sidebar_filter: String,
    confirm_delete: Option<String>,
    status_text: String,
}

impl LogsTool {
    fn new(services: &Services, state: State) -> Self {
        let logs_dir = services.workspace.read().root().join("logs");
        let filter = Filter::compile(&state.chips);
        let highlights = Highlights::compile(&state.highlights);
        let mut t = Self {
            state,
            shared: Arc::new(Mutex::new(Shared::default())),
            runner: None,
            filter,
            highlights,
            filtered: Vec::new(),
            filtered_upto: 0,
            filtered_dropped: 0,
            chips_version: 0,
            chips_seen: 0,
            selected: None,
            find: FindState::default(),
            new_chip: None,
            jump: None,
            add: None,
            saved: SavedSource::load_all(&logs_dir),
            presets: load_presets(&logs_dir),
            preset_name: None,
            paused: false,
            unseen: 0,
            seen_len: 0,
            at_bottom: true,
            scroll_to: None,
            conn_name: String::new(),
            env_id: None,
            is_home: false,
            redactor: crate::redact::Redactor::compile(&crate::redact::RuleSet::load(&logs_dir)),
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
            services: Vec::new(),
            services_scan: None,
            services_scanned: false,
            rows: Vec::new(),
            rows_upto: 0,
            rows_collapse: false,
            expanded: std::collections::HashSet::new(),
            logs_dir,
            sidebar_filter: String::new(),
            confirm_delete: None,
            status_text: String::new(),
        };
        t.bind_connection(services);
        t
    }

    /// The connection a remote source uses: its name and environment.
    fn bind_connection(&mut self, services: &Services) {
        self.conn_name.clear();
        self.env_id = None;
        if let Some(id) = self.state.spec.as_ref().and_then(|s| s.conn_id())
            && let Some(c) = services.connections.read().by_id(id)
        {
            self.conn_name = c.name.clone();
            self.env_id = c.environment.clone();
        }
    }

    fn start(&mut self, services: &Services) {
        let Some(spec) = self.state.spec.clone() else {
            return;
        };
        self.bind_connection(services);
        self.runner = None;
        self.filtered.clear();
        self.filtered_upto = 0;
        self.filtered_dropped = 0;
        self.rows.clear();
        self.rows_upto = 0;
        self.expanded.clear();
        self.selected = None;
        self.unseen = 0;
        self.seen_len = 0;
        self.runner = Some(engine::start(
            services,
            &spec,
            self.state.format,
            self.state.tail_lines,
            self.state.text.clone(),
            self.shared.clone(),
        ));
    }

    fn open_spec(
        &mut self,
        services: &Services,
        spec: SourceSpec,
        format: Format,
        tail: u32,
        text: Option<String>,
    ) {
        self.state.spec = Some(spec);
        self.state.format = format;
        self.state.tail_lines = tail;
        self.state.text = text;
        self.start(services);
    }

    /// Follow on jumps to the tail (as the new-lines badge does); off
    /// leaves the list where it is.
    fn toggle_follow(&mut self) {
        self.state.follow = !self.state.follow;
        if self.state.follow && !self.filtered.is_empty() {
            self.scroll_to = Some(self.filtered.len() - 1);
            self.unseen = 0;
        }
    }

    fn chips_changed(&mut self) {
        self.filter = Filter::compile(&self.state.chips);
        self.chips_version += 1;
    }

    /// Keep `filtered` in step with the records: incremental for appends,
    /// from scratch after a chip change or a window drop.
    fn refilter(&mut self) {
        let sh = self.shared.lock();
        let from_scratch = self.chips_seen != self.chips_version
            || self.filtered_dropped != sh.dropped
            || self.filtered_upto > sh.records.len();
        if from_scratch {
            self.filtered.clear();
            self.filtered_upto = 0;
            self.rows.clear();
            self.rows_upto = 0;
            self.expanded.clear();
            self.chips_seen = self.chips_version;
            self.filtered_dropped = sh.dropped;
            self.selected = None;
        }
        if self.filter.is_empty() {
            self.filtered.extend(self.filtered_upto..sh.records.len());
        } else {
            for (i, r) in sh.records.iter().enumerate().skip(self.filtered_upto) {
                if self.filter.matches(r) {
                    self.filtered.push(i);
                }
            }
        }
        self.filtered_upto = sh.records.len();
        // The first lines of a followed source land at the tail, as
        // `tail -F` does; `stick_to_bottom` alone only holds a list that is
        // already there.
        if self.seen_len == 0 && !self.filtered.is_empty() && self.state.follow && !self.paused {
            self.scroll_to = Some(self.filtered.len() - 1);
        }
        if self.filtered.len() > self.seen_len {
            if !self.at_bottom || self.paused {
                self.unseen += self.filtered.len() - self.seen_len;
            }
            self.seen_len = self.filtered.len();
        } else {
            self.seen_len = self.filtered.len();
        }
        drop(sh);
        self.rebuild_rows();
    }

    /// The rows the list draws. With Collapse off this is one row per
    /// filtered record and costs nothing; with it on, consecutive records
    /// sharing a level and a shape key fold into one (docs/tools/logs.md
    /// §15.5).
    ///
    /// Folding is a view concern only: `self.filtered` is untouched, so
    /// filters, find, bookmarks and every export still see every record.
    fn rebuild_rows(&mut self) {
        if !self.state.collapse {
            // One row per record. Extend rather than rebuild, because a
            // followed source appends on most frames.
            if self.rows_collapse {
                self.rows.clear();
                self.rows_upto = 0;
                self.rows_collapse = false;
            }
            if self.rows_upto < self.filtered.len() {
                self.rows
                    .extend((self.rows_upto..self.filtered.len()).map(crate::shape::Row::One));
                self.rows_upto = self.filtered.len();
            }
            return;
        }
        // Collapsed: the last run can grow as records arrive, so the rows
        // are rebuilt whenever the filtered set changed at all. The cost is
        // a hash per filtered record, which the shape cache below absorbs.
        if self.rows_collapse && self.rows_upto == self.filtered.len() {
            return;
        }
        self.rows_collapse = true;
        self.rows_upto = self.filtered.len();
        let sh = self.shared.lock();
        let filtered = &self.filtered;
        let rows = crate::shape::fold(filtered.len(), MIN_RUN, |pos| {
            let r = sh.records.get(*filtered.get(pos)?)?;
            let body = if r.message.is_empty() {
                r.raw.as_str()
            } else {
                r.message.as_str()
            };
            Some(format!(
                "{}\u{1}{}",
                r.level.map(|l| l.label()).unwrap_or(""),
                crate::shape::key(body)
            ))
        });
        drop(sh);
        self.rows = rows;
    }

    fn find_hits(&mut self) {
        let version = self.shared.lock().version;
        if self.find.for_version == version && self.find.for_text == self.find.text {
            return;
        }
        self.find.for_version = version;
        self.find.for_text = self.find.text.clone();
        self.find.hits.clear();
        let needle = self.find.text.to_lowercase();
        if needle.is_empty() {
            return;
        }
        let sh = self.shared.lock();
        for (pos, &i) in self.filtered.iter().enumerate() {
            if sh.records[i].raw.to_lowercase().contains(&needle) {
                self.find.hits.push(pos);
            }
        }
        self.find.at = self.find.at.min(self.find.hits.len().saturating_sub(1));
    }

    /// Put `name` where `before` is, and renumber. The order lives on
    /// each source's own file, so a move rewrites the ones that shifted.
    fn reorder_saved(&mut self, name: &str, before: Option<&str>, cx: &mut ToolContext) {
        let Some(from) = self.saved.iter().position(|s| s.name == name) else {
            return;
        };
        let to = match before {
            Some(b) => self
                .saved
                .iter()
                .position(|s| s.name == b)
                .unwrap_or(self.saved.len()),
            None => self.saved.len(),
        };
        let mut list = std::mem::take(&mut self.saved);
        hs_ui::reorder::move_item(&mut list, from, to);
        for (i, src) in list.iter_mut().enumerate() {
            let want = i as u32;
            if src.order != want {
                src.order = want;
                if let Err(e) = src.save(&self.logs_dir) {
                    cx.toast(ToastLevel::Error, e.to_string());
                }
            }
        }
        self.saved = list;
    }

    /// What is running on this machine, so a log can be opened without
    /// knowing where it lives (docs/tools/logs.md §16). A service that has
    /// already been saved drops off the list: it belongs to Saved Logs
    /// now (owner's request, 2026-10-23).
    fn running_services(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut ToolContext,
        theme: &Theme,
        height: f32,
    ) {
        let s = cx.services.clone();
        // Collect a finished scan.
        if let Some(rx) = &self.services_scan
            && let Ok(found) = rx.try_recv()
        {
            self.services = found;
            self.services_scan = None;
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(s.t("logs-running-services"))
                    .small()
                    .strong()
                    .color(theme.text_muted),
            );
            hs_ui::toolbar::hard_right(ui, |ui| {
                let busy = self.services_scan.is_some();
                if ui
                    .add_enabled(!busy, egui::Button::new(ph::ARROWS_CLOCKWISE).small())
                    .tip(s.t("logs-rescan-services"))
                    .clicked()
                {
                    self.scan_services();
                }
            });
        });
        // The first look happens once the tab is drawn, not at start: a
        // workspace that never opens Logs pays nothing for it.
        if !self.services_scanned {
            self.services_scanned = true;
            self.scan_services();
        }
        // Anything already saved is not offered again.
        let saved_keys: Vec<String> = self
            .saved
            .iter()
            .map(|x| match &x.spec {
                SourceSpec::LocalFile { path } => path.clone(),
                SourceSpec::Command { command, .. } => command.clone(),
                other => other.uri(),
            })
            .collect();
        let offered: Vec<crate::services::Service> = self
            .services
            .iter()
            .filter(|svc| {
                let mine = svc.key();
                !saved_keys.iter().any(|k| {
                    k == &mine
                        || svc.command.as_deref().is_some_and(|c| k == c)
                        || svc.files.iter().any(|f| k == f)
                })
            })
            .cloned()
            .collect();
        let mut configure: Option<crate::services::Service> = None;
        egui::ScrollArea::vertical()
            .id_salt("logs-services")
            .auto_shrink([false, false])
            .max_height(height)
            .show(ui, |ui| {
                if self.services_scan.is_some() && self.services.is_empty() {
                    form::hint(ui, theme, &s.t("logs-services-scanning"));
                } else if offered.is_empty() {
                    form::hint(ui, theme, &s.t("logs-no-services"));
                }
                for svc in &offered {
                    let r = form::list_row(
                        ui,
                        theme,
                        false,
                        service_icon(svc),
                        &svc.name,
                        Some(&svc.describe()),
                    );
                    if r.tip(s.t("logs-configure-service")).clicked() {
                        configure = Some(svc.clone());
                    }
                    ui.add_space(2.0);
                }
            });
        if let Some(svc) = configure {
            self.add = Some(AddSource::from_service(&svc, self.saved.len() as u32));
        }
    }

    /// Start a scan on the runtime. The result arrives through a channel
    /// the next frame reads; the UI thread never waits.
    fn scan_services(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.services_scan = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(crate::services::detect());
        });
    }

    fn save_source(&mut self, name: &str) -> Result<(), String> {
        let Some(spec) = self.state.spec.clone() else {
            return Err("no source".into());
        };
        let s = SavedSource {
            name: name.to_string(),
            spec,
            format: self.state.format,
            tail_lines: self.state.tail_lines,
            order: self.saved.len() as u32,
            ..Default::default()
        };
        s.save(&self.logs_dir).map_err(|e| e.to_string())?;
        self.saved = SavedSource::load_all(&self.logs_dir);
        Ok(())
    }

    fn save_filtered(&self, services: &Services) -> Result<PathBuf, String> {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("filtered.log")
            .add_filter("Log", &["log", "txt"])
            .save_file()
        else {
            return Err(services.t("logs-save-cancelled"));
        };
        let sh = self.shared.lock();
        let mut out = String::new();
        for &i in &self.filtered {
            out.push_str(&self.leaving(&sh.records[i].raw));
            out.push('\n');
        }
        std::fs::write(&path, out).map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// Text on its way out of the tab: the clipboard, a file, a provider.
    /// Everything that leaves goes through here, so adding an export that
    /// does not mask takes a deliberate detour (docs/tools/logs.md §15.4).
    /// Masking is skipped only when the user has turned **Mask Secrets**
    /// off, which also stops the masking on screen, so what is shown and
    /// what leaves can never disagree.
    fn leaving(&self, text: &str) -> String {
        if self.state.mask {
            self.redactor.apply(text)
        } else {
            text.to_string()
        }
    }
}

fn presets_file(dir: &std::path::Path) -> PathBuf {
    dir.join("presets.toml")
}

#[derive(Default, Serialize, Deserialize)]
struct PresetsFile {
    presets: Vec<Preset>,
}

fn load_presets(dir: &std::path::Path) -> Vec<Preset> {
    hs_core::storage::read_toml::<PresetsFile>(&presets_file(dir))
        .map(|f| f.presets)
        .unwrap_or_default()
}

fn save_presets(dir: &std::path::Path, presets: &[Preset]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    hs_core::storage::write_toml(
        &presets_file(dir),
        &PresetsFile {
            presets: presets.to_vec(),
        },
    )
    .map_err(|e| e.to_string())
}

/// Where a row's message starts: 8 px of margin, then the line number,
/// the time and the level columns (62 + 96 + 52). Pinned columns push it
/// further right and are added on top. Keep in step with `list`.
const TEXT_COLUMN_X: f32 = 8.0 + 62.0 + 96.0 + 52.0;

/// A running service's glyph: what kind of host it was found on, since
/// that is what decides how its log is read.
fn service_icon(svc: &crate::services::Service) -> &'static str {
    use crate::services::Host;
    match svc.host {
        Host::Local => ph::GEAR,
        Host::Container => ph::SHIPPING_CONTAINER,
        Host::Systemd => ph::TREE_STRUCTURE,
        Host::Wsl => ph::LINUX_LOGO,
        Host::Kubernetes => ph::POLYGON,
    }
}

fn level_colour(theme: &Theme, level: Level) -> egui::Color32 {
    match level {
        Level::Trace => theme.text_muted,
        Level::Debug => theme.text_muted,
        Level::Info => theme.accent,
        Level::Warn => theme.warning,
        Level::Error | Level::Fatal => theme.error,
    }
}

fn status_label(s: &Services, st: &Status) -> String {
    match st {
        Status::Idle => s.t("logs-status-idle"),
        Status::Connecting => s.t("logs-status-connecting"),
        Status::Reading => s.t("logs-status-reading"),
        Status::Following => s.t("logs-status-following"),
        Status::Done => s.t("logs-status-done"),
        Status::Failed(e) => e.clone(),
    }
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// A record's time as the list shows it.
fn time_text(r: &Record, local: bool) -> String {
    match r.ts {
        Some(ts) if local => ts
            .with_timezone(&chrono::Local)
            .format("%H:%M:%S%.3f")
            .to_string(),
        Some(ts) => ts.format("%H:%M:%S%.3f").to_string(),
        None => String::new(),
    }
}

/// One line of a record for the list: the first line, cut at 4 k chars,
/// masked when Mask Secrets is on so the list shows what would leave the
/// tab (docs/tools/logs.md §15.4). Masking runs on the one line that is
/// drawn, not on the whole record, so a long stack trace costs nothing.
/// How many consecutive repeats it takes before a run is worth folding.
/// Two is not a pattern; three reads as one.
const MIN_RUN: usize = 3;

fn one_line(r: &Record, red: Option<&crate::redact::Redactor>) -> String {
    let first = r.raw.lines().next().unwrap_or("");
    let mut s: String = first.chars().take(4000).collect();
    if let Some(red) = red {
        s = red.apply(&s);
    }
    let more = r.raw.lines().count().saturating_sub(1);
    if more > 0 {
        s.push_str(&format!("  ⏎ +{more}"));
    }
    s
}

impl Tool for LogsTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        if self.is_home {
            return Cow::Borrowed("Logs");
        }
        match &self.state.spec {
            Some(spec) => Cow::Owned(spec.label()),
            None => Cow::Borrowed("Logs"),
        }
    }

    fn subtitle(&self) -> Option<String> {
        self.state.spec.as_ref().map(|s| {
            if self.conn_name.is_empty() {
                s.describe()
            } else {
                format!("{} · {}", self.conn_name, s.describe())
            }
        })
    }

    fn environment(&self) -> Option<String> {
        self.env_id.clone()
    }

    fn current_uri(&self) -> Option<String> {
        if self.is_home {
            return None;
        }
        self.state.spec.as_ref().map(SourceSpec::uri)
    }

    fn uri_affinity(&self, _uri: &str) -> Option<u8> {
        // An empty tab takes any source; a tab with one leaves it to a
        // new tab. The dashboard takes nothing: a home tab that answers
        // here swallows every request for the activity.
        (!self.is_home && self.state.spec.is_none()).then_some(1)
    }

    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    fn set_home_tab(&mut self, home: bool, _services: &Services) {
        self.is_home = home;
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or(serde_json::Value::Null)
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::File { uri, .. } => {
                let path = uri.strip_prefix("file://").unwrap_or(&uri).to_string();
                self.open_spec(
                    cx.services,
                    SourceSpec::LocalFile { path },
                    Format::Auto,
                    2000,
                    None,
                );
                true
            }
            OpenRequest::Text { mime, body } if mime == SPEC_MIME => {
                match serde_json::from_str::<SavedSource>(&body) {
                    Ok(s) => {
                        self.open_spec(cx.services, s.spec, s.format, s.tail_lines, None);
                        true
                    }
                    Err(_) => false,
                }
            }
            OpenRequest::Text { body, .. } => {
                let title = body
                    .lines()
                    .next()
                    .unwrap_or("Pasted")
                    .chars()
                    .take(32)
                    .collect::<String>();
                self.open_spec(
                    cx.services,
                    SourceSpec::Text { title },
                    Format::Auto,
                    2000,
                    Some(body),
                );
                true
            }
            _ => false,
        }
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        let c = |id: &str, title: &str| CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: None,
        };
        vec![
            c("logs.open_file", "Logs: Open File…"),
            c("logs.add_source", "Logs: Add Source…"),
            c("logs.paste", "Logs: Paste Text…"),
            c("logs.follow", "Logs: Toggle Follow"),
            c("logs.pause", "Logs: Pause / Resume"),
            c("logs.find", "Logs: Find"),
            c("logs.chip", "Logs: Add Filter"),
            c("logs.bookmark", "Logs: Bookmark Record"),
            c("logs.jump", "Logs: Jump to Time"),
            c("logs.ai", "Logs: AI Assistant"),
            c("logs.save_filtered", "Logs: Save Filtered Output…"),
            c("logs.clear_chips", "Logs: Clear Filters"),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "logs.open_file" => {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    let spec = SourceSpec::LocalFile {
                        path: p.to_string_lossy().to_string(),
                    };
                    if self.state.spec.is_some() {
                        cx.open_new_tab(
                            "logs",
                            Some(OpenRequest::Text {
                                mime: SPEC_MIME.into(),
                                body: serde_json::to_string(&SavedSource {
                                    spec,
                                    ..Default::default()
                                })
                                .unwrap_or_default(),
                            }),
                        );
                    } else {
                        self.open_spec(cx.services, spec, Format::Auto, 2000, None);
                    }
                }
                true
            }
            "logs.add_source" => {
                self.add = Some(AddSource {
                    tail_lines: 2000,
                    ..Default::default()
                });
                true
            }
            "logs.paste" => {
                self.add = Some(AddSource {
                    kind: 4,
                    tail_lines: 2000,
                    ..Default::default()
                });
                true
            }
            "logs.follow" => {
                self.toggle_follow();
                true
            }
            "logs.pause" => {
                self.paused = !self.paused;
                true
            }
            "logs.find" => {
                self.find.open = true;
                true
            }
            "logs.chip" => {
                self.new_chip = Some(NewChip {
                    level: Level::Warn,
                    ..Default::default()
                });
                true
            }
            "logs.bookmark" => {
                self.toggle_bookmark();
                true
            }
            "logs.jump" => {
                self.jump = Some(String::new());
                true
            }
            "logs.ai" => {
                self.assistant.toggle("Logs");
                true
            }
            "logs.save_filtered" => {
                match self.save_filtered(cx.services) {
                    Ok(p) => cx.toast(
                        ToastLevel::Success,
                        cx.services
                            .i18n
                            .t_args("logs-saved", &[("path", &p.display().to_string())]),
                    ),
                    Err(e) => cx.toast(ToastLevel::Info, e),
                }
                true
            }
            "logs.clear_chips" => {
                self.state.chips.clear();
                self.chips_changed();
                true
            }
            _ => false,
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        form::header_bar(ui, &theme, |ui| {
            ui.label(egui::RichText::new(s.t("tool-logs")).strong().size(15.0));
        });
        ui.horizontal(|ui| {
            if form::icon_button(ui, &theme, ph::FOLDER_OPEN, 16.0, false)
                .tip(s.t("logs-open-file"))
                .clicked()
            {
                self.run_command("logs.open_file", cx);
            }
            if form::icon_button(ui, &theme, ph::PLUS, 16.0, false)
                .tip(s.t("logs-add-source"))
                .clicked()
            {
                self.run_command("logs.add_source", cx);
            }
            if form::icon_button(ui, &theme, ph::CLIPBOARD_TEXT, 16.0, false)
                .tip(s.t("logs-paste"))
                .clicked()
            {
                self.run_command("logs.paste", cx);
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.sidebar_filter)
                    .hint_text(s.t("logs-filter-sources"))
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(4.0);
        // The column is three stacked sections (owner's request,
        // 2026-10-23): the saved logs take about 40% of it, Running
        // Service 30%, and the filter presets what is left. Each scrolls
        // on its own, so a long list never pushes the others off.
        let col_h = ui.available_height();
        let saved_height = (col_h * 0.40).max(120.0);
        let services_height = (col_h * 0.30).max(100.0);
        // A flat list the user arranges themselves. It used to be grouped
        // by the environment of each source's connection, which said the
        // same thing twice: the environment is chosen in the title bar and
        // every tab already follows it (owner's request, 2026-10-23).
        let needle = self.sidebar_filter.to_lowercase();
        let shown: Vec<SavedSource> = self
            .saved
            .iter()
            .filter(|src| needle.is_empty() || src.name.to_lowercase().contains(&needle))
            .cloned()
            .collect();
        let current_uri = self.state.spec.as_ref().map(SourceSpec::uri);
        let mut open: Option<SavedSource> = None;
        let mut edit: Option<SavedSource> = None;
        let mut delete: Option<String> = None;
        let mut moved: Option<(usize, usize)> = None;
        egui::ScrollArea::vertical()
            .id_salt("logs-sources")
            .auto_shrink([false, false])
            .max_height(saved_height)
            .show(ui, |ui| {
                if self.saved.is_empty() {
                    form::note(ui, &theme, hs_ui::icons::INFO, &s.t("logs-no-sources"));
                }
                // The grip is the drag source, as in every other activity's
                // list; the row itself stays clickable.
                let mut reorder = hs_ui::reorder::Reorder::new("logs-sources");
                for (i, src) in shown.iter().enumerate() {
                    reorder.row(ui, i, |ui| {
                        let live = current_uri.as_deref() == Some(src.spec.uri().as_str());
                        let r = form::list_row(
                            ui,
                            &theme,
                            live,
                            src.spec.icon(),
                            &src.name,
                            Some(&src.spec.describe()),
                        );
                        if r.clicked() {
                            open = Some(src.clone());
                        }
                        let del = s.t("logs-delete-source");
                        let open_label = s.t("logs-open-source");
                        let edit_label = s.t("logs-edit-source");
                        r.context_menu(|ui| {
                            if ui.button(&open_label).clicked() {
                                open = Some(src.clone());
                                ui.close();
                            }
                            if ui.button(&edit_label).clicked() {
                                edit = Some(src.clone());
                                ui.close();
                            }
                            if ui.button(&del).clicked() {
                                delete = Some(src.name.clone());
                                ui.close();
                            }
                        });
                    });
                    ui.add_space(2.0);
                }
                moved = reorder.finish(ui, theme.accent);
            });
        if let Some((from, to)) = moved
            && from < shown.len()
        {
            // The order is the whole saved list's, not the filtered view's,
            // so a move made while filtering still means what it looks like.
            let name = shown[from].name.clone();
            let before = shown.get(to).map(|s| s.name.clone());
            self.reorder_saved(&name, before.as_deref(), cx);
        }
        if let Some(src) = edit {
            self.add = Some(AddSource::editing(&src));
        }
        if let Some(src) = open {
            // One tab per saved log (owner's request, 2026-10-23): a log
            // that is already open is focused rather than opened twice.
            let want = src.spec.uri();
            let existing = s
                .open_tabs
                .of("logs")
                .into_iter()
                .find(|t| t.uri.as_deref() == Some(want.as_str()))
                .map(|t| t.id);
            match existing {
                Some(id) => cx
                    .actions
                    .push(hs_core::tool::Action::FocusTab { tab_id: id }),
                None => cx.open_new_tab(
                    "logs",
                    Some(OpenRequest::Text {
                        mime: SPEC_MIME.into(),
                        body: serde_json::to_string(&src).unwrap_or_default(),
                    }),
                ),
            }
        }

        if let Some(name) = delete {
            self.confirm_delete = Some(name);
        }
        ui.separator();
        self.running_services(ui, cx, &theme, services_height);
        ui.separator();
        ui.label(
            egui::RichText::new(s.t("logs-presets"))
                .small()
                .strong()
                .color(theme.text_muted),
        );
        if self.presets.is_empty() {
            form::hint(ui, &theme, &s.t("logs-no-presets"));
        }
        let mut apply: Option<Vec<Chip>> = None;
        let mut remove: Option<usize> = None;
        for (i, p) in self.presets.iter().enumerate() {
            let r = ui.selectable_label(false, format!("{} {}", ph::FUNNEL, p.name));
            if r.clicked() {
                apply = Some(p.chips.clone());
            }
            let del = s.t("logs-delete-preset");
            r.context_menu(|ui| {
                if ui.button(&del).clicked() {
                    remove = Some(i);
                    ui.close();
                }
            });
        }
        if let Some(chips) = apply {
            self.state.chips = chips;
            self.chips_changed();
        }
        if let Some(i) = remove {
            self.presets.remove(i);
            if let Err(e) = save_presets(&self.logs_dir, &self.presets) {
                cx.toast(ToastLevel::Error, e);
            }
        }
        if let Some(name) = self.confirm_delete.clone() {
            match hs_ui::dialog::confirm(
                ui.ctx(),
                &theme,
                "logs-delete-source",
                &s.t("logs-delete-source"),
                &s.i18n
                    .t_args("logs-delete-source-confirm", &[("name", &name)]),
                &s.t("logs-delete-source"),
                &s.t("cancel"),
                true,
            ) {
                hs_ui::dialog::DialogResult::Ok => {
                    self.confirm_delete = None;
                    if let Some(src) = self.saved.iter().find(|x| x.name == name).cloned() {
                        if let Err(e) = src.delete(&self.logs_dir) {
                            cx.toast(ToastLevel::Error, e.to_string());
                        }
                        self.saved = SavedSource::load_all(&self.logs_dir);
                    }
                }
                hs_ui::dialog::DialogResult::Cancel => self.confirm_delete = None,
                hs_ui::dialog::DialogResult::Open => {}
            }
        }
        true
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let (status, bytes, lines, format) = {
            let sh = self.shared.lock();
            (sh.status.clone(), sh.bytes, sh.lines, sh.format)
        };
        let dot = match status {
            Status::Following => theme.success,
            Status::Reading | Status::Connecting => theme.warning,
            Status::Failed(_) => theme.error,
            Status::Done | Status::Idle => theme.text_muted,
        };
        ui.label(egui::RichText::new("●").color(dot));
        ui.label(status_label(s, &status));
        if self.state.spec.is_some() {
            ui.separator();
            ui.label(
                egui::RichText::new(format!(
                    "{} · {} {}",
                    human_bytes(bytes),
                    lines,
                    s.t("logs-lines")
                ))
                .small()
                .color(theme.text_muted),
            );
            ui.separator();
            if ui
                .selectable_label(
                    self.state.follow,
                    format!("{} {}", ph::ARROW_LINE_DOWN, s.t("logs-follow")),
                )
                .tip(s.t("logs-follow-tip"))
                .clicked()
            {
                self.toggle_follow();
            }
            if ui
                .selectable_label(
                    self.paused,
                    format!(
                        "{} {}",
                        if self.paused { ph::PLAY } else { ph::PAUSE },
                        s.t("logs-pause")
                    ),
                )
                .tip(s.t("logs-pause-tip"))
                .clicked()
            {
                self.paused = !self.paused;
            }
            ui.separator();
            ui.label(s.t("logs-format"));
            let mut chosen = self.state.format;
            egui::ComboBox::from_id_salt("logs-format")
                .selected_text(if chosen == Format::Auto {
                    format!(
                        "{} ({})",
                        s.t(Format::Auto.label_key()),
                        s.t(format.label_key())
                    )
                } else {
                    s.t(chosen.label_key())
                })
                .show_ui(ui, |ui| {
                    for f in Format::ALL {
                        ui.selectable_value(&mut chosen, f, s.t(f.label_key()));
                    }
                });
            if chosen != self.state.format {
                self.state.format = chosen;
                self.start(s);
            }
            ui.separator();
            if ui
                .selectable_label(self.state.local_time, ph::CLOCK)
                .tip(s.t("logs-local-time-tip"))
                .clicked()
            {
                self.state.local_time = !self.state.local_time;
            }
            // Mask Secrets governs the view and every export at once, so
            // what is on screen is what would leave the tab.
            if ui
                .selectable_label(self.state.mask, ph::EYE_SLASH)
                .tip(s.t("logs-mask-tip"))
                .clicked()
            {
                self.state.mask = !self.state.mask;
            }
            if ui
                .selectable_label(self.state.collapse, ph::ARROWS_IN_LINE_VERTICAL)
                .tip(s.t("logs-collapse-tip"))
                .clicked()
            {
                self.state.collapse = !self.state.collapse;
                self.expanded.clear();
                self.rows.clear();
                self.rows_upto = 0;
                self.rows_collapse = !self.state.collapse;
                self.rebuild_rows();
            }
            if ui
                .button(format!("{} {}", ph::FLOPPY_DISK, s.t("logs-save-source")))
                .tip(s.t("logs-save-source-tip"))
                .clicked()
            {
                let name = self
                    .state
                    .spec
                    .as_ref()
                    .map(|x| x.label())
                    .unwrap_or_default();
                match self.save_source(&name) {
                    Ok(()) => cx.toast(ToastLevel::Success, s.t("logs-source-saved")),
                    Err(e) => cx.toast(ToastLevel::Error, e),
                }
            }
            if ui
                .button(format!(
                    "{} {}",
                    ph::DOWNLOAD_SIMPLE,
                    s.t("logs-save-filtered")
                ))
                .clicked()
            {
                self.run_command("logs.save_filtered", cx);
            }
            if ui
                .button(format!("{} {}", ph::ARROWS_CLOCKWISE, s.t("logs-reopen")))
                .tip(s.t("logs-reopen-tip"))
                .clicked()
            {
                self.start(s);
            }
        }
        // The assistant is the same control in every tool, so it sits in
        // the same place: hard right of the toolbar. Not on the dashboard,
        // which has no record to discuss and draws no column, so the
        // control there would be a toggle that does nothing.
        if !cx.is_home {
            hs_ui::toolbar::hard_right(ui, |ui| {
                if ui
                    .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
                    .on_hover_text(s.t("ai-shared-button-tip"))
                    .clicked()
                {
                    self.assistant.toggle("Logs");
                }
            });
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        self.is_home = cx.is_home;
        self.dialogs(ui, cx, &theme);
        if cx.is_home {
            self.dashboard(ui, cx);
            return;
        }
        if self.state.spec.is_none() {
            self.empty_state(ui, cx, &theme);
            return;
        }
        self.keys(ui, cx);
        self.refilter();
        // The column's rectangle is taken out of the tab first, so the
        // list lays out in what is left; it is drawn last, as every other
        // tool draws it, so nothing of the tool paints over it.
        let (mut content, assistant_rect) =
            hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        let outer = ui;
        let ui = &mut content;
        self.chip_row(ui, &s, &theme);
        if self.state.detail_open && self.selected.is_some() {
            egui::Panel::bottom("logs-detail")
                .default_size(200.0)
                .size_range(100.0..=(ui.available_height() * 0.6).max(100.0))
                .resizable(true)
                .show(ui, |ui| self.detail(ui, cx, &theme));
        }
        self.status_line(ui, &s, &theme);
        self.list(ui, &s, &theme);
        self.assistant_column(outer, assistant_rect, cx);
    }
}

impl LogsTool {
    /// How many records either side of the selection the assistant is
    /// given. Enough to show what led to an error, short enough that a
    /// provider's context is not spent on a log tail.
    const AI_CONTEXT: usize = 40;

    /// Everything the assistant is told about this tab, and the column it
    /// is drawn in (docs/tools/logs.md §15.1).
    ///
    /// The context is built from `leaving`, the same funnel Copy and Save
    /// use, so a bearer token in a log line never reaches a provider.
    fn assistant_column(
        &mut self,
        outer: &mut egui::Ui,
        panel: Option<egui::Rect>,
        cx: &mut ToolContext,
    ) {
        if !self.assistant.open {
            return;
        }
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        self.assistant.set_context("Logs", self.ai_context());
        if self.assistant.quick_actions.is_empty() {
            use hs_ui::ai_assistant::QuickAction;
            self.assistant.quick_actions = vec![
                QuickAction::new(s, "ai-qa-logs-explain", "ai-qa-logs-explain-prompt", ""),
                QuickAction::new(s, "ai-qa-logs-trace", "ai-qa-logs-trace-prompt", ""),
                QuickAction::new(s, "ai-qa-logs-why", "ai-qa-logs-why-prompt", ""),
                QuickAction::new(s, "ai-qa-logs-filter", "ai-qa-logs-filter-prompt", ""),
                QuickAction::new(s, "ai-qa-logs-summarise", "ai-qa-logs-summarise-prompt", ""),
            ];
        }
        let Some(panel) = panel else {
            // Too narrow to split: the toggle stays on and the column
            // returns when there is room for it.
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
                // Review only (ADR-019). There is nothing to run here; a
                // block the assistant writes becomes a filter chip, and
                // only because the user pressed Insert.
                self.assistant.can_insert = true;
                let out = self.assistant.ui(ui, s, &theme, "Logs");
                if let Some(code) = out.use_block {
                    self.insert_ai_filter(&code, cx);
                }
            });
    }

    /// The selected record, its neighbours, the filters in force and what
    /// the source is. What is on screen, nothing more, and redacted.
    fn ai_context(&self) -> String {
        let sh = self.shared.lock();
        let source = match &self.state.spec {
            Some(spec) => spec.label(),
            None => "(none)".to_string(),
        };
        let chips = if self.state.chips.is_empty() {
            "(none)".to_string()
        } else {
            self.state
                .chips
                .iter()
                .map(|c| c.label_with(&sh.aliases))
                .collect::<Vec<_>>()
                .join(", ")
        };
        // The window around the selection, in the order it was logged.
        let here = self
            .selected
            .and_then(|idx| self.filtered.iter().position(|&i| i == idx))
            .unwrap_or_else(|| self.filtered.len().saturating_sub(1));
        let from = here.saturating_sub(Self::AI_CONTEXT);
        let to = (here + Self::AI_CONTEXT).min(self.filtered.len().saturating_sub(1));
        let mut lines = String::new();
        for pos in from..=to {
            let Some(&idx) = self.filtered.get(pos) else {
                continue;
            };
            let Some(r) = sh.records.get(idx) else {
                continue;
            };
            if pos == here {
                lines.push_str(">> ");
            }
            lines.push_str(&self.leaving(&r.raw));
            lines.push('\n');
        }
        let selected = match self.selected.and_then(|i| sh.records.get(i)) {
            Some(r) => self.leaving(&r.raw),
            None => "(no record selected)".to_string(),
        };
        format!(
            "Source: {source}\nParser: {:?}\nRecords loaded: {}\nShown after filters: {}\nFilters: {chips}\n\n\
             Selected record (also marked >> below):\n{selected}\n\n\
             Surrounding records, oldest first:\n{lines}",
            sh.format,
            sh.records.len(),
            self.filtered.len(),
        )
    }

    /// A `filter` block the assistant wrote, turned into real chips. The
    /// body is a JSON array of chips in the shape `filter::Chip`
    /// serialises to; anything else is reported rather than guessed at.
    fn insert_ai_filter(&mut self, code: &str, cx: &mut ToolContext) {
        let body = code
            .trim()
            .trim_start_matches("```filter")
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        match serde_json::from_str::<Vec<Chip>>(body) {
            Ok(chips) if !chips.is_empty() => {
                let n = chips.len();
                self.state.chips.extend(chips);
                self.chips_version += 1;
                self.filter = Filter::compile(&self.state.chips);
                cx.toast(
                    ToastLevel::Success,
                    cx.services
                        .i18n
                        .t_args("logs-ai-chips-added", &[("count", &n.to_string())]),
                );
            }
            _ => cx.toast(ToastLevel::Error, cx.services.t("logs-ai-chips-bad")),
        }
    }

    /// What the activity has open and what it could open, in the shape
    /// every other activity's dashboard uses (owner's request,
    /// 2026-10-23).
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let tabs = s.open_tabs.of("logs");
        hs_ui::form::page_title(
            ui,
            &theme,
            &s.t("tool-logs"),
            Some(&s.t("logs-dashboard-sub")),
        );
        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(
                ui,
                &theme,
                &self.saved.len().to_string(),
                &s.t("logs-dash-sources"),
            );
            hs_ui::dashboard::stat(
                ui,
                &theme,
                &self.presets.len().to_string(),
                &s.t("logs-dash-presets"),
            );
            hs_ui::dashboard::stat(ui, &theme, &tabs.len().to_string(), &s.t("dash-open-tabs"));
        });
        egui::ScrollArea::vertical()
            .id_salt("logs-dashboard")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(s.t("logs-dash-start"))
                        .size(15.0)
                        .strong(),
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(format!("{} {}", ph::FOLDER_OPEN, s.t("logs-open-file")))
                        .clicked()
                    {
                        self.run_command("logs.open_file", cx);
                    }
                    if ui
                        .button(format!("{} {}", ph::PLUS, s.t("logs-add-source")))
                        .clicked()
                    {
                        self.run_command("logs.add_source", cx);
                    }
                    if ui
                        .button(format!("{} {}", ph::CLIPBOARD_TEXT, s.t("logs-paste")))
                        .clicked()
                    {
                        self.run_command("logs.paste", cx);
                    }
                });
                // Saved sources, so a dashboard is a way in rather than a
                // page that only counts things.
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new(s.t("logs-dash-saved"))
                        .size(15.0)
                        .strong(),
                );
                ui.add_space(6.0);
                if self.saved.is_empty() {
                    hs_ui::form::hint(ui, &theme, &s.t("logs-dash-no-saved"));
                } else {
                    let saved = self.saved.clone();
                    for src in &saved {
                        let r = hs_ui::form::list_row(
                            ui,
                            &theme,
                            false,
                            ph::SCROLL,
                            &src.name,
                            Some(&src.spec.describe()),
                        );
                        if r.clicked() {
                            cx.actions.push(hs_core::tool::Action::OpenTool {
                                factory_id: "logs".into(),
                                request: Some(hs_core::tool::OpenRequest::File {
                                    uri: src.spec.uri(),
                                    line: None,
                                }),
                            });
                        }
                        ui.add_space(2.0);
                    }
                }
                ui.add_space(16.0);
                let labels = hs_ui::dashboard::DashboardLabels {
                    open_heading: &s.t("dash-open"),
                    nothing_open: &s.t("dash-nothing-open"),
                    no_preview: &s.t("dash-no-preview"),
                };
                if let Some(id) = hs_ui::dashboard::open_tabs(ui, &theme, &tabs, &labels) {
                    cx.actions
                        .push(hs_core::tool::Action::FocusTab { tab_id: id });
                }
            });
    }

    fn keys(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (f, space, ctrl_f, f3, shift_f3, ctrl_l, ctrl_b, ctrl_g, end, home) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::F) && !i.modifiers.any(),
                (i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::P))
                    && !i.modifiers.any(),
                i.modifiers.command && i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::F3) && !i.modifiers.shift,
                i.key_pressed(egui::Key::F3) && i.modifiers.shift,
                i.modifiers.command && i.key_pressed(egui::Key::L),
                i.modifiers.command && i.key_pressed(egui::Key::B),
                i.modifiers.command && i.key_pressed(egui::Key::G),
                i.key_pressed(egui::Key::End),
                i.key_pressed(egui::Key::Home),
            )
        });
        if f {
            self.toggle_follow();
        }
        if space {
            self.paused = !self.paused;
        }
        if ctrl_f {
            self.find.open = true;
        }
        if f3 || shift_f3 {
            self.find_step(!shift_f3);
        }
        if ctrl_l {
            self.run_command("logs.chip", cx);
        }
        if ctrl_b {
            self.toggle_bookmark();
        }
        if ctrl_g {
            self.jump = Some(String::new());
        }
        if end && !self.filtered.is_empty() {
            self.scroll_to = Some(self.filtered.len() - 1);
        }
        if home {
            self.scroll_to = Some(0);
            self.state.follow = false;
        }
    }

    fn find_step(&mut self, forward: bool) {
        self.find_hits();
        if self.find.hits.is_empty() {
            return;
        }
        if forward {
            self.find.at = (self.find.at + 1) % self.find.hits.len();
        } else {
            self.find.at = (self.find.at + self.find.hits.len() - 1) % self.find.hits.len();
        }
        let pos = self.find.hits[self.find.at];
        self.scroll_to = Some(pos);
        self.selected = self.filtered.get(pos).copied();
        self.state.follow = false;
    }

    fn toggle_bookmark(&mut self) {
        let Some(i) = self.selected else { return };
        let line = {
            let sh = self.shared.lock();
            sh.records.get(i).map(|r| r.line)
        };
        let Some(line) = line else { return };
        if let Some(pos) = self.state.bookmarks.iter().position(|(l, _)| *l == line) {
            self.state.bookmarks.remove(pos);
        } else {
            self.state.bookmarks.push((line, String::new()));
            self.state.bookmarks.sort();
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(ph::SCROLL)
                    .size(40.0)
                    .color(theme.text_muted),
            );
            ui.label(
                egui::RichText::new(s.t("logs-empty-title"))
                    .size(18.0)
                    .strong(),
            );
            form::hint(ui, theme, &s.t("logs-empty-hint"));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if form::primary_button(
                    ui,
                    theme,
                    &format!("{} {}", ph::FOLDER_OPEN, s.t("logs-open-file")),
                )
                .clicked()
                {
                    self.run_command("logs.open_file", cx);
                }
                if ui
                    .button(format!("{} {}", ph::PLUS, s.t("logs-add-source")))
                    .clicked()
                {
                    self.run_command("logs.add_source", cx);
                }
                if ui
                    .button(format!("{} {}", ph::CLIPBOARD_TEXT, s.t("logs-paste")))
                    .clicked()
                {
                    self.run_command("logs.paste", cx);
                }
            });
        });
    }

    fn chip_row(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        let mut remove: Option<usize> = None;
        let mut toggle: Option<usize> = None;
        // Levels read in the source's own words (docs/tools/logs.md §6).
        let aliases = self.shared.lock().aliases.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(ph::FUNNEL).color(theme.text_muted));
            for (i, chip) in self.state.chips.iter().enumerate() {
                let fill = if chip.negate {
                    theme.error
                } else {
                    theme.accent
                };
                egui::Frame::new()
                    .fill(fill.linear_multiply(0.18))
                    .stroke(egui::Stroke::new(1.0, fill))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Label::new(chip.label_with(&aliases))
                                        .sense(egui::Sense::click()),
                                )
                                .tip(s.t("logs-chip-negate-tip"))
                                .clicked()
                            {
                                toggle = Some(i);
                            }
                            if ui
                                .add(
                                    egui::Label::new(egui::RichText::new(ph::X).small())
                                        .sense(egui::Sense::click()),
                                )
                                .clicked()
                            {
                                remove = Some(i);
                            }
                        });
                    });
            }
            if ui
                .small_button(format!("{} {}", ph::PLUS, s.t("logs-add-chip")))
                .clicked()
            {
                self.new_chip = Some(NewChip {
                    level: Level::Warn,
                    ..Default::default()
                });
            }
            if !self.state.chips.is_empty() {
                if ui.small_button(s.t("logs-clear-chips")).clicked() {
                    self.state.chips.clear();
                    self.chips_changed();
                }
                if ui
                    .small_button(format!("{} {}", ph::FLOPPY_DISK, s.t("logs-save-preset")))
                    .clicked()
                {
                    self.preset_name = Some(String::new());
                }
            }
            if self.find.open {
                ui.separator();
                ui.label(ph::MAGNIFYING_GLASS);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.find.text)
                        .hint_text(s.t("logs-find-hint"))
                        .desired_width(180.0),
                );
                if r.changed() {
                    self.find.at = 0;
                }
                self.find_hits();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter {
                    self.find_step(true);
                    r.request_focus();
                }
                ui.label(
                    egui::RichText::new(if self.find.hits.is_empty() {
                        s.t("logs-find-none")
                    } else {
                        format!("{}/{}", self.find.at + 1, self.find.hits.len())
                    })
                    .small()
                    .color(theme.text_muted),
                );
                if ui.small_button(ph::CARET_UP).clicked() {
                    self.find_step(false);
                }
                if ui.small_button(ph::CARET_DOWN).clicked() {
                    self.find_step(true);
                }
                if ui.small_button(ph::X).clicked() {
                    self.find.open = false;
                    self.find.text.clear();
                }
            }
        });
        if let Some(i) = toggle {
            self.state.chips[i].negate = !self.state.chips[i].negate;
            self.chips_changed();
        }
        if let Some(i) = remove {
            self.state.chips.remove(i);
            self.chips_changed();
        }
        ui.separator();
    }

    fn status_line(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        let (total, dropped, rotations, cut, stderr) = {
            let sh = self.shared.lock();
            (
                sh.records.len(),
                sh.dropped,
                sh.rotations,
                sh.truncated_start,
                sh.stderr.last().cloned(),
            )
        };
        ui.horizontal_wrapped(|ui| {
            if self.unseen > 0 {
                if ui
                    .button(
                        egui::RichText::new(
                            s.i18n
                                .t_args("logs-new-lines", &[("n", &self.unseen.to_string())]),
                        )
                        .color(theme.accent),
                    )
                    .clicked()
                {
                    self.unseen = 0;
                    if !self.filtered.is_empty() {
                        self.scroll_to = Some(self.filtered.len() - 1);
                    }
                    self.state.follow = true;
                }
                ui.separator();
            }
            let mut text = s.i18n.t_args(
                "logs-shown",
                &[
                    ("shown", &self.filtered.len().to_string()),
                    ("total", &total.to_string()),
                ],
            );
            if dropped > 0 {
                text.push_str(&format!(
                    " · {}",
                    s.i18n
                        .t_args("logs-dropped", &[("n", &dropped.to_string())])
                ));
            }
            if cut {
                text.push_str(&format!(" · {}", s.t("logs-truncated-start")));
            }
            if rotations > 0 {
                text.push_str(&format!(
                    " · {}",
                    s.i18n
                        .t_args("logs-rotations", &[("n", &rotations.to_string())])
                ));
            }
            if !self.state.bookmarks.is_empty() {
                text.push_str(&format!(
                    " · {}",
                    s.i18n.t_args(
                        "logs-bookmarks",
                        &[("n", &self.state.bookmarks.len().to_string())]
                    )
                ));
            }
            text.push_str(&format!(
                " · {}",
                if self.state.local_time {
                    s.t("logs-tz-local")
                } else {
                    "UTC".into()
                }
            ));
            ui.label(egui::RichText::new(text).small().color(theme.text_muted));
            if let Some(e) = stderr {
                ui.separator();
                ui.label(egui::RichText::new(e).small().color(theme.warning));
            }
        });
        self.status_text.clear();
    }

    fn list(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        let row_h = 20.0;
        let structured = {
            let sh = self.shared.lock();
            sh.format.structured()
        };
        let pinned: Vec<String> = self.state.pinned.clone();
        // The list shows what would leave the tab, so a secret is masked in
        // the message, in a pinned column and in the accessibility label.
        let mask = self.state.mask.then_some(&self.redactor);
        let local = self.state.local_time;
        let bookmarks: Vec<u64> = self.state.bookmarks.iter().map(|(l, _)| *l).collect();
        let stick = self.state.follow && !self.paused && self.scroll_to.is_none();
        // How wide the widest line needs. The rows are monospaced, so one
        // glyph width answers for all of them. Without this the rows only
        // ever claimed the width already on screen, so `ScrollArea::both`
        // had nothing to scroll and a long line was simply cut off - and
        // opening the assistant made the cut land mid-sentence (owner's
        // report, 2026-10-23).
        let glyph = ui
            .ctx()
            .fonts_mut(|f| f.glyph_width(&egui::FontId::monospace(12.5), 'M'));
        let longest = self.shared.lock().max_chars;
        let text_w = TEXT_COLUMN_X + pinned.len() as f32 * 150.0 + longest as f32 * glyph + 24.0;
        let mut area = egui::ScrollArea::both()
            .id_salt("logs-list")
            .auto_shrink([false, false])
            .stick_to_bottom(stick);
        // `show_rows` lays rows out at `row_h` plus the item spacing; the
        // offsets below must use that pitch or a jump lands rows short.
        let pitch = row_h + ui.spacing().item_spacing.y;
        if let Some(pos) = self.scroll_to.take() {
            let y = pos as f32 * pitch;
            area = area.vertical_scroll_offset((y - 60.0).max(0.0));
        }
        // One entry per drawn row: the filtered position it shows, and
        // how many records it stands for (1 unless a run is folded).
        let view: Vec<(usize, usize)> = self
            .rows
            .iter()
            .flat_map(|row| match row {
                crate::shape::Row::One(p) => vec![(*p, 1usize)],
                crate::shape::Row::Run { first, count, span } => {
                    if self.expanded.contains(&span.0) {
                        (span.0..=span.1).map(|p| (p, 1usize)).collect()
                    } else {
                        vec![(*first, *count)]
                    }
                }
            })
            .collect();
        let n = view.len();
        let mut clicked: Option<usize> = None;
        let mut toggle_run: Option<usize> = None;
        let find_hit: Option<usize> = self
            .find
            .hits
            .get(self.find.at)
            .copied()
            .filter(|_| self.find.open);
        let out = area.show_rows(ui, row_h, n, |ui, range| {
            ui.set_min_width(ui.available_width());
            let sh = self.shared.lock();
            // Column widths: line, time, level, pinned fields.
            let mono = egui::FontId::monospace(12.5);
            for row_i in range {
                let Some(&(pos, run)) = view.get(row_i) else {
                    continue;
                };
                let Some(&idx) = self.filtered.get(pos) else {
                    continue;
                };
                let Some(r) = sh.records.get(idx) else {
                    continue;
                };
                let selected = self.selected == Some(idx);
                let paint = self.highlights.paint(r);
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width().max(400.0).max(text_w), row_h),
                    egui::Sense::click(),
                );
                if selected {
                    ui.painter()
                        .rect_filled(rect, 2.0, theme.accent.linear_multiply(0.25));
                } else if find_hit == Some(pos) {
                    ui.painter()
                        .rect_filled(rect, 2.0, theme.warning.linear_multiply(0.25));
                } else if row_i % 2 == 1 {
                    ui.painter()
                        .rect_filled(rect, 0.0, theme.surface.linear_multiply(0.5));
                }
                if bookmarks.contains(&r.line) {
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
                        0.0,
                        theme.warning,
                    );
                }
                let mut x = rect.min.x + 8.0;
                let y = rect.center().y;
                let p = ui.painter();
                let text_colour = match paint {
                    Some((c, _)) => egui::Color32::from_rgb(c[0], c[1], c[2]),
                    None => theme.text,
                };
                p.text(
                    egui::pos2(x, y),
                    egui::Align2::LEFT_CENTER,
                    r.line.to_string(),
                    mono.clone(),
                    theme.text_muted,
                );
                x += 62.0;
                let t = time_text(r, local);
                p.text(
                    egui::pos2(x, y),
                    egui::Align2::LEFT_CENTER,
                    t,
                    mono.clone(),
                    theme.text_muted,
                );
                x += 96.0;
                if let Some(l) = r.level {
                    p.text(
                        egui::pos2(x, y),
                        egui::Align2::LEFT_CENTER,
                        l.label(),
                        egui::FontId::monospace(11.5),
                        level_colour(theme, l),
                    );
                }
                x += 52.0;
                if structured {
                    for f in &pinned {
                        let v = r.field(f).unwrap_or("");
                        let mut v: String = v.chars().take(24).collect();
                        if let Some(red) = mask {
                            v = red.apply(&v);
                        }
                        p.text(
                            egui::pos2(x, y),
                            egui::Align2::LEFT_CENTER,
                            v,
                            mono.clone(),
                            theme.text_muted,
                        );
                        x += 150.0;
                    }
                }
                let line = if structured && !r.message.is_empty() {
                    let m = r.message.replace('\n', " ⏎ ");
                    match mask {
                        Some(red) => red.apply(&m),
                        None => m,
                    }
                } else {
                    one_line(r, mask)
                };
                let font = if paint.is_some_and(|(_, b)| b) {
                    egui::FontId::monospace(13.0)
                } else {
                    mono.clone()
                };
                p.text(
                    egui::pos2(x, y),
                    egui::Align2::LEFT_CENTER,
                    line,
                    font,
                    text_colour,
                );
                if run > 1 {
                    let tag = format!("×{run}");
                    let g = egui::FontId::proportional(11.0);
                    let w = 34.0;
                    let tag_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.max.x - w - 8.0, rect.center().y - 8.0),
                        egui::vec2(w, 16.0),
                    );
                    p.rect_filled(tag_rect, 8.0, theme.surface_hover);
                    p.text(
                        tag_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        tag,
                        g,
                        theme.text_muted,
                    );
                }
                if resp.clicked() {
                    if run > 1 {
                        // Opening a run shows its records; it does not
                        // select one, because the user asked to see them.
                        toggle_run = Some(pos + 1 - run);
                    } else {
                        clicked = Some(idx);
                    }
                }
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, one_line(r, mask))
                });
            }
        });
        // At the bottom when the last row is within the viewport.
        let content_h = n as f32 * pitch;
        self.at_bottom = out.state.offset.y + out.inner_rect.height() >= content_h - pitch * 2.0;
        if self.at_bottom && !self.paused {
            self.unseen = 0;
        }
        if let Some(first) = toggle_run {
            // Opening a run shows its records; closing it folds them back.
            if !self.expanded.remove(&first) {
                self.expanded.insert(first);
            }
        }
        if let Some(i) = clicked {
            self.selected = if self.selected == Some(i) {
                None
            } else {
                Some(i)
            };
            if self.selected.is_some() {
                self.state.follow = false;
            }
        }
        // The list is the one surface a drag scrolls, so a scroll up ends
        // following; a scroll back to the bottom does not restart it, the
        // Follow toggle and the badge do.
        if ui.input(|i| i.smooth_scroll_delta.y > 0.0)
            && out
                .inner_rect
                .contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default()))
        {
            self.state.follow = false;
        }
        let _ = s;
    }

    fn detail(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        let Some(i) = self.selected else { return };
        let rec = {
            let sh = self.shared.lock();
            sh.records.get(i).cloned()
        };
        let Some(rec) = rec else {
            self.selected = None;
            return;
        };
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} {}  {}  {}",
                    s.t("logs-record"),
                    rec.line,
                    time_text(&rec, self.state.local_time),
                    rec.level.map(|l| l.label()).unwrap_or("")
                ))
                .strong(),
            );
            if ui
                .small_button(format!("{} {}", ph::CLIPBOARD_TEXT, s.t("logs-copy")))
                .clicked()
            {
                ui.ctx().copy_text(self.leaving(&rec.raw));
            }
            if !rec.fields.is_empty() && ui.small_button(s.t("logs-copy-json")).clicked() {
                let map: serde_json::Map<String, serde_json::Value> = rec
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect();
                let json = serde_json::to_string_pretty(&map).unwrap_or_default();
                ui.ctx().copy_text(self.leaving(&json));
            }
            let marked = self.state.bookmarks.iter().any(|(l, _)| *l == rec.line);
            if ui
                .selectable_label(
                    marked,
                    format!("{} {}", ph::BOOKMARK_SIMPLE, s.t("logs-bookmark")),
                )
                .clicked()
            {
                self.toggle_bookmark();
            }
            if let Some(c) = PATH_LINE.captures(&rec.raw)
                && ui
                    .small_button(format!("{} {}", ph::FILE_TEXT, s.t("logs-open-in-editor")))
                    .tip(c.get(0).map(|m| m.as_str().to_string()).unwrap_or_default())
                    .clicked()
            {
                let path = c.get(1).unwrap().as_str().to_string();
                let line = c.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
                cx.open("editor", Some(OpenRequest::File { uri: path, line }));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(ph::X).clicked() {
                    self.selected = None;
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("logs-detail-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !rec.fields.is_empty() {
                    let mut add_chip: Option<Chip> = None;
                    let mut pin: Option<String> = None;
                    // Keyed by record: a Grid keeps the tallest row it has
                    // seen under an id, and egui memory outlives the selection.
                    egui::Grid::new(("logs-fields", rec.line))
                        .num_columns(3)
                        .striped(true)
                        .spacing([12.0, 2.0])
                        .show(ui, |ui| {
                            for (k, v) in &rec.fields {
                                ui.label(
                                    egui::RichText::new(k).monospace().color(theme.text_muted),
                                );
                                // Actions sit in a fixed column before the
                                // value; the value takes what is left (a
                                // wrapped label in a grid otherwise shrinks
                                // to last frame's width).
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button(ph::FUNNEL)
                                        .tip(s.t("logs-filter-by"))
                                        .clicked()
                                    {
                                        add_chip =
                                            Some(Chip::new(ChipKind::Field(k.clone(), v.clone())));
                                    }
                                    if ui
                                        .small_button(ph::EYE_SLASH)
                                        .tip(s.t("logs-exclude"))
                                        .clicked()
                                    {
                                        add_chip = Some(Chip {
                                            kind: ChipKind::Field(k.clone(), v.clone()),
                                            negate: true,
                                        });
                                    }
                                    let pinned = self.state.pinned.contains(k);
                                    if ui
                                        .add(egui::Button::selectable(pinned, ph::PUSH_PIN).small())
                                        .tip(s.t("logs-pin-column"))
                                        .clicked()
                                    {
                                        pin = Some(k.clone());
                                    }
                                });
                                ui.scope(|ui| {
                                    // logs-detail-value
                                    let w = ui.available_width();
                                    ui.set_min_width(w);
                                    ui.set_max_width(w);
                                    // One line per field: a wrapped label's first-frame
                                    // height sticks in the grid. The raw record below
                                    // carries the whole value.
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(v).monospace())
                                            .truncate(),
                                    )
                                    .on_hover_text(v);
                                });
                                ui.end_row();
                            }
                        });
                    if let Some(c) = add_chip {
                        self.state.chips.push(c);
                        self.chips_changed();
                    }
                    if let Some(k) = pin {
                        if let Some(p) = self.state.pinned.iter().position(|x| *x == k) {
                            self.state.pinned.remove(p);
                        } else {
                            self.state.pinned.push(k);
                        }
                    }
                    ui.separator();
                }
                let mut raw = self.leaving(&rec.raw);
                ui.add(
                    egui::TextEdit::multiline(&mut raw)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
            });
    }

    fn dialogs(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services.clone();
        // Add Source.
        if let Some(mut a) = self.add.take() {
            let mut done = false;
            let mut cancel = false;
            let kinds = [
                "logs-kind-local",
                "logs-kind-ssh",
                "logs-kind-command",
                "logs-kind-remote-command",
                "logs-kind-paste",
            ];
            let m = egui::Modal::new(egui::Id::new("logs-add")).show(ui.ctx(), |ui| {
                ui.set_width(520.0);
                ui.heading(s.t("logs-add-source"));
                ui.add_space(6.0);
                form::field_grid(ui, "logs-add-grid", |ui| {
                    form::label(ui, &s.t("logs-kind"));
                    egui::ComboBox::from_id_salt("logs-add-kind")
                        .selected_text(s.t(kinds[a.kind]))
                        .show_ui(ui, |ui| {
                            for (i, k) in kinds.iter().enumerate() {
                                ui.selectable_value(&mut a.kind, i, s.t(k));
                            }
                        });
                    ui.end_row();
                    match a.kind {
                        0 => {
                            form::label(ui, &s.t("logs-path"));
                            ui.horizontal(|ui| {
                                form::text_field_width(ui, &mut a.path, "C:\\logs\\app.log", 300.0);
                                if ui.button(s.t("logs-browse")).clicked()
                                    && let Some(p) = rfd::FileDialog::new().pick_file()
                                {
                                    a.path = p.to_string_lossy().to_string();
                                }
                            });
                            ui.end_row();
                        }
                        1 | 3 => {
                            form::label(ui, &s.t("logs-connection"));
                            let conns: Vec<(String, String)> = s
                                .connections
                                .read()
                                .of_kind("ssh")
                                .map(|c| (c.id.clone(), c.name.clone()))
                                .collect();
                            let current = conns
                                .iter()
                                .find(|(id, _)| *id == a.conn_id)
                                .map(|(_, n)| n.clone())
                                .unwrap_or_else(|| s.t("logs-pick-connection"));
                            egui::ComboBox::from_id_salt("logs-add-conn")
                                .selected_text(current)
                                .show_ui(ui, |ui| {
                                    for (id, name) in &conns {
                                        ui.selectable_value(&mut a.conn_id, id.clone(), name);
                                    }
                                });
                            ui.end_row();
                            if a.kind == 1 {
                                form::label(ui, &s.t("logs-remote-path"));
                                form::text_field(ui, &mut a.path, "/var/log/app/app.log");
                                ui.end_row();
                                form::label(ui, &s.t("logs-tail-lines"));
                                ui.add(egui::DragValue::new(&mut a.tail_lines).range(0..=100_000));
                                ui.end_row();
                            } else {
                                form::label(ui, &s.t("logs-command"));
                                form::text_field(ui, &mut a.command, "journalctl -f -u app");
                                ui.end_row();
                            }
                        }
                        2 => {
                            form::label(ui, &s.t("logs-command"));
                            form::text_field(ui, &mut a.command, "docker logs -f shop-api");
                            ui.end_row();
                        }
                        _ => {}
                    }
                    if a.kind != 4 {
                        form::label(ui, &s.t("logs-format"));
                        egui::ComboBox::from_id_salt("logs-add-format")
                            .selected_text(s.t(a.format.label_key()))
                            .show_ui(ui, |ui| {
                                for f in Format::ALL {
                                    ui.selectable_value(&mut a.format, f, s.t(f.label_key()));
                                }
                            });
                        ui.end_row();
                        form::label(ui, &s.t("logs-name"));
                        form::text_field(ui, &mut a.name, &s.t("logs-name-hint"));
                        ui.end_row();
                        form::label(ui, "");
                        ui.checkbox(&mut a.save, s.t("logs-save-to-workspace"));
                        ui.end_row();
                    }
                });
                if a.kind == 4 {
                    form::hint(ui, theme, &s.t("logs-paste-hint"));
                    ui.add(
                        egui::TextEdit::multiline(&mut a.pasted)
                            .code_editor()
                            .desired_rows(10)
                            .desired_width(f32::INFINITY),
                    );
                }
                if a.kind == 3 && self.protected_connection(&s, &a.conn_id) {
                    form::note(
                        ui,
                        theme,
                        hs_ui::icons::INFO,
                        &s.t("logs-protected-command"),
                    );
                }
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = match a.kind {
                        0 => !a.path.trim().is_empty(),
                        1 => !a.conn_id.is_empty() && !a.path.trim().is_empty(),
                        2 => !a.command.trim().is_empty(),
                        3 => !a.conn_id.is_empty() && !a.command.trim().is_empty(),
                        _ => !a.pasted.trim().is_empty(),
                    };
                    if ui
                        .add_enabled(ready, egui::Button::new(s.t("logs-open")))
                        .clicked()
                    {
                        done = true;
                    }
                    if ui.button(s.t("cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
            if m.should_close() {
                cancel = true;
            }
            if done {
                let (spec, text) = match a.kind {
                    0 => (
                        SourceSpec::LocalFile {
                            path: a.path.trim().to_string(),
                        },
                        None,
                    ),
                    1 => (
                        SourceSpec::SshFile {
                            conn_id: a.conn_id.clone(),
                            path: a.path.trim().to_string(),
                        },
                        None,
                    ),
                    2 => (
                        SourceSpec::Command {
                            conn_id: None,
                            command: a.command.trim().to_string(),
                        },
                        None,
                    ),
                    3 => (
                        SourceSpec::Command {
                            conn_id: Some(a.conn_id.clone()),
                            command: a.command.trim().to_string(),
                        },
                        None,
                    ),
                    _ => (
                        SourceSpec::Text {
                            title: a
                                .pasted
                                .lines()
                                .next()
                                .unwrap_or("Pasted")
                                .chars()
                                .take(32)
                                .collect(),
                        },
                        Some(a.pasted.clone()),
                    ),
                };
                let name = if a.name.trim().is_empty() {
                    spec.label()
                } else {
                    a.name.trim().to_string()
                };
                let saved = SavedSource {
                    name,
                    spec: spec.clone(),
                    format: a.format,
                    tail_lines: a.tail_lines,
                    order: a.order,
                    ..Default::default()
                };
                if a.save && a.kind != 4 {
                    // A renamed source changes its file name, so the file
                    // it had has to go or the list shows both.
                    if let Some(was) = a.editing.as_deref()
                        && was != saved.name
                        && let Some(old) = self.saved.iter().find(|x| x.name == was)
                        && let Err(e) = old.delete(&self.logs_dir)
                    {
                        cx.toast(ToastLevel::Error, e.to_string());
                    }
                    match saved.save(&self.logs_dir) {
                        Ok(()) => self.saved = SavedSource::load_all(&self.logs_dir),
                        Err(e) => cx.toast(ToastLevel::Error, e.to_string()),
                    }
                }
                if a.editing.is_none() && self.state.spec.is_some() && text.is_none() {
                    cx.open_new_tab(
                        "logs",
                        Some(OpenRequest::Text {
                            mime: SPEC_MIME.into(),
                            body: serde_json::to_string(&saved).unwrap_or_default(),
                        }),
                    );
                } else if self.state.spec.is_some() {
                    cx.open_new_tab(
                        "logs",
                        Some(OpenRequest::Text {
                            mime: "text/plain".into(),
                            body: text.unwrap_or_default(),
                        }),
                    );
                } else {
                    self.open_spec(&s, spec, a.format, a.tail_lines, text);
                }
            } else if !cancel {
                self.add = Some(a);
            }
        }
        // New chip.
        if let Some(mut c) = self.new_chip.take() {
            let mut done = false;
            let mut cancel = false;
            let kinds = [
                "logs-chip-contains",
                "logs-chip-regex",
                "logs-chip-level",
                "logs-chip-field",
                "logs-chip-after",
                "logs-chip-before",
            ];
            let m = egui::Modal::new(egui::Id::new("logs-chip")).show(ui.ctx(), |ui| {
                ui.set_width(440.0);
                ui.heading(s.t("logs-add-chip"));
                ui.add_space(6.0);
                form::field_grid(ui, "logs-chip-grid", |ui| {
                    form::label(ui, &s.t("logs-chip-kind"));
                    egui::ComboBox::from_id_salt("logs-chip-kind")
                        .selected_text(s.t(kinds[c.kind]))
                        .show_ui(ui, |ui| {
                            for (i, k) in kinds.iter().enumerate() {
                                ui.selectable_value(&mut c.kind, i, s.t(k));
                            }
                        });
                    ui.end_row();
                    match c.kind {
                        2 => {
                            form::label(ui, &s.t("logs-chip-level"));
                            let aliases = self.shared.lock().aliases.clone();
                            egui::ComboBox::from_id_salt("logs-chip-level")
                                .selected_text(aliases.name(c.level))
                                .show_ui(ui, |ui| {
                                    for l in Level::ALL {
                                        ui.selectable_value(&mut c.level, l, aliases.name(l));
                                    }
                                });
                            ui.end_row();
                        }
                        3 => {
                            form::label(ui, &s.t("logs-chip-field-name"));
                            form::text_field(ui, &mut c.field, "service");
                            ui.end_row();
                            form::label(ui, &s.t("logs-chip-value"));
                            let r = form::text_field(ui, &mut c.text, "OrderSvc");
                            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                done = true;
                            }
                            ui.end_row();
                        }
                        4 | 5 => {
                            form::label(ui, &s.t("logs-chip-time"));
                            let r = form::text_field(ui, &mut c.text, "2026-10-22T14:32:00Z");
                            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                done = true;
                            }
                            ui.end_row();
                        }
                        _ => {
                            form::label(ui, &s.t("logs-chip-text"));
                            let r = form::text_field(ui, &mut c.text, "timeout");
                            r.request_focus();
                            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                done = true;
                            }
                            ui.end_row();
                        }
                    }
                    form::label(ui, "");
                    ui.checkbox(&mut c.negate, s.t("logs-chip-exclude"));
                    ui.end_row();
                });
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if form::primary_button(ui, theme, &s.t("logs-add-chip")).clicked() {
                        done = true;
                    }
                    if ui.button(s.t("cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
            if m.should_close() {
                cancel = true;
            }
            if done {
                let kind = match c.kind {
                    0 => Some(ChipKind::Contains(c.text.trim().to_string())),
                    1 => Regex::new(c.text.trim())
                        .ok()
                        .map(|_| ChipKind::Regex(c.text.trim().to_string())),
                    2 => Some(ChipKind::LevelAtLeast(c.level)),
                    3 => Some(ChipKind::Field(
                        c.field.trim().to_string(),
                        c.text.trim().to_string(),
                    )),
                    4 => crate::parse::parse_timestamp(&c.text).map(ChipKind::After),
                    _ => crate::parse::parse_timestamp(&c.text).map(ChipKind::Before),
                };
                match kind {
                    Some(k) if !matches!(&k, ChipKind::Contains(t) if t.is_empty()) => {
                        self.state.chips.push(Chip {
                            kind: k,
                            negate: c.negate,
                        });
                        self.chips_changed();
                    }
                    _ => cx.toast(ToastLevel::Warning, s.t("logs-chip-invalid")),
                }
            } else if !cancel {
                self.new_chip = Some(c);
            }
        }
        // Jump to time.
        if let Some(mut text) = self.jump.take() {
            let mut done = false;
            let mut cancel = false;
            let m = egui::Modal::new(egui::Id::new("logs-jump")).show(ui.ctx(), |ui| {
                ui.set_width(380.0);
                ui.heading(s.t("logs-jump"));
                form::hint(ui, theme, &s.t("logs-jump-hint"));
                let r = form::text_field(ui, &mut text, "14:32:01");
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if form::primary_button(ui, theme, &s.t("logs-jump-go")).clicked() {
                        done = true;
                    }
                    if ui.button(s.t("cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
            if m.should_close() {
                cancel = true;
            }
            if done {
                if let Some(pos) = self.position_for_time(&text) {
                    self.scroll_to = Some(pos);
                    self.selected = self.filtered.get(pos).copied();
                    self.state.follow = false;
                } else {
                    cx.toast(ToastLevel::Warning, s.t("logs-jump-none"));
                }
            } else if !cancel {
                self.jump = Some(text);
            }
        }
        // Save preset.
        if let Some(mut name) = self.preset_name.take() {
            let mut done = false;
            let mut cancel = false;
            let m = egui::Modal::new(egui::Id::new("logs-preset")).show(ui.ctx(), |ui| {
                ui.set_width(360.0);
                ui.heading(s.t("logs-save-preset"));
                let r = form::text_field(ui, &mut name, &s.t("logs-preset-name-hint"));
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if form::primary_button(ui, theme, &s.t("save")).clicked() {
                        done = true;
                    }
                    if ui.button(s.t("cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
            if m.should_close() {
                cancel = true;
            }
            if done && !name.trim().is_empty() {
                self.presets.retain(|p| p.name != name.trim());
                self.presets.push(Preset {
                    name: name.trim().to_string(),
                    chips: self.state.chips.clone(),
                });
                if let Err(e) = save_presets(&self.logs_dir, &self.presets) {
                    cx.toast(ToastLevel::Error, e);
                }
            } else if !done && !cancel {
                self.preset_name = Some(name);
            }
        }
    }

    fn protected_connection(&self, s: &Services, conn_id: &str) -> bool {
        let conns = s.connections.read();
        let envs = s.environments.read();
        conns
            .by_id(conn_id)
            .and_then(|c| c.environment.as_deref())
            .and_then(|e| envs.by_id(e))
            .is_some_and(|e| e.protected)
    }

    /// The first shown record at or after a time: `HH:MM[:SS]` today (in
    /// the view's time zone), or a full timestamp. Binary search over the
    /// parsed timestamps, which are in order for any real log.
    fn position_for_time(&self, text: &str) -> Option<usize> {
        let text = text.trim();
        let target = crate::parse::parse_timestamp(text).or_else(|| {
            let t = chrono::NaiveTime::parse_from_str(text, "%H:%M:%S")
                .or_else(|_| chrono::NaiveTime::parse_from_str(text, "%H:%M"))
                .ok()?;
            let today = if self.state.local_time {
                chrono::Local::now().date_naive()
            } else {
                chrono::Utc::now().date_naive()
            };
            let naive = today.and_time(t);
            Some(if self.state.local_time {
                chrono::TimeZone::from_local_datetime(&chrono::Local, &naive)
                    .single()?
                    .with_timezone(&chrono::Utc)
            } else {
                chrono::TimeZone::from_utc_datetime(&chrono::Utc, &naive)
            })
        })?;
        let sh = self.shared.lock();
        let timed: Vec<(usize, chrono::DateTime<chrono::Utc>)> = self
            .filtered
            .iter()
            .enumerate()
            .filter_map(|(pos, &i)| sh.records[i].ts.map(|t| (pos, t)))
            .collect();
        if timed.is_empty() {
            return None;
        }
        let at = timed.partition_point(|(_, t)| *t < target);
        timed.get(at).or(timed.last()).map(|(pos, _)| *pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_line_tokens_are_found() {
        let c = PATH_LINE
            .captures("    at com.shop.OrderSvc.charge(OrderSvc.java:212)")
            .unwrap();
        assert_eq!(c.get(1).unwrap().as_str(), "OrderSvc.java");
        assert_eq!(c.get(2).unwrap().as_str(), "212");
        let c = PATH_LINE
            .captures("error in src/main.rs:42: thing")
            .unwrap();
        assert_eq!(c.get(1).unwrap().as_str(), "src/main.rs");
        let c = PATH_LINE
            .captures(r#"  File "C:\app\run.py", line 7, in <module>"#)
            .unwrap();
        assert_eq!(c.get(1).unwrap().as_str(), r"C:\app\run.py");
        assert_eq!(c.get(2).unwrap().as_str(), "7");
        assert!(PATH_LINE.captures("no path here 12:30").is_none());
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(human_bytes(900), "900 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(1_900_000_000), "1.8 GB");
    }
}
