use std::{env, fs, process};

use aperture_artifacts::{Artifact, ArtifactKey, Artifacts, DownloadDefinition, Storage};
use aperture_http::{AppState, Spectra, SpectraConfig, app};
use aperture_storage::DbId;
use aperture_tasks::{TaskRegistry, TaskStatus, Tasks};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::{CONTENT_TYPE, ETAG, IF_NONE_MATCH, LOCATION};
use axum::http::{HeaderValue, Request, StatusCode};
use axum::response::Response;
use jiff::Timestamp;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

fn at(micros: i64) -> Timestamp {
    Timestamp::from_microsecond(micros).unwrap()
}

fn version(key: &'static str, digest: &str, downloaded_at: i64) -> Artifact {
    Artifact {
        id: DbId::from(0),
        key: ArtifactKey::new(key).unwrap(),
        source: "ghcr.io/stargrid-systems/spectra:0.2.0".to_owned(),
        digest: digest.to_owned(),
        media_type: None,
        version: Some("0.2.0".to_owned()),
        size_bytes: 1234,
        downloaded_at: at(downloaded_at),
        verified_at: None,
    }
}

async fn seeded_app() -> (Router, Artifacts, Storage) {
    // Unique per call so parallel tests in the same binary do not stomp on
    // each other's blob store.
    let root = env::temp_dir().join(format!(
        "aperture-api-{}-{}",
        process::id(),
        uuid::Uuid::new_v4()
    ));
    let _ = fs::remove_dir_all(&root);
    let storage = Storage::open(":memory:").await.unwrap();
    let artifacts = Artifacts::new(storage.clone(), root);

    let repo = storage.artifacts().unwrap();
    repo.record_version(&version("firmware", "sha256:fff", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:aaa", 2_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:bbb", 3_000))
        .await
        .unwrap();

    let mut registry = TaskRegistry::new();
    registry.register(DownloadDefinition::new(artifacts.clone()));
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let spectra = Spectra::new(artifacts.clone(), tasks.clone(), SpectraConfig::default());
    let state = AppState::new("test", Uuid::nil(), storage.clone(), spectra, tasks);
    (app(state), artifacts, storage)
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    read_json(response).await
}

async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    read_json(response).await
}

async fn patch_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    read_json(response).await
}

async fn delete(app: &Router, uri: &str) -> StatusCode {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    response.status()
}

async fn read_json(response: Response) -> (StatusCode, Value) {
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
    let (app, _artifacts, _storage) = seeded_app().await;

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
    let (app, _artifacts, _storage) = seeded_app().await;

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
    let (app, _artifacts, _storage) = seeded_app().await;
    let (status, _) = get_json(&app, "/api/v1/artifacts?cursor=nothex").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn gets_artifact_and_404s_unknown() {
    let (app, _artifacts, _storage) = seeded_app().await;

    let (status, json) = get_json(&app, "/api/v1/artifacts/spectra").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["key"], "spectra");
    assert_eq!(json["version_count"], 2);

    let (status, _) = get_json(&app, "/api/v1/artifacts/missing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lists_versions_newest_first() {
    let (app, _artifacts, _storage) = seeded_app().await;

    let (status, json) = get_json(&app, "/api/v1/artifacts/spectra/versions").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["digest"], "sha256:bbb");
    assert_eq!(items[1]["digest"], "sha256:aaa");
}

