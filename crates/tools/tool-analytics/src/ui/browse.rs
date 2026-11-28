//! The browse column (docs/tools/analytics.md §4.2): the collection tree a
//! level at a time, a filter over what is loaded, and Search over the
//! server. Every read runs on the runtime and lands in an inbox drained at
//! frame start; nothing here blocks the UI thread.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use hs_core::Services;
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tree::{self, TreeIcon, TreeNode, TreeState};
use parking_lot::Mutex;

use crate::provider::{
    BiProvider, Database, FieldMeta, Found, Kind, Node, Result, SEARCH_PAGE, TableMeta,
};

/// The root collection's id, as the server spells it.
const ROOT: &str = "root";

/// The key of the databases read, in the same map as the collections (a
/// collection's id is a number or `root`, so it cannot collide).
const DATABASES: &str = "databases";

/// What one read of a collection has come to.
enum Loaded {
    Loading,
    Ready(Vec<Node>),
    Failed(String),
}

/// Where a search stands.
#[derive(Default)]
enum Search {
    #[default]
    Idle,
    Loading,
    Done(Found),
    Failed(String),
}

/// What the runtime's tasks write and the frame reads.
#[derive(Default)]
struct Inbox {
    children: Vec<(String, Result<Vec<Node>>)>,
    /// A search's answer, with the number of the search it answers (an
    /// older one is dropped) and the offset it began at.
    search: Option<(u64, usize, Result<Found>)>,
    /// The databases, and a database's tables (by the database's id).
    dbs: Option<Result<Vec<Database>>>,
    tables: Vec<(String, Result<Vec<TableMeta>>)>,
}

#[derive(Default)]
pub struct Browse {
    inbox: Arc<Mutex<Inbox>>,
    /// The heading of the data section (Databases, CAS Libraries).
    data_label: String,
    loaded: HashMap<String, Loaded>,
    /// Bumped whenever `loaded` changes, so the tree is rebuilt only then.
    version: u64,
    tree: TreeState,
    /// Narrows the loaded tree by name.
    filter: String,
    /// The tree built for `(version, filter)`.
    built: Option<(u64, String, Vec<TreeNode>)>,
    /// Search mode: the field, the kinds, the answer.
    pub searching: bool,
    query: String,
    kinds: HashSet<Kind>,
    search: Search,
    search_no: u64,
    /// Ask the search field for the keyboard on the next frame.
    focus_search: bool,
    /// The item picked in the tree or the results.
    pub selected: Option<Node>,
    /// Every node seen, by its tree id, so a click finds its node.
    seen: HashMap<String, Node>,
    /// The databases read, by id, and each table's columns by its node id.
    dbs: HashMap<String, Database>,
    fields: HashMap<String, Vec<FieldMeta>>,
}

/// The tree id of a node.
fn node_key(n: &Node) -> String {
    format!("{}:{}", n.kind.tag(), n.id)
}

fn glyph(k: Kind) -> &'static str {
    match k {
        Kind::Collection => ph::FOLDER,
        Kind::Question => ph::CHART_LINE,
        Kind::Model => ph::CUBE,
        Kind::Metric => ph::GAUGE,
        Kind::Dashboard => ph::SQUARES_FOUR,
        Kind::Database => ph::DATABASE,
        Kind::Table => ph::TABLE,
        Kind::Workbook => ph::BOOK_OPEN,
        Kind::View => ph::CHART_BAR,
        Kind::DataSource => ph::DATABASE,
        Kind::Extracts => ph::ARROWS_CLOCKWISE,
        Kind::Report => ph::CHART_BAR,
        Kind::PowerBi => ph::CHART_PIE_SLICE,
        Kind::Kpi => ph::GAUGE,
    }
}

