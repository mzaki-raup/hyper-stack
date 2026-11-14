//! A server's tab (§3.3): the toolbar, the browse column (realm and
//! sections), the section's list in the middle and the item page on the
//! right, the tab's dialogs, and what a finished write does next.

use hs_core::tool::{CommandDescriptor, OpenRequest, ToastLevel, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

use super::IdmTool;
use super::clients::ClientsView;
use super::common::{After, Cx, Out, WriteOk, Writes, rail_row, section_key};
use super::dialogs::{Answer, Dialog};
use super::events::EventsView;
use super::export::ExportView;
use super::groups::GroupsView;
use super::realm::RealmView;
use super::roles::RolesView;
use super::sessions::SessionsView;
use super::users::UsersView;
use crate::provider::{Availability, Page, Section};
use crate::runner::Status;

/// Everything a tab shows, dropped on a realm switch or a reconnect.
pub struct TabView {
    users: UsersView,
    groups: GroupsView,
    realm_roles: RolesView,
    client_roles: RolesView,
    clients: ClientsView,
    sessions: SessionsView,
    login_events: EventsView,
    admin_events: EventsView,
    realm: RealmView,
    export: ExportView,
    dialog: Option<Dialog>,
    writes: Writes,
    /// The realm picker's text, which may be a realm the list lacks.
    realm_text: String,
    /// A realm typed that the server refused, said once.
    restored_user: bool,
}

impl Default for TabView {
    fn default() -> Self {
        TabView {
            users: UsersView::default(),
            groups: GroupsView::default(),
            realm_roles: RolesView::new(false),
            client_roles: RolesView::new(true),
            clients: ClientsView::default(),
            sessions: SessionsView::default(),
            login_events: EventsView::new(false),
            admin_events: EventsView::new(true),
            realm: RealmView::default(),
            export: ExportView::default(),
            dialog: None,
            writes: Writes::default(),
            realm_text: String::new(),
            restored_user: false,
        }
    }
}

impl TabView {
    /// A new session or a new realm: nothing read so far still holds.
    pub fn reset(&mut self) {
        let writes = self.writes.clone();
        *self = TabView::default();
        self.writes = writes;
    }

    fn reset_realm(&mut self) {
        self.users.reset();
        self.groups.reset();
        self.realm_roles.reset();
        self.client_roles.reset();
        self.clients.reset();
        self.sessions.reset();
        self.login_events.reset();
        self.admin_events.reset();
        self.realm.reset();
        self.export = ExportView::default();
    }

    pub fn selected_user(&self) -> Option<String> {
        self.users.selected.clone()
    }

    fn refresh(&mut self, section: Section) {
        match section {
            Section::Users => self.users.refresh(),
            Section::Groups => self.groups.refresh(),
            Section::RealmRoles => self.realm_roles.refresh(),
            Section::ClientRoles => self.client_roles.refresh(),
            Section::Clients => self.clients.refresh(),
            Section::Sessions => self.sessions.refresh(),
            Section::LoginEvents => self.login_events.refresh(),
            Section::AdminEvents => self.admin_events.refresh(),
            Section::RealmSettings => self.realm.reset(),
        }
    }

    /// A finished write: the toast, then the re-read (§4.10).
    fn finished(&mut self, cx: &mut ToolContext) {
        for d in self.writes.drain() {
            match d.result {
                Ok(ok) => {
                    match &ok {
                        WriteOk::LogoutAll(l) if !l.failed.is_empty() => cx.toast(
                            ToastLevel::Warning,
                            cx.services.i18n.t_args(
                                "idm-done-logout-all-partly",
                                &[("clients", &l.failed.join(", "))],
                            ),
                        ),
                        _ => cx.toast(ToastLevel::Success, d.done.clone()),
                    }
                    match (&d.after, ok_id(&ok)) {
                        (After::Select(s), Some(id)) => {
                            self.refresh(*s);
                            self.select(*s, id);
                        }
                        (After::Gone(s), _) => {
                            self.forget(*s);
                            self.refresh(*s);
                        }
                        (After::Reload(s) | After::Select(s), _) => self.refresh(*s),
                    }
                    if let WriteOk::Secret(secret) = ok {
                        self.clients.show_secret(secret);
                    }
                }
                Err(e) => {
                    cx.toast(ToastLevel::Error, hs_core::fmt::sentence(&e.to_string()));
                    // An item a write found gone is dropped from its list.
                    if let crate::provider::IdmError::NotFound { .. } = e {
                        let s = match &d.after {
                            After::Reload(s) | After::Select(s) | After::Gone(s) => *s,
                        };
                        self.forget(s);
                        self.refresh(s);
                    }
                }
            }
        }
    }

    fn select(&mut self, section: Section, id: String) {
        match section {
            Section::Users => self.users.select(id),
            Section::Groups => self.groups.select(id),
            Section::RealmRoles => self.realm_roles.select(id),
            Section::ClientRoles => self.client_roles.select(id),
            Section::Clients => self.clients.select(id),
            _ => {}
        }
    }

    fn forget(&mut self, section: Section) {
        match section {
            Section::Users => self.users.forget_selected(),
            Section::Groups => self.groups.forget_selected(),
            Section::RealmRoles => self.realm_roles.forget_selected(),
            Section::ClientRoles => self.client_roles.forget_selected(),
            Section::Clients => self.clients.forget_selected(),
            _ => {}
        }
    }
}

fn ok_id(ok: &WriteOk) -> Option<String> {
    match ok {
        WriteOk::Created(id) => Some(id.clone()),
        _ => None,
    }
}

/// The tab's body.
pub fn body(tool: &mut IdmTool, ui: &mut egui::Ui, cx: &mut ToolContext) {
    let s = cx.services;
    let theme = Theme::from_settings(&s.settings.read().appearance);
    tool.view.finished(cx);
    if tool.state.conn_id.is_none() {
        form::pane(ui, |ui| {
            form::note(ui, &theme, ph::PLUGS_CONNECTED, &s.t("idm-pick-hint"))
        });
        return;
    }
    if let Some(e) = tool.conn_error.clone() {
        form::pane(ui, |ui| {
            form::note(ui, &theme, ph::WARNING, &hs_core::fmt::sentence(&e));
            ui.add_space(form::ROW_GAP);
            if form::button(ui, &theme, ph::ARROWS_CLOCKWISE, &s.t("idm-retry")).clicked() {
                tool.connect(s);
            }
        });
        return;
    }
    let Some(status) = tool.session.as_ref().map(|x| x.status()) else {
        return;
    };
    match status {
        Status::Connecting => {
            form::pane(ui, |ui| {
                form::centred_row(ui, |ui| {
                    ui.spinner();
                    ui.label(
                        s.i18n
                            .t_args("idm-connecting", &[("server", &tool.address)]),
                    );
                });
            });
            return;
        }
        Status::Failed(e) => {
            form::pane(ui, |ui| {
                form::note(
                    ui,
                    &theme,
                    ph::WARNING,
                    &hs_core::fmt::sentence(&e.to_string()),
                );
                ui.add_space(form::ROW_GAP);
                form::centred_row(ui, |ui| {
                    if form::button(ui, &theme, ph::ARROWS_CLOCKWISE, &s.t("idm-retry")).clicked() {
                        tool.connect(s);
                    }
                    if form::button(ui, &theme, ph::PENCIL_SIMPLE, &s.t("idm-edit-connection"))
                        .clicked()
                        && let Some(id) = &tool.state.conn_id
                    {
                        cx.actions.push(hs_core::tool::Action::RunCommand {
                            id: format!("settings:connection:{id}"),
                        });
                    }
                });
            });
            return;
        }
        Status::Ready => {}
    }
    let (caps, realms) = {
        let Some(session) = tool.session.as_ref() else {
            return;
        };
        // Asked fresh, not the session's snapshot: Allow Writes changes
        // what the guarded provider offers without any request running.
        use crate::provider::IdmProvider;
        let caps = session.provider().capabilities();
        let sh = session.shared();
        (
            caps,
            sh.realms
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .map(|r| r.iter().map(|r| r.name.clone()).collect::<Vec<_>>())
                .unwrap_or_default(),
        )
    };
    // The dashboard's figures follow what this tab last read.
    if let Some(id) = &tool.state.conn_id {
        let users = tool.view.users.total();
        super::dashboard::FIGURES
            .lock()
            .insert(id.clone(), (realms.len(), users));
    }
    // A restored tab opens on the user it showed.
    if !tool.view.restored_user {
        tool.view.restored_user = true;
        if let Some(u) = tool.state.user.take() {
            tool.view.users.select(u);
        }
    }
    if tool.view.realm_text.is_empty() {
        tool.view.realm_text = tool.state.realm.clone();
    }
    let page = Page::of(s.settings.read().idm.page_size);
    let refusal = tool.refusal(s);
    let mut out = Out::default();
    let section: Section = tool.state.section.into();
    let realm = tool.state.realm.clone();

    // The browse column: realm, then the sections.
    if tool.state.browse {
        let mut switch: Option<String> = None;
        let mut go: Option<Section> = None;
        egui::Panel::left(ui.id().with("idm-browse"))
            .resizable(true)
            .default_size(220.0)
            .size_range(180.0..=360.0)
            .show(ui, |ui| {
                form::pane(ui, |ui| {
                    form::subsection(ui, &theme, &s.t("idm-realm"));
                    ui.add_space(form::PANE_GAP);
                    let r = form::pick_field_width(
                        ui,
                        &theme,
                        &mut tool.view.realm_text,
                        &realms,
                        &s.t("idm-realm-hint"),
                        ui.available_width(),
                    );
                    let typed = tool.view.realm_text.trim().to_string();
                    if (r.lost_focus() || realms.contains(&typed))
                        && !typed.is_empty()
                        && typed != realm
                    {
                        switch = Some(typed);
                    }
                    ui.add_space(form::PANE_GAP);
                    ui.separator();
                    ui.add_space(form::PANE_GAP);
                    for sec in Section::ALL {
                        let avail = caps.section(&realm, sec);
                        let r = ui
                            .add_enabled_ui(avail == Availability::Yes, |ui| {
                                rail_row(ui, &theme, sec == section, sec, &s.t(section_key(sec)))
                            })
                            .inner;
                        let r = match &avail {
                            Availability::No(why) => r.tip_disabled(hs_core::fmt::sentence(why)),
                            Availability::Yes => r,
                        };
                        if r.clicked() {
                            go = Some(sec);
                        }
                    }
                });
            });
        if let Some(r) = switch {
            tool.state.realm = r;
            tool.view.reset_realm();
        }
        if let Some(sec) = go {
            tool.state.section = sec.into();
        }
    }
    let section: Section = tool.state.section.into();
    let realm = tool.state.realm.clone();
    let Some(session) = tool.session.as_mut() else {
        return;
    };
    let mut cx2 = Cx {
        s,
        theme: &theme,
        session,
        realm: &realm,
        caps: &caps,
        page,
        refusal,
        out: &mut out,
    };
    let v = &mut tool.view;
    if v.export.open() {
        v.export.show(ui, &mut cx2);
    } else {
        match section {
            Section::RealmSettings => v.realm.show(ui, &mut cx2),
            _ => {
                let min = item_min_width(ui, s, section);
                egui::Panel::right(ui.id().with("idm-item"))
                    .resizable(true)
                    .default_size(min)
                    .size_range(min..=900.0)
                    .show(ui, |ui| match section {
                        Section::Users => v.users.item(ui, &mut cx2),
                        Section::Groups => v.groups.item(ui, &mut cx2),
                        Section::RealmRoles => v.realm_roles.item(ui, &mut cx2),
                        Section::ClientRoles => v.client_roles.item(ui, &mut cx2),
                        Section::Clients => v.clients.item(ui, &mut cx2),
                        Section::Sessions => v.sessions.item(ui, &mut cx2),
                        Section::LoginEvents => v.login_events.item(ui, &mut cx2),
                        Section::AdminEvents => v.admin_events.item(ui, &mut cx2),
                        Section::RealmSettings => {}
                    });
                // The list never draws under the item column: a table
                // wider than what is left scrolls inside it instead.
                // The list never draws under the item column: what is left
                // of the pane clips it, and a table wider than that scrolls
                // sideways inside it.
                let room = ui.max_rect().intersect(ui.clip_rect());
                ui.set_clip_rect(room);
                egui::ScrollArea::horizontal()
                    .id_salt(("idm-list-h", section as u8))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        form::pane(ui, |ui| match section {
                            Section::Users => v.users.list(ui, &mut cx2),
                            Section::Groups => v.groups.list(ui, &mut cx2),
                            Section::RealmRoles => v.realm_roles.list(ui, &mut cx2),
                            Section::ClientRoles => v.client_roles.list(ui, &mut cx2),
                            Section::Clients => v.clients.list(ui, &mut cx2),
                            Section::Sessions => v.sessions.list(ui, &mut cx2),
                            Section::LoginEvents => v.login_events.list(ui, &mut cx2),
                            Section::AdminEvents => v.admin_events.list(ui, &mut cx2),
                            Section::RealmSettings => {}
                        });
                    });
            }
        }
    }
    drop(cx2);
    apply_out(tool, out, ui, cx);
}

