//! Well-known artifact keys: the canonical identifiers for components the
//! gateway itself manages or fetches.
//!
//! These are the only keys with established meaning across crates. Custom and
//! client-driven artifacts can use any validated [`ArtifactKey`].
//!
//! [`ArtifactKey`]: aperture_storage::ArtifactKey

use std::sync::LazyLock;

use aperture_storage::ArtifactKey;

/// Keys for the gateway's self-signed TLS PKI, managed by the TLS subsystem.
pub mod tls {
    use super::*;

    /// The self-signed CA certificate (PEM).
    pub static CA_CERT: LazyLock<ArtifactKey> =
        LazyLock::new(|| ArtifactKey::new("tls/ca-cert").expect("well-known key"));
    /// The CA private key (PEM).
    pub static CA_KEY: LazyLock<ArtifactKey> =
        LazyLock::new(|| ArtifactKey::new("tls/ca-key").expect("well-known key"));
    /// The leaf server certificate (PEM), signed by [`CA_CERT`].
    pub static SERVER_CERT: LazyLock<ArtifactKey> =
        LazyLock::new(|| ArtifactKey::new("tls/server-cert").expect("well-known key"));
    /// The leaf server private key (PEM).
    pub static SERVER_KEY: LazyLock<ArtifactKey> =
        LazyLock::new(|| ArtifactKey::new("tls/server-key").expect("well-known key"));
}

/// Keys for the Spectra frontend bundle.
pub mod spectra {
    use super::*;

    /// The Spectra squashfs bundle.
    pub static SPECTRA: LazyLock<ArtifactKey> =
        LazyLock::new(|| ArtifactKey::new("spectra").expect("well-known key"));
}