/// The word `fileicons::for_kind` knows for a kind.
fn picture_word(k: Kind) -> &'static str {
    match k {
        Kind::Collection => "collection",
        Kind::Question => "saved-query",
        Kind::Model => "table",
        Kind::Metric => "field",
        Kind::Dashboard => "view",
        Kind::Database => "database",
        Kind::Table => "table",
        Kind::Workbook => "collection",
        Kind::View => "view",
        Kind::DataSource => "database",
        Kind::Extracts => "history",
        Kind::Report => "view",
        Kind::PowerBi => "view",
        Kind::Kpi => "field",
    }
}

/// The node of the Extracts and Jobs page, which is not a server item.
pub fn extracts_node(name: String) -> Node {
    Node {
        kind: Kind::Extracts,
        id: "extracts".into(),
        name,
        description: String::new(),
        collection: String::new(),
        edited: String::new(),
        editor: String::new(),
    }
}

fn placeholder(key: &str) -> TreeNode {
    TreeNode::leaf(
        format!("loading:{key}"),
        "…".to_string(),
        TreeIcon::Glyph(ph::SPINNER.to_string()),
    )
}

fn failure(key: &str, e: &str) -> TreeNode {
    TreeNode::leaf(
        format!("failed:{key}"),
        e.to_string(),
        TreeIcon::Glyph(ph::WARNING.to_string()),
    )
}

fn icon(material: bool, k: Kind) -> TreeIcon {
    hs_ui::fileicons::item_icon(material, picture_word(k), glyph(k))
}

impl Browse {
    /// Forget everything: a new session, or Refresh.
    pub fn reset(&mut self) {
        *self = Browse {
            searching: self.searching,
            ..Browse::default()
        };
    }

    /// Show `n` as the picked item, in the tree as well: a tile's title on a
    /// dashboard opens its question and the tree follows.
    pub fn select(&mut self, n: Node) {
        self.tree.selected = Some(node_key(&n));
        self.selected = Some(n);
    }

    /// Open search mode with the keyboard in its field.
    pub fn open_search(&mut self) {
        self.searching = true;
        self.focus_search = true;
    }

    /// Read a collection once; later calls while it is loading or loaded
    /// do nothing.
    fn request(&mut self, s: &Services, provider: &Arc<dyn BiProvider>, parent: &str) {
        if self.loaded.contains_key(parent) {
            return;
        }
        self.loaded.insert(parent.to_string(), Loaded::Loading);
        self.version += 1;
        let (p, inbox, id) = (provider.clone(), self.inbox.clone(), parent.to_string());
        s.runtime.spawn(async move {
            let r = p.children(&id).await;
            inbox.lock().children.push((id, r));
        });
    }

    /// The database with this id, once the list has been read.
    pub fn database(&self, id: &str) -> Option<&Database> {
        self.dbs.get(id)
    }

    /// The tables of database `db` read so far, with their columns; the read
    /// is begun when it has not been, so the answer fills in on a later frame.
    pub fn tables(
        &mut self,
        s: &Services,
        provider: &Arc<dyn BiProvider>,
        db: &str,
    ) -> Vec<TableMeta> {
        self.request_tables(s, provider, db);
        let Some(Loaded::Ready(nodes)) = self.loaded.get(&format!("db:{db}")) else {
            return Vec::new();
        };
        nodes
            .iter()
            .map(|n| TableMeta {
                id: n.id.clone(),
                schema: n.collection.clone(),
                name: n.name.clone(),
                fields: self.fields.get(&n.id).cloned().unwrap_or_default(),
            })
            .collect()
    }

    /// Read the databases if they have not been (a question needs its own
    /// database's details without the Databases section being opened).
    pub fn ensure_databases(&mut self, s: &Services, provider: &Arc<dyn BiProvider>) {
        self.request_databases(s, provider);
    }

    fn request_databases(&mut self, s: &Services, provider: &Arc<dyn BiProvider>) {
        if self.loaded.contains_key(DATABASES) {
            return;
        }
        self.loaded.insert(DATABASES.to_string(), Loaded::Loading);
        self.version += 1;
        let (p, inbox) = (provider.clone(), self.inbox.clone());
        s.runtime.spawn(async move {
            let r = p.databases().await;
            inbox.lock().dbs = Some(r);
        });
    }

