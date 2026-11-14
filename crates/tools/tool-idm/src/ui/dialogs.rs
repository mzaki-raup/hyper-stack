//! The tab's modals (docs/tools/idm.md §6.2): a confirmation naming what
//! changes, a typed confirmation for delete-style verbs, the review of a
//! multi-field save, Reset Password, Allow Writes, and the short create
//! forms of §4.10. A dialog that ends in a write hands its [`WriteReq`]
//! back to the tab; nothing here talks to the server.

use hs_core::Services;
use hs_ui::dialog::{DialogResult, TypedConfirm};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use secrecy::SecretString;
use zeroize::Zeroize;

use super::common::{After, WriteOk, WriteReq};
use crate::provider::{IdmProvider, NewClient, NewRole, NewUser, ObjRef, RoleScope, Section};

pub enum Dialog {
    /// A yes/no naming what changes (Enable, Log Out, End Session).
    Confirm {
        title: String,
        body: String,
        ok: String,
        danger: bool,
        then: WriteReq,
    },
    /// The name must be typed back (Delete, Regenerate Secret, Log Out
    /// All Sessions).
    Typed {
        title: String,
        body: String,
        name: String,
        ok: String,
        typed: String,
        then: WriteReq,
    },
    /// Every changed field, old → new, before a save; `stale` when the
    /// server's copy changed since the page was read.
    Review {
        title: String,
        rows: Vec<(String, String, String)>,
        stale: bool,
        then: WriteReq,
    },
    ResetPassword {
        realm: String,
        user: ObjRef,
        first: String,
        second: String,
        temporary: bool,
        mismatch: bool,
    },
    /// Unlock writes on a protected tab for this session (§6.1).
    AllowWrites {
        env: String,
    },
    Create(Create),
}

pub enum Create {
    User {
        realm: String,
        form: NewUser,
    },
    Group {
        realm: String,
        parent: Option<ObjRef>,
        name: String,
    },
    Role {
        realm: String,
        scope: RoleScope,
        form: NewRole,
    },
    Client {
        realm: String,
        form: NewClient,
    },
}

/// What the user did with a dialog this frame.
pub enum Answer {
    Open(Dialog),
    Closed,
    Send(WriteReq),
    AllowWrites,
    /// Review's Reload: drop the edits and read the item again.
    Reload,
}

impl Dialog {
    pub fn show(self, ctx: &egui::Context, theme: &Theme, s: &Services) -> Answer {
        match self {
            Dialog::Confirm {
                title,
                body,
                ok,
                danger,
                then,
            } => match hs_ui::dialog::confirm(
                ctx,
                theme,
                "idm-confirm",
                &title,
                &body,
                &ok,
                &s.t("cancel"),
                danger,
            ) {
                DialogResult::Ok => Answer::Send(then),
                DialogResult::Cancel => Answer::Closed,
                DialogResult::Open => Answer::Open(Dialog::Confirm {
                    title,
                    body,
                    ok,
                    danger,
                    then,
                }),
            },
            Dialog::Typed {
                title,
                body,
                name,
                ok,
                mut typed,
                then,
            } => match hs_ui::dialog::typed_confirm(
                ctx,
                theme,
                "idm-typed",
                &TypedConfirm {
                    title: &title,
                    body: &body,
                    name: &name,
                    ok_label: &ok,
                    cancel_label: &s.t("cancel"),
                },
                &mut typed,
            ) {
                DialogResult::Ok => Answer::Send(then),
                DialogResult::Cancel => Answer::Closed,
                DialogResult::Open => Answer::Open(Dialog::Typed {
                    title,
                    body,
                    name,
                    ok,
                    typed,
                    then,
                }),
            },
            Dialog::Review {
                title,
                rows,
                stale,
                then,
            } => review(ctx, theme, s, title, rows, stale, then),
            Dialog::ResetPassword {
                realm,
                user,
                first,
                second,
                temporary,
                mismatch,
            } => reset_password(
                ctx, theme, s, realm, user, first, second, temporary, mismatch,
            ),
            Dialog::AllowWrites { env } => match hs_ui::dialog::confirm(
                ctx,
                theme,
                "idm-allow-writes",
                &s.t("idm-allow-writes-title"),
                &s.i18n.t_args("idm-allow-writes-body", &[("env", &env)]),
                &format!("{} {}", ph::LOCK_OPEN, s.t("idm-allow-writes")),
                &s.t("cancel"),
                true,
            ) {
                DialogResult::Ok => Answer::AllowWrites,
                DialogResult::Cancel => Answer::Closed,
                DialogResult::Open => Answer::Open(Dialog::AllowWrites { env }),
            },
            Dialog::Create(c) => create(ctx, theme, s, c),
        }
    }
}

