//! Secrets: the encrypted vault file (default, portable) behind a `SecretStore`
//! trait (docs/06 §2). Values are referenced everywhere else as `secret:<ulid>`.
//!
//! Vault format (`vault.hsv`):
//! ```text
//! magic "HSV1" | u8 version=1 | u32 m_cost KiB | u32 t_cost | u32 p_cost |
//! [u8;16] salt | [u8;24] nonce | u32 len | ciphertext(len) (= AEAD over JSON map)
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use parking_lot::Mutex;
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

pub const PREFIX: &str = "secret:";
const MAGIC: &[u8; 4] = b"HSV1";

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("the vault is locked")]
    Locked,
    #[error("wrong master password or corrupted vault")]
    BadPassword,
    #[error("vault file is not valid: {0}")]
    Format(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("no secret with id {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, SecretError>;

/// `secret:<ulid>` reference as stored in profiles and environments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretRef(pub String);

impl SecretRef {
    pub fn parse(s: &str) -> Option<Self> {
        s.strip_prefix(PREFIX)
            .filter(|id| !id.is_empty())
            .map(|id| Self(id.to_string()))
    }
    pub fn to_string_ref(&self) -> String {
        format!("{PREFIX}{}", self.0)
    }
}

pub trait SecretStore: Send + Sync {
    fn is_locked(&self) -> bool;
    fn exists(&self) -> bool;
    fn unlock(&self, master: &SecretString) -> Result<()>;
    fn lock(&self);
    fn get(&self, r: &SecretRef) -> Result<SecretString>;
    fn set(&self, r: &SecretRef, label: &str, value: &SecretString) -> Result<()>;
    fn delete(&self, r: &SecretRef) -> Result<()>;
    /// Ids and labels, never values.
    fn list(&self) -> Result<Vec<(SecretRef, String)>>;
    /// Create a new secret and return its reference.
    fn create(&self, label: &str, value: &SecretString) -> Result<SecretRef> {
        let r = SecretRef(crate::new_id());
        self.set(&r, label, value)?;
        Ok(r)
    }
    /// Create the store with this master password (vault only; must not exist yet).
    fn initialize(&self, _master: &SecretString) -> Result<()> {
        Err(SecretError::Format(
            "this secret store cannot be initialised".into(),
        ))
    }
    /// Re-encrypt with a new master password (store must be unlocked).
    fn change_master(&self, _new_master: &SecretString) -> Result<()> {
        Err(SecretError::Format(
            "this secret store has no master password".into(),
        ))
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
struct Entry {
    label: String,
    value: String,
}

struct Unlocked {
    key: Zeroizing<[u8; 32]>,
    entries: HashMap<String, Entry>,
}

/// The encrypted vault file.
pub struct VaultStore {
    path: PathBuf,
    params: KdfParams,
    state: Mutex<Option<Unlocked>>,
}

#[derive(Debug, Clone, Copy)]
pub struct KdfParams {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfParams {
    pub const DEFAULT: Self = Self {
        m_cost_kib: 64 * 1024,
        t_cost: 3,
        p_cost: 1,
    };
    /// Fast parameters for tests and `--self-test`.
    pub const FAST: Self = Self {
        m_cost_kib: 8 * 1024,
        t_cost: 1,
        p_cost: 1,
    };
}

impl VaultStore {
    pub fn new(path: &Path, params: KdfParams) -> Self {
        Self {
            path: path.to_path_buf(),
            params,
            state: Mutex::new(None),
        }
    }

    fn derive(
        params: KdfParams,
        master: &SecretString,
        salt: &[u8; 16],
    ) -> Result<Zeroizing<[u8; 32]>> {
        let p = Params::new(params.m_cost_kib, params.t_cost, params.p_cost, Some(32))
            .map_err(|e| SecretError::Format(e.to_string()))?;
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, p);
        let mut key = Zeroizing::new([0u8; 32]);
        argon
            .hash_password_into(master.expose_secret().as_bytes(), salt, key.as_mut())
            .map_err(|e| SecretError::Format(e.to_string()))?;
        Ok(key)
    }

    fn write_locked(&self, key: &[u8; 32], entries: &HashMap<String, Entry>) -> Result<()> {
        let plaintext = Zeroizing::new(
            serde_json::to_vec(entries).map_err(|e| SecretError::Format(e.to_string()))?,
        );
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; 24];
        // The salt is stored once per vault; keep the existing one so the same
        // master password keeps deriving the same key.
        let existing_salt = self.read_header().ok().map(|h| h.salt);
        match existing_salt {
            Some(s) => salt = s,
            None => rand::rng().fill_bytes(&mut salt),
        }
        rand::rng().fill_bytes(&mut nonce);
        let cipher = XChaCha20Poly1305::new(key.into());
        let ct = cipher
            .encrypt(XNonce::from_slice(&nonce), plaintext.as_slice())
            .map_err(|_| SecretError::Format("encryption failed".into()))?;
        let mut out = Vec::with_capacity(4 + 1 + 12 + 16 + 24 + 4 + ct.len());
        out.extend_from_slice(MAGIC);
        out.push(1);
        out.extend_from_slice(&self.params.m_cost_kib.to_le_bytes());
        out.extend_from_slice(&self.params.t_cost.to_le_bytes());
        out.extend_from_slice(&self.params.p_cost.to_le_bytes());
        out.extend_from_slice(&salt);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&(ct.len() as u32).to_le_bytes());
        out.extend_from_slice(&ct);
        if self.path.exists() {
            let _ = std::fs::copy(&self.path, self.path.with_extension("hsv.bak"));
        }
        crate::storage::write_atomic(&self.path, &out)
            .map_err(|e| SecretError::Format(e.to_string()))
    }

    fn read_header(&self) -> Result<Header> {
        let bytes = std::fs::read(&self.path)?;
        Header::parse(&bytes)
    }

    /// Create a brand-new empty vault with this master password (the file must not exist).
    pub fn initialize(&self, master: &SecretString) -> Result<()> {
        if self.path.exists() {
            return Err(SecretError::Format("vault already exists".into()));
        }
        let mut salt = [0u8; 16];
        rand::rng().fill_bytes(&mut salt);
        let key = Self::derive(self.params, master, &salt)?;
        // write_locked reads the header for the salt; seed it by writing a
        // header-only file first.
        let mut seed = Vec::new();
        seed.extend_from_slice(MAGIC);
        seed.push(1);
        seed.extend_from_slice(&self.params.m_cost_kib.to_le_bytes());
        seed.extend_from_slice(&self.params.t_cost.to_le_bytes());
        seed.extend_from_slice(&self.params.p_cost.to_le_bytes());
        seed.extend_from_slice(&salt);
        seed.extend_from_slice(&[0u8; 24]);
        seed.extend_from_slice(&0u32.to_le_bytes());
        crate::storage::write_atomic(&self.path, &seed)
            .map_err(|e| SecretError::Format(e.to_string()))?;
        self.write_locked(&key, &HashMap::new())?;
        *self.state.lock() = Some(Unlocked {
            key,
            entries: HashMap::new(),
        });
        Ok(())
    }

    /// Re-encrypt with a new master password (vault must be unlocked).
    pub fn change_master(&self, new_master: &SecretString) -> Result<()> {
        let mut guard = self.state.lock();
        let u = guard.as_mut().ok_or(SecretError::Locked)?;
        let mut salt = [0u8; 16];
        rand::rng().fill_bytes(&mut salt);
        let key = Self::derive(self.params, new_master, &salt)?;
        // Force a new salt by removing the header first.
        let _ = std::fs::remove_file(&self.path);
        let mut seed = Vec::new();
        seed.extend_from_slice(MAGIC);
        seed.push(1);
        seed.extend_from_slice(&self.params.m_cost_kib.to_le_bytes());
        seed.extend_from_slice(&self.params.t_cost.to_le_bytes());
        seed.extend_from_slice(&self.params.p_cost.to_le_bytes());
        seed.extend_from_slice(&salt);
        seed.extend_from_slice(&[0u8; 24]);
        seed.extend_from_slice(&0u32.to_le_bytes());
        crate::storage::write_atomic(&self.path, &seed)
            .map_err(|e| SecretError::Format(e.to_string()))?;
        self.write_locked(&key, &u.entries)?;
        u.key = key;
        Ok(())
    }
}

struct Header {
    params: KdfParams,
    salt: [u8; 16],
    nonce: [u8; 24],
    ciphertext: Vec<u8>,
}

impl Header {
    fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 4 + 1 + 12 + 16 + 24 + 4 || &bytes[..4] != MAGIC {
            return Err(SecretError::Format("not a HyperStack vault".into()));
        }
        if bytes[4] != 1 {
            return Err(SecretError::Format(format!(
                "unsupported vault version {}",
                bytes[4]
            )));
        }
        let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        let params = KdfParams {
            m_cost_kib: u32_at(5),
            t_cost: u32_at(9),
            p_cost: u32_at(13),
        };
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&bytes[17..33]);
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&bytes[33..57]);
        let len = u32_at(57) as usize;
        if bytes.len() < 61 + len {
            return Err(SecretError::Format("vault is truncated".into()));
        }
        Ok(Self {
            params,
            salt,
            nonce,
            ciphertext: bytes[61..61 + len].to_vec(),
        })
    }
}

