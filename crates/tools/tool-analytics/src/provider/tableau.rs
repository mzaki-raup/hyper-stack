//! Tableau Server through its REST API (docs/tools/analytics.md §4.8, §5.2),
//! read-only. A username and password, or a Personal Access Token, sign in
//! to one site; the token and the site's id live in memory and are sent back
//! as `X-Tableau-Auth`; the tab's end is `auth/signout`. The interactive viz
//! is Tableau's own to draw: a view here is its picture and its data, and
//! Open Interactive leads to the Browser.
//!
//! Every call asks for JSON (`Accept`); a Server answers XML otherwise. The
//! first call, `serverinfo`, uses an old API version every Server knows; the
//! rest use the version it reports, so an older Server answers in what it
//! knows (§4.8, version gating).

use std::collections::HashMap;

use async_trait::async_trait;
use hs_core::service_auth::{Auth, TokenAs};
use hs_core::service_http::{Http, s, seg};
use parking_lot::RwLock;
use reqwest::Method;
use serde_json::{Value, json};

use super::{
    BiProvider, Caps, Column, ColumnKind, ExtractTask, Found, JobInfo, Kind, Node, Result,
    SEARCH_PAGE, ServerInfo, Support, Table, ViewFile, ViewInfo,
};
use crate::profile::TableauConfig;

const AUTH_HEADER: &str = "X-Tableau-Auth";

/// The API version `serverinfo` is asked with: every Server since 2018 has it.
const FIRST_API: &str = "2.4";

/// The oldest and newest REST API minor versions (`3.x`) this tab has been
/// built and tried against: 3.15 is Server 2022.1, 3.23 is 2024.2.
pub const OLDEST_API_MINOR: u32 = 15;
pub const NEWEST_TESTED_API_MINOR: u32 = 23;

/// The first API version whose view picture takes a size.
const VIZ_SIZE_FROM_MINOR: u32 = 23;

/// A page of a list, as the server's own limit allows.
const PAGE: usize = 1000;

/// The most pages one list is read through.
const MAX_PAGES: usize = 20;

#[derive(Default)]
struct State {
    /// The REST API version the calls use.
    api: String,
    token: Option<String>,
    site_id: String,
}

pub struct Tableau {
    cfg: TableauConfig,
    http: Http,
    state: RwLock<State>,
    /// Project names by id, and views' and workbooks' `contentUrl` by id,
    /// filled as lists are read, so a page can say where it is.
    projects: RwLock<HashMap<String, String>>,
    content_urls: RwLock<HashMap<String, String>>,
}

impl Tableau {
    pub fn new(cfg: TableauConfig) -> Result<Tableau> {
        let http = Http::with_tls(&cfg.url, Auth::None, TokenAs::Bearer, cfg.timeout, &cfg.tls)?
            .with_header("Accept", "application/json");
        Ok(Tableau {
            cfg,
            http,
            state: RwLock::new(State {
                api: FIRST_API.into(),
                ..Default::default()
            }),
            projects: RwLock::new(HashMap::new()),
            content_urls: RwLock::new(HashMap::new()),
        })
    }

    fn auth(&self) -> Auth {
        match self.state.read().token.clone() {
            Some(key) => Auth::ApiKey {
                header: AUTH_HEADER.into(),
                key,
            },
            None => Auth::None,
        }
    }

    /// `/api/<version>/sites/<site id>/<rest>`.
    fn site_path(&self, rest: &str) -> String {
        let st = self.state.read();
        format!("/api/{}/sites/{}/{rest}", st.api, st.site_id)
    }

    async fn get(&self, rest: &str) -> Result<Value> {
        let path = self.site_path(rest);
        self.http
            .json(self.http.request_as(Method::GET, &path, &self.auth()))
            .await
    }

