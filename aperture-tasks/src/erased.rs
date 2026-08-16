//! Type-erased view of a [`TaskDefinition`].
//!
//! The registry stores definitions behind this trait so it can hold many keys
//! together. Erasure happens only here: JSON is decoded into the key's typed
//! input on the way in, and the typed output is encoded back out. The body
//! never sees a [`Value`]. A blanket impl bridges every [`TaskDefinition`].

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use aperture_runtime::{RegistryEntry, json_schema};
use futures_util::FutureExt;
use serde::Deserialize;
use serde_json::Value;
use tokio::task::{AbortHandle, JoinSet};
use tracing::Instrument;

use crate::context::TaskContext;
use crate::definition::{Capabilities, TaskDefinition};
use crate::error::{RunError, TaskError};

/// A public description of one registered key: identity and capabilities.
/// The JSON Schemas are derived on demand from the erased trait object.
pub struct TaskDescriptor {
    /// The key string.
    pub key: &'static str,
    /// What the key supports.
    pub capabilities: Capabilities,
}

pub trait ErasedTaskDefinition: Send + Sync + 'static {
    /// The key this definition is registered under.
    fn key(&self) -> &'static str;
    /// Builds a descriptor with identity and capabilities.
    fn descriptor(&self) -> TaskDescriptor;
    /// A standalone JSON Schema document of the key's input type.
    fn input_schema(&self) -> Value;
    /// A standalone JSON Schema document of the key's output type.
    fn output_schema(&self) -> Value;
    /// Checks that `input` decodes into the key's input type.
    ///
    /// # Errors
    ///
    /// Returns `TaskError::DecodeInput` if the input does not deserialize.
    fn validate(&self, input: &Value) -> Result<(), TaskError>;
    /// Spawns the task onto `set`. The future decodes the input, runs the body,
    /// encodes the output, and records the outcome through `ctx`.
    fn spawn_on(
        self: Arc<Self>,
        input: Value,
        ctx: TaskContext,
        set: &mut JoinSet<()>,
    ) -> AbortHandle;
}

impl RegistryEntry for dyn ErasedTaskDefinition {
    fn key(&self) -> &'static str {
        ErasedTaskDefinition::key(self)
    }
}

impl<T: TaskDefinition> ErasedTaskDefinition for T {
    fn key(&self) -> &'static str {
        T::KEY
    }

    fn descriptor(&self) -> TaskDescriptor {
        TaskDescriptor {
            key: T::KEY,
            capabilities: TaskDefinition::capabilities(self),
        }
    }

    fn input_schema(&self) -> Value {
        json_schema::<T::Input>()
    }

    fn output_schema(&self) -> Value {
        json_schema::<T::Output>()
    }

    fn validate(&self, input: &Value) -> Result<(), TaskError> {
        <T::Input as Deserialize>::deserialize(input)
            .map(drop)
            .map_err(TaskError::DecodeInput)
    }

    fn spawn_on(
        self: Arc<Self>,
        input: Value,
        ctx: TaskContext,
        set: &mut JoinSet<()>,
    ) -> AbortHandle {
        let key = T::KEY;
        let id = ctx.id();
        set.spawn(
            async move {
                let run_ctx = ctx.clone();
                // Catch a panic in the body so the task still settles: its durable
                // record is finished and anyone awaiting it (or shutdown) is woken.
                // Without this a panic would leave the phase Running forever.
                let outcome = AssertUnwindSafe(async {
                    let input: T::Input =
                        serde_json::from_value(input).map_err(TaskError::DecodeInput)?;
                    let output = TaskDefinition::run(&*self, input, run_ctx).await?;
                    serde_json::to_value(output).map_err(TaskError::EncodeOutput)
                })
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(TaskError::Run(RunError::Failed(anyhow::format_err!(
                        "task panicked"
                    ))))
                });
                ctx.complete(outcome).await;
            }
            .instrument(tracing::info_span!("task", key, id = id.get())),
        )
    }
}
