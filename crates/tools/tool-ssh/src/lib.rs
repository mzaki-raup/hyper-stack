//! SSH tool (docs/tools/ssh-sftp.md): one host per tab with a dual pane —
//! the remote file browser (SFTP) on the left and the shell on the right.
//! The two stay in sync: the shell's working directory (from its window
//! title) drives the browser, and navigating the browser sends `cd` to the
//! shell. Hosts come from `kind = "ssh"` connection profiles.

mod perms;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hs_core::Services;
use hs_core::events::Event;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, ToastLevel, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use hs_core::tunnels::{ForwardSpec, TunnelError, TunnelInfo, TunnelState};
use hs_ssh::config::{ConfigError, config_from_fields};
use hs_ssh::{HostKeyPrompt, KnownHosts, SftpSession, SshClient, SshFields};
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
    /// The numeric mode, for the permissions dialog. `perms` is the same
    /// thing written for the eye; this is the one that can be edited.
    mode: u32,
    /// Owner and group as the server named them. SFTP v3 only promises the
    /// numeric ids, so these can be empty and the dialog shows blanks
    /// rather than inventing a name.
    owner: String,
    group: String,
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
    runtime: hs_core::runtime::AppRuntime,
    known_hosts: Arc<KnownHosts>,
    session: Option<Session>,
    view: TerminalView,
    table: TableState,
    path_edit: String,
    path_edit_synced: bool,
    rename: Option<(String, String)>,
    new_folder: Option<String>,
    new_file: Option<String>,
    perms: Option<PermsEdit>,
    /// Right-clicked entry (index into the listing) and the pointer, so the
    /// menu opens where the click was.
    entry_menu: Option<(usize, egui::Pos2)>,
    confirm_delete: Option<String>,
    startup_sent: bool,
    conn_name: String,
    /// The "New Tunnel" form in the sidebar, while open.
    tunnel_form: Option<TunnelForm>,
    /// The last connect failed only because the vault was locked: retry as
    /// soon as it is unlocked (a tab restored at start-up hits this).
    waiting_for_vault: bool,
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

/// `ssh -L` as a form: local port (empty = any free one) → remote host:port
/// through an SSH profile.
#[derive(Default)]
struct TunnelForm {
    ssh_id: String,
    remote_host: String,
    remote_port: String,
    local_port: String,
    error: Option<String>,
}

