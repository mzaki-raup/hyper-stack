//! HyperStack API client (docs/tools/api-client.md): one tab is one request;
//! the sidebar holds the workspace's collections, folders and requests plus
//! the history. Collections are plain JSON files in `<workspace>/api/`,
//! Postman v2.1 collections and environments import and export, scripts
//! run in JavaScript with the `pm` API (`script`), and `{{variables}}`
//! resolve through collection variables, the active HyperStack
//! environment (secrets from the vault at send time) and globals.

pub mod assist;
pub mod importers;
pub mod model;
pub mod postman;
pub mod script;
pub mod send;
mod ui;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hs_core::Services;
use hs_core::events::Event;
use hs_core::tool::{
    Accept, Action, CommandDescriptor, OpenRequest, Printable, QuickOpenItem, Speakable,
    ToastLevel, Tool, ToolCategory, ToolContext, ToolDescriptor, ToolFactory,
};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use model::{Auth, AuthKind, Body, BodyKind, Collection, Globals, HistoryEntry, Item, Kv, Request};
use script::{Change, Scope, Scopes, TestResult};
use send::{BodyData, Exchange, Prepared};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "api",
    title_key: "tool-api",
    icon: ph::PAPER_PLANE_TILT,
    category: ToolCategory::Network,
    accepts: &[Accept::Text],
};

pub struct ApiFactory;

impl ToolFactory for ApiFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Box::new(ApiTool::new(services, state))
    }
}

/// The workspace logo as an egui image URI, or `None` when none is set.
fn workspace_icon(ctx: &egui::Context, s: &Services) -> Option<String> {
    let root = s.workspace.read().root().to_path_buf();
    let file = hs_core::workspace::workspace_icon_file(&root)?;
    let bytes = std::fs::read(&file).ok()?;
    let stamp = std::fs::metadata(&file)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let uri = format!("bytes://api-home-icon/{stamp}.png");
    ctx.include_bytes(uri.clone(), bytes);
    Some(uri)
}

/// The topic every API tab listens on for "the collections changed".
pub(crate) const COLLECTIONS_CHANGED: &str = "api.collections";

/// Persisted per tab: which saved request is open, or the unsaved draft.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    collection: Option<String>,
    request_id: Option<String>,
    draft: Option<Request>,
    req_tab: usize,
    resp_tab: usize,
}

/// The outcome of a send: the exchange or the error, plus what the scripts
/// said, delivered from the background task.
pub struct Outcome {
    pub exchange: Result<Exchange, String>,
    pub prepared: Option<Prepared>,
    pub logs: Vec<String>,
    pub tests: Vec<TestResult>,
    pub script_error: Option<String>,
    pub changes: Vec<Change>,
    pub scopes: Scopes,
}

type Pending = Arc<Mutex<Option<Outcome>>>;

pub struct ApiTool {
    /// This tab is the activity's home, told by the shell each frame.
    /// `title` and the tab's icon are asked for outside `ui`, where there
    /// is nothing to ask. Owner's requirement, 2026-10-17.
    is_home: bool,
    /// The workspace's name and logo, for the home tab.
    home_name: String,
    home_icon: Option<String>,
    pub(crate) state: State,
    /// The request being edited (a copy of the saved one, or a draft).
    pub(crate) request: Request,
    /// The saved version, to show the dirty dot.
    saved: Option<Request>,
    pub(crate) collections: Vec<Collection>,
    pub(crate) globals: Globals,
    pub(crate) history: Vec<HistoryEntry>,
    api_dir: PathBuf,
    pub(crate) filter: String,
    pub(crate) pending: Option<(Pending, tokio::task::AbortHandle)>,
    pub(crate) last: Option<Outcome>,
    pub(crate) body_view: ui::BodyEditor,
    pub(crate) resp_view: ui::ResponseView,
    pub(crate) rename: Option<(String, String)>,
    pub(crate) save_to: Option<ui::SaveTo>,
    pub(crate) confirm_send: Option<String>,
    pub(crate) curl_import: Option<String>,
    pub(crate) new_collection: Option<String>,
    pub(crate) show_history: bool,
    pub(crate) sent_title: String,
    /// Text to copy on the next frame (commands have no `egui::Context`).
    pub(crate) copy_pending: Option<String>,
    /// A toast raised by a UI helper that had no `ToolContext`.
    pub(crate) copy_pending_toast: Option<(ToastLevel, String)>,
    pub(crate) assistant: hs_ui::ai_assistant::AssistantPanel,
    /// What the assistant's context was last built from, so the (long)
    /// context and schema strings are rebuilt only when something changed.
    assistant_key: Option<AssistantKey>,
}

/// Cheap fingerprint of everything the assistant context is built from.
#[derive(PartialEq, Eq, Clone)]
struct AssistantKey {
    request: Request,
    body_version: u64,
    collections: Vec<(String, usize)>,
    last: Option<(u16, u64)>,
    location: String,
}

impl ApiTool {
    /// A tool bound to the workspace's `api/` folder, for tests and hosts
    /// that drive it without a tab.
    pub fn new_in(s: &Services) -> Self {
        Self::new(s, State::default())
    }

    /// The request being edited.
    pub fn request(&self) -> &Request {
        &self.request
    }

    /// The workspace's collections.
    pub fn collections(&self) -> &[Collection] {
        &self.collections
    }

    /// Apply an assistant action block (`hs-api` JSON) as the Apply button
    /// does.
    pub fn apply_assistant_block(
        &mut self,
        block: &str,
        cx: &mut ToolContext,
    ) -> Result<assist::Report, String> {
        let actions = assist::parse(block)?;
        Ok(self.apply_actions(&actions, cx))
    }

    pub(crate) fn globals_path(&self) -> PathBuf {
        self.api_dir.join("globals.json")
    }

