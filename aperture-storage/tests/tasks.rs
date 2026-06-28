use aperture_storage::{ListQuery, ParentFilter, StatusFilter, Storage, TaskStatus};
use jiff::Timestamp;

fn at(millis: i64) -> Timestamp {
    Timestamp::from_millisecond(millis).unwrap()
}

#[tokio::test]
async fn create_then_finish_records_lifecycle() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.tasks();

    let id = repo
        .create("download", None, r#"{"key":"spectra"}"#, at(1_000))
        .await
        .unwrap();

    let task = repo.get(id).await.unwrap().unwrap();
    assert_eq!(task.kind, "download");
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.parent_id, None);
    assert_eq!(task.input, r#"{"key":"spectra"}"#);
    assert!(task.started_at.is_none());

    repo.mark_running(id, at(1_100)).await.unwrap();
    repo.finish(id, TaskStatus::Succeeded, at(2_000), Some(r#"{"size":42}"#), None)
        .await
        .unwrap();

    let task = repo.get(id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Succeeded);
    assert_eq!(task.started_at, Some(at(1_100)));
    assert_eq!(task.finished_at, Some(at(2_000)));
    assert_eq!(task.output.as_deref(), Some(r#"{"size":42}"#));
}

#[tokio::test]
async fn list_filters_by_status_kind_and_parent() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.tasks();

    let parent = repo.create("update", None, "{}", at(1_000)).await.unwrap();
    let download = repo
        .create("download", Some(parent), "{}", at(1_100))
        .await
        .unwrap();
    let install = repo
        .create("install", Some(parent), "{}", at(1_200))
        .await
        .unwrap();
    repo.finish(install, TaskStatus::Failed, at(1_300), None, Some("boom"))
        .await
        .unwrap();

    let active = repo
        .list(Some(StatusFilter::Active), None, None, &ListQuery::default())
        .await
        .unwrap();
    let active_ids: Vec<i64> = active.items.iter().map(|task| task.id).collect();
    assert_eq!(active_ids, vec![download, parent]);

    let finished = repo
        .list(Some(StatusFilter::Finished), None, None, &ListQuery::default())
        .await
        .unwrap();
    assert_eq!(finished.items.len(), 1);
    assert_eq!(finished.items[0].id, install);

    let downloads = repo
        .list(None, Some("download"), None, &ListQuery::default())
        .await
        .unwrap();
    assert_eq!(downloads.items.len(), 1);
    assert_eq!(downloads.items[0].id, download);

    let roots = repo
        .list(None, None, Some(ParentFilter::Root), &ListQuery::default())
        .await
        .unwrap();
    assert_eq!(roots.items.len(), 1);
    assert_eq!(roots.items[0].id, parent);

    let children = repo.children(parent).await.unwrap();
    let child_ids: Vec<i64> = children.iter().map(|task| task.id).collect();
    assert_eq!(child_ids, vec![download, install]);
}

#[tokio::test]
async fn list_active_finds_unfinished_invocations() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.tasks();

    let pending = repo.create("download", None, "{}", at(1_000)).await.unwrap();
    let running = repo.create("download", None, "{}", at(1_100)).await.unwrap();
    repo.mark_running(running, at(1_150)).await.unwrap();
    let done = repo.create("download", None, "{}", at(1_200)).await.unwrap();
    repo.finish(done, TaskStatus::Succeeded, at(1_300), None, None)
        .await
        .unwrap();

    let active = repo.list_active().await.unwrap();
    let ids: Vec<i64> = active.iter().map(|task| task.id).collect();
    assert_eq!(ids, vec![pending, running]);
}
