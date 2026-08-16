//! Automation engine: drives task execution from periodic schedules and events.

use std::error::Error as StdError;
use std::time::Duration;

use aperture_events::EventBus;
use aperture_runtime::{Stop, Worker};
use aperture_storage::{ActorId, Event, TaskScheduleRepository};
use serde_json::Value;
use tokio::time::{MissedTickBehavior, interval};

use crate::scheduler::Scheduler;
use crate::tasks::Tasks;

const TICK_INTERVAL: Duration = Duration::from_secs(60);

struct EventRule {
    key: &'static str,
    task_kind: &'static str,
    make_input: Box<dyn Fn(&Value) -> Value + Send + Sync>,
}

/// The automation engine: drives task execution from interval schedules and
/// event subscriptions.
///
/// Owns the task runtime lifecycle: reconciles interrupted tasks on startup
/// and drains running tasks on shutdown. Between those, it ticks the periodic
/// [`Scheduler`] and reacts to events by spawning tasks based on registered
/// rules.
pub struct Automation {
    tasks: Tasks,
    scheduler: Scheduler,
    events: aperture_events::EventStream,
    rules: Vec<EventRule>,
}

impl Automation {
    /// Creates a new automation engine.
    ///
    /// Takes ownership of the task runtime lifecycle: call this once at
    /// startup and spawn the result as a [`Worker`].
    pub fn new(tasks: Tasks, schedule_repo: TaskScheduleRepository, event_bus: &EventBus) -> Self {
        let scheduler = Scheduler::new(schedule_repo, tasks.clone());
        Self {
            tasks,
            scheduler,
            events: event_bus.subscribe_all(),
            rules: Vec::new(),
        }
    }

    /// Registers a rule: when an event with `key` is received, spawn a task
    /// of `task_kind` with input produced by `make_input`.
    pub fn on_event(
        &mut self,
        key: &'static str,
        task_kind: &'static str,
        make_input: impl Fn(&Value) -> Value + Send + Sync + 'static,
    ) {
        self.rules.push(EventRule {
            key,
            task_kind,
            make_input: Box::new(make_input),
        });
    }
}

impl Worker for Automation {
    async fn run(mut self, stop: Stop) {
        if let Err(err) = self.tasks.reconcile().await {
            tracing::error!(error = &err as &dyn StdError, "task reconciliation failed");
        }

        let mut ticker = interval(TICK_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                biased;
                () = stop.cancelled() => break,
                _ = ticker.tick() => {
                    if let Err(err) = self.scheduler.tick().await {
                        tracing::error!(error = &err as &dyn StdError, "scheduler tick failed");
                    }
                }
                event = self.events.recv() => {
                    match event {
                        Some(event) => self.handle_event(&event).await,
                        None => break,
                    }
                }
            }
        }

        self.tasks.shutdown().await;
    }
}

impl Automation {
    async fn handle_event(&self, event: &Event) {
        for rule in &self.rules {
            if event.key.as_str() == rule.key {
                let input = (rule.make_input)(&event.data);
                if let Err(err) = self
                    .tasks
                    .create(rule.task_kind, input, ActorId::SYSTEM)
                    .await
                {
                    tracing::warn!(
                        error = &err as &dyn StdError,
                        task_kind = rule.task_kind,
                        event_key = %event.key,
                        "automation rule failed to spawn task"
                    );
                }
            }
        }
    }
}
