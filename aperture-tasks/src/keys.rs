//! Globally unique registry keys for built-in task definitions.
//!
//! Every task definition must have a unique key. These consts are the single
//! source of truth so that there is no risk of collisions between definitions
//! defined across multiple crates.

/// Fetches an artifact into the blob store.
pub const DOWNLOAD: &str = "download";

/// Re-issues the leaf TLS certificate when it nears expiry.
pub const ROTATE_CERTIFICATE: &str = "rotate-certificate";

/// Re-issues the leaf TLS certificate for a new identity (e.g. hostname).
pub const REGENERATE_CERTIFICATE: &str = "regenerate-certificate";

/// Applies a new hostname to the running system.
pub const APPLY_HOSTNAME: &str = "apply-hostname";
