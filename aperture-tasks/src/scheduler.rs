//! Periodic task scheduler.
//!
//! A scheduler owns a long-running driver that periodically queries the
//! task schedule catalog for due rows and spawns each via [`Tasks::create`].
//! Schedules themselves are managed through the [`TaskScheduleRepository`];
//! the scheduler is read-only with respect to which schedules exist.
//!
//! Errors during a single schedule spawn (unknown kind, storage error) are
//! logged and the schedule is advanced to its next interval; one bad schedule
//! cannot stall the driver.
//!
//! [`Tasks::create`]: crate::Tasks::create
//! [`TaskScheduleRepository`]: aperture_storage::TaskScheduleRepository

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use aperture_storage::{NewTaskSchedule, TaskScheduleRepository};
use jiff::Timestamp;
use serde_json::Value;
use tokio::time::{MissedTickBehavior, interval_at, Instant};
use tokio_util::sync::CancellationToken;

use crate::Tasks;

/// Maximum number of due schedules processed in a single tick. Caps the work
/// one tick can do so a runaway backlog cannot pin the driver.
const TICK_BATCH: usize = 32;

/// Spawns tasks according to the task schedule catalog.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
}

struct Inner {
    schedules: TaskScheduleRepository,
    tasks: Tasks,
}

impl Scheduler {
    /// Creates a scheduler that drives `tasks` using `schedules` as the
    /// catalog of due work.
    pub fn new(schedules: TaskScheduleRepository, tasks: Tasks) -> Self {
        Self {
            inner: Arc::new(Inner { schedules, tasks }),
        }
    }

    /// Creates a new task schedule row.
    pub async fn create(
        &self,
        new: NewTaskSchedule,
    ) -> Result<aperture_storage::TaskSchedule, SchedulerError> {
        let repo = &self.inner.schedules;
        let id = repo.create(&new).await?;
        let schedule = repo.get(id).await?.expect("just created");
        Ok(schedule)
    }

    /// Returns the task schedule with `id`, if it exists.
    pub async fn get(
        &self,
        id: aperture_storage::DbId,
    ) -> Result<Option<aperture_storage::TaskSchedule>, SchedulerError> {
        Ok(self.inner.schedules.get(id).await?)
    }

    /// Lists task schedules, oldest-first by id.
    pub async fn list(
        &self,
        query: &aperture_storage::ListQuery,
    ) -> Result<aperture_storage::Page<aperture_storage::TaskSchedule>, SchedulerError> {
        Ok(self.inner.schedules.list(query).await?)
    }

    /// Updates a task schedule's interval and/or enabled flag.
    pub async fn update(
        &self,
        id: aperture_storage::DbId,
        patch: aperture_storage::TaskSchedulePatch,
    ) -> Result<Option<aperture_storage::TaskSchedule>, SchedulerError> {
        Ok(self.inner.schedules.update(id, &patch).await?)
    }

    /// Deletes the task schedule with `id`. Returns whether a row was removed.
    pub async fn delete(&self, id: aperture_storage::DbId) -> Result<bool, SchedulerError> {
        Ok(self.inner.schedules.delete(id).await?)
    }

