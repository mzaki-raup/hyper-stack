//! One interface over the stores the browser can show: S3, WebDAV and
//! Nexus Repository (owner's request, 2026-10-24).
//!
//! The three are not the same thing, and pretending otherwise is how a
//! browser ends up offering buttons that cannot work:
//!
//! - **S3** has buckets, versioning, multipart upload and presigned
//!   URLs.
//! - **WebDAV** has one tree with no buckets at all, real directories
//!   that must be created before a file can be written into them, and
//!   none of the rest.
//! - **Nexus** has repositories that look like buckets but are typed:
//!   whether you may upload at all depends on the repository's
//!   **format** (`raw` and `maven2` take a plain PUT; npm, NuGet and
//!   Docker have their own publish rules) and on whether it is hosted
//!   or a proxy of somewhere else.
//!
//! So the trait carries a [`Caps`] with it and the UI asks before it
//! offers. The alternative - one lowest common denominator - would
//! throw away versioning and presigning, which are most of why someone
//! opens an S3 bucket in the first place.

use std::path::Path;

use tokio_util::sync::CancellationToken;

use super::s3::{Bucket, Head, Listing, Progress, Result, Versioning};

/// Which store is behind this handle. The browser shows the name and
/// uses it for the few places the wording differs ("Bucket" against a
/// "Repository").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    S3,
    Webdav,
    Nexus,
}

impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Backend::S3 => "S3",
            Backend::Webdav => "WebDAV",
            Backend::Nexus => "Nexus",
        }
    }

    /// What this store calls the thing a key lives in. S3 has buckets,
    /// Nexus repositories; WebDAV has nothing of the sort, and the one
    /// pseudo-container the driver reports is its root.
    pub fn container_key(self) -> &'static str {
        match self {
            Backend::S3 => "s3-pick-bucket",
            Backend::Webdav => "s3-pick-share",
            Backend::Nexus => "s3-pick-repository",
        }
    }
}

/// What a store can actually do.
///
/// Every `false` here is a control the pane hides or disables with a
/// reason, rather than a button that fails when pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// Buckets/repositories can be listed. WebDAV has no such level, so
    /// it reports one pseudo-container and this is false.
    pub containers: bool,
    pub create_container: bool,
    pub delete_container: bool,
    /// Objects can be written at all. Nexus says no for a proxy
    /// repository and for formats whose publish rules are not a PUT.
    pub upload: bool,
    pub delete: bool,
    /// S3 only.
    pub versioning: bool,
    pub presign: bool,
    /// Whether a "folder" has to exist before a key under it can be
    /// written. True for WebDAV, where directories are real; false for
    /// S3 and Nexus, where a key with slashes in it is just a key.
    pub needs_directories: bool,
}

impl Caps {
    /// Everything off: the starting point a backend turns on from.
    pub const NONE: Caps = Caps {
        containers: false,
        create_container: false,
        delete_container: false,
        upload: false,
        delete: false,
        versioning: false,
        presign: false,
        needs_directories: false,
    };
}

/// A store the object browser can show.
///
/// Deliberately the **subset the browser uses**, not everything each
/// backend can do: S3's multipart upload lives inside `upload_file`
/// where it belongs, and the pane has never needed to see it.
#[async_trait::async_trait]
pub trait ObjectStore: Send + Sync {
    fn backend(&self) -> Backend;
    fn caps(&self) -> Caps;

    /// The buckets, shares or repositories on this connection.
    async fn containers(&self) -> Result<Vec<Bucket>>;

    /// One page of a listing. `delimited` groups keys into folders at
    /// `/`, exactly as `ListObjectsV2` does.
    async fn list(
        &self,
        container: &str,
        prefix: &str,
        delimited: bool,
        token: Option<&str>,
        max_keys: u32,
    ) -> Result<Listing>;

    async fn head(&self, container: &str, key: &str) -> Result<Head>;

    /// A small object into memory (the editor round trip).
    async fn get_bytes(&self, container: &str, key: &str) -> Result<Vec<u8>>;

    async fn download(
        &self,
        container: &str,
        key: &str,
        dest: &Path,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Result<u64>;

    async fn put_bytes(
        &self,
        container: &str,
        key: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<()>;

    async fn upload_file(
        &self,
        container: &str,
        key: &str,
        path: &Path,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Result<()>;

    async fn delete(&self, container: &str, key: &str) -> Result<()>;

    /// Returns the keys that could **not** be deleted, with why.
    async fn delete_many(&self, container: &str, keys: &[String]) -> Result<Vec<(String, String)>>;

    /// Every key under a prefix, for a recursive delete or download.
    async fn list_all_keys(&self, container: &str, prefix: &str) -> Result<Vec<String>>;

    /// S3 only; the default is the honest answer for the others.
    async fn versioning(&self, _container: &str) -> Result<Versioning> {
        Ok(Versioning::Never)
    }

    /// A URL that carries its own authorisation, for handing a file to
    /// something that is not this app.
    ///
    /// S3 only: a signature over the request is an S3 idea. WebDAV and
    /// Nexus authorise with the credentials on every request, so there
    /// is nothing to pre-sign and the empty string says so. The pane
    /// reads `caps().presign` and does not offer the control at all.
    fn presign(&self, _method: &str, _container: &str, _key: &str, _expires_secs: u64) -> String {
        String::new()
    }

    /// Make a folder. Only meaningful where `caps().needs_directories`.
    async fn create_directory(&self, _container: &str, _prefix: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every backend has to name itself and the thing it keeps keys in,
    /// because the pane says both out loud.
    #[test]
    fn each_backend_names_itself_and_its_containers() {
        let all = [Backend::S3, Backend::Webdav, Backend::Nexus];
        let mut labels: Vec<&str> = all.iter().map(|b| b.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), all.len(), "no two backends share a name");

        let mut keys: Vec<&str> = all.iter().map(|b| b.container_key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), all.len(), "each has its own word for a bucket");
        for b in all {
            assert!(
                b.container_key().starts_with("s3-pick-"),
                "{b:?} is a locale key"
            );
        }
    }

    /// `NONE` is the starting point every backend turns capabilities on
    /// from; if it ever gained a `true`, a store would claim something
    /// it had not said it could do.
    #[test]
    fn the_empty_capability_set_claims_nothing() {
        let c = Caps::NONE;
        for on in [
            c.containers,
            c.create_container,
            c.delete_container,
            c.upload,
            c.delete,
            c.versioning,
            c.presign,
            c.needs_directories,
        ] {
            assert!(!on, "NONE must claim nothing");
        }
    }
}
