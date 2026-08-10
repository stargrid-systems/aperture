use aperture_storage::{ActorId, ApiKeyHash, ApiKeyId, ListQuery, Order, Storage};
use jiff::Timestamp;

fn at(micros: i64) -> Timestamp {
    Timestamp::from_microsecond(micros).unwrap()
}

fn dummy_hash(n: u8) -> ApiKeyHash {
    ApiKeyHash::new(vec![n; 32])
}

async fn seed_key(
    repo: &aperture_storage::ApiKeyRepository,
    actor: ActorId,
    n: u8,
    created_at: Timestamp,
) -> ApiKeyId {
    repo.create(
        actor,
        &format!("key-{n}"),
        &dummy_hash(n),
        &format!("prefix{n}"),
        created_at,
    )
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn list_for_actor_paginates_by_id_desc() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.api_keys().unwrap();
    let actor = ActorId::SYSTEM;

    for i in 1..=5 {
        seed_key(&repo, actor, i, at(i64::from(i) * 1000)).await;
    }

    let page = repo
        .list_for_actor(
            actor,
            &ListQuery {
                limit: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].name, "key-5");
    assert_eq!(page.items[1].name, "key-4");
    assert!(page.next_cursor.is_some());
    assert!(page.prev_cursor.is_none());

    let page2 = repo
        .list_for_actor(
            actor,
            &ListQuery {
                limit: Some(2),
                cursor: page.next_cursor,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(page2.items.len(), 2);
    assert_eq!(page2.items[0].name, "key-3");
    assert_eq!(page2.items[1].name, "key-2");
    assert!(page2.next_cursor.is_some());

    let page3 = repo
        .list_for_actor(
            actor,
            &ListQuery {
                limit: Some(2),
                cursor: page2.next_cursor,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(page3.items.len(), 1);
    assert_eq!(page3.items[0].name, "key-1");
    assert!(page3.next_cursor.is_none());
}

#[tokio::test]
async fn list_for_actor_paginates_ascending() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.api_keys().unwrap();
    let actor = ActorId::SYSTEM;

    for i in 1..=3 {
        seed_key(&repo, actor, i, at(i64::from(i) * 1000)).await;
    }

    let page = repo
        .list_for_actor(
            actor,
            &ListQuery {
                limit: Some(2),
                order: Some(Order::Asc),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].name, "key-1");
    assert_eq!(page.items[1].name, "key-2");
}

#[tokio::test]
async fn list_for_actor_is_scoped_to_actor() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.api_keys().unwrap();
    let actor_a = ActorId::SYSTEM;
    let actor_b = ActorId::from(2);

    seed_key(&repo, actor_a, 1, at(1000)).await;
    seed_key(&repo, actor_b, 2, at(2000)).await;

    let page = repo
        .list_for_actor(actor_a, &ListQuery::default())
        .await
        .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].name, "key-1");
}

#[tokio::test]
async fn list_for_actor_empty() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.api_keys().unwrap();

    let page = repo
        .list_for_actor(ActorId::SYSTEM, &ListQuery::default())
        .await
        .unwrap();

    assert!(page.items.is_empty());
    assert!(page.next_cursor.is_none());
}