/// The item column's narrowest: its tab strip with every label whole
/// (CR-0069), plus the pane's margins.
fn item_min_width(ui: &egui::Ui, s: &hs_core::Services, section: Section) -> f32 {
    let keys: &[&str] = match section {
        Section::Users => &[
            "idm-tab-details",
            "idm-tab-attributes",
            "idm-tab-credentials",
            "idm-tab-groups",
            "idm-tab-role-mapping",
            "idm-tab-sessions",
        ],
        Section::Groups => &[
            "idm-tab-members",
            "idm-tab-role-mapping",
            "idm-tab-attributes",
            "idm-tab-details",
        ],
        Section::RealmRoles | Section::ClientRoles => &[
            "idm-tab-details",
            "idm-tab-composites",
            "idm-tab-users-in-role",
            "idm-tab-groups-in-role",
        ],
        Section::Clients => &[
            "idm-tab-settings",
            "idm-tab-credentials",
            "idm-tab-roles",
            "idm-tab-service-account",
            "idm-tab-sessions",
        ],
        _ => &[],
    };
    let labels: Vec<String> = keys.iter().map(|k| s.t(k)).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    (hs_ui::tabs::segmented_min_width(ui, &refs, &[], &[]) + 2.0 * form::PANE_MARGIN + 16.0)
        .max(360.0)
}

