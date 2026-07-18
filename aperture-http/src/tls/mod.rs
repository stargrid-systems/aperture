//! TLS infrastructure: PKI generation, certificate management, and the
//! hot-swappable TLS listener.
//!
//! Certificates are stored as artifacts. On first run a self-signed CA and
//! server certificate are generated automatically.

use std::sync::{Arc, LazyLock};

use aperture_artifacts::Artifacts;
use aperture_storage::ArtifactKey;
use arc_swap::ArcSwap;

pub use self::error::TlsError;
pub use self::listener::TlsListener;
pub use self::pki::{ensure_certificates, load_server_config, reload_certificates};
pub use self::redirect::redirect_router;
pub use self::reload::TlsReload;
pub use self::rotate::{RotateCertificateDefinition, install_default_rotation_schedule};

mod error;
mod listener;
mod pki;
mod redirect;
mod reload;
mod rotate;

// Artifact keys for the gateway's self-signed PKI. Validated once at first
// use; clones afterwards are cheap (Cow::Borrowed).
static CA_CERT: LazyLock<ArtifactKey> =
    LazyLock::new(|| ArtifactKey::new("tls/ca-cert").expect("well-known key"));
static CA_KEY: LazyLock<ArtifactKey> =
    LazyLock::new(|| ArtifactKey::new("tls/ca-key").expect("well-known key"));
static SERVER_CERT: LazyLock<ArtifactKey> =
    LazyLock::new(|| ArtifactKey::new("tls/server-cert").expect("well-known key"));
static SERVER_KEY: LazyLock<ArtifactKey> =
    LazyLock::new(|| ArtifactKey::new("tls/server-key").expect("well-known key"));

/// Shared, hot-swappable server configuration.
pub type SharedConfig = Arc<ArcSwap<rustls::ServerConfig>>;

/// Loads the server config from artifacts and wraps it in a [`SharedConfig`]
/// ready for hot-swapping.
pub async fn load_shared_config(artifacts: &Artifacts) -> Result<SharedConfig, TlsError> {
    let config = load_server_config(artifacts).await?;
    Ok(Arc::new(ArcSwap::from_pointee(config)))
}
