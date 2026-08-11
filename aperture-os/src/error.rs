#[derive(Debug, thiserror::Error)]
pub enum OsError {
    #[error("D-Bus error")]
    Dbus(#[from] zbus::Error),

    #[error("invalid hostname: {0}")]
    InvalidHostname(&'static str),
}
