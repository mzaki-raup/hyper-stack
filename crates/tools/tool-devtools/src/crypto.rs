//! Cryptographic primitives the utilities share: key derivation (PBKDF1 as
//! Java's `PBEWithMD5AndDES` does it, PBKDF2-HMAC), CBC/ECB block modes
//! with PKCS#7 padding over the `aes`/`des` block ciphers, AES-256-GCM,
//! RSA PKCS#1 v1.5 signatures and ECDSA P-256 through `ring`, and the key
//! parsing (PEM or bare Base64, PKCS#8 / PKCS#1 / X.509 SPKI) that lets a
//! key pasted from a Java application work unchanged.
//!
//! Everything here is a pure function with known-answer tests.

use base64::Engine;
use cipher::{BlockCipherDecrypt, BlockCipherEncrypt, BlockSizeUser, KeyInit};
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use sha2::{Sha256, Sha512};

pub type Result<T> = std::result::Result<T, String>;

// ---------------------------------------------------------------------------
// Encoding helpers

pub fn b64_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Decode Base64 in any common flavour: standard or URL-safe, with or
/// without padding, ignoring whitespace and line breaks.
pub fn b64_decode(text: &str) -> Result<Vec<u8>> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let engines = [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ];
    let trimmed = compact.trim_end_matches('=');
    for e in engines {
        if let Ok(b) = e.decode(&compact) {
            return Ok(b);
        }
        if let Ok(b) = e.decode(trimmed) {
            return Ok(b);
        }
    }
    Err("not valid Base64".into())
}

