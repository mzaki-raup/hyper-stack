//! A WebDAV client behind the [`ObjectStore`] trait (owner's request,
//! 2026-10-24).
//!
//! WebDAV is a file tree over HTTP, so most of the work here is making
//! a tree look like the flat keyspace the browser wants:
//!
//! - **There are no buckets.** The driver reports one pseudo-container
//!   for the share root, so the pane has something to open.
//! - **Directories are real.** On S3 a key with slashes in it is just a
//!   key; on WebDAV the parent collections have to exist first, or a
//!   `PUT` fails with 409. `upload` creates them (`MKCOL`), which is
//!   what every WebDAV client does and what anyone expects.
//! - **Listing is `PROPFIND`, not GET.** `Depth: 1` lists one level,
//!   which is exactly the delimited listing the pane asks for; the
//!   recursive case walks levels rather than asking for `Depth:
//!   infinity`, which many servers refuse outright.
//!
//! The XML is read with the same small reader the S3 client uses: a
//! WebDAV response is namespaced (`D:` by convention but *not* by
//! rule), so every element is matched on its local name.

use std::path::Path;

use tokio_util::sync::CancellationToken;

use super::s3::{Bucket, Head, Listing, Object, Progress, Result, S3Error};
use super::store::{Backend, Caps, ObjectStore};
use super::xml;

/// How to reach a DAV share.
#[derive(Debug, Clone, Default)]
pub struct WebdavConfig {
    /// The collection the browser opens at, e.g.
    /// `https://files.example.com/remote.php/dav/files/alice`.
    pub base_url: String,
    pub user: String,
    pub password: String,
    /// Accept a certificate that does not verify. Off, and the form
    /// says what it gives up.
    pub insecure_tls: bool,
}

pub struct WebdavClient {
    http: reqwest::Client,
    base: String,
    user: String,
    password: String,
}

/// The one pseudo-container. WebDAV has no bucket level, but the pane
/// is built around one, so the share root is presented as a single
/// entry rather than inventing a level that does not exist.
pub const ROOT: &str = "/";

