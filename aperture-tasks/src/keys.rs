//! Globally unique registry keys for built-in task kinds.
//!
//! Every task definition must have a unique key. These consts are the single
//! source of truth so that there is no risk of collisions between task kinds
//! defined across multiple crates.

/// Fetches an artifact into the blob store.
pub const DOWNLOAD: &str = "download";

/// Re-issues the leaf TLS certificate when it nears expiry.
pub const ROTATE_CERTIFICATE: &str = "rotate-certificate";