/// What the sections asked for: dialogs, writes, toasts, the console.
fn apply_out(tool: &mut IdmTool, out: Out, ui: &mut egui::Ui, cx: &mut ToolContext) {
    let s = cx.services;
    let theme = Theme::from_settings(&s.settings.read().appearance);
    for (level, text) in out.toasts {
        cx.toast(level, text);
    }
    if let Some(d) = out.dialog {
        tool.view.dialog = Some(d);
    }
    for w in out.writes {
        send(tool, cx, w);
    }
    if let Some(page) = out.console {
        open_console(tool, cx, &page);
    }
    for o in out.open {
        match &o {
            OpenRequest::Text { .. } => cx.open("editor", Some(o)),
            _ => cx.open("browser", Some(o)),
        }
    }
    if let Some(d) = tool.view.dialog.take() {
        match d.show(ui.ctx(), &theme, s) {
            Answer::Open(d) => tool.view.dialog = Some(d),
            Answer::Closed => {}
            Answer::Send(w) => send(tool, cx, w),
            Answer::AllowWrites => {
                if let Some(x) = &tool.session {
                    x.gate().unlock();
                }
            }
            Answer::Reload => {
                let section: Section = tool.state.section.into();
                tool.view.refresh(section);
            }
        }
    }
}

fn send(tool: &mut IdmTool, cx: &mut ToolContext, w: super::common::WriteReq) {
    if let Some(x) = &tool.session {
        tool.view.writes.send(cx.services, x.provider(), w);
    }
}