pub fn b64url_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex_decode(text: &str) -> Result<Vec<u8>> {
    let compact: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .collect();
    if !compact.len().is_multiple_of(2) {
        return Err("odd number of hex digits".into());
    }
    (0..compact.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&compact[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Key derivation

/// PKCS#5 v1.5 PBKDF1 with MD5, as Java's `PBEWithMD5AndDES` derives its
/// 8-byte key and 8-byte IV: `T1 = MD5(P || S)`, `Ti = MD5(Ti-1)`.
pub fn pbkdf1_md5(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 16] {
    let mut t = Md5::new()
        .chain_update(password)
        .chain_update(salt)
        .finalize();
    for _ in 1..iterations.max(1) {
        t = Md5::digest(t);
    }
    t.into()
}

macro_rules! pbkdf2_impl {
    ($name:ident, $hash:ty) => {
        /// PBKDF2 (RFC 8018) with this HMAC, `out_len` bytes.
        pub fn $name(password: &[u8], salt: &[u8], iterations: u32, out_len: usize) -> Vec<u8> {
            let mut out = Vec::with_capacity(out_len);
            let mut block: u32 = 1;
            while out.len() < out_len {
                let mut mac = Hmac::<$hash>::new_from_slice(password).expect("any key length");
                mac.update(salt);
                mac.update(&block.to_be_bytes());
                let mut u = mac.finalize().into_bytes();
                let mut t = u;
                for _ in 1..iterations.max(1) {
                    let mut mac = Hmac::<$hash>::new_from_slice(password).expect("any key length");
                    mac.update(&u);
                    u = mac.finalize().into_bytes();
                    for (a, b) in t.iter_mut().zip(u.iter()) {
                        *a ^= b;
                    }
                }
                out.extend_from_slice(&t);
                block += 1;
            }
            out.truncate(out_len);
            out
        }
    };
}
pbkdf2_impl!(pbkdf2_hmac_sha256, Sha256);
pbkdf2_impl!(pbkdf2_hmac_sha512, Sha512);

// ---------------------------------------------------------------------------
// Block modes with PKCS#7 padding

fn pkcs7_pad(data: &[u8], block: usize) -> Vec<u8> {
    let pad = block - (data.len() % block);
    let mut v = data.to_vec();
    v.extend(std::iter::repeat_n(pad as u8, pad));
    v
}

fn pkcs7_unpad(data: &[u8], block: usize) -> Result<Vec<u8>> {
    let Some(&pad) = data.last() else {
        return Err("empty ciphertext".into());
    };
    let pad = pad as usize;
    if pad == 0
        || pad > block
        || pad > data.len()
        || data[data.len() - pad..].iter().any(|&b| b as usize != pad)
    {
        return Err("bad padding (wrong key, salt or algorithm?)".into());
    }
    Ok(data[..data.len() - pad].to_vec())
}

fn block_of<C: BlockSizeUser>(bytes: &[u8]) -> cipher::Block<C> {
    let mut b = cipher::Block::<C>::default();
    b.copy_from_slice(bytes);
    b
}

/// CBC encrypt with PKCS#7 padding. `iv` must be one block long.
pub fn cbc_encrypt<C: BlockCipherEncrypt + KeyInit>(
    key: &[u8],
    iv: &[u8],
    data: &[u8],
) -> Result<Vec<u8>> {
    let cipher = C::new_from_slice(key).map_err(|_| "wrong key length".to_string())?;
    let bs = C::block_size();
    if iv.len() != bs {
        return Err(format!("IV must be {bs} bytes"));
    }
    let padded = pkcs7_pad(data, bs);
    let mut prev = iv.to_vec();
    let mut out = Vec::with_capacity(padded.len());
    for chunk in padded.chunks(bs) {
        let xored: Vec<u8> = chunk.iter().zip(prev.iter()).map(|(a, b)| a ^ b).collect();
        let mut block = block_of::<C>(&xored);
        cipher.encrypt_block(&mut block);
        prev = block.to_vec();
        out.extend_from_slice(&prev);
    }
    Ok(out)
}

pub fn cbc_decrypt<C: BlockCipherDecrypt + KeyInit>(
    key: &[u8],
    iv: &[u8],
    data: &[u8],
) -> Result<Vec<u8>> {
    let cipher = C::new_from_slice(key).map_err(|_| "wrong key length".to_string())?;
    let bs = C::block_size();
    if iv.len() != bs {
        return Err(format!("IV must be {bs} bytes"));
    }
    if data.is_empty() || !data.len().is_multiple_of(bs) {
        return Err("ciphertext is not a whole number of blocks".into());
    }
    let mut prev = iv.to_vec();
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(bs) {
        let mut block = block_of::<C>(chunk);
        cipher.decrypt_block(&mut block);
        out.extend(block.iter().zip(prev.iter()).map(|(a, b)| a ^ b));
        prev = chunk.to_vec();
    }
    pkcs7_unpad(&out, bs)
}

/// ECB encrypt with PKCS#7 padding (Java's `Cipher.getInstance("DES")`
/// and `"AES"` defaults). Only for compatibility with legacy data.
pub fn ecb_encrypt<C: BlockCipherEncrypt + KeyInit>(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let cipher = C::new_from_slice(key).map_err(|_| "wrong key length".to_string())?;
    let bs = C::block_size();
    let padded = pkcs7_pad(data, bs);
    let mut out = Vec::with_capacity(padded.len());
    for chunk in padded.chunks(bs) {
        let mut block = block_of::<C>(chunk);
        cipher.encrypt_block(&mut block);
        out.extend_from_slice(&block);
    }
    Ok(out)
}

pub fn ecb_decrypt<C: BlockCipherDecrypt + KeyInit>(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let cipher = C::new_from_slice(key).map_err(|_| "wrong key length".to_string())?;
    let bs = C::block_size();
    if data.is_empty() || !data.len().is_multiple_of(bs) {
        return Err("ciphertext is not a whole number of blocks".into());
    }
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(bs) {
        let mut block = block_of::<C>(chunk);
        cipher.decrypt_block(&mut block);
        out.extend_from_slice(&block);
    }
    pkcs7_unpad(&out, bs)
}

// ---------------------------------------------------------------------------
// AES-256-GCM (Java `AES/GCM/NoPadding`, 128-bit tag, 96-bit IV)

pub const GCM_IV_LEN: usize = 12;

/// `iv || ciphertext || tag`, the layout the gateway sends.
pub fn aes_gcm_encrypt(key: &[u8], iv: &[u8; GCM_IV_LEN], plaintext: &[u8]) -> Result<Vec<u8>> {
    use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
    let k = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, key).map_err(|_| "AES key must be 32 bytes".to_string())?,
    );
    let mut buf = plaintext.to_vec();
    k.seal_in_place_append_tag(Nonce::assume_unique_for_key(*iv), Aad::empty(), &mut buf)
        .map_err(|_| "encryption failed".to_string())?;
    let mut out = iv.to_vec();
    out.extend_from_slice(&buf);
    Ok(out)
}

/// The inverse of `aes_gcm_encrypt`: the IV is the first 12 bytes.
pub fn aes_gcm_decrypt(key: &[u8], message: &[u8]) -> Result<Vec<u8>> {
    use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
    if message.len() < GCM_IV_LEN + 16 {
        return Err("message too short for an IV and a tag".into());
    }
    let k = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, key).map_err(|_| "AES key must be 32 bytes".to_string())?,
    );
    let mut iv = [0u8; GCM_IV_LEN];
    iv.copy_from_slice(&message[..GCM_IV_LEN]);
    let mut buf = message[GCM_IV_LEN..].to_vec();
    let pt = k
        .open_in_place(Nonce::assume_unique_for_key(iv), Aad::empty(), &mut buf)
        .map_err(|_| "decryption failed: wrong shared key or tampered message".to_string())?;
    Ok(pt.to_vec())
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    use rand::RngCore;
    let mut v = vec![0u8; n];
    rand::rng().fill_bytes(&mut v);
    v
}

