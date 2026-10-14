//! S3 tool (docs/tools/s3.md): one connection profile per tab. The sidebar
//! lists `kind = "s3"` profiles and the selected profile's buckets; the main
//! area browses a bucket by prefix with a breadcrumb, loads 1,000 keys a page,
//! and offers upload (multipart above 16 MiB), download, new folder,
//! guarded delete, presigned URLs and an editor round trip. Transfers run as
//! Jobs with progress and cancel; the client is `hs_core::objstore::s3`.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hs_core::Services;
use hs_core::events::Event;
use hs_core::jobs::Progress;
use hs_core::objstore::profile::{AwsProfile, ProfileError, S3Fields, load_aws_profiles};
use hs_core::objstore::s3::{Bucket, Object, S3Client, Versioning};
use hs_core::tool::{
    Accept, Action, CommandDescriptor, OpenRequest, ToastLevel, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "s3",
    title_key: "tool-s3",
    icon: ph::CLOUD,
    category: ToolCategory::Network,
    accepts: &[Accept::Connection],
};

/// Objects larger than this are not opened in the editor.
const EDIT_LIMIT: u64 = 32 * 1024 * 1024;

pub struct S3Factory;

impl ToolFactory for S3Factory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Box::new(S3Tool::new(services, state))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    conn_id: Option<String>,
    bucket: Option<String>,
    prefix: String,
}

#[derive(Default)]
enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

/// What async tasks write and the UI reads at frame start.
#[derive(Default)]
struct Shared {
    buckets: Load<Vec<Bucket>>,
    /// Objects and folders under the current prefix, and the next page token.
    objects: Load<(Vec<Object>, Vec<String>, Option<String>)>,
    /// Which (bucket, prefix) the listing belongs to, so stale pages are dropped.
    listing_for: (String, String),
    versioning: Option<Versioning>,
    toasts: Vec<(ToastLevel, String)>,
    open_in_editor: Vec<PathBuf>,
    /// Local temp copies opened in the editor → (bucket, key) to put back.
    edits: HashMap<PathBuf, (String, String)>,
    refresh: bool,
}

/// One row of the browser: a folder (common prefix) or an object.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    Folder(String),
    Object(Object),
}

/// What a dialog button does once the dialog has closed.
type DialogAction = Box<dyn FnOnce(&mut S3Tool, &Services)>;

enum DialogState {
    NewFolder(String),
    Delete {
        target: Row,
        /// Required text (the bucket name) for a guarded delete.
        typed: String,
        guarded: bool,
    },
    Presign {
        key: String,
        put: bool,
        expires: u64,
        url: String,
    },
    ImportAws {
        profiles: Vec<(AwsProfile, bool)>,
    },
}

pub struct S3Tool {
    state: State,
    shared: Arc<Mutex<Shared>>,
    client: Option<Arc<S3Client>>,
    client_error: Option<String>,
    waiting_for_vault: bool,
    table: TableState,
    dialog: Option<DialogState>,
    /// The profile's environment (drives the banner and the delete guard).
    env_id: Option<String>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

impl S3Tool {
    fn new(services: &Services, state: State) -> Self {
        let mut t = Self {
            state,
            shared: Arc::new(Mutex::new(Shared::default())),
            client: None,
            client_error: None,
            waiting_for_vault: false,
            table: TableState::default(),
            dialog: None,
            env_id: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
        };
        if t.state.conn_id.is_some() {
            t.connect(services);
        }
        t
    }

    fn fields(&self, services: &Services) -> Option<(String, S3Fields)> {
        let id = self.state.conn_id.as_ref()?;
        let conns = services.connections.read();
        let c = conns.by_id(id)?;
        Some((c.name.clone(), S3Fields::from_table(&c.fields)))
    }

    /// Build the client for the selected profile and list its buckets (or
    /// open the remembered / default bucket).
    fn connect(&mut self, services: &Services) {
        self.client = None;
        self.client_error = None;
        self.waiting_for_vault = false;
        self.env_id = self.environment_of(services);
        let Some((_, f)) = self.fields(services) else {
            self.client_error = Some(services.t("s3-no-profile"));
            return;
        };
        let config = match f.config(services.secrets.as_ref()) {
            Ok(c) => c,
            Err(e) => {
                self.waiting_for_vault = e == ProfileError::VaultLocked;
                self.client_error = Some(match e {
                    ProfileError::VaultLocked => services.t("s3-vault-locked"),
                    ProfileError::NoKeys => services.t("s3-no-keys"),
                    ProfileError::Secret(e) => e,
                });
                return;
            }
        };
        match S3Client::new(config) {
            Ok(c) => self.client = Some(Arc::new(c)),
            Err(e) => {
                self.client_error = Some(e.to_string());
                return;
            }
        }
        if self.state.bucket.is_none() && !f.default_bucket.is_empty() {
            self.state.bucket = Some(f.default_bucket.clone());
        }
        self.load_buckets(services);
        if self.state.bucket.is_some() {
            self.load_objects(services, None);
        }
    }

