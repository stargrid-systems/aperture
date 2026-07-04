//! Task system for the Aperture gateway.
//!
//! A task is a tracked unit of work. Each kind of task is a [`TaskDefinition`]
//! with a typed input and output, registered once in a [`TaskRegistry`].
//! [`Tasks`] spawns tasks, records every invocation in storage, tracks the
//! running ones, and hands back a typed [`TaskHandle`].
//!
//! Cancellation is cooperative: a body observes [`TaskContext::check_cancelled`]
//! and unwinds. Capabilities on a definition declare whether a kind can be
//! cancelled at all and whether it is safe to interrupt across a restart.

pub use aperture_storage::{
    InvalidJsonPath, JsonField, JsonFilter, JsonPath, ListQuery, Order, Page, ParentFilter,
    StatusFilter, TaskInvocation, TaskStatus,
};

pub use self::context::TaskContext;
pub use self::definition::{Capabilities, TaskDefinition};
pub use self::error::{RunError, TaskError};
pub use self::progress::{Progress, ProgressHandle, ProgressMessage};
pub use self::registry::{TaskDescriptor, TaskRegistry};
pub use self::tasks::{ActiveTask, TaskHandle, Tasks};

mod context;
mod definition;
mod erased;
mod error;
mod progress;
mod registry;
mod tasks;
