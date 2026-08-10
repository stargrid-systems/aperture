use aperture_storage::{ActorId, SettingRecord, Storage};
use jiff::Timestamp;
use serde_json::json;

fn at(micros: i64) -> Timestamp {
    Timestamp::from_microsecond(micros).unwrap()
}

#[tokio::test]
async fn get_returns_none_for_missing_key() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.settings().unwrap();

    assert!(repo.get("system").await.unwrap().is_none());
}

#[tokio::test]
async fn put_then_get_round_trips_value() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.settings().unwrap();

    repo.put(
        "system",
        &json!({"hostname": "aperture"}),
        ActorId::SYSTEM,
        at(1_000),
    )
    .await
    .unwrap();

    let record = repo.get("system").await.unwrap().unwrap();
    assert_eq!(record.key, "system");
    assert_eq!(record.value, json!({"hostname": "aperture"}));
    assert_eq!(record.updated_at, at(1_000));
    assert_eq!(record.updated_by, ActorId::SYSTEM);
}

#[tokio::test]
async fn put_replaces_existing_value() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.settings().unwrap();

    repo.put(
        "system",
        &json!({"hostname": "old"}),
        ActorId::SYSTEM,
        at(1_000),
    )
    .await
    .unwrap();
    repo.put(
        "system",
        &json!({"hostname": "new"}),
        ActorId::from(42),
        at(2_000),
    )
    .await
    .unwrap();

    let record = repo.get("system").await.unwrap().unwrap();
    assert_eq!(record.value, json!({"hostname": "new"}));
    assert_eq!(record.updated_at, at(2_000));
    assert_eq!(record.updated_by, ActorId::from(42));
}

#[tokio::test]
async fn list_returns_all_keys_ordered() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.settings().unwrap();

    repo.put("zeta", &json!({"v": 3}), ActorId::SYSTEM, at(1_000))
        .await
        .unwrap();
    repo.put("alpha", &json!({"v": 1}), ActorId::SYSTEM, at(2_000))
        .await
        .unwrap();
    repo.put("mid", &json!({"v": 2}), ActorId::SYSTEM, at(3_000))
        .await
        .unwrap();

    let records: Vec<String> = repo
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.key)
        .collect();
    assert_eq!(records, vec!["alpha", "mid", "zeta"]);
}

#[tokio::test]
async fn list_is_empty_when_nothing_stored() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.settings().unwrap();

    let records: Vec<SettingRecord> = repo.list().await.unwrap();
    assert!(records.is_empty());
}
