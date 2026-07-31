//! Periodic task scheduler driver.

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use aperture_runtime::Stop;
use aperture_storage::{ActorId, TaskScheduleRepository};
use jiff::Timestamp;
use tokio::time::{MissedTickBehavior, interval};

use crate::Tasks;

const TICK_BATCH: usize = 32;

#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
}

struct Inner {
    schedules: TaskScheduleRepository,
    tasks: Tasks,
}

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error(transparent)]
    Storage(#[from] aperture_storage::StorageError),
}

impl Scheduler {
    pub fn new(schedules: TaskScheduleRepository, tasks: Tasks) -> Self {
        Self {
            inner: Arc::new(Inner { schedules, tasks }),
        }
    }

    /// Runs one tick. Kept public so tests can drive a single tick in
    /// isolation.
    pub async fn tick(&self) -> Result<usize, SchedulerError> {
        let now = Timestamp::now();
        let repo = &self.inner.schedules;
        let due = repo.list_due(now, TICK_BATCH).await?;
        let mut spawned = 0;
        for schedule in due {
            let input = schedule.input;
            match self
                .inner
                .tasks
                .create(&schedule.kind, input, ActorId::SYSTEM)
                .await
            {
                Ok(invocation) => {
                    spawned += 1;
                    if let Err(err) = repo
                        .mark_run(schedule.id, now, &schedule.interval, Some(invocation.id))
                        .await
                    {
                        tracing::error!(
                            error = &err as &dyn Error,
                            schedule_id = schedule.id.get(),
                            "failed to advance schedule after spawn",
                        );
                    }
                }
                Err(err) => {
                    tracing::error!(
                        error = &err as &dyn Error,
                        schedule_id = schedule.id.get(),
                        kind = %schedule.kind,
                        "schedule spawn failed, advancing to next interval",
                    );
                    // Advance anyway so a permanently-broken schedule does not
                    // fire on every tick.
                    if let Err(err) = repo
                        .mark_run(schedule.id, now, &schedule.interval, None)
                        .await
                    {
                        tracing::error!(
                            error = &err as &dyn Error,
                            schedule_id = schedule.id.get(),
                            "failed to advance schedule after spawn failure",
                        );
                    }
                }
            }
        }
        Ok(spawned)
    }

    /// Ticks at `tick_interval`. The first tick fires immediately so missed
    /// schedules are caught up without waiting a full interval. Exits when
    /// `stop` is cancelled. Errors inside a tick are logged.
    pub async fn run(self, tick_interval: Duration, stop: Stop) {
        let mut ticker = interval(tick_interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                () = stop.cancelled() => return,
                _ = ticker.tick() => {}
            }
            if let Err(err) = self.tick().await {
                tracing::error!(error = &err as &dyn Error, "scheduler tick failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use aperture_storage::{Interval, ListQuery, NewTaskSchedule, Storage, TaskSchedulePatch};
    use serde_json::{Value, json};

    use super::*;

    struct Ping;

    impl crate::TaskDefinition for Ping {
        const KIND: &'static str = "ping";
        type Input = Value;
        type Output = ();

        fn capabilities(&self) -> crate::Capabilities {
            crate::Capabilities {
                cancellable: true,
                resumable: true,
            }
        }

        async fn run(
            &self,
            _input: Self::Input,
            _ctx: crate::TaskContext,
        ) -> Result<Self::Output, crate::RunError> {
            Ok(())
        }
    }

    fn ts(micros: i64) -> Timestamp {
        Timestamp::from_microsecond(micros).unwrap()
    }

    fn interval(micros: i64) -> Interval {
        Interval::from_micros(micros).unwrap()
    }

    struct Harness {
        scheduler: Scheduler,
        storage: Storage,
    }

    async fn setup() -> Harness {
        let storage = Storage::open(":memory:").await.unwrap();
        let mut registry = crate::TaskRegistry::new();
        registry.register(Ping);
        let tasks = Tasks::new(storage.tasks().unwrap(), registry);
        let schedules = storage.task_schedules().unwrap();
        Harness {
            scheduler: Scheduler::new(schedules, tasks),
            storage,
        }
    }

    async fn create_schedule(
        storage: &Storage,
        kind: &str,
        interval_micros: i64,
        next_run_at: i64,
    ) -> aperture_storage::TaskScheduleId {
        let repo = storage.task_schedules().unwrap();
        repo.create(&NewTaskSchedule {
            kind: kind.to_owned(),
            input: json!({}),
            interval: interval(interval_micros),
            next_run_at: ts(next_run_at),
            created_at: ts(Timestamp::now().as_microsecond()),
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn tick_spawns_due_schedules_and_advances_them() {
        let Harness { scheduler, storage } = setup().await;
        let now = Timestamp::now().as_microsecond();
        create_schedule(&storage, "ping", 60_000_000, now - 1_000_000).await;

        let spawned = scheduler.tick().await.unwrap();
        assert_eq!(spawned, 1);

        let schedules = storage
            .task_schedules()
            .unwrap()
            .list(&ListQuery::default())
            .await
            .unwrap();
        assert_eq!(schedules.items.len(), 1);
        let advanced = &schedules.items[0];
        assert!(advanced.last_run_at.is_some());
        assert!(advanced.last_task_id.is_some());
    }

    #[tokio::test]
    async fn tick_skips_disabled_schedules() {
        let Harness { scheduler, storage } = setup().await;
        let now = Timestamp::now().as_microsecond();
        let id = create_schedule(&storage, "ping", 60_000_000, now - 1_000_000).await;
        storage
            .task_schedules()
            .unwrap()
            .update(
                id,
                &TaskSchedulePatch {
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(scheduler.tick().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn tick_advances_past_unknown_kind_so_it_doesnt_loop() {
        let Harness { scheduler, storage } = setup().await;
        let now = Timestamp::now().as_microsecond();
        let id = create_schedule(&storage, "does-not-exist", 60_000_000, now - 1_000_000).await;

        assert_eq!(scheduler.tick().await.unwrap(), 0);
        let advanced = storage
            .task_schedules()
            .unwrap()
            .get(id)
            .await
            .unwrap()
            .unwrap();
        assert!(advanced.last_run_at.is_some());
        assert!(advanced.last_task_id.is_none());

        assert_eq!(scheduler.tick().await.unwrap(), 0);
    }
}