fn review(
    ctx: &egui::Context,
    theme: &Theme,
    s: &Services,
    title: String,
    rows: Vec<(String, String, String)>,
    stale: bool,
    then: WriteReq,
) -> Answer {
    let mut answer = None;
    let mut cancel = false;
    let modal = egui::Modal::new(egui::Id::new("idm-review")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading(&title);
        ui.add_space(form::PANE_GAP);
        if stale {
            form::note(ui, theme, ph::WARNING, &s.t("idm-review-stale"));
            ui.add_space(form::PANE_GAP);
        }
        if rows.is_empty() {
            ui.label(s.t("idm-review-nothing"));
        } else {
            let empty = s.t("idm-review-empty-value");
            let shown = |v: &str| {
                if v.is_empty() {
                    empty.clone()
                } else {
                    v.to_string()
                }
            };
            let facts: Vec<(String, String)> = rows
                .iter()
                .map(|(f, o, n)| (f.clone(), format!("{}  →  {}", shown(o), shown(n))))
                .collect();
            egui::ScrollArea::vertical()
                .id_salt("idm-review-rows")
                .max_height(260.0)
                .show(ui, |ui| {
                    form::detail_table(
                        ui,
                        theme,
                        "idm-review-table",
                        facts.iter().map(|(a, b)| (a.as_str(), b.as_str())),
                    );
                });
        }
        ui.add_space(form::ROW_GAP);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled_ui(!rows.is_empty(), |ui| {
                    form::primary_button(
                        ui,
                        theme,
                        &format!("{} {}", ph::FLOPPY_DISK, s.t("idm-save")),
                    )
                })
                .inner
                .clicked()
            {
                answer = Some(true);
            }
            ui.add_space(form::BUTTON_GAP - ui.spacing().item_spacing.x);
            if stale && form::button(ui, theme, ph::ARROWS_CLOCKWISE, &s.t("idm-reload")).clicked()
            {
                answer = Some(false);
            }
            if form::button(ui, theme, ph::X, &s.t("cancel")).clicked() {
                cancel = true;
            }
        });
    });
    match answer {
        Some(true) => Answer::Send(then),
        Some(false) => Answer::Reload,
        None if cancel || modal.should_close() => Answer::Closed,
        None => Answer::Open(Dialog::Review {
            title,
            rows,
            stale,
            then,
        }),
    }
}

