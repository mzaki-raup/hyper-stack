//! SAS Viya through its REST services (docs/tools/analytics.md §4.9, §5.3),
//! read-only. SAS Logon's OAuth password grant through a registered client
//! (the token renewed with the refresh token while the tab is open), or an
//! access token pasted in. The interactive report is SAS Visual Analytics'
//! own to draw: a report here is its picture from the Report Images
//! service, and Open Interactive leads to the Browser.
//!
//! Viya has no one version call, so `connect` probes each service once and
//! a service that answers 404 is left out of the capabilities with the
//! reason said in the header. The endpoints are from SAS's REST API reference
//! and the bench mock; they are unconfirmed against a real Viya.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use hs_core::service_auth::{Auth, TokenAs};
use hs_core::service_http::{Http, s, seg};
use parking_lot::RwLock;
use reqwest::Method;
use serde_json::{Value, json};

use super::{
    BiProvider, Caps, Column, ColumnKind, Database, FieldMeta, Found, JobInfo, Kind, Node, Result,
    SEARCH_PAGE, ServerInfo, Support, Table, TableMeta, ViewFile, ViewInfo,
};
use crate::profile::SasViyaConfig;

/// A page of a list, and the most pages one list is read through.
const PAGE: usize = 200;
const MAX_PAGES: usize = 25;

/// How long the picture's job is waited for, in polls of half a second.
const IMAGE_POLLS: usize = 60;

/// Tables whose columns are read when a library is opened: more is a long
/// wait for a tree, and the page of a table reads its own.
const COLUMNS_FOR_TABLES: usize = 50;

#[derive(Default)]
struct Tokens {
    access: Option<String>,
    refresh: Option<String>,
    expires: Option<Instant>,
}

/// Which services answered the probe in `connect`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Services {
    folders: bool,
    reports: bool,
    cas: bool,
    jobs: bool,
}

pub struct SasViya {
    cfg: SasViyaConfig,
    http: Http,
    tokens: RwLock<Tokens>,
    up: RwLock<Services>,
    /// The CAS server's name, from the probe.
    cas_server: RwLock<String>,
}

impl SasViya {
    pub fn new(cfg: SasViyaConfig) -> Result<SasViya> {
        let http = Http::with_tls(&cfg.url, Auth::None, TokenAs::Bearer, cfg.timeout, &cfg.tls)?
            .with_header("Accept", "application/json");
        Ok(SasViya {
            cfg,
            http,
            tokens: RwLock::new(Tokens::default()),
            up: RwLock::new(Services::default()),
            cas_server: RwLock::new(String::new()),
        })
    }

    fn bearer(&self) -> Auth {
        match self.tokens.read().access.clone() {
            Some(token) => Auth::Token {
                user: String::new(),
                token,
            },
            None => Auth::None,
        }
    }

