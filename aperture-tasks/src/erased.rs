//! Type-erased view of a [`TaskDefinition`].
//!
//! The registry stores definitions behind this trait so it can hold many kinds
//! together. Erasure happens only here: JSON is decoded into the kind's typed
//! input on the way in, and the typed output is encoded back out. The body never
//! sees a [`Value`]. A blanket impl bridges every [`TaskDefinition`].

use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use tokio::task::{AbortHandle, JoinSet};
use utoipa::PartialSchema;
use utoipa::ToSchema;
use utoipa::openapi::{RefOr, Schema};

use crate::context::TaskContext;
use crate::definition::{Capabilities, TaskDefinition};
use crate::error::TaskError;

pub(crate) trait ErasedDefinition: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
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
        set.spawn(async move {
            let run_ctx = ctx.clone();
            let outcome = async {
                let input: T::Input = serde_json::from_value(input).map_err(TaskError::DecodeInput)?;
                let output = TaskDefinition::run(&*self, input, run_ctx).await?;
                serde_json::to_value(output).map_err(TaskError::EncodeOutput)
            }
            .await;
            ctx.complete(outcome).await;
        })
    }
}
