//! Hash and HMAC of text or a file, every algorithm at once, with a
//! compare field.

use hmac::{Hmac, Mac};
use hs_core::Services;
use hs_ui::form;
use hs_ui::theme::Theme;
use md5::Digest;
use serde::{Deserialize, Serialize};

use crate::crypto::{b64_encode, hex_encode};
use crate::util;
use hs_ui::tip::TipExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Md5,
    Sha1,
    Sha256,
    Sha384,
    Sha512,
    Sha3_256,
    Sha3_512,
    Blake3,
    Crc32,
}

impl Algo {
    pub const ALL: [Algo; 9] = [
        Algo::Md5,
        Algo::Sha1,
        Algo::Sha256,
        Algo::Sha384,
        Algo::Sha512,
        Algo::Sha3_256,
        Algo::Sha3_512,
        Algo::Blake3,
        Algo::Crc32,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Algo::Md5 => "MD5",
            Algo::Sha1 => "SHA-1",
            Algo::Sha256 => "SHA-256",
            Algo::Sha384 => "SHA-384",
            Algo::Sha512 => "SHA-512",
            Algo::Sha3_256 => "SHA3-256",
            Algo::Sha3_512 => "SHA3-512",
            Algo::Blake3 => "BLAKE3",
            Algo::Crc32 => "CRC32",
        }
    }
    /// HMAC is defined for the Merkle–Damgård and SHA-3 hashes, not CRC32;
    /// BLAKE3 has its own keyed mode.
    pub fn supports_hmac(self) -> bool {
        !matches!(self, Algo::Crc32)
    }
}

pub fn digest(algo: Algo, data: &[u8]) -> Vec<u8> {
    match algo {
        Algo::Md5 => md5::Md5::digest(data).to_vec(),
        Algo::Sha1 => sha1::Sha1::digest(data).to_vec(),
        Algo::Sha256 => sha2::Sha256::digest(data).to_vec(),
        Algo::Sha384 => sha2::Sha384::digest(data).to_vec(),
        Algo::Sha512 => sha2::Sha512::digest(data).to_vec(),
        Algo::Sha3_256 => sha3::Sha3_256::digest(data).to_vec(),
        Algo::Sha3_512 => sha3::Sha3_512::digest(data).to_vec(),
        Algo::Blake3 => blake3::hash(data).as_bytes().to_vec(),
        Algo::Crc32 => crc32fast::hash(data).to_be_bytes().to_vec(),
    }
}

