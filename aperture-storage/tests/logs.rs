use aperture_storage::{EventFilter, Level, ListQuery, SpanFilter, SpanParentFilter, Storage};
use jiff::Timestamp;

fn at(millis: i64) -> Timestamp {
    Timestamp::from_millisecond(millis).unwrap()
}

fn json_map(s: &str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(s)
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

async fn seeded_storage() -> Storage {
    let storage = Storage::open(":memory:").await.unwrap();
    let logs = storage.logs();

    let span_fields = json_map(r#"{"key":"spectra"}"#);
    let span_id = logs
        .insert_span(
            "download",
            Level::Info,
            "aperture_artifacts::fetch",
            at(1_000),
        )
        .parent_id(None)
        .file(Some("src/fetch.rs"))
        .line(Some(42))
        .fields(Some(&span_fields))
        .execute()
        .await
        .unwrap();

    let event_fields = json_map(r#"{"key":"spectra","source":"ghcr.io"}"#);
    logs.insert_event(Level::Info, "aperture_artifacts::fetch", at(1_100))
        .span_id(Some(span_id))
        .message(Some("starting download"))
        .file(Some("src/fetch.rs"))
        .line(Some(10))
        .fields(Some(&event_fields))
        .execute()
        .await
        .unwrap();

    let retry_fields = json_map(r#"{"key":"spectra","attempt":2}"#);
    logs.insert_event(Level::Warn, "aperture_artifacts::fetch", at(1_200))
        .span_id(Some(span_id))
        .message(Some("retrying download after timeout"))
        .file(Some("src/fetch.rs"))
        .line(Some(25))
        .fields(Some(&retry_fields))
        .execute()
        .await
        .unwrap();

    let error_fields = json_map(r#"{"status":500}"#);
    logs.insert_event(Level::Error, "aperture_http::error", at(1_300))
        .message(Some("artifact request failed"))
        .fields(Some(&error_fields))
        .execute()
        .await
        .unwrap();

    logs.close_span(span_id, at(1_400)).await.unwrap();

    storage
}

#[tokio::test]
async fn list_events_newest_first() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: None,
                span_id: None,
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 3);
    assert_eq!(
        page.items[0].message.as_deref(),
        Some("artifact request failed")
    );
    assert_eq!(page.items[0].level, Level::Error);
    assert_eq!(
        page.items[1].message.as_deref(),
        Some("retrying download after timeout")
    );
    assert_eq!(page.items[2].message.as_deref(), Some("starting download"));
}

#[tokio::test]
async fn filter_by_min_level() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: Some(Level::Warn),
                target: Vec::new(),
                query: None,
                span_id: None,
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].level, Level::Error);
    assert_eq!(page.items[1].level, Level::Warn);
}

#[tokio::test]
async fn filter_by_target() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: vec!["aperture_artifacts::fetch".to_owned()],
                query: None,
                span_id: None,
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert!(
        page.items
            .iter()
            .all(|e| e.target == "aperture_artifacts::fetch")
    );
}

#[tokio::test]
async fn filter_by_span_id() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    // First get all events to find the span_id
    let all = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: None,
                span_id: None,
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();
    let span_id = all.items[1].span_id.unwrap();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: None,
                span_id: Some(span_id),
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert!(page.items.iter().all(|e| e.span_id == Some(span_id)));
}

#[tokio::test]
async fn filter_by_time_range() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: None,
                span_id: None,
                since: Some(at(1_150)),
                until: Some(at(1_250)),
                fields: Vec::new(),
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0].message.as_deref(),
        Some("retrying download after timeout")
    );
}

#[tokio::test]
async fn filter_by_structured_fields() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: None,
                span_id: None,
                since: None,
                until: None,
                fields: vec![("key".to_owned(), "spectra".to_owned())],
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert!(
        page.items
            .iter()
            .all(|e| e.target == "aperture_artifacts::fetch")
    );
}

#[tokio::test]
async fn query_matches_message() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: Some("download".to_owned()),
                span_id: None,
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 2);
    assert!(
        page.items
            .iter()
            .all(|e| e.message.as_deref().unwrap().contains("download"))
    );
}

#[tokio::test]
async fn query_matches_target() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: Some("aperture_http".to_owned()),
                span_id: None,
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].target, "aperture_http::error");
}

#[tokio::test]
async fn list_targets() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let all = logs.list_targets(None).await.unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.contains(&"aperture_artifacts::fetch".to_owned()));
    assert!(all.contains(&"aperture_http::error".to_owned()));

    let filtered = logs.list_targets(Some("aperture_http")).await.unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0], "aperture_http::error");
}

#[tokio::test]
async fn list_and_get_spans() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_spans(&SpanFilter::default(), &ListQuery::default())
        .await
        .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].name, "download");
    assert_eq!(page.items[0].ended_at, Some(at(1_400)));

    let span_id = page.items[0].id;
    let span = logs.get_span(span_id).await.unwrap().unwrap();
    assert_eq!(span.name, "download");

    let events = logs.events_for_span(span_id).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].message.as_deref(), Some("starting download"));
    assert_eq!(
        events[1].message.as_deref(),
        Some("retrying download after timeout")
    );
}

