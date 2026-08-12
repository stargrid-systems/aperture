//! Generic definition registry: a typed map from static keys to erased trait
//! objects.
//!
//! Each domain (tasks, settings, events) wraps this in a newtype and adds
//! domain-specific methods for schema projection and registration with typed
//! bounds.

use std::collections::BTreeMap;
use std::sync::Arc;

/// A registry of definitions keyed by a static string.
///
/// Stores type-erased definition objects behind [`Arc`]. Each domain wraps
/// this in a newtype (e.g. `SettingRegistry`, `TaskRegistry`) and adds
/// domain-specific `register` and `descriptors` methods.
///
/// Iteration order is deterministic (sorted by key) so that generated output
/// like the `OpenAPI` spec is reproducible across runs.
pub struct Registry<T: ?Sized + Send + Sync + 'static> {
    map: BTreeMap<&'static str, Arc<T>>,
}

impl<T: ?Sized + Send + Sync + 'static> Registry<T> {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts `definition` under `key`, replacing any existing entry.
    pub fn register(&mut self, key: &'static str, definition: Arc<T>) {
        self.map.insert(key, definition);
    }

    /// Looks up the definition for `key`.
    pub fn get(&self, key: &str) -> Option<&Arc<T>> {
        self.map.get(key)
    }

    /// Iterates over registered keys.
    pub fn keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.map.keys().copied()
    }

    /// Iterates over registered definitions.
    pub fn values(&self) -> impl Iterator<Item = &Arc<T>> + '_ {
        self.map.values()
    }

    /// Returns `true` if no definitions are registered.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl<T: ?Sized + Send + Sync + 'static> Default for Registry<T> {
    fn default() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }
}