    async fn get_bytes(&self, rest: &str) -> Result<Vec<u8>> {
        let path = self.site_path(rest);
        let resp = self
            .http
            .send(self.http.request_as(Method::GET, &path, &self.auth()))
            .await?;
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| e.to_string())
    }

    /// Every item of a list (`workbooks` / `workbook`), a page at a time.
    async fn all(&self, rest: &str, collection: &str, item: &str) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        for number in 1..=MAX_PAGES {
            let sep = if rest.contains('?') { '&' } else { '?' };
            let v = self
                .get(&format!("{rest}{sep}pageSize={PAGE}&pageNumber={number}"))
                .await?;
            let part = v.get(collection).unwrap_or(&Value::Null);
            out.extend(items(part, item));
            let total = part
                .pointer("/pagination/totalAvailable")
                .and_then(number_of)
                .unwrap_or(out.len());
            if out.len() >= total {
                break;
            }
        }
        Ok(out)
    }

    fn api_minor(&self) -> u32 {
        api_minor(&self.state.read().api).unwrap_or(0)
    }

    fn remember_urls(&self, nodes: &[Value]) {
        let mut m = self.content_urls.write();
        for n in nodes {
            let (id, url) = (s(n, "id"), s(n, "contentUrl"));
            if !id.is_empty() && !url.is_empty() {
                m.insert(id, url);
            }
        }
    }

    async fn sign_in(&self) -> Result<String> {
        let rb = self
            .http
            .request_as(
                Method::POST,
                &format!("/api/{}/auth/signin", self.state.read().api),
                &Auth::None,
            )
            .json(&sign_in_body(&self.cfg.auth, &self.cfg.site)?);
        let v = self.http.json(rb).await.map_err(|e| {
            format!(
                "Tableau refused the sign-in: {e}. Check the site's content URL: it is the part after /site/ in the address, and empty for the default site."
            )
        })?;
        let token = s(&v["credentials"], "token");
        if token.is_empty() {
            return Err("Tableau signed in but gave no token.".into());
        }
        let mut st = self.state.write();
        st.token = Some(token);
        st.site_id = s(&v["credentials"]["site"], "id");
        Ok(s(&v["credentials"]["user"], "id"))
    }
}

#[async_trait]
impl BiProvider for Tableau {
    async fn connect(&self) -> Result<ServerInfo> {
        // The version is public: read it first, then talk the Server's own.
        let info = self
            .http
            .get_json(&format!("/api/{FIRST_API}/serverinfo"))
            .await?;
        let (product, api) = serverinfo(&info);
        if !api.is_empty() {
            self.state.write().api = api.clone();
        }
        let support = support_for(&api);
        if support == Support::TooOld {
            return Err(format!(
                "This Tableau Server speaks REST API {api}, older than 3.{OLDEST_API_MINOR} (Server 2022.1), which this tab needs."
            ));
        }
        let user_id = self.sign_in().await?;
        let name = match &self.cfg.auth {
            Auth::Password { user, .. } => user.clone(),
            Auth::ApiKey { header, .. } => header.clone(),
            _ => String::new(),
        };
        // The user's own name, where the Server tells it; the sign-in's
        // name stands when it does not.
        let named = if user_id.is_empty() {
            String::new()
        } else {
            self.get(&format!("users/{}", seg(&user_id)))
                .await
                .map(|u| s(&u["user"], "name"))
                .unwrap_or_default()
        };
        Ok(ServerInfo {
            product: "Tableau Server".into(),
            version_text: format!("{product} (REST API {api})")
                .trim_end_matches(" (REST API )")
                .to_string(),
            version: None,
            signed_in_as: if named.is_empty() { name } else { named },
            support,
        })
    }

    fn capabilities(&self) -> Caps {
        Caps {
            browse: true,
            native_sql: false,
            views: true,
            extracts: true,
            search_kinds: &[Kind::View, Kind::Workbook, Kind::DataSource],
            tables: false,
            data_section: "",
            jobs: true,
            view_data: true,
            view_files: &ViewFile::ALL,
        }
    }

