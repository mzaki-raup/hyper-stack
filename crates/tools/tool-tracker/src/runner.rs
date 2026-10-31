//! The tab's tracker session, owned by one task on the tokio runtime. The
//! UI sends [`Cmd`]s through a bounded channel and reads [`Shared`] at
//! frame start; nothing here runs on the UI thread (ADR-013).

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::driver::{
    self, Caps, CreateForm, Detail, Edit, Found, NewItem, Page, PageRef, ServerInfo, Space,
    Transition,
};
use crate::profile::ConnectConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Connecting,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// Run the query from its first page.
    Search(String),
    /// The next page of the query on screen.
    More,
    /// The poll: the query on screen again, from its first page.
    Refresh,
    Item(String),
    Transitions(String),
    Transition(String, Transition),
    Edit(String, Edit),
    Comment(String, String),
    Fields(String),
    Create(NewItem),
    Spaces,
    Pages(Space),
    Page(String),
    Publish(Page, String),
}

/// A result for one item or page, so a late answer for the one the user
/// has since left is not shown under the one they are on now.
#[derive(Debug, Clone, PartialEq)]
pub struct For<T> {
    pub key: String,
    pub result: Result<T, String>,
}

#[derive(Debug, Default)]
pub struct Shared {
    pub status: Option<Status>,
    pub info: ServerInfo,
    pub caps: Caps,
    pub default_query: String,
    /// The query the items are for.
    pub query: String,
    pub found: Option<Found>,
    pub found_error: Option<String>,
    /// How many open items are assigned to the user, where known.
    pub mine: Option<u64>,
    pub detail: Option<For<Detail>>,
    pub transitions: Option<For<Vec<Transition>>>,
    pub form: Option<Result<CreateForm, String>>,
    /// The key of an item just created, for the tab to select.
    pub created: Option<String>,
    pub spaces: Option<Result<Vec<Space>, String>>,
    /// Keyed by the space's key.
    pub pages: Option<For<Vec<PageRef>>>,
    pub page: Option<For<Page>>,
    /// What is running now, as a Fluent key, for the spinner.
    pub busy: Option<&'static str>,
    /// (success, text) for the tab to toast.
    pub toasts: Vec<(bool, String)>,
}

pub fn spawn(
    runtime: &hs_core::runtime::AppRuntime,
    config: ConnectConfig,
    shared: Arc<Mutex<Shared>>,
) -> mpsc::Sender<Cmd> {
    let (tx, mut rx) = mpsc::channel::<Cmd>(16);
    let waker = runtime.waker();
    shared.lock().status = Some(Status::Connecting);
    runtime.spawn(async move {
        let mut d = match driver::connect(&config).await {
            Ok(d) => d,
            Err(e) => {
                shared.lock().status = Some(Status::Failed(e));
                waker.request_repaint();
                return;
            }
        };
        {
            let mut sh = shared.lock();
            sh.status = Some(Status::Ready);
            sh.info = d.info().clone();
            sh.caps = d.caps();
            sh.default_query = d.default_query();
        }
        waker.request_repaint();
        // Polls pile up behind a slow server; one is enough.
        let mut pending = None;
        while let Some(cmd) =
            hs_core::runtime::next_coalesced(&mut rx, &mut pending, &Cmd::Refresh).await
        {
            shared.lock().busy = Some(busy_key(&cmd));
            waker.request_repaint();
            serve(cmd, d.as_mut(), &shared, &config).await;
            shared.lock().busy = None;
            waker.request_repaint();
        }
    });
    tx
}

fn busy_key(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Search(_) | Cmd::More | Cmd::Refresh => "tracker-busy-search",
        Cmd::Item(_) | Cmd::Page(_) => "tracker-busy-item",
        Cmd::Transitions(_) | Cmd::Fields(_) => "tracker-busy-form",
        Cmd::Spaces | Cmd::Pages(_) => "tracker-busy-pages",
        Cmd::Transition(..)
        | Cmd::Edit(..)
        | Cmd::Comment(..)
        | Cmd::Create(_)
        | Cmd::Publish(..) => "tracker-busy-write",
    }
}

