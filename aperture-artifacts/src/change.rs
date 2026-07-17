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

/// What happened to an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactChange {
    /// The artifact that changed.
    pub key: ArtifactKey,
    /// What kind of change occurred.
    pub kind: ChangeKind,
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
    fn change_eq_ignores_digest() {
        // ArtifactChange carries only the key and the kind. The change feed
        // does not include the digest so that subscribers can match on key
        // without having to know about the underlying content identity.
        let key = ArtifactKey::new("spectra").unwrap();
        let a = ArtifactChange {
            key: key.clone(),
            kind: ChangeKind::Written,
        };
        let b = ArtifactChange {
            key,
            kind: ChangeKind::Written,
        };
        assert_eq!(a, b);
    }
}
