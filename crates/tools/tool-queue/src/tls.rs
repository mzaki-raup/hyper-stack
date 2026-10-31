//! The one TLS client configuration every broker kind uses: rustls with
//! the **ring** provider, the same stack as the database and Redis
//! drivers, verifying against the platform's trust store.

use std::sync::{Arc, Once};

use rustls::ClientConfig;

/// Some clients (NATS among them) build their rustls config with
/// `ClientConfig::builder()`, which needs a process-wide provider when
/// more than one is compiled in (docs/GOTCHAS.md, Database Drivers).
pub fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// A verifying client config.
pub fn client_config() -> Result<Arc<ClientConfig>, String> {
    let p = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls_platform_verifier::Verifier::new(p.clone()).map_err(|e| e.to_string())?;
    let cfg = ClientConfig::builder_with_provider(p)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}
