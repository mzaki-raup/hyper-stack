//! Power BI Report Server through its REST API v2.0 (docs/tools/analytics.md
//! §4.10, §5.4), read-only: `{url}/reports/api/v2.0`, HTTP Basic. The
//! catalogue is read once as a flat list and the tree is built from the
//! items' paths. A paginated report has a picture and CSV data
//! (`Reports({id})/Export/…`); a Power BI report (`.pbix`) has no server-side
//! picture or data API, so it is details and Open Interactive; KPIs are
//! value, goal, status and trend; data sources show their connection string
//! with the secret masked; the cache refresh plans are the extract tasks.
//! NTLM and the current Windows user are not built. The endpoints are from
//! Microsoft's reference and the bench mock; they are unconfirmed against a
//! real Report Server.

use async_trait::async_trait;
use hs_core::service_auth::TokenAs;
use hs_core::service_http::{Http, s, seg};
use parking_lot::RwLock;
use reqwest::Method;
use serde_json::Value;

use super::{
    BiProvider, Caps, ExtractTask, Found, Kind, Kpi, Node, Result, SEARCH_PAGE, ServerInfo,
    Support, Table, ViewFile, ViewInfo,
};
use crate::profile::PowerBiConfig;

const API: &str = "/reports/api/v2.0";

/// A page of a list, and the most pages one list is read through.
const PAGE: usize = 500;
const MAX_PAGES: usize = 40;

/// The first Report Server major version this tab has been written for
/// (15 is the 2019 line the REST API v2.0 shipped in).
const OLDEST_MAJOR: u32 = 15;

pub struct PowerBi {
    cfg: PowerBiConfig,
    http: Http,
    /// The catalogue as the server gave it: every folder, report, KPI and
    /// data source, read when the root is.
    items: RwLock<Vec<Value>>,
}

impl PowerBi {
    pub fn new(cfg: PowerBiConfig) -> Result<PowerBi> {
        let http = Http::with_tls(
            &cfg.url,
            cfg.auth.clone(),
            TokenAs::Bearer,
            cfg.timeout,
            &cfg.tls,
        )?
        .with_header("Accept", "application/json");
        Ok(PowerBi {
            cfg,
            http,
            items: RwLock::new(Vec::new()),
        })
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.http.get_json(&format!("{API}/{path}")).await
    }

    async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let resp = self
            .http
            .send(self.http.request(Method::GET, &format!("{API}/{path}")))
            .await?;
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| e.to_string())
    }

    /// Every entry of an OData list, `PAGE` at a time with `$top` / `$skip`.
    async fn all(&self, path: &str) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        for _ in 0..MAX_PAGES {
            let sep = if path.contains('?') { '&' } else { '?' };
            let v = self
                .get(&format!("{path}{sep}$top={PAGE}&$skip={}", out.len()))
                .await?;
            let page = v
                .get("value")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let got = page.len();
            out.extend(page);
            if got < PAGE {
                break;
            }
        }
        Ok(out)
    }

    /// The catalogue and the shared data sources, one list.
    async fn load(&self) -> Result<()> {
        let mut items = self.all("CatalogItems").await?;
        // Data sources are catalogue items on a real server; the mock keeps
        // them apart: either way each appears once.
        if let Ok(ds) = self.all("DataSources").await {
            for d in ds {
                let id = s(&d, "Id");
                if !items.iter().any(|i| s(i, "Id") == id) {
                    items.push(d);
                }
            }
        }
        *self.items.write() = items;
        Ok(())
    }

    fn path_of(&self, id: &str) -> Option<String> {
        self.items
            .read()
            .iter()
            .find(|i| s(i, "Id") == id)
            .map(|i| s(i, "Path"))
    }
}

