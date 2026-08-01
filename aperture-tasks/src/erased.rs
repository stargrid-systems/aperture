//! Type-erased view of a [`TaskDefinition`].
//!
//! The registry stores definitions behind this trait so it can hold many kinds
//! together. Erasure happens only here: JSON is decoded into the kind's typed
//! input on the way in, and the typed output is encoded back out. The body
//! never sees a [`Value`]. A blanket impl bridges every [`TaskDefinition`].

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures_util::FutureExt;
use serde::Deserialize;
use serde_json::Value;
use tokio::task::{AbortHandle, JoinSet};
use tracing::Instrument;
use utoipa::openapi::{RefOr, Schema};
use utoipa::{PartialSchema, ToSchema};

use crate::context::TaskContext;
use crate::definition::{Capabilities, TaskDefinition};
use crate::error::{RunError, TaskError};

pub trait ErasedDefinition: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn input_name(&self) -> String;
    fn output_name(&self) -> String;
    fn input_schema(&self) -> RefOr<Schema>;
    fn output_schema(&self) -> RefOr<Schema>;
    /// Pushes the named component schemas this kind references (its input and
    /// output types plus their dependencies) into `out`.
    fn collect_schemas(&self, out: &mut Vec<(String, RefOr<Schema>)>);
    /// Checks that `input` decodes into the kind's input type.
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

impl<T: TaskDefinition> ErasedDefinition for T {
    fn kind(&self) -> &'static str {
        T::KIND
    }

    fn capabilities(&self) -> Capabilities {
        TaskDefinition::capabilities(self)
    }

    fn input_name(&self) -> String {
        <T::Input as ToSchema>::name().into_owned()
    }

    fn output_name(&self) -> String {
        <T::Output as ToSchema>::name().into_owned()
    }

    fn input_schema(&self) -> RefOr<Schema> {
        <T::Input as PartialSchema>::schema()
    }

    fn output_schema(&self) -> RefOr<Schema> {
        <T::Output as PartialSchema>::schema()
    }

    fn collect_schemas(&self, out: &mut Vec<(String, RefOr<Schema>)>) {
        out.push((
            <T::Input as ToSchema>::name().into_owned(),
            <T::Input as PartialSchema>::schema(),
        ));
        out.push((
            <T::Output as ToSchema>::name().into_owned(),
            <T::Output as PartialSchema>::schema(),
        ));
        <T::Input as ToSchema>::schemas(out);
        <T::Output as ToSchema>::schemas(out);
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
        let kind = T::KIND;
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
            .instrument(tracing::info_span!("task", kind, id = id.get())),
        )
    }
}
