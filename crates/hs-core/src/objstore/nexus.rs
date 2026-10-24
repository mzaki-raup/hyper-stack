//! Sonatype Nexus Repository behind the [`ObjectStore`] trait (owner's
//! request, 2026-10-24).
//!
//! Nexus looks like object storage and is not quite: a **repository**
//! stands in for a bucket and an **asset** for an object, but a
//! repository is *typed*, and the type decides what may be done to it.
//! Two differences matter enough that the pane is told about them
//! rather than discovering them by failing:
//!
//! - **A proxy repository is somebody else's content.** Uploading into
//!   one is refused by the server, and should be refused by the UI
//!   before anyone tries.
//! - **Publishing is format-specific.** `raw` and `maven2` accept a
//!   plain `PUT` to the repository path, which is what this driver
//!   does. npm, NuGet, Docker and the rest have their own publish
//!   protocols that are not a file write at all, so for those the
//!   driver reports read-only rather than pretending.
//!
//! Listing goes through the REST API (`/service/rest/v1/assets`), which
//! pages with a `continuationToken` and gives each asset a path, a
//! size, a checksum and an id. The **id** is what deletion needs - a
//! path alone is not enough - so the driver keeps the two together.
//!
//! Reads and writes of content go to `/repository/<name>/<path>`
//! instead: that is the ordinary repository URL any build tool uses,
//! and it streams, where the REST API would not.

use std::path::Path;
use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

use super::s3::{Bucket, Head, Listing, Object, Progress, Result, S3Error};
use super::store::{Backend, Caps, ObjectStore};

/// How to reach a Nexus server.
#[derive(Debug, Clone, Default)]
pub struct NexusConfig {
    /// The server root, e.g. `https://nexus.example.com`. Not the
    /// repository: the driver lists them.
    pub base_url: String,
    pub user: String,
    /// A password or, better, a user token.
    pub password: String,
    /// Only this repository, when the account may not list them all.
    /// Empty means "ask the server what is there".
    pub repository: String,
    pub insecure_tls: bool,
}

pub struct NexusClient {
    http: reqwest::Client,
    base: String,
    user: String,
    password: String,
    only: String,
    /// What the last listing learned about each repository, so `caps`
    /// and the asset ids survive between calls. A listing is the only
    /// thing that knows an asset's id, and deletion needs it.
    known: Mutex<Known>,
}

#[derive(Debug, Default)]
struct Known {
    /// repository name -> (format, type)
    repos: Vec<(String, String, String)>,
    /// (repository, path) -> asset id
    assets: Vec<((String, String), String)>,
}

/// The formats this driver can write, because for them publishing *is*
/// a file write. Everything else is listed and read, never written.
const WRITABLE_FORMATS: &[&str] = &["raw", "maven2"];

