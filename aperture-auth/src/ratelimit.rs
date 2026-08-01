//! In-memory login attempt throttling.
//!
//! Slows online password guessing two ways: a progressive per-username backoff
//! and a global counter that slows password spraying across many usernames.
//! State lives in-process and resets on restart, which is fine for the current
//! single-node embedded gateway.
//!
//! Keying is by username rather than client IP: axum's connect-info is only
//! available for the plain `TcpListener`, not the custom TLS listener used on
//! the primary HTTPS path, so per-IP keying cannot be applied uniformly today.
//! Username-only still slows guessing against a targeted account. The tradeoff
//! is that an attacker who knows a username can push it into backoff (a
//! lockout-style denial of service), but they can only delay a legitimate
//! login, never bypass it: once the backoff expires, a correct password
//! succeeds and clears the entry.
//!
//! `check` only reads backoff state. It does not refresh `last_seen`, so stale
//! entries can still be evicted while a client polls the limiter.
// TODO(#154): extend the key to (username, IpAddr) once the TLS listener
// exposes peer addressing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::error::{AuthError, Result};

/// Failures allowed before per-username backoff kicks in.
const THRESHOLD: u32 = 5;
/// Base backoff applied at the threshold. Doubles with each further failure.
const BASE_BACKOFF: Duration = Duration::from_secs(2);
/// Maximum backoff, regardless of failure count.
const CAP_BACKOFF: Duration = Duration::from_secs(60);
/// Seconds of idle time after which an entry is forgotten, bounding memory and
/// letting a transient lock clear.
const RESET_SECS: u64 = 15 * 60;
/// Idle time after which an entry is forgotten.
const RESET_AFTER: Duration = Duration::from_secs(RESET_SECS);
/// Total failures across all usernames before global backoff kicks in. Slows
/// password spraying that spreads attempts across many accounts.
const GLOBAL_THRESHOLD: u32 = 30;

/// In-memory login attempt limiter. Cheap to clone: the table is shared.
#[derive(Clone, Default)]
pub struct LoginLimiter {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    per_user: HashMap<String, Entry>,
    global: Entry,
}

struct Entry {
    failures: u32,
    locked_until: Option<Instant>,
    last_seen: Instant,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            failures: 0,
            locked_until: None,
            last_seen: Instant::now(),
        }
    }
}

impl LoginLimiter {
    /// Returns `Ok(())` when a login attempt may proceed, or
    /// [`AuthError::TooManyAttempts`] when `username` or the global counter is
    /// in backoff.
    pub fn check(&self, username: &str) -> Result<()> {
        self.check_at(username, Instant::now())
    }

    /// Records a failed attempt, engaging backoff once the threshold is
    /// reached.
    pub fn record_failure(&self, username: &str) {
        self.record_failure_at(username, Instant::now());
    }

    /// Records a successful attempt, clearing per-username backoff for
    /// `username`.
    pub fn record_success(&self, username: &str) {
        self.record_success_at(username, Instant::now());
    }

    /// Time-injectable variant of [`check`](Self::check). Read-only: it never
    /// mutates `last_seen`.
    fn check_at(&self, username: &str, now: Instant) -> Result<()> {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = inner.per_user.get(username)
            && matches!(entry.locked_until, Some(until) if now < until)
        {
            return Err(AuthError::TooManyAttempts);
        }
        if matches!(inner.global.locked_until, Some(until) if now < until) {
            return Err(AuthError::TooManyAttempts);
        }
        Ok(())
    }

    /// Time-injectable variant of [`record_failure`](Self::record_failure).
    fn record_failure_at(&self, username: &str, now: Instant) {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        Self::evict_stale(&mut inner, now);
        let entry = inner.per_user.entry(username.to_owned()).or_insert(Entry {
            failures: 0,
            locked_until: None,
            last_seen: now,
        });
        entry.failures = entry.failures.saturating_add(1);
        entry.last_seen = now;
        if entry.failures >= THRESHOLD {
            entry.locked_until = Some(now + Self::backoff_after(entry.failures));
        }
        inner.global.failures = inner.global.failures.saturating_add(1);
        inner.global.last_seen = now;
        if inner.global.failures >= GLOBAL_THRESHOLD {
            let backoff = Self::backoff_from(inner.global.failures, GLOBAL_THRESHOLD);
            inner.global.locked_until = Some(now + backoff);
        }
    }

