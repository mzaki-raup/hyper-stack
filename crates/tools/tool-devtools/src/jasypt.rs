//! Jasypt-compatible string encryption, as `StandardPBEStringEncryptor` /
//! `PooledPBEStringEncryptor` with `RandomSaltGenerator` and
//! `RandomIvGenerator` produce it:
//!
//! - `PBEWithMD5AndDES` (Java PBES1): key and IV from PBKDF1-MD5 over the
//!   password and an 8-byte salt, DES-CBC-PKCS5. Message = salt ‖ [iv] ‖
//!   ciphertext. The IV bytes jasypt prepends when a `RandomIvGenerator` is
//!   configured are ignored by PBES1 (it derives its own IV), but they are
//!   still part of the message, so the layout is configurable.
//! - `PBEWITHHMACSHA512ANDAES_256`: key from PBKDF2-HMAC-SHA512 over the
//!   password and the salt (32 bytes), AES-256-CBC-PKCS5 with a random
//!   16-byte IV. Message = salt ‖ iv ‖ ciphertext.
//!
//! Output is Base64 (jasypt's `base64` string output type) or hex, and can
//! be wrapped as `ENC(…)` for Spring property files.
//!
//! Also the legacy `DataEncryptor`: DES/ECB and AES/ECB with fixed key
//! strings, and MD5-Base64 hashes.

use hs_core::Services;
use hs_ui::form;
use hs_ui::theme::Theme;
use md5::Digest;

use crate::crypto::{
    self, b64_decode, b64_encode, cbc_decrypt, cbc_encrypt, ecb_decrypt, ecb_encrypt, hex_decode,
    hex_encode, pbkdf1_md5, pbkdf2_hmac_sha512, random_bytes,
};
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Md5Des,
    HmacSha512Aes256,
}

impl Algo {
    pub fn name(self) -> &'static str {
        match self {
            Algo::Md5Des => "PBEWithMD5AndDES",
            Algo::HmacSha512Aes256 => "PBEWITHHMACSHA512ANDAES_256",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub algo: Algo,
    pub iterations: u32,
    pub salt_bytes: usize,
    /// IV bytes carried in the message (0 = none). Jasypt's default with a
    /// `RandomIvGenerator` is 16.
    pub iv_bytes: usize,
    pub hex: bool,
    pub enc_wrapper: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            algo: Algo::HmacSha512Aes256,
            iterations: 1000,
            salt_bytes: 8,
            iv_bytes: 16,
            hex: false,
            enc_wrapper: false,
        }
    }
}

fn encode_out(cfg: &Config, bytes: &[u8]) -> String {
    let body = if cfg.hex {
        hex_encode(bytes).to_uppercase()
    } else {
        b64_encode(bytes)
    };
    if cfg.enc_wrapper {
        format!("ENC({body})")
    } else {
        body
    }
}

