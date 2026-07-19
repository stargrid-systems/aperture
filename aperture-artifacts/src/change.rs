//! Change feed for the artifact store.
//!
//! [`Artifacts::subscribe`] returns a [`Receiver`] that observes every
//! successful write or removal. The feed is in-process and best-effort: late
//! subscribers do not see events from before they subscribed, and a full
//! channel drops events (the next subscriber-visible event still arrives).
//!
//! Use the feed to react to artifact changes without coupling writers to
//! consumers. The TLS subsystem, for example, reloads certificates when
//! `tls/server-cert` or `tls/server-key` changes, replacing the previous
//! `tls/` prefix sniff in the HTTP upload handler.
//!
//! [`Artifacts::subscribe`]: crate::Artifacts::subscribe
//! [`Receiver`]: tokio::sync::broadcast::Receiver

use aperture_storage::ArtifactKey;

use crate::digest::Digest;

/// What happened to an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactChange {
    /// The artifact that changed.
    pub key: ArtifactKey,
    /// What kind of change occurred.
    pub kind: ChangeKind,
    /// Content digest of the new latest version (Written), or `None` for
    /// Removed or when the writer could not compute it.
    ///
    /// Subscribers that only care about content-level changes can compare
    /// this against the last digest they saw and skip no-op refreshes.
    pub digest: Option<Digest>,
}

/// Kind of artifact change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// A new or replacement version was recorded.
    Written,
    /// A version was evicted.
    Removed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_with_same_key_kind_and_digest_are_equal() {
        let key = ArtifactKey::new("spectra").unwrap();
        let digest: Digest = "sha256:0123".parse().unwrap();
        let a = ArtifactChange {
            key: key.clone(),
            kind: ChangeKind::Written,
            digest: Some(digest.clone()),
        };
        let b = ArtifactChange {
            key,
            kind: ChangeKind::Written,
            digest: Some(digest),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn changes_with_different_kinds_are_not_equal() {
        let key = ArtifactKey::new("spectra").unwrap();
        let written = ArtifactChange {
            key: key.clone(),
            kind: ChangeKind::Written,
            digest: None,
        };
        let removed = ArtifactChange {
            key,
            kind: ChangeKind::Removed,
            digest: None,
        };
        assert_ne!(written, removed);
    }
}
