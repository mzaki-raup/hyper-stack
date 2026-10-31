//! One trait every tracker kind implements (docs/tools/tracker.md §8), and
//! the types the tab sees. What a kind cannot do it says through [`Caps`],
//! and the tab leaves that control out.

use async_trait::async_trait;

use crate::profile::ConnectConfig;

pub type Result<T> = std::result::Result<T, String>;

/// Listing bounds (docs/tools/tracker.md §7).
pub const PAGE: usize = 100;
pub const MAX_COMMENTS: usize = 200;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {
    /// "Jira", "Redmine", "GitHub Issues", …
    pub product: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    pub transition: bool,
    pub edit: bool,
    pub comment: bool,
    pub create: bool,
    /// Confluence: spaces and pages instead of items.
    pub pages: bool,
}

/// One row of the items table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    /// What the other calls take, and what people call it: "OPS-12", "#42".
    pub key: String,
    pub summary: String,
    pub status: String,
    pub assignee: String,
    pub priority: String,
    /// Unix milliseconds.
    pub updated_ms: Option<i64>,
    /// `YYYY-MM-DD`, where the kind has due dates.
    pub due: String,
    /// The item's page, for Open in Browser.
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comment {
    pub author: String,
    pub created_ms: Option<i64>,
    /// Markdown.
    pub body: String,
}

/// The selected item, read in full.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Detail {
    pub item: Item,
    /// Name, value: type, reporter, created…
    pub fields: Vec<(String, String)>,
    /// Markdown.
    pub description: String,
    /// What converting the description dropped, if anything: an edit
    /// would lose it, so the tab says so before sending one.
    pub lossy: Option<String>,
    pub labels: Vec<String>,
    pub due: String,
    pub comments: Vec<Comment>,
    /// Why the comments could not be read, when they could not.
    pub comments_note: Option<String>,
    /// Name, URL.
    pub attachments: Vec<(String, String)>,
    /// How it is linked, the other item's key.
    pub links: Vec<(String, String)>,
}

/// A status the item can move to: a Jira transition, a Redmine status,
/// open / closed on the forges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transition {
    pub id: String,
    pub name: String,
}

/// What Edit sends: only the fields that changed are `Some`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edit {
    pub summary: Option<String>,
    pub description: Option<String>,
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub labels: Option<Vec<String>>,
    pub due: Option<String>,
}

impl Edit {
    pub fn is_empty(&self) -> bool {
        *self == Edit::default()
    }
}

