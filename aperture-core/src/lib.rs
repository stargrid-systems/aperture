//! Core domain logic for the Aperture gateway.
//!
//! This crate holds the service layer. It stays free of any HTTP,
//! serialization, or storage framework so the business logic remains portable.

/// Version information about the running gateway.
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Version of the Aperture gateway.
    pub aperture: &'static str,
}

/// The central service that ties together the gateway's capabilities.
#[derive(Debug, Default)]
pub struct Core {
    _private: (),
}

impl Core {
    /// Creates a new core service.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// Returns version information about the gateway.
    pub fn version(&self) -> VersionInfo {
        VersionInfo {
            aperture: env!("CARGO_PKG_VERSION"),
        }
    }
}