    fn load_buckets(&mut self, services: &Services) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.shared.lock().buckets = Load::Loading;
        let shared = self.shared.clone();
        services.runtime.spawn(async move {
            let r = client.list_buckets().await;
            shared.lock().buckets = match r {
                Ok(mut b) => {
                    b.sort_by(|x, y| x.name.cmp(&y.name));
                    Load::Ready(b)
                }
                Err(e) => Load::Failed(e.to_string()),
            };
        });
    }

    /// First page of the current prefix, or the next page when `token`.
    fn load_objects(&mut self, services: &Services, token: Option<String>) {
        let (Some(client), Some(bucket)) = (self.client.clone(), self.state.bucket.clone()) else {
            return;
        };
        let prefix = self.state.prefix.clone();
        let more = token.is_some();
        {
            let mut sh = self.shared.lock();
            sh.listing_for = (bucket.clone(), prefix.clone());
            if !more {
                sh.objects = Load::Loading;
                sh.versioning = None;
            }
        }
        if !more {
            self.table.selected = None;
        }
        let shared = self.shared.clone();
        services.runtime.spawn(async move {
            let r = client
                .list(&bucket, &prefix, true, token.as_deref(), 1000)
                .await;
            let versioning = if more {
                None
            } else {
                client.versioning(&bucket).await.ok()
            };
            let mut sh = shared.lock();
            if sh.listing_for != (bucket, prefix) {
                return; // the user moved on
            }
            if versioning.is_some() {
                sh.versioning = versioning;
            }
            match r {
                Ok(page) => {
                    let page_objects: Vec<Object> = page.objects;
                    match (&mut sh.objects, more) {
                        (Load::Ready((objs, prefixes, next)), true) => {
                            objs.extend(page_objects);
                            prefixes.extend(page.prefixes);
                            *next = page.next;
                        }
                        _ => sh.objects = Load::Ready((page_objects, page.prefixes, page.next)),
                    }
                }
                Err(e) => sh.objects = Load::Failed(e.to_string()),
            }
        });
    }

    fn open_bucket(&mut self, services: &Services, bucket: String) {
        self.state.bucket = Some(bucket);
        self.state.prefix.clear();
        self.load_objects(services, None);
    }

    fn navigate(&mut self, services: &Services, prefix: String) {
        self.state.prefix = prefix;
        self.load_objects(services, None);
    }

    fn up(&mut self, services: &Services) {
        if self.state.prefix.is_empty() {
            if self.state.bucket.take().is_some() {
                self.shared.lock().objects = Load::Idle;
            }
            return;
        }
        let trimmed = self.state.prefix.trim_end_matches('/');
        let parent = match trimmed.rfind('/') {
            Some(i) => trimmed[..=i].to_string(),
            None => String::new(),
        };
        self.navigate(services, parent);
    }

    fn rows(&self) -> Vec<Row> {
        let sh = self.shared.lock();
        let Load::Ready((objects, prefixes, _)) = &sh.objects else {
            return Vec::new();
        };
        let mut rows: Vec<Row> = prefixes.iter().cloned().map(Row::Folder).collect();
        rows.extend(
            objects
                .iter()
                // The folder's own zero-byte marker.
                .filter(|o| o.key != self.state.prefix)
                .cloned()
                .map(Row::Object),
        );
        rows
    }

    fn selected_row(&self) -> Option<Row> {
        let i = self.table.selected?;
        self.rows().get(i).cloned()
    }

    fn is_protected(&self, services: &Services) -> bool {
        let env_id = self
            .environment_of(services)
            .or_else(|| services.environments.read().active.clone());
        env_id
            .and_then(|id| services.environments.read().by_id(&id).map(|e| e.protected))
            .unwrap_or(false)
    }

    fn environment_of(&self, services: &Services) -> Option<String> {
        let id = self.state.conn_id.as_ref()?;
        services
            .connections
            .read()
            .by_id(id)
            .and_then(|c| c.environment.clone())
    }