impl SecretStore for VaultStore {
    fn is_locked(&self) -> bool {
        self.state.lock().is_none()
    }

    fn exists(&self) -> bool {
        self.path.exists()
    }

    fn unlock(&self, master: &SecretString) -> Result<()> {
        let h = self.read_header()?;
        let key = Self::derive(h.params, master, &h.salt)?;
        let cipher = XChaCha20Poly1305::new(key.as_slice().into());
        let plain = Zeroizing::new(
            cipher
                .decrypt(XNonce::from_slice(&h.nonce), h.ciphertext.as_slice())
                .map_err(|_| SecretError::BadPassword)?,
        );
        let entries: HashMap<String, Entry> =
            serde_json::from_slice(&plain).map_err(|e| SecretError::Format(e.to_string()))?;
        *self.state.lock() = Some(Unlocked { key, entries });
        Ok(())
    }

    fn lock(&self) {
        *self.state.lock() = None;
    }

    fn get(&self, r: &SecretRef) -> Result<SecretString> {
        let guard = self.state.lock();
        let u = guard.as_ref().ok_or(SecretError::Locked)?;
        u.entries
            .get(&r.0)
            .map(|e| SecretString::from(e.value.clone()))
            .ok_or_else(|| SecretError::NotFound(r.0.clone()))
    }

