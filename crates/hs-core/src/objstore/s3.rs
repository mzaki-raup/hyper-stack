//! The S3 client: buckets, listing with delimiter and continuation, head,
//! download to a file, uploads (single `PutObject` or multipart with four
//! parts in flight), deletes, presigned URLs and bucket versioning.
//!
//! Every request is signed with SigV4 and the body hash. 5xx and throttling
//! answers are retried with backoff; a "wrong region" answer switches the
//! bucket to the region the server names and retries once.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::Engine as _;
use futures_util::StreamExt;
use md5::{Digest as _, Md5};
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use super::sigv4::{self, Credentials, EMPTY_SHA256};
use super::xml::{self, Element};

/// Uploads up to this size go in one `PutObject`.
pub const SINGLE_PUT_LIMIT: u64 = 16 * 1024 * 1024;
/// Smallest multipart part (S3 requires ≥ 5 MiB except the last).
pub const MIN_PART: u64 = 8 * 1024 * 1024;
const PARTS_IN_FLIGHT: usize = 4;
const RETRIES: u32 = 3;

#[derive(Debug, Clone)]
pub struct S3Config {
    /// `https://s3.<region>.amazonaws.com` when empty.
    pub endpoint: String,
    pub region: String,
    pub path_style: bool,
    pub credentials: Credentials,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3Error {
    /// The server answered with an S3 error document (or a bare status).
    Api {
        status: u16,
        code: String,
        message: String,
    },
    Http(String),
    Io(String),
    Parse(String),
    Cancelled,
}

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            S3Error::Api {
                status,
                code,
                message,
            } => {
                if message.is_empty() {
                    write!(f, "{code} (HTTP {status})")
                } else {
                    write!(f, "{code}: {message} (HTTP {status})")
                }
            }
            S3Error::Http(e) | S3Error::Io(e) | S3Error::Parse(e) => f.write_str(e),
            S3Error::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for S3Error {}

pub type Result<T> = std::result::Result<T, S3Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bucket {
    pub name: String,
    pub created: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub key: String,
    pub size: u64,
    pub modified: String,
    pub etag: String,
    pub storage_class: String,
}

/// One page of `ListObjectsV2`: objects and "folders" (common prefixes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub objects: Vec<Object>,
    pub prefixes: Vec<String>,
    /// Pass back to get the next page; `None` when this was the last.
    pub next: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Head {
    pub size: u64,
    pub content_type: String,
    pub etag: String,
    pub modified: String,
    pub storage_class: String,
    pub version_id: Option<String>,
    /// `x-amz-meta-*` without the prefix.
    pub metadata: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Versioning {
    Never,
    Enabled,
    Suspended,
}

/// Called with (bytes done, bytes total) as a transfer moves.
pub type Progress = Arc<dyn Fn(u64, u64) + Send + Sync>;

pub struct S3Client {
    http: reqwest::Client,
    config: S3Config,
    /// Region per bucket, learnt from redirects.
    regions: Mutex<HashMap<String, String>>,
}

struct Call<'a> {
    method: reqwest::Method,
    bucket: Option<&'a str>,
    key: &'a str,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl<'a> Call<'a> {
    fn new(method: reqwest::Method, bucket: Option<&'a str>, key: &'a str) -> Self {
        Self {
            method,
            bucket,
            key,
            query: Vec::new(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
    fn q(mut self, k: &str, v: impl Into<String>) -> Self {
        self.query.push((k.to_string(), v.into()));
        self
    }
    fn h(mut self, k: &str, v: impl Into<String>) -> Self {
        self.headers.push((k.to_string(), v.into()));
        self
    }
    fn body(mut self, b: Vec<u8>) -> Self {
        self.body = b;
        self
    }
}

fn amz_now() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

fn http_err(e: reqwest::Error) -> S3Error {
    S3Error::Http(e.to_string())
}

fn io_err(e: std::io::Error) -> S3Error {
    S3Error::Io(e.to_string())
}

fn parse_xml(text: &str) -> Result<Element> {
    xml::parse(text).map_err(S3Error::Parse)
}

/// The error document of a failed response.
fn api_error(status: u16, body: &str) -> S3Error {
    let (code, message) = match xml::parse(body) {
        Ok(e) if e.name == "Error" => (e.text_of("Code"), e.text_of("Message")),
        _ => (
            reqwest::StatusCode::from_u16(status)
                .ok()
                .and_then(|s| s.canonical_reason())
                .unwrap_or("Error")
                .replace(' ', ""),
            String::new(),
        ),
    };
    S3Error::Api {
        status,
        code,
        message,
    }
}

fn retryable(status: u16) -> bool {
    status >= 500 || status == 429
}

impl S3Client {
    pub fn new(config: S3Config) -> Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .user_agent(concat!("HyperStack/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(http_err)?;
        Ok(Self {
            http,
            config,
            regions: Mutex::new(HashMap::new()),
        })
    }

    pub fn config(&self) -> &S3Config {
        &self.config
    }

    fn region_for(&self, bucket: Option<&str>) -> String {
        bucket
            .and_then(|b| self.regions.lock().get(b).cloned())
            .unwrap_or_else(|| self.config.region.clone())
    }

    fn endpoint(&self, region: &str) -> (String, String) {
        let ep = if self.config.endpoint.trim().is_empty() {
            format!("https://s3.{region}.amazonaws.com")
        } else {
            self.config
                .endpoint
                .trim()
                .trim_end_matches('/')
                .to_string()
        };
        match ep.split_once("://") {
            Some((scheme, host)) => (scheme.to_string(), host.to_string()),
            None => ("https".into(), ep),
        }
    }

    /// (url without query, host header, canonical path) for a bucket/key.
    pub fn locate(&self, bucket: Option<&str>, key: &str) -> (String, String, String) {
        let region = self.region_for(bucket);
        let (scheme, host) = self.endpoint(&region);
        let key_path = sigv4::uri_encode(key, false);
        match bucket {
            None => (format!("{scheme}://{host}/"), host, "/".into()),
            // Virtual-hosted needs a DNS-safe bucket name.
            Some(b) if !self.config.path_style && !b.contains('.') => {
                let host = format!("{b}.{host}");
                let path = format!("/{key_path}");
                (format!("{scheme}://{host}{path}"), host, path)
            }
            Some(b) => {
                let path = if key.is_empty() {
                    format!("/{}", sigv4::uri_encode(b, true))
                } else {
                    format!("/{}/{key_path}", sigv4::uri_encode(b, true))
                };
                (format!("{scheme}://{host}{path}"), host, path)
            }
        }
    }

    async fn send(&self, call: Call<'_>) -> Result<reqwest::Response> {
        let payload_hash = if call.body.is_empty() {
            EMPTY_SHA256.to_string()
        } else {
            sigv4::sha256_hex(&call.body)
        };
        let mut attempt = 0u32;
        let mut redirected = false;
        loop {
            let region = self.region_for(call.bucket);
            let (url, host, path) = self.locate(call.bucket, call.key);
            let date = amz_now();
            let mut headers = call.headers.clone();
            headers.push(("host".into(), host));
            headers.push(("x-amz-date".into(), date.clone()));
            headers.push(("x-amz-content-sha256".into(), payload_hash.clone()));
            if let Some(t) = &self.config.credentials.session_token {
                headers.push(("x-amz-security-token".into(), t.clone()));
            }
            let auth = sigv4::authorization(
                &self.config.credentials,
                &region,
                "s3",
                &date,
                &sigv4::Request {
                    method: call.method.as_str(),
                    path: &path,
                    query: &call.query,
                    headers: &headers,
                    payload_hash: &payload_hash,
                },
            );
            let query = sigv4::canonical_query(&call.query);
            let full = if query.is_empty() {
                url
            } else {
                format!("{url}?{query}")
            };
            let mut rb = self
                .http
                .request(call.method.clone(), &full)
                .header("authorization", auth);
            for (k, v) in &headers {
                if k != "host" {
                    rb = rb.header(k.as_str(), v.as_str());
                }
            }
            if !call.body.is_empty() || call.method == reqwest::Method::PUT {
                rb = rb.body(call.body.clone());
            }
            let resp = match rb.send().await {
                Ok(r) => r,
                Err(e) if attempt < RETRIES && (e.is_connect() || e.is_timeout()) => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(250 << attempt)).await;
                    continue;
                }
                Err(e) => return Err(http_err(e)),
            };
            let status = resp.status().as_u16();
            if resp.status().is_success() {
                return Ok(resp);
            }
            // Wrong region: the server names the right one.
            if !redirected
                && let Some(b) = call.bucket
                && let Some(r) = resp
                    .headers()
                    .get("x-amz-bucket-region")
                    .and_then(|v| v.to_str().ok())
                && r != region
            {
                self.regions.lock().insert(b.to_string(), r.to_string());
                redirected = true;
                continue;
            }
            if retryable(status) && attempt < RETRIES {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(250 << attempt)).await;
                continue;
            }
            let body = resp.text().await.unwrap_or_default();
            return Err(api_error(status, &body));
        }
    }

    async fn send_xml(&self, call: Call<'_>) -> Result<Element> {
        let text = self.send(call).await?.text().await.map_err(http_err)?;
        parse_xml(&text)
    }

    pub async fn list_buckets(&self) -> Result<Vec<Bucket>> {
        let root = self
            .send_xml(Call::new(reqwest::Method::GET, None, ""))
            .await?;
        Ok(root
            .child("Buckets")
            .map(|b| {
                b.all("Bucket")
                    .map(|e| Bucket {
                        name: e.text_of("Name"),
                        created: e.text_of("CreationDate"),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// One page under `prefix`, "folders" split at `/` when `delimited`.
    pub async fn list(
        &self,
        bucket: &str,
        prefix: &str,
        delimited: bool,
        token: Option<&str>,
        max_keys: u32,
    ) -> Result<Listing> {
        let mut call = Call::new(reqwest::Method::GET, Some(bucket), "")
            .q("list-type", "2")
            .q("max-keys", max_keys.to_string());
        if !prefix.is_empty() {
            call = call.q("prefix", prefix);
        }
        if delimited {
            call = call.q("delimiter", "/");
        }
        if let Some(t) = token {
            call = call.q("continuation-token", t);
        }
        let root = self.send_xml(call).await?;
        Ok(parse_listing(&root))
    }

    pub async fn head(&self, bucket: &str, key: &str) -> Result<Head> {
        let resp = self
            .send(Call::new(reqwest::Method::HEAD, Some(bucket), key))
            .await?;
        let h = resp.headers();
        let get = |n: &str| {
            h.get(n)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        let metadata = h
            .iter()
            .filter_map(|(k, v)| {
                k.as_str()
                    .strip_prefix("x-amz-meta-")
                    .map(|m| (m.to_string(), v.to_str().unwrap_or_default().to_string()))
            })
            .collect();
        Ok(Head {
            size: get("content-length").parse().unwrap_or(0),
            content_type: get("content-type"),
            etag: get("etag").trim_matches('"').to_string(),
            modified: get("last-modified"),
            storage_class: {
                let c = get("x-amz-storage-class");
                if c.is_empty() { "STANDARD".into() } else { c }
            },
            version_id: Some(get("x-amz-version-id")).filter(|v| !v.is_empty()),
            metadata,
        })
    }

    /// Small objects into memory (the editor round trip).
    pub async fn get_bytes(&self, bucket: &str, key: &str) -> Result<Vec<u8>> {
        let resp = self
            .send(Call::new(reqwest::Method::GET, Some(bucket), key))
            .await?;
        Ok(resp.bytes().await.map_err(http_err)?.to_vec())
    }

    /// Stream an object to `dest` (written to `dest.part`, then renamed).
    pub async fn download(
        &self,
        bucket: &str,
        key: &str,
        dest: &Path,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Result<u64> {
        let resp = self
            .send(Call::new(reqwest::Method::GET, Some(bucket), key))
            .await?;
        let total = resp.content_length().unwrap_or(0);
        let part = dest.with_extension(match dest.extension() {
            Some(e) => format!("{}.part", e.to_string_lossy()),
            None => "part".into(),
        });
        let mut file = tokio::fs::File::create(&part).await.map_err(io_err)?;
        let mut done = 0u64;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if cancel.is_cancelled() {
                drop(file);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(S3Error::Cancelled);
            }
            let chunk = chunk.map_err(http_err)?;
            file.write_all(&chunk).await.map_err(io_err)?;
            done += chunk.len() as u64;
            progress(done, total.max(done));
        }
        file.flush().await.map_err(io_err)?;
        drop(file);
        tokio::fs::rename(&part, dest).await.map_err(io_err)?;
        Ok(done)
    }

    pub async fn put_bytes(
        &self,
        bucket: &str,
        key: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<()> {
        let mut call = Call::new(reqwest::Method::PUT, Some(bucket), key).body(body);
        if !content_type.is_empty() {
            call = call.h("content-type", content_type);
        }
        self.send(call).await?;
        Ok(())
    }

    /// Upload a local file: one `PutObject` up to [`SINGLE_PUT_LIMIT`],
    /// multipart above it (aborted on error or cancel).
    pub async fn upload_file(
        &self,
        bucket: &str,
        key: &str,
        path: &Path,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Result<()> {
        let size = tokio::fs::metadata(path).await.map_err(io_err)?.len();
        let content_type = content_type_for(key);
        if size <= SINGLE_PUT_LIMIT {
            let body = tokio::fs::read(path).await.map_err(io_err)?;
            if cancel.is_cancelled() {
                return Err(S3Error::Cancelled);
            }
            self.put_bytes(bucket, key, body, content_type).await?;
            progress(size, size);
            return Ok(());
        }
        let part_size = part_size_for(size);
        let upload_id = self.create_multipart(bucket, key, content_type).await?;
        let result = self
            .upload_parts(
                bucket, key, &upload_id, path, size, part_size, progress, cancel,
            )
            .await;
        match result {
            Ok(parts) => {
                self.complete_multipart(bucket, key, &upload_id, &parts)
                    .await
            }
            Err(e) => {
                let _ = self.abort_multipart(bucket, key, &upload_id).await;
                Err(e)
            }
        }
    }

    pub async fn create_multipart(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
    ) -> Result<String> {
        let mut call = Call::new(reqwest::Method::POST, Some(bucket), key).q("uploads", "");
        if !content_type.is_empty() {
            call = call.h("content-type", content_type);
        }
        let root = self.send_xml(call).await?;
        let id = root.text_of("UploadId");
        if id.is_empty() {
            return Err(S3Error::Parse("no UploadId in the answer".into()));
        }
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)] // one call site; a struct would only rename them
    async fn upload_parts(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        path: &Path,
        size: u64,
        part_size: u64,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Result<Vec<(u32, String)>> {
        let count = size.div_ceil(part_size);
        let done = Arc::new(AtomicU64::new(0));
        let mut results: Vec<(u32, String)> = futures_util::stream::iter(0..count)
            .map(|i| {
                let done = done.clone();
                let progress = progress.clone();
                let cancel = cancel.clone();
                async move {
                    if cancel.is_cancelled() {
                        return Err(S3Error::Cancelled);
                    }
                    let offset = i * part_size;
                    let len = part_size.min(size - offset) as usize;
                    let mut file = tokio::fs::File::open(path).await.map_err(io_err)?;
                    file.seek(std::io::SeekFrom::Start(offset))
                        .await
                        .map_err(io_err)?;
                    let mut buf = vec![0u8; len];
                    file.read_exact(&mut buf).await.map_err(io_err)?;
                    let number = (i + 1) as u32;
                    let resp = self
                        .send(
                            Call::new(reqwest::Method::PUT, Some(bucket), key)
                                .q("partNumber", number.to_string())
                                .q("uploadId", upload_id)
                                .body(buf),
                        )
                        .await?;
                    let etag = resp
                        .headers()
                        .get("etag")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let now = done.fetch_add(len as u64, Ordering::Relaxed) + len as u64;
                    progress(now, size);
                    Ok((number, etag))
                }
            })
            .buffer_unordered(PARTS_IN_FLIGHT)
            .collect::<Vec<Result<(u32, String)>>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        results.sort_by_key(|(n, _)| *n);
        Ok(results)
    }

    pub async fn complete_multipart(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> Result<()> {
        let mut body = String::from("<CompleteMultipartUpload>");
        for (n, etag) in parts {
            body.push_str(&format!(
                "<Part><PartNumber>{n}</PartNumber><ETag>{}</ETag></Part>",
                xml::escape(etag)
            ));
        }
        body.push_str("</CompleteMultipartUpload>");
        let resp = self
            .send(
                Call::new(reqwest::Method::POST, Some(bucket), key)
                    .q("uploadId", upload_id)
                    .h("content-type", "application/xml")
                    .body(body.into_bytes()),
            )
            .await?;
        // S3 can answer 200 with an error document here.
        let text = resp.text().await.map_err(http_err)?;
        if let Ok(e) = xml::parse(&text)
            && e.name == "Error"
        {
            return Err(api_error(200, &text));
        }
        Ok(())
    }

    pub async fn abort_multipart(&self, bucket: &str, key: &str, upload_id: &str) -> Result<()> {
        self.send(Call::new(reqwest::Method::DELETE, Some(bucket), key).q("uploadId", upload_id))
            .await?;
        Ok(())
    }

    pub async fn delete(&self, bucket: &str, key: &str) -> Result<()> {
        self.send(Call::new(reqwest::Method::DELETE, Some(bucket), key))
            .await?;
        Ok(())
    }

    /// Delete up to 1,000 keys per request; returns the keys that failed
    /// with their error codes.
    pub async fn delete_many(
        &self,
        bucket: &str,
        keys: &[String],
    ) -> Result<Vec<(String, String)>> {
        let mut failed = Vec::new();
        for chunk in keys.chunks(1000) {
            let mut body = String::from("<Delete><Quiet>true</Quiet>");
            for k in chunk {
                body.push_str(&format!("<Object><Key>{}</Key></Object>", xml::escape(k)));
            }
            body.push_str("</Delete>");
            let md5 =
                base64::engine::general_purpose::STANDARD.encode(Md5::digest(body.as_bytes()));
            let root = self
                .send_xml(
                    Call::new(reqwest::Method::POST, Some(bucket), "")
                        .q("delete", "")
                        .h("content-md5", md5)
                        .h("content-type", "application/xml")
                        .body(body.into_bytes()),
                )
                .await?;
            for e in root.all("Error") {
                failed.push((e.text_of("Key"), e.text_of("Code")));
            }
        }
        Ok(failed)
    }

    /// Every key under `prefix` (no delimiter), page by page.
    pub async fn list_all_keys(&self, bucket: &str, prefix: &str) -> Result<Vec<String>> {
        let mut keys = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let page = self
                .list(bucket, prefix, false, token.as_deref(), 1000)
                .await?;
            keys.extend(page.objects.into_iter().map(|o| o.key));
            match page.next {
                Some(t) => token = Some(t),
                None => return Ok(keys),
            }
        }
    }

    pub async fn versioning(&self, bucket: &str) -> Result<Versioning> {
        let root = self
            .send_xml(Call::new(reqwest::Method::GET, Some(bucket), "").q("versioning", ""))
            .await?;
        Ok(match root.text_of("Status").as_str() {
            "Enabled" => Versioning::Enabled,
            "Suspended" => Versioning::Suspended,
            _ => Versioning::Never,
        })
    }

    pub async fn create_bucket(&self, bucket: &str) -> Result<()> {
        let region = self.region_for(Some(bucket));
        let body = if region == "us-east-1" || !self.config.endpoint.trim().is_empty() {
            Vec::new()
        } else {
            format!(
                "<CreateBucketConfiguration><LocationConstraint>{region}</LocationConstraint></CreateBucketConfiguration>"
            )
            .into_bytes()
        };
        self.send(Call::new(reqwest::Method::PUT, Some(bucket), "").body(body))
            .await?;
        Ok(())
    }

    /// Remove an empty bucket.
    pub async fn delete_bucket(&self, bucket: &str) -> Result<()> {
        self.send(Call::new(reqwest::Method::DELETE, Some(bucket), ""))
            .await?;
        Ok(())
    }

    /// A presigned URL valid for `expires_secs` (S3 allows up to 7 days).
    pub fn presign(&self, method: &str, bucket: &str, key: &str, expires_secs: u64) -> String {
        let region = self.region_for(Some(bucket));
        let (url, host, path) = self.locate(Some(bucket), key);
        let q = sigv4::presign_query(
            &self.config.credentials,
            &region,
            &amz_now(),
            &sigv4::Presign {
                method,
                host: &host,
                path: &path,
                query: &[],
                expires_secs,
            },
        );
        format!("{url}?{}", sigv4::canonical_query(&q))
    }
}

fn parse_listing(root: &Element) -> Listing {
    let objects = root
        .all("Contents")
        .map(|c| Object {
            key: c.text_of("Key"),
            size: c.text_of("Size").parse().unwrap_or(0),
            modified: c.text_of("LastModified"),
            etag: c.text_of("ETag").trim_matches('"').to_string(),
            storage_class: c.text_of("StorageClass"),
        })
        .collect();
    let prefixes = root
        .all("CommonPrefixes")
        .map(|p| p.text_of("Prefix"))
        .collect();
    let truncated = root.text_of("IsTruncated") == "true";
    let next = Some(root.text_of("NextContinuationToken")).filter(|t| truncated && !t.is_empty());
    Listing {
        objects,
        prefixes,
        next,
    }
}

/// Parts big enough to stay under S3's 10,000-part limit.
pub fn part_size_for(size: u64) -> u64 {
    MIN_PART.max(size.div_ceil(10_000))
}

/// A content type from the key's extension (the editor round trip and
/// uploads keep browsers happy when objects are served directly).
pub fn content_type_for(key: &str) -> &'static str {
    let ext = key.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "yaml" | "yml" => "application/yaml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

// --- the shared interface --------------------------------------------
//
// S3 is the backend the others are shaped after, so this is a thin
// delegation: the work it does beyond the trait - multipart upload,
// presigned URLs, versioning - lives in the methods above and stays
// reachable for callers that hold an `S3Client` directly.

#[async_trait::async_trait]
impl super::store::ObjectStore for S3Client {
    fn backend(&self) -> super::store::Backend {
        super::store::Backend::S3
    }

    fn caps(&self) -> super::store::Caps {
        super::store::Caps {
            containers: true,
            create_container: true,
            delete_container: true,
            upload: true,
            delete: true,
            versioning: true,
            presign: true,
            // A key with slashes in it is just a key.
            needs_directories: false,
        }
    }

    async fn containers(&self) -> Result<Vec<Bucket>> {
        self.list_buckets().await
    }

    async fn list(
        &self,
        container: &str,
        prefix: &str,
        delimited: bool,
        token: Option<&str>,
        max_keys: u32,
    ) -> Result<Listing> {
        S3Client::list(self, container, prefix, delimited, token, max_keys).await
    }

    async fn head(&self, container: &str, key: &str) -> Result<Head> {
        S3Client::head(self, container, key).await
    }

    async fn get_bytes(&self, container: &str, key: &str) -> Result<Vec<u8>> {
        S3Client::get_bytes(self, container, key).await
    }

    async fn download(
        &self,
        container: &str,
        key: &str,
        dest: &std::path::Path,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Result<u64> {
        S3Client::download(self, container, key, dest, progress, cancel).await
    }

    async fn put_bytes(
        &self,
        container: &str,
        key: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<()> {
        S3Client::put_bytes(self, container, key, body, content_type).await
    }

    async fn upload_file(
        &self,
        container: &str,
        key: &str,
        path: &std::path::Path,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Result<()> {
        S3Client::upload_file(self, container, key, path, progress, cancel).await
    }

    async fn delete(&self, container: &str, key: &str) -> Result<()> {
        S3Client::delete(self, container, key).await
    }

    async fn delete_many(&self, container: &str, keys: &[String]) -> Result<Vec<(String, String)>> {
        S3Client::delete_many(self, container, keys).await
    }

    async fn list_all_keys(&self, container: &str, prefix: &str) -> Result<Vec<String>> {
        S3Client::list_all_keys(self, container, prefix).await
    }

    async fn versioning(&self, container: &str) -> Result<Versioning> {
        S3Client::versioning(self, container).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(endpoint: &str, path_style: bool) -> S3Client {
        S3Client::new(S3Config {
            endpoint: endpoint.into(),
            region: "eu-west-1".into(),
            path_style,
            credentials: Credentials {
                access_key: "AK".into(),
                secret_key: "SK".into(),
                session_token: None,
            },
        })
        .unwrap()
    }

    #[test]
    fn urls_for_aws_and_custom_endpoints() {
        let c = client("", false);
        assert_eq!(
            c.locate(Some("shop"), "a b/c+d.txt").0,
            "https://shop.s3.eu-west-1.amazonaws.com/a%20b/c%2Bd.txt"
        );
        assert_eq!(c.locate(None, "").0, "https://s3.eu-west-1.amazonaws.com/");
        // Dotted buckets fall back to path style (TLS wildcard certificates).
        assert_eq!(
            c.locate(Some("my.site"), "x").0,
            "https://s3.eu-west-1.amazonaws.com/my.site/x"
        );
        let m = client("http://127.0.0.1:9000/", true);
        let (url, host, path) = m.locate(Some("dev"), "k");
        assert_eq!(url, "http://127.0.0.1:9000/dev/k");
        assert_eq!(host, "127.0.0.1:9000");
        assert_eq!(path, "/dev/k");
        assert_eq!(m.locate(Some("dev"), "").0, "http://127.0.0.1:9000/dev");
    }

    #[test]
    fn presigned_urls_carry_the_signature() {
        let c = client("http://127.0.0.1:9000", true);
        let url = c.presign("GET", "dev", "a.txt", 3600);
        assert!(url.starts_with("http://127.0.0.1:9000/dev/a.txt?"), "{url}");
        assert!(url.contains("X-Amz-Expires=3600"));
        assert!(url.contains("X-Amz-Signature="));
        assert!(!url.contains("SK"), "the secret never appears");
    }

    #[test]
    fn listing_with_folders_and_continuation() {
        let doc = r#"<ListBucketResult><IsTruncated>true</IsTruncated>
            <NextContinuationToken>tok/1+2</NextContinuationToken>
            <Contents><Key>logs/a.log</Key><Size>12</Size><LastModified>2026-10-11T10:00:00.000Z</LastModified><ETag>"abc"</ETag><StorageClass>STANDARD</StorageClass></Contents>
            <CommonPrefixes><Prefix>logs/2026/</Prefix></CommonPrefixes>
            </ListBucketResult>"#;
        let l = parse_listing(&xml::parse(doc).unwrap());
        assert_eq!(l.objects[0].key, "logs/a.log");
        assert_eq!(l.objects[0].etag, "abc");
        assert_eq!(l.prefixes, vec!["logs/2026/"]);
        assert_eq!(l.next.as_deref(), Some("tok/1+2"));
        let last = parse_listing(
            &xml::parse("<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>")
                .unwrap(),
        );
        assert_eq!(last.next, None);
    }

    #[test]
    fn errors_read_the_error_document() {
        let e = api_error(
            403,
            "<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>",
        );
        assert_eq!(e.to_string(), "AccessDenied: Access Denied (HTTP 403)");
        assert_eq!(api_error(404, "").to_string(), "NotFound (HTTP 404)");
    }

    #[test]
    fn part_sizes_respect_the_part_limit() {
        assert_eq!(part_size_for(20 * 1024 * 1024), MIN_PART);
        let huge = 5u64 * 1024 * 1024 * 1024 * 1024; // 5 TiB
        assert!(huge.div_ceil(part_size_for(huge)) <= 10_000);
    }
}
