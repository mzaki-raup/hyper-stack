//! SSH tool (docs/tools/ssh-sftp.md): one host per tab with a dual pane —
//! the remote file browser (SFTP) on the left and the shell on the right.
//! The two stay in sync: the shell's working directory (from its window
//! title) drives the browser, and navigating the browser sends `cd` to the
//! shell. Hosts come from `kind = "ssh"` connection profiles.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hs_core::Services;
use hs_core::events::Event;
use hs_core::secrets::SecretRef;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, ToastLevel, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use hs_ssh::{Auth, HostKeyPrompt, KnownHosts, SftpSession, SshClient, SshConfig, SshFields};
use hs_term::session::{Session, TermEvent};
use hs_term::shells::ShellProfile;
use hs_term::widget::{TermTheme, TerminalView};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "ssh",
    title_key: "tool-ssh",
    icon: ph::COMPUTER_TOWER,
    category: ToolCategory::Network,
    accepts: &[Accept::Connection],
};

pub struct SshFactory;

impl ToolFactory for SshFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Box::new(SshTool::new(services, state))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct State {
    conn_id: Option<String>,
    sync_cwd: bool,
    remote_path: Option<String>,
    show_hidden: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            conn_id: None,
            sync_cwd: true,
            remote_path: None,
            show_hidden: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Idle,
    Connecting,
    Connected,
    Failed(String),
    Disconnected,
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    is_dir: bool,
    is_link: bool,
    size: u64,
    mtime: Option<i64>,
    perms: String,
}

#[derive(Default)]
struct Remote {
    path: String,
    entries: Vec<Entry>,
    loading: bool,
    error: Option<String>,
    /// Bumped on every listing so stale results are ignored.
    generation: u64,
}

/// Everything the async side writes and the UI reads.
#[derive(Default)]
struct Shared {
    status: Option<Status>,
    shell: Option<(hs_ssh::ShellTransport, hs_ssh::ShellReader)>,
    client: Option<Arc<SshClient>>,
    sftp: Option<Arc<SftpSession>>,
    remote: Remote,
    home: Option<String>,
    hostkey: Option<HostKeyPrompt>,
    toasts: Vec<(ToastLevel, String)>,
    /// Remote files whose local temp copy must be uploaded when saved.
    edits: HashMap<PathBuf, String>,
    /// Local temp files to open in the editor once their download finished.
    open_in_editor: Vec<PathBuf>,
}

pub struct SshTool {
    state: State,
    status: Status,
    shared: Arc<Mutex<Shared>>,
    known_hosts: Arc<KnownHosts>,
    session: Option<Session>,
    view: TerminalView,
    table: TableState,
    path_edit: String,
    path_edit_synced: bool,
    rename: Option<(String, String)>,
    new_folder: Option<String>,
    confirm_delete: Option<String>,
    startup_sent: bool,
    conn_name: String,
}

impl SshTool {
    fn new(services: &Services, state: State) -> Self {
        let st = services.settings.read();
        let mut view = TerminalView::new(
            egui::Id::new(("hs-ssh-term", hs_core::new_id())),
            st.terminal.font_size,
        );
        view.copy_on_select = st.terminal.copy_on_select;
        view.right_click_copy_paste = st.terminal.right_click_copy_paste;
        drop(st);
        let known_hosts = Arc::new(KnownHosts::load(&services.paths.known_hosts_file()));
        let mut t = Self {
            state,
            status: Status::Idle,
            shared: Arc::new(Mutex::new(Shared::default())),
            known_hosts,
            session: None,
            view,
            table: TableState::default(),
            path_edit: String::new(),
            path_edit_synced: true,
            rename: None,
            new_folder: None,
            confirm_delete: None,
            startup_sent: false,
            conn_name: String::new(),
        };
        if t.state.conn_id.is_some() {
            t.connect(services);
        }
        t
    }

    fn fields(&self, services: &Services) -> Option<(String, SshFields)> {
        let id = self.state.conn_id.as_ref()?;
        let conns = services.connections.read();
        let c = conns.by_id(id)?;
        Some((c.name.clone(), SshFields::from_table(&c.fields)))
    }

    fn secret(services: &Services, value: &str) -> Result<Option<secrecy::SecretString>, String> {
        if value.is_empty() {
            return Ok(None);
        }
        match SecretRef::parse(value) {
            Some(r) => {
                if services.secrets.is_locked() {
                    return Err(services.t("ssh-vault-locked"));
                }
                services
                    .secrets
                    .get(&r)
                    .map(Some)
                    .map_err(|e| e.to_string())
            }
            // A plain value (never written by the form, but tolerated).
            None => Ok(Some(secrecy::SecretString::from(value.to_string()))),
        }
    }