    /// A token request to SAS Logon, through the registered client.
    async fn token_request(&self, form: String) -> Result<()> {
        let rb = self
            .http
            .request_as(Method::POST, "/SASLogon/oauth/token", &Auth::None)
            .basic_auth(&self.cfg.client_id, Some(&self.cfg.client_secret))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form);
        let v = self.http.json(rb).await?;
        let access = s(&v, "access_token");
        if access.is_empty() {
            return Err("SAS Logon answered with no access token.".into());
        }
        let expires_in = v.get("expires_in").and_then(Value::as_u64).unwrap_or(0);
        let mut t = self.tokens.write();
        t.access = Some(access);
        t.refresh = Some(s(&v, "refresh_token")).filter(|r| !r.is_empty());
        t.expires = (expires_in > 0).then(|| Instant::now() + Duration::from_secs(expires_in));
        Ok(())
    }

    async fn sign_in(&self) -> Result<()> {
        match &self.cfg.auth {
            Auth::Password { user, password } => self
                .token_request(format!(
                    "grant_type=password&username={}&password={}",
                    seg(user),
                    seg(password)
                ))
                .await
                .map_err(|e| {
                    format!(
                        "SAS Logon refused the sign-in: {e}. Check the OAuth client, its secret and the account; a site that signs people in through single sign-on takes an access token instead."
                    )
                }),
            Auth::Token { token, .. } => {
                self.tokens.write().access = Some(token.clone());
                Ok(())
            }
            _ => Err("Choose how to sign in.".into()),
        }
    }

    /// Renew the access token with the refresh token when it is about to end.
    async fn renew(&self) {
        let refresh = {
            let t = self.tokens.read();
            match (&t.refresh, t.expires) {
                (Some(r), Some(at)) if at <= Instant::now() + Duration::from_secs(60) => {
                    Some(r.clone())
                }
                _ => None,
            }
        };
        if let Some(r) = refresh {
            // A failure leaves the old token to fail with its own words.
            let _ = self
                .token_request(format!(
                    "grant_type=refresh_token&refresh_token={}",
                    seg(&r)
                ))
                .await;
        }
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.renew().await;
        self.http
            .json(self.http.request_as(Method::GET, path, &self.bearer()))
            .await
    }

    /// Every item of a paged list, `start` and `limit` at a time.
    async fn all(&self, path: &str) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        for _ in 0..MAX_PAGES {
            let sep = if path.contains('?') { '&' } else { '?' };
            let v = self
                .get(&format!("{path}{sep}start={}&limit={PAGE}", out.len()))
                .await?;
            let items = v
                .get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let got = items.len();
            out.extend(items);
            let count = v.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
            if got == 0 || out.len() >= count {
                break;
            }
        }
        Ok(out)
    }

    fn server(&self) -> String {
        self.cas_server.read().clone()
    }

    fn caslib_path(&self, lib: &str) -> String {
        format!(
            "/casManagement/servers/{}/caslibs/{}",
            seg(&self.server()),
            seg(lib)
        )
    }
}