    fn request_tables(&mut self, s: &Services, provider: &Arc<dyn BiProvider>, db: &str) {
        let key = format!("db:{db}");
        if self.loaded.contains_key(&key) {
            return;
        }
        self.loaded.insert(key, Loaded::Loading);
        self.version += 1;
        let (p, inbox, id) = (provider.clone(), self.inbox.clone(), db.to_string());
        s.runtime.spawn(async move {
            let r = p.tables(&id).await;
            inbox.lock().tables.push((id, r));
        });
    }

    fn run_search(&mut self, s: &Services, provider: &Arc<dyn BiProvider>, offset: usize) {
        let query = self.query.trim().to_string();
        if query.is_empty() {
            self.search = Search::Idle;
            return;
        }
        self.search_no += 1;
        let no = self.search_no;
        if offset == 0 {
            self.search = Search::Loading;
        }
        let offered = provider.capabilities().search_kinds;
        let mut kinds: Vec<Kind> = offered
            .iter()
            .copied()
            .filter(|k| self.kinds.contains(k))
            .collect();
        if kinds.is_empty() {
            kinds = offered.to_vec();
        }
        let (p, inbox) = (provider.clone(), self.inbox.clone());
        s.runtime.spawn(async move {
            let r = p.search(&query, &kinds, offset).await;
            inbox.lock().search = Some((no, offset, r));
        });
    }

    /// Apply what the runtime has delivered since the last frame.
    fn drain(&mut self) {
        let (children, search, dbs, tables) = {
            let mut g = self.inbox.lock();
            (
                std::mem::take(&mut g.children),
                g.search.take(),
                g.dbs.take(),
                std::mem::take(&mut g.tables),
            )
        };
        if let Some(r) = dbs {
            self.loaded.insert(
                DATABASES.to_string(),
                match r {
                    Ok(list) => {
                        let nodes = list
                            .iter()
                            .map(|d| Node {
                                kind: Kind::Database,
                                id: d.id.clone(),
                                name: d.name.clone(),
                                description: d.engine.clone(),
                                collection: String::new(),
                                edited: String::new(),
                                editor: String::new(),
                            })
                            .collect::<Vec<_>>();
                        for n in &nodes {
                            self.seen.insert(node_key(n), n.clone());
                        }
                        for d in list {
                            self.dbs.insert(d.id.clone(), d);
                        }
                        Loaded::Ready(nodes)
                    }
                    Err(e) => Loaded::Failed(e),
                },
            );
            self.version += 1;
        }
        for (db, r) in tables {
            let db_name = self
                .dbs
                .get(&db)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            let loaded = match r {
                Ok(list) => {
                    let mut nodes = Vec::with_capacity(list.len());
                    for t in list {
                        let n = Node {
                            kind: Kind::Table,
                            id: format!("{db}/{}", t.id),
                            name: t.name,
                            description: db_name.clone(),
                            collection: t.schema,
                            edited: String::new(),
                            editor: String::new(),
                        };
                        self.seen.insert(node_key(&n), n.clone());
                        self.fields.insert(n.id.clone(), t.fields);
                        nodes.push(n);
                    }
                    Loaded::Ready(nodes)
                }
                Err(e) => Loaded::Failed(e),
            };
            self.loaded.insert(format!("db:{db}"), loaded);
            self.version += 1;
        }
        for (parent, r) in children {
            if let Ok(nodes) = &r {
                for n in nodes {
                    self.seen.insert(node_key(n), n.clone());
                }
            }
            self.loaded.insert(
                parent,
                match r {
                    Ok(nodes) => Loaded::Ready(nodes),
                    Err(e) => Loaded::Failed(e),
                },
            );
            self.version += 1;
        }
        if let Some((no, offset, r)) = search
            && no == self.search_no
        {
            self.search = match (r, std::mem::take(&mut self.search)) {
                (Err(e), _) => Search::Failed(e),
                // More: the new page joins the ones already shown.
                (Ok(more), Search::Done(mut have)) if offset > 0 => {
                    have.nodes.extend(more.nodes);
                    have.total = more.total;
                    Search::Done(have)
                }
                (Ok(found), _) => Search::Done(found),
            };
            if let Search::Done(f) = &self.search {
                for n in &f.nodes {
                    self.seen.insert(node_key(n), n.clone());
                }
            }
        }
    }