#[async_trait]
impl BiProvider for PowerBi {
    async fn connect(&self) -> Result<ServerInfo> {
        let system = self.get("System").await.map_err(|e| {
            format!(
                "Power BI Report Server refused the sign-in or the address: {e}. Basic sign-in needs the account's username (DOMAIN\\user for a domain account) and password; a server that takes only Windows authentication is not supported yet."
            )
        })?;
        // One catalogue read says whether the credentials work at all.
        self.get("CatalogItems?$top=1").await?;
        let version = s(&system, "ProductVersion");
        let product = Some(s(&system, "ProductName"))
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "Power BI Report Server".into());
        let user = match &self.cfg.auth {
            hs_core::service_auth::Auth::Password { user, .. } => user.clone(),
            _ => String::new(),
        };
        Ok(ServerInfo {
            product,
            version_text: version.clone(),
            version: None,
            signed_in_as: user,
            support: support_for(&version),
        })
    }

    fn capabilities(&self) -> Caps {
        Caps {
            browse: true,
            native_sql: false,
            views: true,
            extracts: true,
            search_kinds: &[Kind::Report, Kind::PowerBi, Kind::Kpi, Kind::DataSource],
            tables: false,
            data_section: "",
            jobs: false,
            view_data: true,
            view_files: &[ViewFile::Image, ViewFile::Csv],
            view_filters: false,
            kpis: true,
            extracts_keys: [
                "analytics-kind-refresh",
                "analytics-refresh-sub",
                "analytics-refresh-plans",
                "analytics-refresh-plans-empty",
            ],
        }
    }

    async fn children(&self, parent: &str) -> Result<Vec<Node>> {
        let path = if parent == "root" {
            self.load().await?;
            "/".to_string()
        } else {
            self.path_of(parent)
                .ok_or_else(|| "That folder is not in the catalogue any more.".to_string())?
        };
        Ok(children_of(&self.items.read(), &path))
    }

    async fn search(&self, query: &str, kinds: &[Kind], offset: usize) -> Result<Found> {
        let filter = format!("contains(Name,'{}')", query.replace('\'', "''"));
        let v = self
            .get(&format!(
                "CatalogItems?$filter={}&$top={SEARCH_PAGE}&$skip={offset}&$count=true",
                seg(&filter)
            ))
            .await?;
        let mut nodes: Vec<Node> = v
            .get("value")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(node_of).collect())
            .unwrap_or_default();
        let narrowed = !kinds.is_empty();
        if narrowed {
            nodes.retain(|n| kinds.contains(&n.kind));
        }
        let total = v
            .get("@odata.count")
            .and_then(Value::as_u64)
            .map(|c| c as usize)
            .filter(|_| !narrowed)
            .unwrap_or(nodes.len());
        Ok(Found { nodes, total })
    }

    fn item_url(&self, kind: Kind, id: &str) -> String {
        let base = format!("{}/reports", self.cfg.url);
        let path = self.path_of(id).unwrap_or_default();
        let enc: String = path.split('/').map(seg).collect::<Vec<_>>().join("/");
        match kind {
            Kind::Report if !path.is_empty() => format!("{base}/report{enc}"),
            Kind::PowerBi if !path.is_empty() => format!("{base}/powerbi{enc}"),
            Kind::Kpi if !path.is_empty() => format!("{base}/kpis{enc}"),
            _ if path.is_empty() => format!("{base}/"),
            _ => format!("{base}/browse{enc}"),
        }
    }

    fn web_url(&self) -> String {
        format!("{}/reports/", self.cfg.url)
    }

    async fn view(&self, id: &str) -> Result<ViewInfo> {
        let r = self
            .get(&format!("Reports({})", seg(id)))
            .await
            .unwrap_or(Value::Null);
        let path = s(&r, "Path");
        Ok(ViewInfo {
            id: id.to_string(),
            name: s(&r, "Name"),
            workbook: String::new(),
            project: dirname(&path),
            updated: s(&r, "ModifiedDate"),
        })
    }

    async fn view_file(
        &self,
        id: &str,
        file: ViewFile,
        _width: u32,
        _values: &[(String, String)],
    ) -> Result<Vec<u8>> {
        let format = match file {
            ViewFile::Image => "IMAGE",
            ViewFile::Csv => "CSV",
            ViewFile::Pdf => return Err(super::NOT_OFFERED.into()),
        };
        self.get_bytes(&format!("Reports({})/Export/{format}", seg(id)))
            .await
            .map_err(|e| format!("The server could not render this report as {format}: {e}"))
    }

    async fn view_data(
        &self,
        id: &str,
        values: &[(String, String)],
        limit: usize,
    ) -> Result<Table> {
        let started = std::time::Instant::now();
        let bytes = self.view_file(id, ViewFile::Csv, 0, values).await?;
        let mut t = super::tableau::table_from_csv(&String::from_utf8_lossy(&bytes), limit)?;
        t.millis = started.elapsed().as_millis() as u64;
        Ok(t)
    }

    async fn kpi(&self, id: &str) -> Result<Kpi> {
        let v = self.get(&format!("Kpis({})", seg(id))).await?;
        Ok(kpi_from(&v))
    }

    async fn extracts(&self) -> Result<Vec<ExtractTask>> {
        let plans = self.all("CacheRefreshPlans").await?;
        Ok(plans.iter().map(plan_from).collect())
    }

    async fn disconnect(&self) {}
}

// ---- parsers: JSON in, the screens' types out -------------------------------