impl NexusClient {
    pub fn new(config: NexusConfig) -> Result<Self> {
        let mut builder = reqwest::Client::builder();
        if config.insecure_tls {
            builder = builder.danger_accept_invalid_certs(true);
        }
        let http = builder
            .build()
            .map_err(|e| S3Error::Http(format!("could not build an HTTP client: {e}")))?;
        Ok(Self {
            http,
            base: config.base_url.trim_end_matches('/').to_string(),
            user: config.user,
            password: config.password,
            only: config.repository,
            known: Mutex::new(Known::default()),
        })
    }

    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let r = self.http.request(method, url);
        if self.user.is_empty() {
            r
        } else {
            r.basic_auth(&self.user, Some(&self.password))
        }
    }

    fn api(&self, path: &str) -> String {
        format!("{}/service/rest/v1/{path}", self.base)
    }

    /// The ordinary repository URL, which is what build tools use and
    /// what streams.
    fn content_url(&self, repo: &str, key: &str) -> String {
        format!(
            "{}/repository/{}/{}",
            self.base,
            repo,
            key.trim_start_matches('/')
        )
    }

    async fn get_json(&self, url: &str) -> Result<serde_json::Value> {
        let resp = self
            .request(reqwest::Method::GET, url)
            .send()
            .await
            .map_err(|e| S3Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(status_error(resp.status(), url));
        }
        let text = resp
            .text()
            .await
            .map_err(|e| S3Error::Http(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| S3Error::Parse(e.to_string()))
    }

    /// What a repository is, from whatever the last listing learned.
    fn repo_info(&self, repo: &str) -> Option<(String, String)> {
        let known = self.known.lock().ok()?;
        known
            .repos
            .iter()
            .find(|(n, _, _)| n == repo)
            .map(|(_, f, t)| (f.clone(), t.clone()))
    }

    /// Whether a repository can be written to, and why not when it
    /// cannot. `None` means it can.
    pub fn refusal(&self, repo: &str) -> Option<String> {
        let (format, kind) = self.repo_info(repo)?;
        if kind != "hosted" {
            return Some(format!(
                "{repo} is a {kind} repository, so its content belongs to \
                 somewhere else and cannot be written here"
            ));
        }
        if !WRITABLE_FORMATS.contains(&format.as_str()) {
            return Some(format!(
                "a {format} repository is published with its own tooling \
                 rather than by writing a file, so uploads are not offered"
            ));
        }
        None
    }

    /// Every asset of a repository, following the continuation token.
    async fn all_assets(&self, repo: &str) -> Result<Vec<Asset>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut url = self.api(&format!("assets?repository={repo}"));
            if let Some(t) = &token {
                url.push_str(&format!("&continuationToken={t}"));
            }
            let doc = self.get_json(&url).await?;
            for item in doc
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
            {
                let path = item
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim_start_matches('/')
                    .to_string();
                if path.is_empty() {
                    continue;
                }
                out.push(Asset {
                    id: item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    size: item.get("fileSize").and_then(|v| v.as_u64()).unwrap_or(0),
                    modified: item
                        .get("lastModified")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    etag: item
                        .get("checksum")
                        .and_then(|c| c.get("sha1"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    content_type: item
                        .get("contentType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    path,
                });
            }
            token = doc
                .get("continuationToken")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if token.is_none() {
                break;
            }
        }
        // Remember the ids: deleting an asset needs one, and nothing
        // else in the API will give it for a bare path.
        if let Ok(mut known) = self.known.lock() {
            known.assets.retain(|((r, _), _)| r != repo);
            for a in &out {
                known
                    .assets
                    .push(((repo.to_string(), a.path.clone()), a.id.clone()));
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn asset_id(&self, repo: &str, path: &str) -> Option<String> {
        let known = self.known.lock().ok()?;
        known
            .assets
            .iter()
            .find(|((r, p), _)| r == repo && p == path)
            .map(|(_, id)| id.clone())
    }
}

#[derive(Debug, Clone)]
struct Asset {
    id: String,
    path: String,
    size: u64,
    modified: String,
    etag: String,
    content_type: String,
}

fn status_error(status: reqwest::StatusCode, url: &str) -> S3Error {
    let code = status.as_u16();
    let what = match code {
        401 => "the server refused those credentials",
        403 => "that account may not do this here",
        404 => "there is no such repository or asset",
        // Nexus answers 400 for a write a repository's format or policy
        // does not allow, which is worth saying plainly.
        400 => "the repository refused the request - check its format and write policy",
        _ => "the server refused the request",
    };
    S3Error::Api {
        status: code,
        code: status.canonical_reason().unwrap_or("Error").to_string(),
        message: format!("{what} - {url}"),
    }
}

/// Group a flat asset list into one delimited page, exactly as S3's
/// `ListObjectsV2` with `delimiter=/` would.
fn delimit(assets: &[Asset], prefix: &str) -> Listing {
    let mut listing = Listing::default();
    let mut seen: Vec<String> = Vec::new();
    for a in assets {
        let Some(rest) = a.path.strip_prefix(prefix) else {
            continue;
        };
        match rest.find('/') {
            Some(i) => {
                let folder = format!("{}{}/", prefix, &rest[..i]);
                if !seen.contains(&folder) {
                    seen.push(folder.clone());
                    listing.prefixes.push(folder);
                }
            }
            None => listing.objects.push(Object {
                key: a.path.clone(),
                size: a.size,
                modified: a.modified.clone(),
                etag: a.etag.clone(),
                storage_class: String::new(),
            }),
        }
    }
    listing
}

#[async_trait::async_trait]
impl ObjectStore for NexusClient {
    fn backend(&self) -> Backend {
        Backend::Nexus
    }

    fn caps(&self) -> Caps {
        Caps {
            containers: true,
            // Whether *this* repository takes an upload depends on its
            // format and type, which `refusal` answers per repository.
            // The capability says the backend can write at all.
            upload: true,
            delete: true,
            ..Caps::NONE
        }
    }

    async fn containers(&self) -> Result<Vec<Bucket>> {
        let doc = self.get_json(&self.api("repositories")).await?;
        let rows = doc.as_array().cloned().unwrap_or_default();
        let mut out = Vec::new();
        let mut learned = Vec::new();
        for r in rows {
            let name = r
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            let format = r
                .get("format")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let kind = r
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            learned.push((name.clone(), format.clone(), kind.clone()));
            if !self.only.is_empty() && self.only != name {
                continue;
            }
            out.push(Bucket {
                name,
                // The row that would be a creation date on S3 says what
                // this repository *is*, which is the useful thing here.
                created: format!("{format} · {kind}"),
            });
        }
        if let Ok(mut known) = self.known.lock() {
            known.repos = learned;
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn list(
        &self,
        container: &str,
        prefix: &str,
        delimited: bool,
        _token: Option<&str>,
        _max_keys: u32,
    ) -> Result<Listing> {
        // The whole repository, then grouped here: Nexus's own paging
        // is by asset across the repository and does not understand
        // prefixes, so a page of it is not a page of a folder.
        let assets = self.all_assets(container).await?;
        if !delimited {
            return Ok(Listing {
                objects: assets
                    .iter()
                    .filter(|a| a.path.starts_with(prefix))
                    .map(|a| Object {
                        key: a.path.clone(),
                        size: a.size,
                        modified: a.modified.clone(),
                        etag: a.etag.clone(),
                        storage_class: String::new(),
                    })
                    .collect(),
                prefixes: Vec::new(),
                next: None,
            });
        }
        Ok(delimit(&assets, prefix))
    }

    async fn head(&self, container: &str, key: &str) -> Result<Head> {
        let assets = self.all_assets(container).await?;
        let a = assets
            .into_iter()
            .find(|a| a.path == key.trim_start_matches('/'))
            .ok_or_else(|| S3Error::Api {
                status: 404,
                code: "NotFound".into(),
                message: format!("{container} has no asset at {key}"),
            })?;
        Ok(Head {
            size: a.size,
            content_type: a.content_type,
            etag: a.etag,
            modified: a.modified,
            storage_class: String::new(),
            version_id: None,
            metadata: Vec::new(),
        })
    }

    async fn get_bytes(&self, container: &str, key: &str) -> Result<Vec<u8>> {
        let url = self.content_url(container, key);
        let resp = self
            .request(reqwest::Method::GET, &url)
            .send()
            .await
            .map_err(|e| S3Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(status_error(resp.status(), &url));
        }
        Ok(resp
            .bytes()
            .await
            .map_err(|e| S3Error::Http(e.to_string()))?
            .to_vec())
    }

    async fn download(
        &self,
        container: &str,
        key: &str,
        dest: &Path,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Result<u64> {
        use tokio::io::AsyncWriteExt;
        let url = self.content_url(container, key);
        let mut resp = self
            .request(reqwest::Method::GET, &url)
            .send()
            .await
            .map_err(|e| S3Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(status_error(resp.status(), &url));
        }
        let total = resp.content_length().unwrap_or(0);
        let mut file = tokio::fs::File::create(dest)
            .await
            .map_err(|e| S3Error::Io(e.to_string()))?;
        let mut done = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(S3Error::Cancelled);
            }
            let chunk = resp
                .chunk()
                .await
                .map_err(|e| S3Error::Http(e.to_string()))?;
            let Some(chunk) = chunk else { break };
            file.write_all(&chunk)
                .await
                .map_err(|e| S3Error::Io(e.to_string()))?;
            done += chunk.len() as u64;
            progress(done, total);
        }
        file.flush().await.map_err(|e| S3Error::Io(e.to_string()))?;
        Ok(done)
    }

    async fn put_bytes(
        &self,
        container: &str,
        key: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<()> {
        // Refuse here as well as in the UI: a pane is not the only
        // caller, and a proxy repository answering 400 three layers
        // down is a worse message than this one.
        if let Some(why) = self.refusal(container) {
            return Err(S3Error::Api {
                status: 400,
                code: "NotWritable".into(),
                message: why,
            });
        }
        let url = self.content_url(container, key);
        let mut r = self.request(reqwest::Method::PUT, &url).body(body);
        if !content_type.is_empty() {
            r = r.header("Content-Type", content_type);
        }
        let resp = r.send().await.map_err(|e| S3Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(status_error(resp.status(), &url));
        }
        Ok(())
    }

    async fn upload_file(
        &self,
        container: &str,
        key: &str,
        path: &Path,
        progress: Progress,
        _cancel: CancellationToken,
    ) -> Result<()> {
        let body = tokio::fs::read(path)
            .await
            .map_err(|e| S3Error::Io(e.to_string()))?;
        let total = body.len() as u64;
        self.put_bytes(container, key, body, "").await?;
        progress(total, total);
        Ok(())
    }

    async fn delete(&self, container: &str, key: &str) -> Result<()> {
        let key = key.trim_start_matches('/');
        // The id is what deletion takes. If this key has not been
        // listed yet, list it now rather than failing for a reason the
        // user cannot see.
        let id = match self.asset_id(container, key) {
            Some(id) => id,
            None => {
                self.all_assets(container).await?;
                self.asset_id(container, key).ok_or_else(|| S3Error::Api {
                    status: 404,
                    code: "NotFound".into(),
                    message: format!("{container} has no asset at {key}"),
                })?
            }
        };
        let url = self.api(&format!("assets/{id}"));
        let resp = self
            .request(reqwest::Method::DELETE, &url)
            .send()
            .await
            .map_err(|e| S3Error::Http(e.to_string()))?;
        if !resp.status().is_success() && resp.status().as_u16() != 404 {
            return Err(status_error(resp.status(), &url));
        }
        Ok(())
    }

    async fn delete_many(&self, container: &str, keys: &[String]) -> Result<Vec<(String, String)>> {
        let mut failed = Vec::new();
        for k in keys {
            if let Err(e) = self.delete(container, k).await {
                failed.push((k.clone(), e.to_string()));
            }
        }
        Ok(failed)
    }

    async fn list_all_keys(&self, container: &str, prefix: &str) -> Result<Vec<String>> {
        Ok(self
            .all_assets(container)
            .await?
            .into_iter()
            .map(|a| a.path)
            .filter(|p| p.starts_with(prefix))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> NexusClient {
        NexusClient::new(NexusConfig {
            base_url: "https://nexus.example.com/".into(),
            user: "admin".into(),
            password: "pw".into(),
            repository: String::new(),
            insecure_tls: false,
        })
        .expect("a client")
    }

    /// Content goes to the repository URL a build tool would use, and
    /// the REST API lives somewhere else entirely.
    #[test]
    fn content_and_the_api_are_different_urls() {
        let c = client();
        assert_eq!(
            c.content_url("hs-raw", "docs/a.txt"),
            "https://nexus.example.com/repository/hs-raw/docs/a.txt"
        );
        // A trailing slash on the base must not double up.
        assert_eq!(
            c.api("repositories"),
            "https://nexus.example.com/service/rest/v1/repositories"
        );
        assert_eq!(
            c.content_url("r", "/leading"),
            "https://nexus.example.com/repository/r/leading"
        );
    }

    /// Whether a repository can be written to is a property of the
    /// repository, not of the server: a proxy holds someone else's
    /// content, and most formats are published by their own tooling
    /// rather than by writing a file.
    #[test]
    fn a_proxy_and_a_format_that_cannot_be_written_are_refused_with_a_reason() {
        let c = client();
        {
            let mut k = c.known.lock().unwrap();
            k.repos = vec![
                ("hs-raw".into(), "raw".into(), "hosted".into()),
                ("maven-releases".into(), "maven2".into(), "hosted".into()),
                ("maven-central".into(), "maven2".into(), "proxy".into()),
                ("npm-hosted".into(), "npm".into(), "hosted".into()),
            ];
        }
        assert!(
            c.refusal("hs-raw").is_none(),
            "a raw hosted repo takes files"
        );
        assert!(c.refusal("maven-releases").is_none());

        let proxy = c.refusal("maven-central").expect("a proxy is refused");
        assert!(proxy.contains("proxy"), "{proxy}");

        let npm = c.refusal("npm-hosted").expect("npm is refused");
        assert!(npm.contains("npm") && npm.contains("own tooling"), "{npm}");

        // A repository nothing is known about yet is not refused on a
        // guess; the server gets to answer.
        assert!(c.refusal("never-listed").is_none());
    }

    /// Nexus answers a flat list of every asset, so the folder grouping
    /// the pane shows is made here - the same shape `ListObjectsV2`
    /// with a delimiter returns.
    #[test]
    fn a_flat_asset_list_groups_into_folders() {
        let a = |p: &str| Asset {
            id: format!("id-{p}"),
            path: p.into(),
            size: 10,
            modified: String::new(),
            etag: String::new(),
            content_type: String::new(),
        };
        let assets = vec![
            a("b.txt"),
            a("docs/a.txt"),
            a("docs/deep/c.txt"),
            a("docs/d.txt"),
            a("other/e.txt"),
        ];
        let root = delimit(&assets, "");
        assert_eq!(
            root.objects.iter().map(|o| &o.key).collect::<Vec<_>>(),
            ["b.txt"]
        );
        assert_eq!(root.prefixes, ["docs/", "other/"], "each folder once");

        let docs = delimit(&assets, "docs/");
        assert_eq!(
            docs.objects.iter().map(|o| &o.key).collect::<Vec<_>>(),
            ["docs/a.txt", "docs/d.txt"]
        );
        assert_eq!(docs.prefixes, ["docs/deep/"]);
    }

    /// The pane needs the asset's **id** to delete it, and only a
    /// listing has ever seen one.
    #[test]
    fn an_assets_id_is_remembered_from_the_listing() {
        let c = client();
        assert_eq!(c.asset_id("r", "a.txt"), None);
        c.known
            .lock()
            .unwrap()
            .assets
            .push((("r".into(), "a.txt".into()), "the-id".into()));
        assert_eq!(c.asset_id("r", "a.txt").as_deref(), Some("the-id"));
        assert_eq!(c.asset_id("other", "a.txt"), None, "ids are per repository");
    }

    /// A refusal has to carry the server's own status, or the pane
    /// cannot tell "wrong password" from "no such repository".
    #[test]
    fn the_status_decides_what_the_message_says() {
        let unauth = status_error(reqwest::StatusCode::UNAUTHORIZED, "u");
        assert!(format!("{unauth}").contains("credentials"), "{unauth}");
        let forbidden = status_error(reqwest::StatusCode::FORBIDDEN, "u");
        assert!(format!("{forbidden}").contains("may not"), "{forbidden}");
        let bad = status_error(reqwest::StatusCode::BAD_REQUEST, "u");
        assert!(format!("{bad}").contains("write policy"), "{bad}");
    }
}