#[tokio::test]
async fn prune_before_deletes_old_events() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let deleted = logs.prune_before(at(1_250)).await.unwrap();
    assert_eq!(deleted, 2);

    let remaining = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: None,
                span_id: None,
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(remaining.items.len(), 1);
}

#[tokio::test]
async fn record_dropped_inserts_synthetic_event() {
    let storage = Storage::open(":memory:").await.unwrap();
    let logs = storage.logs();

    let mut batch = logs.batch().await.unwrap();
    batch
        .record_dropped(42, at(1_000), "00000000-0000-0000-0000-000000000001")
        .await
        .unwrap();
    batch.commit().await.unwrap();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: None,
                span_id: None,
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].level, Level::Warn);
    assert_eq!(page.items[0].target, "aperture::log");
    assert_eq!(
        page.items[0].message.as_deref().unwrap(),
        "dropped log records due to full buffer"
    );
    assert!(page.items[0].fields.as_deref().unwrap().contains("42"));
}

#[tokio::test]
async fn paginate_events() {
    let storage = Storage::open(":memory:").await.unwrap();
    let logs = storage.logs();

    for i in 0..5 {
        logs.insert_event(Level::Info, "test", at(i * 100))
            .message(Some(&format!("event {i}")))
            .execute()
            .await
            .unwrap();
    }

    let first = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: None,
                span_id: None,
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery {
                limit: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(first.items.len(), 2);
    assert_eq!(first.items[0].message.as_deref(), Some("event 4"));
    assert!(first.next_cursor.is_some());
    assert!(first.prev_cursor.is_none());

    let second = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Vec::new(),
                query: None,
                span_id: None,
                since: None,
                until: None,
                fields: Vec::new(),
            },
            &ListQuery {
                limit: Some(2),
                cursor: first.next_cursor,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(second.items.len(), 2);
    assert_eq!(second.items[0].message.as_deref(), Some("event 2"));
    assert!(second.prev_cursor.is_some());
}

#[tokio::test]
async fn nested_spans_preserve_parent_child() {
    let storage = Storage::open(":memory:").await.unwrap();
    let logs = storage.logs();

    let parent_id = logs
        .insert_span("parent", Level::Info, "aperture::test", at(1_000))
        .execute()
        .await
        .unwrap();

    let child_id = logs
        .insert_span("child", Level::Debug, "aperture::test", at(1_100))
        .parent_id(Some(parent_id))
        .execute()
        .await
        .unwrap();

    let grandchild_id = logs
        .insert_span("grandchild", Level::Trace, "aperture::test", at(1_200))
        .parent_id(Some(child_id))
        .execute()
        .await
        .unwrap();

    assert_eq!(grandchild_id, child_id + 1);

    let child = logs.get_span(child_id).await.unwrap().unwrap();
    assert_eq!(child.parent_id, Some(parent_id));

    let grandchild = logs.get_span(grandchild_id).await.unwrap().unwrap();
    assert_eq!(grandchild.parent_id, Some(child_id));

    let roots = logs
        .list_spans(
            &SpanFilter {
                parent: SpanParentFilter::RootOnly,
                ..Default::default()
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(roots.items.len(), 1);
    assert_eq!(roots.items[0].name, "parent");

    let children = logs
        .list_spans(
            &SpanFilter {
                parent: SpanParentFilter::ChildrenOf(parent_id),
                ..Default::default()
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(children.items.len(), 1);
    assert_eq!(children.items[0].name, "child");

    let grandchildren = logs
        .list_spans(
            &SpanFilter {
                parent: SpanParentFilter::ChildrenOf(child_id),
                ..Default::default()
            },
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(grandchildren.items.len(), 1);
    assert_eq!(grandchildren.items[0].name, "grandchild");
}

#[tokio::test]
async fn list_boots_groups_by_boot_id() {
    use aperture_storage::BootInfo;
    let a = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let b = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
    let storage = Storage::open(":memory:").await.unwrap();
    let logs = storage.logs();

    // Two distinct boot ids, with events interleaved in time but grouped apart.
    logs.insert_event(Level::Info, "aperture", at(1_000))
        .message(Some("first boot start"))
        .boot_id(Some("00000000-0000-0000-0000-000000000001"))
        .execute()
        .await
        .unwrap();
    logs.insert_event(Level::Info, "aperture", at(2_000))
        .message(Some("first boot end"))
        .boot_id(Some("00000000-0000-0000-0000-000000000001"))
        .execute()
        .await
        .unwrap();
    logs.insert_event(Level::Info, "aperture", at(3_000))
        .message(Some("second boot start"))
        .boot_id(Some("00000000-0000-0000-0000-000000000002"))
        .execute()
        .await
        .unwrap();

    let boots: Vec<BootInfo> = logs.list_boots().await.unwrap();

    // Newest first.
    assert_eq!(boots.len(), 2);
    assert_eq!(boots[0].boot_id, b);
    assert_eq!(boots[0].event_count, 1);
    assert_eq!(boots[0].first_seen, at(3_000));
    assert_eq!(boots[0].last_seen, at(3_000));
    assert_eq!(boots[1].boot_id, a);
    assert_eq!(boots[1].event_count, 2);
    assert_eq!(boots[1].first_seen, at(1_000));
    assert_eq!(boots[1].last_seen, at(2_000));
}