    /// Spawn a Job; `done` messages land as toasts and trigger a refresh.
    fn job<F, Fut>(&self, services: &Services, title: String, f: F)
    where
        F: FnOnce(Arc<S3Client>, hs_core::jobs::JobHandle) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        let Some(client) = self.client.clone() else {
            return;
        };
        let shared = self.shared.clone();
        let waker = services.runtime.waker();
        services.jobs.spawn(title, "s3", move |h| async move {
            let r = f(client, h).await;
            {
                let mut sh = shared.lock();
                match &r {
                    Ok(msg) => {
                        if !msg.is_empty() {
                            sh.toasts.push((ToastLevel::Success, msg.clone()));
                        }
                        sh.refresh = true;
                    }
                    Err(e) => sh.toasts.push((ToastLevel::Error, e.clone())),
                }
            }
            waker.request_repaint();
            r.map(|_| ())
        });
    }

    fn upload(&mut self, services: &Services) {
        let Some(bucket) = self.state.bucket.clone() else {
            return;
        };
        let Some(files) = rfd::FileDialog::new().pick_files() else {
            return;
        };
        for path in files {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let key = format!("{}{name}", self.state.prefix);
            let bucket = bucket.clone();
            let ok = services.i18n.t_args("s3-uploaded", &[("name", &name)]);
            self.job(
                services,
                format!("Upload s3://{bucket}/{key}"),
                move |client, h| async move {
                    let h2 = h.clone();
                    client
                        .upload_file(
                            &bucket,
                            &key,
                            &path,
                            Arc::new(move |done, total| {
                                h2.set_progress(Progress::Bytes {
                                    done,
                                    total: Some(total),
                                })
                            }),
                            h.cancel_token(),
                        )
                        .await
                        .map(|_| ok)
                        .map_err(|e| e.to_string())
                },
            );
        }
    }

    fn download(&mut self, services: &Services, obj: Object) {
        let Some(bucket) = self.state.bucket.clone() else {
            return;
        };
        let name = obj.key.rsplit('/').next().unwrap_or(&obj.key).to_string();
        let Some(dest) = rfd::FileDialog::new().set_file_name(&name).save_file() else {
            return;
        };
        let ok = services.i18n.t_args("s3-downloaded", &[("name", &name)]);
        self.job(
            services,
            format!("Download s3://{bucket}/{}", obj.key),
            move |client, h| async move {
                let h2 = h.clone();
                client
                    .download(
                        &bucket,
                        &obj.key,
                        &dest,
                        Arc::new(move |done, total| {
                            h2.set_progress(Progress::Bytes {
                                done,
                                total: Some(total),
                            })
                        }),
                        h.cancel_token(),
                    )
                    .await
                    .map(|_| ok)
                    .map_err(|e| e.to_string())
            },
        );
    }

    fn open_in_editor(&mut self, services: &Services, obj: Object) {
        let Some(bucket) = self.state.bucket.clone() else {
            return;
        };
        if obj.size > EDIT_LIMIT {
            self.shared
                .lock()
                .toasts
                .push((ToastLevel::Warning, services.t("s3-too-big-to-edit")));
            return;
        }
        let conn = self.state.conn_id.clone().unwrap_or_default();
        let local = services
            .paths
            .tmp()
            .join("s3")
            .join(hs_core::storage::slug(&conn))
            .join(hs_core::storage::slug(&bucket))
            .join(obj.key.trim_start_matches('/'));
        self.shared
            .lock()
            .edits
            .insert(local.clone(), (bucket.clone(), obj.key.clone()));
        let shared = self.shared.clone();
        self.job(
            services,
            format!("Open s3://{bucket}/{}", obj.key),
            move |client, _h| async move {
                let bytes = client
                    .get_bytes(&bucket, &obj.key)
                    .await
                    .map_err(|e| e.to_string())?;
                if let Some(p) = local.parent() {
                    std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                std::fs::write(&local, bytes).map_err(|e| e.to_string())?;
                shared.lock().open_in_editor.push(local);
                Ok(String::new())
            },
        );
    }

    fn put_back(&mut self, services: &Services, local: PathBuf, bucket: String, key: String) {
        let ok = services.i18n.t_args("s3-saved", &[("name", &key)]);
        self.job(
            services,
            format!("Save s3://{bucket}/{key}"),
            move |client, _h| async move {
                let bytes = std::fs::read(&local).map_err(|e| e.to_string())?;
                let ct = hs_core::objstore::s3::content_type_for(&key);
                client
                    .put_bytes(&bucket, &key, bytes, ct)
                    .await
                    .map(|_| ok)
                    .map_err(|e| e.to_string())
            },
        );
    }

    fn create_folder(&mut self, services: &Services, name: &str) {
        let Some(bucket) = self.state.bucket.clone() else {
            return;
        };
        let name = name.trim().trim_matches('/');
        if name.is_empty() {
            return;
        }
        let key = format!("{}{name}/", self.state.prefix);
        let ok = services.t("s3-folder-created");
        self.job(
            services,
            format!("New folder s3://{bucket}/{key}"),
            move |client, _h| async move {
                client
                    .put_bytes(&bucket, &key, Vec::new(), "")
                    .await
                    .map(|_| ok)
                    .map_err(|e| e.to_string())
            },
        );
    }

    fn delete(&mut self, services: &Services, target: Row) {
        let Some(bucket) = self.state.bucket.clone() else {
            return;
        };
        let ok = services.t("s3-deleted");
        match target {
            Row::Object(o) => self.job(
                services,
                format!("Delete s3://{bucket}/{}", o.key),
                move |client, _h| async move {
                    client
                        .delete(&bucket, &o.key)
                        .await
                        .map(|_| ok)
                        .map_err(|e| e.to_string())
                },
            ),
            Row::Folder(prefix) => self.job(
                services,
                format!("Delete s3://{bucket}/{prefix}"),
                move |client, h| async move {
                    h.set_message("Listing keys…");
                    let keys = client
                        .list_all_keys(&bucket, &prefix)
                        .await
                        .map_err(|e| e.to_string())?;
                    h.set_message(format!("Deleting {} objects", keys.len()));
                    let failed = client
                        .delete_many(&bucket, &keys)
                        .await
                        .map_err(|e| e.to_string())?;
                    if failed.is_empty() {
                        Ok(ok)
                    } else {
                        Err(format!(
                            "{} of {} objects were not deleted ({})",
                            failed.len(),
                            keys.len(),
                            failed[0].1
                        ))
                    }
                },
            ),
        }
    }

    fn ask_delete(&mut self, services: &Services) {
        let Some(target) = self.selected_row() else {
            return;
        };
        // Guarded (type the bucket name): any prefix, or anything on a
        // protected environment (docs/tools/s3.md §9).
        let guarded = matches!(target, Row::Folder(_)) || self.is_protected(services);
        self.dialog = Some(DialogState::Delete {
            target,
            typed: String::new(),
            guarded,
        });
    }

    fn ask_presign(&mut self) {
        if let Some(Row::Object(o)) = self.selected_row() {
            self.dialog = Some(DialogState::Presign {
                key: o.key,
                put: false,
                expires: 3600,
                url: String::new(),
            });
        }
    }

    /// Create connections (secrets into the vault) for the chosen AWS profiles.
    fn import_aws(&mut self, services: &Services, chosen: Vec<AwsProfile>) {
        if services.secrets.is_locked() {
            self.shared
                .lock()
                .toasts
                .push((ToastLevel::Warning, services.t("s3-vault-locked")));
            return;
        }
        let mut count = 0;
        for p in chosen {
            let name = format!("aws: {}", p.name);
            let store = |label: &str, v: &str| -> Result<String, String> {
                if v.is_empty() {
                    return Ok(String::new());
                }
                services
                    .secrets
                    .create(
                        &format!("{name}: {label}"),
                        &secrecy::SecretString::from(v.to_string()),
                    )
                    .map(|r| r.to_string_ref())
                    .map_err(|e| e.to_string())
            };
            let fields = match (
                store("secret key", &p.secret_key),
                store("session token", &p.session_token),
            ) {
                (Ok(secret_key), Ok(session_token)) => S3Fields {
                    provider: if p.endpoint.is_empty() {
                        "aws"
                    } else {
                        "other"
                    }
                    .into(),
                    endpoint: p.endpoint.clone(),
                    region: if p.region.is_empty() {
                        "us-east-1".into()
                    } else {
                        p.region.clone()
                    },
                    access_key: p.access_key.clone(),
                    secret_key,
                    session_token,
                    path_style: !p.endpoint.is_empty(),
                    default_bucket: String::new(),
                },
                (Err(e), _) | (_, Err(e)) => {
                    self.shared.lock().toasts.push((ToastLevel::Error, e));
                    continue;
                }
            };
            let mut c = hs_core::connections::ConnectionProfile {
                name,
                kind: "s3".into(),
                ..Default::default()
            };
            fields.write_into(&mut c.fields);
            match services.connections.write().upsert(c) {
                Ok(()) => count += 1,
                Err(e) => self
                    .shared
                    .lock()
                    .toasts
                    .push((ToastLevel::Error, e.to_string())),
            }
        }
        if count > 0 {
            services
                .events
                .publish(hs_core::events::Event::ConnectionsChanged);
            self.shared.lock().toasts.push((
                ToastLevel::Success,
                services
                    .i18n
                    .t_args("s3-imported", &[("count", &count.to_string())]),
            ));
        }
    }

    fn dialogs(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        let bucket = self.state.bucket.clone().unwrap_or_default();
        let mut close = false;
        let mut action: Option<DialogAction> = None;
        let modal = egui::Modal::new(egui::Id::new(("s3-dialog", self.state.conn_id.clone())))
            .show(ui.ctx(), |ui| {
                ui.set_width(460.0);
                match dialog {
                    DialogState::NewFolder(name) => {
                        ui.heading(s.t("s3-new-folder"));
                        ui.add_space(6.0);
                        let r = ui.add(
                            egui::TextEdit::singleline(name)
                                .hint_text(s.t("s3-folder-name"))
                                .desired_width(f32::INFINITY),
                        );
                        // Focus once when the dialog opens; Enter (which drops
                        // the focus) creates.
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if name.is_empty() && !r.has_focus() {
                            r.request_focus();
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if form::primary_button(ui, theme, &s.t("s3-create")).clicked() || enter
                            {
                                let n = name.clone();
                                action = Some(Box::new(move |t, s| t.create_folder(s, &n)));
                            }
                            if ui.button(s.t("cancel")).clicked() {
                                close = true;
                            }
                        });
                    }
                    DialogState::Delete {
                        target,
                        typed,
                        guarded,
                    } => {
                        let what = match target {
                            Row::Folder(p) => p.clone(),
                            Row::Object(o) => o.key.clone(),
                        };
                        ui.heading(s.t("s3-delete-title"));
                        ui.add_space(6.0);
                        let body = match target {
                            Row::Folder(_) => "s3-delete-prefix-body",
                            Row::Object(_) => "s3-delete-object-body",
                        };
                        ui.label(s.i18n.t_args(body, &[("name", &what), ("bucket", &bucket)]));
                        let versioning = self.shared.lock().versioning;
                        ui.label(
                            egui::RichText::new(s.t(match versioning {
                                Some(Versioning::Enabled) => "s3-versioning-on",
                                Some(Versioning::Suspended) => "s3-versioning-suspended",
                                _ => "s3-versioning-off",
                            }))
                            .color(theme.text_muted),
                        );
                        if *guarded {
                            ui.add_space(6.0);
                            ui.label(s.i18n.t_args("s3-type-bucket", &[("bucket", &bucket)]));
                            ui.add(
                                egui::TextEdit::singleline(typed)
                                    .hint_text(&bucket)
                                    .desired_width(f32::INFINITY),
                            );
                        }
                        ui.add_space(10.0);
                        let allowed = !*guarded || typed.trim() == bucket;
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    allowed,
                                    egui::Button::new(
                                        egui::RichText::new(s.t("s3-delete"))
                                            .color(egui::Color32::WHITE),
                                    )
                                    .fill(theme.error),
                                )
                                .clicked()
                            {
                                let t = target.clone();
                                action = Some(Box::new(move |me, s| me.delete(s, t)));
                            }
                            if ui.button(s.t("cancel")).clicked() {
                                close = true;
                            }
                        });
                    }
                    DialogState::Presign {
                        key,
                        put,
                        expires,
                        url,
                    } => {
                        ui.heading(s.t("s3-presign-title"));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(format!("s3://{bucket}/{key}")).monospace());
                        ui.add_space(6.0);
                        let before = (*put, *expires);
                        ui.horizontal(|ui| {
                            ui.selectable_value(put, false, "GET");
                            ui.selectable_value(put, true, "PUT");
                            ui.separator();
                            for (secs, label) in [
                                (900u64, "15 min"),
                                (3600, "1 h"),
                                (86_400, "24 h"),
                                (604_800, "7 days"),
                            ] {
                                ui.selectable_value(expires, secs, label);
                            }
                        });
                        // Signed once per choice: the signature carries the
                        // time, so re-signing every frame would flicker.
                        if (url.is_empty() || before != (*put, *expires))
                            && let Some(c) = &self.client
                        {
                            *url =
                                c.presign(if *put { "PUT" } else { "GET" }, &bucket, key, *expires);
                        }
                        ui.add_space(6.0);
                        let mut shown = url.clone();
                        ui.add(
                            egui::TextEdit::multiline(&mut shown)
                                .desired_rows(4)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                        ui.label(
                            egui::RichText::new(s.t("s3-presign-hint")).color(theme.text_muted),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if form::primary_button(ui, theme, &s.t("s3-copy-url")).clicked() {
                                ui.ctx().copy_text(url.clone());
                                action = Some(Box::new(|t, s| {
                                    t.shared
                                        .lock()
                                        .toasts
                                        .push((ToastLevel::Success, s.t("s3-url-copied")))
                                }));
                            }
                            if ui.button(s.t("close")).clicked() {
                                close = true;
                            }
                        });
                    }
                    DialogState::ImportAws { profiles } => {
                        ui.heading(s.t("s3-import-title"));
                        ui.add_space(6.0);
                        if profiles.is_empty() {
                            ui.label(s.t("s3-import-none"));
                        } else {
                            ui.label(s.t("s3-import-body"));
                            ui.add_space(4.0);
                            for (p, on) in profiles.iter_mut() {
                                let region = if p.region.is_empty() {
                                    "—"
                                } else {
                                    &p.region
                                };
                                ui.checkbox(
                                    on,
                                    format!("{} · {} · {region}", p.name, p.access_key),
                                );
                            }
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            let chosen: Vec<AwsProfile> = profiles
                                .iter()
                                .filter(|(_, on)| *on)
                                .map(|(p, _)| p.clone())
                                .collect();
                            if ui
                                .add_enabled(
                                    !chosen.is_empty(),
                                    egui::Button::new(s.t("s3-import")),
                                )
                                .clicked()
                            {
                                action = Some(Box::new(move |t, s| t.import_aws(s, chosen)));
                            }
                            if ui.button(s.t("cancel")).clicked() {
                                close = true;
                            }
                        });
                    }
                }
            });
        if modal.should_close() {
            close = true;
        }
        if let Some(a) = action {
            self.dialog = None;
            a(self, s);
        } else if close {
            self.dialog = None;
        }
    }

    fn browser(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        let bucket = self.state.bucket.clone().unwrap_or_default();
        // Breadcrumb: bucket ▸ segment ▸ segment.
        let mut go: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(egui::Button::new(format!("{} {bucket}", ph::ARCHIVE)).frame(false))
                .clicked()
            {
                go = Some(String::new());
            }
            let mut acc = String::new();
            for seg in self.state.prefix.split('/').filter(|p| !p.is_empty()) {
                acc.push_str(seg);
                acc.push('/');
                ui.label(egui::RichText::new(ph::CARET_RIGHT).color(theme.text_muted));
                if ui.add(egui::Button::new(seg).frame(false)).clicked() {
                    go = Some(acc.clone());
                }
            }
        });
        if let Some(p) = go {
            self.navigate(s, p);
        }
        ui.add_space(4.0);
        let (status, next, loaded) = {
            let sh = self.shared.lock();
            match &sh.objects {
                Load::Idle | Load::Loading => (Some(s.t("s3-loading")), None, 0),
                Load::Failed(e) => (Some(e.clone()), None, 0),
                Load::Ready((o, p, n)) => (None, n.clone(), o.len() + p.len()),
            }
        };
        if let Some(msg) = status {
            ui.label(egui::RichText::new(msg).color(theme.text_muted));
            return;
        }
        let rows = self.rows();
        if rows.is_empty() {
            form::note(ui, theme, ph::INFO, &s.t("s3-empty-prefix"));
        }
        let cells: Vec<Vec<String>> = rows
            .iter()
            .map(|r| match r {
                Row::Folder(p) => {
                    let name = p.strip_prefix(&self.state.prefix).unwrap_or(p).to_string();
                    vec![
                        format!("{} {name}", ph::FOLDER_SIMPLE),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                    ]
                }
                Row::Object(o) => {
                    let name = o
                        .key
                        .strip_prefix(&self.state.prefix)
                        .unwrap_or(&o.key)
                        .to_string();
                    vec![
                        format!("{} {name}", ph::FILE),
                        human_size(o.size),
                        short_time(&o.modified),
                        o.storage_class.clone(),
                        o.etag.chars().take(12).collect(),
                    ]
                }
            })
            .collect();
        let columns = [
            ColumnSpec::new(s.t("s3-col-name"), 320.0),
            ColumnSpec::new(s.t("s3-col-size"), 90.0),
            ColumnSpec::new(s.t("s3-col-modified"), 140.0),
            ColumnSpec::new(s.t("s3-col-class"), 100.0),
            ColumnSpec::new(s.t("s3-col-etag"), 110.0),
        ];
        let copy_label = s.t("table-copy-row");
        let footer_h = 30.0;
        let table_h = (ui.available_height() - footer_h).max(80.0);
        let r = ui
            .allocate_ui(egui::vec2(ui.available_width(), table_h), |ui| {
                DataTable {
                    theme,
                    columns: &columns,
                    rows: &cells,
                    row_height: 24.0,
                    show_filter: true,
                    copy_row_label: &copy_label,
                }
                .show(ui, &mut self.table)
            })
            .inner;
        if let Some(i) = r.double_clicked {
            match rows.get(i).cloned() {
                Some(Row::Folder(p)) => self.navigate(s, p),
                Some(Row::Object(o)) => self.open_in_editor(s, o),
                None => {}
            }
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(
                    s.i18n
                        .t_args("s3-loaded", &[("count", &loaded.to_string())]),
                )
                .color(theme.text_muted),
            );
            if let Some(token) = next
                && ui.button(s.t("s3-load-more")).clicked()
            {
                self.load_objects(s, Some(token));
            }
        });
    }
}

