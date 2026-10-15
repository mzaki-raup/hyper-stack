//! The microphone toggle beside an assistant's Send button: speech to
//! text through `hs_core::stt`, typed into the input as phrases arrive.
//! Nothing is sent to the model until the user presses Send.

use hs_core::Services;
use hs_core::stt::Stt;

use crate::icons::ph;
use crate::theme::Theme;
use crate::tip::TipExt;

/// Draw the toggle (when the setting allows it and an engine exists) and
/// move what was heard into `input`. Returns an error to toast, if the
/// engine stopped on its own.
pub fn mic_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    stt: &Stt,
    input: &mut String,
) -> Option<String> {
    let (enabled, language) = {
        let a = &s.settings.read().accessibility;
        (a.stt_enabled, a.stt_language.clone())
    };
    if !enabled || !Stt::available() {
        return None;
    }
    let listening = stt.is_listening();
    let mut error = None;
    let label = if listening {
        egui::RichText::new(format!("{} {}", ph::MICROPHONE, s.t("ai-listening")))
            .color(theme.error)
    } else {
        egui::RichText::new(ph::MICROPHONE_SLASH).color(theme.text_muted)
    };
    let r = ui.selectable_label(listening, label).tip(s.t(if listening {
        "ai-mic-stop-tip"
    } else {
        "ai-mic-tip"
    }));
    if r.clicked() {
        if listening {
            stt.stop();
        } else if let Err(e) = stt.start(&language) {
            error = Some(e);
        }
    }
    if listening {
        for phrase in stt.take_heard() {
            if !input.is_empty() && !input.ends_with(char::is_whitespace) {
                input.push(' ');
            }
            input.push_str(&phrase);
        }
        // Phrases arrive from a thread; poll while listening.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(150));
    }
    if let Some(e) = stt.take_error() {
        error = Some(s.i18n.t_args("ai-mic-error", &[("error", &e)]));
    }
    error
}