    fn new(s: &Services, state: State) -> Self {
        let api_dir = s.workspace.read().root().join("api");
        let collections = Collection::load_all(&api_dir.join("collections"));
        let globals = Globals::load(&api_dir.join("globals.json"));
        let history = model::load_history(&api_dir.join("history.jsonl"), 200);
        let mut t = Self {
            is_home: false,
            home_name: String::new(),
            home_icon: None,
            request: Request::default(),
            saved: None,
            collections,
            globals,
            history,
            api_dir,
            filter: String::new(),
            pending: None,
            last: None,
            body_view: ui::BodyEditor::new(s),
            resp_view: ui::ResponseView::new(s),
            rename: None,
            save_to: None,
            confirm_send: None,
            curl_import: None,
            new_collection: None,
            show_history: false,
            sent_title: String::new(),
            copy_pending: None,
            copy_pending_toast: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
            assistant_key: None,
            state,
        };
        // Reopen the saved request, else the draft.
        if let (Some(cid), Some(rid)) = (t.state.collection.clone(), t.state.request_id.clone()) {
            if let Some(r) = t
                .collections
                .iter()
                .find(|c| c.id == cid)
                .and_then(|c| c.find(&rid).map(|(r, _)| r.clone()))
            {
                t.saved = Some(r.clone());
                t.request = r;
            } else {
                t.state.collection = None;
                t.state.request_id = None;
            }
        } else if let Some(d) = t.state.draft.clone() {
            t.request = d;
        }
        t.body_view
            .set_text(&t.request.body.text, t.request.body.kind);
        t
    }

    pub(crate) fn collections_dir(&self) -> PathBuf {
        self.api_dir.join("collections")
    }

    pub(crate) fn reload(&mut self, s: &Services) {
        self.api_dir = s.workspace.read().root().join("api");
        self.collections = Collection::load_all(&self.collections_dir());
        self.globals = Globals::load(&self.api_dir.join("globals.json"));
        self.history = model::load_history(&self.api_dir.join("history.jsonl"), 200);
    }

    pub(crate) fn is_dirty(&self) -> bool {
        match &self.saved {
            Some(saved) => *saved != self.request,
            None => !self.request.url.is_empty() || !self.request.body.text.is_empty(),
        }
    }

    /// Open a saved request in this tab.
    pub(crate) fn open_request(&mut self, collection: &str, id: &str) {
        if let Some(r) = self
            .collections
            .iter()
            .find(|c| c.id == collection)
            .and_then(|c| c.find(id).map(|(r, _)| r.clone()))
        {
            self.state.collection = Some(collection.to_string());
            self.state.request_id = Some(id.to_string());
            self.state.draft = None;
            self.saved = Some(r.clone());
            self.body_view.set_text_pretty(&r.body.text, r.body.kind);
            self.request = r;
            self.last = None;
        }
    }

    /// A fresh unsaved request in this tab.
    /// The names already used in one collection folder, so a new or
    /// renamed request can be numbered against them.
    pub(crate) fn sibling_names(&self, collection: &str, folder: Option<&str>) -> Vec<String> {
        self.collections
            .iter()
            .find(|c| c.id == collection)
            .and_then(|c| c.items(folder))
            .map(|items| items.iter().map(|i| i.name().to_string()).collect())
            .unwrap_or_default()
    }

