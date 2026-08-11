//! Task system for the Aperture gateway.
//!
//! A task is a tracked unit of work. Each kind of task is a [`TaskDefinition`]
//! with a typed input and output, registered once in a [`TaskRegistry`].
//! [`Tasks`] spawns tasks, records every invocation in storage, tracks the
//! running ones, and hands back a typed [`TaskHandle`].
//!
//! Cancellation is cooperative: a body observes
//! [`TaskContext::check_cancelled`] and unwinds. Capabilities on a definition
//! declare whether a kind can be cancelled at all and whether it is safe to
//! interrupt across a restart.
//!
//! [`Scheduler`] drives registered kinds on a periodic schedule.

pub use aperture_storage::{
    Interval, InvalidJsonPath, JsonField, JsonFilter, JsonPath, ListQuery, NewTaskSchedule, Order,
    Page, ParentFilter, StatusFilter, TaskInvocation, TaskSchedulePatch, TaskStatus,
};

pub use self::automation::Automation;
pub use self::context::TaskContext;
pub use self::definition::{Capabilities, TaskDefinition};
pub use self::error::{RunError, TaskError};
pub use self::progress::{Progress, ProgressHandle, ProgressMessage};
pub use self::registry::{TaskDescriptor, TaskRegistry};
pub use self::scheduler::{Scheduler, SchedulerError};
pub use self::tasks::{ActiveTask, TaskHandle, Tasks};

mod automation;
mod context;
mod definition;
mod erased;
mod error;
mod progress;
mod registry;
mod scheduler;
mod tasks;