async fn search(d: &mut dyn driver::Driver, shared: &Arc<Mutex<Shared>>, query: String) {
    let r = d.search(&query, None).await;
    // What is assigned to the user, for the dashboard; one more read,
    // and only where the tracker knows who the user is.
    let mine = match d.mine_query() {
        Some(m) => d
            .search(&m, None)
            .await
            .ok()
            .map(|f| f.total.unwrap_or(f.items.len() as u64)),
        None => None,
    };
    let mut sh = shared.lock();
    sh.query = query;
    sh.mine = mine;
    match r {
        Ok(f) => {
            sh.found = Some(f);
            sh.found_error = None;
        }
        Err(e) => {
            sh.found = None;
            sh.found_error = Some(e);
        }
    }
}

async fn reread(d: &mut dyn driver::Driver, shared: &Arc<Mutex<Shared>>, key: String) {
    let r = d.item(&key).await;
    shared.lock().detail = Some(For { key, result: r });
}

async fn serve(
    cmd: Cmd,
    d: &mut dyn driver::Driver,
    shared: &Arc<Mutex<Shared>>,
    cfg: &ConnectConfig,
) {
    let toast = |ok: bool, text: String| shared.lock().toasts.push((ok, text));
    let query = || shared.lock().query.clone();
    match cmd {
        Cmd::Search(q) => search(d, shared, q).await,
        Cmd::Refresh => {
            let q = query();
            search(d, shared, q).await;
        }
        Cmd::More => {
            let (q, from) = {
                let sh = shared.lock();
                (
                    sh.query.clone(),
                    sh.found.as_ref().and_then(|f| f.next.clone()),
                )
            };
            let Some(from) = from else { return };
            match d.search(&q, Some(&from)).await {
                Ok(more) => {
                    let mut sh = shared.lock();
                    // The query changed while this page was read.
                    if sh.query != q {
                        return;
                    }
                    if let Some(f) = sh.found.as_mut() {
                        f.items.extend(more.items);
                        f.next = more.next;
                    }
                }
                Err(e) => toast(false, e),
            }
        }
        Cmd::Item(key) => reread(d, shared, key).await,
        Cmd::Transitions(key) => {
            let r = d.transitions(&key).await;
            shared.lock().transitions = Some(For { key, result: r });
        }
        Cmd::Transition(key, to) => match d.transition(&key, &to).await {
            Ok(said) => {
                tracing::info!("tracker: {key} to {} on {}", to.name, cfg.kind);
                toast(true, said);
                reread(d, shared, key).await;
                search(d, shared, query()).await;
            }
            Err(e) => toast(false, e),
        },
        Cmd::Edit(key, edit) => match d.edit(&key, &edit).await {
            Ok(said) => {
                tracing::info!("tracker: edited {key} on {}", cfg.kind);
                toast(true, said);
                reread(d, shared, key).await;
                search(d, shared, query()).await;
            }
            Err(e) => toast(false, e),
        },
        Cmd::Comment(key, body) => match d.comment(&key, &body).await {
            Ok(said) => {
                tracing::info!("tracker: commented on {key} on {}", cfg.kind);
                toast(true, said);
                if d.caps().pages {
                    let r = d.page(&key).await;
                    shared.lock().page = Some(For { key, result: r });
                } else {
                    reread(d, shared, key).await;
                }
            }
            Err(e) => toast(false, e),
        },
        Cmd::Fields(project) => {
            let r = d.fields(&project).await;
            shared.lock().form = Some(r);
        }
        Cmd::Create(item) => match d.create(&item).await {
            Ok(key) => {
                tracing::info!("tracker: created {key} on {}", cfg.kind);
                toast(true, format!("{key} created"));
                shared.lock().created = Some(key.clone());
                search(d, shared, query()).await;
                reread(d, shared, key).await;
            }
            Err(e) => toast(false, e),
        },
        Cmd::Spaces => {
            let r = d.spaces().await;
            shared.lock().spaces = Some(r);
        }
        Cmd::Pages(space) => {
            let r = d.pages(&space).await;
            shared.lock().pages = Some(For {
                key: space.key,
                result: r,
            });
        }
        Cmd::Page(id) => {
            let r = d.page(&id).await;
            shared.lock().page = Some(For { key: id, result: r });
        }
        Cmd::Publish(page, body) => match d.publish(&page, &body).await {
            Ok(said) => {
                tracing::info!("tracker: published page {} on {}", page.id, cfg.kind);
                toast(true, said);
                let r = d.page(&page.id).await;
                shared.lock().page = Some(For {
                    key: page.id,
                    result: r,
                });
            }
            Err(e) => toast(false, e),
        },
    }
}