    /// Runs one scheduler tick: queries due schedules and spawns each. Returns
    /// the number of tasks spawned. Exposed so callers (e.g. `serve`) can run
    /// a tick at boot to catch up after downtime.
    pub async fn tick(&self) -> Result<usize, SchedulerError> {
        let now = Timestamp::now();
        let repo = &self.inner.schedules;
        let due = repo.list_due(now, TICK_BATCH).await?;
        let mut spawned = 0;
        for schedule in due {
            let input = Value::Object(schedule.input);
            match self.inner.tasks.create(&schedule.kind, input).await {
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
                        "schedule spawn failed; advancing to next interval",
                    );
                    // Advance anyway so a permanently-broken schedule does not
                    // fire on every tick. No task was spawned, so leave
                    // last_task_id NULL.
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

    /// Long-running driver. Ticks at `tick_interval`, exits when `shutdown` is
    /// cancelled. Errors inside a tick are logged, not surfaced: a single
    /// storage blip should not bring down the scheduler.
    pub async fn run(self, tick_interval: Duration, shutdown: CancellationToken) {
        // First fire one full period out so a boot tick (run by the caller
        // before `run`) doesn't get a follow-up immediately. Delay policy:
        // when a tick overruns, the next fire is rescheduled from the
        // overrun instant. This keeps the schedule stable under normal
        // load and only drifts when a tick actually exceeds the period.
        let mut ticker = interval_at(Instant::now() + tick_interval, tick_interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                () = shutdown.cancelled() => return,
                _ = ticker.tick() => {}
            }
            if let Err(err) = self.tick().await {
                tracing::error!(error = &err as &dyn Error, "scheduler tick failed");
            }
        }
    }
}

/// Errors returned by the scheduler.
#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    /// A storage-layer error.
    #[error(transparent)]
    Storage(#[from] aperture_storage::StorageError),
}

#[cfg(test)]
mod tests {
    use aperture_storage::{Interval, ListQuery, NewTaskSchedule, TaskSchedulePatch};
    use serde_json::Map;

    use super::*;

    /// A trivial task kind that always succeeds. The scheduler is agnostic to
    /// the body, but we need one registered so `Tasks::create` accepts the
    /// kind string.
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

    async fn setup() -> Scheduler {
        let storage = aperture_storage::Storage::open(":memory:").await.unwrap();
        let mut registry = crate::TaskRegistry::new();
        registry.register(Ping);
        let tasks = Tasks::new(storage.tasks().unwrap(), registry);
        let schedules = storage.task_schedules().unwrap();
        Scheduler::new(schedules, tasks)
    }

    #[tokio::test]
    async fn tick_spawns_due_schedules_and_advances_them() {
        let scheduler = setup().await;
        let now = Timestamp::now().as_microsecond();
        let schedule = scheduler
            .create(NewTaskSchedule {
                kind: "ping".to_owned(),
                input: Map::new(),
                interval: interval(60_000_000),
                next_run_at: ts(now - 1_000_000),
                created_at: ts(now),
            })
            .await
            .unwrap();

        let spawned = scheduler.tick().await.unwrap();
        assert_eq!(spawned, 1);

        let schedules = scheduler.list(&ListQuery::default()).await.unwrap();
        assert_eq!(schedules.items.len(), 1);
        let advanced = &schedules.items[0];
        assert!(advanced.last_run_at.is_some());
        assert!(advanced.last_task_id.is_some());
        let _ = &schedule;
    }

    #[tokio::test]
    async fn tick_skips_disabled_schedules() {
        let scheduler = setup().await;
        let now = Timestamp::now().as_microsecond();
        let schedule = scheduler
            .create(NewTaskSchedule {
                kind: "ping".to_owned(),
                input: Map::new(),
                interval: interval(60_000_000),
                next_run_at: ts(now - 1_000_000),
                created_at: ts(now),
            })
            .await
            .unwrap();
        let id = schedule.id;
        scheduler
            .update(
                id,
                TaskSchedulePatch {
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
        let scheduler = setup().await;
        let now = Timestamp::now().as_microsecond();
        let schedule = scheduler
            .create(NewTaskSchedule {
                kind: "does-not-exist".to_owned(),
                input: Map::new(),
                interval: interval(60_000_000),
                next_run_at: ts(now - 1_000_000),
                created_at: ts(now),
            })
            .await
            .unwrap();
        let id = schedule.id;

        // First tick sees the due schedule, fails to spawn, but advances it.
        assert_eq!(scheduler.tick().await.unwrap(), 0);
        let advanced = scheduler.get(id).await.unwrap().unwrap();
        assert!(advanced.last_run_at.is_some());
        // A failed spawn leaves last_task_id NULL.
        assert!(advanced.last_task_id.is_none());

        // Second tick (immediately) should not re-fire: next_run_at is in the
        // future now.
        assert_eq!(scheduler.tick().await.unwrap(), 0);
    }
}