    fn connect(&mut self, services: &Services) {
        let Some((name, f)) = self.fields(services) else {
            self.status = Status::Failed(services.t("ssh-no-profile"));
            return;
        };
        self.conn_name = name;
        self.session = None;
        self.startup_sent = false;
        let auth = match f.auth.as_str() {
            "key" => match Self::secret(services, &f.key_passphrase) {
                Ok(p) => Auth::KeyFile {
                    path: PathBuf::from(expand_home(&f.key_path)),
                    passphrase: p,
                },
                Err(e) => {
                    self.status = Status::Failed(e);
                    return;
                }
            },
            "agent" => Auth::Agent,
            _ => match Self::secret(services, &f.password) {
                Ok(Some(p)) => Auth::Password(p),
                Ok(None) => {
                    self.status = Status::Failed(services.t("ssh-no-password"));
                    return;
                }
                Err(e) => {
                    self.status = Status::Failed(e);
                    return;
                }
            },
        };
        let cfg = SshConfig {
            host: f.host.clone(),
            port: f.port,
            user: f.user.clone(),
            auth,
            keepalive_secs: f.keepalive_secs,
            connect_timeout_secs: f.connect_timeout_secs,
        };
        self.status = Status::Connecting;
        {
            let mut sh = self.shared.lock();
            *sh = Shared::default();
            sh.status = Some(Status::Connecting);
        }
        let shared = self.shared.clone();
        let known_hosts = self.known_hosts.clone();
        let waker = services.runtime.waker();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || waker.request_repaint());
        let prompt_shared = shared.clone();
        let prompt_wake = wake.clone();
        let prompt: hs_ssh::PromptSink = Arc::new(move |p: HostKeyPrompt| {
            prompt_shared.lock().hostkey = Some(p);
            prompt_wake();
        });
        let remote_dir = f.remote_dir.clone();
        let start_dir = self.state.remote_path.clone();
        services.runtime.spawn(async move {
            let result = async {
                let client = Arc::new(SshClient::connect(cfg, known_hosts, Some(prompt)).await?);
                let shell = client.open_shell(120, 30).await?;
                let sftp = Arc::new(client.open_sftp().await?);
                let home = sftp.canonicalize(".").await.ok();
                Ok::<_, hs_ssh::SshError>((client, shell, sftp, home))
            }
            .await;
            match result {
                Ok((client, shell, sftp, home)) => {
                    let first = start_dir
                        .filter(|p| !p.is_empty())
                        .or_else(|| (!remote_dir.is_empty()).then(|| remote_dir.clone()))
                        .or_else(|| home.clone())
                        .unwrap_or_else(|| ".".into());
                    {
                        let mut sh = shared.lock();
                        sh.client = Some(client);
                        sh.shell = Some(shell);
                        sh.sftp = Some(sftp.clone());
                        sh.home = home;
                        sh.status = Some(Status::Connected);
                    }
                    wake();
                    list_dir(shared.clone(), sftp, first, wake.clone()).await;
                }
                Err(e) => {
                    shared.lock().status = Some(Status::Failed(e.to_string()));
                    wake();
                }
            }
        });
    }

    fn disconnect(&mut self) {
        if let Some(s) = &self.session {
            s.kill();
        }
        self.session = None;
        let client = self.shared.lock().client.take();
        self.shared.lock().sftp = None;
        self.status = Status::Disconnected;
        if let Some(c) = client {
            tokio::spawn(async move {
                c.disconnect().await;
            });
        }
    }

    fn navigate(&mut self, services: &Services, path: String, from_shell: bool) {
        let (sftp, current) = {
            let sh = self.shared.lock();
            (sh.sftp.clone(), sh.remote.path.clone())
        };
        let Some(sftp) = sftp else { return };
        let path = self.expand_tilde(&path);
        if path == current && !self.shared.lock().remote.entries.is_empty() {
            return;
        }
        if !from_shell
            && self.state.sync_cwd
            && let Some(s) = &self.session
        {
            s.write(format!(" cd {}\n", hs_ssh::shell_quote(&path)).as_bytes());
        }
        let shared = self.shared.clone();
        let waker = services.runtime.waker();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || waker.request_repaint());
        services.runtime.spawn(async move {
            list_dir(shared, sftp, path, wake).await;
        });
    }

    fn expand_tilde(&self, path: &str) -> String {
        if (path == "~" || path.starts_with("~/"))
            && let Some(home) = self.shared.lock().home.clone()
        {
            return format!("{}{}", home.trim_end_matches('/'), &path[1..]);
        }
        path.to_string()
    }

    fn refresh(&mut self, services: &Services) {
        let path = self.shared.lock().remote.path.clone();
        self.shared.lock().remote.entries.clear();
        if !path.is_empty() {
            self.navigate(services, path, true);
        }
    }

    fn join(dir: &str, name: &str) -> String {
        if dir.ends_with('/') {
            format!("{dir}{name}")
        } else {
            format!("{dir}/{name}")
        }
    }

    fn parent(path: &str) -> String {
        let trimmed = path.trim_end_matches('/');
        match trimmed.rfind('/') {
            Some(0) | None => "/".into(),
            Some(i) => trimmed[..i].to_string(),
        }
    }

    fn with_sftp<F, Fut>(&self, services: &Services, title: String, f: F)
    where
        F: FnOnce(Arc<SftpSession>, hs_core::jobs::JobHandle) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        let Some(sftp) = self.shared.lock().sftp.clone() else {
            return;
        };
        services.jobs.spawn(title, "ssh", move |h| f(sftp, h));
    }

    /// After a job touched the listing, reload it and surface a toast.
    fn after_job(
        &self,
        services: &Services,
        ok_message: String,
    ) -> impl Fn(Result<(), String>) + Send + 'static {
        let shared = self.shared.clone();
        let sftp = self.shared.lock().sftp.clone();
        let waker = services.runtime.waker();
        let runtime = services.runtime.clone();
        move |r: Result<(), String>| {
            let wake: Arc<dyn Fn() + Send + Sync> = {
                let w = waker.clone();
                Arc::new(move || w.request_repaint())
            };
            match r {
                Ok(()) => shared
                    .lock()
                    .toasts
                    .push((ToastLevel::Success, ok_message.clone())),
                Err(e) => shared.lock().toasts.push((ToastLevel::Error, e)),
            }
            if let Some(sftp) = sftp.clone() {
                let path = shared.lock().remote.path.clone();
                let shared = shared.clone();
                runtime.spawn(async move {
                    list_dir(shared, sftp, path, wake).await;
                });
            } else {
                wake();
            }
        }
    }

    fn open_in_editor(&mut self, cx: &mut ToolContext, remote: String) {
        let s = cx.services;
        let conn = self.state.conn_id.clone().unwrap_or_default();
        let local = s
            .paths
            .tmp()
            .join("sftp")
            .join(hs_core::storage::slug(&conn))
            .join(remote.trim_start_matches('/'));
        self.shared
            .lock()
            .edits
            .insert(local.clone(), remote.clone());
        let shared = self.shared.clone();
        let waker = s.runtime.waker();
        let ok_msg = s.i18n.t_args("ssh-downloaded", &[("name", &remote)]);
        let local2 = local.clone();
        self.with_sftp(s, format!("Download {remote}"), move |sftp, h| async move {
            let r = async {
                let bytes = download_bytes(&sftp, &remote, &h).await?;
                if let Some(p) = local2.parent() {
                    std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                std::fs::write(&local2, bytes).map_err(|e| e.to_string())
            }
            .await;
            match &r {
                Ok(()) => {
                    // The editor is opened by ui() once the file is on disk.
                    shared.lock().open_in_editor.push(local2.clone());
                    shared
                        .lock()
                        .toasts
                        .push((ToastLevel::Success, ok_msg.clone()));
                }
                Err(e) => shared.lock().toasts.push((ToastLevel::Error, e.clone())),
            }
            waker.request_repaint();
            r
        });
    }

    fn upload_saved(&mut self, services: &Services, local: PathBuf, remote: String) {
        let done = self.after_job(
            services,
            services.i18n.t_args("ssh-uploaded", &[("name", &remote)]),
        );
        self.with_sftp(
            services,
            format!("Upload {remote}"),
            move |sftp, _h| async move {
                let r = async {
                    let bytes = std::fs::read(&local).map_err(|e| e.to_string())?;
                    sftp.write(&remote, &bytes).await.map_err(|e| e.to_string())
                }
                .await;
                done(r.clone());
                r
            },
        );
    }

    fn selected_entry(&self) -> Option<Entry> {
        let sh = self.shared.lock();
        self.table
            .selected
            .and_then(|i| sh.remote.entries.get(i).cloned())
    }

    // ---- UI -----------------------------------------------------------------

    fn hostkey_dialog(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let prompt = self.shared.lock().hostkey.take();
        let Some(p) = prompt else { return };
        let mut decision: Option<bool> = None;
        egui::Modal::new(self.view.id.with("hostkey")).show(ui.ctx(), |ui| {
            ui.set_width(520.0);
            let title = if p.changed.is_some() {
                s.t("ssh-hostkey-changed-title")
            } else {
                s.t("ssh-hostkey-title")
            };
            ui.heading(title);
            ui.add_space(6.0);
            let body = if p.changed.is_some() {
                s.i18n.t_args(
                    "ssh-hostkey-changed-body",
                    &[("host", &p.host), ("fingerprint", &p.fingerprint)],
                )
            } else {
                s.i18n.t_args(
                    "ssh-hostkey-body",
                    &[("host", &p.host), ("fingerprint", &p.fingerprint)],
                )
            };
            ui.add(egui::Label::new(body).wrap());
            ui.add_space(4.0);
            ui.monospace(format!("{} {}", p.algorithm, p.fingerprint));
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let trust_label = if p.changed.is_some() {
                    s.t("ssh-hostkey-retrust")
                } else {
                    s.t("ssh-hostkey-trust")
                };
                let btn =
                    egui::Button::new(egui::RichText::new(trust_label).color(egui::Color32::WHITE))
                        .fill(if p.changed.is_some() {
                            theme.error
                        } else {
                            theme.accent
                        });
                if ui.add(btn).clicked() {
                    decision = Some(true);
                }
                if ui.button(s.t("cancel")).clicked() {
                    decision = Some(false);
                }
            });
        });
        match decision {
            Some(trust) => p.answer(trust, &self.known_hosts),
            None => self.shared.lock().hostkey = Some(p),
        }
    }

    fn prompts(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        // Rename.
        if let Some((old, mut name)) = self.rename.take() {
            let mut done = false;
            let mut cancel = false;
            let m = egui::Modal::new(self.view.id.with("rename")).show(ui.ctx(), |ui| {
                ui.set_width(380.0);
                ui.heading(s.t("ssh-rename"));
                ui.add_space(6.0);
                let r = form::text_field(ui, &mut name, "");
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if form::primary_button(ui, theme, &s.t("ok")).clicked() {
                        done = true;
                    }
                    if ui.button(s.t("cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
            if m.should_close() {
                cancel = true;
            }
            if done && !name.trim().is_empty() {
                let dir = Self::parent(&old);
                let new = Self::join(&dir, name.trim());
                let done = self.after_job(s, s.t("ssh-renamed"));
                self.with_sftp(s, format!("Rename {old}"), move |sftp, _h| async move {
                    let r = sftp.rename(&old, &new).await.map_err(|e| e.to_string());
                    done(r.clone());
                    r
                });
            } else if !done && !cancel {
                self.rename = Some((old, name));
            }
        }
        // New folder.
        if let Some(mut name) = self.new_folder.take() {
            let mut done = false;
            let mut cancel = false;
            let m = egui::Modal::new(self.view.id.with("mkdir")).show(ui.ctx(), |ui| {
                ui.set_width(380.0);
                ui.heading(s.t("ssh-new-folder"));
                ui.add_space(6.0);
                let r = form::text_field(ui, &mut name, &s.t("editor-name-prompt"));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if form::primary_button(ui, theme, &s.t("ok")).clicked() {
                        done = true;
                    }
                    if ui.button(s.t("cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
            if m.should_close() {
                cancel = true;
            }
            if done && !name.trim().is_empty() {
                let dir = self.shared.lock().remote.path.clone();
                let path = Self::join(&dir, name.trim());
                let done = self.after_job(s, s.t("ssh-folder-created"));
                self.with_sftp(s, format!("Create {path}"), move |sftp, _h| async move {
                    let r = sftp.create_dir(&path).await.map_err(|e| e.to_string());
                    done(r.clone());
                    r
                });
            } else if !done && !cancel {
                self.new_folder = Some(name);
            }
        }
        // Delete confirmation.
        if let Some(path) = self.confirm_delete.clone() {
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let host = self.conn_name.clone();
            match hs_ui::dialog::confirm(
                ui.ctx(),
                theme,
                "ssh-delete",
                &s.t("ssh-delete"),
                &s.i18n
                    .t_args("ssh-delete-confirm", &[("name", &name), ("host", &host)]),
                &s.t("ssh-delete"),
                &s.t("cancel"),
                true,
            ) {
                hs_ui::dialog::DialogResult::Ok => {
                    self.confirm_delete = None;
                    let is_dir = self
                        .shared
                        .lock()
                        .remote
                        .entries
                        .iter()
                        .any(|e| e.name == name && e.is_dir);
                    let done = self.after_job(s, s.t("ssh-deleted"));
                    self.with_sftp(s, format!("Delete {path}"), move |sftp, _h| async move {
                        let r = if is_dir {
                            remove_dir_recursive(&sftp, &path).await
                        } else {
                            sftp.remove_file(&path).await.map_err(|e| e.to_string())
                        };
                        done(r.clone());
                        r
                    });
                }
                hs_ui::dialog::DialogResult::Cancel => self.confirm_delete = None,
                hs_ui::dialog::DialogResult::Open => {}
            }
        }
    }

    fn file_pane(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let (path, entries, loading, error) = {
            let sh = self.shared.lock();
            (
                sh.remote.path.clone(),
                sh.remote.entries.clone(),
                sh.remote.loading,
                sh.remote.error.clone(),
            )
        };
        if self.path_edit_synced {
            self.path_edit = path.clone();
        }
        // Toolbar row.
        form::header_bar(ui, theme, |ui| {
            if form::icon_button(ui, theme, ph::ARROW_UP, 16.0, false)
                .tip(s.t("ssh-up"))
                .clicked()
            {
                let p = Self::parent(&path);
                self.navigate(s, p, false);
            }
            if form::icon_button(ui, theme, ph::HOUSE, 16.0, false)
                .tip(s.t("ssh-home"))
                .clicked()
            {
                self.navigate(s, "~".into(), false);
            }
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.path_edit)
                    .margin(form::FIELD_MARGIN)
                    .desired_width(ui.available_width() - 150.0),
            );
            if r.changed() {
                self.path_edit_synced = false;
            }
            if r.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let p = self.path_edit.trim().to_string();
                    self.navigate(s, p, false);
                }
                self.path_edit_synced = true;
            }
            if form::icon_button(ui, theme, ph::ARROW_CLOCKWISE, 16.0, false)
                .tip(s.t("ssh-refresh"))
                .clicked()
            {
                self.refresh(s);
            }
            if form::icon_button(ui, theme, ph::FOLDER_PLUS, 16.0, false)
                .tip(s.t("ssh-new-folder"))
                .clicked()
            {
                self.new_folder = Some(String::new());
            }
            if form::icon_button(ui, theme, ph::UPLOAD_SIMPLE, 16.0, false)
                .tip(s.t("ssh-upload"))
                .clicked()
                && let Some(files) = rfd::FileDialog::new().pick_files()
            {
                for local in files {
                    let name = local
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let remote = Self::join(&path, &name);
                    let done = self.after_job(s, s.i18n.t_args("ssh-uploaded", &[("name", &name)]));
                    self.with_sftp(s, format!("Upload {name}"), move |sftp, h| async move {
                        let r = upload_file(&sftp, &local, &remote, &h).await;
                        done(r.clone());
                        r
                    });
                }
            }
            if form::icon_button(ui, theme, ph::EYE_SLASH, 16.0, self.state.show_hidden)
                .tip(s.t("ssh-show-hidden"))
                .clicked()
            {
                self.state.show_hidden = !self.state.show_hidden;
            }
        });
        if let Some(e) = error {
            ui.colored_label(theme.error, e);
        }
        if loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new(s.t("ssh-loading")).weak());
            });
        }
        // Rows.
        let shown: Vec<(usize, &Entry)> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| self.state.show_hidden || !e.name.starts_with('.'))
            .collect();
        let rows: Vec<Vec<String>> = shown
            .iter()
            .map(|(_, e)| {
                let icon = if e.is_dir {
                    ph::FOLDER
                } else if e.is_link {
                    ph::LINK
                } else {
                    ph::FILE
                };
                vec![
                    format!("{icon} {}", e.name),
                    if e.is_dir {
                        String::new()
                    } else {
                        human_size(e.size)
                    },
                    e.mtime
                        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_default(),
                    e.perms.clone(),
                ]
            })
            .collect();
        let columns = [
            ColumnSpec::new(s.t("ssh-col-name"), 240.0),
            ColumnSpec::new(s.t("ssh-col-size"), 80.0).numeric(),
            ColumnSpec::new(s.t("ssh-col-modified"), 130.0),
            ColumnSpec::new(s.t("ssh-col-perms"), 90.0),
        ];
        let copy_label = s.t("table-copy-row");
        let table = DataTable {
            theme,
            columns: &columns,
            rows: &rows,
            row_height: 24.0,
            show_filter: false,
            copy_row_label: &copy_label,
        };
        let resp = egui::ScrollArea::both()
            .id_salt(self.view.id.with("remote-table"))
            .auto_shrink([false, false])
            .show(ui, |ui| table.show(ui, &mut self.table))
            .inner;
        // Map table row (shown index) back to the entry index.
        let to_entry = |i: usize| shown.get(i).map(|(idx, _)| *idx);
        if let Some(i) = resp.clicked {
            self.table.selected = to_entry(i);
        }
        if let Some(i) = resp.double_clicked
            && let Some(e) = to_entry(i).and_then(|idx| entries.get(idx).cloned())
        {
            let target = Self::join(&path, &e.name);
            if e.is_dir {
                self.navigate(s, target, false);
            } else {
                self.open_in_editor(cx, target);
            }
        }
        // Actions for the selected entry.
        if let Some(e) = self.selected_entry() {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(&e.name).strong());
                let target = Self::join(&path, &e.name);
                if !e.is_dir && ui.button(s.t("ssh-open-in-editor")).clicked() {
                    self.open_in_editor(cx, target.clone());
                }
                if !e.is_dir
                    && ui.button(s.t("ssh-download")).clicked()
                    && let Some(local) = rfd::FileDialog::new().set_file_name(&e.name).save_file()
                {
                    let done =
                        self.after_job(s, s.i18n.t_args("ssh-downloaded", &[("name", &e.name)]));
                    let remote = target.clone();
                    self.with_sftp(
                        s,
                        format!("Download {}", e.name),
                        move |sftp, h| async move {
                            let r = download_file(&sftp, &remote, &local, &h).await;
                            done(r.clone());
                            r
                        },
                    );
                }
                if ui.button(s.t("ssh-rename")).clicked() {
                    self.rename = Some((target.clone(), e.name.clone()));
                }
                if ui
                    .button(egui::RichText::new(s.t("ssh-delete")).color(theme.error))
                    .clicked()
                {
                    self.confirm_delete = Some(target.clone());
                }
                if ui.button(s.t("editor-copy-path")).clicked() {
                    ui.ctx().copy_text(target.clone());
                }
                if e.is_dir
                    && ui.button(s.t("editor-open-terminal-here")).clicked()
                    && let Some(sess) = &self.session
                {
                    sess.write(format!(" cd {}\n", hs_ssh::shell_quote(&target)).as_bytes());
                }
            });
        }
    }
}

