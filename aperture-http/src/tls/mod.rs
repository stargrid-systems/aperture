//! TLS infrastructure: PKI generation, certificate management, and the
//! hot-swappable TLS listener.
//!
//! Certificates are stored as artifacts. On first run a self-signed CA and
//! server certificate are generated automatically.
//!
//! # Threat model
//!
//! The well-known PKI keys (`tls/ca-cert`, `tls/ca-key`, `tls/server-cert`,
//! `tls/server-key`) live in the same artifact store as everything else the
//! gateway manages. There is no per-key namespace reservation today. Any
//! caller with write access to the artifact API can therefore overwrite the
//! CA private key with their own and have the gateway mint certificates
//! signed by it on the next rotation.
//!
//! Concretely: treat the artifact API as equally trusted with the gateway's
//! data directory. Do not expose it on an untrusted network until the
//! authentication layer ships. Replacing `tls/ca-key` is sufficient to
//! compromise every derived cert.
//!
//! This module is private to the crate. The crate boundary exposes only
//! `RotateCertificateDefinition`, `install_default_rotation_schedule`, and
//! `init_crypto_provider` (see `lib.rs`). Everything else stays internal so
//! the `TlsEndpoint` pairing of listener + reload watcher can be enforced
//! structurally without leaking implementation types.

use std::sync::Arc;

use aperture_artifacts::Artifacts;
use aperture_storage::ArtifactKey;
use arc_swap::ArcSwap;
use tokio::net::TcpListener;

pub use self::error::TlsError;
pub use self::listener::TlsListener;
pub use self::pki::{ensure_certificates, load_server_config};
pub use self::redirect::redirect_router;
pub use self::reload::TlsReload;
pub use self::rotate::{RotateCertificateDefinition, install_default_rotation_schedule};

mod error;
mod listener;
mod pki;
mod redirect;
mod reload;
mod rotate;

// Artifact keys for the gateway's self-signed PKI. `const` (not `LazyLock`)
// because validation is a `const fn`, so these resolve at compile time.
const CA_CERT: ArtifactKey = ArtifactKey::from_static("tls/ca-cert");
const CA_KEY: ArtifactKey = ArtifactKey::from_static("tls/ca-key");
const SERVER_CERT: ArtifactKey = ArtifactKey::from_static("tls/server-cert");
const SERVER_KEY: ArtifactKey = ArtifactKey::from_static("tls/server-key");

/// Shared, hot-swappable server configuration.
pub type SharedConfig = Arc<ArcSwap<rustls::ServerConfig>>;

/// A TLS listener paired with its certificate reload watcher.
///
/// Construction guarantees both pieces exist together: the listener cannot
/// outlive its reload watcher, and a reload watcher cannot run without a
/// listener to consume the swaps. `HttpServer::serve_tls` is the only way
/// to attach TLS to the server, so the invariant is structural.
///
/// Build one with [`TlsEndpoint::new`], which loads the current cert from
/// artifacts and wires the change feed to the shared config.
pub struct TlsEndpoint {
    listener: TlsListener,
    reload: TlsReload,
}

impl TlsEndpoint {
    /// Builds a TLS endpoint from `tcp_listener`, loading the current cert
    /// from `artifacts` and subscribing to future changes.
    pub async fn new(artifacts: Artifacts, tcp_listener: TcpListener) -> Result<Self, TlsError> {
        let shared = load_shared_config(&artifacts).await?;
        Ok(Self {
            listener: TlsListener::new(tcp_listener, shared.clone()),
            reload: TlsReload::new(artifacts, shared),
        })
    }

    /// Splits the endpoint into its listener and reload watcher.
    ///
    /// Used by `HttpServer::run` to spawn the two halves independently.
    pub fn into_parts(self) -> (TlsListener, TlsReload) {
        (self.listener, self.reload)
    }
}

/// Loads the server config from artifacts and wraps it in a [`SharedConfig`]
/// ready for hot-swapping.
pub async fn load_shared_config(artifacts: &Artifacts) -> Result<SharedConfig, TlsError> {
    let config = load_server_config(artifacts).await?;
    Ok(Arc::new(ArcSwap::from_pointee(config)))
}

/// Installs the `ring` crypto provider as the process-wide default.
///
/// Panics if a different provider was already installed. The chosen provider
/// drives cipher suite selection and certificate signing, so silently falling
/// back to a different one would change the security posture without the
/// operator's knowledge.
pub fn init_crypto_provider() {
    use rustls::crypto::ring;

    ring::default_provider()
        .install_default()
        .expect("a crypto provider is already installed, refusing to silently override it");
}