#[tokio::test]
async fn evicts_a_version() {
    let (app, _artifacts, _storage) = seeded_app().await;

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
async fn lists_task_definitions_with_schemas() {
    let (app, _artifacts, _storage) = seeded_app().await;

    let (status, json) = get_json(&app, "/api/v1/task-definitions").await;
    assert_eq!(status, StatusCode::OK);
    let download = json
        .as_array()
        .unwrap()
        .iter()
        .find(|def| def["kind"] == "download")
        .expect("download kind registered");
    assert_eq!(download["cancellable"], true);
    assert_eq!(download["resumable"], true);
    assert!(download["input_schema"].is_object());
}

#[tokio::test]
async fn reads_recorded_tasks() {
    let (app, _artifacts, storage) = seeded_app().await;
    let repo = storage.tasks().unwrap();
    let id = repo
        .create("download", None, &json!({"key": "spectra"}), at(1_000))
        .await
        .unwrap();
    repo.finish(
        id,
        TaskStatus::Succeeded,
        at(2_000),
        Some(&json!({"digest": "sha256:bbb"})),
        None,
    )
    .await
    .unwrap();

    let (status, list) = get_json(&app, "/api/v1/tasks").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["kind"], "download");

    let (status, task) = get_json(&app, &format!("/api/v1/tasks/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task["status"], "succeeded");
    assert_eq!(task["input"]["key"], "spectra");
    assert_eq!(task["output"]["digest"], "sha256:bbb");
}

#[tokio::test]
async fn filters_tasks_by_json_field() {
    let (app, _artifacts, storage) = seeded_app().await;
    let repo = storage.tasks().unwrap();
    let spectra = repo
        .create("download", None, &json!({"key": "spectra"}), at(1_000))
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
        .create("download", None, &json!({"key": "other"}), at(1_100))
        .await
        .unwrap();
    repo.finish(other, TaskStatus::Failed, at(1_150), None, Some("boom"))
        .await
        .unwrap();

    // Download history for one artifact key.
    let (status, list) = get_json(
        &app,
        "/api/v1/tasks?kind=download&input_path=key&input_value=spectra",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["input"]["key"], "spectra");

    // Filter by an output field.
    let (status, list) = get_json(&app, "/api/v1/tasks?output_path=version&output_value=1.0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    // A path without its value is a bad request.
    let (status, _) = get_json(&app, "/api/v1/tasks?input_path=key").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A malformed path is a bad request.
    let (status, _) = get_json(&app, "/api/v1/tasks?input_path=key;drop&input_value=x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A structurally invalid path (empty segment) is a bad request, not a 500.
    let (status, _) = get_json(&app, "/api/v1/tasks?input_path=a..b&input_value=x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_rejects_unknown_kind() {
    let (app, _artifacts, _storage) = seeded_app().await;
    let (status, _) = post_json(&app, "/api/v1/tasks", json!({"kind": "nope", "input": {}})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_and_cancel_unknown_task_404() {
    let (app, _artifacts, _storage) = seeded_app().await;

    let (status, _) = get_json(&app, "/api/v1/tasks/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = post_json(&app, "/api/v1/tasks/999/cancel", Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn filters_artifacts_and_versions() {
    let (app, _artifacts, _storage) = seeded_app().await;

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
async fn task_schedule_lifecycle() {
    let (app, _artifacts, _storage) = seeded_app().await;

    // Empty listing.
    let (status, json) = get_json(&app, "/api/v1/task-schedules").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["items"].as_array().unwrap().is_empty());

    // Create.
    let (status, created) = post_json(
        &app,
        "/api/v1/task-schedules",
        json!({
            "kind": "download",
            "input": {"key": "spectra"},
            "interval": "PT5M",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["kind"], "download");
    assert_eq!(created["interval"], "PT5M");
    assert_eq!(created["input"]["key"], "spectra");
    assert_eq!(created["enabled"], true);
    let id = created["id"].as_str().unwrap();

    // Listing reflects the new row.
    let (_, json) = get_json(&app, "/api/v1/task-schedules").await;
    assert_eq!(json["items"].as_array().unwrap().len(), 1);
    assert_eq!(json["items"][0]["id"], id);

    // Get.
    let (status, fetched) = get_json(&app, &format!("/api/v1/task-schedules/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["kind"], "download");

    // Patch only the interval; enabled stays true.
    let (status, updated) = patch_json(
        &app,
        &format!("/api/v1/task-schedules/{id}"),
        json!({"interval": "PT1H"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["interval"], "PT1H");
    assert_eq!(updated["enabled"], true);

    // Patch only enabled; interval stays as set above.
    let (status, updated) = patch_json(
        &app,
        &format!("/api/v1/task-schedules/{id}"),
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["interval"], "PT1H");
    assert_eq!(updated["enabled"], false);

    // Empty patch returns the row unchanged.
    let (status, updated) =
        patch_json(&app, &format!("/api/v1/task-schedules/{id}"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["interval"], "PT1H");
    assert_eq!(updated["enabled"], false);

    // Delete.
    assert_eq!(
        delete(&app, &format!("/api/v1/task-schedules/{id}")).await,
        StatusCode::NO_CONTENT
    );
    // Second delete is a 404.
    assert_eq!(
        delete(&app, &format!("/api/v1/task-schedules/{id}")).await,
        StatusCode::NOT_FOUND
    );
    let (status, _) = get_json(&app, &format!("/api/v1/task-schedules/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn task_schedule_unknown_returns_404() {
    let (app, _artifacts, _storage) = seeded_app().await;

    let (status, _) = get_json(&app, "/api/v1/task-schedules/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = patch_json(
        &app,
        "/api/v1/task-schedules/999",
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn put_then_get_round_trips_blob_bytes() {
    let (app, _artifacts, _storage) = seeded_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/artifacts/firmware")
                .header(CONTENT_TYPE, "application/octet-stream")
                .body(Body::from(&b"hello aperture"[..]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let location = response
        .headers()
        .get(LOCATION)
        .expect("201 has a Location header")
        .to_str()
        .unwrap()
        .to_owned();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let digest = json["digest"].as_str().unwrap().to_owned();
    assert!(
        digest.starts_with("sha256:"),
        "digest should be sha256-prefixed, got {digest}"
    );
    // The Location header must point at a route the gateway actually serves.
    // Following it must reach the version we just uploaded.
    assert_eq!(
        location,
        format!("/api/v1/artifacts/firmware/versions/{digest}")
    );
    let (status, _) = get_json(&app, &location).await;
    assert_eq!(status, StatusCode::OK);

    // Fetch the blob bytes back.
    let blob_uri = format!("/api/v1/artifacts/firmware/versions/{digest}/blob");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&blob_uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/octet-stream"
    );
    assert_eq!(
        response.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    let etag = response
        .headers()
        .get(ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(etag, format!("\"{digest}\""));
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&body[..], b"hello aperture");
}

#[tokio::test]
async fn put_url_encodes_location_for_multi_segment_key() {
    // Regression test: a key like `tls/server-cert` contains a slash. The
    // Location header must percent-encode it so following the link routes
    // back to the resource instead of producing a 404.
    let (app, _artifacts, _storage) = seeded_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/artifacts/tls%2Fserver-cert")
                .header(CONTENT_TYPE, "application/pkix-cert")
                .body(Body::from(&b"der-bytes"[..]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let location = response
        .headers()
        .get(LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        location.starts_with("/api/v1/artifacts/tls%2Fserver-cert/versions/"),
        "Location must encode the slash, got {location}"
    );

    // Following the encoded Location must succeed.
    let (status, _) = get_json(&app, &location).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn put_rejects_html_media_type_parameters() {
    // Sanitisation rejects media types with parameters so a malicious client
    // cannot inject `text/html; charset=...` and have it echoed back verbatim.
    let (app, _artifacts, _storage) = seeded_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/artifacts/firmware")
                .header(CONTENT_TYPE, "text/html; charset=utf-8")
                .body(Body::from(&b"<html>"[..]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let digest = json["digest"].as_str().unwrap().to_owned();

    let blob_uri = format!("/api/v1/artifacts/firmware/versions/{digest}/blob");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&blob_uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // The sanitised media type is dropped, so the GET falls back to the safe
    // default rather than replaying `text/html`.
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/octet-stream"
    );
}

#[tokio::test]
async fn blob_returns_304_when_etag_matches() {
    let (app, _artifacts, _storage) = seeded_app().await;

    let put = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/artifacts/firmware")
                .body(Body::from(&b"conditional"[..]))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(put.into_body(), usize::MAX).await.unwrap();
    let digest = serde_json::from_slice::<Value>(&body).unwrap()["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let etag = HeaderValue::from_str(&format!("\"{digest}\"")).unwrap();

    let blob_uri = format!("/api/v1/artifacts/firmware/versions/{digest}/blob");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&blob_uri)
                .header(IF_NONE_MATCH, etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        response.headers().get(ETAG).unwrap().to_str().unwrap(),
        format!("\"{digest}\"")
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(body.is_empty(), "304 response body should be empty");
}

#[tokio::test]
async fn blob_404_when_digest_unknown() {
    let (app, _artifacts, _storage) = seeded_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/artifacts/firmware/versions/sha256:deadbeef/blob")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
