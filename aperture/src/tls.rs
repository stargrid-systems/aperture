//! TLS infrastructure: PKI generation, certificate management, and the
//! hot-swappable TLS listener.
//!
//! Certificates are stored as artifacts (`tls/ca-cert`, `tls/ca-key`,
//! `tls/server-cert`, `tls/server-key`). On first run a self-signed CA and
//! server certificate are generated automatically. New certificates uploaded
//! via the artifact API trigger a live reload.

use std::sync::Arc;

use arc_swap::ArcSwap;

pub use self::listener::TlsListener;
pub use self::pki::{ensure_certificates, load_server_config, reload_certificates};
pub use self::redirect::redirect_router;
pub use self::rotate::RotateCertificateDefinition;

mod error;
mod listener;
mod pki;
mod redirect;
mod rotate;

/// Shared, hot-swappable server configuration.
pub type SharedConfig = Arc<ArcSwap<rustls::ServerConfig>>;