    /// The tree for what is loaded, narrowed by the filter.
    fn build(&self, parent: &str, material: bool, needle: &str) -> Vec<TreeNode> {
        let Some(Loaded::Ready(nodes)) = self.loaded.get(parent) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(nodes.len());
        for n in nodes {
            let key = node_key(n);
            let own = needle.is_empty() || n.name.to_lowercase().contains(needle);
            if !n.kind.is_branch() {
                if own {
                    out.push(TreeNode::leaf(key, n.name.clone(), icon(material, n.kind)));
                }
                continue;
            }
            let mut node =
                TreeNode::branch(key, n.name.clone(), icon(material, n.kind), Vec::new());
            match self.loaded.get(&n.id) {
                None => node.children = None,
                Some(Loaded::Loading) => {
                    node.children = Some(vec![TreeNode::leaf(
                        format!("loading:{}", n.id),
                        "…".to_string(),
                        TreeIcon::Glyph(ph::SPINNER.to_string()),
                    )]);
                }
                Some(Loaded::Failed(e)) => {
                    node.children = Some(vec![TreeNode::leaf(
                        format!("failed:{}", n.id),
                        e.clone(),
                        TreeIcon::Glyph(ph::WARNING.to_string()),
                    )]);
                }
                Some(Loaded::Ready(_)) => {
                    node.children = Some(self.build(&n.id, material, needle));
                }
            }
            // With a filter, a collection stays for its own name or for what
            // matched inside it.
            let kept =
                needle.is_empty() || own || node.children.as_ref().is_some_and(|c| !c.is_empty());
            if kept {
                out.push(node);
            }
        }
        out
    }

    /// The Databases section: each database a level down, its tables a level
    /// below, each table's columns its leaves.
    fn databases_node(&self, material: bool) -> TreeNode {
        let mut node = TreeNode::branch(
            DATABASES,
            if self.data_label.is_empty() {
                "Databases".to_string()
            } else {
                self.data_label.clone()
            },
            icon(material, Kind::Database),
            Vec::new(),
        );
        node.children = match self.loaded.get(DATABASES) {
            None => None,
            Some(Loaded::Loading) => Some(vec![placeholder("databases")]),
            Some(Loaded::Failed(e)) => Some(vec![failure("databases", e)]),
            Some(Loaded::Ready(dbs)) => Some(
                dbs.iter()
                    .map(|d| {
                        let mut db = TreeNode::branch(
                            node_key(d),
                            d.name.clone(),
                            icon(material, Kind::Database),
                            Vec::new(),
                        );
                        db.children = match self.loaded.get(&format!("db:{}", d.id)) {
                            None => None,
                            Some(Loaded::Loading) => Some(vec![placeholder(&d.id)]),
                            Some(Loaded::Failed(e)) => Some(vec![failure(&d.id, e)]),
                            Some(Loaded::Ready(tables)) => Some(
                                tables
                                    .iter()
                                    .map(|t| self.table_node(t, material))
                                    .collect(),
                            ),
                        };
                        db
                    })
                    .collect(),
            ),
        };
        node
    }