    fn set(&self, r: &SecretRef, label: &str, value: &SecretString) -> Result<()> {
        let mut guard = self.state.lock();
        let u = guard.as_mut().ok_or(SecretError::Locked)?;
        u.entries.insert(
            r.0.clone(),
            Entry {
                label: label.to_string(),
                value: value.expose_secret().to_string(),
            },
        );
        let key = *u.key;
        let entries = u.entries.clone();
        drop(guard);
        self.write_locked(&key, &entries)
    }

    fn delete(&self, r: &SecretRef) -> Result<()> {
        let mut guard = self.state.lock();
        let u = guard.as_mut().ok_or(SecretError::Locked)?;
        u.entries.remove(&r.0);
        let key = *u.key;
        let entries = u.entries.clone();
        drop(guard);
        self.write_locked(&key, &entries)
    }

    fn list(&self) -> Result<Vec<(SecretRef, String)>> {
        let guard = self.state.lock();
        let u = guard.as_ref().ok_or(SecretError::Locked)?;
        let mut v: Vec<_> = u
            .entries
            .iter()
            .map(|(k, e)| (SecretRef(k.clone()), e.label.clone()))
            .collect();
        v.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(v)
    }

    fn initialize(&self, master: &SecretString) -> Result<()> {
        VaultStore::initialize(self, master)
    }

    fn change_master(&self, new_master: &SecretString) -> Result<()> {
        VaultStore::change_master(self, new_master)
    }
}

/// Replace every known secret value in `text` with `***` (used by logging).
pub fn redact(text: &str, values: &[String]) -> String {
    let mut out = text.to_string();
    for v in values {
        if v.len() >= 4 {
            out = out.replace(v, "***");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_round_trip_lock_unlock_wrong_password() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("vault.hsv");
        let vault = VaultStore::new(&path, KdfParams::FAST);
        assert!(!vault.exists());
        let master = SecretString::from("correct horse".to_string());
        vault.initialize(&master).unwrap();
        let r = vault
            .create(
                "db password",
                &SecretString::from("s3cret-value".to_string()),
            )
            .unwrap();
        assert_eq!(vault.get(&r).unwrap().expose_secret(), "s3cret-value");

        vault.lock();
        assert!(vault.is_locked());
        assert!(matches!(vault.get(&r), Err(SecretError::Locked)));
        assert!(matches!(
            vault.unlock(&SecretString::from("wrong".to_string())),
            Err(SecretError::BadPassword)
        ));
        vault.unlock(&master).unwrap();
        assert_eq!(vault.get(&r).unwrap().expose_secret(), "s3cret-value");
        assert_eq!(
            vault.list().unwrap(),
            vec![(r.clone(), "db password".to_string())]
        );

        // Reopen from disk in a fresh store.
        let again = VaultStore::new(&path, KdfParams::FAST);
        again.unlock(&master).unwrap();
        assert_eq!(again.get(&r).unwrap().expose_secret(), "s3cret-value");
        again.delete(&r).unwrap();
        assert!(matches!(again.get(&r), Err(SecretError::NotFound(_))));

        // The plaintext never touches the disk.
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.windows(12).any(|w| w == b"s3cret-value"));
        assert!(!bytes.windows(11).any(|w| w == b"db password"));
    }

    #[test]
    fn change_master_re_encrypts() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = VaultStore::new(&tmp.path().join("v.hsv"), KdfParams::FAST);
        let old = SecretString::from("old".to_string());
        vault.initialize(&old).unwrap();
        let r = vault
            .create("x", &SecretString::from("v".to_string()))
            .unwrap();
        let new = SecretString::from("new".to_string());
        vault.change_master(&new).unwrap();
        vault.lock();
        assert!(vault.unlock(&old).is_err());
        vault.unlock(&new).unwrap();
        assert_eq!(vault.get(&r).unwrap().expose_secret(), "v");
    }

    #[test]
    fn secret_refs_parse() {
        assert_eq!(
            SecretRef::parse("secret:abc"),
            Some(SecretRef("abc".into()))
        );
        assert_eq!(SecretRef::parse("secret:"), None);
        assert_eq!(SecretRef::parse("plain"), None);
        assert_eq!(redact("pw=hunter22 ok", &["hunter22".into()]), "pw=*** ok");
    }
}