pub fn hmac(algo: Algo, key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    macro_rules! mac {
        ($h:ty) => {{
            let mut m = Hmac::<$h>::new_from_slice(key).ok()?;
            m.update(data);
            Some(m.finalize().into_bytes().to_vec())
        }};
    }
    match algo {
        Algo::Md5 => mac!(md5::Md5),
        Algo::Sha1 => mac!(sha1::Sha1),
        Algo::Sha256 => mac!(sha2::Sha256),
        Algo::Sha384 => mac!(sha2::Sha384),
        Algo::Sha512 => mac!(sha2::Sha512),
        Algo::Sha3_256 => mac!(sha3::Sha3_256),
        Algo::Sha3_512 => mac!(sha3::Sha3_512),
        Algo::Blake3 => {
            let mut k = [0u8; 32];
            // BLAKE3's keyed mode takes exactly 32 bytes: hash a longer key down.
            if key.len() == 32 {
                k.copy_from_slice(key);
            } else {
                k.copy_from_slice(blake3::hash(key).as_bytes());
            }
            Some(blake3::keyed_hash(&k, data).as_bytes().to_vec())
        }
        Algo::Crc32 => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Output {
    #[default]
    Hex,
    Base64,
}

/// One row per algorithm: the digest, or `None` when HMAC does not apply.
pub type DigestRows = Vec<(Algo, Option<Vec<u8>>)>;

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct State {
    pub input: String,
    /// A file chosen instead of the text, with its name.
    #[serde(skip)]
    pub file: Option<(std::path::PathBuf, Vec<u8>)>,
    pub hmac_key: String,
    pub use_hmac: bool,
    pub minify_json: bool,
    pub output: Output,
    pub expected: String,
    /// Digests of the current input, keyed by a fingerprint of what they
    /// were computed from, so a frame that changes nothing hashes nothing.
    #[serde(skip)]
    pub cache: Option<(u64, DigestRows)>,
}

impl State {
    /// Cheap identity of the input: the text (or the file's path and size),
    /// the options and the HMAC key as expanded.
    fn fingerprint(&self, hmac_key: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match &self.file {
            Some((path, bytes)) => {
                1u8.hash(&mut h);
                path.hash(&mut h);
                bytes.len().hash(&mut h);
            }
            None => {
                0u8.hash(&mut h);
                self.input.hash(&mut h);
            }
        }
        self.minify_json.hash(&mut h);
        self.use_hmac.hash(&mut h);
        hmac_key.hash(&mut h);
        h.finish()
    }

    pub fn for_save(&self) -> State {
        let mut c = self.clone();
        c.hmac_key = crate::keep_reference(&c.hmac_key);
        c
    }

    fn data(&self) -> Vec<u8> {
        if let Some((_, bytes)) = &self.file {
            return bytes.clone();
        }
        if self.minify_json
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&self.input)
            && let Ok(min) = serde_json::to_string(&v)
        {
            return min.into_bytes();
        }
        self.input.as_bytes().to_vec()
    }
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::page_title(ui, theme, &s.t("dt-hash"), Some(&s.t("dt-hash-sub")));
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
                util::input(ui, s, theme, &mut st.input, &s.t("dt-hash-input-hint"), 5);
                ui.horizontal(|ui| {
                    if ui
                        .button(format!(
                            "{} {}",
                            hs_ui::icons::ph::FOLDER_OPEN,
                            s.t("dt-hash-file")
                        ))
                        .clicked()
                        && let Some(p) = rfd::FileDialog::new().pick_file()
                    {
                        match std::fs::read(&p) {
                            Ok(bytes) => st.file = Some((p, bytes)),
                            Err(e) => tracing::warn!("hash file: {e}"),
                        }
                    }
                    ui.checkbox(&mut st.minify_json, s.t("dt-hash-minify"));
                });
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut st.use_hmac, s.t("dt-hash-hmac"));
            if st.use_hmac {
                util::secret_line(ui, s, theme, &mut st.hmac_key, &s.t("dt-hash-hmac-key"));
            }
            ui.separator();
            ui.selectable_value(&mut st.output, Output::Hex, "hex");
            ui.selectable_value(&mut st.output, Output::Base64, "Base64");
        });
    });
    form::section(ui, theme, &s.t("dt-hash-results"), |ui| {
        ui.horizontal(|ui| {
            ui.label(s.t("dt-hash-compare"));
            ui.add(
                egui::TextEdit::singleline(&mut st.expected)
                    .hint_text(s.t("dt-hash-compare-hint"))
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(6.0);
        let expected = st.expected.trim().to_ascii_lowercase();
        let hmac_key = if st.use_hmac {
            util::expand(s, &st.hmac_key).unwrap_or_default()
        } else {
            String::new()
        };
        let fp = st.fingerprint(&hmac_key);
        if st.cache.as_ref().map(|(k, _)| *k) != Some(fp) {
            let data = st.data();
            let rows = Algo::ALL
                .into_iter()
                .map(|algo| {
                    let bytes = if st.use_hmac {
                        hmac(algo, hmac_key.as_bytes(), &data)
                    } else {
                        Some(digest(algo, &data))
                    };
                    (algo, bytes)
                })
                .collect();
            st.cache = Some((fp, rows));
        }
        let rows: Vec<(Algo, Option<Vec<u8>>)> = st
            .cache
            .as_ref()
            .map(|(_, r)| r.clone())
            .unwrap_or_default();
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::exact(110.0))
            .column(egui_extras::Column::remainder().clip(true))
            .column(egui_extras::Column::exact(44.0))
            .column(egui_extras::Column::exact(84.0))
            .header(26.0, |mut h| {
                h.col(|ui| {
                    ui.label(egui::RichText::new(s.t("dt-hash-col-algorithm")).strong());
                });
                h.col(|ui| {
                    ui.label(egui::RichText::new(s.t("dt-hash-col-digest")).strong());
                });
                h.col(|ui| {
                    ui.label(egui::RichText::new(s.t("dt-hash-col-bits")).strong());
                });
                h.col(|ui| {
                    ui.label(egui::RichText::new(s.t("dt-hash-col-actions")).strong());
                });
            })
            .body(|mut body| {
                for (algo, bytes) in rows {
                    body.row(28.0, |mut row| {
                        row.col(|ui| {
                            ui.label(egui::RichText::new(algo.name()).strong());
                        });
                        match bytes {
                            Some(b) => {
                                let text = match st.output {
                                    Output::Hex => hex_encode(&b),
                                    Output::Base64 => b64_encode(&b),
                                };
                                let matches = !expected.is_empty()
                                    && (expected == text.to_ascii_lowercase()
                                        || expected == hex_encode(&b)
                                        || st.expected.trim() == b64_encode(&b));
                                row.col(|ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&text).monospace().color(
                                                if matches { theme.success } else { theme.text },
                                            ),
                                        )
                                        .truncate()
                                        .selectable(true),
                                    )
                                    .tip(&text);
                                });
                                row.col(|ui| {
                                    ui.label(
                                        egui::RichText::new((b.len() * 8).to_string())
                                            .color(theme.text_muted),
                                    );
                                });
                                row.col(|ui| {
                                    if ui
                                        .small_button(hs_ui::icons::COPY)
                                        .tip(s.t("dt-copy"))
                                        .clicked()
                                    {
                                        ui.ctx().copy_text(text.clone());
                                    }
                                    if matches {
                                        ui.colored_label(theme.success, hs_ui::icons::CHECK);
                                    }
                                });
                            }
                            None => {
                                row.col(|ui| {
                                    ui.colored_label(theme.text_muted, s.t("dt-hash-no-hmac"));
                                });
                                row.col(|_| {});
                                row.col(|_| {});
                            }
                        }
                    });
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_answers() {
        assert_eq!(
            hex_encode(&digest(Algo::Md5, b"")),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            hex_encode(&digest(Algo::Sha256, b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hex_encode(&digest(Algo::Crc32, b"123456789")), "cbf43926");
        assert_eq!(
            hex_encode(
                &hmac(
                    Algo::Sha256,
                    b"key",
                    b"The quick brown fox jumps over the lazy dog"
                )
                .unwrap()
            ),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
        assert!(hmac(Algo::Crc32, b"k", b"d").is_none());
    }
}