    async fn children(&self, parent: &str) -> Result<Vec<Node>> {
        if parent == "root" {
            let list = self.all("projects", "projects", "project").await?;
            let mut names = self.projects.write();
            for p in &list {
                names.insert(s(p, "id"), s(p, "name"));
            }
            return Ok(sorted(projects_from(&list)));
        }
        if self.projects.read().contains_key(parent) {
            // The server's own filter narrows the read; the id check keeps
            // a Server that ignores it honest.
            let name = self
                .projects
                .read()
                .get(parent)
                .cloned()
                .unwrap_or_default();
            let filter = format!("?filter=projectName:eq:{}", seg(&name));
            let wbs = self
                .all(&format!("workbooks{filter}"), "workbooks", "workbook")
                .await?;
            let dss = self
                .all(&format!("datasources{filter}"), "datasources", "datasource")
                .await?;
            self.remember_urls(&wbs);
            let mut out = workbooks_from(&wbs, Some(parent));
            out.extend(datasources_from(&dss, Some(parent)));
            return Ok(sorted(out));
        }
        // A workbook: its views.
        let v = self
            .get(&format!("workbooks/{}/views", seg(parent)))
            .await?;
        let list = items(v.get("views").unwrap_or(&Value::Null), "view");
        self.remember_urls(&list);
        Ok(sorted(views_from(&list, "")))
    }

    async fn search(&self, query: &str, kinds: &[Kind], _offset: usize) -> Result<Found> {
        let wanted = |k: Kind| kinds.is_empty() || kinds.contains(&k);
        let filter = format!("?filter=name:has:{}", seg(query));
        let page = |kind: &str| format!("{kind}{filter}&pageSize={SEARCH_PAGE}&pageNumber=1");
        let mut nodes = Vec::new();
        if wanted(Kind::View) {
            let v = self.get(&page("views")).await?;
            let list = items(v.get("views").unwrap_or(&Value::Null), "view");
            self.remember_urls(&list);
            nodes.extend(views_from(&list, ""));
        }
        if wanted(Kind::Workbook) {
            let v = self.get(&page("workbooks")).await?;
            let list = items(v.get("workbooks").unwrap_or(&Value::Null), "workbook");
            self.remember_urls(&list);
            nodes.extend(workbooks_from(&list, None));
        }
        if wanted(Kind::DataSource) {
            let v = self.get(&page("datasources")).await?;
            let list = items(v.get("datasources").unwrap_or(&Value::Null), "datasource");
            nodes.extend(datasources_from(&list, None));
        }
        let total = nodes.len();
        Ok(Found { nodes, total })
    }

    fn item_url(&self, kind: Kind, id: &str) -> String {
        let site = &self.cfg.site;
        let content = self.content_urls.read().get(id).cloned();
        match (kind, content) {
            (Kind::View, Some(c)) => interactive_url(&self.cfg.url, site, &c),
            _ => self.web_url(),
        }
    }

    fn web_url(&self) -> String {
        let site = &self.cfg.site;
        if site.is_empty() {
            format!("{}/#/home", self.cfg.url)
        } else {
            format!("{}/#/site/{site}/home", self.cfg.url)
        }
    }

    async fn view(&self, id: &str) -> Result<ViewInfo> {
        // The facts are a nicety: a Server that does not answer `Get View`
        // still has a picture and data to show.
        let mut info = match self.get(&format!("views/{}", seg(id))).await {
            Ok(v) => view_from(&v["view"]),
            Err(_) => ViewInfo::default(),
        };
        info.id = id.to_string();
        if let Some(name) = self.projects.read().get(&info.project) {
            info.project = name.clone();
        }
        Ok(info)
    }