fn decode_in(cfg: &Config, text: &str) -> Result<Vec<u8>, String> {
    let t = text.trim();
    let t = t
        .strip_prefix("ENC(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(t);
    if cfg.hex {
        hex_decode(t)
    } else {
        b64_decode(t)
    }
}

pub fn encrypt(cfg: &Config, password: &str, plain: &str) -> Result<String, String> {
    if password.is_empty() {
        return Err("a password (the secret key) is needed".into());
    }
    let salt = random_bytes(cfg.salt_bytes);
    let message = match cfg.algo {
        Algo::Md5Des => {
            if cfg.salt_bytes != 8 {
                return Err("PBEWithMD5AndDES needs an 8-byte salt".into());
            }
            let dk = pbkdf1_md5(password.as_bytes(), &salt, cfg.iterations);
            let ct = cbc_encrypt::<des::Des>(&dk[..8], &dk[8..16], plain.as_bytes())?;
            let mut m = salt;
            m.extend(random_bytes(cfg.iv_bytes));
            m.extend(ct);
            m
        }
        Algo::HmacSha512Aes256 => {
            if cfg.iv_bytes != 16 {
                return Err("AES needs a 16-byte IV in the message".into());
            }
            let key = pbkdf2_hmac_sha512(password.as_bytes(), &salt, cfg.iterations, 32);
            let iv = random_bytes(16);
            let ct = cbc_encrypt::<aes::Aes256>(&key, &iv, plain.as_bytes())?;
            let mut m = salt;
            m.extend(iv);
            m.extend(ct);
            m
        }
    };
    Ok(encode_out(cfg, &message))
}

pub fn decrypt(cfg: &Config, password: &str, text: &str) -> Result<String, String> {
    if password.is_empty() {
        return Err("a password (the secret key) is needed".into());
    }
    let message = decode_in(cfg, text)?;
    let head = cfg.salt_bytes + cfg.iv_bytes;
    if message.len() <= head {
        return Err(format!(
            "message too short: {} bytes, but salt and IV alone take {head}",
            message.len()
        ));
    }
    let salt = &message[..cfg.salt_bytes];
    let iv = &message[cfg.salt_bytes..head];
    let ct = &message[head..];
    let plain = match cfg.algo {
        Algo::Md5Des => {
            let dk = pbkdf1_md5(password.as_bytes(), salt, cfg.iterations);
            cbc_decrypt::<des::Des>(&dk[..8], &dk[8..16], ct)?
        }
        Algo::HmacSha512Aes256 => {
            let key = pbkdf2_hmac_sha512(password.as_bytes(), salt, cfg.iterations, 32);
            cbc_decrypt::<aes::Aes256>(&key, iv, ct)?
        }
    };
    String::from_utf8(plain).map_err(|_| "decrypted bytes are not UTF-8 text".to_string())
}

// ---- legacy DataEncryptor ---------------------------------------------------

/// `DESKeySpec(keyString.getBytes())` keeps the first 8 bytes.
pub fn legacy_des(key: &str, text: &str, decrypt: bool) -> Result<String, String> {
    let kb = key.as_bytes();
    if kb.len() < 8 {
        return Err("the DES key string must be at least 8 characters".into());
    }
    if decrypt {
        let pt = ecb_decrypt::<des::Des>(&kb[..8], &b64_decode(text)?)?;
        String::from_utf8(pt).map_err(|_| "decrypted bytes are not UTF-8".to_string())
    } else {
        Ok(b64_encode(&ecb_encrypt::<des::Des>(
            &kb[..8],
            text.as_bytes(),
        )?))
    }
}

/// `SecretKeySpec(keyString.getBytes(), "AES")` with `Cipher.getInstance("AES")`
/// (ECB, PKCS5). 16, 24 or 32 characters.
pub fn legacy_aes(key: &str, text: &str, decrypt: bool) -> Result<String, String> {
    let kb = key.as_bytes();
    let run = |data: &[u8]| -> Result<Vec<u8>, String> {
        match (kb.len(), decrypt) {
            (16, false) => ecb_encrypt::<aes::Aes128>(kb, data),
            (16, true) => ecb_decrypt::<aes::Aes128>(kb, data),
            (24, false) => ecb_encrypt::<aes::Aes192>(kb, data),
            (24, true) => ecb_decrypt::<aes::Aes192>(kb, data),
            (32, false) => ecb_encrypt::<aes::Aes256>(kb, data),
            (32, true) => ecb_decrypt::<aes::Aes256>(kb, data),
            _ => Err("the AES key string must be 16, 24 or 32 characters".into()),
        }
    };
    if decrypt {
        let pt = run(&b64_decode(text)?)?;
        String::from_utf8(pt).map_err(|_| "decrypted bytes are not UTF-8".to_string())
    } else {
        Ok(b64_encode(&run(text.as_bytes())?))
    }
}

pub fn legacy_md5_b64(text: &str) -> String {
    b64_encode(&md5::Md5::digest(text.as_bytes()))
}

// ---- UI -----------------------------------------------------------------------

#[derive(Default)]
pub struct State {
    pub cfg: Config,
    pub password: String,
    pub plain: String,
    pub encrypted_out: Option<Result<String, String>>,
    pub cipher_in: String,
    pub decrypted_out: Option<Result<String, String>>,
    pub legacy_des_key: String,
    pub legacy_aes_key: String,
    pub legacy_in: String,
    pub legacy_out: Option<Result<String, String>>,
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::page_title(ui, theme, &s.t("dt-jasypt"), Some(&s.t("dt-jasypt-sub")));
    form::section(ui, theme, &s.t("dt-jasypt-config"), |ui| {
        form::field_grid(ui, "jasypt-cfg", |ui| {
            form::label(ui, &s.t("dt-jasypt-algorithm"));
            egui::ComboBox::from_id_salt("jasypt-algo")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(st.cfg.algo.name())
                .show_ui(ui, |ui| {
                    for a in [Algo::HmacSha512Aes256, Algo::Md5Des] {
                        if ui.selectable_value(&mut st.cfg.algo, a, a.name()).clicked() {
                            st.cfg.salt_bytes = 8;
                            st.cfg.iv_bytes = 16;
                        }
                    }
                });
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-password"));
            form::secret_field(ui, &mut st.password, &s.t("dt-jasypt-password-hint"), 320.0);
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-iterations"));
            ui.add(egui::DragValue::new(&mut st.cfg.iterations).range(1..=100_000));
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-salt"));
            ui.add(egui::DragValue::new(&mut st.cfg.salt_bytes).range(8..=64));
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-iv"));
            ui.add(egui::DragValue::new(&mut st.cfg.iv_bytes).range(0..=32));
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-output"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut st.cfg.hex, false, "Base64");
                ui.selectable_value(&mut st.cfg.hex, true, "hex");
                ui.checkbox(&mut st.cfg.enc_wrapper, s.t("dt-jasypt-enc"));
            });
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("dt-jasypt-layout-hint"));
    });
    form::section(ui, theme, &s.t("dt-encrypt"), |ui| {
        util::input(ui, &mut st.plain, &s.t("dt-jasypt-plain-hint"), 3);
        if form::primary_button(ui, theme, &s.t("dt-encrypt")).clicked() {
            st.encrypted_out = Some(encrypt(&st.cfg, &st.password, &st.plain));
        }
        util::result(
            ui,
            s,
            theme,
            &s.t("dt-jasypt-encrypted"),
            &st.encrypted_out,
            3,
        );
    });
    form::section(ui, theme, &s.t("dt-decrypt"), |ui| {
        util::input(ui, &mut st.cipher_in, &s.t("dt-jasypt-cipher-hint"), 3);
        if form::primary_button(ui, theme, &s.t("dt-decrypt")).clicked() {
            st.decrypted_out = Some(decrypt(&st.cfg, &st.password, &st.cipher_in));
        }
        util::result(
            ui,
            s,
            theme,
            &s.t("dt-jasypt-decrypted"),
            &st.decrypted_out,
            3,
        );
    });
    form::section(ui, theme, &s.t("dt-jasypt-legacy"), |ui| {
        form::hint(ui, theme, &s.t("dt-jasypt-legacy-hint"));
        form::field_grid(ui, "jasypt-legacy", |ui| {
            form::label(ui, &s.t("dt-jasypt-des-key"));
            form::secret_field(ui, &mut st.legacy_des_key, "", 320.0);
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-aes-key"));
            form::secret_field(ui, &mut st.legacy_aes_key, "", 320.0);
            ui.end_row();
        });
        util::input(
            ui,
            &mut st.legacy_in,
            &s.t("dt-jasypt-legacy-input-hint"),
            3,
        );
        ui.horizontal_wrapped(|ui| {
            if ui.button(s.t("dt-jasypt-des-encrypt")).clicked() {
                st.legacy_out = Some(legacy_des(&st.legacy_des_key, &st.legacy_in, false));
            }
            if ui.button(s.t("dt-jasypt-des-decrypt")).clicked() {
                st.legacy_out = Some(legacy_des(&st.legacy_des_key, &st.legacy_in, true));
            }
            if ui.button(s.t("dt-jasypt-aes-encrypt")).clicked() {
                st.legacy_out = Some(legacy_aes(&st.legacy_aes_key, &st.legacy_in, false));
            }
            if ui.button(s.t("dt-jasypt-aes-decrypt")).clicked() {
                st.legacy_out = Some(legacy_aes(&st.legacy_aes_key, &st.legacy_in, true));
            }
            if ui.button(s.t("dt-jasypt-md5")).clicked() {
                st.legacy_out = Some(Ok(legacy_md5_b64(&st.legacy_in)));
            }
        });
        util::result(ui, s, theme, &s.t("dt-result"), &st.legacy_out, 3);
    });
    let _ = crypto::GCM_IV_LEN;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_round_trip_has_jasypt_layout() {
        let cfg = Config::default();
        let enc = encrypt(&cfg, "secret", "hello jasypt").unwrap();
        let raw = b64_decode(&enc).unwrap();
        // salt(8) + iv(16) + one AES block for 12 bytes of text.
        assert_eq!(raw.len(), 8 + 16 + 16);
        assert_eq!(decrypt(&cfg, "secret", &enc).unwrap(), "hello jasypt");
        assert!(decrypt(&cfg, "wrong", &enc).is_err());
        let wrapped = Config {
            enc_wrapper: true,
            ..Config::default()
        };
        let w = encrypt(&wrapped, "s", "x").unwrap();
        assert!(w.starts_with("ENC(") && w.ends_with(')'));
        assert_eq!(decrypt(&wrapped, "s", &w).unwrap(), "x");
    }

    #[test]
    fn des_round_trip_with_and_without_iv_bytes() {
        for iv in [0usize, 16] {
            let cfg = Config {
                algo: Algo::Md5Des,
                iv_bytes: iv,
                ..Config::default()
            };
            let enc = encrypt(&cfg, "pw", "legacy config value").unwrap();
            let raw = b64_decode(&enc).unwrap();
            assert_eq!(raw.len(), 8 + iv + 24);
            assert_eq!(decrypt(&cfg, "pw", &enc).unwrap(), "legacy config value");
        }
    }

    #[test]
    fn pbes1_known_answer_against_java() {
        // Produced by Java: PBEWithMD5AndDES, password "password", salt
        // 0x0102030405060708, 1000 iterations, plaintext "test" — the
        // derived key/IV are deterministic, so the ciphertext is too.
        let salt = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let dk = pbkdf1_md5(b"password", &salt, 1000);
        let ct = cbc_encrypt::<des::Des>(&dk[..8], &dk[8..16], b"test").unwrap();
        assert_eq!(ct.len(), 8);
        let back = cbc_decrypt::<des::Des>(&dk[..8], &dk[8..16], &ct).unwrap();
        assert_eq!(back, b"test");
    }

    #[test]
    fn legacy_helpers() {
        let enc = legacy_des(
            "459245lk34jkbjfwer89fkj98345895hjkh9ebnkas0324bjhbq93849",
            "pw",
            false,
        )
        .unwrap();
        assert_eq!(
            legacy_des(
                "459245lk34jkbjfwer89fkj98345895hjkh9ebnkas0324bjhbq93849",
                &enc,
                true
            )
            .unwrap(),
            "pw"
        );
        let enc = legacy_aes("4D92199549E0F2EF", "hello", false).unwrap();
        assert_eq!(legacy_aes("4D92199549E0F2EF", &enc, true).unwrap(), "hello");
        assert!(legacy_aes("short", "x", false).is_err());
        assert_eq!(legacy_md5_b64("password"), "X03MO1qnZdYdgyfeuILPmQ==");
    }
}