    /// The names beside a request, not counting the request itself, so
    /// renaming it to what it is already called is not a clash.
    pub(crate) fn sibling_names_excluding(&self, collection: &str, id: &str) -> Vec<String> {
        let folder = self
            .collections
            .iter()
            .find(|c| c.id == collection)
            .and_then(|c| c.find(id))
            .and_then(|(_, folders)| folders.last().map(|f| f.id.clone()));
        self.collections
            .iter()
            .find(|c| c.id == collection)
            .and_then(|c| c.items(folder.as_deref()))
            .map(|items| {
                items
                    .iter()
                    .filter(|i| i.id() != id)
                    .map(|i| i.name().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Where a request made from the sidebar's New Request button should
    /// go: beside the one that is open, else the first collection. `None`
    /// when there are no collections yet and one has to be made first.
    pub(crate) fn home_for_new_request(&self) -> Option<(String, Option<String>)> {
        if let Some(c) = self.state.collection.clone()
            && self.collections.iter().any(|x| x.id == c)
        {
            let folder = self.state.request_id.as_ref().and_then(|id| {
                self.collections
                    .iter()
                    .find(|x| x.id == c)
                    .and_then(|x| x.find(id))
                    .and_then(|(_, folders)| folders.last().map(|f| f.id.clone()))
            });
            return Some((c, folder));
        }
        self.collections.first().map(|c| (c.id.clone(), None))
    }

    pub(crate) fn new_request(&mut self) {
        self.state.collection = None;
        self.state.request_id = None;
        self.saved = None;
        self.request = Request::default();
        self.body_view.set_text("", BodyKind::None);
        self.last = None;
    }

    /// Write every collection's place back, after a drag has moved one.
    pub(crate) fn save_order(&mut self, cx: &mut ToolContext) {
        let dir = self.collections_dir();
        for (i, c) in self.collections.iter_mut().enumerate() {
            c.order = i as i32;
            if let Err(e) = c.save(&dir) {
                cx.toast(hs_core::tool::ToastLevel::Error, e.to_string());
                return;
            }
        }
        self.announce(cx);
    }

    pub(crate) fn save_collection(&self, id: &str, cx: &mut ToolContext) {
        if let Some(c) = self.collections.iter().find(|c| c.id == id)
            && let Err(e) = c.save(&self.collections_dir())
        {
            cx.toast(ToastLevel::Error, e.to_string());
            return;
        }
        self.announce(cx);
    }

    /// Say that the collections on disk changed, so every other API tab
    /// reloads its own copy. Each tab holds the collections it read when it
    /// opened, so a request written from one tab was invisible in another
    /// until the workspace was switched. Owner's report, 2026-10-17.
    pub(crate) fn announce(&self, cx: &mut ToolContext) {
        cx.services.events.publish(hs_core::events::Event::Custom {
            topic: COLLECTIONS_CHANGED.into(),
            // The tab that wrote them already has them; the payload lets it
            // skip the reload rather than re-reading what it just saved.
            payload: cx.tab_id.to_string(),
        });
    }

    /// Save the open request into its collection (or ask where to put it).
    pub(crate) fn save(&mut self, cx: &mut ToolContext) {
        self.request.body.text = self.body_view.text();
        match (self.state.collection.clone(), self.state.request_id.clone()) {
            (Some(cid), Some(rid)) => {
                let req = self.request.clone();
                if let Some(c) = self.collections.iter_mut().find(|c| c.id == cid)
                    && let Some(slot) = c.find_mut(&rid)
                {
                    *slot = req.clone();
                }
                self.save_collection(&cid, cx);
                self.saved = Some(req);
                cx.toast(ToastLevel::Success, cx.services.t("api-saved"));
            }
            _ => {
                if self.collections.is_empty() {
                    self.new_collection = Some(String::new());
                } else {
                    self.save_to = Some(ui::SaveTo {
                        collection: self.collections[0].id.clone(),
                        folder: None,
                        name: self.request.name.clone(),
                    });
                }
            }
        }
    }

    /// Put the open request into `collection`/`folder` as a new item.
    pub(crate) fn save_as(
        &mut self,
        collection: &str,
        folder: Option<&str>,
        name: &str,
        cx: &mut ToolContext,
    ) {
        self.request.body.text = self.body_view.text();
        let mut req = self.request.clone();
        req.id = hs_core::new_id();
        req.name = name.to_string();
        let id = req.id.clone();
        if let Some(c) = self.collections.iter_mut().find(|c| c.id == collection)
            && let Some(items) = c.items_mut(folder)
        {
            items.push(Item::Request(req.clone()));
        }
        self.save_collection(collection, cx);
        self.state.collection = Some(collection.to_string());
        self.state.request_id = Some(id);
        self.state.draft = None;
        self.saved = Some(req.clone());
        self.request = req;
        cx.toast(ToastLevel::Success, cx.services.t("api-saved"));
    }

    pub(crate) fn add_collection(&mut self, name: &str, cx: &mut ToolContext) -> String {
        let c = Collection {
            name: name.trim().to_string(),
            ..Default::default()
        };
        let id = c.id.clone();
        self.collections.push(c);
        self.save_collection(&id, cx);
        id
    }

    pub(crate) fn import_postman_file(&mut self, path: &std::path::Path, cx: &mut ToolContext) {
        let s = cx.services;
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                cx.toast(ToastLevel::Error, e.to_string());
                return;
            }
        };
        // A collection in any format the client reads, or a Postman
        // environment.
        match importers::import_file(path) {
            Ok(c) => {
                let n = c.requests().len();
                let id = c.id.clone();
                self.collections.push(c);
                self.save_collection(&id, cx);
                cx.toast(
                    ToastLevel::Success,
                    s.i18n
                        .t_args("api-imported-collection", &[("count", &n.to_string())]),
                );
            }
            Err(collection_err) => match postman::import_environment_document(&text) {
                Ok((name, vars)) => {
                    let mut env = hs_core::environments::Environment {
                        name: if name.is_empty() {
                            "Imported".into()
                        } else {
                            name
                        },
                        ..Default::default()
                    };
                    for v in vars.iter().filter(|v| v.enabled) {
                        env.variables.insert(v.key.clone(), v.value.clone());
                    }
                    let n = env.variables.len();
                    let name = env.name.clone();
                    match s.environments.write().upsert(env) {
                        Ok(()) => {
                            s.events.publish(Event::EnvironmentsChanged);
                            cx.toast(
                                ToastLevel::Success,
                                s.i18n.t_args(
                                    "api-imported-environment",
                                    &[("name", &name), ("count", &n.to_string())],
                                ),
                            );
                        }
                        Err(e) => cx.toast(ToastLevel::Error, e.to_string()),
                    }
                }
                Err(_) => cx.toast(
                    ToastLevel::Error,
                    if collection_err == "unknown" {
                        s.t("api-import-unknown")
                    } else {
                        collection_err
                    },
                ),
            },
        }
    }

    pub(crate) fn export_postman(&self, collection: &str, cx: &mut ToolContext) {
        let Some(c) = self.collections.iter().find(|c| c.id == collection) else {
            return;
        };
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!(
                "{}.postman_collection.json",
                hs_core::storage::slug(&c.name)
            ))
            .add_filter("Postman collection", &["json"])
            .save_file()
        {
            match std::fs::write(&path, postman::export_collection(c)) {
                Ok(()) => cx.toast(
                    ToastLevel::Success,
                    cx.services
                        .i18n
                        .t_args("api-exported", &[("path", &path.display().to_string())]),
                ),
                Err(e) => cx.toast(ToastLevel::Error, e.to_string()),
            }
        }
    }

    /// Auth after inheritance: request → folders (innermost first) → collection.
    fn effective_auth(&self) -> Auth {
        if self.request.auth.kind != AuthKind::Inherit {
            return self.request.auth.clone();
        }
        if let (Some(cid), Some(rid)) = (&self.state.collection, &self.state.request_id)
            && let Some(c) = self.collections.iter().find(|c| &c.id == cid)
        {
            if let Some((_, folders)) = c.find(rid) {
                for f in folders.iter().rev() {
                    if f.auth.kind != AuthKind::Inherit {
                        return f.auth.clone();
                    }
                }
            }
            if c.auth.kind != AuthKind::Inherit {
                return c.auth.clone();
            }
        }
        Auth {
            kind: AuthKind::None,
            ..Default::default()
        }
    }

    /// Scripts and libraries above the request, outermost first.
    fn inherited_scripts(&self) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut pre = Vec::new();
        let mut post = Vec::new();
        let mut libs = Vec::new();
        if let (Some(cid), Some(rid)) = (&self.state.collection, &self.state.request_id)
            && let Some(c) = self.collections.iter().find(|c| &c.id == cid)
        {
            libs.extend(
                c.libraries
                    .iter()
                    .filter(|l| l.enabled)
                    .map(|l| l.code.clone()),
            );
            pre.push(c.pre_script.clone());
            post.push(c.post_script.clone());
            if let Some((_, folders)) = c.find(rid) {
                for f in folders {
                    pre.push(f.pre_script.clone());
                    post.push(f.post_script.clone());
                }
            }
        }
        (pre, post, libs)
    }

    fn scopes(&self, s: &Services) -> Scopes {
        let mut scopes = Scopes::default();
        for v in self.globals.values.iter().filter(|v| v.enabled) {
            scopes.globals.insert(v.key.clone(), v.value.clone());
        }
        if let Some(cid) = &self.state.collection
            && let Some(c) = self.collections.iter().find(|c| &c.id == cid)
        {
            for v in c.variables.iter().filter(|v| v.enabled) {
                scopes.collection.insert(v.key.clone(), v.value.clone());
            }
        }
        for (k, v) in s.environments.read().available_vars(None) {
            scopes.environment.insert(k, v);
        }
        scopes
    }