impl SshTool {
    /// The home tab: what this activity has open, with the last picture of
    /// each pane. Owner's requirement, 2026-10-17.
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = hs_ui::theme::Theme::from_settings(&s.settings.read().appearance);
        let count = s.connections.read().of_kind("ssh").count();
        let tabs = s.open_tabs.of("ssh");
        hs_ui::form::page_title(
            ui,
            &theme,
            &s.t("tool-ssh"),
            Some(&s.t("ssh-dashboard-sub")),
        );
        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(ui, &theme, &count.to_string(), &s.t("dash-connections"));
            hs_ui::dashboard::stat(ui, &theme, &tabs.len().to_string(), &s.t("dash-open-tabs"));
        });
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        egui::ScrollArea::vertical()
            .id_salt("ssh-dashboard")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(id) = hs_ui::dashboard::open_tabs(ui, &theme, &tabs, &labels) {
                    cx.actions
                        .push(hs_core::tool::Action::FocusTab { tab_id: id });
                }
            });
    }

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
            runtime: services.runtime.clone(),
            known_hosts,
            session: None,
            view,
            table: TableState::default(),
            path_edit: String::new(),
            path_edit_synced: true,
            rename: None,
            new_folder: None,
            new_file: None,
            perms: None,
            entry_menu: None,
            confirm_delete: None,
            startup_sent: false,
            conn_name: String::new(),
            tunnel_form: None,
            waiting_for_vault: false,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
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

    fn connect(&mut self, services: &Services) {
        let Some((name, f)) = self.fields(services) else {
            self.status = Status::Failed(services.t("ssh-no-profile"));
            return;
        };
        self.conn_name = name;
        self.session = None;
        self.startup_sent = false;
        let cfg = match config_from_fields(&f, services.secrets.as_ref()) {
            Ok(c) => c,
            Err(e) => {
                self.waiting_for_vault = e == ConfigError::VaultLocked;
                self.status = Status::Failed(match e {
                    ConfigError::VaultLocked => services.t("ssh-vault-locked"),
                    ConfigError::NoPassword => services.t("ssh-no-password"),
                    ConfigError::Secret(e) => e,
                });
                return;
            }
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
            self.runtime.spawn(async move {
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

    /// Show an SFTP action in the shell pane, so the two halves of the tab
    /// tell the same story: do it in the file pane, read the command for it
    /// in the shell. It goes in as a **comment**, not a command — the action
    /// has already happened over SFTP and running it a second time would be
    /// wrong (deleting what was just deleted, say). `cd` is the exception and
    /// is sent for real by `navigate`, because the shell's own working
    /// directory really does have to move.
    fn echo_sftp(&self, command: String) {
        if !self.state.sync_cwd {
            return;
        }
        if let Some(sess) = &self.session {
            sess.write(format!(" # {command}\n").as_bytes());
        }
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
                self.echo_sftp(format!(
                    "mv {} {}",
                    hs_ssh::shell_quote(&old),
                    hs_ssh::shell_quote(&new)
                ));
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
                self.echo_sftp(format!("mkdir {}", hs_ssh::shell_quote(&path)));
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
        // New file: an empty file, named with its extension by the user.
        if let Some(mut name) = self.new_file.take() {
            let mut done = false;
            let mut cancel = false;
            let m = egui::Modal::new(self.view.id.with("touch")).show(ui.ctx(), |ui| {
                ui.set_width(380.0);
                ui.heading(s.t("ssh-new-file"));
                ui.add_space(6.0);
                let r = form::text_field(ui, &mut name, &s.t("ssh-new-file-hint"));
                r.request_focus();
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
                self.echo_sftp(format!("touch {}", hs_ssh::shell_quote(&path)));
                let done = self.after_job(
                    s,
                    s.i18n.t_args("ssh-file-created", &[("name", name.trim())]),
                );
                self.with_sftp(s, format!("Create {path}"), move |sftp, _h| async move {
                    // Refuse to flatten something that is already there.
                    // create(), not write(): write() opens with WRITE alone
                    // and the server answers "no such file" for a name that
                    // is not there yet, which is the whole point here.
                    let r = match sftp.try_exists(path.clone()).await {
                        Ok(true) => {
                            Err("a file or folder of that name is already here".to_string())
                        }
                        Ok(false) | Err(_) => match sftp.create(path.clone()).await {
                            Ok(file) => file.close().await.map_err(|e| e.to_string()),
                            Err(e) => Err(e.to_string()),
                        },
                    };
                    done(r.clone());
                    r
                });
            } else if !done && !cancel {
                self.new_file = Some(name);
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
                    self.echo_sftp(format!(
                        "rm {}{}",
                        if is_dir { "-r " } else { "" },
                        hs_ssh::shell_quote(&path)
                    ));
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
            if form::icon_button(ui, theme, ph::FILE_PLUS, 16.0, false)
                .tip(s.t("ssh-new-file"))
                .clicked()
            {
                self.new_file = Some(String::new());
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
            // Paste whatever the file manager put on the clipboard into the
            // folder on screen.
            let has_files = hs_core::clipboard_files::has_files();
            if form::icon_button(ui, theme, ph::CLIPBOARD_TEXT, 16.0, false)
                .tip(s.t(if has_files {
                    "ssh-paste"
                } else {
                    "ssh-paste-empty"
                }))
                .clicked()
            {
                self.paste_from_clipboard(s);
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
                // The name carries no glyph of its own: the table draws the
                // file-type icon beside it (row_icons below). A link keeps
                // its arrow, which no icon theme shows.
                let name = if e.is_link {
                    format!("{} {}", e.name, ph::ARROW_BEND_UP_RIGHT)
                } else {
                    e.name.clone()
                };
                vec![
                    name,
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
        // The same Material icon set the editor's project tree uses, so a
        // remote listing reads like a local one.
        let icons: Vec<egui::ImageSource<'static>> = shown
            .iter()
            .map(|(_, e): &(usize, &Entry)| {
                if e.is_dir {
                    hs_ui::fileicons::for_folder(&e.name)
                } else {
                    hs_ui::fileicons::for_file(&e.name)
                }
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
            // The pane has a menu of its own, below.
            built_in_menu: false,
            row_icons: Some(&icons),
        };
        let resp = table.show(ui, &mut self.table);
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
        if let Some(i) = resp.secondary {
            self.table.selected = to_entry(i);
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                self.entry_menu = to_entry(i).map(|e| (e, pos));
            }
        }
        // Double-clicking the empty space below the listing goes up a level,
        // the way a file manager does.
        let below = ui.available_rect_before_wrap();
        if below.height() > 2.0 {
            let r = ui.interact(below, ui.id().with("sftp-empty"), egui::Sense::click());
            if r.double_clicked() {
                let p = Self::parent(&path);
                self.navigate(s, p, false);
            }
            if r.secondary_clicked() {
                self.table.selected = None;
                self.entry_menu = None;
            }
        }
    }
}

impl SshTool {
    /// View and change a remote entry's permissions, owner and group.
    ///
    /// The mode goes over SFTP, which is the one part every server allows
    /// without a shell. Owner and group do not: SFTP wants numeric ids and
    /// the change usually needs root, so those are sent as a `chown` in the
    /// shell, where the user sees the command and its error if it fails.
    fn perms_dialog(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let Some(mut e) = self.perms.take() else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let m = egui::Modal::new(self.view.id.with("perms")).show(ui.ctx(), |ui| {
            ui.set_width(430.0);
            ui.heading(s.t("ssh-perms-title"));
            ui.add_space(2.0);
            ui.label(egui::RichText::new(&e.name).color(theme.text_muted).small());
            ui.add_space(10.0);
            form::field_grid(ui, "ssh-perms-grid", |ui| {
                form::label(ui, &s.t("ssh-perms-owner"));
                form::text_field(ui, &mut e.owner, &s.t("ssh-perms-owner"));
                ui.end_row();
                form::label(ui, &s.t("ssh-perms-group"));
                form::text_field(ui, &mut e.group, &s.t("ssh-perms-group"));
                ui.end_row();
                form::label(ui, &s.t("ssh-perms-mode"));
                // Typing a number ticks the boxes; anything that is not a
                // mode leaves them alone rather than guessing.
                if form::text_field(ui, &mut e.typed, "644").changed()
                    && let Some(m) = perms::parse_octal(&e.typed)
                {
                    e.mode = m;
                }
                ui.end_row();
            });
            ui.add_space(8.0);
            egui::Grid::new("ssh-perms-bits")
                .num_columns(4)
                .spacing([18.0, 6.0])
                .show(ui, |ui| {
                    ui.label("");
                    for key in ["ssh-perms-read", "ssh-perms-write", "ssh-perms-execute"] {
                        ui.label(
                            egui::RichText::new(s.t(key))
                                .small()
                                .color(theme.text_muted),
                        );
                    }
                    ui.end_row();
                    for (who, key) in perms::Who::ALL.into_iter().zip([
                        "ssh-perms-user",
                        "ssh-perms-group-row",
                        "ssh-perms-other",
                    ]) {
                        ui.label(s.t(key));
                        for bit in perms::Bit::ALL {
                            let mut on = perms::has(e.mode, who, bit);
                            if ui.checkbox(&mut on, "").changed() {
                                e.mode = perms::with(e.mode, who, bit, on);
                                e.typed = perms::to_octal(e.mode);
                            }
                        }
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!(
                    "{}  {}",
                    perms::to_octal(e.mode),
                    perms::to_symbolic(e.mode)
                ))
                .monospace()
                .color(theme.text_muted),
            );
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::primary_button(ui, theme, &s.t("save")).clicked() {
                    apply = true;
                }
                if ui.button(s.t("cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if m.should_close() {
            cancel = true;
        }
        if cancel {
            return;
        }
        if !apply {
            self.perms = Some(e);
            return;
        }
        let (was_mode, was_owner, was_group) = e.was.clone();
        if e.mode != was_mode {
            let path = e.path.clone();
            let mode = e.mode;
            self.echo_sftp(format!(
                "chmod {} {}",
                perms::to_octal(mode),
                hs_ssh::shell_quote(&path)
            ));
            let done = self.after_job(
                s,
                s.i18n.t_args(
                    "ssh-perms-changed",
                    &[("name", &e.name), ("mode", &perms::to_octal(mode))],
                ),
            );
            self.with_sftp(
                s,
                format!("chmod {} {path}", perms::to_octal(mode)),
                move |sftp, _h| async move {
                    let attrs = hs_ssh::FileAttributes {
                        permissions: Some(mode),
                        ..Default::default()
                    };
                    let r = sftp
                        .set_metadata(path, attrs)
                        .await
                        .map_err(|e| e.to_string());
                    done(r.clone());
                    r
                },
            );
        }
        // Owner and group go through the shell, so the command and anything
        // it says are in front of the user.
        if (e.owner != was_owner || e.group != was_group)
            && let Some(sess) = &self.session
        {
            let who = if e.group.trim().is_empty() {
                e.owner.trim().to_string()
            } else {
                format!("{}:{}", e.owner.trim(), e.group.trim())
            };
            if !who.is_empty() && who != ":" {
                sess.write(format!(" chown {} {}\n", who, hs_ssh::shell_quote(&e.path)).as_bytes());
            }
        }
        let _ = e.is_dir;
    }

    /// The right-click menu for a file or folder. Everything the pane can
    /// do to one entry lives here; the pane itself has no action bar.
    fn entry_menu(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let Some((idx, pos)) = self.entry_menu else {
            return;
        };
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let (dir, entry) = {
            let sh = self.shared.lock();
            (sh.remote.path.clone(), sh.remote.entries.get(idx).cloned())
        };
        let Some(e) = entry else {
            self.entry_menu = None;
            return;
        };
        let target = Self::join(&dir, &e.name);
        let mut close = false;
        let area = egui::Area::new(self.view.id.with("sftp-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(190.0);
                    if !e.is_dir && ui.button(s.t("ssh-open-in-editor")).clicked() {
                        self.open_in_editor(cx, target.clone());
                        close = true;
                    }
                    if ui.button(s.t("ssh-rename")).clicked() {
                        self.rename = Some((target.clone(), e.name.clone()));
                        close = true;
                    }
                    ui.separator();
                    if !e.is_dir && ui.button(s.t("ssh-download")).clicked() {
                        if let Some(local) =
                            rfd::FileDialog::new().set_file_name(&e.name).save_file()
                        {
                            let done = self.after_job(
                                s,
                                s.i18n.t_args("ssh-downloaded", &[("name", &e.name)]),
                            );
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
                        close = true;
                    }
                    // A folder, or a file someone wants compressed.
                    ui.menu_button(s.t("ssh-download-zipped"), |ui| {
                        for format in hs_core::archive::Format::ALL {
                            if ui.button(format.extension().to_uppercase()).clicked() {
                                self.download_zipped(s, target.clone(), e.name.clone(), format);
                                ui.close();
                                close = true;
                            }
                        }
                    });
                    ui.separator();
                    if ui
                        .button(s.t("ssh-copy-file"))
                        .on_hover_text(s.t("ssh-copy-file-tip"))
                        .clicked()
                    {
                        self.copy_to_clipboard(s, target.clone(), e.name.clone());
                        close = true;
                    }
                    if ui.button(s.t("editor-copy-path")).clicked() {
                        ui.ctx().copy_text(target.clone());
                        close = true;
                    }
                    ui.separator();
                    if ui
                        .button(egui::RichText::new(s.t("ssh-delete")).color(theme.error))
                        .clicked()
                    {
                        self.confirm_delete = Some(target.clone());
                        close = true;
                    }
                    ui.separator();
                    if ui.button(s.t("ssh-perms")).clicked() {
                        self.perms = Some(PermsEdit {
                            path: target.clone(),
                            name: e.name.clone(),
                            is_dir: e.is_dir,
                            mode: perms::permission_bits(e.mode),
                            typed: perms::to_octal(e.mode),
                            owner: e.owner.clone(),
                            group: e.group.clone(),
                            was: (
                                perms::permission_bits(e.mode),
                                e.owner.clone(),
                                e.group.clone(),
                            ),
                        });
                        close = true;
                    }
                    if e.is_dir {
                        ui.separator();
                        if ui.button(s.t("editor-open-terminal-here")).clicked() {
                            if let Some(sess) = &self.session {
                                sess.write(
                                    format!(" cd {}\n", hs_ssh::shell_quote(&target)).as_bytes(),
                                );
                            }
                            close = true;
                        }
                    }
                });
            });
        // A click anywhere else puts the menu away. The menu's own rect is
        // tested here rather than trusting clicked_elsewhere, which reports
        // true for clicks on the area's own buttons.
        let clicked_out = ui.input(|i| i.pointer.any_click())
            && ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| !area.response.rect.contains(p));
        if close || clicked_out || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.entry_menu = None;
        }
    }

    /// Fetch a remote file or folder and pack it into one archive the user
    /// picks a place for. The fetch is a job like any other transfer; the
    /// packing happens on a worker so a large tree does not stall the UI.
    fn download_zipped(
        &mut self,
        s: &Services,
        remote: String,
        name: String,
        format: hs_core::archive::Format,
    ) {
        let Some(target) = rfd::FileDialog::new()
            .set_file_name(format.file_name(&name))
            .save_file()
        else {
            return;
        };
        let staging = s
            .paths
            .tmp()
            .join(format!("sftp-zip-{}", hs_core::new_id()));
        if let Err(e) = std::fs::create_dir_all(&staging) {
            self.shared.lock().remote.error = Some(e.to_string());
            return;
        }
        let done = self.after_job(s, s.i18n.t_args("ssh-zipped", &[("name", &name)]));
        let fetched = staging.join(&name);
        self.with_sftp(s, format!("Download {name}"), move |sftp, h| async move {
            let r = download_tree(&sftp, &remote, &staging, &h)
                .await
                .and_then(|_| {
                    // A folder packs as itself; a single file packs as the
                    // folder it was fetched into, under its own name.
                    let dir = if fetched.is_dir() { &fetched } else { &staging };
                    hs_core::archive::pack(dir, &target, format).map(|n| n as u64)
                });
            let _ = std::fs::remove_dir_all(&staging);
            done(r.clone().map(|_| ()));
            r.map(|_| ())
        });
    }

    /// Copy the entry the table has selected, if any.
    fn copy_selected(&mut self, s: &Services) {
        let Some(e) = self.selected_entry() else {
            return;
        };
        let path = self.shared.lock().remote.path.clone();
        let target = Self::join(&path, &e.name);
        self.copy_to_clipboard(s, target, e.name.clone());
    }

    /// Copy a remote file or folder to the system clipboard: it is fetched
    /// into a folder of its own under the data directory first, because the
    /// clipboard can only carry paths that exist.
    fn copy_to_clipboard(&mut self, s: &Services, remote: String, name: String) {
        // Under the data folder's scratch space, which is cleared on start
        // (ADR-017): nothing is written outside the portable folder.
        let dir = s
            .paths
            .tmp()
            .join(format!("sftp-copy-{}", hs_core::new_id()));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.shared.lock().remote.error = Some(e.to_string());
            return;
        }
        let done = self.after_job(s, s.i18n.t_args("ssh-copied", &[("name", &name)]));
        let target = dir.join(&name);
        self.with_sftp(s, format!("Copy {name}"), move |sftp, h| async move {
            let r = download_tree(&sftp, &remote, &dir, &h)
                .await
                .and_then(|_| hs_core::clipboard_files::write(&[target]));
            done(r.clone());
            r
        });
    }

    /// Upload whatever the clipboard holds into the folder on screen.
    fn paste_from_clipboard(&mut self, s: &Services) {
        let paths = hs_core::clipboard_files::read();
        if paths.is_empty() {
            self.shared.lock().remote.error = Some(s.t("ssh-paste-empty"));
            return;
        }
        let remote = self.shared.lock().remote.path.clone();
        let count = paths.len();
        let done = self.after_job(s, s.i18n.t_args("ssh-pasted", &[("n", &count.to_string())]));
        self.with_sftp(
            s,
            format!("Upload {count} items"),
            move |sftp, h| async move {
                let mut r = Ok(());
                for p in &paths {
                    if let Err(e) = upload_tree(&sftp, p, &remote, &h).await {
                        r = Err(e);
                        break;
                    }
                }
                done(r.clone());
                r
            },
        );
    }

    fn new_tunnel_form(&mut self, services: &Services) {
        let ssh_id = self
            .state
            .conn_id
            .clone()
            .or_else(|| {
                services
                    .connections
                    .read()
                    .of_kind("ssh")
                    .next()
                    .map(|c| c.id.clone())
            })
            .unwrap_or_default();
        self.tunnel_form = Some(TunnelForm {
            ssh_id,
            remote_host: "localhost".into(),
            ..Default::default()
        });
    }

    /// Port forwards (docs/tools/ssh-sftp.md §8): the list from the
    /// `Tunnels` service, each with its traffic and a stop button, and the
    /// "New Tunnel" form.
    fn tunnels_section(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        form::header_bar(ui, theme, |ui| {
            ui.label(egui::RichText::new(s.t("ssh-tunnels")).strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::icon_button(ui, theme, ph::PLUS, 16.0, false)
                    .tip(s.t("ssh-tunnel-new"))
                    .clicked()
                {
                    self.new_tunnel_form(s);
                }
            });
        });
        let tunnels = s.tunnels.list();
        if !tunnels.is_empty() {
            // Byte counters move without other UI events.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(1));
        }
        for t in &tunnels {
            self.tunnel_row(ui, cx, theme, t);
            ui.add_space(2.0);
        }
        if tunnels.is_empty() && self.tunnel_form.is_none() {
            ui.label(egui::RichText::new(s.t("ssh-tunnels-none")).weak());
        }
        if let Some(form_state) = &mut self.tunnel_form {
            let hosts: Vec<(String, String)> = s
                .connections
                .read()
                .of_kind("ssh")
                .map(|c| (c.id.clone(), c.name.clone()))
                .collect();
            let mut start = false;
            let mut cancel = false;
            egui::Frame::new()
                .fill(theme.panel_bg)
                .inner_margin(egui::Margin::same(8))
                .corner_radius(egui::CornerRadius::same(6))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(s.t("ssh-tunnel-via")).weak());
                    let current = hosts
                        .iter()
                        .find(|(id, _)| *id == form_state.ssh_id)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    // Widths below stay inside the sidebar: anything wider
                    // grows the panel a little every frame.
                    egui::ComboBox::from_id_salt("ssh-tunnel-host")
                        .selected_text(current)
                        .width((ui.available_width() - 24.0).max(60.0))
                        .show_ui(ui, |ui| {
                            for (id, name) in &hosts {
                                ui.selectable_value(&mut form_state.ssh_id, id.clone(), name);
                            }
                        });
                    ui.label(egui::RichText::new(s.t("ssh-tunnel-remote-host")).weak());
                    ui.add(
                        egui::TextEdit::singleline(&mut form_state.remote_host)
                            .hint_text("localhost")
                            .desired_width(f32::INFINITY),
                    );
                    ui.label(egui::RichText::new(s.t("ssh-tunnel-remote-port")).weak());
                    ui.add(
                        egui::TextEdit::singleline(&mut form_state.remote_port)
                            .hint_text("5432")
                            .desired_width(f32::INFINITY),
                    );
                    ui.label(egui::RichText::new(s.t("ssh-tunnel-local")).weak());
                    ui.add(
                        egui::TextEdit::singleline(&mut form_state.local_port)
                            .hint_text(s.t("ssh-tunnel-local-hint"))
                            .desired_width(f32::INFINITY),
                    );
                    if let Some(e) = &form_state.error {
                        ui.colored_label(theme.error, e);
                    }
                    ui.horizontal(|ui| {
                        if ui.button(s.t("ssh-tunnel-start")).clicked() {
                            start = true;
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            cancel = true;
                        }
                    });
                });
            if start {
                match parse_tunnel_form(form_state) {
                    Ok(spec) => {
                        s.tunnels.open(spec);
                        self.tunnel_form = None;
                    }
                    Err(key) => form_state.error = Some(s.t(key)),
                }
            } else if cancel {
                self.tunnel_form = None;
            }
        }
    }

    fn tunnel_row(&self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme, t: &TunnelInfo) {
        let s = cx.services;
        let local = match &t.state {
            TunnelState::Listening { local_port } => local_port.to_string(),
            _ if t.spec.local_port != 0 => t.spec.local_port.to_string(),
            _ => "…".into(),
        };
        let title = format!(
            ":{local} {} {}:{}",
            ph::ARROW_RIGHT,
            t.spec.remote_host,
            t.spec.remote_port
        );
        let detail = match &t.state {
            TunnelState::Starting => s.t("ssh-tunnel-starting"),
            TunnelState::Failed(e) => tunnel_error_text(s, e),
            TunnelState::Listening { .. } => s.i18n.t_args(
                "ssh-tunnel-traffic",
                &[
                    ("via", &t.via),
                    ("open", &t.open.to_string()),
                    ("up", &human_size(t.bytes_up)),
                    ("down", &human_size(t.bytes_down)),
                ],
            ),
        };
        let icon = match &t.state {
            TunnelState::Failed(_) => ph::WARNING,
            TunnelState::Starting => ph::HOURGLASS,
            TunnelState::Listening { .. } => ph::PLUGS_CONNECTED,
        };
        // The row takes exactly the sidebar width (list_row paints its text);
        // the stop button is overlaid on its right end, never beside it, or
        // the side panel would grow a little every frame.
        let r = form::list_row(ui, theme, false, icon, &title, Some(&detail));
        let stop_rect = egui::Rect::from_center_size(
            egui::pos2(r.rect.right() - 16.0, r.rect.center().y),
            egui::vec2(24.0, 24.0),
        );
        let stop = ui.interact(stop_rect, r.id.with("stop"), egui::Sense::click());
        stop.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, s.t("ssh-tunnel-stop"))
        });
        if r.hovered() || stop.hovered() {
            if stop.hovered() {
                ui.painter().rect_filled(
                    stop_rect,
                    egui::CornerRadius::same(4),
                    theme.surface_hover,
                );
            }
            ui.painter().text(
                stop_rect.center(),
                egui::Align2::CENTER_CENTER,
                ph::X,
                egui::FontId::proportional(14.0),
                theme.text,
            );
        }
        if stop.tip(s.t("ssh-tunnel-stop")).clicked() {
            s.tunnels.stop(t.id);
            return;
        }
        let r = if let TunnelState::Failed(_) = &t.state {
            r.on_hover_text(detail.clone())
        } else {
            r
        };
        if let TunnelState::Listening { local_port } = &t.state
            && r.tip(s.t("ssh-tunnel-copy")).clicked()
        {
            ui.ctx().copy_text(format!("127.0.0.1:{local_port}"));
            cx.toast(ToastLevel::Info, s.t("ssh-tunnel-copied"));
        }
    }
}

/// Validate the form; errors are Fluent keys.
fn parse_tunnel_form(f: &TunnelForm) -> Result<ForwardSpec, &'static str> {
    if f.ssh_id.is_empty() {
        return Err("ssh-tunnel-need-host");
    }
    let remote_port: u16 = f
        .remote_port
        .trim()
        .parse()
        .ok()
        .filter(|p| *p > 0)
        .ok_or("ssh-tunnel-bad-port")?;
    let local_port: u16 = if f.local_port.trim().is_empty() {
        0
    } else {
        f.local_port
            .trim()
            .parse()
            .map_err(|_| "ssh-tunnel-bad-port")?
    };
    let host = f.remote_host.trim();
    Ok(ForwardSpec {
        ssh_id: f.ssh_id.clone(),
        remote_host: if host.is_empty() {
            "localhost".into()
        } else {
            host.into()
        },
        remote_port,
        local_port,
    })
}

fn tunnel_error_text(s: &Services, e: &TunnelError) -> String {
    match e {
        TunnelError::NoProfile => s.t("ssh-no-profile"),
        TunnelError::VaultLocked => s.t("ssh-vault-locked"),
        TunnelError::NoPassword => s.t("ssh-no-password"),
        TunnelError::UnknownHostKey => s.t("ssh-tunnel-untrusted"),
        TunnelError::Other(e) => e.clone(),
    }
}

impl Tool for SshTool {
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }
    /// Which host this session is on.
    fn subtitle(&self) -> Option<String> {
        Some(self.conn_name.clone()).filter(|s| !s.is_empty())
    }

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
        let theme = Theme::from_settings(&s.settings.read().appearance);
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // The assistant is the same control in every tool, so it
            // sits in the same place: hard right of the toolbar, away
            // from the buttons that act on what is selected.
            if ui
                .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
                .tip(s.t("ai-shared-button-tip"))
                .clicked()
            {
                self.assistant.toggle("SSH");
            }
            if let Some(sess) = &self.session {
                let (c, r) = sess.size();
                ui.label(egui::RichText::new(format!("{c}×{r}")).weak());
            }
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if form::sidebar_header(
            ui,
            &theme,
            &s.t("ssh-hosts"),
            &[(ph::PLUS, &s.t("ssh-new-host"))],
        )
        .is_some()
        {
            cx.actions.push(hs_core::tool::Action::RunCommand {
                id: "settings:connections".into(),
            });
        }
        let hosts: Vec<(String, String, String, bool)> = s
            .connections
            .read()
            .of_kind("ssh")
            .map(|c| {
                (
                    c.id.clone(),
                    c.name.clone(),
                    SshFields::from_table(&c.fields).label(),
                    c.pinned,
                )
            })
            .collect();
        if hosts.is_empty() {
            form::note(ui, &theme, ph::LIGHTBULB, &s.t("ssh-no-hosts"));
            return true;
        }
        // The order is the user's own; a row can be dragged to a new place.
        let mut reorder = hs_ui::reorder::Reorder::new("ssh-hosts");
        let mut open: Option<(String, bool)> = None;
        let mut pin: Option<(String, bool)> = None;
        for (i, (id, name, label, is_pinned)) in hosts.iter().enumerate() {
            let selected = self.state.conn_id.as_deref() == Some(id.as_str());
            reorder.row(ui, i, |ui| {
                let row = form::connection_row(
                    ui,
                    &theme,
                    selected,
                    ph::COMPUTER_TOWER,
                    name,
                    Some(label),
                    *is_pinned,
                    &s.t("conn-pin-home"),
                    &s.t("conn-unpin-home"),
                );
                if row.response.tip(s.t("ssh-open-host")).clicked() {
                    open = Some((id.clone(), selected));
                }
                if let Some(p) = row.pin {
                    pin = Some((id.clone(), p));
                }
            });
            ui.add_space(2.0);
        }
        if let Some((id, p)) = pin
            && let Err(e) = s.connections.write().set_pinned(&id, p)
        {
            cx.toast(hs_core::tool::ToastLevel::Error, e.to_string());
        }
        if let Some((from, to)) = reorder.finish(ui, theme.accent) {
            if let Err(e) = s.connections.write().reorder(&["ssh"], from, to) {
                cx.toast(hs_core::tool::ToastLevel::Error, e.to_string());
            }
            ui.ctx().request_repaint();
        }
        // The shell decides which tab this lands in; a host may hold up
        // to three sessions, so clicking one again opens another until it
        // has them.
        if let Some((id, selected)) = open
            && !selected
        {
            cx.open("ssh", Some(OpenRequest::Connection { id }));
        }
        ui.add_space(10.0);
        self.tunnels_section(ui, cx, &theme);
        ui.add_space(10.0);
        form::note(ui, &theme, ph::LIGHTBULB, &s.t("ssh-hint"));
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if cx.is_home {
            self.dashboard(ui, cx);
            return;
        }
        // The assistant takes its column first; the file pane and the shell
        // then split what is left, instead of the shell being half covered.
        let (mut content, assistant_rect) =
            hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        let outer = ui;
        let ui = &mut content;
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if self.waiting_for_vault && !s.secrets.is_locked() {
            self.waiting_for_vault = false;
            self.connect(s);
        }
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
        self.entry_menu(ui, cx);
        self.perms_dialog(ui, &theme, s);

        // Layout: a straight 50/50 split of the tool's width — file pane on the
        // left, shell on the right. Both panes are carved into fixed
        // sub-rectangles of exactly half the available width and clipped to
        // them, so neither pane's content can push the divider or feed back into
        // the split (an overfull file table clips instead of growing). The
        // split is recomputed from the available rect every frame, so it tracks
        // the workspace width.
        let term_theme = TermTheme::for_theme(&theme);
        let sftp_ready = self.shared.lock().sftp.is_some();
        let full = ui.available_rect_before_wrap();
        let gap = 7.0;
        let file_w = ((full.width() - gap) * 0.5).floor().max(240.0);
        let file_rect = egui::Rect::from_min_size(full.min, egui::vec2(file_w, full.height()));
        let sep_x = (full.min.x + file_w + gap * 0.5).round();
        let term_rect =
            egui::Rect::from_min_max(egui::pos2(full.min.x + file_w + gap, full.min.y), full.max);

        let mut file_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(file_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        file_ui.set_clip_rect(file_rect.intersect(ui.clip_rect()));
        if sftp_ready {
            self.file_pane(&mut file_ui, &theme, cx);
        } else {
            file_ui.add_space(12.0);
            form::note(&mut file_ui, &theme, ph::INFO, &s.t("ssh-no-sftp-yet"));
        }

        ui.painter()
            .vline(sep_x, full.y_range(), egui::Stroke::new(1.0, theme.border));

        let mut term_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(term_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        term_ui.set_clip_rect(term_rect.intersect(ui.clip_rect()));
        {
            let ui = &mut term_ui;
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
        }
        ui.allocate_rect(full, egui::Sense::hover());
        if self.assistant.open {
            let remote_path = self.shared.lock().remote.path.clone();
            let visible = self
                .session
                .as_ref()
                .map(|session| session.visible_text(80))
                .unwrap_or_default();
            let error = match &self.status {
                Status::Failed(e) => e.clone(),
                _ => "(none)".into(),
            };
            let ctx_text = format!(
                "Host: {}\nStatus: {:?}\nRemote directory: {}\nLast SSH error: {}\n\nRecent visible remote shell output:\n{}",
                self.conn_name, self.status, remote_path, error, visible
            );
            self.assistant.set_context("SSH session", ctx_text);
            if self.assistant.quick_actions.is_empty() {
                self.assistant.quick_actions = hs_ui::ai_assistant::shell_actions(s);
            }
            let Some(panel) = assistant_rect else {
                // Too narrow to split three ways: the toggle stays on and
                // the column returns when there is room for it.
                return;
            };
            let mut child = outer.new_child(
                egui::UiBuilder::new()
                    .max_rect(panel)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(panel.intersect(outer.clip_rect()));
            egui::Frame::new()
                .fill(theme.panel_bg)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(&mut child, |ui| {
                    ui.set_min_width(370.0);
                    self.assistant.can_insert = self.session.is_some();
                    self.assistant.can_run = self.session.is_some();
                    let out = self.assistant.ui(ui, s, &theme, "SSH");
                    if let Some(block) = out.use_block
                        && let Some(session) = &self.session
                    {
                        // Typed into the remote shell; Insert and Run adds
                        // the Return (ADR-019).
                        session.write(block.trim_end().as_bytes());
                        if out.run {
                            session.write(b"\r");
                        }
                    }
                });
        }
        if self.status == Status::Connecting || self.shared.lock().remote.loading {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    fn speakable(&self) -> Vec<hs_core::tool::Speakable> {
        let mut out = Vec::new();
        if let Some(session) = &self.session {
            let screen = session.visible_text(200);
            if !screen.trim().is_empty() {
                out.push(hs_core::tool::Speakable::new("SSH screen", screen));
            }
        }
        if let Some(a) = self.assistant.last_answer() {
            out.push(hs_core::tool::Speakable::new("Assistant's last answer", a));
        }
        out
    }

    fn printable(&self) -> Option<hs_core::tool::Printable> {
        let session = self.session.as_ref()?;
        let screen = session.visible_text(10_000);
        Some(hs_core::tool::Printable {
            title: self.title().to_string(),
            subtitle: "SSH".into(),
            html: hs_core::html::pre(&screen),
        })
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        let c = |id: &str, title: &str| CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: None,
        };
        vec![
            c("ssh.ai", "AI: Toggle Assistant"),
            c("ssh.connect", "SSH: Connect"),
            c("ssh.disconnect", "SSH: Disconnect"),
            c("ssh.refresh", "SSH: Refresh Files"),
            c("ssh.sync", "SSH: Toggle Folder Sync With Shell"),
            c("ssh.tunnel.new", "SSH: New Tunnel"),
            c("ssh.clipboard.copy", "SSH: Copy to Clipboard"),
            c("ssh.clipboard.paste", "SSH: Paste From Clipboard"),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "ssh.ai" => self.assistant.toggle("SSH"),
            "ssh.connect" => self.connect(cx.services),
            "ssh.disconnect" => self.disconnect(),
            "ssh.refresh" => self.refresh(cx.services),
            "ssh.sync" => self.state.sync_cwd = !self.state.sync_cwd,
            "ssh.tunnel.new" => self.new_tunnel_form(cx.services),
            "ssh.clipboard.copy" => self.copy_selected(cx.services),
            "ssh.clipboard.paste" => self.paste_from_clipboard(cx.services),
            _ => return false,
        }
        true
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or_default()
    }

    /// Three shells on one host is ordinary SSH practice — a build in one,
    /// a tail in another, a prompt in the third — so this is the one
    /// connection-bound tool that may be duplicated, up to that many.
    /// Owner's requirement, 2026-10-17.
    fn tabs_per_connection(&self) -> usize {
        3
    }

    fn current_connection(&self) -> Option<&str> {
        self.state.conn_id.as_deref()
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
                    mode: md.permissions.unwrap_or(0),
                    owner: md
                        .user
                        .clone()
                        .or_else(|| md.uid.map(|u| u.to_string()))
                        .unwrap_or_default(),
                    group: md
                        .group
                        .clone()
                        .or_else(|| md.gid.map(|g| g.to_string()))
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

/// The permissions dialog's working copy. `mode` is the nine bits being
/// edited; `typed` is what is in the number field, which may not be a
/// number yet while it is being typed.
#[derive(Debug, Clone)]
struct PermsEdit {
    path: String,
    name: String,
    is_dir: bool,
    mode: u32,
    typed: String,
    owner: String,
    group: String,
    /// What they were when the dialog opened, so only real changes are sent.
    was: (u32, String, String),
}

/// Upload a file or a whole folder, keeping the folder's shape. Returns how
/// many files were written.
async fn upload_tree(
    sftp: &SftpSession,
    local: &std::path::Path,
    remote_dir: &str,
    h: &hs_core::jobs::JobHandle,
) -> Result<u64, String> {
    let name = local
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| format!("{} has no name", local.display()))?;
    let target = join_remote(remote_dir, &name);
    if local.is_file() {
        upload_file(sftp, local, &target, h).await?;
        return Ok(1);
    }
    if !local.is_dir() {
        return Err(format!(
            "{} is neither a file nor a folder",
            local.display()
        ));
    }
    // An existing folder is fine; anything else is the error it says.
    if let Err(e) = sftp.create_dir(&target).await {
        let text = e.to_string();
        if !text.contains("exist") {
            return Err(text);
        }
    }
    let mut count = 0u64;
    let mut entries = tokio::fs::read_dir(local)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        if h.is_cancelled() {
            return Err("cancelled".into());
        }
        count += Box::pin(upload_tree(sftp, &entry.path(), &target, h)).await?;
    }
    Ok(count)
}

/// Download a file or a whole folder into `local_dir`. Returns how many files
/// were written.
async fn download_tree(
    sftp: &SftpSession,
    remote: &str,
    local_dir: &std::path::Path,
    h: &hs_core::jobs::JobHandle,
) -> Result<u64, String> {
    let name = remote.rsplit('/').next().unwrap_or(remote).to_string();
    let target = local_dir.join(&name);
    let is_dir = sftp
        .metadata(remote)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false);
    if !is_dir {
        download_file(sftp, remote, &target, h).await?;
        return Ok(1);
    }
    tokio::fs::create_dir_all(&target)
        .await
        .map_err(|e| e.to_string())?;
    let entries = sftp.read_dir(remote).await.map_err(|e| e.to_string())?;
    let mut count = 0u64;
    for entry in entries {
        if h.is_cancelled() {
            return Err("cancelled".into());
        }
        let child = join_remote(remote, &entry.file_name());
        count += Box::pin(download_tree(sftp, &child, &target, h)).await?;
    }
    Ok(count)
}

/// Join a remote folder and a name, without doubling the separator.
fn join_remote(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_form_validation() {
        let mut f = TunnelForm {
            ssh_id: "x".into(),
            remote_host: " db ".into(),
            remote_port: "5432".into(),
            ..Default::default()
        };
        let spec = parse_tunnel_form(&f).unwrap();
        assert_eq!(
            (spec.remote_host.as_str(), spec.remote_port, spec.local_port),
            ("db", 5432, 0)
        );
        f.local_port = "15432".into();
        assert_eq!(parse_tunnel_form(&f).unwrap().local_port, 15432);
        f.remote_port = "0".into();
        assert_eq!(parse_tunnel_form(&f), Err("ssh-tunnel-bad-port"));
        f.remote_port = "70000".into();
        assert_eq!(parse_tunnel_form(&f), Err("ssh-tunnel-bad-port"));
        f.remote_port = "22".into();
        f.ssh_id.clear();
        assert_eq!(parse_tunnel_form(&f), Err("ssh-tunnel-need-host"));
    }

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

    #[test]
    fn a_remote_path_joins_without_doubling_the_separator() {
        assert_eq!(join_remote("/home/dev", "notes"), "/home/dev/notes");
        assert_eq!(join_remote("/", "notes"), "/notes");
        assert_eq!(join_remote("/home/dev/", "notes"), "/home/dev/notes");
    }
}
