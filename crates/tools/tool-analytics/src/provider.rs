//! The provider model (docs/tools/analytics.md §4.1): one trait the
//! screens talk to, one module per product. What a product cannot do is
//! said through [`Caps`], never by hiding a section.

use async_trait::async_trait;

pub mod metabase;
pub mod powerbi;
pub mod sasviya;
pub mod superset;
pub mod tableau;

use std::sync::Arc;

use crate::profile::AnalyticsFields;

/// What a provider says when asked for something its product does not have.
pub const NOT_OFFERED: &str = "This server's product does not offer that.";

/// The provider for a saved server of `kind`, from its resolved profile.
pub fn open(
    kind: &str,
    fields: &AnalyticsFields,
    secrets: &dyn hs_core::secrets::SecretStore,
    lookup: &dyn Fn(&str) -> std::result::Result<String, Vec<String>>,
    timeout: std::time::Duration,
) -> std::result::Result<Arc<dyn BiProvider>, crate::profile::ProfileError> {
    use crate::profile::ProfileError;
    match kind {
        "tableau" => {
            let mut cfg = fields.resolve_tableau(secrets, lookup)?;
            cfg.timeout = timeout;
            tableau::Tableau::new(cfg)
                .map(|p| Arc::new(p) as Arc<dyn BiProvider>)
                .map_err(ProfileError::Invalid)
        }
        "powerbi" => {
            let mut cfg = fields.resolve_powerbi(secrets, lookup)?;
            cfg.timeout = timeout;
            powerbi::PowerBi::new(cfg)
                .map(|p| Arc::new(p) as Arc<dyn BiProvider>)
                .map_err(ProfileError::Invalid)
        }
        "superset" => {
            let mut cfg = fields.resolve_superset(secrets, lookup)?;
            cfg.timeout = timeout;
            superset::Superset::new(cfg)
                .map(|p| Arc::new(p) as Arc<dyn BiProvider>)
                .map_err(ProfileError::Invalid)
        }
        "sasviya" => {
            let mut cfg = fields.resolve_sasviya(secrets, lookup)?;
            cfg.timeout = timeout;
            sasviya::SasViya::new(cfg)
                .map(|p| Arc::new(p) as Arc<dyn BiProvider>)
                .map_err(ProfileError::Invalid)
        }
        _ => {
            let mut cfg = fields.resolve(secrets, lookup)?;
            cfg.timeout = timeout;
            metabase::Metabase::new(cfg)
                .map(|p| Arc::new(p) as Arc<dyn BiProvider>)
                .map_err(ProfileError::Invalid)
        }
    }
}

/// Errors are sentences (`hs_core::service_http`): they reach the tab as
/// they are.
pub type Result<T> = std::result::Result<T, String>;

/// The oldest Metabase this tab supports (§8.3): API keys arrived in 49,
/// dashboard tabs in 47, and the card query paths are as they are from 50.
pub const OLDEST_MINOR: u32 = 48;

/// The newest Metabase this tab has been tried against. A newer one is
/// tried all the same and its header says so.
pub const NEWEST_TESTED_MINOR: u32 = 52;

/// The first Metabase that takes an API key.
pub const API_KEYS_FROM_MINOR: u32 = 49;

/// A Metabase version tag: `v0.52.4` is the open-source build, `v1.52.4`
/// the Enterprise build; the second number is the release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub edition: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// `v0.52.4`, `1.52.4` or `v0.52` (a missing patch reads as 0); `None`
    /// for anything else, such as `vLOCAL_DEV`.
    pub fn parse(tag: &str) -> Option<Version> {
        let mut parts = tag.trim().trim_start_matches('v').split('.');
        let mut next = || parts.next()?.trim().parse::<u32>().ok();
        let edition = next()?;
        let minor = next()?;
        let patch = next().unwrap_or(0);
        Some(Version {
            edition,
            minor,
            patch,
        })
    }

    pub fn support(self) -> Support {
        if self.minor < OLDEST_MINOR {
            Support::TooOld
        } else if self.minor > NEWEST_TESTED_MINOR {
            Support::Newer
        } else {
            Support::Supported
        }
    }
}

