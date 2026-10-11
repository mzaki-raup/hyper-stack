//! rustls client configuration for the `ssl` modes of a DB profile
//! (docs/06-security.md §5, same vocabulary as libpq):
//!
//! - `disable`: no TLS.
//! - `prefer` / `require`: encrypted, certificate not checked (libpq's
//!   meaning: protects against eavesdropping, not against impersonation).
//! - `verify-ca` / `verify-full`: the platform's trust store plus `ssl_ca`,
//!   with the host name checked in both (stricter than libpq's `verify-ca`).
//!
//! The `ring` provider is used explicitly so the Windows build needs no
//! NASM/CMake (CLAUDE.md, SSH gotcha).

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};

use super::{DbError, Result};

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Accepts any certificate but still checks the handshake signatures.
#[derive(Debug)]
struct NoVerify(Arc<CryptoProvider>);

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Whether the mode verifies the server certificate.
pub fn verifies(mode: &str) -> bool {
    matches!(mode, "verify-ca" | "verify-full")
}

/// Extra trust anchors from a PEM file.
pub fn read_ca(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let certs: std::result::Result<Vec<_>, _> = CertificateDer::pem_file_iter(path)
        .map_err(|e| DbError::msg(format!("CA file {path}: {e}")))?
        .collect();
    certs.map_err(|e| DbError::msg(format!("CA file {path}: {e}")))
}

/// The client config for a mode, or `None` for `disable`.
pub fn client_config(mode: &str, ca: Option<&str>) -> Result<Option<Arc<ClientConfig>>> {
    if mode == "disable" {
        return Ok(None);
    }
    let p = provider();
    let builder = ClientConfig::builder_with_provider(p.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| DbError::msg(e.to_string()))?;
    let cfg = if verifies(mode) {
        let extra = match ca.filter(|c| !c.trim().is_empty()) {
            Some(path) => read_ca(path)?,
            None => Vec::new(),
        };
        let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(extra, p)
            .map_err(|e| DbError::msg(e.to_string()))?;
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth()
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerify(p)))
            .with_no_client_auth()
    };
    Ok(Some(Arc::new(cfg)))
}
