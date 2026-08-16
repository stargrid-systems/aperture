//! Type-erased event payloads and the dispatch envelope.
//!
//! The dispatch path never touches serde: payloads travel as
//! [`SmallBox<dyn ErasedPayload>`] (up to 64 bytes inline, heap fallback
//! beyond), typed subscribers downcast through [`Any`], and JSON exists only
//! when a consumer asks for it via [`EventEnvelope::payload_json`].

use std::any::Any;
use std::fmt;

use aperture_storage::{ActorId, EventId};
use jiff::Timestamp;
use serde_json::Value;
use smallbox::SmallBox;
use smallbox::space::S8;

use crate::definition::EventDefinition;

/// A type-erased event payload.
///
/// Every [`EventDefinition`] implements this through a blanket impl, so the
/// vtable is generated per payload type: it carries the event key, lazy JSON
/// serialization, cloning into an inline-or-heap box, and downcasting.
trait ErasedPayload: Send + Sync + 'static {
    /// The event key of the payload's definition.
    fn key(&self) -> &'static str;

    /// Clones the payload into a fresh inline-or-heap box.
    fn clone_boxed(&self) -> SmallBox<dyn ErasedPayload, S8>;

    /// Serializes the payload to JSON.
    fn to_json(&self) -> Result<Value, serde_json::Error>;

    /// Returns the payload for downcasting.
    fn as_any(&self) -> &dyn Any;
}

impl<D: EventDefinition> ErasedPayload for D {
    fn key(&self) -> &'static str {
        D::KEY
    }

    fn clone_boxed(&self) -> SmallBox<dyn ErasedPayload, S8> {
        smallbox::smallbox!(self.clone())
    }

    fn to_json(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A live event on the bus: the typed payload plus envelope metadata.
///
/// Cheap to clone: payloads up to 64 bytes sit inline, so cloning is a copy
/// with no allocation. The id is a `UUIDv7` assigned at emit time.
pub struct EventEnvelope {
    /// Unique id, also the primary key of the persisted row.
    pub id: EventId,
    /// Actor that triggered the event.
    pub actor: ActorId,
    /// When the event was emitted.
    pub timestamp: Timestamp,
    payload: SmallBox<dyn ErasedPayload, S8>,
}

impl Clone for EventEnvelope {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            actor: self.actor,
            timestamp: self.timestamp,
            payload: self.payload.clone_boxed(),
        }
    }
}

impl EventEnvelope {
    /// Wraps `payload` with a fresh id and `timestamp`.
    pub(crate) fn new<D: EventDefinition>(
        payload: D,
        actor: ActorId,
        timestamp: Timestamp,
    ) -> Self {
        Self {
            id: EventId::generate(),
            actor,
            timestamp,
            payload: smallbox::smallbox!(payload),
        }
    }

    /// The event key of the carried payload.
    pub fn key(&self) -> &'static str {
        self.payload.key()
    }

    /// Serializes the payload to JSON.
    ///
    /// Only consumers that persist or relay events need this. The dispatch
    /// path never calls it.
    ///
    /// # Errors
    ///
    /// Returns the serde error if the payload fails to serialize.
    pub fn payload_json(&self) -> Result<Value, serde_json::Error> {
        self.payload.to_json()
    }

    /// Returns the typed payload if this envelope carries `D`, which holds
    /// when the event key matches `D::KEY`.
    pub fn payload<D: EventDefinition>(&self) -> Option<&D> {
        self.payload.as_any().downcast_ref::<D>()
    }
}

impl fmt::Debug for EventEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventEnvelope")
            .field("id", &self.id)
            .field("key", &self.key())
            .field("actor", &self.actor)
            .field("timestamp", &self.timestamp)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use serde::{Deserialize, Serialize};
    use utoipa::ToSchema;

    use super::*;

    #[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
    struct Small {
        n: u32,
    }

    impl EventDefinition for Small {
        const KEY: &'static str = "test.small";
    }

    /// Mirrors the layout of the largest current payload
    /// (`SettingChange`: a `String` plus a `serde_json::Value`).
    #[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
    struct Large {
        key: String,
        value: Value,
    }

    impl EventDefinition for Large {
        const KEY: &'static str = "test.large";
    }

    /// Fixed-layout payloads (e.g. `ArtifactWritten`) travel inline.
    /// Payloads carrying a `serde_json::Value` (e.g. `SettingChange`, 96
    /// bytes with `preserve_order` enabled workspace-wide by the pinned
    /// dicebear crates) spill to the heap fallback, covered below.
    #[test]
    fn fixed_layout_payloads_fit_inline() {
        // Mirrors ArtifactWritten (String + Option<String>).
        #[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
        struct Written {
            key: String,
            digest: Option<String>,
        }

        assert!(size_of::<Written>() <= size_of::<S8>());
    }

    #[test]
    fn oversized_payload_still_works() {
        #[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
        struct Huge {
            words: [u64; 32],
        }

        impl EventDefinition for Huge {
            const KEY: &'static str = "test.huge";
        }

        let envelope =
            EventEnvelope::new(Huge { words: [0; 32] }, ActorId::SYSTEM, Timestamp::now());
        assert_eq!(envelope.key(), "test.huge");
        assert_eq!(envelope.payload::<Huge>().unwrap().words.len(), 32);
    }

    #[test]
    fn downcast_returns_typed_payload() {
        let envelope = EventEnvelope::new(Small { n: 7 }, ActorId::SYSTEM, Timestamp::now());
        assert_eq!(envelope.key(), "test.small");
        assert_eq!(envelope.payload::<Small>().unwrap().n, 7);
        assert!(envelope.payload::<Large>().is_none());
    }

    #[test]
    fn clone_copies_inline_payload() {
        let envelope = EventEnvelope::new(Small { n: 7 }, ActorId::SYSTEM, Timestamp::now());
        let clone = envelope.clone();
        assert_eq!(clone.payload::<Small>().unwrap().n, 7);
        assert_eq!(clone.id, envelope.id);
    }

    #[test]
    fn json_is_lazy_and_correct() {
        let envelope = EventEnvelope::new(Small { n: 7 }, ActorId::SYSTEM, Timestamp::now());
        assert_eq!(
            envelope.payload_json().unwrap(),
            serde_json::json!({ "n": 7 })
        );
    }
}