fn open_console(tool: &IdmTool, cx: &mut ToolContext, page: &str) {
    let Some(x) = &tool.session else {
        return;
    };
    use crate::provider::IdmProvider;
    let url = x.provider().console_url(&tool.state.realm, page);
    cx.open("browser", Some(OpenRequest::Url { url }));
}

/// The main toolbar (§3.3): Browse first, Refresh, Open in Admin
/// Console, Export Realm, Allow Writes on a protected tab.
pub fn toolbar(tool: &mut IdmTool, ui: &mut egui::Ui, cx: &mut ToolContext) {
    let s = cx.services;
    let theme = Theme::from_settings(&s.settings.read().appearance);
    let ready = tool
        .session
        .as_ref()
        .is_some_and(|x| x.status() == Status::Ready);
    hs_ui::toolbar::toggle(
        ui,
        &theme,
        &mut tool.state.browse,
        ph::SIDEBAR_SIMPLE,
        &s.t("idm-browse"),
    )
    .tip(s.t("idm-browse-tip"));
    if hs_ui::toolbar::btn_enabled(ui, &theme, ready, ph::ARROWS_CLOCKWISE, &s.t("idm-refresh"))
        .tip(s.t("idm-refresh-tip"))
        .clicked()
    {
        run_command(tool, "idm.refresh", cx);
    }
    if hs_ui::toolbar::btn_enabled(ui, &theme, ready, ph::ARROW_SQUARE_OUT, &s.t("idm-console"))
        .tip(s.t("idm-console-tip"))
        .clicked()
    {
        run_command(tool, "idm.console", cx);
    }
    if hs_ui::toolbar::btn_enabled(ui, &theme, ready, ph::DOWNLOAD_SIMPLE, &s.t("idm-export"))
        .tip(s.t("idm-export-tip"))
        .clicked()
    {
        run_command(tool, "idm.export", cx);
    }
    if let Some(x) = &tool.session
        && x.gate().can_unlock()
    {
        ui.separator();
        if hs_ui::toolbar::btn(ui, &theme, ph::LOCK, &s.t("idm-allow-writes"))
            .tip(
                s.i18n
                    .t_args("idm-allow-writes-tip", &[("env", &tool.env_name)]),
            )
            .clicked()
        {
            tool.view.dialog = Some(Dialog::AllowWrites {
                env: tool.env_name.clone(),
            });
        }
    }
}