impl WebdavClient {
    pub fn new(config: WebdavConfig) -> Result<Self> {
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
        })
    }

    /// The absolute URL of a key under the share.
    ///
    /// Each segment is encoded on its own, so a space or a `#` in a
    /// name survives but the slashes that make the path keep their
    /// meaning.
    fn url_for(&self, key: &str) -> String {
        let path = key
            .trim_start_matches('/')
            .split('/')
            .map(encode_segment)
            .collect::<Vec<_>>()
            .join("/");
        if path.is_empty() {
            format!("{}/", self.base)
        } else {
            format!("{}/{}", self.base, path)
        }
    }

    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let r = self.http.request(method, url);
        if self.user.is_empty() {
            r
        } else {
            r.basic_auth(&self.user, Some(&self.password))
        }
    }

    /// `PROPFIND` one level under `prefix`.
    async fn propfind(&self, prefix: &str, depth: &str) -> Result<Vec<Entry>> {
        let url = self.url_for(prefix);
        let resp = self
            .request(
                reqwest::Method::from_bytes(b"PROPFIND").expect("PROPFIND is a valid method"),
                &url,
            )
            .header("Depth", depth)
            .header("Content-Type", "application/xml")
            .body(PROPFIND_BODY)
            .send()
            .await
            .map_err(|e| S3Error::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(status_error(status, &url));
        }
        let text = resp
            .text()
            .await
            .map_err(|e| S3Error::Http(e.to_string()))?;
        let root = xml::parse(&text).map_err(S3Error::Parse)?;
        // The *base* path as the server spells it, so the listing can be
        // made relative to it. The first response is the collection
        // itself, which is how a server tells us its own prefix.
        let mut out = Vec::new();
        for r in root.all("response") {
            let href = r.text_of("href");
            if href.is_empty() {
                continue;
            }
            let is_dir = r
                .child("propstat")
                .and_then(|p| p.child("prop"))
                .and_then(|p| p.child("resourcetype"))
                .is_some_and(|rt| rt.child("collection").is_some());
            let prop = r.child("propstat").and_then(|p| p.child("prop"));
            let size = prop
                .map(|p| p.text_of("getcontentlength"))
                .unwrap_or_default()
                .trim()
                .parse()
                .unwrap_or(0);
            out.push(Entry {
                href: decode_path(&href),
                is_dir,
                size,
                modified: prop
                    .map(|p| p.text_of("getlastmodified"))
                    .unwrap_or_default(),
                etag: prop
                    .map(|p| p.text_of("getetag"))
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string(),
                content_type: prop
                    .map(|p| p.text_of("getcontenttype"))
                    .unwrap_or_default(),
            });
        }
        Ok(out)
    }

    /// The path the share's own root has on the server, so hrefs can be
    /// cut down to keys.
    fn base_path(&self) -> String {
        // `https://host/remote.php/dav` -> `/remote.php/dav`
        let after_scheme = self
            .base
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(&self.base);
        match after_scheme.find('/') {
            Some(i) => decode_path(&after_scheme[i..]),
            None => String::new(),
        }
    }

    /// Turn an href into a key relative to the share root.
    fn key_of(&self, href: &str) -> String {
        let base = self.base_path();
        let rest = href.strip_prefix(base.as_str()).unwrap_or(href);
        rest.trim_start_matches('/').to_string()
    }

    /// Create every missing collection on the way to `key`.
    async fn ensure_parents(&self, key: &str) -> Result<()> {
        let Some((dir, _)) = key.rsplit_once('/') else {
            return Ok(());
        };
        let mut built = String::new();
        for part in dir.split('/').filter(|p| !p.is_empty()) {
            if !built.is_empty() {
                built.push('/');
            }
            built.push_str(part);
            let url = self.url_for(&built);
            let resp = self
                .request(
                    reqwest::Method::from_bytes(b"MKCOL").expect("MKCOL is a valid method"),
                    &url,
                )
                .send()
                .await
                .map_err(|e| S3Error::Http(e.to_string()))?;
            // 405 is "it is already there", which is success for us.
            // 409 means a *parent* is missing, and the loop above is
            // what prevents that.
            let code = resp.status().as_u16();
            if !resp.status().is_success() && code != 405 {
                return Err(status_error(resp.status(), &url));
            }
        }
        Ok(())
    }
}

/// One `<response>` of a PROPFIND.
#[derive(Debug, Clone)]
struct Entry {
    href: String,
    is_dir: bool,
    size: u64,
    modified: String,
    etag: String,
    content_type: String,
}

/// Ask for the handful of properties the pane shows. Asking for
/// `<allprop/>` makes some servers return a great deal more.
const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<propfind xmlns="DAV:"><prop>
<resourcetype/><getcontentlength/><getlastmodified/><getetag/><getcontenttype/>
</prop></propfind>"#;

fn status_error(status: reqwest::StatusCode, url: &str) -> S3Error {
    let code = status.as_u16();
    let what = match code {
        401 | 403 => "the server refused those credentials",
        404 => "there is nothing at that path",
        405 => "the server does not allow that here",
        409 => "a parent folder does not exist",
        423 => "that path is locked by someone else",
        507 => "the server is out of space",
        _ => "the server refused the request",
    };
    S3Error::Api {
        status: code,
        code: status.canonical_reason().unwrap_or("Error").to_string(),
        message: format!("{what} - {url}"),
    }
}

