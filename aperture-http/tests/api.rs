use std::sync::Arc;
use std::{env, fs, process};

use aperture_artifacts::{Artifact, Artifacts, DownloadStatus, Storage};
use aperture_http::{AppState, Spectra, SpectraConfig, app};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use jiff::Timestamp;
use serde_json::Value;
use tower::ServiceExt;

fn at(millis: i64) -> Timestamp {
    Timestamp::from_millisecond(millis).unwrap()
}

fn version(key: &str, digest: &str, downloaded_at: i64) -> Artifact {
    Artifact {
        id: 0,
        key: key.to_owned(),
        source: "ghcr.io/stargrid-systems/spectra:0.2.0".to_owned(),
        digest: digest.to_owned(),
        media_type: None,
        version: Some("0.2.0".to_owned()),
        size_bytes: 1234,
        downloaded_at: at(downloaded_at),
        verified_at: None,
    }
}

async fn seeded_app() -> (Router, Artifacts) {
    let root = env::temp_dir().join(format!("aperture-api-{}", process::id()));
    let _ = fs::remove_dir_all(&root);
    let storage = Storage::open(":memory:").await.unwrap();
    let artifacts = Artifacts::new(storage, root);

    let repo = artifacts.storage().artifacts().unwrap();
    repo.record_version(&version("firmware", "sha256:fff", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:aaa", 2_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:bbb", 3_000))
        .await
        .unwrap();

    let started = at(1_700_000_000_000);
    let id = repo
        .start_download("spectra", "src", started)
        .await
        .unwrap();
    repo.finish_download(
        id,
        DownloadStatus::Succeeded,
        started,
        Some("sha256:bbb"),
        Some(1234),
        None,
    )
    .await
    .unwrap();

    let spectra = Spectra::new(Arc::new(artifacts.clone()), SpectraConfig::default());
    let state = AppState::new(
        "test",
        uuid::Uuid::parse_str("00000000-0000-0000-0000-0000000000aa").unwrap(),
        spectra,
    );
    (app(state), artifacts)
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    (status, json)
}

#[tokio::test]
async fn lists_artifacts_with_summary() {
    let (app, _artifacts) = seeded_app().await;

    let (status, json) = get_json(&app, "/api/v1/artifacts").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    // Ascending by key.
    assert_eq!(items[0]["key"], "firmware");
    assert_eq!(items[1]["key"], "spectra");
    assert_eq!(items[1]["version_count"], 2);
    assert_eq!(items[1]["digest"], "sha256:bbb");
    assert!(json["next_cursor"].is_null());
}

#[tokio::test]
async fn paginates_artifacts_with_cursor() {
    let (app, _artifacts) = seeded_app().await;

    let (status, first) = get_json(&app, "/api/v1/artifacts?limit=1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    assert_eq!(first["items"][0]["key"], "firmware");
    assert!(first["prev_cursor"].is_null());
    let cursor = first["next_cursor"].as_str().unwrap();

    let (_, second) = get_json(&app, &format!("/api/v1/artifacts?limit=1&cursor={cursor}")).await;
    assert_eq!(second["items"][0]["key"], "spectra");
    assert!(second["next_cursor"].is_null());

    // Page back to the first item using the prev cursor.
    let back_cursor = second["prev_cursor"].as_str().expect("a previous page");
    let (_, back) = get_json(
        &app,
        &format!("/api/v1/artifacts?limit=1&cursor={back_cursor}"),
    )
    .await;
    assert_eq!(back["items"][0]["key"], "firmware");
}

#[tokio::test]
async fn rejects_bad_cursor() {
    let (app, _artifacts) = seeded_app().await;
    let (status, _) = get_json(&app, "/api/v1/artifacts?cursor=nothex").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn gets_artifact_and_404s_unknown() {
    let (app, _artifacts) = seeded_app().await;

    let (status, json) = get_json(&app, "/api/v1/artifacts/spectra").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["key"], "spectra");
    assert_eq!(json["version_count"], 2);

    let (status, _) = get_json(&app, "/api/v1/artifacts/missing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lists_versions_newest_first() {
    let (app, _artifacts) = seeded_app().await;

    let (status, json) = get_json(&app, "/api/v1/artifacts/spectra/versions").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["digest"], "sha256:bbb");
    assert_eq!(items[1]["digest"], "sha256:aaa");
}

#[tokio::test]
async fn evicts_a_version() {
    let (app, _artifacts) = seeded_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/artifacts/spectra/versions/sha256:aaa")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (status, _) = get_json(&app, "/api/v1/artifacts/spectra/versions/sha256:aaa").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn filters_artifacts_and_versions() {
    let (app, _artifacts) = seeded_app().await;

    // Substring match on key.
    let (status, json) = get_json(&app, "/api/v1/artifacts?q=spec").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["key"], "spectra");

    // Exact version filter on versions.
    let (_, hit) = get_json(&app, "/api/v1/artifacts/spectra/versions?version=0.2.0").await;
    assert_eq!(hit["items"].as_array().unwrap().len(), 2);
    let (_, miss) = get_json(&app, "/api/v1/artifacts/spectra/versions?version=9.9.9").await;
    assert!(miss["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn lists_downloads_with_filter() {
    let (app, _artifacts) = seeded_app().await;

    let (status, json) = get_json(&app, "/api/v1/downloads").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["items"].as_array().unwrap().len(), 1);
    assert_eq!(json["items"][0]["key"], "spectra");
    assert_eq!(json["items"][0]["status"], "succeeded");

    let (_, none) = get_json(&app, "/api/v1/downloads?status=failed").await;
    assert!(none["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn lists_logs_and_targets() {
    let (app, artifacts) = seeded_app().await;

    // Insert a test log event.
    let logs = artifacts.storage().logs().unwrap();
    logs.insert_event(
        aperture_artifacts::Level::Info,
        "aperture::test",
        jiff::Timestamp::now(),
    )
    .message(Some("test log message"))
    .fields(Some(r#"{"key":"value"}"#))
    .execute()
    .await
    .unwrap();

    let (status, json) = get_json(&app, "/api/v1/logs").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().unwrap();
    assert!(!items.is_empty());
    assert_eq!(items[0]["target"], "aperture::test");
    assert_eq!(items[0]["message"], "test log message");
    assert_eq!(items[0]["level"], "info");
    assert_eq!(items[0]["fields"]["key"], "value");

    let (status, json) = get_json(&app, "/api/v1/logs/targets").await;
    assert_eq!(status, StatusCode::OK);
    let targets = json.as_array().unwrap();
    assert!(targets.iter().any(|t| t == "aperture::test"));

    let (status, json) = get_json(&app, "/api/v1/logs?q=test").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!json["items"].as_array().unwrap().is_empty());

    let (status, json) = get_json(&app, "/api/v1/logs?min_level=warn").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().unwrap();
    assert!(items.iter().all(|i| {
        let level = i["level"].as_str().unwrap();
        matches!(level, "warn" | "error")
    }));
}

#[tokio::test]
async fn get_unknown_span_returns_404() {
    let (app, _artifacts) = seeded_app().await;

    let (status, _) = get_json(&app, "/api/v1/logs/spans/99999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_fields_filter_returns_400() {
    let (app, _artifacts) = seeded_app().await;

    let (status, _) = get_json(&app, "/api/v1/logs?fields=not-json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