pub fn commands() -> Vec<CommandDescriptor> {
    let c = |id: &str, title: &str, key: Option<&str>| CommandDescriptor {
        id: id.into(),
        title: title.into(),
        shortcut_action: key.map(String::from),
    };
    vec![
        c("idm.refresh", "Identity: Refresh", Some("run.alt")),
        c("idm.search", "Identity: Search…", Some("idm.search")),
        c("idm.realm", "Identity: Switch Realm…", Some("idm.realm")),
        c("idm.browse", "Identity: Toggle Browse Column", None),
        c("idm.go.users", "Identity: Go to Users", None),
        c("idm.go.groups", "Identity: Go to Groups", None),
        c("idm.go.realm_roles", "Identity: Go to Realm Roles", None),
        c("idm.go.client_roles", "Identity: Go to Client Roles", None),
        c("idm.go.clients", "Identity: Go to Clients", None),
        c("idm.go.sessions", "Identity: Go to Sessions", None),
        c("idm.go.login_events", "Identity: Go to Login Events", None),
        c("idm.go.admin_events", "Identity: Go to Admin Events", None),
        c("idm.go.realm", "Identity: Go to Realm Settings", None),
        c("idm.export", "Identity: Export Realm…", None),
        c("idm.console", "Identity: Open in Admin Console", None),
        c("idm.logout_all", "Identity: Log Out All Sessions…", None),
        c("idm.add", "Identity: Add Identity Server", None),
    ]
}

pub fn run_command(tool: &mut IdmTool, id: &str, cx: &mut ToolContext) -> bool {
    let go = |tool: &mut IdmTool, s: Section| {
        tool.state.section = s.into();
        true
    };
    match id {
        "idm.refresh" | "run.alt" => {
            let section: Section = tool.state.section.into();
            tool.view.refresh(section);
            true
        }
        "idm.browse" => {
            tool.state.browse = !tool.state.browse;
            true
        }
        "idm.search" => {
            tool.state.section = Section::Users.into();
            tool.view.users.focus_search();
            true
        }
        "idm.realm" => {
            tool.state.browse = true;
            true
        }
        "idm.go.users" => go(tool, Section::Users),
        "idm.go.groups" => go(tool, Section::Groups),
        "idm.go.realm_roles" => go(tool, Section::RealmRoles),
        "idm.go.client_roles" => go(tool, Section::ClientRoles),
        "idm.go.clients" => go(tool, Section::Clients),
        "idm.go.sessions" => go(tool, Section::Sessions),
        "idm.go.login_events" => go(tool, Section::LoginEvents),
        "idm.go.admin_events" => go(tool, Section::AdminEvents),
        "idm.go.realm" => go(tool, Section::RealmSettings),
        "idm.export" => {
            tool.view.export.start();
            true
        }
        "idm.console" => {
            let page = tool.view.console_page(tool.state.section.into());
            open_console(tool, cx, &page);
            true
        }
        "idm.logout_all" => {
            tool.state.section = Section::Sessions.into();
            tool.view.sessions.ask_logout_all();
            true
        }
        "idm.add" => {
            cx.actions.push(hs_core::tool::Action::RunCommand {
                id: "settings:identity:new".into(),
            });
            true
        }
        _ => false,
    }
}

impl TabView {
    /// The admin console's page for what the tab shows.
    fn console_page(&self, section: Section) -> String {
        match section {
            Section::Users => match &self.users.selected {
                Some(id) => format!("users/{id}/settings"),
                None => "users".into(),
            },
            Section::Groups => "groups".into(),
            Section::RealmRoles => "roles".into(),
            Section::ClientRoles | Section::Clients => match self.clients.selected_id() {
                Some(id) => format!("clients/{id}/settings"),
                None => "clients".into(),
            },
            Section::Sessions => "sessions".into(),
            Section::LoginEvents | Section::AdminEvents => "events".into(),
            Section::RealmSettings => "realm-settings".into(),
        }
    }
}
