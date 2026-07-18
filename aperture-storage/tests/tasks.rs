use aperture_storage::{
    DbId, JsonField, JsonFilter, JsonPath, ListQuery, ParentFilter, StatusFilter, Storage,
    TaskStatus,
};
use jiff::Timestamp;
use serde_json::json;

fn at(micros: i64) -> Timestamp {
    Timestamp::from_microsecond(micros).unwrap()
}

#[tokio::test]
async fn create_then_finish_records_lifecycle() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.tasks().unwrap();

    let id = repo
        .create("download", None, &json!({"key": "spectra"}), at(1_000))
        .await
        .unwrap();

    let task = repo.get(id).await.unwrap().unwrap();
    assert_eq!(task.kind, "download");
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.parent_id, None);
    assert_eq!(task.input, json!({"key": "spectra"}));
    assert!(task.started_at.is_none());

    repo.mark_running(id, at(1_100)).await.unwrap();
    repo.finish(
        id,
        TaskStatus::Succeeded,
        at(2_000),
        Some(&json!({"size": 42})),
        None,
    )
    .await
    .unwrap();

    let task = repo.get(id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Succeeded);
    assert_eq!(task.started_at, Some(at(1_100)));
    assert_eq!(task.finished_at, Some(at(2_000)));
    assert_eq!(task.output, Some(json!({"size": 42})));
}

#[tokio::test]
async fn create_running_starts_in_running_state() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.tasks().unwrap();

    let id = repo
        .create_running("download", None, &json!({"key": "spectra"}), at(1_000))
        .await
        .unwrap();

    let task = repo.get(id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Running);
    assert_eq!(task.started_at, Some(at(1_000)));
    assert_eq!(task.finished_at, None);
}

#[tokio::test]
async fn finish_does_not_overwrite_a_finished_row() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.tasks().unwrap();

    let id = repo
        .create_running("download", None, &json!({}), at(1_000))
        .await
        .unwrap();
    repo.finish(
        id,
        TaskStatus::Succeeded,
        at(1_100),
        Some(&json!({"ok": true})),
        None,
    )
    .await
    .unwrap();

    // A late interrupt (shutdown racing a task that just succeeded) must not
    // clobber the terminal row.
    repo.finish(
        id,
        TaskStatus::Interrupted,
        at(1_200),
        None,
        Some("interrupted"),
    )
    .await
    .unwrap();

    let task = repo.get(id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Succeeded);
    assert_eq!(task.output, Some(json!({"ok": true})));
    assert_eq!(task.finished_at, Some(at(1_100)));
    assert!(task.error.is_none());
}

#[tokio::test]
async fn list_filters_by_status_kind_and_parent() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.tasks().unwrap();

    let parent = repo
        .create("update", None, &json!({}), at(1_000))
        .await
        .unwrap();
    let download = repo
        .create("download", Some(parent), &json!({}), at(1_100))
        .await
        .unwrap();
    let install = repo
        .create("install", Some(parent), &json!({}), at(1_200))
        .await
        .unwrap();
    repo.finish(install, TaskStatus::Failed, at(1_300), None, Some("boom"))
        .await
        .unwrap();

    let active = repo
        .list(
            Some(StatusFilter::Active),
            None,
            None,
            &[],
            &ListQuery::default(),
        )
        .await
        .unwrap();
    let active_ids: Vec<DbId> = active.items.iter().map(|task| task.id).collect();
    assert_eq!(active_ids, vec![download, parent]);

    let finished = repo
        .list(
            Some(StatusFilter::Finished),
            None,
            None,
            &[],
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(finished.items.len(), 1);
    assert_eq!(finished.items[0].id, install);

    let downloads = repo
        .list(None, Some("download"), None, &[], &ListQuery::default())
        .await
        .unwrap();
    assert_eq!(downloads.items.len(), 1);
    assert_eq!(downloads.items[0].id, download);

    let roots = repo
        .list(
            None,
            None,
            Some(ParentFilter::Root),
            &[],
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(roots.items.len(), 1);
    assert_eq!(roots.items[0].id, parent);

    let children = repo.children(parent).await.unwrap();
    let child_ids: Vec<DbId> = children.iter().map(|task| task.id).collect();
    assert_eq!(child_ids, vec![download, install]);
}

#[tokio::test]
async fn list_filters_by_json_input_and_output() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.tasks().unwrap();

    let spectra = repo
        .create_running(
            "download",
            None,
            &json!({"key": "spectra", "source": {"reference": "ghcr.io/x/spectra:1"}}),
            at(1_000),
        )
        .await
        .unwrap();
    repo.finish(
        spectra,
        TaskStatus::Succeeded,
        at(1_050),
        Some(&json!({"version": "1.0"})),
        None,
    )
    .await
    .unwrap();
    let other = repo
        .create_running("download", None, &json!({"key": "other"}), at(1_100))
        .await
        .unwrap();
    repo.finish(
        other,
        TaskStatus::Succeeded,
        at(1_150),
        Some(&json!({"version": "2.0"})),
        None,
    )
    .await
    .unwrap();

    // Filter by a top-level input field: the download history for one key.
    let by_key = repo
        .list(
            None,
            Some("download"),
            None,
            &[JsonFilter {
                field: JsonField::Input,
                path: JsonPath::new("key").unwrap(),
                value: "spectra",
            }],
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        by_key.items.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![spectra]
    );

    // Filter by a nested input path.
    let by_reference = repo
        .list(
            None,
            None,
            None,
            &[JsonFilter {
                field: JsonField::Input,
                path: JsonPath::new("source.reference").unwrap(),
                value: "ghcr.io/x/spectra:1",
            }],
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(by_reference.items.len(), 1);
    assert_eq!(by_reference.items[0].id, spectra);

    // Filter by an output field.
    let by_version = repo
        .list(
            None,
            None,
            None,
            &[JsonFilter {
                field: JsonField::Output,
                path: JsonPath::new("version").unwrap(),
                value: "2.0",
            }],
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(by_version.items.len(), 1);
    assert_eq!(by_version.items[0].id, other);

    // A non-matching value returns nothing.
    let none = repo
        .list(
            None,
            None,
            None,
            &[JsonFilter {
                field: JsonField::Input,
                path: JsonPath::new("key").unwrap(),
                value: "missing",
            }],
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert!(none.items.is_empty());
}

#[tokio::test]
async fn list_active_finds_unfinished_invocations() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.tasks().unwrap();

    let pending = repo
        .create("download", None, &json!({}), at(1_000))
        .await
        .unwrap();
    let running = repo
        .create("download", None, &json!({}), at(1_100))
        .await
        .unwrap();
    repo.mark_running(running, at(1_150)).await.unwrap();
    let done = repo
        .create("download", None, &json!({}), at(1_200))
        .await
        .unwrap();
    repo.finish(done, TaskStatus::Succeeded, at(1_300), None, None)
        .await
        .unwrap();

    let active = repo.list_active().await.unwrap();
    let ids: Vec<DbId> = active.iter().map(|task| task.id).collect();
    assert_eq!(ids, vec![pending, running]);
}