impl Tool for SshTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        if self.conn_name.is_empty() {
            Cow::Borrowed("SSH")
        } else {
            Cow::Owned(self.conn_name.clone())
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::by_name(&s.settings.read().appearance.theme);
        let (dot, label) = match &self.status {
            Status::Idle => (theme.text_muted, s.t("ssh-idle")),
            Status::Connecting => (theme.warning, s.t("ssh-connecting")),
            Status::Connected => (theme.success, s.t("ssh-connected")),
            Status::Failed(_) => (theme.error, s.t("ssh-failed")),
            Status::Disconnected => (theme.text_muted, s.t("ssh-disconnected")),
        };
        ui.colored_label(dot, "●");
        ui.label(label);
        if !self.conn_name.is_empty() {
            ui.label(egui::RichText::new(&self.conn_name).strong());
        }
        ui.separator();
        match self.status {
            Status::Connected | Status::Connecting => {
                if ui.button(s.t("ssh-disconnect")).clicked() {
                    self.disconnect();
                }
            }
            _ => {
                if self.state.conn_id.is_some() && ui.button(s.t("ssh-connect")).clicked() {
                    self.connect(s);
                }
            }
        }
        if ui.button(s.t("terminal-clear")).clicked()
            && let Some(sess) = &self.session
        {
            sess.write(b"\x1b[3J\x1b[2J\x1b[H");
        }
        ui.toggle_value(&mut self.state.sync_cwd, s.t("ssh-sync-cwd"))
            .tip(s.t("ssh-sync-cwd-hint"));
        if let Some(sess) = &self.session {
            let (c, r) = sess.size();
            ui.label(egui::RichText::new(format!("{c}×{r}")).weak());
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::by_name(&s.settings.read().appearance.theme);
        form::header_bar(ui, &theme, |ui| {
            ui.label(egui::RichText::new(s.t("ssh-hosts")).strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::icon_button(ui, &theme, ph::PLUS, 16.0, false)
                    .tip(s.t("ssh-new-host"))
                    .clicked()
                {
                    cx.actions.push(hs_core::tool::Action::RunCommand {
                        id: "settings:connections".into(),
                    });
                }
            });
        });
        let hosts: Vec<(String, String, String)> = s
            .connections
            .read()
            .of_kind("ssh")
            .map(|c| {
                (
                    c.id.clone(),
                    c.name.clone(),
                    SshFields::from_table(&c.fields).label(),
                )
            })
            .collect();
        if hosts.is_empty() {
            form::note(ui, &theme, ph::LIGHTBULB, &s.t("ssh-no-hosts"));
            return true;
        }
        for (id, name, label) in hosts {
            let selected = self.state.conn_id.as_deref() == Some(&id);
            if form::list_row(
                ui,
                &theme,
                selected,
                ph::COMPUTER_TOWER,
                &name,
                Some(&label),
            )
            .tip(s.t("ssh-open-host"))
            .clicked()
            {
                if self.state.conn_id.is_none() {
                    self.state.conn_id = Some(id);
                    self.connect(s);
                } else if !selected {
                    cx.open("ssh", Some(OpenRequest::Connection { id }));
                }
            }
            ui.add_space(2.0);
        }
        ui.add_space(10.0);
        form::note(ui, &theme, ph::LIGHTBULB, &s.t("ssh-hint"));
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::by_name(&s.settings.read().appearance.theme);
        // Pull async results.
        {
            let mut sh = self.shared.lock();
            if let Some(st) = sh.status.take() {
                self.status = st;
            }
            for (level, text) in sh.toasts.drain(..) {
                cx.toast(level, text);
            }
            for local in sh.open_in_editor.drain(..) {
                cx.open(
                    "editor",
                    Some(OpenRequest::File {
                        uri: local.to_string_lossy().to_string(),
                        line: None,
                    }),
                );
            }
            if let Some((transport, reader)) = sh.shell.take() {
                let waker = s.runtime.waker();
                let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || waker.request_repaint());
                let scrollback = s.settings.read().terminal.scrollback;
                let profile = ShellProfile::new(
                    &format!("ssh:{}", self.state.conn_id.clone().unwrap_or_default()),
                    &self.conn_name,
                    "",
                    "ssh",
                    &[],
                );
                match Session::with_transport(
                    profile,
                    Box::new(transport),
                    reader,
                    120,
                    30,
                    scrollback,
                    wake,
                ) {
                    Ok(sess) => self.session = Some(sess),
                    Err(e) => self.status = Status::Failed(e),
                }
            }
            if !sh.remote.path.is_empty()
                && self.state.remote_path.as_deref() != Some(&sh.remote.path)
            {
                self.state.remote_path = Some(sh.remote.path.clone());
            }
        }
        // Startup command once the shell is up.
        if !self.startup_sent
            && let Some(sess) = &self.session
            && let Some((_, f)) = self.fields(s)
        {
            self.startup_sent = true;
            if !f.startup_command.trim().is_empty() {
                sess.write(format!("{}\n", f.startup_command.trim()).as_bytes());
            }
        }
        // Shell events: titles drive the file pane.
        let mut cwd_from_shell: Option<String> = None;
        if let Some(sess) = &mut self.session {
            for ev in sess.drain_events() {
                match ev {
                    TermEvent::Title(t) => {
                        if let Some(p) = hs_ssh::cwd_from_title(&t) {
                            cwd_from_shell = Some(p);
                        }
                    }
                    TermEvent::ClipboardStore(text) => ui.ctx().copy_text(text),
                    TermEvent::Exit => {
                        if self.status == Status::Connected {
                            self.status = Status::Disconnected;
                        }
                    }
                    TermEvent::Bell => {}
                }
            }
        }
        if self.state.sync_cwd
            && let Some(p) = cwd_from_shell
        {
            self.navigate(s, p, true);
        }
        self.hostkey_dialog(ui, &theme, s);
        self.prompts(ui, &theme, cx);

        // Layout: file pane left, shell right, both resizable.
        let total = ui.available_width();
        egui::Panel::left(self.view.id.with("files"))
            .default_size((total * 0.45).round())
            .size_range(260.0..=(total - 300.0).max(300.0))
            .resizable(true)
            .show(ui, |ui| {
                if self.shared.lock().sftp.is_some() {
                    self.file_pane(ui, &theme, cx);
                } else {
                    ui.add_space(12.0);
                    form::note(ui, &theme, ph::INFO, &s.t("ssh-no-sftp-yet"));
                }
            });
        let term_theme = TermTheme::for_theme(&theme);
        match (&mut self.session, &self.status) {
            (Some(sess), _) => {
                self.view.show(ui, sess, &term_theme);
            }
            (None, Status::Connecting) => {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.label(
                        s.i18n
                            .t_args("ssh-connecting-to", &[("host", &self.conn_name)]),
                    );
                });
            }
            (None, Status::Failed(e)) => {
                let e = e.clone();
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.colored_label(theme.error, e);
                    ui.add_space(8.0);
                    if form::primary_button(ui, &theme, &s.t("ssh-connect")).clicked() {
                        self.connect(s);
                    }
                });
            }
            (None, _) => {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    if self.state.conn_id.is_none() {
                        ui.label(egui::RichText::new(s.t("ssh-pick-host")).weak());
                    } else if form::primary_button(ui, &theme, &s.t("ssh-connect")).clicked() {
                        self.connect(s);
                    }
                });
            }
        }
        if self.status == Status::Connecting || self.shared.lock().remote.loading {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        let c = |id: &str, title: &str| CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: None,
        };
        vec![
            c("ssh.connect", "SSH: Connect"),
            c("ssh.disconnect", "SSH: Disconnect"),
            c("ssh.refresh", "SSH: Refresh Files"),
            c("ssh.sync", "SSH: Toggle Folder Sync With Shell"),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "ssh.connect" => self.connect(cx.services),
            "ssh.disconnect" => self.disconnect(),
            "ssh.refresh" => self.refresh(cx.services),
            "ssh.sync" => self.state.sync_cwd = !self.state.sync_cwd,
            _ => return false,
        }
        true
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or_default()
    }

    fn current_uri(&self) -> Option<String> {
        self.state.conn_id.as_ref().map(|id| format!("ssh://{id}"))
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Connection { id } => {
                if self.state.conn_id.as_deref() == Some(&id) {
                    if !matches!(self.status, Status::Connected | Status::Connecting) {
                        self.connect(cx.services);
                    }
                } else if self.state.conn_id.is_none() {
                    self.state.conn_id = Some(id);
                    self.connect(cx.services);
                } else {
                    cx.open("ssh", Some(OpenRequest::Connection { id }));
                }
                true
            }
            OpenRequest::Terminal {
                host: Some(host),
                cwd,
                ..
            } if self.state.conn_id.as_deref() == Some(&host) => {
                // "open terminal here" on a remote host: same tab if it is ours.
                if let (Some(d), Some(sess)) = (cwd, &self.session) {
                    sess.write(format!(" cd {}\n", hs_ssh::shell_quote(&d)).as_bytes());
                }
                true
            }
            _ => false,
        }
    }

    fn on_event(&mut self, ev: &Event, cx: &mut ToolContext) {
        match ev {
            Event::FileSaved { uri } => {
                let local = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
                let remote = self.shared.lock().edits.get(&local).cloned();
                if let Some(remote) = remote {
                    self.upload_saved(cx.services, local, remote);
                }
            }
            Event::SettingsChanged => {
                let st = cx.services.settings.read();
                self.view.font_size = st.terminal.font_size;
                self.view.copy_on_select = st.terminal.copy_on_select;
                self.view.right_click_copy_paste = st.terminal.right_click_copy_paste;
            }
            _ => {}
        }
    }

    fn on_close(&mut self) -> hs_core::tool::CloseDecision {
        self.disconnect();
        hs_core::tool::CloseDecision::Close
    }
}

