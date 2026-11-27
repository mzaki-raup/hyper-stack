//! The provider model (docs/tools/analytics.md §4.1): one trait the
//! screens talk to, one module per product. What a product cannot do is
//! said through [`Caps`], never by hiding a section.

use async_trait::async_trait;

pub mod metabase;

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
}

/// What a node of the browse tree or a search result is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Collection,
    Question,
    Model,
    Metric,
    Dashboard,
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
        }
    }

    pub fn from_tag(tag: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.tag() == tag)
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
    async fn card(&self, id: &str) -> Result<Card>;
    /// Run it with the parameter `values` (by parameter id; an empty value
    /// is left out), reading at most `limit` rows.
    async fn run(&self, card: &Card, values: &[(String, String)], limit: usize) -> Result<Table>;
    /// The result as a file, through the server's own export (its limit
    /// applies, not the run limit).
    async fn export(
        &self,
        card: &Card,
        values: &[(String, String)],
        format: ExportFormat,
    ) -> Result<Vec<u8>>;
    /// The server's own web interface, for Open in Browser.
    fn web_url(&self) -> String;
    /// End the server-side session, where there is one.
    async fn disconnect(&self);
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
