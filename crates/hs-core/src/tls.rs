//! One rustls client configuration for the protocols that are not HTTP
//! (LDAP first, ADR-033): the **ring** provider, the stack every driver in
//! the workspace builds, so Windows needs no NASM or CMake.
//!
//! Verifying, it trusts the platform's store plus the PEM file the profile
//! names (a company CA) and checks the host name. Not verifying - the
//! profile's Verify TLS off - it accepts any certificate but still checks
//! the handshake signatures, and the screens say so with the red badge
//! (docs/06-security.md §5).

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};

/// The client configuration: verifying against the platform's roots and
/// `ca_file` (empty for none), or accepting any certificate.
pub fn client_config(verify: bool, ca_file: &str) -> Result<Arc<ClientConfig>, String> {
    let p = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(p.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?;
    let verifier: Arc<dyn ServerCertVerifier> = if verify {
        let extra = read_ca(ca_file)?;
        Arc::new(
            rustls_platform_verifier::Verifier::new_with_extra_roots(extra, p)
                .map_err(|e| e.to_string())?,
        )
    } else {
        Arc::new(NoVerify(p))
    };
    Ok(Arc::new(
        builder
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth(),
    ))
}

/// The certificates of a PEM file, none for an empty path; a missing or
/// unreadable file is said in words.
pub fn read_ca(path: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(Vec::new());
    }
    let certs: Result<Vec<_>, _> = CertificateDer::pem_file_iter(path)
        .map_err(|e| format!("the CA file {path} could not be read: {e}"))?
        .collect();
    let certs = certs.map_err(|e| format!("the CA file {path} is not PEM: {e}"))?;
    if certs.is_empty() {
        return Err(format!("the CA file {path} holds no certificate"));
    }
    Ok(certs)
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
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
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
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_builds_both_ways_and_a_bad_ca_file_is_said() {
        assert!(client_config(true, "").is_ok());
        assert!(client_config(false, "").is_ok());
        let missing = client_config(true, "no/such/ca.pem").unwrap_err();
        assert!(missing.contains("no/such/ca.pem"), "{missing}");
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.pem");
        std::fs::write(&empty, "not a certificate").unwrap();
        let e = read_ca(empty.to_str().unwrap()).unwrap_err();
        assert!(e.contains("no certificate"), "{e}");
    }
}
