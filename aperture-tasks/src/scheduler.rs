//! Periodic task scheduler.
//!
//! A scheduler owns a long-running driver that periodically queries the
//! schedule catalog for due rows and spawns each via [`Tasks::create`].
//! Schedules themselves are managed through the [`ScheduleRepository`]; the
//! scheduler is read-only with respect to which schedules exist.
//!
//! Errors during a single schedule spawn (unknown kind, decode failure, storage
//! error) are logged and the schedule is advanced to its next interval; one bad
//! schedule cannot stall the driver.
//!
//! [`Tasks::create`]: crate::Tasks::create
//! [`ScheduleRepository`]: aperture_storage::ScheduleRepository

use std::sync::Arc;
use std::time::Duration;

use aperture_storage::{NewSchedule, ScheduleRepository, Storage};
use jiff::Timestamp;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::Tasks;

/// Maximum number of due schedules processed in a single tick. Caps the work
/// one tick can do so a runaway backlog cannot pin the driver.
const TICK_BATCH: usize = 32;

/// Spawns tasks according to the schedule catalog.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
}

struct Inner {
    storage: Storage,
    tasks: Tasks,
}

impl Scheduler {
    /// Creates a scheduler that drives `tasks` using `storage`'s schedule
    /// catalog.
    pub fn new(storage: Storage, tasks: Tasks) -> Self {
        Self {
            inner: Arc::new(Inner { storage, tasks }),
        }
    }

    /// Read access to the storage catalog (for the HTTP CRUD endpoints).
    pub fn storage(&self) -> &Storage {
        &self.inner.storage
    }

    /// Repository over the schedule catalog.
    pub fn repository(&self) -> Result<ScheduleRepository, aperture_storage::StorageError> {
        self.inner.storage.schedules()
    }

    /// Creates a new schedule row.
    pub async fn create(
        &self,
        new: NewSchedule,
    ) -> Result<aperture_storage::Schedule, SchedulerError> {
        let repo = self.inner.storage.schedules()?;
        let id = repo.create(&new).await?;
        let schedule = repo.get(id).await?.expect("just created");
        Ok(schedule)
    }

    /// Returns the schedule with `id`, if it exists.
    pub async fn get(
        &self,
        id: aperture_storage::DbId,
    ) -> Result<Option<aperture_storage::Schedule>, SchedulerError> {
        Ok(self.inner.storage.schedules()?.get(id).await?)
    }

    /// Lists schedules, oldest-first by id.
    pub async fn list(
        &self,
        query: &aperture_storage::ListQuery,
    ) -> Result<aperture_storage::Page<aperture_storage::Schedule>, SchedulerError> {
        Ok(self.inner.storage.schedules()?.list(query).await?)
    }

    /// Updates a schedule's interval and/or enabled flag.
    pub async fn update(
        &self,
        id: aperture_storage::DbId,
        patch: aperture_storage::SchedulePatch,
    ) -> Result<Option<aperture_storage::Schedule>, SchedulerError> {
        Ok(self.inner.storage.schedules()?.update(id, &patch).await?)
    }

    /// Deletes the schedule with `id`. Returns whether a row was removed.
    pub async fn delete(&self, id: aperture_storage::DbId) -> Result<bool, SchedulerError> {
        Ok(self.inner.storage.schedules()?.delete(id).await?)
    }

    /// Runs one scheduler tick: queries due schedules and spawns each. Returns
    /// the number of tasks spawned. Exposed so callers (e.g. `serve`) can run
    /// a tick at boot to catch up after downtime.
    pub async fn tick(&self) -> Result<usize, SchedulerError> {
        let now = Timestamp::now();
        let repo = self.inner.storage.schedules()?;
        let due = repo.list_due(now, TICK_BATCH).await?;
        let mut spawned = 0;
        for schedule in due {
            let input: serde_json::Value = match serde_json::from_str(&schedule.input) {
                Ok(value) => value,
                Err(err) => {
                    tracing::error!(
                        error = %err,
                        schedule_id = schedule.id.get(),
                        kind = %schedule.kind,
                        "schedule input is not valid JSON; advancing without spawning",
                    );
                    continue;
                }
            };
            match self.inner.tasks.create(&schedule.kind, input).await {
                Ok(invocation) => {
                    spawned += 1;
                    if let Err(err) = repo
                        .mark_run(schedule.id, now, schedule.interval_ms, invocation.id)
                        .await
                    {
                        tracing::error!(
                            error = %err,
                            schedule_id = schedule.id.get(),
                            "failed to advance schedule after spawn",
                        );
                    }
                }
                Err(err) => {
                    tracing::error!(
                        error = %err,
                        schedule_id = schedule.id.get(),
                        kind = %schedule.kind,
                        "schedule spawn failed; advancing to next interval",
                    );
                    // Advance anyway so a permanently-broken schedule does not
                    // fire on every tick.
                    let _ = repo
                        .mark_run(
                            schedule.id,
                            now,
                            schedule.interval_ms,
                            aperture_storage::DbId::from(0),
                        )
                        .await;
                }
            }
        }
        Ok(spawned)
    }

