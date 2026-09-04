//! Automation engine: drives task execution from periodic schedules and events.

use std::error::Error as StdError;
use std::time::Duration;

use aperture_events::{Delivery, EventBus, EventEnvelope};
use aperture_runtime::{Stop, Worker};
use aperture_storage::{ActorId, TaskScheduleRepository};
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
                delivery = self.events.recv() => {
                    match delivery {
                        Some(Delivery::Event(event)) => self.handle_event(&event).await,
                        Some(Delivery::Lagged(dropped)) => {
                            tracing::warn!(
                                dropped,
                                "automation event stream lagged, events were dropped"
                            );
                        }
                        None => break,
                    }
                }
            }
        }

        self.tasks.shutdown().await;
    }
}

impl Automation {
    async fn handle_event(&self, event: &EventEnvelope) {
        for rule in &self.rules {
            if event.key() == rule.key {
                let data = match event.payload_json() {
                    Ok(data) => data,
                    Err(err) => {
                        tracing::warn!(
                            error = &err as &dyn StdError,
                            event_key = event.key(),
                            "failed to serialize event payload for automation rule"
                        );
                        continue;
                    }
                };
                let input = (rule.make_input)(&data);
                if let Err(err) = self
                    .tasks
                    .create(rule.task_kind, input, ActorId::SYSTEM)
                    .await
                {
                    tracing::warn!(
                        error = &err as &dyn StdError,
                        task_kind = rule.task_kind,
                        event_key = event.key(),
                        "automation rule failed to spawn task"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::{Future, ready};
    use std::sync::Arc;

    use aperture_events::{EventBus, EventDefinition};
    use aperture_storage::{ActorId, ListQuery, Storage};
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};
    use utoipa::ToSchema;

    use super::*;

    struct Ping;

    impl crate::TaskDefinition for Ping {
        const KEY: &'static str = "ping";

        type Input = Value;
        type Output = ();

        fn capabilities(&self) -> crate::Capabilities {
            crate::Capabilities::NONE
        }

        fn run(
            &self,
            _input: Self::Input,
            _ctx: crate::TaskContext,
        ) -> impl Future<Output = Result<Self::Output, crate::RunError>> + Send {
            ready(Ok(()))
        }
    }

    /// The payload the rule matches on.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
    struct SourceEvent {
        hostname: String,
    }

    impl EventDefinition for SourceEvent {
        const KEY: &'static str = "source.event";
    }

    /// A payload whose key matches no rule.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
    struct OtherEvent {
        hostname: String,
    }

    impl EventDefinition for OtherEvent {
        const KEY: &'static str = "other.event";
    }

    async fn automation_with_rule() -> (Automation, Storage, EventBus) {
        let storage = Storage::open(":memory:").await.unwrap();
        let mut registry = crate::TaskRegistry::new();
        registry.register(Arc::new(Ping));
        let tasks = Tasks::new(storage.tasks().unwrap(), registry);
        let schedules = storage.task_schedules().unwrap();
        let event_bus = EventBus::new();
        let mut automation = Automation::new(tasks, schedules, &event_bus);
        automation.on_event(
            SourceEvent::KEY,
            "ping",
            |data| json!({ "hostname": data["hostname"] }),
        );
        (automation, storage, event_bus)
    }

    async fn spawned_inputs(storage: &Storage) -> Vec<Value> {
        storage
            .tasks()
            .unwrap()
            .list(None, Some("ping"), None, &[], &ListQuery::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|task| task.input)
            .collect()
    }

    #[tokio::test]
    async fn matching_event_creates_task_with_mapped_input() {
        let (automation, storage, bus) = automation_with_rule().await;
        let envelope = bus
            .emit(
                SourceEvent {
                    hostname: "aperture".to_owned(),
                },
                ActorId::SYSTEM,
            )
            .await
            .unwrap();

        automation.handle_event(&envelope).await;

        let inputs = spawned_inputs(&storage).await;
        assert_eq!(inputs, [json!({ "hostname": "aperture" })]);
    }

    #[tokio::test]
    async fn non_matching_event_creates_no_task() {
        let (automation, storage, bus) = automation_with_rule().await;
        let envelope = bus
            .emit(
                OtherEvent {
                    hostname: "aperture".to_owned(),
                },
                ActorId::SYSTEM,
            )
            .await
            .unwrap();

        automation.handle_event(&envelope).await;

        let inputs = spawned_inputs(&storage).await;
        assert!(inputs.is_empty());
    }
}
