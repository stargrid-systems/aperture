use aperture_storage::{EventFilter, Level, ListQuery, SpanFilter, Storage};
use jiff::Timestamp;

fn at(millis: i64) -> Timestamp {
    Timestamp::from_millisecond(millis).unwrap()
}

async fn seeded_storage() -> Storage {
    let storage = Storage::open(":memory:").await.unwrap();
    let logs = storage.logs();

    let span_id = logs
        .insert_span("download", Level::Info, "aperture_artifacts::fetch", at(1_000))
        .parent_id(None)
        .file(Some("src/fetch.rs"))
        .line(Some(42))
        .fields(Some(r#"{"key":"spectra"}"#))
        .execute()
        .await
        .unwrap();

    logs.insert_event(Level::Info, "aperture_artifacts::fetch", at(1_100))
        .span_id(Some(span_id))
        .message(Some("starting download"))
        .file(Some("src/fetch.rs"))
        .line(Some(10))
        .fields(Some(r#"{"key":"spectra","source":"ghcr.io"}"#))
        .execute()
        .await
        .unwrap();

    logs.insert_event(Level::Warn, "aperture_artifacts::fetch", at(1_200))
        .span_id(Some(span_id))
        .message(Some("retrying download after timeout"))
        .file(Some("src/fetch.rs"))
        .line(Some(25))
        .fields(Some(r#"{"key":"spectra","attempt":2}"#))
        .execute()
        .await
        .unwrap();

    logs.insert_event(Level::Error, "aperture_http::error", at(1_300))
        .message(Some("artifact request failed"))
        .fields(Some(r#"{"status":500}"#))
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
                target: None,
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
    assert_eq!(page.items[0].message.as_deref(), Some("artifact request failed"));
    assert_eq!(page.items[0].level, Level::Error);
    assert_eq!(page.items[1].message.as_deref(), Some("retrying download after timeout"));
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
                target: None,
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
async fn filter_by_target_prefix() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: Some("aperture_artifacts".to_owned()),
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
    assert!(page.items.iter().all(|e| e.target == "aperture_artifacts::fetch"));
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
                target: None,
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
                target: None,
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
                target: None,
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
    assert_eq!(page.items[0].message.as_deref(), Some("retrying download after timeout"));
}

#[tokio::test]
async fn filter_by_structured_fields() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: None,
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
    assert!(page.items.iter().all(|e| e.target == "aperture_artifacts::fetch"));
}

#[tokio::test]
async fn fts_message_search() {
    let storage = seeded_storage().await;
    let logs = storage.logs();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: None,
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
    assert!(page.items.iter().all(|e| e.message.as_deref().unwrap().contains("download")));
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
        .list_spans(
            &SpanFilter {
                min_level: None,
                target: None,
                since: None,
                until: None,
            },
            &ListQuery::default(),
        )
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
    assert_eq!(events[1].message.as_deref(), Some("retrying download after timeout"));
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
                target: None,
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

    logs.record_dropped(42, at(1_000)).await.unwrap();

    let page = logs
        .list_events(
            &EventFilter {
                min_level: None,
                target: None,
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
    assert!(page.items[0].message.as_deref().unwrap().contains("42"));
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
                target: None,
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
                target: None,
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
