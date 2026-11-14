//! What every section of the tab shares (docs/tools/idm.md §3.3, §4.10):
//! a read's three states, the paging line, the sections' marks, and the
//! queue that writes go through.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hs_core::Services;
use hs_core::tool::ToastLevel;
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;

use crate::guarded::Guarded;
use crate::provider::{Availability, Caps, IdmError, LogoutAll, Page, Paged, Secret, Section};
use crate::runner::Session;

/// A read the screen is waiting for or showing.
#[derive(Debug, Clone, Default)]
pub enum Load<T> {
    /// Not asked for yet.
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(IdmError),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(v) => Some(v),
            _ => None,
        }
    }

    pub fn ready_mut(&mut self) -> Option<&mut T> {
        match self {
            Load::Ready(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Load::Idle)
    }
}

impl<T> From<crate::provider::Result<T>> for Load<T> {
    fn from(r: crate::provider::Result<T>) -> Self {
        match r {
            Ok(v) => Load::Ready(v),
            Err(e) => Load::Failed(e),
        }
    }
}

/// Take the newest answer of `topic` into `slot`, once it has come.
pub fn take<T: Send + 'static>(session: &Session, topic: &'static str, slot: &mut Load<T>) {
    if let Some(r) = session.take::<crate::provider::Result<T>>(topic) {
        *slot = r.into();
    }
}

/// Queue a read under `topic` and mark `slot` loading. A full queue
/// leaves the slot as it was, to be asked again next frame.
pub fn ask<T, F, Fut>(session: &mut Session, topic: &'static str, slot: &mut Load<T>, work: F)
where
    T: Send + 'static,
    F: FnOnce(Arc<Guarded>) -> Fut + Send + 'static,
    Fut: Future<Output = crate::provider::Result<T>> + Send + 'static,
{
    if session.submit(topic, work).is_ok() {
        *slot = Load::Loading;
    }
}

/// What a finished write hands back to the tab.
pub enum WriteOk {
    Done,
    /// The id of what was created, to select it.
    Created(String),
    /// A regenerated client secret, shown masked.
    Secret(Secret),
    LogoutAll(LogoutAll),
}

/// What the tab re-reads after a write (§4.10: every write ends with a
/// re-read of what it changed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum After {
    /// The section's list and its open item.
    Reload(Section),
    /// The section's list, then select the created item.
    Select(Section),
    /// The section's list; the item it showed is gone.
    Gone(Section),
}

type WriteFuture = Pin<Box<dyn Future<Output = crate::provider::Result<WriteOk>> + Send>>;
pub type WriteFn = Box<dyn FnOnce(Arc<Guarded>) -> WriteFuture + Send>;

/// One write, ready to send once its dialog (if any) is answered.
pub struct WriteReq {
    /// The toast on success, in words ("Disabled alice.").
    pub done: String,
    pub run: WriteFn,
    pub after: After,
}

impl WriteReq {
    pub fn new<F, Fut>(done: String, after: After, run: F) -> WriteReq
    where
        F: FnOnce(Arc<Guarded>) -> Fut + Send + 'static,
        Fut: Future<Output = crate::provider::Result<WriteOk>> + Send + 'static,
    {
        WriteReq {
            done,
            run: Box::new(move |p| Box::pin(run(p))),
            after,
        }
    }
}

/// A write that came back.
pub struct Done {
    pub done: String,
    pub after: After,
    pub result: crate::provider::Result<WriteOk>,
}

/// Writes in flight. They do not go through the session's topics: a
/// newer write must never abort an older one half-way.
#[derive(Default, Clone)]
pub struct Writes {
    finished: Arc<Mutex<Vec<Done>>>,
    running: Arc<Mutex<usize>>,
}

impl Writes {
    pub fn send(&self, s: &Services, provider: Arc<Guarded>, req: WriteReq) {
        let finished = self.finished.clone();
        let running = self.running.clone();
        *running.lock() += 1;
        let waker = s.runtime.waker();
        let WriteReq { done, run, after } = req;
        s.runtime.spawn(async move {
            let result = run(provider).await;
            finished.lock().push(Done {
                done,
                after,
                result,
            });
            *running.lock() -= 1;
            waker.request_repaint();
        });
    }

