//! TLS: PKI generation, certificate management, and hot-swappable listener.
//!
//! The PKI keys live in the artifact store with no access control today.
//! Treat the artifact API as equally trusted with the data directory.

use std::sync::Arc;

use aperture_artifacts::Artifacts;
use aperture_storage::ArtifactKey;
use arc_swap::ArcSwap;
use tokio::net::TcpListener;

pub use self::error::TlsError;
pub use self::listener::TlsListener;
pub use self::pki::{ensure_certificates, load_server_config, regenerate_leaf_for_identity};
pub use self::redirect::redirect_router;
pub use self::reload::TlsReload;
pub use self::rotate::{RotateCertificateDefinition, install_default_rotation_schedule};

mod error;
mod listener;
mod pki;
mod redirect;
mod reload;
mod rotate;

const CA_CERT: ArtifactKey = ArtifactKey::from_static("tls_ca-cert");
const CA_KEY: ArtifactKey = ArtifactKey::from_static("tls_ca-key");
const SERVER_CERT: ArtifactKey = ArtifactKey::from_static("tls_server-cert");
const SERVER_KEY: ArtifactKey = ArtifactKey::from_static("tls_server-key");

/// Shared, hot-swappable server configuration.
pub type SharedConfig = Arc<ArcSwap<rustls::ServerConfig>>;

/// A TLS listener paired with its certificate reload watcher.
pub struct TlsEndpoint {
    listener: TlsListener,
    reload: TlsReload,
}

impl TlsEndpoint {
    /// Loads the current cert and wires the change feed.
    pub async fn new(artifacts: Artifacts, tcp_listener: TcpListener) -> Result<Self, TlsError> {
        let shared = load_shared_config(&artifacts).await?;
        Ok(Self {
            listener: TlsListener::new(tcp_listener, shared.clone()),
            reload: TlsReload::new(artifacts, shared),
        })
    }

    /// Splits into listener and reload watcher.
    pub fn into_parts(self) -> (TlsListener, TlsReload) {
        (self.listener, self.reload)
    }
}

/// Loads the server config wrapped in a [`SharedConfig`] for hot-swapping.
pub async fn load_shared_config(artifacts: &Artifacts) -> Result<SharedConfig, TlsError> {
    let config = load_server_config(artifacts).await?;
    Ok(Arc::new(ArcSwap::from_pointee(config)))
}