    /// Time-injectable variant of [`record_success`](Self::record_success).
    fn record_success_at(&self, username: &str, now: Instant) {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = inner.per_user.get_mut(username) {
            entry.last_seen = now;
        }
        Self::evict_stale(&mut inner, now);
        inner.per_user.remove(username);
    }

    /// Backoff applied after `failures` consecutive failures (>= THRESHOLD).
    /// Pure for unit testing.
    fn backoff_after(failures: u32) -> Duration {
        Self::backoff_from(failures, THRESHOLD)
    }

    /// Backoff applied after `failures` failures past `threshold`. Pure for
    /// unit testing.
    fn backoff_from(failures: u32, threshold: u32) -> Duration {
        debug_assert!(failures >= threshold);
        let exponent = failures - threshold;
        BASE_BACKOFF
            .checked_mul(2_u32.checked_pow(exponent).unwrap_or(u32::MAX))
            .unwrap_or(CAP_BACKOFF)
            .min(CAP_BACKOFF)
    }

    /// Drops entries idle longer than [`RESET_AFTER`] and resets the global
    /// counter if it is stale.
    fn evict_stale(inner: &mut Inner, now: Instant) {
        inner
            .per_user
            .retain(|_, entry| now.duration_since(entry.last_seen) < RESET_AFTER);
        if now.duration_since(inner.global.last_seen) >= RESET_AFTER {
            inner.global = Entry {
                failures: 0,
                locked_until: None,
                last_seen: now,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_attempts_below_threshold() {
        let limiter = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..THRESHOLD {
            assert!(limiter.check_at("alice", now).is_ok());
            limiter.record_failure_at("alice", now);
        }
    }

    #[test]
    fn blocks_once_threshold_reached() {
        let limiter = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..THRESHOLD {
            limiter.record_failure_at("alice", now);
        }
        assert!(matches!(
            limiter.check_at("alice", now),
            Err(AuthError::TooManyAttempts)
        ));
    }

    #[test]
    fn success_clears_backoff() {
        let limiter = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..THRESHOLD {
            limiter.record_failure_at("alice", now);
        }
        limiter.record_success_at("alice", now);
        assert!(limiter.check_at("alice", now).is_ok());
    }

    #[test]
    fn usernames_are_independent() {
        let limiter = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..THRESHOLD {
            limiter.record_failure_at("alice", now);
        }
        assert!(limiter.check_at("bob", now).is_ok());
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

    #[test]
    fn backoff_expires_after_window() {
        let limiter = LoginLimiter::default();
        let mut now = Instant::now();
        for _ in 0..THRESHOLD {
            limiter.record_failure_at("alice", now);
        }
        assert!(matches!(
            limiter.check_at("alice", now),
            Err(AuthError::TooManyAttempts)
        ));
        now += LoginLimiter::backoff_after(THRESHOLD) + Duration::from_nanos(1);
        assert!(limiter.check_at("alice", now).is_ok());
    }

    #[test]
    fn correct_attempt_blocked_during_backoff() {
        let limiter = LoginLimiter::default();
        let mut now = Instant::now();
        for _ in 0..THRESHOLD {
            limiter.record_failure_at("alice", now);
        }
        assert!(matches!(
            limiter.check_at("alice", now),
            Err(AuthError::TooManyAttempts)
        ));
        now += LoginLimiter::backoff_after(THRESHOLD) + Duration::from_nanos(1);
        assert!(limiter.check_at("alice", now).is_ok());
    }

    #[test]
    fn global_threshold_engages() {
        let limiter = LoginLimiter::default();
        let now = Instant::now();
        let usernames = ["alice", "bob", "carol", "dave", "eve"];
        for name in &usernames {
            for _ in 0..6 {
                limiter.record_failure_at(name, now);
            }
        }
        assert!(matches!(
            limiter.check_at("frank", now),
            Err(AuthError::TooManyAttempts)
        ));
    }

    #[test]
    fn success_does_not_reset_global() {
        let limiter = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..GLOBAL_THRESHOLD {
            limiter.record_failure_at("alice", now);
        }
        assert!(matches!(
            limiter.check_at("bob", now),
            Err(AuthError::TooManyAttempts)
        ));
        limiter.record_success_at("alice", now);
        assert!(matches!(
            limiter.check_at("bob", now),
            Err(AuthError::TooManyAttempts)
        ));
    }
}