#[async_trait]
impl BiProvider for SasViya {
    async fn connect(&self) -> Result<ServerInfo> {
        self.sign_in().await?;
        let who = self.get("/identities/users/@currentUser").await;
        let name = who
            .as_ref()
            .map(|u| {
                [s(u, "name"), s(u, "id")]
                    .into_iter()
                    .find(|n| !n.is_empty())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        // Probe each service once: a refusal other than "not found" is
        // the account's, and the service is there.
        let probe = |r: &Result<Value>| match r {
            Ok(_) => true,
            Err(e) => !e.contains("(404)"),
        };
        let folders = self.get("/folders/folders/@myFolder").await;
        let reports = self.get("/reports/reports?limit=1").await;
        let servers = self.get("/casManagement/servers?limit=1").await;
        let jobs = self.get("/jobExecution/jobs?limit=1").await;
        let cas_server = servers
            .as_ref()
            .ok()
            .and_then(|v| v.pointer("/items/0/name"))
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(n) = &cas_server {
            *self.cas_server.write() = n.clone();
        }
        let up = Services {
            folders: probe(&folders),
            reports: probe(&reports),
            cas: probe(&servers) && cas_server.is_some(),
            jobs: probe(&jobs),
        };
        *self.up.write() = up;
        if who.is_err() && !(up.folders || up.reports || up.cas || up.jobs) {
            return Err(who.err().unwrap_or_default());
        }
        let mut have = Vec::new();
        let mut lack = Vec::new();
        for (name, ok) in [
            ("Folders", up.folders),
            ("Reports", up.reports),
            ("CAS", up.cas),
            ("Jobs", up.jobs),
        ] {
            if ok { &mut have } else { &mut lack }.push(name);
        }
        let mut text = format!("Services: {}", have.join(", "));
        if !lack.is_empty() {
            text.push_str(&format!(" (no {})", lack.join(", ")));
        }
        Ok(ServerInfo {
            product: "SAS Viya".into(),
            version_text: text,
            version: None,
            signed_in_as: name,
            support: Support::Unknown,
        })
    }

    fn capabilities(&self) -> Caps {
        let up = *self.up.read();
        Caps {
            browse: up.folders || up.reports,
            native_sql: false,
            views: up.reports,
            extracts: false,
            search_kinds: if up.reports { &[Kind::Report] } else { &[] },
            tables: up.cas,
            data_section: "analytics-section-cas",
            jobs: up.jobs,
            view_data: false,
            view_files: &[ViewFile::Image],
        }
    }

    async fn children(&self, parent: &str) -> Result<Vec<Node>> {
        if parent == "root" {
            // The root folders, and the account's own folder.
            let mut out: Vec<Node> = Vec::new();
            let roots = match self.all("/folders/rootFolders").await {
                Ok(r) if !r.is_empty() => Ok(r),
                _ => self.all("/folders/folders/@rootFolder/members").await,
            };
            let me = self.get("/folders/folders/@myFolder").await;
            // Nothing readable at all is an error to show, not an empty tree.
            if let (Err(e), Err(_)) = (&roots, &me) {
                return Err(e.clone());
            }
            out.extend(roots.unwrap_or_default().iter().filter_map(member_node));
            if let Ok(me) = me
                && let Some(n) = folder_node(&me)
                && !out.iter().any(|o| o.id == n.id)
            {
                out.push(n);
            }
            return Ok(sorted(out));
        }
        let members = self
            .all(&format!("/folders/folders/{}/members", seg(parent)))
            .await?;
        Ok(sorted(members.iter().filter_map(member_node).collect()))
    }

    async fn search(&self, query: &str, _kinds: &[Kind], offset: usize) -> Result<Found> {
        let filter = format!("contains(name,'{}')", query.replace('\'', "''"));
        let v = self
            .get(&format!(
                "/reports/reports?filter={}&limit={SEARCH_PAGE}&start={offset}",
                seg(&filter)
            ))
            .await?;
        let nodes: Vec<Node> = v
            .get("items")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(report_node).collect())
            .unwrap_or_default();
        let total = v
            .get("count")
            .and_then(Value::as_u64)
            .map_or(nodes.len(), |c| c as usize);
        Ok(Found { nodes, total })
    }

    fn item_url(&self, kind: Kind, id: &str) -> String {
        match kind {
            Kind::Report => format!(
                "{}/SASVisualAnalyticsViewer/?reportUri=%2Freports%2Freports%2F{}",
                self.cfg.url,
                seg(id)
            ),
            _ => self.web_url(),
        }
    }

    fn web_url(&self) -> String {
        format!("{}/SASDrive/", self.cfg.url)
    }

    async fn view(&self, id: &str) -> Result<ViewInfo> {
        let r = self
            .get(&format!("/reports/reports/{}", seg(id)))
            .await
            .unwrap_or(Value::Null);
        Ok(ViewInfo {
            id: id.to_string(),
            name: s(&r, "name"),
            workbook: String::new(),
            project: s(&r, "createdBy"),
            updated: s(&r, "modifiedTimeStamp"),
        })
    }

    async fn view_file(
        &self,
        id: &str,
        file: ViewFile,
        width: u32,
        _values: &[(String, String)],
    ) -> Result<Vec<u8>> {
        if file != ViewFile::Image {
            return Err(super::NOT_OFFERED.into());
        }
        self.renew().await;
        let body = json!({
            "reportUri": format!("/reports/reports/{id}"),
            "layoutType": "entireSection",
            "sectionIndex": 0,
            "size": format!("{width}x{}", width * 5 / 8),
        });
        let rb = self
            .http
            .request_as(Method::POST, "/reportImages/jobs", &self.bearer())
            .json(&body);
        let job = self.http.json(rb).await?;
        let job_id = s(&job, "id");
        if job_id.is_empty() {
            return Err("The Report Images service gave no job.".into());
        }
        for _ in 0..IMAGE_POLLS {
            let j = self
                .get(&format!("/reportImages/jobs/{}", seg(&job_id)))
                .await?;
            match s(&j, "state").as_str() {
                "completed" => {
                    let href = image_href(&j)
                        .unwrap_or_else(|| format!("/reportImages/jobs/{}/images/0", seg(&job_id)));
                    let resp = self
                        .http
                        .send(self.http.request_as(Method::GET, &href, &self.bearer()))
                        .await?;
                    return resp
                        .bytes()
                        .await
                        .map(|b| b.to_vec())
                        .map_err(|e| e.to_string());
                }
                "failed" | "error" | "cancelled" | "timedOut" => {
                    return Err(format!(
                        "The Report Images service could not draw the report ({}).",
                        s(&j, "state")
                    ));
                }
                _ => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        }
        Err("The Report Images service did not finish in time.".into())
    }

    async fn databases(&self) -> Result<Vec<Database>> {
        let path = format!("/casManagement/servers/{}/caslibs", seg(&self.server()));
        let libs = self.all(&path).await?;
        Ok(libs
            .iter()
            .map(|l| Database {
                id: s(l, "name"),
                name: s(l, "name"),
                engine: "cas".into(),
                native: false,
                host: String::new(),
                port: String::new(),
                dbname: String::new(),
            })
            .collect())
    }

    async fn tables(&self, database: &str) -> Result<Vec<TableMeta>> {
        let base = self.caslib_path(database);
        let list = self.all(&format!("{base}/tables")).await?;
        let mut out = Vec::with_capacity(list.len());
        for (i, t) in list.iter().enumerate() {
            let name = s(t, "name");
            let fields = if i < COLUMNS_FOR_TABLES {
                self.all(&format!("{base}/tables/{}/columns", seg(&name)))
                    .await
                    .map(|c| fields_from(&c))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            out.push(TableMeta {
                id: name.clone(),
                schema: String::new(),
                name,
                fields,
            });
        }
        out.sort_by_key(|t| t.name.to_lowercase());
        Ok(out)
    }

    async fn preview(&self, database: &str, table: &str, limit: usize) -> Result<Table> {
        let started = Instant::now();
        let base = self.caslib_path(database);
        let cols = self
            .all(&format!("{base}/tables/{}/columns", seg(table)))
            .await?;
        let rows_path = format!(
            "/casRowSets/servers/{}/caslibs/{}/tables/{}/rows",
            seg(&self.server()),
            seg(database),
            seg(table)
        );
        let v = self
            .get(&format!("{rows_path}?start=0&limit={limit}"))
            .await?;
        let mut t = table_from(&cols, &v, limit);
        t.millis = started.elapsed().as_millis() as u64;
        Ok(t)
    }

    async fn jobs(&self) -> Result<Vec<JobInfo>> {
        let v = self.get("/jobExecution/jobs?limit=100").await?;
        Ok(jobs_from(&v))
    }

    async fn disconnect(&self) {
        *self.tokens.write() = Tokens::default();
    }
}

// ---- parsers: JSON in, the screens' types out -------------------------------

/// The id at the end of a service `uri` (`/reports/reports/<id>`).
fn id_of(v: &Value) -> String {
    let id = s(v, "id");
    if !id.is_empty() {
        return id;
    }
    s(v, "uri")
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn folder_node(v: &Value) -> Option<Node> {
    let id = id_of(v);
    (!id.is_empty()).then(|| Node {
        kind: Kind::Collection,
        id,
        name: s(v, "name"),
        description: s(v, "description"),
        collection: String::new(),
        edited: s(v, "modifiedTimeStamp"),
        editor: String::new(),
    })
}

fn report_node(v: &Value) -> Node {
    Node {
        kind: Kind::Report,
        id: id_of(v),
        name: s(v, "name"),
        description: s(v, "description"),
        collection: String::new(),
        edited: s(v, "modifiedTimeStamp"),
        editor: s(v, "createdBy"),
    }
}

/// A folder member: a folder or a report; data flows, files and the rest
/// are left out (only what this tab can show).
fn member_node(v: &Value) -> Option<Node> {
    match s(v, "contentType").as_str() {
        // A root folder listing has no content type: it is a folder.
        "folder" | "" if v.get("uri").is_none_or(|u| !s_is(u, "/reports/")) => folder_node(v),
        "report" => Some(report_node(v)),
        _ => None,
    }
}

fn s_is(v: &Value, part: &str) -> bool {
    v.as_str().is_some_and(|t| t.contains(part))
}

/// Folders first, then the rest, each by name.
fn sorted(mut nodes: Vec<Node>) -> Vec<Node> {
    nodes.sort_by_key(|n| (n.kind != Kind::Collection, n.name.to_lowercase()));
    nodes
}

/// The picture's address in a completed Report Images job.
fn image_href(job: &Value) -> Option<String> {
    job.pointer("/images/0/links")?
        .as_array()?
        .iter()
        .find(|l| s(l, "rel") == "image")
        .map(|l| s(l, "href"))
        .filter(|h| !h.is_empty())
}

fn kind_of(ty: &str) -> ColumnKind {
    match ty.to_lowercase().as_str() {
        "int32" | "int64" | "integer" | "int" | "bigint" | "smallint" => ColumnKind::Integer,
        "double" | "decimal" | "float" | "number" | "numeric" | "real" => ColumnKind::Decimal,
        "date" | "datetime" | "timestamp" | "time" => ColumnKind::Time,
        "varchar" | "char" | "string" | "text" => ColumnKind::Text,
        _ => ColumnKind::Other,
    }
}

fn fields_from(cols: &[Value]) -> Vec<FieldMeta> {
    cols.iter()
        .map(|c| FieldMeta {
            name: s(c, "name"),
            ty: s(c, "type"),
        })
        .collect()
}

/// A row preview: the columns with their kinds and the rows' cells.
fn table_from(cols: &[Value], rows: &Value, limit: usize) -> Table {
    let columns: Vec<Column> = cols
        .iter()
        .map(|c| Column {
            name: s(c, "name"),
            title: s(c, "name"),
            kind: kind_of(&s(c, "type")),
        })
        .collect();
    let items = rows
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let count = rows.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
    let mut out: Vec<Vec<Value>> = items
        .iter()
        .map(|r| {
            r.get("cells")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let truncated = out.len() > limit || count > out.len();
    out.truncate(limit);
    Table {
        columns,
        rows: out,
        sql: None,
        millis: 0,
        truncated,
    }
}

/// The recent jobs, newest first.
fn jobs_from(v: &Value) -> Vec<JobInfo> {
    let mut out: Vec<JobInfo> = v
        .get("items")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|j| {
                    let state = s(j, "state");
                    let notes = [
                        s(&j["results"], "message"),
                        s(&j["error"], "message"),
                        s(j, "statusMessage"),
                    ]
                    .into_iter()
                    .find(|n| !n.is_empty())
                    .unwrap_or_default();
                    JobInfo {
                        id: s(j, "id"),
                        kind: s(j, "name"),
                        progress: if state == "completed" { 100 } else { 0 },
                        ok: match state.as_str() {
                            "completed" => Some(true),
                            "failed" | "canceled" | "cancelled" | "timedOut" => Some(false),
                            _ => None,
                        },
                        started: s(j, "creationTimeStamp"),
                        completed: s(j, "endTimeStamp"),
                        notes,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.started.cmp(&a.started));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_is_a_folder_or_a_report_and_the_rest_are_left_out() {
        let folder = json!({ "id": "f1", "name": "Sales", "contentType": "folder", "uri": "/folders/folders/f1" });
        let report = json!({ "id": "r1", "name": "Revenue", "contentType": "report", "uri": "/reports/reports/r1" });
        let flow = json!({ "id": "d1", "name": "Flow", "contentType": "dataFlow", "uri": "/dataFlows/flows/d1" });
        assert_eq!(member_node(&folder).map(|n| n.kind), Some(Kind::Collection));
        assert_eq!(member_node(&report).map(|n| n.kind), Some(Kind::Report));
        assert!(member_node(&flow).is_none());
        // A root folder listing has no content type.
        let root = json!({ "id": "f0", "name": "SAS Content" });
        assert_eq!(member_node(&root).map(|n| n.kind), Some(Kind::Collection));
        // The id falls back to the end of the uri.
        let by_uri = json!({ "name": "X", "contentType": "report", "uri": "/reports/reports/abc" });
        assert_eq!(member_node(&by_uri).map(|n| n.id), Some("abc".to_string()));
        let mixed = sorted(vec![
            report_node(&json!({ "id": "r", "name": "A" })),
            folder_node(&json!({ "id": "f", "name": "Z" })).unwrap(),
        ]);
        assert_eq!(mixed[0].kind, Kind::Collection, "folders first");
    }

    #[test]
    fn a_completed_image_job_names_its_picture() {
        let job = json!({ "state": "completed", "images": [{ "sectionIndex": 0, "links": [
            { "rel": "self", "href": "/x" },
            { "rel": "image", "type": "image/png", "href": "/reportImages/jobs/j/images/0" }] }] });
        assert_eq!(
            image_href(&job).as_deref(),
            Some("/reportImages/jobs/j/images/0")
        );
        assert_eq!(image_href(&json!({ "state": "completed" })), None);
    }

    #[test]
    fn a_row_preview_has_typed_columns_and_says_when_it_was_cut() {
        let cols = [
            json!({ "name": "OrderID", "type": "double" }),
            json!({ "name": "Region", "type": "varchar" }),
            json!({ "name": "Day", "type": "date" }),
        ];
        let rows = json!({ "count": 4, "items": [
            { "cells": [1, "East", "2026-11-01"] }, { "cells": [2, "West", "2026-11-02"] }] });
        let t = table_from(&cols, &rows, 100);
        assert_eq!(
            t.columns.iter().map(|c| c.kind).collect::<Vec<_>>(),
            [ColumnKind::Decimal, ColumnKind::Text, ColumnKind::Time]
        );
        assert_eq!(t.rows.len(), 2);
        assert!(t.truncated, "the server has 4 rows and 2 came back");
        let cut = table_from(&cols, &rows, 1);
        assert_eq!((cut.rows.len(), cut.truncated), (1, true));
        let whole = table_from(
            &cols,
            &json!({ "count": 1, "items": [{ "cells": [1, "a", "b"] }] }),
            10,
        );
        assert!(!whole.truncated);
    }

    #[test]
    fn jobs_say_how_they_ended_newest_first() {
        let v = json!({ "items": [
            { "id": "j1", "name": "Nightly load", "state": "completed",
              "creationTimeStamp": "2026-11-03T02:00:00Z", "endTimeStamp": "2026-11-03T02:04:00Z" },
            { "id": "j2", "name": "Churn scoring", "state": "failed",
              "creationTimeStamp": "2026-11-03T03:00:00Z",
              "results": { "message": "ERROR: Table CUSTOMERS_SCORE not found." } },
            { "id": "j3", "name": "Running now", "state": "running",
              "creationTimeStamp": "2026-11-03T01:00:00Z" }] });
        let j = jobs_from(&v);
        assert_eq!(
            j.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            ["j2", "j1", "j3"]
        );
        assert_eq!((j[0].ok, j[1].ok, j[2].ok), (Some(false), Some(true), None));
        assert!(j[0].notes.contains("CUSTOMERS_SCORE"));
        assert_eq!((j[1].progress, j[2].progress), (100, 0));
    }

    #[test]
    fn column_types_map_to_kinds() {
        assert_eq!(kind_of("DOUBLE"), ColumnKind::Decimal);
        assert_eq!(kind_of("int64"), ColumnKind::Integer);
        assert_eq!(kind_of("varchar"), ColumnKind::Text);
        assert_eq!(kind_of("datetime"), ColumnKind::Time);
        assert_eq!(kind_of("blob"), ColumnKind::Other);
    }
}