/// Whether the server's version is one this tab has been built against.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Support {
    /// The server did not say.
    #[default]
    Unknown,
    Supported,
    TooOld,
    Newer,
}

/// What a session learned when it connected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {
    pub product: String,
    /// The version as the server wrote it ("v0.52.4").
    pub version_text: String,
    pub version: Option<Version>,
    /// The account's name or email; an API key shows the key's user.
    pub signed_in_as: String,
    pub support: Support,
}

/// What a product can do, for the screens to enable or disable with a
/// reason.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    /// Collections, questions and dashboards can be browsed.
    pub browse: bool,
    /// Ad-hoc SQL can be run through the server.
    pub native_sql: bool,
    /// Views have a picture and data (Tableau).
    pub views: bool,
    /// Extract refresh tasks and jobs can be read.
    pub extracts: bool,
    /// The kinds Search can be narrowed to, in the order the chips show.
    pub search_kinds: &'static [Kind],
    /// Tables can be browsed under a section of their own: a Metabase
    /// database's, a SAS Viya CAS library's.
    pub tables: bool,
    /// The Fluent key of that section's heading.
    pub data_section: &'static str,
    /// Background jobs can be read.
    pub jobs: bool,
    /// A view has data as well as a picture.
    pub view_data: bool,
    /// The files a view can be saved as.
    pub view_files: &'static [ViewFile],
    /// A view takes filter fields (`vf_<field>=value`).
    pub view_filters: bool,
    /// KPIs can be read (Power BI Report Server).
    pub kpis: bool,
    /// The Fluent keys of the extract tasks page: its title, its one line,
    /// its table's heading and its empty sentence.
    pub extracts_keys: [&'static str; 4],
}

/// What a node of the browse tree or a search result is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Collection,
    Question,
    Model,
    Metric,
    Dashboard,
    /// A database the server queries, and one of its tables: the Databases
    /// section's entries, never search results.
    Database,
    Table,
    /// Tableau: a workbook (its views are its children), a view, a data
    /// source, and the one page that lists extract tasks and jobs.
    Workbook,
    View,
    DataSource,
    Extracts,
    /// SAS Viya and Power BI Report Server: a report.
    Report,
    /// Power BI Report Server: a `.pbix` report (details only) and a KPI.
    PowerBi,
    Kpi,
}

impl Kind {
    /// The kinds a search can be narrowed to, in the order the chips show.
    pub const ALL: [Kind; 5] = [
        Kind::Question,
        Kind::Model,
        Kind::Metric,
        Kind::Dashboard,
        Kind::Collection,
    ];

    /// A stable word for ids and Fluent keys.
    pub fn tag(self) -> &'static str {
        match self {
            Kind::Collection => "collection",
            Kind::Question => "question",
            Kind::Model => "model",
            Kind::Metric => "metric",
            Kind::Dashboard => "dashboard",
            Kind::Database => "database",
            Kind::Table => "table",
            Kind::Workbook => "workbook",
            Kind::View => "view",
            Kind::DataSource => "datasource",
            Kind::Extracts => "extracts",
            Kind::Report => "report",
            Kind::PowerBi => "powerbi",
            Kind::Kpi => "kpi",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Kind> {
        Kind::ALL
            .into_iter()
            .chain([
                Kind::Workbook,
                Kind::View,
                Kind::DataSource,
                Kind::Report,
                Kind::PowerBi,
                Kind::Kpi,
            ])
            .find(|k| k.tag() == tag)
    }

    /// Whether the browse tree opens this kind to show its children.
    pub fn is_branch(self) -> bool {
        matches!(self, Kind::Collection | Kind::Workbook)
    }
}

/// One entry of a collection, or one search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: Kind,
    /// The server's id (a number as text, or `root`).
    pub id: String,
    pub name: String,
    pub description: String,
    /// The collection it sits in, where the server says (search results).
    pub collection: String,
    /// When it was last changed and by whom, as the server wrote them.
    pub edited: String,
    pub editor: String,
}