/// `/Sales/Regional Sales` is in `/Sales`; `/Sales` is in `/`.
fn dirname(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/".to_string(),
        Some((dir, _)) => dir.to_string(),
    }
}

fn support_for(version: &str) -> Support {
    match version
        .split('.')
        .next()
        .and_then(|m| m.trim().parse::<u32>().ok())
    {
        Some(m) if m >= OLDEST_MAJOR => Support::Supported,
        Some(_) => Support::TooOld,
        None => Support::Unknown,
    }
}

/// A catalogue item as a node; kinds the tab does not show (datasets, mobile
/// reports, resources) are left out.
fn node_of(v: &Value) -> Option<Node> {
    let kind = match s(v, "Type").as_str() {
        "Folder" => Kind::Collection,
        "Report" => Kind::Report,
        "PowerBIReport" => Kind::PowerBi,
        "Kpi" => Kind::Kpi,
        "DataSource" => Kind::DataSource,
        _ => return None,
    };
    let id = s(v, "Id");
    if id.is_empty() {
        return None;
    }
    let description = if kind == Kind::DataSource {
        mask_connection_string(&s(v, "ConnectionString"))
    } else {
        s(v, "Description")
    };
    Some(Node {
        kind,
        id,
        name: s(v, "Name"),
        description,
        collection: dirname(&s(v, "Path")),
        edited: s(v, "ModifiedDate"),
        editor: s(v, "ModifiedBy"),
    })
}

/// The items directly inside the folder at `path`: folders first, each by name.
fn children_of(items: &[Value], path: &str) -> Vec<Node> {
    let mut out: Vec<Node> = items
        .iter()
        .filter(|i| dirname(&s(i, "Path")) == path && s(i, "Path") != "/")
        .filter_map(node_of)
        .collect();
    out.sort_by_key(|n| (n.kind != Kind::Collection, n.name.to_lowercase()));
    out
}