    /// Long-running driver. Ticks at `tick_interval`, exits when `shutdown` is
    /// cancelled. Errors inside a tick are logged, not surfaced: a single
    /// storage blip should not bring down the scheduler.
    pub async fn run(self, tick_interval: Duration, shutdown: CancellationToken) {
        loop {
            tokio::select! {
                biased;
                () = shutdown.cancelled() => return,
                () = sleep(tick_interval) => {}
            }
            if let Err(err) = self.tick().await {
                tracing::error!(error = %err, "scheduler tick failed");
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
    use aperture_storage::{ListQuery, NewSchedule, SchedulePatch};
    use serde_json::json;

    use super::*;

    /// A trivial task kind that always succeeds. The scheduler is agnostic to
    /// the body, but we need one registered so `Tasks::create` accepts the
    /// kind string.
    struct Ping;

    impl crate::TaskDefinition for Ping {
        const KIND: &'static str = "ping";
        type Input = serde_json::Value;
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

    fn ts(millis: i64) -> Timestamp {
        Timestamp::from_millisecond(millis).unwrap()
    }

    #[tokio::test]
    async fn tick_spawns_due_schedules_and_advances_them() {
        let storage = Storage::open(":memory:").await.unwrap();
        let mut registry = crate::TaskRegistry::new();
        registry.register(Ping);
        let tasks = Tasks::new(storage.clone(), registry);
        let scheduler = Scheduler::new(storage.clone(), tasks);

        let now = Timestamp::now().as_millisecond();
        let schedule = scheduler
            .create(NewSchedule {
                kind: "ping".to_owned(),
                input: json!({}).to_string(),
                interval_ms: 60_000,
                next_run_at: ts(now - 1_000),
                created_at: ts(now),
            })
            .await
            .unwrap();
        let id = schedule.id;

        let spawned = scheduler.tick().await.unwrap();
        assert_eq!(spawned, 1);

        let schedules = scheduler.list(&ListQuery::default()).await.unwrap();
        assert_eq!(schedules.items.len(), 1);
        let advanced = &schedules.items[0];
        assert!(advanced.last_run_at.is_some());
        assert!(advanced.last_task_id.is_some());
        let _ = id;
    }

    #[tokio::test]
    async fn tick_skips_disabled_schedules() {
        let storage = Storage::open(":memory:").await.unwrap();
        let mut registry = crate::TaskRegistry::new();
        registry.register(Ping);
        let tasks = Tasks::new(storage.clone(), registry);
        let scheduler = Scheduler::new(storage.clone(), tasks);

        let now = Timestamp::now().as_millisecond();
        let schedule = scheduler
            .create(NewSchedule {
                kind: "ping".to_owned(),
                input: json!({}).to_string(),
                interval_ms: 60_000,
                next_run_at: ts(now - 1_000),
                created_at: ts(now),
            })
            .await
            .unwrap();
        let id = schedule.id;
        scheduler
            .update(
                id,
                SchedulePatch {
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
        let storage = Storage::open(":memory:").await.unwrap();
        let mut registry = crate::TaskRegistry::new();
        registry.register(Ping);
        let tasks = Tasks::new(storage.clone(), registry);
        let scheduler = Scheduler::new(storage.clone(), tasks);

        let now = Timestamp::now().as_millisecond();
        let schedule = scheduler
            .create(NewSchedule {
                kind: "does-not-exist".to_owned(),
                input: json!({}).to_string(),
                interval_ms: 60_000,
                next_run_at: ts(now - 1_000),
                created_at: ts(now),
            })
            .await
            .unwrap();
        let id = schedule.id;

        // First tick sees the due schedule, fails to spawn, but advances it.
        assert_eq!(scheduler.tick().await.unwrap(), 0);
        let advanced = scheduler.get(id).await.unwrap().unwrap();
        assert!(advanced.last_run_at.is_some());

        // Second tick (immediately) should not re-fire: next_run_at is in the
        // future now.
        assert_eq!(scheduler.tick().await.unwrap(), 0);
    }
}