/// One page of search results.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub nodes: Vec<Node>,
    /// How many the server has in all, so the screens can offer More.
    pub total: usize,
}

/// What a column holds, as far as the screens format and align it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    Integer,
    Decimal,
    Text,
    /// A date or a date and time, as the server wrote it.
    Time,
    Bool,
    Other,
}

impl ColumnKind {
    pub fn is_number(self) -> bool {
        matches!(self, ColumnKind::Integer | ColumnKind::Decimal)
    }
}

/// One column of a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// The name the query gave it (what a chart's settings refer to).
    pub name: String,
    /// The title the server shows.
    pub title: String,
    pub kind: ColumnKind,
}

/// A question's result: its columns and rows, read once and kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<serde_json::Value>>,
    /// The SQL the server ran, where it says (a native question's own, or
    /// what a query-builder question compiles to).
    pub sql: Option<String>,
    pub millis: u64,
    /// More rows existed than the run limit let through.
    pub truncated: bool,
}

/// What a parameter's field is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Text,
    Number,
    Date,
}

/// A question's parameter: a native question's template tag or a filter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub id: String,
    pub name: String,
    /// The server's type word (`number/=`, `date/single`, `category`).
    pub ty: String,
    pub kind: ParamKind,
    pub default: String,
    /// Where the value goes in the query, handed back as the server gave it.
    pub target: serde_json::Value,
}

/// A saved question, model or metric.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub description: String,
    /// The server's chart word (`table`, `line`, `pie`, ...).
    pub display: String,
    /// The question's own SQL when it is a native question.
    pub native_sql: Option<String>,
    /// The id of the database the question queries.
    pub database: String,
    pub params: Vec<Param>,
    /// The server's `visualization_settings`, read by the chart code.
    pub viz: serde_json::Value,
}

/// What an export is saved as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    Xlsx,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 3] = [ExportFormat::Csv, ExportFormat::Json, ExportFormat::Xlsx];

    pub fn ext(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
            ExportFormat::Xlsx => "xlsx",
        }
    }
}

/// A database the server queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Database {
    pub id: String,
    pub name: String,
    /// The driver (`postgres`, `h2`, ...).
    pub engine: String,
    /// Whether this account may run SQL on it.
    pub native: bool,
    /// Where it lives, as the server's admins see it; empty for anyone else.
    /// Read to find the saved Database connection that is the same server.
    pub host: String,
    pub port: String,
    pub dbname: String,
}

/// One column of a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMeta {
    pub name: String,
    /// The type as the server states it (`type/Integer`), or the database's.
    pub ty: String,
}

/// A table of a database with its columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableMeta {
    pub id: String,
    pub schema: String,
    pub name: String,
    pub fields: Vec<FieldMeta>,
}

/// A dashboard's tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashTab {
    pub id: String,
    pub name: String,
}

/// A dashboard filter.
#[derive(Debug, Clone, PartialEq)]
pub struct DashParam {
    pub id: String,
    pub name: String,
    /// The server's type word (`string/=`, `date/single`, `number/=`).
    pub ty: String,
    pub kind: ParamKind,
    pub default: String,
}

/// Which of a card's own fields a dashboard filter writes to.
#[derive(Debug, Clone, PartialEq)]
pub struct Mapping {
    pub param_id: String,
    pub target: serde_json::Value,
}

/// What a dashboard tile holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// A saved question, with the dashboard's overrides of its settings
    /// merged in, and the filters that reach it.
    Card {
        card: Box<Card>,
        mappings: Vec<Mapping>,
    },
    /// A text card's Markdown, or a heading.
    Text { text: String, heading: bool },
    /// A link card.
    Link { title: String, url: String },
}

/// One tile on the 24-column grid.
#[derive(Debug, Clone, PartialEq)]
pub struct DashCard {
    /// The dashboard card's own id (not the question's).
    pub id: String,
    pub tab: Option<String>,
    pub row: u32,
    pub col: u32,
    pub size_x: u32,
    pub size_y: u32,
    pub content: Content,
}