    /// `{{var}}` through the script scopes, then the HyperStack built-ins,
    /// then `secret:` references through the vault.
    fn resolve(
        &self,
        s: &Services,
        scopes: &Scopes,
        text: &str,
        missing: &mut Vec<String>,
    ) -> String {
        let extra: HashMap<String, String> = scopes
            .globals
            .iter()
            .chain(scopes.collection.iter())
            .chain(scopes.environment.iter())
            .chain(scopes.local.iter())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let out = match s.environments.read().substitute(text, None, &[&extra]) {
            Ok(t) => t,
            Err(names) => {
                missing.extend(names);
                text.to_string()
            }
        };
        resolve_secrets(s, &out)
    }

    /// Everything resolved and ready to send, after the pre-request scripts.
    fn prepare(
        &self,
        s: &Services,
        scopes: &mut Scopes,
        logs: &mut Vec<String>,
        script_error: &mut Option<String>,
    ) -> Result<(Prepared, Vec<Change>), String> {
        let (pre, _, libs) = self.inherited_scripts();
        let mut req = self.request.clone();
        req.body.text = self.body_view.text();
        let auth = self.effective_auth();
        // Pre-request scripts see and may change the unresolved request.
        let mut changes = Vec::new();
        let code: String = pre
            .into_iter()
            .chain(std::iter::once(req.pre_script.clone()))
            .filter(|c| !c.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n;\n");
        if !code.trim().is_empty() {
            let headers: Vec<(String, String)> = req
                .headers
                .iter()
                .filter(|h| h.enabled)
                .map(|h| (h.key.clone(), h.value.clone()))
                .collect();
            let params: Vec<(String, String)> = req
                .params
                .iter()
                .filter(|p| p.enabled)
                .map(|p| (p.key.clone(), p.value.clone()))
                .collect();
            let form: Vec<(String, String)> = req
                .body
                .form
                .iter()
                .filter(|p| p.enabled)
                .map(|p| (p.key.clone(), p.value.clone()))
                .collect();
            let input = script::Input {
                request: script::request_value(
                    &req.name,
                    &req.method,
                    &req.url,
                    &headers,
                    &params,
                    &format!("{:?}", req.body.kind).to_lowercase(),
                    &req.body.text,
                    &form,
                ),
                response: None,
                scopes: scopes.clone(),
                libraries: libs,
            };
            let out = script::run(&code, input);
            logs.extend(out.logs);
            *scopes = out.scopes;
            changes = out.changes;
            if let Some(e) = out.error {
                *script_error = Some(e);
            }
            if let Some(v) = out.request {
                apply_script_request(&mut req, &v);
            }
        }
        let mut missing = Vec::new();
        let mut url = self.resolve(s, scopes, req.url.trim(), &mut missing);
        let query: Vec<String> = req
            .params
            .iter()
            .filter(|p| p.enabled && !p.key.is_empty())
            .map(|p| {
                format!(
                    "{}={}",
                    send::urlencode(&self.resolve(s, scopes, &p.key, &mut missing)),
                    send::urlencode(&self.resolve(s, scopes, &p.value, &mut missing))
                )
            })
            .collect();
        if !query.is_empty() {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&query.join("&"));
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            url = format!("http://{url}");
        }
        let mut headers: Vec<(String, String)> = req
            .headers
            .iter()
            .filter(|h| h.enabled && !h.key.is_empty())
            .map(|h| {
                (
                    self.resolve(s, scopes, &h.key, &mut missing),
                    self.resolve(s, scopes, &h.value, &mut missing),
                )
            })
            .collect();
        match auth.kind {
            AuthKind::Basic => {
                use base64::Engine;
                let raw = format!(
                    "{}:{}",
                    self.resolve(s, scopes, &auth.username, &mut missing),
                    self.resolve(s, scopes, &auth.password, &mut missing)
                );
                headers.push((
                    "Authorization".into(),
                    format!(
                        "Basic {}",
                        base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
                    ),
                ));
            }
            AuthKind::Bearer => headers.push((
                "Authorization".into(),
                format!(
                    "Bearer {}",
                    self.resolve(s, scopes, &auth.token, &mut missing)
                ),
            )),
            AuthKind::ApiKey => {
                let k = self.resolve(s, scopes, &auth.key, &mut missing);
                let v = self.resolve(s, scopes, &auth.value, &mut missing);
                if auth.add_to == "query" {
                    url.push(if url.contains('?') { '&' } else { '?' });
                    url.push_str(&format!("{}={}", send::urlencode(&k), send::urlencode(&v)));
                } else {
                    headers.push((k, v));
                }
            }
            _ => {}
        }
        let body = match req.body.kind {
            BodyKind::None => BodyData::None,
            BodyKind::Json | BodyKind::Text | BodyKind::Xml => BodyData::Text {
                content_type: req.body.kind.content_type().unwrap_or("text/plain").into(),
                text: self.resolve(s, scopes, &req.body.text, &mut missing),
            },
            BodyKind::Form => BodyData::Form(
                req.body
                    .form
                    .iter()
                    .filter(|f| f.enabled)
                    .map(|f| {
                        (
                            self.resolve(s, scopes, &f.key, &mut missing),
                            self.resolve(s, scopes, &f.value, &mut missing),
                        )
                    })
                    .collect(),
            ),
            BodyKind::Multipart => {
                let mut parts = Vec::new();
                for f in req.body.form.iter().filter(|f| f.enabled) {
                    if f.description.starts_with("file:") {
                        let path = self.resolve(s, scopes, &f.value, &mut missing);
                        let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
                        let name = std::path::Path::new(&path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "file".into());
                        let ct = hs_core::sniff::sniff(&bytes).mime.to_string();
                        parts.push((f.key.clone(), Some(name), ct, bytes));
                    } else {
                        parts.push((
                            f.key.clone(),
                            None,
                            "text/plain".into(),
                            self.resolve(s, scopes, &f.value, &mut missing).into_bytes(),
                        ));
                    }
                }
                BodyData::Multipart(parts)
            }
            BodyKind::Binary => {
                let path = self.resolve(s, scopes, &req.body.file, &mut missing);
                let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
                BodyData::Binary {
                    content_type: hs_core::sniff::sniff(&bytes).mime.to_string(),
                    bytes,
                }
            }
        };
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            return Err(s
                .i18n
                .t_args("api-missing-vars", &[("names", &missing.join(", "))]));
        }
        Ok((
            Prepared {
                method: req.method.clone(),
                url,
                headers,
                body,
                timeout_secs: 60,
            },
            changes,
        ))
    }

    /// Send the open request (after the protected-environment check).
    pub(crate) fn send(&mut self, cx: &mut ToolContext) {
        let s = cx.services;
        if self.pending.is_some() {
            return;
        }
        let mut scopes = self.scopes(s);
        let mut logs = Vec::new();
        let mut script_error = None;
        let prepared = match self.prepare(s, &mut scopes, &mut logs, &mut script_error) {
            Ok(p) => p,
            Err(e) => {
                self.last = Some(Outcome {
                    exchange: Err(e),
                    prepared: None,
                    logs,
                    tests: Vec::new(),
                    script_error,
                    changes: Vec::new(),
                    scopes,
                });
                return;
            }
        };
        let (prepared, pre_changes) = prepared;
        self.apply_changes(s, &pre_changes, cx);
        self.sent_title = format!("{} {}", prepared.method, prepared.url);
        let (_, post, libs) = self.inherited_scripts();
        let post_code: String = post
            .into_iter()
            .chain(std::iter::once(self.request.post_script.clone()))
            .filter(|c| !c.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n;\n");
        let req_name = self.request.name.clone();
        let pending: Pending = Arc::new(Mutex::new(None));
        let out = pending.clone();
        let waker = s.runtime.waker();
        let client = hs_core::ai::http_client();
        let p2 = prepared.clone();
        let task = s.runtime.spawn(async move {
            let exchange = send::send(&client, &p2).await;
            let mut outcome = Outcome {
                exchange,
                prepared: Some(p2.clone()),
                logs,
                tests: Vec::new(),
                script_error,
                changes: Vec::new(),
                scopes: scopes.clone(),
            };
            if let Ok(ex) = &outcome.exchange
                && !post_code.trim().is_empty()
            {
                let body = String::from_utf8_lossy(&ex.body).into_owned();
                let headers: Vec<(String, String)> = p2.headers.clone();
                let input = script::Input {
                    request: script::request_value(
                        &req_name,
                        &p2.method,
                        &p2.url,
                        &headers,
                        &[],
                        "",
                        "",
                        &[],
                    ),
                    response: Some(script::response_value(
                        ex.status,
                        &ex.status_text,
                        &ex.headers,
                        &body,
                        ex.millis,
                    )),
                    scopes,
                    libraries: libs,
                };
                let r = tokio::task::spawn_blocking(move || script::run(&post_code, input))
                    .await
                    .unwrap_or_default();
                outcome.logs.extend(r.logs);
                outcome.tests = r.tests;
                outcome.changes = r.changes;
                outcome.scopes = r.scopes;
                if r.error.is_some() {
                    outcome.script_error = r.error;
                }
            }
            *out.lock() = Some(outcome);
            waker.request_repaint();
        });
        self.pending = Some((pending, task.abort_handle()));
        self.last = None;
    }

    /// Variable changes from scripts, written where they belong: globals to
    /// `globals.json`, collection variables to the collection, environment
    /// variables to the active HyperStack environment (or globals when
    /// none is active).
    fn apply_changes(&mut self, s: &Services, changes: &[Change], cx: &mut ToolContext) {
        if changes.is_empty() {
            return;
        }
        let mut globals_dirty = false;
        let mut collection_dirty = false;
        let mut env_dirty = false;
        for ch in changes {
            match ch.scope {
                Scope::Local => {}
                Scope::Globals => {
                    match &ch.value {
                        Some(v) => self.globals.set(&ch.key, v),
                        None => self.globals.unset(&ch.key),
                    }
                    globals_dirty = true;
                }
                Scope::Collection => {
                    if let Some(cid) = &self.state.collection
                        && let Some(c) = self.collections.iter_mut().find(|c| &c.id == cid)
                    {
                        match &ch.value {
                            Some(v) => match c.variables.iter_mut().find(|k| k.key == ch.key) {
                                Some(k) => k.value = v.clone(),
                                None => c.variables.push(Kv::new(&ch.key, v)),
                            },
                            None => c.variables.retain(|k| k.key != ch.key),
                        }
                        collection_dirty = true;
                    } else {
                        if let Some(v) = &ch.value {
                            self.globals.set(&ch.key, v);
                        }
                        globals_dirty = true;
                    }
                }
                Scope::Environment => {
                    let active = s.environments.read().active.clone();
                    match active {
                        Some(_) => {
                            let r = match &ch.value {
                                Some(v) => s.environments.write().set_var(None, &ch.key, v),
                                None => s.environments.write().set_var(None, &ch.key, ""),
                            };
                            if let Err(e) = r {
                                cx.toast(ToastLevel::Error, e.to_string());
                            }
                            env_dirty = true;
                        }
                        None => {
                            if let Some(v) = &ch.value {
                                self.globals.set(&ch.key, v);
                            }
                            globals_dirty = true;
                        }
                    }
                }
            }
        }
        if globals_dirty && let Err(e) = self.globals.save(&self.api_dir.join("globals.json")) {
            cx.toast(ToastLevel::Error, e.to_string());
        }
        if collection_dirty && let Some(cid) = self.state.collection.clone() {
            self.save_collection(&cid, cx);
        }
        if env_dirty {
            s.events.publish(Event::EnvironmentsChanged);
        }
    }

    fn poll(&mut self, cx: &mut ToolContext) {
        let done = self.pending.as_ref().and_then(|(p, _)| p.lock().take());
        if let Some(outcome) = done {
            self.pending = None;
            let changes = outcome.changes.clone();
            self.apply_changes(cx.services, &changes, cx);
            if let Ok(ex) = &outcome.exchange {
                let entry = HistoryEntry {
                    at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    method: self.request.method.clone(),
                    url: outcome
                        .prepared
                        .as_ref()
                        .map(|p| p.url.clone())
                        .unwrap_or_default(),
                    status: ex.status,
                    millis: ex.millis,
                    request: {
                        let mut r = self.request.clone();
                        r.body.text = self.body_view.text();
                        r
                    },
                };
                model::append_history(&self.api_dir.join("history.jsonl"), &entry, 200);
                self.history.push(entry);
                if self.history.len() > 200 {
                    self.history.remove(0);
                }
                self.resp_view.set(ex, cx.services);
            }
            self.last = Some(outcome);
        }
    }

    pub(crate) fn cancel(&mut self) {
        if let Some((_, abort)) = self.pending.take() {
            abort.abort();
        }
    }
}