// ---- async helpers ----------------------------------------------------------

async fn list_dir(
    shared: Arc<Mutex<Shared>>,
    sftp: Arc<SftpSession>,
    path: String,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let generation = {
        let mut sh = shared.lock();
        sh.remote.loading = true;
        sh.remote.error = None;
        sh.remote.generation += 1;
        sh.remote.generation
    };
    wake();
    let result = async {
        let canonical = sftp.canonicalize(&path).await.map_err(|e| e.to_string())?;
        let rd = sftp.read_dir(&canonical).await.map_err(|e| e.to_string())?;
        let mut entries: Vec<Entry> = rd
            .map(|d| {
                let md = d.metadata();
                let name = d.file_name();
                Entry {
                    is_dir: md.is_dir(),
                    is_link: md.is_symlink(),
                    size: md.size.unwrap_or(0),
                    mtime: md.mtime.map(|t| t as i64),
                    perms: md
                        .permissions
                        .map(|p| format_perms(p & 0o7777, md.is_dir()))
                        .unwrap_or_default(),
                    name,
                }
            })
            .filter(|e| e.name != "." && e.name != "..")
            .collect();
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok::<_, String>((canonical, entries))
    }
    .await;
    let mut sh = shared.lock();
    if sh.remote.generation != generation {
        return;
    }
    sh.remote.loading = false;
    match result {
        Ok((canonical, entries)) => {
            sh.remote.path = canonical;
            sh.remote.entries = entries;
        }
        Err(e) => sh.remote.error = Some(e),
    }
    drop(sh);
    wake();
}