/// The connection string with each secret's value hidden: `Password`, `Pwd`,
/// `Secret`, `Token`, `Account Key`, `Shared Access Key` and `API Key`
/// whatever their case. Anything else is kept as it is.
pub fn mask_connection_string(cs: &str) -> String {
    const SECRET_KEYS: &[&str] = &[
        "password",
        "pwd",
        "secret",
        "client secret",
        "clientsecret",
        "token",
        "accountkey",
        "account key",
        "sharedaccesskey",
        "shared access key",
        "apikey",
        "api key",
    ];
    cs.split(';')
        .map(|part| match part.split_once('=') {
            Some((k, _)) if SECRET_KEYS.contains(&k.trim().to_lowercase().as_str()) => {
                format!("{k}=********")
            }
            _ => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn kpi_from(v: &Value) -> Kpi {
    let data = &v["Data"];
    let num = |x: &Value| x.as_f64().unwrap_or(0.0);
    Kpi {
        id: s(v, "Id"),
        name: s(v, "Name"),
        value: num(&data["Value"]),
        goal: num(&data["Goal"]),
        status: data
            .get("Status")
            .and_then(Value::as_i64)
            .map_or(0, |n| n.clamp(-1, 1) as i32),
        trend: data
            .get("TrendSet")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default(),
        format: s(v, "ValueFormat"),
    }
}

fn plan_from(p: &Value) -> ExtractTask {
    let status = s(p, "LastStatus");
    ExtractTask {
        id: s(p, "Id"),
        kind: s(p, "Description"),
        target_kind: "Report".into(),
        target: s(p, "CatalogItemPath"),
        schedule: s(p, "ScheduleDescription"),
        frequency: s(p, "EventType"),
        failed: u32::from(status.to_lowercase().starts_with("fail")),
        last_run: s(p, "LastRunTime"),
        last_status: status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalogue() -> Vec<Value> {
        vec![
            json!({ "Id": "f1", "Name": "Sales", "Path": "/Sales", "Type": "Folder" }),
            json!({ "Id": "f2", "Name": "Operations", "Path": "/Operations", "Type": "Folder" }),
            json!({ "Id": "r1", "Name": "Regional Sales", "Path": "/Sales/Regional Sales", "Type": "PowerBIReport" }),
            json!({ "Id": "r2", "Name": "Order Fulfilment", "Path": "/Operations/Order Fulfilment", "Type": "Report" }),
            json!({ "Id": "k1", "Name": "Revenue vs Goal", "Path": "/Sales/Revenue vs Goal", "Type": "Kpi" }),
            json!({ "Id": "d1", "Name": "Sales DW", "Path": "/Sales/Sales DW", "Type": "DataSource",
                    "ConnectionString": "Data Source=dw;User ID=u;Password=hunter2" }),
            json!({ "Id": "x1", "Name": "A dataset", "Path": "/Sales/A dataset", "Type": "Dataset" }),
            json!({ "Id": "r0", "Name": "Top", "Path": "/Top", "Type": "Report" }),
        ]
    }

    #[test]
    fn a_path_is_in_the_folder_above_it() {
        assert_eq!(dirname("/Sales/Regional Sales"), "/Sales");
        assert_eq!(dirname("/Sales"), "/");
        assert_eq!(dirname("/A/B/C"), "/A/B");
        assert_eq!(dirname("/"), "/");
    }

    #[test]
    fn the_tree_is_built_from_the_paths_folders_first() {
        let root = children_of(&catalogue(), "/");
        assert_eq!(
            root.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            ["Operations", "Sales", "Top"]
        );
        assert_eq!(root[0].kind, Kind::Collection);
        let sales = children_of(&catalogue(), "/Sales");
        assert_eq!(
            sales
                .iter()
                .map(|n| (n.kind, n.name.as_str()))
                .collect::<Vec<_>>(),
            [
                (Kind::PowerBi, "Regional Sales"),
                (Kind::Kpi, "Revenue vs Goal"),
                (Kind::DataSource, "Sales DW"),
            ],
            "a dataset is left out; the rest by name"
        );
        assert!(
            children_of(&catalogue(), "/Operations")
                .iter()
                .all(|n| n.kind == Kind::Report)
        );
    }

    #[test]
    fn a_connection_strings_secrets_are_masked_and_the_rest_kept() {
        assert_eq!(
            mask_connection_string(
                "Data Source=dw.bench.local;Initial Catalog=Sales;User ID=report_reader;Password=hunter2-not-real"
            ),
            "Data Source=dw.bench.local;Initial Catalog=Sales;User ID=report_reader;Password=********"
        );
        assert_eq!(
            mask_connection_string("server=a;PWD=x;Token = y;AccountKey=z;Shared Access Key=k"),
            "server=a;PWD=********;Token =********;AccountKey=********;Shared Access Key=********"
        );
        assert_eq!(
            mask_connection_string(
                "Provider=Microsoft.ACE.OLEDB.12.0;Data Source=\\\\files\\targets.xlsx"
            ),
            "Provider=Microsoft.ACE.OLEDB.12.0;Data Source=\\\\files\\targets.xlsx"
        );
        // A data source node never carries the secret.
        let d = node_of(&catalogue()[5]).expect("a node");
        assert!(d.description.contains("Password=********") && !d.description.contains("hunter2"));
        assert_eq!(mask_connection_string(""), "");
    }

    #[test]
    fn a_kpi_has_its_value_goal_status_and_trend() {
        let k = kpi_from(
            &json!({ "Id": "k", "Name": "Revenue", "ValueFormat": "Currency",
            "Data": { "Value": 1_284_000, "Goal": 1_500_000, "Status": -1, "TrendSet": [1.0, 1.1, 1.28] } }),
        );
        assert_eq!((k.value, k.goal, k.status), (1_284_000.0, 1_500_000.0, -1));
        assert_eq!(k.trend, [1.0, 1.1, 1.28]);
        assert_eq!(k.format, "Currency");
        let odd = kpi_from(&json!({ "Data": { "Status": 7 } }));
        assert_eq!(odd.status, 1, "clamped");
        assert!(odd.trend.is_empty());
    }

    #[test]
    fn a_refresh_plan_says_when_it_ran_and_whether_it_failed() {
        let ok = plan_from(&json!({ "Id": "p", "Description": "Nightly refresh",
            "CatalogItemPath": "/Sales/Regional Sales", "ScheduleDescription": "Every day at 02:00",
            "EventType": "TimedSubscription", "LastRunTime": "2026-11-03T02:00:12Z",
            "LastStatus": "Completed Data Refresh" }));
        assert_eq!(
            (ok.failed, ok.target.as_str()),
            (0, "/Sales/Regional Sales")
        );
        assert_eq!(ok.last_run, "2026-11-03T02:00:12Z");
        let bad =
            plan_from(&json!({ "LastStatus": "Failure: the data source could not be reached" }));
        assert_eq!(bad.failed, 1);
    }

    #[test]
    fn the_version_decides_the_support_chip() {
        assert_eq!(support_for("15.0.1115.172"), Support::Supported);
        assert_eq!(support_for("14.0.600"), Support::TooOld);
        assert_eq!(support_for(""), Support::Unknown);
    }
}