/// A Tableau view's facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewInfo {
    pub id: String,
    pub name: String,
    pub workbook: String,
    pub project: String,
    pub updated: String,
}

/// What a view is read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewFile {
    /// The picture, as PNG.
    Image,
    Csv,
    Pdf,
}

impl ViewFile {
    pub const ALL: [ViewFile; 3] = [ViewFile::Image, ViewFile::Csv, ViewFile::Pdf];

    pub fn ext(self) -> &'static str {
        match self {
            ViewFile::Image => "png",
            ViewFile::Csv => "csv",
            ViewFile::Pdf => "pdf",
        }
    }
}

/// One extract refresh task, with its schedule and last result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractTask {
    pub id: String,
    /// `FullRefresh` or `IncrementalRefresh`.
    pub kind: String,
    /// `Workbook` or `Data Source`, and its name where the server's lists
    /// give it.
    pub target_kind: String,
    pub target: String,
    pub schedule: String,
    pub frequency: String,
    /// Refreshes that failed in a row.
    pub failed: u32,
    /// When it last ran and what it said, where the server tells (Power BI
    /// Report Server's refresh plans).
    pub last_run: String,
    pub last_status: String,
}

/// A KPI: its value against its goal, how it stands, and its trend.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Kpi {
    pub id: String,
    pub name: String,
    pub value: f64,
    pub goal: f64,
    /// `-1` bad, `0` neutral, `1` good, as the server writes it.
    pub status: i32,
    pub trend: Vec<f64>,
    /// How the value is written (`Currency`, `Integer`, ...).
    pub format: String,
}

/// One background job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobInfo {
    pub id: String,
    pub kind: String,
    pub progress: u32,
    /// `None` while it runs, else whether it succeeded.
    pub ok: Option<bool>,
    pub started: String,
    pub completed: String,
    /// What the server said about a failure.
    pub notes: String,
}

/// A dashboard: its grid, tabs and filters.
#[derive(Debug, Clone, PartialEq)]
pub struct Dashboard {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tabs: Vec<DashTab>,
    pub params: Vec<DashParam>,
    pub cards: Vec<DashCard>,
}

/// Search results a page holds.
pub const SEARCH_PAGE: usize = 50;

#[async_trait]
pub trait BiProvider: Send + Sync {
    /// Sign in and read the server's facts. Called once per session.
    async fn connect(&self) -> Result<ServerInfo>;
    fn capabilities(&self) -> Caps;
    /// The entries of one collection (`root` for the top): collections
    /// first, then the rest, each by name; archived items left out.
    async fn children(&self, collection: &str) -> Result<Vec<Node>>;
    /// Search the server for `query` among `kinds` (all when empty),
    /// `SEARCH_PAGE` results from `offset`.
    async fn search(&self, query: &str, kinds: &[Kind], offset: usize) -> Result<Found>;
    /// The item's page in the server's own interface.
    fn item_url(&self, kind: Kind, id: &str) -> String;
    /// A question, model or metric with its parameters and chart settings.
    async fn card(&self, _id: &str) -> Result<Card> {
        Err(NOT_OFFERED.into())
    }
    /// Run it with the parameter `values` (by parameter id; an empty value
    /// is left out), reading at most `limit` rows.
    async fn run(
        &self,
        _card: &Card,
        _values: &[(String, String)],
        _limit: usize,
    ) -> Result<Table> {
        Err(NOT_OFFERED.into())
    }
    /// The databases the server queries.
    async fn databases(&self) -> Result<Vec<Database>> {
        Err(NOT_OFFERED.into())
    }
    /// A database's tables with their columns.
    async fn tables(&self, _database: &str) -> Result<Vec<TableMeta>> {
        Err(NOT_OFFERED.into())
    }
    /// Run `sql` on a database as the signed-in account, reading at most
    /// `limit` rows. Nothing is saved on the server.
    async fn run_sql(&self, _database: &str, _sql: &str, _limit: usize) -> Result<Table> {
        Err(NOT_OFFERED.into())
    }
    /// A dashboard with its grid, tabs, filters and cards.
    async fn dashboard(&self, _id: &str) -> Result<Dashboard> {
        Err(NOT_OFFERED.into())
    }
    /// Run one tile of a dashboard with the filter `values` (by filter id).
    async fn run_dashcard(
        &self,
        _dashboard: &Dashboard,
        _card: &DashCard,
        _values: &[(String, String)],
        _limit: usize,
    ) -> Result<Table> {
        Err(NOT_OFFERED.into())
    }
    /// The values a filter offers, where the server lists them.
    async fn filter_values(&self, _dashboard: &str, _param: &str) -> Result<Vec<String>> {
        Err(NOT_OFFERED.into())
    }
    /// The result as a file, through the server's own export (its limit
    /// applies, not the run limit).
    async fn export(
        &self,
        _card: &Card,
        _values: &[(String, String)],
        _format: ExportFormat,
    ) -> Result<Vec<u8>> {
        Err(NOT_OFFERED.into())
    }
    /// The server's own web interface, for Open in Browser.
    fn web_url(&self) -> String;
    /// End the server-side session, where there is one.
    async fn disconnect(&self);