/// `SHA-256(uuid)` as the gateway's `deriveKeyFromUUID` (32 bytes → AES-256).
pub fn key_from_uuid(uuid: &str) -> [u8; 32] {
    Sha256::digest(uuid.as_bytes()).into()
}

// ---------------------------------------------------------------------------
// Key parsing

/// Base64 body of a PEM block or of a bare Base64 string (Java's
/// `Base64.getEncoder().encodeToString(key.getEncoded())`).
pub fn key_bytes(text: &str) -> Result<Vec<u8>> {
    let body: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("-----"))
        .collect();
    if body.is_empty() {
        return Err("no key given".into());
    }
    b64_decode(&body)
}

/// Minimal DER reader: (tag, content, rest).
fn der_tlv(bytes: &[u8]) -> Result<(u8, &[u8], &[u8])> {
    if bytes.len() < 2 {
        return Err("truncated DER".into());
    }
    let tag = bytes[0];
    let (len, head) = if bytes[1] & 0x80 == 0 {
        (bytes[1] as usize, 2)
    } else {
        let n = (bytes[1] & 0x7F) as usize;
        if n == 0 || n > 4 || bytes.len() < 2 + n {
            return Err("bad DER length".into());
        }
        let mut len = 0usize;
        for b in &bytes[2..2 + n] {
            len = (len << 8) | *b as usize;
        }
        (len, 2 + n)
    };
    if bytes.len() < head + len {
        return Err("truncated DER".into());
    }
    Ok((tag, &bytes[head..head + len], &bytes[head + len..]))
}

fn der_len(len: usize) -> Vec<u8> {
    if len < 0x80 {
        vec![len as u8]
    } else {
        let bytes = len.to_be_bytes();
        let first = bytes
            .iter()
            .position(|b| *b != 0)
            .unwrap_or(bytes.len() - 1);
        let mut v = vec![0x80 | (bytes.len() - first) as u8];
        v.extend_from_slice(&bytes[first..]);
        v
    }
}

fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend(der_len(content.len()));
    v.extend_from_slice(content);
    v
}

const OID_RSA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];

/// What kind of key a DER blob is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyForm {
    Pkcs8Private,
    Pkcs1Private,
    Spki,
    Pkcs1Public,
}

pub fn key_form(der_bytes: &[u8]) -> Result<KeyForm> {
    let (tag, content, _) = der_tlv(der_bytes)?;
    if tag != 0x30 {
        return Err("not a DER SEQUENCE: paste the key as PEM or Base64".into());
    }
    let (t1, c1, rest1) = der_tlv(content)?;
    match t1 {
        // INTEGER first: PKCS#8 (version 0, then AlgorithmIdentifier) or
        // PKCS#1 private (version, then modulus INTEGER) / public (n, e).
        0x02 => {
            let (t2, _, rest2) = der_tlv(rest1)?;
            if t2 == 0x30 {
                Ok(KeyForm::Pkcs8Private)
            } else if t2 == 0x02 && c1.len() <= 1 && !rest2.is_empty() {
                Ok(KeyForm::Pkcs1Private)
            } else {
                Ok(KeyForm::Pkcs1Public)
            }
        }
        0x30 => Ok(KeyForm::Spki),
        _ => Err("unrecognised key structure".into()),
    }
}

/// A private key as PKCS#8 DER, wrapping PKCS#1 when needed.
pub fn private_key_pkcs8(text: &str) -> Result<Vec<u8>> {
    let bytes = key_bytes(text)?;
    match key_form(&bytes)? {
        KeyForm::Pkcs8Private => Ok(bytes),
        KeyForm::Pkcs1Private => {
            let alg = der(0x30, &[der(0x06, OID_RSA), der(0x05, &[])].concat());
            let inner = [der(0x02, &[0]), alg, der(0x04, &bytes)].concat();
            Ok(der(0x30, &inner))
        }
        _ => Err("this is a public key; a private key is needed here".into()),
    }
}

