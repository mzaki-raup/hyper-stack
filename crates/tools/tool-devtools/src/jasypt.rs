//! Jasypt-compatible string encryption, as `StandardPBEStringEncryptor` /
//! `PooledPBEStringEncryptor` with `RandomSaltGenerator` and
//! `RandomIvGenerator` produce it:
//!
//! Jasypt sizes the salt and the IV to the cipher's block size: 8 bytes
//! each for DES, 16 each for AES. Message = salt ‖ [iv] ‖ ciphertext, the
//! IV there only with a `RandomIvGenerator`.
//!
//! - `PBEWithMD5AndDES` (Java PBES1): key and IV from PBKDF1-MD5 over the
//!   password and the salt, DES-CBC-PKCS5. PBES1 derives its own IV and
//!   ignores the one jasypt prepends, but the bytes are still in the
//!   message.
//! - `PBEWITHHMACSHA512ANDAES_256`: a 256-bit key from PBKDF2-HMAC-SHA512
//!   over the password and the salt, AES-256-CBC-PKCS5 with the message's
//!   IV. It needs a `RandomIvGenerator`: without one Java picks an IV it
//!   never writes down.
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
use serde::{Deserialize, Serialize};

use crate::crypto::{
    self, b64_decode, b64_encode, cbc_decrypt, cbc_encrypt, ecb_decrypt, ecb_encrypt, hex_decode,
    hex_encode, pbkdf1_md5, pbkdf2_hmac_sha512, random_bytes,
};
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

    /// The cipher's block size, which jasypt uses for the salt and the IV.
    pub fn block(self) -> usize {
        match self {
            Algo::Md5Des => 8,
            Algo::HmacSha512Aes256 => 16,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub algo: Algo,
    pub iterations: u32,
    /// `RandomIvGenerator` (an IV in the message) or `NoIvGenerator`.
    pub random_iv: bool,
    pub hex: bool,
    pub enc_wrapper: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            algo: Algo::HmacSha512Aes256,
            iterations: 1000,
            random_iv: true,
            hex: false,
            enc_wrapper: false,
        }
    }
}

