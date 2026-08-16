use aperture_storage::{ActorId, EventFilter, EventId, ListQuery, NewEvent, Storage};
use jiff::Timestamp;
use serde_json::json;

fn at(micros: i64) -> Timestamp {
    Timestamp::from_microsecond(micros).unwrap()
}

fn new_event(key: &str, n: u8, timestamp: Timestamp) -> NewEvent {
    NewEvent {
        id: EventId::generate(),
        key: key.to_owned(),
        data: json!({ "n": n }),
        actor: ActorId::SYSTEM,
        timestamp,
    }
}

#[tokio::test]
async fn create_assigns_row_and_get_roundtrips() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.events().unwrap();

    let event = new_event("artifact.written", 1, at(1_000));
    repo.create(&event).await.unwrap();

    let fetched = repo.get(event.id).await.unwrap().expect("row exists");
    assert_eq!(fetched.id, event.id);
    assert_eq!(fetched.key, "artifact.written");
    assert_eq!(fetched.data, json!({ "n": 1 }));
    assert_eq!(fetched.timestamp, event.timestamp);

    assert!(repo.get(EventId::generate()).await.unwrap().is_none());
}

#[tokio::test]
async fn batch_inserts_all_or_nothing() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.events().unwrap();

    let events: Vec<NewEvent> = (1..=3)
        .map(|n| new_event("artifact.removed", n, at(i64::from(n) * 1_000)))
        .collect();
    let ids: Vec<EventId> = events.iter().map(|event| event.id).collect();

    let mut batch = repo.batch().await.unwrap();
    for event in &events {
        batch.insert(event).await.unwrap();
    }
    batch.commit().await.unwrap();

    for id in ids {
        assert!(repo.get(id).await.unwrap().is_some(), "{id} persisted");
    }
}

#[tokio::test]
async fn batch_rolls_back_on_error() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.events().unwrap();

    let event = new_event("artifact.written", 1, at(1_000));
    let mut batch = repo.batch().await.unwrap();
    batch.insert(&event).await.unwrap();
    // Duplicate id violates the primary key.
    batch.insert(&event).await.unwrap_err();
    drop(batch);

    assert!(repo.get(event.id).await.unwrap().is_none());
}

#[tokio::test]
async fn list_paginates_by_timestamp_and_id() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.events().unwrap();

    for n in 1..=5u8 {
        let event = new_event("setting.changed", n, at(i64::from(n) * 1_000));
        repo.create(&event).await.unwrap();
    }
    // Same timestamp: the uuid tiebreak must still order pages deterministically.
    let tie = new_event("setting.changed", 6, at(5_000));
    repo.create(&tie).await.unwrap();

    let page = repo
        .list(
            &EventFilter::default(),
            &ListQuery {
                limit: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 2);
    // Two rows share the newest timestamp; the uuid tiebreak picks one.
    let n0 = page.items[0].data["n"].as_u64().unwrap();
    assert!(
        n0 == 5 || n0 == 6,
        "first page starts at the newest row: {n0}"
    );
    assert!(page.next_cursor.is_some());
    assert!(page.prev_cursor.is_none());

    let mut cursor = page.next_cursor;
    let mut seen = page.items.len();
    while let Some(encoded) = cursor {
        let page = repo
            .list(
                &EventFilter::default(),
                &ListQuery {
                    limit: Some(2),
                    cursor: Some(encoded),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        seen += page.items.len();
        cursor = page.next_cursor;
    }
    assert_eq!(seen, 6, "pagination covers every row exactly once");

    let filtered = repo
        .list(
            &EventFilter {
                key_prefix: Some("artifact".to_owned()),
                ..Default::default()
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert!(filtered.items.is_empty());
}