/// The subject public key bits of an X.509 SPKI, or the PKCS#1 key itself.
/// For RSA that is the PKCS#1 `RSAPublicKey`; for EC the uncompressed point.
pub fn public_key_bits(text: &str) -> Result<Vec<u8>> {
    let bytes = key_bytes(text)?;
    match key_form(&bytes)? {
        KeyForm::Spki => {
            let (_, content, _) = der_tlv(&bytes)?;
            let (_, _alg, rest) = der_tlv(content)?;
            let (tag, bits, _) = der_tlv(rest)?;
            if tag != 0x03 || bits.is_empty() {
                return Err("SPKI without a BIT STRING".into());
            }
            Ok(bits[1..].to_vec())
        }
        KeyForm::Pkcs1Public => Ok(bytes),
        _ => Err("this is a private key; a public key is needed here".into()),
    }
}

// ---------------------------------------------------------------------------
// Signatures

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigAlg {
    RsaSha256,
    RsaSha384,
    RsaSha512,
    EcdsaP256Sha256,
}

/// Sign `message` with a private key (PEM or Base64, PKCS#8 or PKCS#1).
pub fn sign(alg: SigAlg, private_key: &str, message: &[u8]) -> Result<Vec<u8>> {
    let der = private_key_pkcs8(private_key)?;
    let rng = ring::rand::SystemRandom::new();
    match alg {
        SigAlg::EcdsaP256Sha256 => {
            let kp = ring::signature::EcdsaKeyPair::from_pkcs8(
                &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                &der,
                &rng,
            )
            .map_err(|e| format!("not a P-256 private key: {e}"))?;
            Ok(kp
                .sign(&rng, message)
                .map_err(|_| "signing failed".to_string())?
                .as_ref()
                .to_vec())
        }
        _ => {
            let kp = ring::signature::RsaKeyPair::from_pkcs8(&der)
                .map_err(|e| format!("not an RSA private key: {e}"))?;
            let padding = match alg {
                SigAlg::RsaSha256 => &ring::signature::RSA_PKCS1_SHA256,
                SigAlg::RsaSha384 => &ring::signature::RSA_PKCS1_SHA384,
                _ => &ring::signature::RSA_PKCS1_SHA512,
            };
            let mut sig = vec![0u8; kp.public().modulus_len()];
            kp.sign(padding, &rng, message, &mut sig)
                .map_err(|_| "signing failed".to_string())?;
            Ok(sig)
        }
    }
}

/// Verify with a public key (PEM or Base64, SPKI or PKCS#1).
pub fn verify(alg: SigAlg, public_key: &str, message: &[u8], signature: &[u8]) -> Result<bool> {
    let bits = public_key_bits(public_key)?;
    let algorithm: &dyn ring::signature::VerificationAlgorithm = match alg {
        SigAlg::RsaSha256 => &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        SigAlg::RsaSha384 => &ring::signature::RSA_PKCS1_2048_8192_SHA384,
        SigAlg::RsaSha512 => &ring::signature::RSA_PKCS1_2048_8192_SHA512,
        SigAlg::EcdsaP256Sha256 => &ring::signature::ECDSA_P256_SHA256_FIXED,
    };
    Ok(ring::signature::UnparsedPublicKey::new(algorithm, bits)
        .verify(message, signature)
        .is_ok())
}