impl Config {
    /// The IV bytes the message carries.
    fn iv_len(&self) -> usize {
        if self.random_iv { self.algo.block() } else { 0 }
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

/// Refusals both ways: a missing password, AES without an IV.
fn check(cfg: &Config, password: &str) -> Result<(), String> {
    if password.is_empty() {
        return Err("A password (the secret key) is needed.".into());
    }
    if cfg.algo == Algo::HmacSha512Aes256 && !cfg.random_iv {
        return Err(
            "PBEWITHHMACSHA512ANDAES_256 needs RandomIvGenerator: without one the IV is lost."
                .into(),
        );
    }
    Ok(())
}

pub fn encrypt(cfg: &Config, password: &str, plain: &str) -> Result<String, String> {
    check(cfg, password)?;
    let salt = random_bytes(cfg.algo.block());
    let iv = random_bytes(cfg.iv_len());
    let message = seal(cfg, password, plain, salt, iv)?;
    Ok(encode_out(cfg, &message))
}

/// salt ‖ iv ‖ ciphertext for the given salt and IV.
fn seal(
    cfg: &Config,
    password: &str,
    plain: &str,
    salt: Vec<u8>,
    iv: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let ct = match cfg.algo {
        Algo::Md5Des => {
            let dk = pbkdf1_md5(password.as_bytes(), &salt, cfg.iterations);
            cbc_encrypt::<des::Des>(&dk[..8], &dk[8..16], plain.as_bytes())?
        }
        Algo::HmacSha512Aes256 => {
            let key = pbkdf2_hmac_sha512(password.as_bytes(), &salt, cfg.iterations, 32);
            cbc_encrypt::<aes::Aes256>(&key, &iv, plain.as_bytes())?
        }
    };
    let mut m = salt;
    m.extend(iv);
    m.extend(ct);
    Ok(m)
}

pub fn decrypt(cfg: &Config, password: &str, text: &str) -> Result<String, String> {
    check(cfg, password)?;
    let message = decode_in(cfg, text)?;
    let salt_len = cfg.algo.block();
    let head = salt_len + cfg.iv_len();
    if message.len() <= head {
        return Err(format!(
            "The message is too short: {} bytes, but the salt and IV alone take {head}.",
            message.len()
        ));
    }
    let salt = &message[..salt_len];
    let iv = &message[salt_len..head];
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

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct State {
    pub cfg: Config,
    pub password: String,
    pub plain: String,
    #[serde(skip)]
    pub encrypted_out: Option<Result<String, String>>,
    pub cipher_in: String,
    #[serde(skip)]
    pub decrypted_out: Option<Result<String, String>>,
    pub legacy_des_key: String,
    pub legacy_aes_key: String,
    pub legacy_in: String,
    #[serde(skip)]
    pub legacy_out: Option<Result<String, String>>,
}

impl State {
    pub fn for_save(&self) -> State {
        let mut c = self.clone();
        c.password = crate::keep_reference(&c.password);
        c.legacy_des_key = crate::keep_reference(&c.legacy_des_key);
        c.legacy_aes_key = crate::keep_reference(&c.legacy_aes_key);
        c
    }
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    crate::util::page_title(ui, s, theme, "dt-jasypt", Some("dt-jasypt-sub"));
    form::section(ui, theme, &s.t("dt-jasypt-config"), |ui| {
        form::field_grid(ui, "jasypt-cfg", |ui| {
            form::label(ui, &s.t("dt-jasypt-algorithm"));
            egui::ComboBox::from_id_salt("jasypt-algo")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(st.cfg.algo.name())
                .show_ui(ui, |ui| {
                    for a in [Algo::HmacSha512Aes256, Algo::Md5Des] {
                        hs_ui::form::selectable_value(ui, &mut st.cfg.algo, a, a.name());
                    }
                });
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-password"));
            util::secret_line(
                ui,
                s,
                theme,
                &mut st.password,
                &s.t("dt-jasypt-password-hint"),
            );
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-iterations"));
            ui.add(egui::DragValue::new(&mut st.cfg.iterations).range(1..=100_000));
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-iv"));
            let iv_name = |random: bool| {
                if random {
                    "RandomIvGenerator"
                } else {
                    "NoIvGenerator"
                }
            };
            egui::ComboBox::from_id_salt("jasypt-iv")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(iv_name(st.cfg.random_iv))
                .show_ui(ui, |ui| {
                    for random in [true, false] {
                        hs_ui::form::selectable_value(
                            ui,
                            &mut st.cfg.random_iv,
                            random,
                            iv_name(random),
                        );
                    }
                });
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-output"));
            form::centred_row(ui, |ui| {
                hs_ui::form::selectable_value(ui, &mut st.cfg.hex, false, "Base64");
                hs_ui::form::selectable_value(ui, &mut st.cfg.hex, true, s.t("dt-hash-hex"));
                ui.add_space(form::OPTION_GAP);
                ui.checkbox(&mut st.cfg.enc_wrapper, s.t("dt-jasypt-enc"));
            });
            ui.end_row();
        });
        ui.add_space(form::ROW_GAP);
        form::hint(ui, theme, &s.t("dt-jasypt-layout-hint"));
    });
    form::section(ui, theme, &s.t("dt-encrypt"), |ui| {
        util::input(ui, s, theme, &mut st.plain, &s.t("dt-jasypt-plain-hint"), 3);
        ui.add_space(form::ROW_GAP);
        if form::primary_button(
            ui,
            theme,
            &format!("{} {}", hs_ui::icons::ph::LOCK_KEY, s.t("dt-encrypt")),
        )
        .clicked()
        {
            st.encrypted_out =
                Some(util::expand(s, &st.password).and_then(|pw| encrypt(&st.cfg, &pw, &st.plain)));
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
        util::input(
            ui,
            s,
            theme,
            &mut st.cipher_in,
            &s.t("dt-jasypt-cipher-hint"),
            3,
        );
        ui.add_space(form::ROW_GAP);
        if form::primary_button(
            ui,
            theme,
            &format!("{} {}", hs_ui::icons::ph::LOCK_KEY_OPEN, s.t("dt-decrypt")),
        )
        .clicked()
        {
            st.decrypted_out = Some(
                util::expand(s, &st.password).and_then(|pw| decrypt(&st.cfg, &pw, &st.cipher_in)),
            );
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
        ui.add_space(form::ROW_GAP);
        form::field_grid(ui, "jasypt-legacy", |ui| {
            form::label(ui, &s.t("dt-jasypt-des-key"));
            util::secret_line(ui, s, theme, &mut st.legacy_des_key, "");
            ui.end_row();
            form::label(ui, &s.t("dt-jasypt-aes-key"));
            util::secret_line(ui, s, theme, &mut st.legacy_aes_key, "");
            ui.end_row();
        });
        util::input(
            ui,
            s,
            theme,
            &mut st.legacy_in,
            &s.t("dt-jasypt-legacy-input-hint"),
            3,
        );
        ui.add_space(form::ROW_GAP);
        util::button_row(ui, |ui| {
            use hs_ui::icons::ph;
            if form::button(ui, theme, ph::LOCK_KEY, &s.t("dt-jasypt-des-encrypt")).clicked() {
                st.legacy_out = Some(
                    util::expand(s, &st.legacy_des_key)
                        .and_then(|k| legacy_des(&k, &st.legacy_in, false)),
                );
            }
            if form::button(ui, theme, ph::LOCK_KEY_OPEN, &s.t("dt-jasypt-des-decrypt")).clicked() {
                st.legacy_out = Some(
                    util::expand(s, &st.legacy_des_key)
                        .and_then(|k| legacy_des(&k, &st.legacy_in, true)),
                );
            }
            if form::button(ui, theme, ph::LOCK_KEY, &s.t("dt-jasypt-aes-encrypt")).clicked() {
                st.legacy_out = Some(
                    util::expand(s, &st.legacy_aes_key)
                        .and_then(|k| legacy_aes(&k, &st.legacy_in, false)),
                );
            }
            if form::button(ui, theme, ph::LOCK_KEY_OPEN, &s.t("dt-jasypt-aes-decrypt")).clicked() {
                st.legacy_out = Some(
                    util::expand(s, &st.legacy_aes_key)
                        .and_then(|k| legacy_aes(&k, &st.legacy_in, true)),
                );
            }
            if form::button(ui, theme, ph::HASH, &s.t("dt-jasypt-md5")).clicked() {
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

    // Made by Java 21's SunJCE exactly as jasypt 1.9.3 does it (block-size
    // salt and IV, `PBEParameterSpec(salt, 1000, IvParameterSpec(iv))`,
    // salt ‖ iv ‖ ciphertext): password "hyperstack", salt 01 02 …, IV
    // 21 22 …, plaintext "jdbc-password".
    const JAVA_DES: &str = "AQIDBAUGBwghIiMkJSYnKNzeY+V48sK7xFiCsryZnlQ=";
    const JAVA_AES: &str = "AQIDBAUGBwgJCgsMDQ4PECEiIyQlJicoKSorLC0uLzCwGC25Na8yYtsF8K9MzSb5";

    fn seq(n: u8, start: u8) -> Vec<u8> {
        (0..n).map(|i| start + i).collect()
    }

    fn des() -> Config {
        Config {
            algo: Algo::Md5Des,
            ..Config::default()
        }
    }

    #[test]
    fn both_algorithms_match_java() {
        let aes = Config::default();
        assert_eq!(
            decrypt(&aes, "hyperstack", JAVA_AES).unwrap(),
            "jdbc-password"
        );
        assert_eq!(
            decrypt(&des(), "hyperstack", JAVA_DES).unwrap(),
            "jdbc-password"
        );
        let sealed = seal(
            &aes,
            "hyperstack",
            "jdbc-password",
            seq(16, 1),
            seq(16, 0x21),
        );
        assert_eq!(b64_encode(&sealed.unwrap()), JAVA_AES);
        let sealed = seal(
            &des(),
            "hyperstack",
            "jdbc-password",
            seq(8, 1),
            seq(8, 0x21),
        );
        assert_eq!(b64_encode(&sealed.unwrap()), JAVA_DES);
    }

    #[test]
    fn des_without_an_iv_generator_drops_the_iv_bytes() {
        // PBES1 ignores the IV, so NoIvGenerator's message is Java's
        // without its eight IV bytes.
        let mut raw = b64_decode(JAVA_DES).unwrap();
        raw.drain(8..16);
        let no_iv = Config {
            random_iv: false,
            ..des()
        };
        assert_eq!(
            decrypt(&no_iv, "hyperstack", &b64_encode(&raw)).unwrap(),
            "jdbc-password"
        );
    }

    #[test]
    fn round_trips_and_refusals() {
        for cfg in [Config::default(), des()] {
            let enc = encrypt(&cfg, "secret", "hello jasypt").unwrap();
            let raw = b64_decode(&enc).unwrap();
            // salt and IV at the block size, then whole blocks.
            let block = cfg.algo.block();
            assert_eq!(raw.len(), 2 * block + 16);
            assert_eq!(decrypt(&cfg, "secret", &enc).unwrap(), "hello jasypt");
            assert!(decrypt(&cfg, "wrong", &enc).is_err());
        }
        let wrapped = Config {
            enc_wrapper: true,
            ..Config::default()
        };
        let w = encrypt(&wrapped, "s", "x").unwrap();
        assert!(w.starts_with("ENC(") && w.ends_with(')'));
        assert_eq!(decrypt(&wrapped, "s", &w).unwrap(), "x");
        let aes_no_iv = Config {
            random_iv: false,
            ..Config::default()
        };
        assert!(encrypt(&aes_no_iv, "s", "x").is_err());
        assert!(encrypt(&des(), "", "x").is_err());
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
