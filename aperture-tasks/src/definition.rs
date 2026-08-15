//! The task definition trait: a typed, registered key of work.

use std::future::Future;

use serde::Serialize;
use serde::de::DeserializeOwned;
use utoipa::ToSchema;

use crate::context::TaskContext;
use crate::error::RunError;

/// What a task key supports beyond running to completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// The task can be asked to stop before it finishes. Cancellation is always
    /// cooperative: the body observes [`TaskContext::check_cancelled`] and
    /// unwinds.
    pub cancellable: bool,
    /// The task is safe to interrupt across a process stop, because it can be
    /// resumed or simply re-run later. An unresumable task must be allowed to
    /// finish before the gateway shuts down.
    pub resumable: bool,
}

impl Capabilities {
    /// A task that can neither be cancelled nor safely interrupted.
    pub const NONE: Self = Self {
        cancellable: false,
        resumable: false,
    };
}

/// A key of task. Each definition fixes a unique [`TaskDefinition::KEY`], a
/// typed input and output, its capabilities, and the work to run.
///
/// The input and output are real types. They are validated and (de)serialized
/// at the boundary, so the body in [`TaskDefinition::run`] only ever sees typed
/// values.
pub trait TaskDefinition: Send + Sync + 'static {
    /// The unique key string this definition is registered under.
    const KEY: &'static str;
    /// The typed input the task is created with.
    type Input: DeserializeOwned + Serialize + ToSchema + Send;
    /// The typed output the task produces on success.
    type Output: DeserializeOwned + Serialize + ToSchema + Send;

    /// What this key supports.
    fn capabilities(&self) -> Capabilities;

    /// Runs the task. The returned future must be `Send` so it can be spawned.
    fn run(
        &self,
        input: Self::Input,
        ctx: TaskContext,
    ) -> impl Future<Output = Result<Self::Output, RunError>> + Send;
}