    pub fn drain(&self) -> Vec<Done> {
        std::mem::take(&mut *self.finished.lock())
    }
}

/// What a section asks of the tab this frame.
#[derive(Default)]
pub struct Out {
    pub dialog: Option<super::dialogs::Dialog>,
    pub writes: Vec<WriteReq>,
    pub toasts: Vec<(ToastLevel, String)>,
    /// An admin console page (`#/{realm}/…` after the console's base).
    pub console: Option<String>,
    pub open: Vec<hs_core::tool::OpenRequest>,
}

/// What a section draws with.
pub struct Cx<'a> {
    pub s: &'a Services,
    pub theme: &'a Theme,
    pub session: &'a mut Session,
    pub realm: &'a str,
    pub caps: &'a Caps,
    /// The page size from Settings → Identity.
    pub page: Page,
    /// Why writes are refused on this tab, when they are (§6.1).
    pub refusal: Option<String>,
    pub out: &'a mut Out,
}

impl Cx<'_> {
    /// Whether a write that needs `role` can be offered, and if not why
    /// (the read-only tab first, then the account's roles).
    pub fn may_write(&self, role: &str) -> Result<(), String> {
        if let Some(r) = &self.refusal {
            return Err(r.clone());
        }
        match self.caps.write(self.realm, role) {
            Availability::Yes => Ok(()),
            Availability::No(why) => Err(hs_core::fmt::sentence(&why)),
        }
    }

    pub fn t(&self, key: &str) -> String {
        self.s.t(key)
    }

    pub fn ta(&self, key: &str, args: &[(&str, &str)]) -> String {
        self.s.i18n.t_args(key, args)
    }
}

/// A button for a write: enabled only when `may` is Ok and `enabled`,
/// with the reason it is not as its tooltip (never hidden, §3.3).
pub fn write_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    may: &Result<(), String>,
    enabled: bool,
    icon: &str,
    label: &str,
    disabled_tip: &str,
) -> bool {
    let on = may.is_ok() && enabled;
    let r = ui
        .add_enabled_ui(on, |ui| form::button(ui, theme, icon, label))
        .inner;
    let tip = match may {
        Err(why) => why.clone(),
        Ok(()) => disabled_tip.to_string(),
    };
    r.tip_disabled(tip).clicked()
}

/// The paging line under a list (§3.3): "1–50 of 1,284", Previous and
/// Next, or More where the server gives no count. Returns the page to
/// read, when one was asked for.
pub fn paging<T>(ui: &mut egui::Ui, theme: &Theme, s: &Services, p: &Paged<T>) -> Option<Page> {
    let mut go = None;
    ui.add_space(form::PANE_GAP);
    form::centred_row(ui, |ui| {
        let (from, to, total) = p.span();
        let text = match total {
            Some(t) => s.i18n.t_args(
                "idm-paging-of",
                &[
                    ("from", &from.to_string()),
                    ("to", &to.to_string()),
                    ("total", &group(t)),
                ],
            ),
            None => s.i18n.t_args(
                "idm-paging",
                &[("from", &from.to_string()), ("to", &to.to_string())],
            ),
        };
        ui.label(egui::RichText::new(text).color(theme.text_muted));
        ui.add_space(form::BUTTON_GAP);
        let prev = p.page.previous();
        if ui
            .add_enabled_ui(prev.is_some(), |ui| {
                form::button(ui, theme, ph::CARET_LEFT, &s.t("idm-previous"))
            })
            .inner
            .clicked()
        {
            go = prev;
        }
        let label = if total.is_some() {
            s.t("idm-next")
        } else {
            s.t("idm-more")
        };
        if ui
            .add_enabled_ui(p.more, |ui| {
                form::button(ui, theme, ph::CARET_RIGHT, &label)
            })
            .inner
            .clicked()
        {
            go = Some(p.page.next());
        }
    });
    go
}

