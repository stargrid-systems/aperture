//! Subscription filters for the event bus.

/// What events a subscriber wants to receive.
#[derive(Debug, Clone)]
pub enum Subscription {
    /// Receive all events.
    All,
    /// Receive events whose key matches this exact string.
    Key(&'static str),
    /// Receive events whose key matches one of these exact strings.
    Keys(&'static [&'static str]),
    /// Receive events whose key starts with this prefix.
    Prefix(&'static str),
}

impl Subscription {
    /// Returns whether an event with `key` matches this subscription.
    pub fn matches(&self, key: &str) -> bool {
        match self {
            Self::All => true,
            Self::Key(k) => key == *k,
            Self::Keys(keys) => keys.contains(&key),
            Self::Prefix(prefix) => key.starts_with(prefix),
        }
    }
}