impl Tool for S3Tool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        match &self.state.bucket {
            Some(b) => Cow::Owned(format!("s3://{b}")),
            None => Cow::Borrowed("S3"),
        }
    }

    fn environment(&self) -> Option<String> {
        self.env_id.clone()
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let in_bucket = self.state.bucket.is_some() && self.client.is_some();
        if ui
            .button(format!("{} AI", ph::SPARKLE))
            .tip(s.t("ai-shared-button-tip"))
            .clicked()
        {
            self.assistant.toggle("S3");
        }
        let selected = self.selected_row();
        let is_object = matches!(selected, Some(Row::Object(_)));
        let btn = |ui: &mut egui::Ui, enabled: bool, icon: &str, key: &str| {
            ui.add_enabled(enabled, egui::Button::new(format!("{icon} {}", s.t(key))))
                .clicked()
        };
        if btn(ui, self.client.is_some(), ph::ARROW_UP, "s3-up") {
            self.up(s);
        }
        if btn(ui, self.client.is_some(), ph::ARROW_CLOCKWISE, "s3-refresh") {
            self.load_buckets(s);
            self.load_objects(s, None);
        }
        ui.separator();
        if btn(ui, in_bucket, ph::UPLOAD_SIMPLE, "s3-upload") {
            self.upload(s);
        }
        if btn(
            ui,
            in_bucket && is_object,
            ph::DOWNLOAD_SIMPLE,
            "s3-download",
        ) && let Some(Row::Object(o)) = selected.clone()
        {
            self.download(s, o);
        }
        if btn(ui, in_bucket, ph::FOLDER_PLUS, "s3-new-folder") {
            self.dialog = Some(DialogState::NewFolder(String::new()));
        }
        if btn(ui, in_bucket && is_object, ph::PENCIL_SIMPLE, "s3-edit")
            && let Some(Row::Object(o)) = selected.clone()
        {
            self.open_in_editor(s, o);
        }
        if btn(ui, in_bucket && is_object, ph::LINK, "s3-presign") {
            self.ask_presign();
        }
        if btn(ui, in_bucket && selected.is_some(), ph::TRASH, "s3-delete") {
            self.ask_delete(s);
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        form::header_bar(ui, &theme, |ui| {
            ui.label(egui::RichText::new(s.t("s3-profiles")).strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::icon_button(ui, &theme, ph::PLUS, 16.0, false)
                    .tip(s.t("s3-new-profile"))
                    .clicked()
                {
                    cx.actions.push(Action::RunCommand {
                        id: "settings:connections".into(),
                    });
                }
                if form::icon_button(ui, &theme, ph::CLOUD_ARROW_DOWN, 16.0, false)
                    .tip(s.t("s3-import-title"))
                    .clicked()
                {
                    self.dialog = Some(DialogState::ImportAws {
                        profiles: load_aws_profiles().into_iter().map(|p| (p, true)).collect(),
                    });
                }
            });
        });
        let profiles: Vec<(String, String, String)> = s
            .connections
            .read()
            .of_kind("s3")
            .map(|c| {
                (
                    c.id.clone(),
                    c.name.clone(),
                    S3Fields::from_table(&c.fields).label(),
                )
            })
            .collect();
        if profiles.is_empty() {
            form::note(ui, &theme, ph::LIGHTBULB, &s.t("s3-no-profiles"));
            return true;
        }
        for (id, name, label) in profiles {
            let selected = self.state.conn_id.as_deref() == Some(&id);
            if form::list_row(ui, &theme, selected, ph::CLOUD, &name, Some(&label))
                .tip(s.t("s3-open-profile"))
                .clicked()
            {
                if self.state.conn_id.is_none() {
                    self.state.conn_id = Some(id);
                    self.state.bucket = None;
                    self.state.prefix.clear();
                    self.connect(s);
                } else if !selected {
                    cx.open("s3", Some(OpenRequest::Connection { id }));
                }
            }
            ui.add_space(2.0);
            if selected {
                let buckets: Vec<String> = match &self.shared.lock().buckets {
                    Load::Ready(b) => b.iter().map(|b| b.name.clone()).collect(),
                    Load::Loading => {
                        ui.label(egui::RichText::new(s.t("s3-loading")).weak());
                        Vec::new()
                    }
                    Load::Failed(e) => {
                        ui.colored_label(theme.error, e);
                        Vec::new()
                    }
                    Load::Idle => Vec::new(),
                };
                let mut open: Option<String> = None;
                for b in buckets {
                    let on = self.state.bucket.as_deref() == Some(&b);
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        if ui
                            .selectable_label(on, format!("{} {b}", ph::ARCHIVE))
                            .clicked()
                        {
                            open = Some(b.clone());
                        }
                    });
                }
                if let Some(b) = open {
                    self.open_bucket(s, b);
                }
                ui.add_space(6.0);
            }
        }
        ui.add_space(10.0);
        form::note(ui, &theme, ph::LIGHTBULB, &s.t("s3-hint"));
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if self.waiting_for_vault && !s.secrets.is_locked() {
            self.connect(s);
        }
        let (toasts, open, refresh) = {
            let mut sh = self.shared.lock();
            (
                std::mem::take(&mut sh.toasts),
                std::mem::take(&mut sh.open_in_editor),
                std::mem::take(&mut sh.refresh),
            )
        };
        for (level, text) in toasts {
            cx.toast(level, text);
        }
        for local in open {
            cx.open(
                "editor",
                Some(OpenRequest::File {
                    uri: local.to_string_lossy().to_string(),
                    line: None,
                }),
            );
        }
        if refresh {
            self.load_objects(s, None);
        }
        if let Some(e) = self.client_error.clone() {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.colored_label(theme.error, e);
                if self.state.conn_id.is_some() && ui.button(s.t("s3-retry")).clicked() {
                    self.connect(s);
                }
            });
        } else if self.state.conn_id.is_none() {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.label(egui::RichText::new(s.t("s3-pick-profile")).weak());
            });
        } else if self.state.bucket.is_none() {
            let (buckets, failed) = match &self.shared.lock().buckets {
                Load::Ready(b) => (Some(b.clone()), None),
                Load::Failed(e) => (None, Some(e.clone())),
                _ => (None, None),
            };
            ui.add_space(6.0);
            if let Some(e) = failed {
                ui.colored_label(theme.error, e);
                if ui.button(s.t("s3-retry")).clicked() {
                    self.connect(s);
                }
                self.dialogs(ui, cx, &theme);
                return;
            }
            match buckets {
                Some(b) if b.is_empty() => {
                    form::note(ui, &theme, ph::INFO, &s.t("s3-no-buckets"));
                }
                Some(b) => {
                    ui.label(egui::RichText::new(s.t("s3-pick-bucket")).weak());
                    ui.add_space(4.0);
                    let mut open = None;
                    for bucket in b {
                        let created = short_time(&bucket.created);
                        if form::list_row(
                            ui,
                            &theme,
                            false,
                            ph::ARCHIVE,
                            &bucket.name,
                            Some(&created),
                        )
                        .clicked()
                        {
                            open = Some(bucket.name.clone());
                        }
                        ui.add_space(2.0);
                    }
                    if let Some(b) = open {
                        self.open_bucket(s, b);
                    }
                }
                None => {
                    ui.label(egui::RichText::new(s.t("s3-loading")).weak());
                }
            }
        } else {
            self.browser(ui, cx, &theme);
        }
        self.dialogs(ui, cx, &theme);
        if self.assistant.open {
            let selected = self
                .selected_row()
                .map(|row| match row {
                    Row::Folder(prefix) => format!("Folder prefix: {prefix}"),
                    Row::Object(object) => format!(
                        "Object key: {}\nSize: {} bytes\nETag: {}",
                        object.key, object.size, object.etag
                    ),
                })
                .unwrap_or_default();
            let profile = self
                .state
                .conn_id
                .as_ref()
                .and_then(|id| s.connections.read().by_id(id).map(|p| p.name.clone()))
                .unwrap_or_else(|| "(none)".into());
            let listing_error = match &self.shared.lock().objects {
                Load::Failed(e) => e.clone(),
                _ => "(none)".into(),
            };
            let context = format!(
                "Profile: {profile}\nBucket: {}\nPrefix: {}\nSelected item:\n{}\n\nLast listing error: {listing_error}",
                self.state.bucket.as_deref().unwrap_or("(none)"),
                self.state.prefix,
                selected
            );
            self.assistant.set_context("S3 selection", context);
            let rect = ui.available_rect_before_wrap();
            let panel = egui::Rect::from_min_max(
                egui::pos2(rect.right() - 390.0, rect.top()),
                rect.right_bottom(),
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
                    let _ = self.assistant.ui(ui, s, &theme, "S3");
                });
        }
    }

    fn speakable(&self) -> Vec<hs_core::tool::Speakable> {
        self.assistant
            .last_answer()
            .map(|a| vec![hs_core::tool::Speakable::new("Assistant's last answer", a)])
            .unwrap_or_default()
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        let c = |id: &str, title: &str| CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: None,
        };
        vec![
            c("s3.ai", "AI: Toggle Assistant"),
            c("s3.refresh", "S3: Refresh"),
            c("s3.up", "S3: Parent Folder"),
            c("s3.upload", "S3: Upload Files"),
            c("s3.new_folder", "S3: New Folder"),
            c("s3.delete", "S3: Delete"),
            c("s3.presign", "S3: Presigned URL"),
            c("s3.import_aws", "S3: Import AWS Profiles"),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        match id {
            "s3.ai" => self.assistant.toggle("S3"),
            "s3.refresh" | "run.alt" => {
                self.load_buckets(s);
                self.load_objects(s, None);
            }
            "s3.up" => self.up(s),
            "s3.upload" => self.upload(s),
            "s3.new_folder" => self.dialog = Some(DialogState::NewFolder(String::new())),
            "s3.delete" => self.ask_delete(s),
            "s3.presign" => self.ask_presign(),
            "s3.import_aws" => {
                self.dialog = Some(DialogState::ImportAws {
                    profiles: load_aws_profiles().into_iter().map(|p| (p, true)).collect(),
                })
            }
            _ => return false,
        }
        true
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or_default()
    }

    fn current_connection(&self) -> Option<&str> {
        self.state.conn_id.as_deref()
    }

    fn current_uri(&self) -> Option<String> {
        self.state.conn_id.as_ref().map(|id| format!("s3://{id}"))
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Connection { id } => {
                if self.state.conn_id.as_deref() == Some(&id) {
                    if self.client.is_none() {
                        self.connect(cx.services);
                    }
                } else if self.state.conn_id.is_none() {
                    self.state.conn_id = Some(id);
                    self.connect(cx.services);
                } else {
                    cx.open("s3", Some(OpenRequest::Connection { id }));
                }
                true
            }
            _ => false,
        }
    }

    fn on_event(&mut self, ev: &Event, cx: &mut ToolContext) {
        if let Event::FileSaved { uri } = ev {
            let local = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
            let target = self.shared.lock().edits.get(&local).cloned();
            if let Some((bucket, key)) = target {
                self.put_back(cx.services, local, bucket, key);
            }
        }
    }
}

fn human_size(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// `2026-10-11T17:31:05.000Z` → `2026-10-11 17:31`.
fn short_time(iso: &str) -> String {
    if iso.len() >= 16 && iso.as_bytes()[10] == b'T' {
        format!("{} {}", &iso[..10], &iso[11..16])
    } else {
        iso.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_sizes_read_well() {
        assert_eq!(short_time("2026-10-11T17:31:05.000Z"), "2026-10-11 17:31");
        assert_eq!(short_time("x"), "x");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(20 * 1024 * 1024), "20.0 MB");
    }

    #[test]
    fn up_walks_prefixes_then_leaves_the_bucket() {
        let parent = |p: &str| {
            let trimmed = p.trim_end_matches('/');
            match trimmed.rfind('/') {
                Some(i) => trimmed[..=i].to_string(),
                None => String::new(),
            }
        };
        assert_eq!(parent("a/b/c/"), "a/b/");
        assert_eq!(parent("a/"), "");
    }
}
