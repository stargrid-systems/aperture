use aperture_storage::{ActorId, EventFilter, ListQuery, NewEvent, Order, Storage};
use jiff::Timestamp;
use serde_json::json;

fn at(micros: i64) -> Timestamp {
    Timestamp::from_microsecond(micros).unwrap()
}

async fn seed(repo: &aperture_storage::EventRepository) {
    for (key, hostname, micros) in [
        ("artifact.written", "aperture", 1_000),
        ("os.hostname_applied", "aperture", 2_000),
        ("os.hostname_applied", "gateway", 3_000),
    ] {
        repo.create(&NewEvent {
            key: key.to_owned(),
            data: json!({ "hostname": hostname }),
            actor: ActorId::SYSTEM,
            timestamp: at(micros),
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn create_then_get_round_trips_event() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.events().unwrap();

    let id = repo
        .create(&NewEvent {
            key: "os.hostname_applied".to_owned(),
            data: json!({ "hostname": "aperture" }),
            actor: ActorId::SYSTEM,
            timestamp: at(1_000),
        })
        .await
        .unwrap();

    let event = repo.get(id).await.unwrap().unwrap();
    assert_eq!(event.id, id);
    assert_eq!(event.key, "os.hostname_applied");
    assert_eq!(event.data, json!({ "hostname": "aperture" }));
    assert_eq!(event.actor, ActorId::SYSTEM);
    assert_eq!(event.timestamp, at(1_000));
}

#[tokio::test]
async fn get_returns_none_for_missing_event() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.events().unwrap();

    assert!(repo.get(1.into()).await.unwrap().is_none());
}

#[tokio::test]
async fn list_orders_by_timestamp_desc() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.events().unwrap();
    seed(&repo).await;

    let page = repo
        .list(&EventFilter::default(), &ListQuery::default())
        .await
        .unwrap();
    let timestamps: Vec<i64> = page
        .items
        .iter()
        .map(|event| event.timestamp.as_microsecond())
        .collect();
    assert_eq!(timestamps, [3_000, 2_000, 1_000]);
}

#[tokio::test]
async fn list_filters_by_key_prefix() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.events().unwrap();
    seed(&repo).await;

    let filter = EventFilter {
        key_prefix: Some("os.".to_owned()),
        ..Default::default()
    };
    let page = repo
        .list(
            &filter,
            &ListQuery {
                order: Some(Order::Asc),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let keys: Vec<&str> = page.items.iter().map(|event| event.key.as_str()).collect();
    assert_eq!(keys, ["os.hostname_applied", "os.hostname_applied"]);
    assert_eq!(page.items[0].data, json!({ "hostname": "aperture" }));
    assert_eq!(page.items[1].data, json!({ "hostname": "gateway" }));
}