/// 1284 → "1,284".
pub fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A read's state drawn where its content goes: a spinner, the error, or
/// nothing once it is ready (the caller draws that).
pub fn state_note<T>(ui: &mut egui::Ui, theme: &Theme, s: &Services, load: &Load<T>) -> bool {
    match load {
        Load::Idle | Load::Loading => {
            form::centred_row(ui, |ui| {
                ui.spinner();
                ui.label(s.t("idm-loading"));
            });
            false
        }
        Load::Failed(e) => {
            form::note(
                ui,
                theme,
                ph::WARNING,
                &hs_core::fmt::sentence(&e.to_string()),
            );
            false
        }
        Load::Ready(_) => true,
    }
}

/// A section's picture and colour, the same in the browse column, the
/// item page's header and the palette (§3.6).
pub fn section_mark(section: Section) -> (&'static str, egui::Color32) {
    let (glyph, rgb) = match section {
        Section::Users => (ph::USER, [0x24, 0x96, 0xed]),
        Section::Groups => (ph::USERS_THREE, [0x8e, 0x6c, 0xef]),
        Section::RealmRoles => (ph::SHIELD_STAR, [0xef, 0x6c, 0x00]),
        Section::ClientRoles => (ph::SHIELD, [0xf9, 0xa8, 0x25]),
        Section::Clients => (ph::APP_WINDOW, [0x26, 0xa6, 0x9a]),
        Section::Sessions => (ph::CLOCK, [0x43, 0xa0, 0x47]),
        Section::LoginEvents => (ph::SIGN_IN, [0xd8, 0x1b, 0x60]),
        Section::AdminEvents => (ph::LIST_CHECKS, [0x5c, 0x6b, 0xc0]),
        Section::RealmSettings => (ph::GEAR_SIX, [0x78, 0x90, 0x9c]),
    };
    (glyph, egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
}

pub fn section_key(section: Section) -> &'static str {
    match section {
        Section::Users => "idm-section-users",
        Section::Groups => "idm-section-groups",
        Section::RealmRoles => "idm-section-realm-roles",
        Section::ClientRoles => "idm-section-client-roles",
        Section::Clients => "idm-section-clients",
        Section::Sessions => "idm-section-sessions",
        Section::LoginEvents => "idm-section-login-events",
        Section::AdminEvents => "idm-section-admin-events",
        Section::RealmSettings => "idm-section-realm-settings",
    }
}

/// One row of the item-kind column (CR-0074): a 20 px glyph in the
/// section's colour, the name in body text, a 30 px row.
pub fn rail_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    section: Section,
    name: &str,
) -> egui::Response {
    let (glyph, colour) = section_mark(section);
    let body = egui::TextStyle::Body.resolve(ui.style());
    let fmt = |font: egui::FontId, color: egui::Color32| egui::TextFormat {
        font_id: font,
        color,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(glyph, 0.0, fmt(egui::FontId::proportional(20.0), colour));
    job.append("  ", 0.0, fmt(body.clone(), theme.text));
    job.append(name, 0.0, fmt(body, theme.text));
    form::add_selectable(
        ui,
        egui::Button::selectable(selected, job).min_size(egui::vec2(ui.available_width(), 30.0)),
    )
}

/// A coloured chip of state ("Enabled", "Disabled", "Temporary").
pub fn chip(ui: &mut egui::Ui, text: &str, colour: egui::Color32) {
    hs_ui::badge::badge(ui, text, colour.linear_multiply(0.25), colour);
}

/// An epoch-milliseconds time as local "2026-11-08 10:12", or "" for none.
pub fn when(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The item page's empty state: "Select a user to see it here."
pub fn nothing_selected(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    form::pane(ui, |ui| form::note(ui, theme, ph::INFO, text));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_in_thousands() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(1284), "1,284");
        assert_eq!(group(1_000_000), "1,000,000");
    }

    #[test]
    fn a_time_of_zero_is_blank() {
        assert_eq!(when(0), "");
        assert!(when(1_762_600_000_000).starts_with("2025-"));
    }
}
