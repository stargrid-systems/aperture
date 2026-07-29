use std::io;

use tokio::task::JoinError;

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("artifact error: {0}")]
    Artifact(#[from] aperture_artifacts::ArtifactError),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("no TLS artifact found")]
    NoArtifact,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<JoinError> for TlsError {
    fn from(err: JoinError) -> Self {
        Self::Other(err.into())
    }
}
