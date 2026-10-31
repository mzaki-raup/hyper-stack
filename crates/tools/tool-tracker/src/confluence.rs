//! Confluence through REST v2 (Cloud) or v1 (Data Center)
//! (docs/tools/tracker.md §4). Pages, not items: spaces and their page
//! tree, a page in storage format shown as Markdown, edited and published
//! back with its version bumped, and page comments. The query is CQL,
//! through the v1 search both deployments still offer.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Value, json};

use crate::driver::{
    Caps, Comment, CreateForm, Detail, Driver, Edit, Found, Item, MAX_COMMENTS, NewItem, Page,
    PageRef, Result, ServerInfo, Space, Transition, iso_ms, q,
};
use crate::markup::{md_to_storage, storage_to_md};
use crate::profile::ConnectConfig;
use hs_core::service_http::{Http, s, seg};

/// Pages and spaces read at most (one listing each).
const MAX_PAGES: usize = 250;

pub struct Confluence {
    http: Http,
    /// Where page links start: `…/wiki` on Cloud.
    web: String,
    cloud: bool,
    space: String,
    info: ServerInfo,
}

impl Confluence {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Confluence> {
        let http = cfg.http(&cfg.url)?;
        let web = if cfg.cloud {
            format!("{}/wiki", cfg.url)
        } else {
            cfg.url.clone()
        };
        let c = Confluence {
            http,
            web,
            cloud: cfg.cloud,
            space: cfg.project.clone(),
            info: ServerInfo {
                product: "Confluence".into(),
                version: String::new(),
            },
        };
        c.http
            .get_json(&format!("{}/space?limit=1", c.v1()))
            .await?;
        Ok(c)
    }

    fn v1(&self) -> &'static str {
        if self.cloud {
            "/wiki/rest/api"
        } else {
            "/rest/api"
        }
    }

    fn link(&self, v: &Value) -> String {
        let w = v
            .pointer("/_links/webui")
            .and_then(Value::as_str)
            .unwrap_or("");
        if w.is_empty() {
            String::new()
        } else {
            format!("{}{w}", self.web)
        }
    }

    async fn comments(&self, id: &str) -> Result<Vec<Comment>> {
        let list = if self.cloud {
            self.http
                .get_json(&format!(
                    "/wiki/api/v2/pages/{}/footer-comments?body-format=storage&limit={MAX_COMMENTS}",
                    seg(id)
                ))
                .await?
        } else {
            self.http
                .get_json(&format!(
                    "/rest/api/content/{}/child/comment?expand=body.storage,version&limit={MAX_COMMENTS}",
                    seg(id)
                ))
                .await?
        };
        Ok(list
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|c| {
                let version = c.get("version").cloned().unwrap_or_default();
                Comment {
                    author: version
                        .get("by")
                        .map(|b| s(b, "displayName"))
                        .unwrap_or_default(),
                    created_ms: iso_ms(&s(&version, "createdAt"))
                        .or_else(|| iso_ms(&s(&version, "when"))),
                    body: storage_to_md(
                        c.pointer("/body/storage/value")
                            .and_then(Value::as_str)
                            .unwrap_or(""),
                    )
                    .0,
                }
            })
            .collect())
    }
}

#[async_trait]
impl Driver for Confluence {
    fn info(&self) -> &ServerInfo {
        &self.info
    }

    fn caps(&self) -> Caps {
        Caps {
            transition: false,
            edit: false,
            comment: true,
            create: false,
            pages: true,
        }
    }

    fn default_query(&self) -> String {
        if self.space.is_empty() {
            "type = page ORDER BY lastmodified DESC".into()
        } else {
            format!(
                "space = \"{}\" AND type = page ORDER BY lastmodified DESC",
                self.space
            )
        }
    }

