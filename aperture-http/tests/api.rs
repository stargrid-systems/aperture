use std::sync::Arc;
use std::{env, fs, process};

use aperture_artifacts::{Artifact, Artifacts, DownloadDefinition, Storage};
use aperture_http::{AppState, Spectra, SpectraConfig, app};
use aperture_tasks::{TaskRegistry, TaskStatus, Tasks};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use jiff::Timestamp;
use serde_json::{Value, json};
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

async fn seeded_app() -> (Router, Arc<Artifacts>) {
    let root = env::temp_dir().join(format!("aperture-api-{}", process::id()));
    let _ = fs::remove_dir_all(&root);
    let storage = Storage::open(":memory:").await.unwrap();
    let artifacts = Arc::new(Artifacts::new(storage, root));

    let repo = artifacts.storage().artifacts();
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
    registry.register(DownloadDefinition::new(Arc::clone(&artifacts)));
    let tasks = Tasks::new(artifacts.storage().clone(), registry);

    let spectra = Spectra::new(
        Arc::clone(&artifacts),
        tasks.clone(),
        SpectraConfig::default(),
    );
    let state = AppState::new("test", uuid::Uuid::nil(), spectra, tasks);
    (app(state), artifacts)
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
async fn lists_task_definitions_with_schemas() {
    let (app, _artifacts) = seeded_app().await;

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
    let (app, artifacts) = seeded_app().await;
    let repo = artifacts.storage().tasks();
    let id = repo
        .create("download", None, r#"{"key":"spectra"}"#, at(1_000))
        .await
        .unwrap();
    repo.finish(
        id,
        TaskStatus::Succeeded,
        at(2_000),
        Some(r#"{"digest":"sha256:bbb"}"#),
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
    let (app, artifacts) = seeded_app().await;
    let repo = artifacts.storage().tasks();
    let spectra = repo
        .create("download", None, r#"{"key":"spectra"}"#, at(1_000))
        .await
        .unwrap();
    repo.finish(
        spectra,
        TaskStatus::Succeeded,
        at(1_050),
        Some(r#"{"version":"1.0"}"#),
        None,
    )
    .await
    .unwrap();
    let other = repo
        .create("download", None, r#"{"key":"other"}"#, at(1_100))
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
    let (app, _artifacts) = seeded_app().await;
    let (status, _) = post_json(&app, "/api/v1/tasks", json!({"kind": "nope", "input": {}})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_and_cancel_unknown_task_404() {
    let (app, _artifacts) = seeded_app().await;

    let (status, _) = get_json(&app, "/api/v1/tasks/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = post_json(&app, "/api/v1/tasks/999/cancel", Value::Null).await;
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
