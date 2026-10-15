//! Base64: encode text or a file, decode anything with auto-detection of
//! the flavour, preview what came out (picture, text, document) and save
//! it; plus the Basic authentication header and data URI helpers.

use hs_core::Services;
use hs_core::sniff::{self, Kind};
use hs_ui::form;
use hs_ui::theme::Theme;
use serde::{Deserialize, Serialize};

use crate::crypto::{b64_decode, b64_encode, b64url_encode};
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Decode,
    Encode,
    BasicAuth,
}

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct State {
    pub mode: Mode,
    pub input: String,
    #[serde(skip)]
    pub file: Option<(std::path::PathBuf, Vec<u8>)>,
    pub url_safe: bool,
    pub no_padding: bool,
    pub user: String,
    pub pass: String,
    /// Decoded bytes of the last input, so the preview is not re-decoded
    /// every frame.
    #[serde(skip)]
    decoded: Option<(u64, Result<Vec<u8>, String>)>,
}

impl State {
    /// What is saved with the tab: secrets only as `{{references}}`.
    pub fn for_save(&self) -> State {
        let mut c = self.clone();
        c.pass = crate::keep_reference(&c.pass);
        c
    }
}

fn hash_of(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::page_title(ui, theme, &s.t("dt-base64"), Some(&s.t("dt-base64-sub")));
    ui.horizontal(|ui| {
        ui.selectable_value(&mut st.mode, Mode::Decode, s.t("dt-base64-decode"));
        ui.selectable_value(&mut st.mode, Mode::Encode, s.t("dt-base64-encode"));
        ui.selectable_value(&mut st.mode, Mode::BasicAuth, s.t("dt-base64-basic"));
    });
    ui.add_space(8.0);
    match st.mode {
        Mode::Decode => decode(st, ui, s, theme),
        Mode::Encode => encode(st, ui, s, theme),
        Mode::BasicAuth => basic(st, ui, s, theme),
    }
}

fn decode(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::section(ui, theme, &s.t("dt-input"), |ui| {
        util::input(
            ui,
            s,
            theme,
            &mut st.input,
            &s.t("dt-base64-decode-hint"),
            6,
        );
        form::hint(ui, theme, &s.t("dt-base64-auto-hint"));
    });
    let key = hash_of(&st.input);
    if st.decoded.as_ref().map(|(k, _)| *k) != Some(key) {
        let text = st.input.trim();
        // A data URI pastes straight in.
        let body = text
            .strip_prefix("data:")
            .and_then(|r| r.split_once(","))
            .map(|(_, b)| b)
            .unwrap_or(text);
        st.decoded = Some((
            key,
            if body.is_empty() {
                Err(String::new())
            } else {
                b64_decode(body)
            },
        ));
    }
    let Some((_, decoded)) = &st.decoded else {
        return;
    };
    form::section(ui, theme, &s.t("dt-base64-decoded"), |ui| match decoded {
        Err(e) if e.is_empty() => {
            form::hint(ui, theme, &s.t("dt-base64-empty"));
        }
        Err(e) => util::error(ui, theme, e),
        Ok(bytes) => {
            let sn = sniff::sniff(bytes);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {} · {}",
                        sn.label,
                        sn.mime,
                        s.i18n
                            .t_args("dt-bytes", &[("count", &bytes.len().to_string())])
                    ))
                    .color(theme.text_muted),
                );
                if ui
                    .button(format!(
                        "{} {}",
                        hs_ui::icons::ph::FLOPPY_DISK,
                        s.t("dt-save-as")
                    ))
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name(format!("decoded.{}", sn.ext))
                        .add_filter(sn.label, &[sn.ext])
                        .save_file()
                    && let Err(e) = std::fs::write(&path, bytes)
                {
                    tracing::warn!("save decoded: {e}");
                }
            });
            ui.add_space(6.0);
            match sn.kind {
                Kind::Image | Kind::Svg => {
                    let uri = format!("bytes://b64-{}.{}", hash_of(&st.input), sn.ext);
                    ui.add(
                        egui::Image::from_bytes(uri, bytes.clone())
                            .max_size(egui::vec2(640.0, 420.0))
                            .corner_radius(4.0),
                    );
                    ui.add_space(6.0);
                }
                Kind::Text => {
                    let text = String::from_utf8_lossy(bytes).into_owned();
                    // A decoded payload is very often JSON or XML on one
                    // line: lay it out so it can be read.
                    let shown = hs_core::fmt::pretty_auto(&text).unwrap_or(text);
                    util::output(ui, s, theme, &s.t("dt-base64-text"), &shown, 10);
                }
                _ => {}
            }
            egui::CollapsingHeader::new(s.t("dt-base64-hex"))
                .default_open(sn.kind == Kind::Binary)
                .show(ui, |ui| {
                    util::output(
                        ui,
                        s,
                        theme,
                        &s.t("dt-base64-hex"),
                        &sniff::hex_dump(bytes, 512),
                        10,
                    );
                });
        }
    });
}

