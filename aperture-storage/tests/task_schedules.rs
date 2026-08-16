use aperture_storage::{Interval, ListQuery, NewTaskSchedule, Storage, TaskId, TaskSchedulePatch};
use jiff::Timestamp;
use serde_json::json;

fn ts(micros: i64) -> Timestamp {
    Timestamp::from_microsecond(micros).unwrap()
}

fn interval(micros: i64) -> Interval {
    Interval::from_micros(micros).unwrap()
}

fn new_schedule(key: &str, interval_micros: i64, next_run_at: i64) -> NewTaskSchedule {
    NewTaskSchedule {
        key: key.to_owned(),
        input: json!({}),
        interval: interval(interval_micros),
        next_run_at: ts(next_run_at),
        created_at: ts(0),
    }
}

#[tokio::test]
async fn create_get_list_update_delete() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.task_schedules().unwrap();

    let id = repo
        .create(&new_schedule("rotate-certificate", 86_400_000_000, 1_000))
        .await
        .unwrap();
    let fetched = repo.get(id).await.unwrap().unwrap();
    assert_eq!(fetched.key, "rotate-certificate");
    assert_eq!(fetched.interval, interval(86_400_000_000));
    assert_eq!(fetched.next_run_at, ts(1_000));
    assert!(fetched.enabled);

    let page = repo.list(&ListQuery::default()).await.unwrap();
    assert_eq!(page.items.len(), 1);

    let updated = repo
        .update(
            id,
            &TaskSchedulePatch {
                enabled: Some(false),
                interval: Some(interval(60_000_000)),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(!updated.enabled);
    assert_eq!(updated.interval, interval(60_000_000));

    assert!(repo.delete(id).await.unwrap());
    assert!(repo.get(id).await.unwrap().is_none());
    assert!(!repo.delete(id).await.unwrap());
}

#[tokio::test]
async fn update_with_empty_patch_returns_row_unchanged() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.task_schedules().unwrap();
    let id = repo
        .create(&new_schedule("a", 60_000_000, 1_000))
        .await
        .unwrap();

    let updated = repo
        .update(id, &TaskSchedulePatch::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.id, id);
    assert_eq!(updated.interval, interval(60_000_000));
    assert!(updated.enabled);
}

#[tokio::test]
async fn update_only_sets_provided_fields() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.task_schedules().unwrap();
    let id = repo
        .create(&new_schedule("a", 60_000_000, 1_000))
        .await
        .unwrap();

    let updated = repo
        .update(
            id,
            &TaskSchedulePatch {
                enabled: Some(false),
                interval: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(!updated.enabled);
    assert_eq!(updated.interval, interval(60_000_000));
}

#[tokio::test]
async fn list_due_returns_only_enabled_past_due() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.task_schedules().unwrap();
    // Due, enabled.
    repo.create(&new_schedule("a", 1_000, 500)).await.unwrap();
    // Not yet due.
    repo.create(&new_schedule("b", 1_000, 5_000)).await.unwrap();
    // Due but disabled.
    let disabled_id = repo.create(&new_schedule("c", 1_000, 500)).await.unwrap();
    repo.update(
        disabled_id,
        &TaskSchedulePatch {
            enabled: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let due = repo.list_due(ts(1_000), 16).await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].key, "a");
}

#[tokio::test]
async fn mark_run_advances_next_run_at() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.task_schedules().unwrap();
    let id = repo
        .create(&new_schedule("a", 60_000_000, 1_000))
        .await
        .unwrap();
    repo.mark_run(id, ts(2_000), &interval(60_000_000), Some(TaskId::from(42)))
        .await
        .unwrap();
    let fetched = repo.get(id).await.unwrap().unwrap();
    assert_eq!(fetched.last_run_at, Some(ts(2_000)));
    assert_eq!(fetched.last_task_id, Some(TaskId::from(42)));
    assert_eq!(fetched.next_run_at, ts(60_002_000));
}

#[tokio::test]
async fn mark_run_stores_null_last_task_id_on_failure() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.task_schedules().unwrap();
    let id = repo
        .create(&new_schedule("a", 60_000_000, 1_000))
        .await
        .unwrap();
    repo.mark_run(id, ts(2_000), &interval(60_000_000), None)
        .await
        .unwrap();
    let fetched = repo.get(id).await.unwrap().unwrap();
    assert_eq!(fetched.last_task_id, None);
    assert_eq!(fetched.next_run_at, ts(60_002_000));
}