/// Reset Password (§4.3): typed twice, Temporary on by default. The text
/// goes from the field into the request as a `SecretString` and both
/// fields are zeroised; it is never stored, logged or shown.
#[allow(clippy::too_many_arguments)] // reason: the dialog's own fields, carried between frames
fn reset_password(
    ctx: &egui::Context,
    theme: &Theme,
    s: &Services,
    realm: String,
    user: ObjRef,
    mut first: String,
    mut second: String,
    mut temporary: bool,
    mut mismatch: bool,
) -> Answer {
    let mut go = false;
    let mut cancel = false;
    let modal = egui::Modal::new(egui::Id::new("idm-reset-password")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.heading(s.i18n.t_args("idm-reset-title", &[("user", &user.name)]));
        ui.add_space(form::PANE_GAP);
        form::field_grid(ui, "idm-reset-grid", |ui| {
            form::label(ui, &s.t("idm-reset-new"));
            let r = ui.add(form::line_edit(&mut first).password(true));
            if r.changed() {
                mismatch = false;
            }
            ui.end_row();
            form::label(ui, &s.t("idm-reset-again"));
            if ui
                .add(form::line_edit(&mut second).password(true))
                .changed()
            {
                mismatch = false;
            }
            ui.end_row();
            form::label(ui, &s.t("idm-reset-temporary"));
            ui.checkbox(&mut temporary, "");
            ui.end_row();
            form::row_hint(ui, theme, &s.t("idm-reset-temporary-hint"));
        });
        if mismatch {
            ui.add_space(form::PANE_GAP);
            ui.colored_label(theme.error, s.t("idm-reset-mismatch"));
        }
        ui.add_space(form::ROW_GAP);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled_ui(!first.is_empty(), |ui| {
                    form::primary_button(
                        ui,
                        theme,
                        &format!("{} {}", ph::PASSWORD, s.t("idm-reset-password")),
                    )
                })
                .inner
                .clicked()
            {
                go = true;
            }
            ui.add_space(form::BUTTON_GAP - ui.spacing().item_spacing.x);
            if form::button(ui, theme, ph::X, &s.t("cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if cancel || (modal.should_close() && !go) {
        first.zeroize();
        second.zeroize();
        return Answer::Closed;
    }
    if go {
        if first != second {
            second.zeroize();
            return Answer::Open(Dialog::ResetPassword {
                realm,
                user,
                first,
                second,
                temporary,
                mismatch: true,
            });
        }
        let password = SecretString::from(std::mem::take(&mut first));
        second.zeroize();
        let done = s.i18n.t_args("idm-done-reset", &[("user", &user.name)]);
        return Answer::Send(WriteReq::new(
            done,
            After::Reload(Section::Users),
            move |p| async move {
                p.reset_password(&realm, &user, password, temporary)
                    .await
                    .map(|()| WriteOk::Done)
            },
        ));
    }
    Answer::Open(Dialog::ResetPassword {
        realm,
        user,
        first,
        second,
        temporary,
        mismatch,
    })
}

/// The create forms of §4.10. OK sends the create; the tab selects what
/// was made.
fn create(ctx: &egui::Context, theme: &Theme, s: &Services, mut c: Create) -> Answer {
    let mut go = false;
    let mut cancel = false;
    let title = match &c {
        Create::User { .. } => s.t("idm-new-user"),
        Create::Group { parent: None, .. } => s.t("idm-new-group"),
        Create::Group {
            parent: Some(p), ..
        } => s.i18n.t_args("idm-new-sub-group", &[("parent", &p.name)]),
        Create::Role { .. } => s.t("idm-new-role"),
        Create::Client { .. } => s.t("idm-new-client"),
    };
    let modal = egui::Modal::new(egui::Id::new("idm-create")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.heading(&title);
        ui.add_space(form::PANE_GAP);
        form::field_grid(ui, "idm-create-grid", |ui| match &mut c {
            Create::User { form: f, .. } => {
                form::label(ui, &s.t("idm-username"));
                form::text_field(ui, &mut f.username, "alice");
                ui.end_row();
                form::label(ui, &s.t("idm-email"));
                form::text_field(ui, &mut f.email, "alice@example.com");
                ui.end_row();
                form::label(ui, &s.t("idm-first-name"));
                form::text_field(ui, &mut f.first_name, "");
                ui.end_row();
                form::label(ui, &s.t("idm-last-name"));
                form::text_field(ui, &mut f.last_name, "");
                ui.end_row();
                form::label(ui, &s.t("idm-enabled"));
                ui.checkbox(&mut f.enabled, "");
                ui.end_row();
                form::label(ui, &s.t("idm-email-verified"));
                ui.checkbox(&mut f.email_verified, "");
                ui.end_row();
            }
            Create::Group { name, .. } => {
                form::label(ui, &s.t("idm-name"));
                form::text_field(ui, name, "engineering");
                ui.end_row();
            }
            Create::Role { form: f, .. } => {
                form::label(ui, &s.t("idm-name"));
                form::text_field(ui, &mut f.name, "reports-reader");
                ui.end_row();
                form::label(ui, &s.t("idm-description"));
                form::text_field(ui, &mut f.description, "");
                ui.end_row();
            }
            Create::Client { form: f, .. } => {
                form::label(ui, &s.t("idm-client-id"));
                form::text_field(ui, &mut f.client_id, "my-service");
                ui.end_row();
                form::label(ui, &s.t("idm-name"));
                form::text_field(ui, &mut f.name, "");
                ui.end_row();
                form::label(ui, &s.t("idm-description"));
                form::text_field(ui, &mut f.description, "");
                ui.end_row();
                form::label(ui, &s.t("idm-protocol"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut f.protocol, "openid-connect".into(), "OpenID Connect");
                    ui.add_space(form::OPTION_GAP);
                    ui.radio_value(&mut f.protocol, "saml".into(), "SAML");
                });
                ui.end_row();
                let mut confidential = !f.public_client;
                form::label(ui, &s.t("idm-client-authentication"));
                if ui.checkbox(&mut confidential, "").changed() {
                    f.public_client = !confidential;
                    if f.public_client {
                        f.service_accounts = false;
                    }
                }
                ui.end_row();
                form::label(ui, &s.t("idm-service-accounts"));
                ui.add_enabled(
                    !f.public_client,
                    egui::Checkbox::new(&mut f.service_accounts, ""),
                );
                ui.end_row();
                form::label(ui, &s.t("idm-standard-flow"));
                ui.checkbox(&mut f.standard_flow, "");
                ui.end_row();
                form::label(ui, &s.t("idm-direct-access"));
                ui.checkbox(&mut f.direct_access_grants, "");
                ui.end_row();
                form::label(ui, &s.t("idm-redirect-uris"));
                let mut text = f.redirect_uris.join("\n");
                if form::code_field(ui, &mut text, 2).changed() {
                    f.redirect_uris = lines(&text);
                }
                ui.end_row();
                form::label(ui, &s.t("idm-web-origins"));
                let mut text = f.web_origins.join("\n");
                if form::code_field(ui, &mut text, 2).changed() {
                    f.web_origins = lines(&text);
                }
                ui.end_row();
                form::row_hint(ui, theme, &s.t("idm-one-per-line"));
            }
        });
        ui.add_space(form::ROW_GAP);
        let ready = match &c {
            Create::User { form: f, .. } => !f.username.trim().is_empty(),
            Create::Group { name, .. } => !name.trim().is_empty(),
            Create::Role { form: f, .. } => !f.name.trim().is_empty(),
            Create::Client { form: f, .. } => !f.client_id.trim().is_empty(),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled_ui(ready, |ui| {
                    form::primary_button(ui, theme, &format!("{} {}", ph::PLUS, s.t("idm-create")))
                })
                .inner
                .clicked()
            {
                go = true;
            }
            ui.add_space(form::BUTTON_GAP - ui.spacing().item_spacing.x);
            if form::button(ui, theme, ph::X, &s.t("cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if cancel || (modal.should_close() && !go) {
        return Answer::Closed;
    }
    if !go {
        return Answer::Open(Dialog::Create(c));
    }
    Answer::Send(match c {
        Create::User { realm, mut form } => {
            form.username = form.username.trim().to_string();
            let done = s
                .i18n
                .t_args("idm-done-created", &[("name", &form.username)]);
            WriteReq::new(done, After::Select(Section::Users), move |p| async move {
                p.create_user(&realm, &form).await.map(WriteOk::Created)
            })
        }
        Create::Group {
            realm,
            parent,
            name,
        } => {
            let name = name.trim().to_string();
            let done = s.i18n.t_args("idm-done-created", &[("name", &name)]);
            WriteReq::new(done, After::Select(Section::Groups), move |p| async move {
                p.create_group(&realm, parent.as_ref(), &name)
                    .await
                    .map(WriteOk::Created)
            })
        }
        Create::Role {
            realm,
            scope,
            mut form,
        } => {
            form.name = form.name.trim().to_string();
            let section = match scope {
                RoleScope::Realm => Section::RealmRoles,
                RoleScope::Client(_) => Section::ClientRoles,
            };
            let done = s.i18n.t_args("idm-done-created", &[("name", &form.name)]);
            WriteReq::new(done, After::Select(section), move |p| async move {
                p.create_role(&realm, &scope, &form)
                    .await
                    .map(|r| WriteOk::Created(r.id))
            })
        }
        Create::Client { realm, mut form } => {
            form.client_id = form.client_id.trim().to_string();
            let done = s
                .i18n
                .t_args("idm-done-created", &[("name", &form.client_id)]);
            WriteReq::new(done, After::Select(Section::Clients), move |p| async move {
                p.create_client(&realm, &form).await.map(WriteOk::Created)
            })
        }
    })
}

/// A one-per-line list, blank lines dropped.
pub fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::lines;

    #[test]
    fn a_list_field_drops_blank_lines() {
        assert_eq!(
            lines("https://a/*\n\n  https://b/cb \n"),
            vec!["https://a/*".to_string(), "https://b/cb".to_string()]
        );
    }
}