fn encode(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::section(ui, theme, &s.t("dt-input"), |ui| {
        let file_info = st
            .file
            .as_ref()
            .map(|(p, b)| (p.display().to_string(), b.len()));
        match file_info {
            Some((path, len)) => {
                ui.horizontal(|ui| {
                    ui.label(format!("{} {path} ({len} bytes)", hs_ui::icons::ph::FILE));
                    if ui.button(s.t("dt-use-text-instead")).clicked() {
                        st.file = None;
                    }
                });
            }
            None => {
                util::input(
                    ui,
                    s,
                    theme,
                    &mut st.input,
                    &s.t("dt-base64-encode-hint"),
                    6,
                );
                if ui
                    .button(format!(
                        "{} {}",
                        hs_ui::icons::ph::FOLDER_OPEN,
                        s.t("dt-base64-file")
                    ))
                    .clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_file()
                {
                    match std::fs::read(&p) {
                        Ok(bytes) => st.file = Some((p, bytes)),
                        Err(e) => tracing::warn!("read file: {e}"),
                    }
                }
            }
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut st.url_safe, s.t("dt-base64-url-safe"));
            ui.checkbox(&mut st.no_padding, s.t("dt-base64-no-padding"));
        });
    });
    let data: Vec<u8> = match &st.file {
        Some((_, b)) => b.clone(),
        None => st.input.as_bytes().to_vec(),
    };
    if data.is_empty() {
        return;
    }
    let mut encoded = if st.url_safe {
        b64url_encode(&data)
    } else {
        b64_encode(&data)
    };
    if st.url_safe && !st.no_padding {
        while encoded.len() % 4 != 0 {
            encoded.push('=');
        }
    } else if st.no_padding {
        encoded = encoded.trim_end_matches('=').to_string();
    }
    form::section(ui, theme, &s.t("dt-base64-encoded"), |ui| {
        util::output(ui, s, theme, "Base64", &encoded, 6);
        let sn = sniff::sniff(&data);
        let data_uri = format!("data:{};base64,{}", sn.mime, b64_encode(&data));
        util::output(ui, s, theme, &s.t("dt-base64-data-uri"), &data_uri, 3);
    });
}

fn basic(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::section(ui, theme, &s.t("dt-base64-basic"), |ui| {
        form::field_grid(ui, "basic-auth", |ui| {
            form::label(ui, &s.t("dt-base64-user"));
            util::line(ui, s, theme, &mut st.user, "");
            ui.end_row();
            form::label(ui, &s.t("dt-base64-pass"));
            util::secret_line(ui, s, theme, &mut st.pass, "");
            ui.end_row();
        });
        if !st.user.is_empty() {
            let user = util::expand(s, &st.user).unwrap_or_default();
            let pass = util::expand(s, &st.pass).unwrap_or_default();
            let token = b64_encode(format!("{user}:{pass}").as_bytes());
            ui.add_space(6.0);
            util::output(ui, s, theme, "Authorization", &format!("Basic {token}"), 1);
            util::output(
                ui,
                s,
                theme,
                "curl",
                &format!("-H \"Authorization: Basic {token}\""),
                1,
            );
        }
        form::hint(ui, theme, &s.t("dt-base64-basic-hint"));
    });
}