    async fn view_file(
        &self,
        id: &str,
        file: ViewFile,
        width: u32,
        values: &[(String, String)],
    ) -> Result<Vec<u8>> {
        let mut q = filter_query(values);
        let what = match file {
            ViewFile::Image => {
                q.push_str("&resolution=high");
                if self.api_minor() >= VIZ_SIZE_FROM_MINOR && width > 0 {
                    q.push_str(&format!("&vizWidth={width}"));
                }
                "image"
            }
            ViewFile::Csv => "data",
            ViewFile::Pdf => "pdf",
        };
        let q = q.trim_start_matches('&');
        self.get_bytes(&format!("views/{}/{what}?{q}", seg(id)))
            .await
    }

    async fn view_data(
        &self,
        id: &str,
        values: &[(String, String)],
        limit: usize,
    ) -> Result<Table> {
        let started = std::time::Instant::now();
        let bytes = self.view_file(id, ViewFile::Csv, 0, values).await?;
        let mut table = table_from_csv(&String::from_utf8_lossy(&bytes), limit)?;
        table.millis = started.elapsed().as_millis() as u64;
        Ok(table)
    }

    async fn extracts(&self) -> Result<Vec<ExtractTask>> {
        let v = self.get("tasks/extractRefreshes").await?;
        let wbs = self.all("workbooks", "workbooks", "workbook").await?;
        let dss = self.all("datasources", "datasources", "datasource").await?;
        let names: HashMap<String, String> = wbs
            .iter()
            .chain(dss.iter())
            .map(|x| (s(x, "id"), s(x, "name")))
            .collect();
        Ok(tasks_from(&v, &names))
    }

    async fn jobs(&self) -> Result<Vec<JobInfo>> {
        let v = self.get("jobs").await?;
        Ok(jobs_from(&v))
    }

    async fn disconnect(&self) {
        let token = {
            let mut st = self.state.write();
            st.token.take().map(|t| (t, st.api.clone()))
        };
        if let Some((token, api)) = token {
            let rb = self
                .http
                .request_as(
                    Method::POST,
                    &format!("/api/{api}/auth/signout"),
                    &Auth::ApiKey {
                        header: AUTH_HEADER.into(),
                        key: token,
                    },
                )
                .body("");
            let _ = self.http.send(rb).await;
        }
    }
}

// ---- parsers: JSON in, the screens' types out -------------------------------

/// A JSON number, or one a Server wrote as text (`"7"`), as a count.
fn number_of(v: &Value) -> Option<usize> {
    v.as_u64()
        .map(|n| n as usize)
        .or_else(|| v.as_str().and_then(|t| t.trim().parse().ok()))
}

/// The entries of a list object, whether the Server wrote one as an object
/// or a list of them.
fn items(part: &Value, item: &str) -> Vec<Value> {
    match part.get(item) {
        Some(Value::Array(a)) => a.clone(),
        Some(o @ Value::Object(_)) => vec![o.clone()],
        _ => Vec::new(),
    }
}

