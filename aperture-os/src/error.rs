/// Validation errors for a hostname.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostnameError {
    #[error("hostname must be 1-253 characters")]
    InvalidLength,
    #[error("hostname contains an empty label")]
    EmptyLabel,
    #[error("hostname label exceeds 63 characters")]
    LabelTooLong,
    #[error("hostname label contains invalid characters")]
    InvalidChars,
    #[error("hostname label cannot start or end with a hyphen")]
    HyphenAtEdge,
}
