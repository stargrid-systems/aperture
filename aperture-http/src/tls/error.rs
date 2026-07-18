use std::io;

use tokio::task::JoinError;

/// Errors from the TLS subsystem.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("certificate generation failed: {0}")]
    Rcgen(#[from] rcgen::Error),

    #[error("rustls error: {0}")]
    Rustls(#[from] rustls::Error),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("artifact error: {0}")]
    Artifact(#[from] aperture_artifacts::ArtifactError),

    #[error("no server certificate found in artifacts")]
    NoCertificate,

    #[error("certificate parse error: {0}")]
    CertParse(String),

    #[error("blocking task failed: {0}")]
    Join(#[from] JoinError),
}