/// Percent-encode one path segment: everything that is not unreserved.
fn encode_segment(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for b in part.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~') {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Undo percent-encoding in a path the server sent back.
fn decode_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[async_trait::async_trait]
impl ObjectStore for WebdavClient {
    fn backend(&self) -> Backend {
        Backend::Webdav
    }

    fn caps(&self) -> Caps {
        Caps {
            // One share, no bucket level: the pane opens it directly.
            containers: false,
            upload: true,
            delete: true,
            // Directories are real here, which is the one way WebDAV
            // differs from the other two in a way the pane must know.
            needs_directories: true,
            ..Caps::NONE
        }
    }

    async fn containers(&self) -> Result<Vec<Bucket>> {
        Ok(vec![Bucket {
            name: ROOT.to_string(),
            created: String::new(),
        }])
    }

    async fn list(
        &self,
        _container: &str,
        prefix: &str,
        delimited: bool,
        _token: Option<&str>,
        _max_keys: u32,
    ) -> Result<Listing> {
        // `Depth: 1` is one level, which is the delimited listing. The
        // undelimited case walks the tree, because `Depth: infinity` is
        // refused by a great many servers (and is a bad idea on a large
        // share even where it is allowed).
        if !delimited {
            let keys = self.list_all_keys(_container, prefix).await?;
            let mut objects = Vec::new();
            for k in keys {
                let h = self.head(_container, &k).await?;
                objects.push(Object {
                    key: k,
                    size: h.size,
                    modified: h.modified,
                    etag: h.etag,
                    storage_class: String::new(),
                });
            }
            return Ok(Listing {
                objects,
                prefixes: Vec::new(),
                next: None,
            });
        }
        let entries = self.propfind(prefix, "1").await?;
        let here = self.key_of(&self.url_path(prefix));
        let mut listing = Listing::default();
        for e in entries {
            let key = self.key_of(&e.href);
            // The first entry is the collection itself.
            if key.trim_end_matches('/') == here.trim_end_matches('/') {
                continue;
            }
            if e.is_dir {
                listing
                    .prefixes
                    .push(format!("{}/", key.trim_end_matches('/')));
            } else {
                listing.objects.push(Object {
                    key,
                    size: e.size,
                    modified: e.modified,
                    etag: e.etag,
                    storage_class: String::new(),
                });
            }
        }
        listing.objects.sort_by(|a, b| a.key.cmp(&b.key));
        listing.prefixes.sort();
        Ok(listing)
    }

    async fn head(&self, _container: &str, key: &str) -> Result<Head> {
        let entries = self.propfind(key, "0").await?;
        let e = entries.into_iter().next().ok_or_else(|| S3Error::Api {
            status: 404,
            code: "NotFound".into(),
            message: format!("there is nothing at {key}"),
        })?;
        Ok(Head {
            size: e.size,
            content_type: e.content_type,
            etag: e.etag,
            modified: e.modified,
            storage_class: String::new(),
            version_id: None,
            metadata: Vec::new(),
        })
    }

    async fn get_bytes(&self, _container: &str, key: &str) -> Result<Vec<u8>> {
        let url = self.url_for(key);
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
        _container: &str,
        key: &str,
        dest: &Path,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Result<u64> {
        use tokio::io::AsyncWriteExt;
        let url = self.url_for(key);
        let resp = self
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
        let mut stream = resp;
        loop {
            if cancel.is_cancelled() {
                return Err(S3Error::Cancelled);
            }
            let chunk = stream
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
        _container: &str,
        key: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<()> {
        self.ensure_parents(key).await?;
        let url = self.url_for(key);
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
        // Whole-file, because WebDAV has no multipart: a server that
        // wants chunks offers its own extension and no two agree.
        let body = tokio::fs::read(path)
            .await
            .map_err(|e| S3Error::Io(e.to_string()))?;
        let total = body.len() as u64;
        self.put_bytes(container, key, body, "").await?;
        progress(total, total);
        Ok(())
    }

    async fn delete(&self, _container: &str, key: &str) -> Result<()> {
        let url = self.url_for(key);
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
        // One request each: WebDAV has no bulk delete, and pretending
        // otherwise would mean a half-done delete reported as whole.
        let mut failed = Vec::new();
        for k in keys {
            if let Err(e) = self.delete(container, k).await {
                failed.push((k.clone(), e.to_string()));
            }
        }
        Ok(failed)
    }

    async fn list_all_keys(&self, container: &str, prefix: &str) -> Result<Vec<String>> {
        // Walk a level at a time. `Depth: infinity` is refused by many
        // servers and is unkind to the rest.
        let mut out = Vec::new();
        let mut todo = vec![prefix.to_string()];
        while let Some(dir) = todo.pop() {
            let page = self.list(container, &dir, true, None, 0).await?;
            out.extend(page.objects.into_iter().map(|o| o.key));
            todo.extend(page.prefixes);
        }
        out.sort();
        Ok(out)
    }

    async fn create_directory(&self, _container: &str, prefix: &str) -> Result<()> {
        // `ensure_parents` builds every level of a *key*, so give it a
        // name under the folder being made.
        self.ensure_parents(&format!("{}/x", prefix.trim_end_matches('/')))
            .await
    }
}

impl WebdavClient {
    /// The path part of a key's URL, for comparing against an href.
    fn url_path(&self, key: &str) -> String {
        let url = self.url_for(key);
        let after = url.split_once("://").map(|(_, r)| r).unwrap_or(&url);
        match after.find('/') {
            Some(i) => decode_path(&after[i..]),
            None => "/".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> WebdavClient {
        WebdavClient::new(WebdavConfig {
            base_url: "https://files.example.com/remote.php/dav/files/alice".into(),
            user: "alice".into(),
            password: "pw".into(),
            insecure_tls: false,
        })
        .expect("a client")
    }

    /// A name with a space or a `#` in it must survive, while the
    /// slashes that make the path keep their meaning.
    #[test]
    fn each_segment_is_encoded_but_the_slashes_are_not() {
        let c = client();
        assert_eq!(
            c.url_for("docs/my report #2.txt"),
            "https://files.example.com/remote.php/dav/files/alice/docs/my%20report%20%232.txt"
        );
        // A leading slash is not a second one.
        assert_eq!(c.url_for("/a"), c.url_for("a"));
        assert!(c.url_for("").ends_with("/alice/"));
    }

    /// The server answers with its own absolute paths; a key is what is
    /// left after the share's own prefix.
    #[test]
    fn an_href_becomes_a_key_relative_to_the_share() {
        let c = client();
        assert_eq!(c.base_path(), "/remote.php/dav/files/alice");
        assert_eq!(
            c.key_of("/remote.php/dav/files/alice/docs/a.txt"),
            "docs/a.txt"
        );
        // Percent-encoded on the way back, too.
        assert_eq!(
            c.key_of(&decode_path("/remote.php/dav/files/alice/my%20file.txt")),
            "my file.txt"
        );
        // An href from a server that answers with a full URL path we do
        // not recognise is kept rather than mangled.
        assert_eq!(c.key_of("/other/place.txt"), "other/place.txt");
    }

    #[test]
    fn percent_decoding_survives_multi_byte_text() {
        assert_eq!(decode_path("/a/%E6%97%A5%E6%9C%AC.txt"), "/a/日本.txt");
        // A stray `%` is not an escape and must not eat the next bytes.
        assert_eq!(decode_path("/100%"), "/100%");
        assert_eq!(decode_path("/a%zz"), "/a%zz");
    }

    /// WebDAV has no buckets and no versioning, and directories are
    /// real. The pane reads these to decide what to offer.
    #[test]
    fn the_capabilities_say_what_webdav_is() {
        let c = client().caps();
        assert!(!c.containers, "there is no bucket level");
        assert!(c.needs_directories, "collections must exist first");
        assert!(c.upload && c.delete);
        assert!(!c.versioning && !c.presign && !c.create_container);
    }

    /// The server's own words, with the code, beat "request failed".
    #[test]
    fn a_refusal_says_what_the_code_meant() {
        let e = status_error(reqwest::StatusCode::CONFLICT, "http://h/a/b");
        assert!(format!("{e}").contains("parent folder"), "{e}");
        let locked = status_error(reqwest::StatusCode::LOCKED, "http://h/a");
        assert!(format!("{locked}").contains("locked"), "{locked}");
        let auth = status_error(reqwest::StatusCode::UNAUTHORIZED, "http://h/a");
        assert!(format!("{auth}").contains("credentials"), "{auth}");
    }
}