    fn table_node(&self, t: &Node, material: bool) -> TreeNode {
        let label = if t.collection.is_empty() {
            t.name.clone()
        } else {
            format!("{}.{}", t.collection, t.name)
        };
        let fields = self
            .fields
            .get(&t.id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut node = TreeNode::branch(
            node_key(t),
            label,
            icon(material, Kind::Table),
            fields
                .iter()
                .map(|f| {
                    let mut leaf = TreeNode::leaf(
                        format!("field:{}:{}", t.id, f.name),
                        f.name.clone(),
                        TreeIcon::Glyph(ph::COLUMNS.to_string()),
                    );
                    leaf.badge = Some(f.ty.trim_start_matches("type/").to_string());
                    leaf
                })
                .collect(),
        );
        node.children = Some(node.children.take().unwrap_or_default());
        node
    }

    /// Draw the column's body. Returns the node picked this frame.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        provider: &Arc<dyn BiProvider>,
    ) -> Option<Node> {
        self.drain();
        let material = s.settings.read().appearance.material_icons("analytics");
        self.request(s, provider, ROOT);
        if self.searching {
            self.show_search(ui, s, theme, provider, material)
        } else {
            self.show_tree(ui, s, theme, provider, material)
        }
    }

    fn show_tree(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        provider: &Arc<dyn BiProvider>,
        material: bool,
    ) -> Option<Node> {
        form::text_field(ui, &mut self.filter, &s.t("analytics-filter"));
        ui.add_space(form::PANE_GAP);
        match self.loaded.get(ROOT) {
            Some(Loaded::Failed(e)) => {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e));
                ui.add_space(form::ROW_GAP);
                if form::button(ui, theme, ph::ARROWS_CLOCKWISE, &s.t("analytics-retry")).clicked()
                {
                    self.loaded.remove(ROOT);
                }
                return None;
            }
            None | Some(Loaded::Loading) => {
                form::centred_row(ui, |ui| {
                    ui.spinner();
                    ui.label(s.t("analytics-loading"));
                });
                return None;
            }
            Some(Loaded::Ready(n)) if n.is_empty() => {
                form::note(ui, theme, ph::LIGHTBULB, &s.t("analytics-tree-empty"));
                return None;
            }
            Some(Loaded::Ready(_)) => {}
        }
        let needle = self.filter.trim().to_lowercase();
        let stale = !matches!(&self.built,
            Some((v, f, _)) if *v == self.version && *f == needle);
        if stale {
            let mut nodes = self.build(ROOT, material, &needle);
            // The Databases section goes below the collections, unless a
            // filter is narrowing what is loaded; so does Extracts and Jobs.
            let caps = provider.capabilities();
            if !caps.data_section.is_empty() {
                self.data_label = s.t(caps.data_section);
            }
            if needle.is_empty() && caps.tables {
                nodes.push(self.databases_node(material));
            }
            if needle.is_empty() && (caps.extracts || caps.jobs) {
                let key = if caps.extracts {
                    caps.extracts_keys[0]
                } else {
                    "analytics-kind-jobs"
                };
                let n = extracts_node(s.t(key));
                nodes.push(TreeNode::leaf(
                    node_key(&n),
                    n.name.clone(),
                    icon(material, Kind::Extracts),
                ));
                self.seen.insert(node_key(&n), n);
            }
            if !needle.is_empty() {
                // A match inside a collection is shown, so open its way.
                open_branches(&nodes, &mut self.tree.expanded);
            }
            self.built = Some((self.version, needle.clone(), nodes));
        }
        let Some((_, _, nodes)) = &self.built else {
            return None;
        };
        if nodes.is_empty() {
            form::note(ui, theme, ph::LIGHTBULB, &s.t("analytics-filter-empty"));
            return None;
        }
        let resp = tree::show(ui, theme, &mut self.tree, nodes);
        let mut picked = None;
        for id in resp.load {
            if let Some(coll) = id.strip_prefix("collection:") {
                self.request(s, provider, coll);
            } else if id == DATABASES {
                self.request_databases(s, provider);
            } else if let Some(wb) = id.strip_prefix("workbook:") {
                self.request(s, provider, wb);
            } else if let Some(db) = id.strip_prefix("database:") {
                self.request_tables(s, provider, db);
            }
        }
        if let Some(key) = resp.clicked
            && let Some(n) = self.seen.get(&key)
        {
            picked = Some(n.clone());
        }
        if let Some(n) = &picked {
            self.selected = Some(n.clone());
        }
        picked
    }

    fn show_search(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        provider: &Arc<dyn BiProvider>,
        material: bool,
    ) -> Option<Node> {
        let mut go = false;
        form::centred_row_wrapped(ui, |ui| {
            // The field leaves the button its whole label and the gaps.
            let button = form::button_width(ui, ph::MAGNIFYING_GLASS, &s.t("analytics-search"));
            let width =
                (ui.available_width() - button - ui.spacing().item_spacing.x * 2.0).max(80.0);
            let r =
                form::text_field_width(ui, &mut self.query, &s.t("analytics-search-hint"), width);
            if std::mem::take(&mut self.focus_search) {
                r.request_focus();
            }
            go |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            go |= form::button(ui, theme, ph::MAGNIFYING_GLASS, &s.t("analytics-search")).clicked();
        });
        ui.add_space(form::PANE_GAP);
        // Kind filters: none chosen means every kind.
        let mut kinds_changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            for &k in provider.capabilities().search_kinds {
                let mut on = self.kinds.contains(&k);
                let label = format!(
                    "{} {}",
                    glyph(k),
                    s.t(&format!("analytics-kinds-{}", k.tag()))
                );
                if form::toggle_value(ui, &mut on, label).changed() {
                    if on {
                        self.kinds.insert(k);
                    } else {
                        self.kinds.remove(&k);
                    }
                    kinds_changed = true;
                }
            }
        });
        if go || (kinds_changed && !self.query.trim().is_empty()) {
            self.run_search(s, provider, 0);
        }
        ui.add_space(form::ROW_GAP);
        let mut picked = None;
        let mut more = false;
        match &self.search {
            Search::Idle => form::hint(ui, theme, &s.t("analytics-search-idle")),
            Search::Loading => {
                form::centred_row(ui, |ui| {
                    ui.spinner();
                    ui.label(s.t("analytics-searching"));
                });
            }
            Search::Failed(e) => form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e)),
            Search::Done(f) if f.nodes.is_empty() => {
                form::note(ui, theme, ph::LIGHTBULB, &s.t("analytics-search-none"));
            }
            Search::Done(f) => {
                ui.label(s.i18n.t_args(
                    "analytics-search-count",
                    &[
                        ("shown", &f.nodes.len().to_string()),
                        ("total", &f.total.to_string()),
                    ],
                ));
                ui.add_space(form::PANE_GAP);
                egui::ScrollArea::vertical()
                    .id_salt("analytics-results")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for n in &f.nodes {
                            if result_row(ui, theme, material, n, self.selected.as_ref()).clicked()
                            {
                                picked = Some(n.clone());
                            }
                        }
                        if f.nodes.len() < f.total
                            && form::button(ui, theme, ph::PLUS, &s.t("analytics-search-more"))
                                .clicked()
                        {
                            more = true;
                        }
                    });
            }
        }
        if more && let Search::Done(f) = &self.search {
            let offset = f.nodes.len().div_ceil(SEARCH_PAGE) * SEARCH_PAGE;
            self.run_search(s, provider, offset);
        }
        if let Some(n) = &picked {
            self.selected = Some(n.clone());
        }
        picked
    }
}

