//! Export Realm (§4.9): what to include, then a partial export as a Job
//! with cancel; the JSON read-only, already masked, with Save As, Open in
//! Editor and Copy.

use std::sync::Arc;

use hs_core::jobs::JobHandle;
use hs_ui::form;
use hs_ui::icons::ph;
use parking_lot::Mutex;

use super::common::Cx;
use crate::provider::{ExportScope, IdmProvider};

type Slot = Arc<Mutex<Option<Result<String, String>>>>;

#[derive(Default)]
pub struct ExportView {
    /// The scope modal is up.
    asking: bool,
    clients: bool,
    groups_and_roles: bool,
    job: Option<JobHandle>,
    slot: Slot,
    json: Option<String>,
    error: Option<String>,
    realm: String,
}

impl ExportView {
    /// Whether the export takes the work pane (asking, running or shown).
    pub fn open(&self) -> bool {
        self.asking || self.job.is_some() || self.json.is_some() || self.error.is_some()
    }

    /// Export Realm…: ask what to include (both on, §4.9).
    pub fn start(&mut self) {
        if self.job.is_none() {
            self.asking = true;
            self.clients = true;
            self.groups_and_roles = true;
        }
    }

    fn close(&mut self) {
        if let Some(j) = self.job.take() {
            j.cancel_token().cancel();
        }
        *self = ExportView::default();
    }

    pub fn show(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let theme = cx.theme;
        if let Some(r) = self.slot.lock().take() {
            self.job = None;
            match r {
                Ok(json) => self.json = Some(json),
                Err(e) => self.error = Some(e),
            }
        }
        if self.asking {
            self.ask(ui, cx);
        }
        form::pane(ui, |ui| {
            form::centred_row(ui, |ui| {
                ui.label(
                    egui::RichText::new(ph::DOWNLOAD_SIMPLE)
                        .size(20.0)
                        .color(theme.accent),
                );
                ui.label(
                    egui::RichText::new(cx.ta("idm-export-title", &[("realm", cx.realm)]))
                        .strong()
                        .size(16.0),
                );
            });
            ui.add_space(form::PANE_GAP);
            if self.job.is_some() {
                form::centred_row(ui, |ui| {
                    ui.spinner();
                    ui.label(cx.t("idm-exporting"));
                    if form::button(ui, theme, ph::X, &cx.t("cancel")).clicked() {
                        self.close();
                    }
                });
                return;
            }
            if let Some(e) = self.error.clone() {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                ui.add_space(form::ROW_GAP);
                if form::button(ui, theme, ph::X, &cx.t("idm-close")).clicked() {
                    self.close();
                }
                return;
            }
            let Some(json) = self.json.clone() else {
                return;
            };
            ui.horizontal_wrapped(|ui| {
                if form::button(ui, theme, ph::FLOPPY_DISK, &cx.t("idm-save-as")).clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name(format!("{}-realm.json", self.realm))
                        .add_filter("JSON", &["json"])
                        .save_file()
                {
                    let body = json.clone();
                    let waker = cx.s.runtime.waker();
                    // Written off the UI thread.
                    cx.s.runtime.spawn_blocking(move || {
                        if let Err(e) = std::fs::write(&path, body) {
                            tracing::warn!("idm: export not written: {e}");
                        }
                        waker.request_repaint();
                    });
                    cx.out
                        .toasts
                        .push((hs_core::tool::ToastLevel::Success, cx.t("idm-export-saved")));
                }
                if form::button(ui, theme, ph::ARROW_SQUARE_OUT, &cx.t("idm-open-in-editor"))
                    .clicked()
                {
                    cx.out.open.push(hs_core::tool::OpenRequest::Text {
                        mime: "application/json".into(),
                        body: json.clone(),
                    });
                }
                if form::button(ui, theme, ph::COPY, &cx.t("idm-copy")).clicked() {
                    ui.ctx().copy_text(json.clone());
                    cx.out
                        .toasts
                        .push((hs_core::tool::ToastLevel::Info, cx.t("idm-copied")));
                }
                if form::button(ui, theme, ph::X, &cx.t("idm-close")).clicked() {
                    self.close();
                }
            });
            ui.add_space(form::PANE_GAP);
            form::hint(ui, theme, &cx.t("idm-export-no-users"));
            ui.add_space(form::PANE_GAP);
            let h = ui.available_height().max(200.0);
            form::scroll_well(ui, theme, "idm-export-json", Some(h), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&json).monospace()).wrap());
            });
        });
    }

    fn ask(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let theme = cx.theme;
        let mut go = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("idm-export-ask")).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.heading(cx.ta("idm-export-title", &[("realm", cx.realm)]));
            ui.add_space(form::PANE_GAP);
            ui.checkbox(&mut self.clients, cx.t("idm-export-clients"));
            ui.add_space(form::ROW_GAP);
            ui.checkbox(&mut self.groups_and_roles, cx.t("idm-export-groups-roles"));
            ui.add_space(form::ROW_GAP);
            form::hint(ui, theme, &cx.t("idm-export-hint"));
            ui.add_space(form::ROW_GAP);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::primary_button(
                    ui,
                    theme,
                    &format!("{} {}", ph::DOWNLOAD_SIMPLE, cx.t("idm-export")),
                )
                .clicked()
                {
                    go = true;
                }
                ui.add_space(form::BUTTON_GAP - ui.spacing().item_spacing.x);
                if form::button(ui, theme, ph::X, &cx.t("cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if cancel || (modal.should_close() && !go) {
            self.close();
            return;
        }
        if !go {
            return;
        }
        self.asking = false;
        let provider = cx.session.provider();
        let realm = cx.realm.to_string();
        self.realm = realm.clone();
        let scope = ExportScope {
            clients: self.clients,
            groups_and_roles: self.groups_and_roles,
        };
        let slot = self.slot.clone();
        let waker = cx.s.runtime.waker();
        let title = cx.ta("idm-export-job", &[("realm", &realm)]);
        self.job = Some(cx.s.jobs.spawn(title, "idm", move |h| async move {
            let r = provider
                .export(&realm, scope, h.cancel_token())
                .await
                .map_err(|e| e.to_string());
            let out = r.clone().map(|_| ());
            *slot.lock() = Some(r);
            waker.request_repaint();
            out
        }));
    }
}
