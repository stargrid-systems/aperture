//! Task system for the Aperture gateway.
//!
//! A task is a tracked unit of work. Each task is a [`TaskDefinition`]
//! with a typed input and output, registered once in a [`TaskRegistry`].
//! [`Tasks`] spawns tasks, records every invocation in storage, tracks the
//! running ones, and hands back a typed [`TaskHandle`].
//!
//! Cancellation is cooperative: a body observes
//! [`TaskContext::check_cancelled`] and unwinds. Capabilities on a definition
//! declare whether a key can be cancelled at all and whether it is safe to
//! interrupt across a restart.
//!
//! [`Scheduler`] drives registered definitions on a periodic schedule.

use aperture_runtime::Registry;
pub use aperture_storage::{
    Interval, InvalidJsonPath, JsonField, JsonFilter, JsonPath, ListQuery, NewTaskSchedule, Order,
    Page, ParentFilter, StatusFilter, TaskInvocation, TaskSchedulePatch, TaskStatus,
};

pub use self::context::TaskContext;
pub use self::definition::{Capabilities, TaskDefinition};
pub use self::erased::{ErasedTaskDefinition, TaskDescriptor};
pub use self::error::{RunError, TaskError};
pub use self::progress::{Progress, ProgressHandle, ProgressMessage};
pub use self::scheduler::{Scheduler, SchedulerError};
pub use self::tasks::{ActiveTask, TaskHandle, Tasks};

mod context;
mod definition;
mod erased;
mod error;
pub mod keys;
mod progress;
mod scheduler;
mod tasks;

/// The registry of task definitions, keyed by definition key.
pub type TaskRegistry = Registry<dyn ErasedTaskDefinition>;