/// One search result: its picture and name, the collection under them.
fn result_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    material: bool,
    n: &Node,
    selected: Option<&Node>,
) -> egui::Response {
    let on = selected.is_some_and(|x| x.kind == n.kind && x.id == n.id);
    let h = 40.0;
    let (rect, r) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
    if on {
        ui.painter()
            .rect_filled(rect, 3.0, theme.accent.linear_multiply(0.3));
    } else if r.hovered() {
        ui.painter().rect_filled(rect, 3.0, theme.surface_hover);
    }
    let mid = rect.center().y;
    match icon(material, n.kind) {
        TreeIcon::Glyph(g) => {
            ui.painter().text(
                egui::pos2(rect.left() + 10.0, mid),
                egui::Align2::LEFT_CENTER,
                g,
                egui::FontId::proportional(15.0),
                theme.text_muted,
            );
        }
        TreeIcon::Image(src) => {
            let at = egui::Rect::from_center_size(
                egui::pos2(rect.left() + 18.0, mid),
                egui::Vec2::splat(18.0),
            );
            egui::Image::new(src)
                .fit_to_exact_size(egui::Vec2::splat(18.0))
                .paint_at(ui, at);
        }
    }
    let x = rect.left() + 34.0;
    let room = (rect.right() - x - 8.0).max(20.0);
    paint_elided(
        ui,
        egui::pos2(x, mid - 8.0),
        &n.name,
        13.0,
        theme.text,
        room,
    );
    let under = if n.collection.is_empty() {
        n.kind.tag().to_string()
    } else {
        n.collection.clone()
    };
    paint_elided(
        ui,
        egui::pos2(x, mid + 9.0),
        &under,
        11.0,
        theme.text_muted,
        room,
    );
    r
}