async fn remove_dir_recursive(sftp: &SftpSession, path: &str) -> Result<(), String> {
    let rd = sftp.read_dir(path).await.map_err(|e| e.to_string())?;
    for d in rd {
        let name = d.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let child = format!("{}/{}", path.trim_end_matches('/'), name);
        if d.metadata().is_dir() {
            Box::pin(remove_dir_recursive(sftp, &child)).await?;
        } else {
            sftp.remove_file(&child).await.map_err(|e| e.to_string())?;
        }
    }
    sftp.remove_dir(path).await.map_err(|e| e.to_string())
}

async fn upload_file(
    sftp: &SftpSession,
    local: &std::path::Path,
    remote: &str,
    h: &hs_core::jobs::JobHandle,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let bytes = tokio::fs::read(local).await.map_err(|e| e.to_string())?;
    let total = bytes.len() as u64;
    let mut file = sftp.create(remote).await.map_err(|e| e.to_string())?;
    let mut done = 0u64;
    for chunk in bytes.chunks(256 * 1024) {
        if h.is_cancelled() {
            return Err("cancelled".into());
        }
        file.write_all(chunk).await.map_err(|e| e.to_string())?;
        done += chunk.len() as u64;
        h.set_progress(hs_core::jobs::Progress::Bytes {
            done,
            total: Some(total),
        });
    }
    file.shutdown().await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn download_bytes(
    sftp: &SftpSession,
    remote: &str,
    h: &hs_core::jobs::JobHandle,
) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    let total = sftp.metadata(remote).await.ok().and_then(|m| m.size);
    let mut file = sftp.open(remote).await.map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        if h.is_cancelled() {
            return Err("cancelled".into());
        }
        let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        h.set_progress(hs_core::jobs::Progress::Bytes {
            done: out.len() as u64,
            total,
        });
    }
    Ok(out)
}