    // What only some products have. Each returns [`NOT_OFFERED`] unless the
    // product overrides it, and `capabilities` says which do.

    /// A view's facts.
    async fn view(&self, _id: &str) -> Result<ViewInfo> {
        Err(NOT_OFFERED.into())
    }
    /// A view as a picture, a CSV or a PDF, with the filter `values` (field,
    /// value) applied. `width` is the picture's width in pixels.
    async fn view_file(
        &self,
        _id: &str,
        _file: ViewFile,
        _width: u32,
        _values: &[(String, String)],
    ) -> Result<Vec<u8>> {
        Err(NOT_OFFERED.into())
    }
    /// A view's data, at most `limit` rows.
    async fn view_data(
        &self,
        _id: &str,
        _values: &[(String, String)],
        _limit: usize,
    ) -> Result<Table> {
        Err(NOT_OFFERED.into())
    }
    /// The first rows of a table of a database (a CAS library's table), at
    /// most `limit`.
    async fn preview(&self, _database: &str, _table: &str, _limit: usize) -> Result<Table> {
        Err(NOT_OFFERED.into())
    }
    /// A KPI's value, goal, status and trend.
    async fn kpi(&self, _id: &str) -> Result<Kpi> {
        Err(NOT_OFFERED.into())
    }
    /// The extract refresh tasks.
    async fn extracts(&self) -> Result<Vec<ExtractTask>> {
        Err(NOT_OFFERED.into())
    }
    /// The recent background jobs.
    async fn jobs(&self) -> Result<Vec<JobInfo>> {
        Err(NOT_OFFERED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_metabase_tag_is_parsed_for_both_editions() {
        assert_eq!(
            Version::parse("v0.52.4"),
            Some(Version {
                edition: 0,
                minor: 52,
                patch: 4
            })
        );
        assert_eq!(Version::parse("v1.49.1").map(|v| v.edition), Some(1));
        assert_eq!(Version::parse("1.50.0").map(|v| v.minor), Some(50));
        assert_eq!(Version::parse("v0.52").map(|v| v.patch), Some(0));
        assert_eq!(Version::parse("vLOCAL_DEV"), None);
        assert_eq!(Version::parse(""), None);
    }

    #[test]
    fn the_version_decides_the_support_chip() {
        let at = |minor| Version {
            edition: 0,
            minor,
            patch: 0,
        };
        assert_eq!(at(OLDEST_MINOR - 1).support(), Support::TooOld);
        assert_eq!(at(OLDEST_MINOR).support(), Support::Supported);
        assert_eq!(at(NEWEST_TESTED_MINOR).support(), Support::Supported);
        assert_eq!(at(NEWEST_TESTED_MINOR + 1).support(), Support::Newer);
    }
}