/// The public half of an RSA private key, as PKCS#1 DER (for display and
/// for checking a pair belongs together).
pub fn rsa_public_from_private(private_key: &str) -> Result<Vec<u8>> {
    let der = private_key_pkcs8(private_key)?;
    let kp = ring::signature::RsaKeyPair::from_pkcs8(&der)
        .map_err(|e| format!("not an RSA private key: {e}"))?;
    Ok(kp.public().as_ref().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair;

    #[test]
    fn pbkdf2_sha256_known_answer() {
        // RFC 7914 / common vector: password "password", salt "salt", 1 iteration.
        let dk = pbkdf2_hmac_sha256(b"password", b"salt", 1, 32);
        assert_eq!(
            hex_encode(&dk),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        let dk = pbkdf2_hmac_sha256(b"password", b"salt", 4096, 32);
        assert_eq!(
            hex_encode(&dk),
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
        );
    }

    #[test]
    fn pbkdf1_md5_matches_openssl_style_derivation() {
        // Deterministic: one iteration is plain MD5(password || salt).
        let dk = pbkdf1_md5(b"pw", b"12345678", 1);
        assert_eq!(dk.to_vec(), Md5::digest(b"pw12345678").to_vec());
        let dk2 = pbkdf1_md5(b"pw", b"12345678", 2);
        assert_eq!(dk2.to_vec(), Md5::digest(dk).to_vec());
    }

    #[test]
    fn cbc_and_ecb_round_trip_with_padding() {
        let key = b"0123456789abcdef0123456789abcdef";
        let iv = b"fedcba9876543210";
        for len in [0usize, 1, 15, 16, 17, 100] {
            let data: Vec<u8> = (0..len as u8).collect();
            let ct = cbc_encrypt::<aes::Aes256>(key, iv, &data).unwrap();
            assert_eq!(ct.len() % 16, 0);
            assert_eq!(cbc_decrypt::<aes::Aes256>(key, iv, &ct).unwrap(), data);
            let ct = ecb_encrypt::<des::Des>(b"8bytekey", &data).unwrap();
            assert_eq!(ecb_decrypt::<des::Des>(b"8bytekey", &ct).unwrap(), data);
        }
        assert!(cbc_decrypt::<aes::Aes256>(key, iv, &[0u8; 16]).is_err());
    }

    #[test]
    fn aes_ecb_known_answer_fips197() {
        // FIPS-197 C.1: AES-128, one block, no padding needed for the check.
        let key = hex_decode("000102030405060708090a0b0c0d0e0f").unwrap();
        let pt = hex_decode("00112233445566778899aabbccddeeff").unwrap();
        let ct = ecb_encrypt::<aes::Aes128>(&key, &pt).unwrap();
        assert_eq!(hex_encode(&ct[..16]), "69c4e0d86a7b0430d8cdb78070b4c55a");
    }

    #[test]
    fn gcm_round_trip_and_tamper_detection() {
        let key = key_from_uuid("5b6f1c2e-0000-4000-8000-000000000001");
        let iv = [7u8; 12];
        let msg = aes_gcm_encrypt(&key, &iv, b"{\"hello\":1}").unwrap();
        assert_eq!(&msg[..12], &iv);
        assert_eq!(aes_gcm_decrypt(&key, &msg).unwrap(), b"{\"hello\":1}");
        let mut bad = msg.clone();
        bad[20] ^= 1;
        assert!(aes_gcm_decrypt(&key, &bad).is_err());
    }

    #[test]
    fn b64_decodes_every_flavour() {
        assert_eq!(b64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(b64_decode("aGVsbG8").unwrap(), b"hello");
        assert_eq!(b64_decode("-_-_").unwrap(), [0xFB, 0xFF, 0xBF]);
        assert_eq!(b64_decode("aGVs\nbG8=\n").unwrap(), b"hello");
        assert!(b64_decode("@@").is_err());
    }

    #[test]
    fn der_helpers_and_pkcs1_wrapping() {
        assert_eq!(der_len(5), vec![5]);
        assert_eq!(der_len(200), vec![0x81, 200]);
        assert_eq!(der_len(0x1234), vec![0x82, 0x12, 0x34]);
        let (tag, content, rest) = der_tlv(&[0x02, 0x01, 0x05, 0xFF]).unwrap();
        assert_eq!((tag, content, rest), (0x02, &[5u8][..], &[0xFFu8][..]));
    }

    #[test]
    fn ecdsa_sign_verify_round_trip_from_generated_key() {
        let rng = ring::rand::SystemRandom::new();
        let doc = ring::signature::EcdsaKeyPair::generate_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            &rng,
        )
        .unwrap();
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
            b64_encode(doc.as_ref())
        );
        let sig = sign(SigAlg::EcdsaP256Sha256, &pem, b"msg").unwrap();
        assert_eq!(sig.len(), 64);
        let kp = ring::signature::EcdsaKeyPair::from_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            doc.as_ref(),
            &rng,
        )
        .unwrap();
        // Wrap the raw point in an SPKI the way Java's getEncoded() does.
        let alg = der(
            0x30,
            &[
                der(0x06, &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01]),
                der(0x06, &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07]),
            ]
            .concat(),
        );
        let mut bits = vec![0u8];
        bits.extend_from_slice(kp.public_key().as_ref());
        let spki = der(0x30, &[alg, der(0x03, &bits)].concat());
        let pub_b64 = b64_encode(&spki);
        assert_eq!(key_form(&spki).unwrap(), KeyForm::Spki);
        assert!(verify(SigAlg::EcdsaP256Sha256, &pub_b64, b"msg", &sig).unwrap());
        assert!(!verify(SigAlg::EcdsaP256Sha256, &pub_b64, b"other", &sig).unwrap());
    }
}