/// One line of user data, cut with an ellipsis at `room` instead of
/// running under the column's edge.
fn paint_elided(
    ui: &egui::Ui,
    left_centre: egui::Pos2,
    text: &str,
    size: f32,
    colour: egui::Color32,
    room: f32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_string(),
        egui::FontId::proportional(size),
        colour,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let galley = ui.painter().layout_job(job);
    let pos = egui::pos2(left_centre.x, left_centre.y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, colour);
}

/// Insert the id of every branch that has children shown.
fn open_branches(nodes: &[TreeNode], open: &mut HashSet<String>) {
    for n in nodes {
        if let Some(c) = &n.children
            && !c.is_empty()
        {
            open.insert(n.id.clone());
            open_branches(c, open);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: Kind, id: &str, name: &str) -> Node {
        Node {
            kind,
            id: id.into(),
            name: name.into(),
            description: String::new(),
            collection: String::new(),
            edited: String::new(),
            editor: String::new(),
        }
    }

    /// root: [Sales (4): [Revenue question, EU (5)], Overview dashboard]
    fn sample() -> Browse {
        let mut b = Browse::default();
        b.loaded.insert(
            ROOT.into(),
            Loaded::Ready(vec![
                node(Kind::Collection, "4", "Sales"),
                node(Kind::Dashboard, "9", "Overview"),
            ]),
        );
        b.loaded.insert(
            "4".into(),
            Loaded::Ready(vec![
                node(Kind::Collection, "5", "EU"),
                node(Kind::Question, "7", "Revenue"),
            ]),
        );
        b
    }

    #[test]
    fn an_unread_collection_has_no_children_yet_and_a_read_one_has() {
        let mut b = sample();
        b.loaded.remove("4");
        let t = b.build(ROOT, false, "");
        assert_eq!(t[0].id, "collection:4");
        assert!(t[0].children.is_none(), "caret, children on expand");
        assert!(t[1].leaf);
        let t = sample().build(ROOT, false, "");
        assert_eq!(t[0].children.as_ref().map(Vec::len), Some(2));
    }

    #[test]
    fn a_filter_keeps_a_match_and_the_collections_around_it() {
        let t = sample().build(ROOT, false, "rev");
        assert_eq!(t.len(), 1, "Overview is not a match");
        assert_eq!(t[0].id, "collection:4");
        let inside = t[0].children.as_ref().expect("loaded");
        assert_eq!(inside.len(), 1);
        assert_eq!(inside[0].id, "question:7");
        assert!(sample().build(ROOT, false, "zzz").is_empty());
    }

    #[test]
    fn a_collection_still_loading_shows_a_placeholder_and_a_failed_one_its_error() {
        let mut b = sample();
        b.loaded.insert("4".into(), Loaded::Loading);
        let t = b.build(ROOT, false, "");
        assert_eq!(
            t[0].children.as_ref().map(|c| c[0].id.as_str()),
            Some("loading:4")
        );
        b.loaded
            .insert("4".into(), Loaded::Failed("no access".into()));
        let t = b.build(ROOT, false, "");
        assert_eq!(
            t[0].children.as_ref().map(|c| c[0].label.as_str()),
            Some("no access")
        );
    }

    #[test]
    fn the_databases_section_loads_a_level_at_a_time() {
        let mut b = Browse::default();
        // Nothing read: a caret, children on expand.
        assert!(b.databases_node(false).children.is_none());
        let db = Node {
            kind: Kind::Database,
            id: "1".into(),
            name: "Sample".into(),
            description: "h2".into(),
            collection: String::new(),
            edited: String::new(),
            editor: String::new(),
        };
        b.loaded.insert(DATABASES.into(), Loaded::Ready(vec![db]));
        let section = b.databases_node(false);
        let dbs = section.children.as_ref().expect("the databases");
        assert_eq!(dbs[0].id, "database:1");
        assert!(dbs[0].children.is_none(), "its tables are not read yet");
        // Tables read: each a branch of its columns, typed.
        let table = Node {
            kind: Kind::Table,
            id: "1/9".into(),
            name: "ORDERS".into(),
            description: "Sample".into(),
            collection: "PUBLIC".into(),
            edited: String::new(),
            editor: String::new(),
        };
        b.fields.insert(
            "1/9".into(),
            vec![FieldMeta {
                name: "TOTAL".into(),
                ty: "type/Float".into(),
            }],
        );
        b.loaded.insert("db:1".into(), Loaded::Ready(vec![table]));
        let section = b.databases_node(false);
        let t = &section.children.as_ref().expect("dbs")[0]
            .children
            .as_ref()
            .expect("tables")[0];
        assert_eq!(
            (t.id.as_str(), t.label.as_str()),
            ("table:1/9", "PUBLIC.ORDERS")
        );
        let column = &t.children.as_ref().expect("columns")[0];
        assert_eq!(column.label, "TOTAL");
        assert_eq!(column.badge.as_deref(), Some("Float"), "type/ is not shown");
    }

    #[test]
    fn a_database_that_failed_to_read_says_so_in_its_place() {
        let mut b = Browse::default();
        b.loaded
            .insert(DATABASES.into(), Loaded::Failed("no access".into()));
        let section = b.databases_node(false);
        let kids = section.children.as_ref().expect("a placeholder");
        assert_eq!(kids[0].label, "no access");
    }

    #[test]
    fn selecting_an_item_moves_the_trees_highlight_with_it() {
        let mut b = sample();
        b.tree.selected = Some("dashboard:9".into());
        b.select(node(Kind::Question, "7", "Revenue"));
        assert_eq!(b.tree.selected.as_deref(), Some("question:7"));
        assert_eq!(b.selected.as_ref().map(|n| n.id.as_str()), Some("7"));
    }

    #[test]
    fn an_older_searchs_answer_is_dropped() {
        let mut b = Browse {
            search_no: 2,
            search: Search::Loading,
            ..Browse::default()
        };
        b.inbox.lock().search = Some((1, 0, Ok(Found::default())));
        b.drain();
        assert!(
            matches!(b.search, Search::Loading),
            "the answer to search 1 is stale"
        );
        b.inbox.lock().search = Some((2, 0, Ok(Found::default())));
        b.drain();
        assert!(matches!(b.search, Search::Done(_)));
    }

    #[test]
    fn more_results_join_the_page_already_shown() {
        let mut b = Browse {
            search_no: 3,
            search: Search::Done(Found {
                nodes: vec![node(Kind::Question, "1", "a")],
                total: 3,
            }),
            ..Browse::default()
        };
        b.inbox.lock().search = Some((
            3,
            1,
            Ok(Found {
                nodes: vec![node(Kind::Question, "2", "b")],
                total: 3,
            }),
        ));
        b.drain();
        let Search::Done(f) = &b.search else {
            panic!("done");
        };
        assert_eq!(f.nodes.len(), 2);
    }
}