async fn download_file(
    sftp: &SftpSession,
    remote: &str,
    local: &std::path::Path,
    h: &hs_core::jobs::JobHandle,
) -> Result<(), String> {
    use tokio::io::AsyncReadExt;
    let total = sftp.metadata(remote).await.ok().and_then(|m| m.size);
    let mut file = sftp.open(remote).await.map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        if h.is_cancelled() {
            return Err("cancelled".into());
        }
        let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        h.set_progress(hs_core::jobs::Progress::Bytes {
            done: out.len() as u64,
            total,
        });
    }
    if let Some(p) = local.parent() {
        tokio::fs::create_dir_all(p)
            .await
            .map_err(|e| e.to_string())?;
    }
    tokio::fs::write(local, out)
        .await
        .map_err(|e| e.to_string())
}

fn format_perms(mode: u32, is_dir: bool) -> String {
    let mut s = String::with_capacity(10);
    s.push(if is_dir { 'd' } else { '-' });
    for shift in [6u32, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
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

/// `~/x` → the user's home on this machine (for key paths).
fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    {
        return std::path::Path::new(&home)
            .join(rest)
            .to_string_lossy()
            .to_string();
    }
    p.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_perms_and_sizes() {
        assert_eq!(SshTool::parent("/var/www/html"), "/var/www");
        assert_eq!(SshTool::parent("/var"), "/");
        assert_eq!(SshTool::parent("/"), "/");
        assert_eq!(SshTool::join("/var/www", "a"), "/var/www/a");
        assert_eq!(SshTool::join("/", "a"), "/a");
        assert_eq!(format_perms(0o755, true), "drwxr-xr-x");
        assert_eq!(format_perms(0o644, false), "-rw-r--r--");
        assert_eq!(human_size(2048), "2.0 KB");
    }
}