/// What New Item asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateForm {
    /// Jira issue types, Redmine trackers; empty where there is no choice.
    pub kinds: Vec<String>,
    pub priorities: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewItem {
    /// Jira project key, Redmine project identifier; the forges use the
    /// connection's repository.
    pub project: String,
    pub kind: String,
    pub summary: String,
    pub description: String,
    pub priority: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub items: Vec<Item>,
    /// Where More starts, when there is more.
    pub next: Option<String>,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Space {
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageRef {
    pub id: String,
    pub title: String,
    /// The parent page's id; empty at the top of the space.
    pub parent: String,
}

/// A Confluence page, read in full.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub version: u64,
    /// Markdown.
    pub body: String,
    /// Why the page cannot be edited here (macros, layouts), if it cannot.
    pub read_only: Option<String>,
    pub url: String,
    pub comments: Vec<Comment>,
}

#[async_trait]
pub trait Driver: Send {
    fn info(&self) -> &ServerInfo;
    fn caps(&self) -> Caps;
    /// What the query bar starts with.
    fn default_query(&self) -> String;
    /// The query for what is assigned to the signed-in user and still
    /// open, for the dashboard; `None` where the tracker cannot say who
    /// that is (no credential, or pages rather than items).
    fn mine_query(&self) -> Option<String> {
        None
    }
    async fn search(&mut self, query: &str, from: Option<&str>) -> Result<Found>;
    async fn item(&mut self, key: &str) -> Result<Detail>;
    async fn transitions(&mut self, key: &str) -> Result<Vec<Transition>>;
    /// Returns what to say in the toast.
    async fn transition(&mut self, key: &str, to: &Transition) -> Result<String>;
    async fn edit(&mut self, key: &str, edit: &Edit) -> Result<String>;
    async fn comment(&mut self, key: &str, body: &str) -> Result<String>;
    async fn fields(&mut self, project: &str) -> Result<CreateForm>;
    /// Returns the new item's key.
    async fn create(&mut self, item: &NewItem) -> Result<String>;
    async fn spaces(&mut self) -> Result<Vec<Space>> {
        Err("this tracker has no spaces".into())
    }
    async fn pages(&mut self, _space: &Space) -> Result<Vec<PageRef>> {
        Err("this tracker has no pages".into())
    }
    async fn page(&mut self, _id: &str) -> Result<Page> {
        Err("this tracker has no pages".into())
    }
    async fn publish(&mut self, _page: &Page, _body: &str) -> Result<String> {
        Err("this tracker has no pages".into())
    }
}

/// Open a session to the tracker `cfg` describes, within its timeout.
pub async fn connect(cfg: &ConnectConfig) -> Result<Box<dyn Driver>> {
    let open = async {
        let d: Box<dyn Driver> = match cfg.kind.as_str() {
            "jira" => Box::new(crate::jira::Jira::connect(cfg).await?),
            "redmine" => Box::new(crate::redmine::Redmine::connect(cfg).await?),
            "github-issues" => Box::new(crate::github::GitHub::connect(cfg).await?),
            "gitlab-issues" => Box::new(crate::gitlab::GitLab::connect(cfg).await?),
            "forgejo-issues" => Box::new(crate::forgejo::Forgejo::connect(cfg).await?),
            "confluence" => Box::new(crate::confluence::Confluence::connect(cfg).await?),
            other => return Err(format!("“{other}” is not a tracker kind")),
        };
        Ok(d)
    };
    let inner = tokio::time::timeout(cfg.timeout, open)
        .await
        .map_err(|_| {
            format!(
                "the server did not answer within {} s",
                cfg.timeout.as_secs()
            )
        })??;
    Ok(Box::new(Guarded {
        inner,
        read_only: cfg.read_only,
    }))
}

/// Refuses every write on a read-only session, whatever asked for it: the
/// tab disables the controls, and this is what stops a palette command
/// getting round them (docs/tools/tracker.md §6).
pub struct Guarded {
    inner: Box<dyn Driver>,
    read_only: bool,
}

impl Guarded {
    fn allow(&self, what: &str, can: bool) -> Result<()> {
        if self.read_only {
            Err(format!(
                "{what} refused: this connection is read-only (a protected environment, or the profile says so)"
            ))
        } else if !can {
            Err(format!("{what} is not offered by this tracker"))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl Driver for Guarded {
    fn info(&self) -> &ServerInfo {
        self.inner.info()
    }
    fn caps(&self) -> Caps {
        self.inner.caps()
    }
    fn default_query(&self) -> String {
        self.inner.default_query()
    }
    fn mine_query(&self) -> Option<String> {
        self.inner.mine_query()
    }
    async fn search(&mut self, query: &str, from: Option<&str>) -> Result<Found> {
        self.inner.search(query, from).await
    }
    async fn item(&mut self, key: &str) -> Result<Detail> {
        self.inner.item(key).await
    }
    async fn transitions(&mut self, key: &str) -> Result<Vec<Transition>> {
        self.inner.transitions(key).await
    }
    async fn transition(&mut self, key: &str, to: &Transition) -> Result<String> {
        self.allow("Change Status", self.inner.caps().transition)?;
        self.inner.transition(key, to).await
    }
    async fn edit(&mut self, key: &str, edit: &Edit) -> Result<String> {
        self.allow("Edit", self.inner.caps().edit)?;
        self.inner.edit(key, edit).await
    }
    async fn comment(&mut self, key: &str, body: &str) -> Result<String> {
        self.allow("Comment", self.inner.caps().comment)?;
        self.inner.comment(key, body).await
    }
    async fn fields(&mut self, project: &str) -> Result<CreateForm> {
        self.inner.fields(project).await
    }
    async fn create(&mut self, item: &NewItem) -> Result<String> {
        self.allow("New Item", self.inner.caps().create)?;
        self.inner.create(item).await
    }
    async fn spaces(&mut self) -> Result<Vec<Space>> {
        self.inner.spaces().await
    }
    async fn pages(&mut self, space: &Space) -> Result<Vec<PageRef>> {
        self.inner.pages(space).await
    }
    async fn page(&mut self, id: &str) -> Result<Page> {
        self.inner.page(id).await
    }
    async fn publish(&mut self, page: &Page, body: &str) -> Result<String> {
        self.allow("Publish", self.inner.caps().pages)?;
        if let Some(why) = &page.read_only {
            return Err(format!("Publish refused: {why}"));
        }
        self.inner.publish(page, body).await
    }
}

/// A Unix time in milliseconds as the tables show it.
pub fn time_ms(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// RFC 3339 / ISO 8601 to Unix milliseconds. Jira writes the offset
/// without a colon (`+0000`), which RFC 3339 does not allow.
pub fn iso_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .ok()
        .map(|t| t.timestamp_millis())
}

/// A query parameter, percent-encoded.
pub fn q(s: &str) -> String {
    hs_core::service_http::seg(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake {
        info: ServerInfo,
        writes: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Driver for Fake {
        fn info(&self) -> &ServerInfo {
            &self.info
        }
        fn caps(&self) -> Caps {
            Caps {
                transition: true,
                edit: true,
                comment: true,
                create: false,
                pages: false,
            }
        }
        fn default_query(&self) -> String {
            String::new()
        }
        async fn search(&mut self, _: &str, _: Option<&str>) -> Result<Found> {
            Ok(Found::default())
        }
        async fn item(&mut self, _: &str) -> Result<Detail> {
            Ok(Detail::default())
        }
        async fn transitions(&mut self, _: &str) -> Result<Vec<Transition>> {
            Ok(Vec::new())
        }
        async fn transition(&mut self, _: &str, _: &Transition) -> Result<String> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(String::new())
        }
        async fn edit(&mut self, _: &str, _: &Edit) -> Result<String> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(String::new())
        }
        async fn comment(&mut self, _: &str, _: &str) -> Result<String> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(String::new())
        }
        async fn fields(&mut self, _: &str) -> Result<CreateForm> {
            Ok(CreateForm::default())
        }
        async fn create(&mut self, _: &NewItem) -> Result<String> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(String::new())
        }
    }

    fn guarded(read_only: bool, writes: &Arc<AtomicUsize>) -> Guarded {
        Guarded {
            inner: Box::new(Fake {
                info: ServerInfo::default(),
                writes: writes.clone(),
            }),
            read_only,
        }
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
    }

    #[test]
    fn a_read_only_session_refuses_every_write_before_the_server_sees_it() {
        let writes = Arc::new(AtomicUsize::new(0));
        let mut g = guarded(true, &writes);
        rt().block_on(async {
            let t = Transition::default();
            assert!(
                g.transition("A-1", &t)
                    .await
                    .unwrap_err()
                    .contains("read-only")
            );
            assert!(g.edit("A-1", &Edit::default()).await.is_err());
            assert!(g.comment("A-1", "hi").await.is_err());
            assert!(g.create(&NewItem::default()).await.is_err());
            assert!(g.search("", None).await.is_ok());
        });
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_write_the_kind_does_not_offer_is_refused_and_one_it_does_goes_through() {
        let writes = Arc::new(AtomicUsize::new(0));
        let mut g = guarded(false, &writes);
        rt().block_on(async {
            assert!(
                g.create(&NewItem::default())
                    .await
                    .unwrap_err()
                    .contains("not offered")
            );
            assert!(g.comment("A-1", "hi").await.is_ok());
        });
        assert_eq!(writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn jira_times_without_a_colon_in_the_offset_are_read() {
        assert_eq!(
            iso_ms("2026-10-31T10:00:00.000+0000"),
            Some(1_793_440_800_000)
        );
        assert_eq!(iso_ms("2026-10-31T10:00:00Z"), Some(1_793_440_800_000));
    }
}
