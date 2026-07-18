//! Periodic task scheduler driver.
//!
//! A scheduler owns a long-running driver that periodically queries the
//! task schedule catalog for due rows and spawns each via [`Tasks::create`].
//! The catalog itself (creating, listing, patching, deleting schedules) is
//! managed through the [`TaskScheduleRepository`] in [`aperture-storage`];
//! the scheduler is a pure runtime driver over that catalog.
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

use aperture_storage::TaskScheduleRepository;
use jiff::Timestamp;
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

    /// Runs one scheduler tick: queries due schedules and spawns each. Returns
    /// the number of tasks spawned. Exposed so callers (e.g. `serve`) can run
    /// a tick at boot to catch up after downtime.
    pub async fn tick(&self) -> Result<usize, SchedulerError> {
        let now = Timestamp::now();
        let repo = &self.inner.schedules;
        let due = repo.list_due(now, TICK_BATCH).await?;
        let mut spawned = 0;
        for schedule in due {
            let input = schedule.input;
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
    use aperture_storage::{
        Interval, ListQuery, NewTaskSchedule, Storage, TaskSchedulePatch,
    };
    use serde_json::{Value, json};

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

    /// Bundles the scheduler with the storage it drives so tests can seed the
    /// schedule catalog directly through the repository.
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
    ) -> aperture_storage::DbId {
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

        // First tick sees the due schedule, fails to spawn, but advances it.
        assert_eq!(scheduler.tick().await.unwrap(), 0);
        let advanced = storage.task_schedules().unwrap().get(id).await.unwrap().unwrap();
        assert!(advanced.last_run_at.is_some());
        // A failed spawn leaves last_task_id NULL.
        assert!(advanced.last_task_id.is_none());

        // Second tick (immediately) should not re-fire: next_run_at is in the
        // future now.
        assert_eq!(scheduler.tick().await.unwrap(), 0);
    }
}
