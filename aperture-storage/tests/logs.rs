use aperture_storage::{
    EventFilter, EventRecord, Level, ListQuery, SpanFilter, SpanParentFilter, SpanRecord, Storage,
};
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
    let logs = storage.logs().unwrap();

    let span_fields = json_map(r#"{"key":"spectra"}"#);
    let event_fields = json_map(r#"{"key":"spectra","source":"ghcr.io"}"#);
    let retry_fields = json_map(r#"{"key":"spectra","attempt":2}"#);
    let error_fields = json_map(r#"{"status":500}"#);

    let mut batch = logs.batch().await.unwrap();
    batch
        .insert_span(SpanRecord {
            tracing_id: 1,
            parent_tracing_id: None,
            boot_id: uuid::Uuid::nil(),
            name: "download",
            level: Level::Info,
            target: "aperture_artifacts::fetch",
            file: Some("src/fetch.rs"),
            line: Some(42),
            started_at: at(1_000),
            fields: Some(&span_fields),
        })
        .await
        .unwrap();

    batch
        .insert_event(EventRecord {
            span_tracing_id: Some(1),
            level: Level::Info,
            target: "aperture_artifacts::fetch",
            message: Some("starting download"),
            timestamp: at(1_100),
            file: Some("src/fetch.rs"),
            line: Some(10),
            boot_id: Some(uuid::Uuid::nil()),
            fields: Some(&event_fields),
        })
        .await
        .unwrap();

    batch
        .insert_event(EventRecord {
            span_tracing_id: Some(1),
            level: Level::Warn,
            target: "aperture_artifacts::fetch",
            message: Some("retrying download after timeout"),
            timestamp: at(1_200),
            file: Some("src/fetch.rs"),
            line: Some(25),
            boot_id: Some(uuid::Uuid::nil()),
            fields: Some(&retry_fields),
        })
        .await
        .unwrap();

    batch
        .insert_event(EventRecord {
            span_tracing_id: None,
            level: Level::Error,
            target: "aperture_http::error",
            message: Some("artifact request failed"),
            timestamp: at(1_300),
            file: None,
            line: None,
            boot_id: Some(uuid::Uuid::nil()),
            fields: Some(&error_fields),
        })
        .await
        .unwrap();

    batch
        .close_span(1, uuid::Uuid::nil(), at(1_400))
        .await
        .unwrap();
    batch.commit().await.unwrap();

    storage
}

#[tokio::test]
async fn list_events_newest_first() {
    let storage = seeded_storage().await;
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

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
    let logs = storage.logs().unwrap();

    let mut batch = logs.batch().await.unwrap();
    batch
        .record_dropped(42, at(1_000), uuid::Uuid::nil())
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
    assert_eq!(
        page.items[0].fields.get("dropped"),
        Some(&serde_json::json!(42))
    );
}

#[tokio::test]
async fn paginate_events() {
    let storage = Storage::open(":memory:").await.unwrap();
    let logs = storage.logs().unwrap();

    let mut batch = logs.batch().await.unwrap();
    for i in 0..5 {
        batch
            .insert_event(EventRecord {
                span_tracing_id: None,
                level: Level::Info,
                target: "test",
                message: Some(&format!("event {i}")),
                timestamp: at(i * 100),
                file: None,
                line: None,
                boot_id: None,
                fields: None,
            })
            .await
            .unwrap();
    }
    batch.commit().await.unwrap();

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
    let logs = storage.logs().unwrap();

    let mut batch = logs.batch().await.unwrap();
    batch
        .insert_span(SpanRecord {
            tracing_id: 1,
            parent_tracing_id: None,
            boot_id: uuid::Uuid::nil(),
            name: "parent",
            level: Level::Info,
            target: "aperture::test",
            file: None,
            line: None,
            started_at: at(1_000),
            fields: None,
        })
        .await
        .unwrap();

    batch
        .insert_span(SpanRecord {
            tracing_id: 2,
            parent_tracing_id: Some(1),
            boot_id: uuid::Uuid::nil(),
            name: "child",
            level: Level::Debug,
            target: "aperture::test",
            file: None,
            line: None,
            started_at: at(1_100),
            fields: None,
        })
        .await
        .unwrap();

    batch
        .insert_span(SpanRecord {
            tracing_id: 3,
            parent_tracing_id: Some(2),
            boot_id: uuid::Uuid::nil(),
            name: "grandchild",
            level: Level::Trace,
            target: "aperture::test",
            file: None,
            line: None,
            started_at: at(1_200),
            fields: None,
        })
        .await
        .unwrap();
    batch.commit().await.unwrap();

    let page = logs
        .list_spans(&SpanFilter::default(), &ListQuery::default())
        .await
        .unwrap();
    assert_eq!(page.items.len(), 3);

    let parent = page.items.iter().find(|s| s.name == "parent").unwrap();
    let child = page.items.iter().find(|s| s.name == "child").unwrap();
    let grandchild = page.items.iter().find(|s| s.name == "grandchild").unwrap();

    assert_eq!(child.parent_id, Some(parent.id));
    assert_eq!(grandchild.parent_id, Some(child.id));

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
                parent: SpanParentFilter::ChildrenOf(parent.id),
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
                parent: SpanParentFilter::ChildrenOf(child.id),
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
    let logs = storage.logs().unwrap();

    let mut batch = logs.batch().await.unwrap();
    batch
        .insert_event(EventRecord {
            span_tracing_id: None,
            level: Level::Info,
            target: "aperture",
            message: Some("first boot start"),
            timestamp: at(1_000),
            file: None,
            line: None,
            boot_id: Some(a),
            fields: None,
        })
        .await
        .unwrap();
    batch
        .insert_event(EventRecord {
            span_tracing_id: None,
            level: Level::Info,
            target: "aperture",
            message: Some("first boot end"),
            timestamp: at(2_000),
            file: None,
            line: None,
            boot_id: Some(a),
            fields: None,
        })
        .await
        .unwrap();
    batch
        .insert_event(EventRecord {
            span_tracing_id: None,
            level: Level::Info,
            target: "aperture",
            message: Some("second boot start"),
            timestamp: at(3_000),
            file: None,
            line: None,
            boot_id: Some(b),
            fields: None,
        })
        .await
        .unwrap();
    batch.commit().await.unwrap();

    let boots: Vec<BootInfo> = logs.list_boots().await.unwrap();

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
