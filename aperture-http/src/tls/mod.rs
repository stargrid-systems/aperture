//! TLS infrastructure: PKI generation, certificate management, and the
//! hot-swappable TLS listener.
//!
//! Certificates are stored as artifacts. On first run a self-signed CA and
//! server certificate are generated automatically.

use std::sync::Arc;

use aperture_artifacts::Artifacts;
use arc_swap::ArcSwap;

pub use self::error::TlsError;
pub use self::listener::TlsListener;
pub use self::pki::{ensure_certificates, load_server_config, reload_certificates};
pub use self::redirect::redirect_router;
pub use self::reload::TlsReload;
pub use self::rotate::RotateCertificateDefinition;

mod error;
mod listener;
mod pki;
mod redirect;
mod reload;
mod rotate;

/// Shared, hot-swappable server configuration.
pub type SharedConfig = Arc<ArcSwap<rustls::ServerConfig>>;

/// Loads the server config from artifacts and wraps it in a [`SharedConfig`]
/// ready for hot-swapping.
pub async fn load_shared_config(artifacts: &Artifacts) -> Result<SharedConfig, TlsError> {
    let config = load_server_config(artifacts).await?;
    Ok(Arc::new(ArcSwap::from_pointee(config)))
}