    async fn search(&mut self, query: &str, from: Option<&str>) -> Result<Found> {
        let start: u64 = from.and_then(|p| p.parse().ok()).unwrap_or(0);
        let v = self
            .http
            .get_json(&format!(
                "{}/content/search?cql={}&limit={}&start={start}&expand=space,version",
                self.v1(),
                q(query),
                crate::driver::PAGE
            ))
            .await?;
        let items: Vec<Item> = v
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|p| Item {
                key: s(p, "id"),
                summary: s(p, "title"),
                status: p.get("space").map(|x| s(x, "key")).unwrap_or_default(),
                assignee: p
                    .pointer("/version/by")
                    .map(|b| s(b, "displayName"))
                    .unwrap_or_default(),
                priority: String::new(),
                updated_ms: p.get("version").and_then(|x| iso_ms(&s(x, "when"))),
                due: String::new(),
                url: self.link(p),
            })
            .collect();
        let size = v.get("size").and_then(Value::as_u64).unwrap_or(0);
        // A next link says there is more; the count is not given.
        let next = v
            .pointer("/_links/next")
            .is_some()
            .then(|| (start + size).to_string());
        Ok(Found {
            items,
            next,
            total: v.get("totalSize").and_then(Value::as_u64),
        })
    }

    async fn item(&mut self, _key: &str) -> Result<Detail> {
        Err("a Confluence result opens as a page".into())
    }

    async fn transitions(&mut self, _key: &str) -> Result<Vec<Transition>> {
        Ok(Vec::new())
    }

    async fn transition(&mut self, _: &str, _: &Transition) -> Result<String> {
        Err("pages have no status".into())
    }

    async fn edit(&mut self, _: &str, _: &Edit) -> Result<String> {
        Err("a page is edited through Publish".into())
    }

    async fn comment(&mut self, key: &str, body: &str) -> Result<String> {
        let value = md_to_storage(body);
        let rb = if self.cloud {
            self.http
                .request(Method::POST, "/wiki/api/v2/footer-comments")
                .json(&json!({
                    "pageId": key,
                    "body": { "representation": "storage", "value": value },
                }))
        } else {
            self.http
                .request(Method::POST, "/rest/api/content")
                .json(&json!({
                    "type": "comment",
                    "container": { "id": key, "type": "page" },
                    "body": { "storage": { "value": value, "representation": "storage" } },
                }))
        };
        self.http.json(rb).await?;
        Ok("Comment added".into())
    }

    async fn fields(&mut self, _: &str) -> Result<CreateForm> {
        Ok(CreateForm::default())
    }

    async fn create(&mut self, _: &NewItem) -> Result<String> {
        Err("pages are not created from here yet".into())
    }

    async fn spaces(&mut self) -> Result<Vec<Space>> {
        let v = if self.cloud {
            self.http
                .get_json(&format!("/wiki/api/v2/spaces?limit={MAX_PAGES}"))
                .await?
        } else {
            self.http
                .get_json(&format!("/rest/api/space?limit={MAX_PAGES}"))
                .await?
        };
        let mut out: Vec<Space> = v
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|x| Space {
                id: s(x, "id"),
                key: s(x, "key"),
                name: s(x, "name"),
            })
            .collect();
        out.sort_by_key(|x| x.name.to_lowercase());
        Ok(out)
    }

    async fn pages(&mut self, space: &Space) -> Result<Vec<PageRef>> {
        let v = if self.cloud {
            self.http
                .get_json(&format!(
                    "/wiki/api/v2/spaces/{}/pages?limit={MAX_PAGES}&status=current",
                    seg(&space.id)
                ))
                .await?
        } else {
            self.http
                .get_json(&format!(
                    "/rest/api/content?spaceKey={}&type=page&limit={MAX_PAGES}&expand=ancestors",
                    q(&space.key)
                ))
                .await?
        };
        Ok(v.get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|p| PageRef {
                id: s(p, "id"),
                title: s(p, "title"),
                parent: if self.cloud {
                    s(p, "parentId")
                } else {
                    p.get("ancestors")
                        .and_then(Value::as_array)
                        .and_then(|a| a.last())
                        .map(|a| s(a, "id"))
                        .unwrap_or_default()
                },
            })
            .collect())
    }

    async fn page(&mut self, id: &str) -> Result<Page> {
        let v = if self.cloud {
            self.http
                .get_json(&format!(
                    "/wiki/api/v2/pages/{}?body-format=storage",
                    seg(id)
                ))
                .await?
        } else {
            self.http
                .get_json(&format!(
                    "/rest/api/content/{}?expand=body.storage,version",
                    seg(id)
                ))
                .await?
        };
        let storage = v
            .pointer("/body/storage/value")
            .and_then(Value::as_str)
            .unwrap_or("");
        let (body, lost) = storage_to_md(storage);
        Ok(Page {
            id: s(&v, "id"),
            title: s(&v, "title"),
            version: v
                .pointer("/version/number")
                .and_then(Value::as_u64)
                .unwrap_or(1),
            body,
            read_only: lost.map(|l| format!("the page holds content Markdown cannot keep ({l})")),
            url: self.link(&v),
            comments: self.comments(id).await?,
        })
    }

    async fn publish(&mut self, page: &Page, body: &str) -> Result<String> {
        let value = md_to_storage(body);
        let version = page.version + 1;
        let rb = if self.cloud {
            self.http
                .request(
                    Method::PUT,
                    &format!("/wiki/api/v2/pages/{}", seg(&page.id)),
                )
                .json(&json!({
                    "id": page.id,
                    "status": "current",
                    "title": page.title,
                    "body": { "representation": "storage", "value": value },
                    "version": { "number": version },
                }))
        } else {
            self.http
                .request(Method::PUT, &format!("/rest/api/content/{}", seg(&page.id)))
                .json(&json!({
                    "id": page.id,
                    "type": "page",
                    "title": page.title,
                    "body": { "storage": { "value": value, "representation": "storage" } },
                    "version": { "number": version },
                }))
        };
        self.http.json(rb).await?;
        Ok(format!("{} published as version {version}", page.title))
    }
}
