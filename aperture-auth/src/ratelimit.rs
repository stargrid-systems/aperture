//! In-memory login attempt throttling.
//!
//! Slows online password guessing with a progressive per-username backoff.
//! State is in-process and resets on restart, which is acceptable for the
//! current single-node embedded gateway.
//!
//! Keying is by username rather than client IP: axum's connect-info is only
//! available for the plain `TcpListener`, not the custom TLS listener used on
//! the primary HTTPS path, so per-IP keying cannot be applied uniformly today.
//! Username-only still slows guessing against a targeted account. The tradeoff
//! is that an attacker who knows a username can push it into backoff (a
//! lockout-style denial of service), but they can only delay a legitimate
//! login, never bypass it: once the backoff expires, a correct password
//! succeeds and clears the entry.
// TODO(#154): extend the key to (username, IpAddr) once the TLS listener
// exposes peer addressing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{AuthError, Result};

/// Failures allowed before backoff kicks in.
const THRESHOLD: u32 = 5;
/// Base backoff applied at the threshold. Doubles with each further failure.
const BASE_BACKOFF: Duration = Duration::from_secs(2);
/// Maximum backoff, regardless of failure count.
const CAP_BACKOFF: Duration = Duration::from_secs(60);
/// Idle time after which an entry is forgotten, bounding memory and letting a
/// transient lock clear.
const RESET_AFTER: Duration = Duration::from_secs(15 * 60);

/// In-memory login attempt limiter. Cheap to clone: the table is shared.
#[derive(Clone, Default)]
pub struct LoginLimiter {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
}

struct Entry {
    failures: u32,
    locked_until: Option<Instant>,
    last_seen: Instant,
}

impl LoginLimiter {
    /// Returns `Ok(())` when a login attempt may proceed, or
    /// [`AuthError::TooManyAttempts`] when `username` is in backoff.
    pub fn check(&self, username: &str) -> Result<()> {
        let now = Instant::now();
        let mut inner = self.inner.lock().expect("login limiter poisoned");
        let Some(entry) = inner.get_mut(username) else {
            return Ok(());
        };
        entry.last_seen = now;
        match entry.locked_until {
            Some(until) if now < until => Err(AuthError::TooManyAttempts),
            _ => Ok(()),
        }
    }

    /// Records a failed attempt, engaging or extending backoff once the
    /// threshold is reached.
    pub fn record_failure(&self, username: &str) {
        let now = Instant::now();
        let mut inner = self.inner.lock().expect("login limiter poisoned");
        Self::evict_stale(&mut inner, now);
        let entry = inner.entry(username.to_owned()).or_insert(Entry {
            failures: 0,
            locked_until: None,
            last_seen: now,
        });
        entry.failures = entry.failures.saturating_add(1);
        entry.last_seen = now;
        if entry.failures >= THRESHOLD {
            let backoff = Self::backoff_after(entry.failures);
            entry.locked_until = Some(now + backoff);
        }
    }

    /// Records a successful attempt, clearing any backoff for `username`.
    pub fn record_success(&self, username: &str) {
        let mut inner = self.inner.lock().expect("login limiter poisoned");
        inner.remove(username);
    }

    /// Backoff applied after `failures` consecutive failures (>= THRESHOLD).
    /// Pure for unit testing.
    fn backoff_after(failures: u32) -> Duration {
        debug_assert!(failures >= THRESHOLD);
        let exponent = failures - THRESHOLD;
        let scaled = BASE_BACKOFF.saturating_mul(2_u32.saturating_pow(exponent));
        scaled.min(CAP_BACKOFF)
    }

    /// Drops entries idle longer than [`RESET_AFTER`].
    fn evict_stale(inner: &mut HashMap<String, Entry>, now: Instant) {
        inner.retain(|_, entry| now.duration_since(entry.last_seen) < RESET_AFTER);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_attempts_below_threshold() {
        let limiter = LoginLimiter::default();
        for _ in 0..THRESHOLD {
            assert!(limiter.check("alice").is_ok());
            limiter.record_failure("alice");
        }
    }

    #[test]
    fn blocks_once_threshold_reached() {
        let limiter = LoginLimiter::default();
        for _ in 0..THRESHOLD {
            limiter.record_failure("alice");
        }
        assert!(matches!(
            limiter.check("alice"),
            Err(AuthError::TooManyAttempts)
        ));
    }

    #[test]
    fn success_clears_backoff() {
        let limiter = LoginLimiter::default();
        for _ in 0..THRESHOLD {
            limiter.record_failure("alice");
        }
        limiter.record_success("alice");
        assert!(limiter.check("alice").is_ok());
    }

    #[test]
    fn usernames_are_independent() {
        let limiter = LoginLimiter::default();
        for _ in 0..THRESHOLD {
            limiter.record_failure("alice");
        }
        assert!(limiter.check("bob").is_ok());
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(
            LoginLimiter::backoff_after(THRESHOLD),
            Duration::from_secs(2)
        );
        assert_eq!(
            LoginLimiter::backoff_after(THRESHOLD + 1),
            Duration::from_secs(4)
        );
        assert_eq!(
            LoginLimiter::backoff_after(THRESHOLD + 2),
            Duration::from_secs(8)
        );
        assert!(LoginLimiter::backoff_after(1000) <= CAP_BACKOFF);
    }
}