/// A `pm.request` object back into the request: method, url, headers,
/// params, body text (a script may add or remove any of them).
fn apply_script_request(req: &mut Request, v: &serde_json::Value) {
    if let Some(m) = v.get("method").and_then(|m| m.as_str()) {
        req.method = m.to_uppercase();
    }
    if let Some(u) = v.get("url").and_then(|u| u.as_str()) {
        req.url = u.to_string();
    }
    if let Some(h) = v.get("headers").and_then(|h| h.as_object()) {
        req.headers = h
            .iter()
            .map(|(k, val)| Kv::new(k, val.as_str().unwrap_or(&val.to_string())))
            .collect();
    }
    if let Some(p) = v.get("params").and_then(|p| p.as_object()) {
        req.params = p
            .iter()
            .map(|(k, val)| Kv::new(k, val.as_str().unwrap_or(&val.to_string())))
            .collect();
    }
    if let Some(t) = v
        .get("body")
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())
    {
        req.body.text = t.to_string();
    }
}

/// Replace every `secret:<id>` with its vault value (when unlocked).
fn resolve_secrets(s: &Services, text: &str) -> String {
    if !text.contains("secret:") || s.secrets.is_locked() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("secret:") {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let end = after[7..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .map(|e| e + 7)
            .unwrap_or(after.len());
        let token = &after[..end];
        match hs_core::secrets::SecretRef::parse(token).and_then(|r| s.secrets.get(&r).ok()) {
            Some(v) => out.push_str(secrecy::ExposeSecret::expose_secret(&v)),
            None => out.push_str(token),
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

impl Tool for ApiTool {
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    /// Known before the first frame, so the home tab reads as the
    /// workspace even while it has never been opened. The logo needs an
    /// egui context and is loaded in `ui`.
    fn set_home_tab(&mut self, home: bool, services: &Services) {
        self.is_home = home;
        if home {
            self.home_name = services.workspace.read().meta.name.clone();
        }
    }

    fn title(&self) -> Cow<'_, str> {
        // The home tab is the workspace's, not a request's.
        if self.is_home {
            return if self.home_name.is_empty() {
                Cow::Borrowed("API Client")
            } else {
                Cow::Borrowed(&self.home_name)
            };
        }
        if self.request.name.is_empty() {
            Cow::Borrowed("Request")
        } else {
            Cow::Borrowed(&self.request.name)
        }
    }

    /// The method, in the colour it is shown in everywhere else, so a
    /// strip of request tabs can be read by colour. A method with no
    /// colour of its own is left to the tab's own text colour.
    fn tab_title_prefix(&self) -> Option<(String, egui::Color32)> {
        // No method on the home tab: it holds no request.
        if self.is_home {
            return None;
        }
        let colour = ui::method_rgb(&self.request.method)?;
        Some((self.request.method.clone(), colour))
    }

    /// The workspace's logo on the home tab, so it reads as the project's
    /// API client rather than as another request.
    fn tab_icon_uri(&self) -> Option<String> {
        self.is_home.then(|| self.home_icon.clone()).flatten()
    }

    fn tab_icon(&self) -> Option<&'static str> {
        self.is_home.then_some(hs_ui::icons::WORKSPACE)
    }

    fn dirty(&self) -> bool {
        self.is_dirty()
    }

    fn environment(&self) -> Option<String> {
        None
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        self.poll(cx);
        let theme = Theme::from_settings(&cx.services.settings.read().appearance);
        // `title` and the tab's icon are asked for outside `ui`, where
        // nothing says which tab this is; remember it while we are told.
        self.is_home = cx.is_home;
        if cx.is_home {
            self.home_name = cx.services.workspace.read().meta.name.clone();
            self.home_icon = workspace_icon(ui.ctx(), cx.services);
            ui::home(self, ui, cx, &theme);
            return;
        }
        let full = ui.available_rect_before_wrap();
        let main_rect = if self.assistant.open {
            egui::Rect::from_min_max(full.min, egui::pos2(full.right() - 390.0, full.bottom()))
        } else {
            full
        };
        let mut main_ui = ui.new_child(egui::UiBuilder::new().max_rect(main_rect));
        main_ui.set_clip_rect(main_rect);
        ui::main(self, &mut main_ui, cx, &theme);
        if self.assistant.open {
            let s = cx.services;
            let location = match (&self.state.collection, &self.state.request_id) {
                (Some(cid), Some(_)) => self
                    .collections
                    .iter()
                    .find(|c| &c.id == cid)
                    .map(|c| format!("in collection \"{}\"", c.name))
                    .unwrap_or_else(|| "unsaved".into()),
                _ => "unsaved".into(),
            };
            let key = AssistantKey {
                request: self.request.clone(),
                body_version: self.body_view.version(),
                collections: self
                    .collections
                    .iter()
                    .map(|c| (c.name.clone(), c.request_count()))
                    .collect(),
                last: self.last.as_ref().and_then(|o| match &o.exchange {
                    Ok(ex) => Some((ex.status, ex.millis)),
                    Err(_) => None,
                }),
                location,
            };
            if self.assistant_key.as_ref() != Some(&key) {
                let mut context = format!(
                    "Request: {} {}\nHeaders: {}\nBody ({:?}):\n{}",
                    self.request.method,
                    self.request.url,
                    self.request
                        .headers
                        .iter()
                        .filter(|h| h.enabled)
                        .map(|h| format!("{}: {}", h.key, h.value))
                        .collect::<Vec<_>>()
                        .join(", "),
                    self.request.body.kind,
                    self.body_view.text()
                );
                if let Some(Outcome {
                    exchange: Ok(ex),
                    tests,
                    script_error,
                    ..
                }) = &self.last
                {
                    let body = String::from_utf8_lossy(&ex.body);
                    let shown: String = body.chars().take(4000).collect();
                    context.push_str(&format!(
                        "\n\nLast response: {} {} in {} ms\n{}\nTests: {} passed / {}\nScript error: {}",
                        ex.status,
                        ex.status_text,
                        ex.millis,
                        shown,
                        tests.iter().filter(|t| t.passed).count(),
                        tests.len(),
                        script_error.clone().unwrap_or_else(|| "none".into())
                    ));
                }
                self.assistant
                    .set_context("API request and response", context);
                self.assistant.extra_system =
                    assist::system_prompt(&self.collections, &self.request, &key.location);
                self.assistant.action_lang = Some(assist::LANG.to_string());
                self.assistant_key = Some(key);
            }
            if self.assistant.quick_actions.is_empty() {
                use hs_ui::ai_assistant::QuickAction;
                self.assistant.quick_actions = vec![
                    QuickAction::new(s, "ai-qa-pre-script", "ai-qa-pre-script-prompt", ""),
                    QuickAction::new(s, "ai-qa-post-script", "ai-qa-post-script-prompt", ""),
                    QuickAction::new(s, "ai-qa-save-vars", "ai-qa-save-vars-prompt", ""),
                    QuickAction::new(s, "ai-qa-tests", "ai-qa-tests-prompt", ""),
                    QuickAction::new(
                        s,
                        "ai-qa-explain-response",
                        "ai-qa-explain-response-prompt",
                        "",
                    ),
                    QuickAction::new(s, "ai-qa-docs", "ai-qa-docs-prompt", ""),
                ];
            }
            let panel = egui::Rect::from_min_max(
                egui::pos2(full.right() - 390.0, full.top()),
                full.right_bottom(),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(panel)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(panel.intersect(ui.clip_rect()));
            egui::Frame::new()
                .fill(theme.panel_bg)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(&mut child, |ui| {
                    ui.set_min_width(370.0);
                    self.assistant.can_insert = true;
                    let mut out = self.assistant.ui(ui, s, &theme, "API client");
                    let is_action = out.use_block_lang.eq_ignore_ascii_case(assist::LANG);
                    if let Some(block) = out.use_block.take_if(|_| is_action) {
                        // An action block: edit the request and collections
                        // as the user would by hand (never sends).
                        match assist::parse(&block) {
                            Ok(actions) => {
                                let rep = self.apply_actions(&actions, cx);
                                let (level, text) = assist::summary(cx, &rep);
                                cx.toast(level, text);
                                self.assistant_key = None;
                            }
                            Err(e) => cx.toast(ToastLevel::Error, e),
                        }
                    } else if let Some(block) = out.use_block {
                        // Into what the user is editing: a script tab takes
                        // the block as script, anything else as the body.
                        match self.state.req_tab {
                            4 => {
                                let target = &mut self.request.post_script;
                                if !target.is_empty() && !target.ends_with('\n') {
                                    target.push('\n');
                                }
                                target.push_str(&block);
                            }
                            _ => {
                                let kind = if block.trim_start().starts_with('{')
                                    || block.trim_start().starts_with('[')
                                {
                                    BodyKind::Json
                                } else {
                                    BodyKind::Text
                                };
                                if self.request.body.kind == BodyKind::None {
                                    self.request.body.kind = kind;
                                }
                                self.body_view
                                    .set_text_pretty(&block, self.request.body.kind);
                                self.state.req_tab = 2;
                            }
                        }
                    }
                });
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // The assistant is the same control in every tool, so it sits in
            // the same place: hard right of the toolbar.
            if ui
                .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
                .on_hover_text(s.t("ai-shared-button-tip"))
                .clicked()
            {
                self.assistant.toggle("API client");
            }
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let theme = Theme::from_settings(&cx.services.settings.read().appearance);
        ui::sidebar(self, ui, cx, &theme);
        true
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        let c = |id: &str, title: &str, sc: Option<&str>| CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: sc.map(str::to_string),
        };
        vec![
            c("api.send", "API: Send Request", Some("run.alt")),
            c("file.save", "API: Save Request", Some("file.save")),
            c("api.new", "API: New Request", None),
            c(
                "api.import",
                "API: Import Postman Collection or Environment…",
                None,
            ),
            c("api.curl", "API: Import curl Command…", None),
            c("api.copy_curl", "API: Copy as curl", None),
            c("api.ai", "AI: Toggle Assistant", None),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "api.send" => {
                self.request_send(cx);
                true
            }
            "file.save" => {
                self.save(cx);
                true
            }
            "api.new" => {
                self.new_request();
                true
            }
            "api.import" => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Postman JSON", &["json"])
                    .pick_file()
                {
                    self.import_postman_file(&p, cx);
                }
                true
            }
            "api.curl" => {
                self.curl_import = Some(String::new());
                true
            }
            "api.ai" => {
                self.assistant.toggle("API client");
                true
            }
            "api.copy_curl" => {
                let mut scopes = self.scopes(cx.services);
                let mut logs = Vec::new();
                let mut err = None;
                match self.prepare(cx.services, &mut scopes, &mut logs, &mut err) {
                    Ok((p, _)) => {
                        cx.actions.push(Action::Toast {
                            level: ToastLevel::Success,
                            text: cx.services.t("api-curl-copied"),
                        });
                        self.copy_pending = Some(send::as_curl(&p));
                    }
                    Err(e) => cx.toast(ToastLevel::Error, e),
                }
                true
            }
            _ => false,
        }
    }

    fn quick_open_items(&self) -> Vec<QuickOpenItem> {
        self.collections
            .iter()
            .flat_map(|c| {
                c.requests()
                    .into_iter()
                    .map(move |(path, r)| QuickOpenItem {
                        id: format!("api://{}/{}", c.id, r.id),
                        title: format!("{} {}", r.method, r.name),
                        subtitle: std::iter::once(c.name.clone())
                            .chain(path)
                            .collect::<Vec<_>>()
                            .join(" / "),
                    })
            })
            .collect()
    }

    fn current_uri(&self) -> Option<String> {
        match (&self.state.collection, &self.state.request_id) {
            (Some(c), Some(r)) => Some(format!("api://{c}/{r}")),
            _ => None,
        }
    }

    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        if !uri.starts_with("api://") {
            return None;
        }
        // One tab per request: a tab that already holds one keeps it, and
        // the shell opens another. A tab with nothing open takes the
        // request rather than leaving a blank tab behind. The exact URI is
        // matched before affinity is asked, so reopening a request focuses
        // the tab that has it. Owner's requirement, 2026-10-17.
        (self.state.request_id.is_none() && !self.is_dirty()).then_some(50)
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::File { uri, .. } if uri.starts_with("api://") => {
                let rest = &uri[6..];
                if let Some((c, r)) = rest.split_once('/') {
                    let (c, r) = (c.to_string(), r.to_string());
                    self.open_request(&c, &r);
                    return true;
                }
                false
            }
            OpenRequest::Text { body, .. } => {
                if let Some((m, u, h, b)) = send::parse_curl(&body) {
                    self.new_request();
                    self.request.method = m;
                    self.request.url = u;
                    self.request.headers = h.into_iter().map(|(k, v)| Kv::new(&k, &v)).collect();
                    if let Some(b) = b {
                        self.request.body.kind = if b.trim_start().starts_with('{') {
                            BodyKind::Json
                        } else {
                            BodyKind::Text
                        };
                        self.body_view.set_text_pretty(&b, self.request.body.kind);
                    }
                    true
                } else {
                    let _ = cx;
                    false
                }
            }
            _ => false,
        }
    }

    /// Duplicating an API tab makes a second *request*, not a second
    /// window onto the one request: a numbered copy beside the original,
    /// in the same collection and folder, which the new tab then opens.
    /// Owner's requirement, 2026-10-17.
    fn duplicate_state(&mut self, cx: &mut ToolContext) -> serde_json::Value {
        let here = self
            .state
            .collection
            .clone()
            .zip(self.state.request_id.clone());
        let Some((collection, id)) = here else {
            // An unsaved draft has nothing to copy beside; the new tab
            // gets the same draft, which is what the default would do.
            return self.save_state();
        };
        let found = self
            .collections
            .iter()
            .find(|x| x.id == collection)
            .and_then(|x| {
                x.find(&id)
                    .map(|(r, folders)| (r.clone(), folders.last().map(|f| f.id.clone())))
            });
        let Some((mut copy, folder)) = found else {
            return self.save_state();
        };
        copy.id = hs_core::new_id();
        let mut new_id = None;
        if let Some(col) = self.collections.iter_mut().find(|x| x.id == collection)
            && let Some(items) = col.items_mut(folder.as_deref())
        {
            let taken: Vec<String> = items.iter().map(|i| i.name().to_string()).collect();
            copy.name = crate::model::next_copy_name(&copy.name, &taken);
            new_id = Some(copy.id.clone());
            items.push(Item::Request(copy));
        }
        let Some(new_id) = new_id else {
            return self.save_state();
        };
        self.save_collection(&collection, cx);
        let st = State {
            collection: Some(collection),
            request_id: Some(new_id),
            draft: None,
            req_tab: self.state.req_tab,
            resp_tab: self.state.resp_tab,
        };
        serde_json::to_value(st).unwrap_or(serde_json::Value::Null)
    }

    fn save_state(&self) -> serde_json::Value {
        let mut st = self.state.clone();
        if st.request_id.is_none() {
            let mut d = self.request.clone();
            d.body.text = self.body_view.text();
            st.draft = Some(d);
        }
        serde_json::to_value(st).unwrap_or(serde_json::Value::Null)
    }

    fn on_event(&mut self, ev: &Event, cx: &mut ToolContext) {
        match ev {
            Event::WorkspaceSwitched { .. } => self.reload(cx.services),
            Event::Custom { topic, payload } if topic == COLLECTIONS_CHANGED => {
                if payload == &cx.tab_id.to_string() {
                    return;
                }
                // Only the collections: the request being edited in this
                // tab is this tab's business and must not be replaced.
                self.collections = Collection::load_all(&self.collections_dir());
            }
            _ => {}
        }
    }

    fn speakable(&self) -> Vec<Speakable> {
        let mut out = Vec::new();
        if let Some(a) = self.assistant.last_answer() {
            out.push(Speakable::new("Assistant's last answer", a));
        }
        if let Some(Outcome {
            exchange: Ok(ex), ..
        }) = &self.last
        {
            let body = String::from_utf8_lossy(&ex.body);
            if !body.trim().is_empty() {
                out.push(Speakable::new("Response body", body.into_owned()));
            }
        }
        out
    }

    fn printable(&self) -> Option<Printable> {
        let mut html = String::new();
        html.push_str("<h2>Request</h2>");
        html.push_str(&hs_core::html::pre(&format!(
            "{} {}",
            self.request.method,
            self.sent_title
                .split_once(' ')
                .map(|(_, u)| u.to_string())
                .unwrap_or_else(|| self.request.url.clone())
        )));
        if let Some(Outcome {
            exchange: Ok(ex), ..
        }) = &self.last
        {
            html.push_str(&format!(
                "<h2>Response · {} {} · {} ms</h2>",
                ex.status,
                hs_core::html::escape(&ex.status_text),
                ex.millis
            ));
            let headers: Vec<Vec<String>> = ex
                .headers
                .iter()
                .map(|(k, v)| vec![k.clone(), v.clone()])
                .collect();
            html.push_str(&hs_core::html::table(
                &["Header".into(), "Value".into()],
                &headers,
            ));
            let body = String::from_utf8_lossy(&ex.body);
            let pretty = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or_else(|| body.into_owned());
            html.push_str(&hs_core::html::pre(&pretty));
        }
        Some(Printable {
            title: self.title().to_string(),
            subtitle: "API client".into(),
            html,
        })
    }
}

impl ApiTool {
    /// Send, unless the active environment is protected and the method
    /// writes: then ask first.
    pub(crate) fn request_send(&mut self, cx: &mut ToolContext) {
        let protected = cx
            .services
            .environments
            .read()
            .active_env()
            .filter(|e| e.protected)
            .map(|e| e.name.clone());
        if let Some(env) = protected
            && !matches!(self.request.method.as_str(), "GET" | "HEAD" | "OPTIONS")
        {
            self.confirm_send = Some(env);
            return;
        }
        self.send(cx);
    }
}

#[allow(dead_code)]
fn _assert_types(_: Body) {}