/// `(product version, REST API version)` from `serverinfo`.
fn serverinfo(v: &Value) -> (String, String) {
    let info = &v["serverInfo"];
    (
        info.pointer("/productVersion/value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        s(info, "restApiVersion"),
    )
}

/// `3.23` as 23; `None` for any other major or anything unparsable.
fn api_minor(api: &str) -> Option<u32> {
    let (major, minor) = api.trim().split_once('.')?;
    (major == "3").then(|| minor.parse().ok())?
}

fn support_for(api: &str) -> Support {
    match api_minor(api) {
        None => Support::Unknown,
        Some(m) if m < OLDEST_API_MINOR => Support::TooOld,
        Some(m) if m > NEWEST_TESTED_API_MINOR => Support::Newer,
        Some(_) => Support::Supported,
    }
}

/// The body of `auth/signin` for a password or a Personal Access Token.
fn sign_in_body(auth: &Auth, site: &str) -> Result<Value> {
    let site = json!({ "contentUrl": site });
    Ok(match auth {
        Auth::Password { user, password } => json!({ "credentials": {
            "name": user, "password": password, "site": site } }),
        Auth::ApiKey { header, key } => json!({ "credentials": {
            "personalAccessTokenName": header,
            "personalAccessTokenSecret": key, "site": site } }),
        _ => return Err("Choose how to sign in.".into()),
    })
}

/// Folders (projects) first, then the rest, each by name.
fn sorted(mut nodes: Vec<Node>) -> Vec<Node> {
    nodes.sort_by_key(|n| (n.kind != Kind::Collection, n.name.to_lowercase()));
    nodes
}

fn node(kind: Kind, v: &Value, collection: String) -> Node {
    Node {
        kind,
        id: s(v, "id"),
        name: s(v, "name"),
        description: s(v, "description"),
        collection,
        edited: s(v, "updatedAt"),
        editor: String::new(),
    }
}

fn projects_from(list: &[Value]) -> Vec<Node> {
    list.iter()
        .map(|p| node(Kind::Collection, p, String::new()))
        .collect()
}

/// Workbooks, narrowed to project `only` when given.
fn workbooks_from(list: &[Value], only: Option<&str>) -> Vec<Node> {
    list.iter()
        .filter(|w| {
            only.is_none_or(|p| w.pointer("/project/id").and_then(Value::as_str) == Some(p))
        })
        .map(|w| node(Kind::Workbook, w, s(&w["project"], "name")))
        .collect()
}

fn datasources_from(list: &[Value], only: Option<&str>) -> Vec<Node> {
    list.iter()
        .filter(|d| {
            only.is_none_or(|p| d.pointer("/project/id").and_then(Value::as_str) == Some(p))
        })
        .map(|d| node(Kind::DataSource, d, s(&d["project"], "name")))
        .collect()
}

/// Views, said to sit in `workbook` where that is known.
fn views_from(list: &[Value], workbook: &str) -> Vec<Node> {
    list.iter()
        .map(|v| node(Kind::View, v, workbook.to_string()))
        .collect()
}

fn view_from(v: &Value) -> ViewInfo {
    ViewInfo {
        id: s(v, "id"),
        name: s(v, "name"),
        workbook: s(&v["workbook"], "name"),
        project: s(&v["project"], "id"),
        updated: s(v, "updatedAt"),
    }
}

/// `RegionalSales/sheets/Overview` opens at `#/views/RegionalSales/Overview`,
/// under `#/site/<site>/` on a site that is not the default one.
fn interactive_url(base: &str, site: &str, content_url: &str) -> String {
    let path = content_url.replacen("/sheets/", "/", 1);
    if site.is_empty() {
        format!("{base}/#/views/{path}")
    } else {
        format!("{base}/#/site/{site}/views/{path}")
    }
}

/// `&vf_Region=East&vf_Segment=Retail` for the filter fields.
fn filter_query(values: &[(String, String)]) -> String {
    values
        .iter()
        .filter(|(f, v)| !f.trim().is_empty() && !v.trim().is_empty())
        .map(|(f, v)| format!("&vf_{}={}", seg(f.trim()), seg(v.trim())))
        .collect()
}

/// A CSV as a table: the first row names the columns; a column is a number
/// when every value in it is one.
pub fn table_from_csv(text: &str, limit: usize) -> Result<Table> {
    let mut rd = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(text.trim_start_matches('\u{feff}').as_bytes());
    let headers: Vec<String> = rd
        .headers()
        .map_err(|e| format!("the view's data was not a CSV: {e}"))?
        .iter()
        .map(str::to_string)
        .collect();
    let mut cells: Vec<Vec<String>> = Vec::new();
    let mut truncated = false;
    for rec in rd.records() {
        let rec = rec.map_err(|e| format!("the view's data was not a CSV: {e}"))?;
        if cells.len() >= limit {
            truncated = true;
            break;
        }
        cells.push(
            (0..headers.len())
                .map(|i| rec.get(i).unwrap_or_default().to_string())
                .collect(),
        );
    }
    let kinds: Vec<ColumnKind> = (0..headers.len())
        .map(|i| {
            let col = cells.iter().map(|r| r[i].trim()).filter(|c| !c.is_empty());
            let mut n = 0;
            let (mut ints, mut nums) = (true, true);
            for c in col {
                n += 1;
                ints &= c.parse::<i64>().is_ok();
                nums &= c.parse::<f64>().is_ok();
            }
            match (n, ints, nums) {
                (0, ..) => ColumnKind::Text,
                (_, true, _) => ColumnKind::Integer,
                (_, _, true) => ColumnKind::Decimal,
                _ => ColumnKind::Text,
            }
        })
        .collect();
    let rows = cells
        .iter()
        .map(|r| {
            r.iter()
                .zip(&kinds)
                .map(|(c, k)| match k {
                    ColumnKind::Integer if !c.trim().is_empty() => {
                        c.trim().parse::<i64>().map_or(Value::Null, Value::from)
                    }
                    ColumnKind::Decimal if !c.trim().is_empty() => {
                        c.trim().parse::<f64>().map_or(Value::Null, Value::from)
                    }
                    _ if c.is_empty() => Value::Null,
                    _ => Value::String(c.clone()),
                })
                .collect()
        })
        .collect();
    Ok(Table {
        columns: headers
            .iter()
            .zip(kinds)
            .map(|(h, kind)| Column {
                name: h.clone(),
                title: h.clone(),
                kind,
            })
            .collect(),
        rows,
        sql: None,
        millis: 0,
        truncated,
    })
}

/// The extract refresh tasks; `names` gives workbooks' and data sources'
/// names by id.
fn tasks_from(v: &Value, names: &HashMap<String, String>) -> Vec<ExtractTask> {
    items(v.get("tasks").unwrap_or(&Value::Null), "task")
        .iter()
        .filter_map(|t| t.get("extractRefresh"))
        .map(|t| {
            let (target_kind, target_id) = match (t.get("workbook"), t.get("datasource")) {
                (Some(w), _) => ("Workbook", s(w, "id")),
                (_, Some(d)) => ("Data Source", s(d, "id")),
                _ => ("", String::new()),
            };
            ExtractTask {
                id: s(t, "id"),
                kind: s(t, "type"),
                target_kind: target_kind.to_string(),
                target: names.get(&target_id).cloned().unwrap_or(target_id),
                schedule: s(&t["schedule"], "name"),
                frequency: s(&t["schedule"], "frequency"),
                failed: number_of(&t["consecutiveFailedCount"]).unwrap_or(0) as u32,
            }
        })
        .collect()
}

/// The background jobs, newest first.
fn jobs_from(v: &Value) -> Vec<JobInfo> {
    let mut out: Vec<JobInfo> = items(
        v.get("backgroundJobs").unwrap_or(&Value::Null),
        "backgroundJob",
    )
    .iter()
    .map(|j| {
        let notes = items(j.get("statusNotes").unwrap_or(&Value::Null), "statusNote")
            .iter()
            .map(|n| {
                [s(n, "text"), s(n, "value")]
                    .into_iter()
                    .find(|x| !x.is_empty())
                    .unwrap_or_default()
            })
            .filter(|n| !n.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
        JobInfo {
            id: s(j, "id"),
            kind: s(j, "type"),
            progress: number_of(&j["progress"]).unwrap_or(0) as u32,
            // `finishCode` is 0 for success once a job has ended.
            ok: j
                .get("finishCode")
                .and_then(|c| number_of(c).or_else(|| c.as_i64().map(|n| n as usize)))
                .map(|c| c == 0),
            started: s(j, "startedAt"),
            completed: s(j, "completedAt"),
            notes,
        }
    })
    .collect();
    out.sort_by(|a, b| b.started.cmp(&a.started));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serverinfo_gives_the_product_and_the_rest_api_version() {
        let v = json!({ "serverInfo": {
            "productVersion": { "value": "2024.2.0", "build": "x" }, "restApiVersion": "3.23" } });
        assert_eq!(serverinfo(&v), ("2024.2.0".into(), "3.23".into()));
        assert_eq!(serverinfo(&json!({})), (String::new(), String::new()));
    }

    #[test]
    fn the_rest_api_version_decides_the_support_chip() {
        assert_eq!(support_for("3.14"), Support::TooOld);
        assert_eq!(support_for("3.15"), Support::Supported);
        assert_eq!(support_for("3.23"), Support::Supported);
        assert_eq!(support_for("3.24"), Support::Newer);
        assert_eq!(support_for(""), Support::Unknown);
        assert_eq!(support_for("2.8"), Support::Unknown);
        assert_eq!(api_minor("3.9"), Some(9));
    }

    #[test]
    fn sign_in_sends_a_password_or_a_token_into_the_site() {
        let pw = sign_in_body(
            &Auth::Password {
                user: "ada".into(),
                password: "pw".into(),
            },
            "hs-demo",
        )
        .expect("body");
        assert_eq!(pw["credentials"]["name"], "ada");
        assert_eq!(pw["credentials"]["site"]["contentUrl"], "hs-demo");
        let pat = sign_in_body(
            &Auth::ApiKey {
                header: "hyperstack".into(),
                key: "s3".into(),
            },
            "",
        )
        .expect("body");
        assert_eq!(pat["credentials"]["personalAccessTokenName"], "hyperstack");
        assert_eq!(pat["credentials"]["personalAccessTokenSecret"], "s3");
        assert_eq!(
            pat["credentials"]["site"]["contentUrl"], "",
            "the default site"
        );
        assert!(sign_in_body(&Auth::None, "").is_err());
    }

    #[test]
    fn projects_come_first_and_a_project_holds_only_its_own_items() {
        let wbs = json!([
            { "id": "w1", "name": "Regional Sales", "project": { "id": "p1", "name": "Sales" } },
            { "id": "w2", "name": "Order Fulfilment", "project": { "id": "p2", "name": "Operations" } }]);
        let dss =
            json!([{ "id": "d1", "name": "Regions", "project": { "id": "p1", "name": "Sales" } }]);
        let mut nodes = workbooks_from(wbs.as_array().unwrap(), Some("p1"));
        nodes.extend(datasources_from(dss.as_array().unwrap(), Some("p1")));
        assert_eq!(
            nodes
                .iter()
                .map(|n| (n.kind, n.id.as_str()))
                .collect::<Vec<_>>(),
            [(Kind::Workbook, "w1"), (Kind::DataSource, "d1")]
        );
        assert_eq!(nodes[0].collection, "Sales");
        let all = workbooks_from(wbs.as_array().unwrap(), None);
        assert_eq!(all.len(), 2, "a search result has no project to keep to");
        let mixed = sorted(vec![
            node(
                Kind::Workbook,
                &json!({ "id": "a", "name": "A" }),
                String::new(),
            ),
            node(
                Kind::Collection,
                &json!({ "id": "z", "name": "Z" }),
                String::new(),
            ),
        ]);
        assert_eq!(mixed[0].kind, Kind::Collection);
    }

    #[test]
    fn a_list_of_one_may_be_an_object_and_a_count_may_be_text() {
        let one = json!({ "view": { "id": "v1" } });
        assert_eq!(items(&one, "view").len(), 1);
        assert_eq!(items(&json!({ "view": [{}, {}] }), "view").len(), 2);
        assert!(items(&Value::Null, "view").is_empty());
        assert_eq!(number_of(&json!("7")), Some(7));
        assert_eq!(number_of(&json!(7)), Some(7));
    }

    #[test]
    fn a_view_opens_interactively_under_its_site() {
        let c = "RegionalSales/sheets/Overview";
        assert_eq!(
            interactive_url("https://t.example.com", "", c),
            "https://t.example.com/#/views/RegionalSales/Overview"
        );
        assert_eq!(
            interactive_url("https://t.example.com", "hs-demo", c),
            "https://t.example.com/#/site/hs-demo/views/RegionalSales/Overview"
        );
    }

    #[test]
    fn filters_become_vf_parameters_and_blank_ones_are_left_out() {
        let q = filter_query(&[
            ("Region".into(), "East".into()),
            ("Product Line".into(), "A & B".into()),
            ("Empty".into(), " ".into()),
        ]);
        assert_eq!(q, "&vf_Region=East&vf_Product%20Line=A%20%26%20B");
    }

    #[test]
    fn a_csv_is_a_table_with_numbers_where_a_column_is_all_numbers() {
        let t = table_from_csv(
            "Region,Sales,Share\nEast,120340,0.5\nWest,98210,0.25\nSouth,,0.25\n",
            100,
        )
        .expect("a table");
        assert_eq!(
            t.columns.iter().map(|c| c.kind).collect::<Vec<_>>(),
            [ColumnKind::Text, ColumnKind::Integer, ColumnKind::Decimal]
        );
        assert_eq!(t.rows[0][1], json!(120340));
        assert_eq!(t.rows[2][1], Value::Null, "an empty cell");
        assert_eq!(t.rows[1][2], json!(0.25));
        assert!(!t.truncated);
        let cut = table_from_csv("a\n1\n2\n3\n", 2).expect("a table");
        assert_eq!((cut.rows.len(), cut.truncated), (2, true));
        let quoted = table_from_csv("Name,Note\n\"Doe, Ada\",\"say \"\"hi\"\"\"\n", 10).unwrap();
        assert_eq!(quoted.rows[0][0], json!("Doe, Ada"));
        assert_eq!(quoted.rows[0][1], json!("say \"hi\""));
    }

    #[test]
    fn extract_tasks_name_their_target_and_count_failures() {
        let v = json!({ "tasks": { "task": [
            { "extractRefresh": { "id": "t1", "type": "FullRefresh", "consecutiveFailedCount": "0",
              "schedule": { "name": "Nightly", "frequency": "Daily" }, "datasource": { "id": "d1" } } },
            { "extractRefresh": { "id": "t2", "type": "IncrementalRefresh", "consecutiveFailedCount": "2",
              "schedule": { "name": "Hourly", "frequency": "Hourly" }, "workbook": { "id": "w9" } } } ] } });
        let names = HashMap::from([("d1".to_string(), "Orders (extract)".to_string())]);
        let t = tasks_from(&v, &names);
        assert_eq!(
            (t[0].target_kind.as_str(), t[0].target.as_str()),
            ("Data Source", "Orders (extract)")
        );
        assert_eq!(
            (t[1].target_kind.as_str(), t[1].target.as_str()),
            ("Workbook", "w9"),
            "an unknown id is shown as it is"
        );
        assert_eq!((t[0].failed, t[1].failed), (0, 2));
        assert_eq!(t[1].schedule, "Hourly");
    }

    #[test]
    fn jobs_say_whether_they_failed_and_why_newest_first() {
        let v = json!({ "backgroundJobs": { "backgroundJob": [
            { "id": "j1", "type": "RefreshExtracts", "progress": "100", "finishCode": "0",
              "startedAt": "2026-11-03T02:00:02Z" },
            { "id": "j2", "type": "RefreshExtracts", "progress": "100", "finishCode": "1",
              "startedAt": "2026-11-03T03:00:01Z",
              "statusNotes": { "statusNote": [{ "type": "failure", "text": "Could not connect" }] } },
            { "id": "j3", "type": "RefreshExtracts", "progress": "40",
              "startedAt": "2026-11-03T01:00:00Z" } ] } });
        let j = jobs_from(&v);
        assert_eq!(
            j.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            ["j2", "j1", "j3"]
        );
        assert_eq!((j[0].ok, j[1].ok, j[2].ok), (Some(false), Some(true), None));
        assert_eq!(j[0].notes, "Could not connect");
        assert_eq!(j[2].progress, 40);
    }
}
