//! The microphone toggle beside an assistant's Send button: speech to
//! text through `hs_core::stt`, typed into the input as phrases arrive.
//! Nothing is sent to the model until the user presses Send.

use hs_core::Services;
use hs_core::stt::Stt;

use crate::icons::ph;
use crate::theme::Theme;
use crate::tip::TipExt;

/// Draw the toggle (when the setting allows it and an engine exists) and
/// move what was heard into `input`. Returns an error to show, if the
/// engine stopped on its own.
pub fn mic_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    stt: &Stt,
    input: &mut String,
) -> Option<String> {
    let (enabled, language, max_secs) = {
        let a = &s.settings.read().accessibility;
        (a.stt_enabled, a.stt_language.clone(), a.stt_max_secs)
    };
    if !enabled || !Stt::available() {
        return None;
    }
    let listening = stt.is_listening();
    let active = stt.is_active();
    let mut error = None;
    let label = if listening {
        let left = stt.remaining_secs().unwrap_or(0);
        egui::RichText::new(format!(
            "{} {} {left}s",
            ph::MICROPHONE,
            s.t("ai-listening")
        ))
        .color(theme.error)
    } else if active {
        egui::RichText::new(format!("{} {}", ph::MICROPHONE, s.t("ai-mic-finishing")))
            .color(theme.text_muted)
    } else {
        egui::RichText::new(ph::MICROPHONE_SLASH).color(theme.text_muted)
    };
    let r = ui
        .add_enabled(
            listening || !active,
            egui::Button::selectable(listening, label),
        )
        .tip(s.t(if listening {
            "ai-mic-stop-tip"
        } else {
            "ai-mic-tip"
        }));
    if r.clicked() {
        if listening {
            stt.stop();
        } else if let Err(e) = stt.start(&language, max_secs) {
            error = Some(e);
        }
    }
    if active {
        // Phrases arrive from a thread (also while a stop drains the last
        // one); poll until the session is reaped.
        for phrase in stt.take_heard() {
            if !input.is_empty() && !input.ends_with(char::is_whitespace) {
                input.push(' ');
            }
            input.push_str(&phrase);
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(150));
    }
    if let Some(e) = stt.take_error() {
        error = Some(s.i18n.t_args("ai-mic-error", &[("error", &e)]));
    }
    error
}
