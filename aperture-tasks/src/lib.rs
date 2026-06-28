//! Task system for the Aperture gateway.
//!
//! A task is a tracked unit of work. Each kind of task is a [`TaskDefinition`]
//! with a typed input and output, registered once in a [`TaskRegistry`]. The
//! [`TaskManager`] spawns tasks, records every invocation in storage, tracks the
//! running ones, and hands back a typed [`TaskHandle`].
//!
//! Cancellation is cooperative: a body observes [`TaskContext::check_cancelled`]
//! and unwinds. Capabilities on a definition declare whether a kind can be
//! cancelled at all and whether it is safe to interrupt across a restart.

pub use self::context::TaskContext;
pub use self::definition::{Capabilities, TaskDefinition};
pub use self::error::TaskError;
pub use self::manager::{ActiveTask, TaskHandle, TaskManager};
pub use self::progress::{Progress, ProgressHandle};
pub use self::registry::{TaskDescriptor, TaskRegistry};

mod context;
mod definition;
mod erased;
mod error;
mod manager;
mod progress;
mod registry;
