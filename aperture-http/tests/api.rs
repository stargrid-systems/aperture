use std::sync::Arc;
use std::{env, fs, process, str};

use aperture_artifacts::{Artifact, ArtifactKey, Artifacts, DownloadDefinition, Storage};
use aperture_auth::{Password, Role, Username};
use aperture_http::{AppState, AvatarAnimation, AvatarStyle, Spectra, SpectraConfig, app};
use aperture_settings::{SettingRegistry, Settings};
use aperture_storage::{ActorId, ArtifactId};
use aperture_tasks::{TaskRegistry, TaskStatus, Tasks};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH, LOCATION, SET_COOKIE};
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
        id: ArtifactId::from(0),
        key: ArtifactKey::new(key).unwrap(),
        source: "ghcr.io/stargrid-systems/spectra:0.2.0".to_owned(),
        digest: digest.parse().unwrap(),
        media_type: None,
        version: Some("0.2.0".to_owned()),
        size_bytes: 1234,
        downloaded_at: at(downloaded_at),
        verified_at: None,
    }
}

/// Builds a `Settings` whose registry knows about every setting the gateway
/// registers, so per-request reads fall back to definition defaults.
fn test_settings(storage: &Storage) -> Settings {
    let mut registry = SettingRegistry::new();
    registry.register(Arc::new(AvatarStyle::default()));
    registry.register(Arc::new(AvatarAnimation::default()));
    Settings::new(storage.settings().unwrap(), registry)
}

async fn seeded_app() -> (Router, Artifacts, Storage, String) {
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
    repo.record_version(&version("firmware", "sha256:ffff", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:aaaa", 2_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:bbbb", 3_000))
        .await
        .unwrap();

    let mut registry = TaskRegistry::new();
    registry.register(Arc::new(DownloadDefinition::new(artifacts.clone())));
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let auth = aperture_auth::AuthHandle::new(storage.clone())
        .await
        .unwrap();

    let spectra = Spectra::new(
        artifacts.clone(),
        tasks.clone(),
        SpectraConfig::default(),
        ActorId::SYSTEM,
    );

    let password = Password::generate();
    let actor = auth
        .create_user(&"test".parse::<Username>().unwrap(), &password, None)
        .await
        .unwrap();
    let (raw_key, api_key) = auth.create_api_key(actor.id, "test-key").await.unwrap();
    let subject = aperture_auth::apikey_subject(api_key.id);
    auth.assign_role(&subject, Role::Admin).await.unwrap();

    let settings = test_settings(&storage);
    let state = AppState::new(
        "test",
        Uuid::nil(),
        storage.clone(),
        spectra,
        tasks,
        settings,
        auth,
    );
    (app(state), artifacts, storage, raw_key.as_str().to_owned())
}

async fn get_json(app: &Router, token: &str, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    read_json(response).await
}

async fn post_json(app: &Router, token: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    read_json(response).await
}

async fn patch_json(app: &Router, token: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    read_json(response).await
}

async fn delete(app: &Router, token: &str, uri: &str) -> StatusCode {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
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
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, json) = get_json(&app, &token, "/api/v1/artifacts").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    // Ascending by key.
    assert_eq!(items[0]["key"], "firmware");
    assert_eq!(items[1]["key"], "spectra");
    assert_eq!(items[1]["version_count"], 2);
    assert_eq!(items[1]["digest"], "sha256:bbbb");
    assert!(json["next_cursor"].is_null());
}

#[tokio::test]
async fn paginates_artifacts_with_cursor() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, first) = get_json(&app, &token, "/api/v1/artifacts?limit=1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    assert_eq!(first["items"][0]["key"], "firmware");
    assert!(first["prev_cursor"].is_null());
    let cursor = first["next_cursor"].as_str().unwrap();

    let (_, second) = get_json(
        &app,
        &token,
        &format!("/api/v1/artifacts?limit=1&cursor={cursor}"),
    )
    .await;
    assert_eq!(second["items"][0]["key"], "spectra");
    assert!(second["next_cursor"].is_null());

    // Page back to the first item using the prev cursor.
    let back_cursor = second["prev_cursor"].as_str().expect("a previous page");
    let (_, back) = get_json(
        &app,
        &token,
        &format!("/api/v1/artifacts?limit=1&cursor={back_cursor}"),
    )
    .await;
    assert_eq!(back["items"][0]["key"], "firmware");
}

#[tokio::test]
async fn rejects_bad_cursor() {
    let (app, _artifacts, _storage, token) = seeded_app().await;
    let (status, _) = get_json(&app, &token, "/api/v1/artifacts?cursor=nothex").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn gets_artifact_and_404s_unknown() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, json) = get_json(&app, &token, "/api/v1/artifacts/spectra").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["key"], "spectra");
    assert_eq!(json["version_count"], 2);

    let (status, _) = get_json(&app, &token, "/api/v1/artifacts/missing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lists_versions_newest_first() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, json) = get_json(&app, &token, "/api/v1/artifacts/spectra/versions").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["digest"], "sha256:bbbb");
    assert_eq!(items[1]["digest"], "sha256:aaaa");
}

#[tokio::test]
async fn evicts_a_version() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/artifacts/spectra/versions/sha256:aaaa")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (status, _) = get_json(
        &app,
        &token,
        "/api/v1/artifacts/spectra/versions/sha256:aaaa",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lists_task_definitions() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, json) = get_json(&app, &token, "/api/v1/task-definitions").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().expect("paged items");
    let download = items
        .iter()
        .find(|def| def["key"] == "download")
        .expect("download key registered");
    assert_eq!(download["cancellable"], true);
    assert_eq!(download["resumable"], true);
    assert!(json["next_cursor"].is_null());
}

#[tokio::test]
async fn gets_task_definition_schema() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, json) = get_json(&app, &token, "/api/v1/task-definitions/download").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["key"], "download");
    let schema = &json["input_schema"];
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert!(
        schema["$defs"].is_object(),
        "download input should carry its dependencies: {schema}"
    );

    let (status, _) = get_json(&app, &token, "/api/v1/task-definitions/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn paginates_setting_definitions() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, json) = get_json(&app, &token, "/api/v1/setting-definitions?limit=1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["items"].as_array().map(Vec::len), Some(1));
    let cursor = json["next_cursor"]
        .as_str()
        .expect("another page")
        .to_owned();

    let (status, json) = get_json(
        &app,
        &token,
        &format!("/api/v1/setting-definitions?limit=1&cursor={cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["items"].as_array().map(Vec::len), Some(1));
    assert!(json["prev_cursor"].is_string());
}

#[tokio::test]
async fn lists_setting_definitions() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, json) = get_json(&app, &token, "/api/v1/setting-definitions").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().expect("paged items");
    assert!(
        items.iter().any(|def| def["key"] == "avatar_style"),
        "avatar style registered: {json}"
    );
}

#[tokio::test]
async fn gets_setting_definition_schema() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, json) = get_json(&app, &token, "/api/v1/setting-definitions/avatar_style").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["key"], "avatar_style");
    assert_eq!(
        json["value_schema"]["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );

    let (status, _) = get_json(&app, &token, "/api/v1/setting-definitions/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn serves_definition_routes_without_credentials() {
    let (app, _artifacts, _storage, _token) = seeded_app().await;

    for uri in [
        "/api/v1/task-definitions",
        "/api/v1/setting-definitions",
        "/api/v1/task-definitions/download",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "GET {uri} without credentials"
        );
    }
}

#[tokio::test]
async fn reads_recorded_tasks() {
    let (app, _artifacts, storage, token) = seeded_app().await;
    let repo = storage.tasks().unwrap();
    let id = repo
        .create(
            "download",
            None,
            ActorId::SYSTEM,
            &json!({"key": "spectra"}),
            at(1_000),
        )
        .await
        .unwrap();
    repo.finish(
        id,
        TaskStatus::Succeeded,
        at(2_000),
        Some(&json!({"digest": "sha256:bbbb"})),
        None,
    )
    .await
    .unwrap();

    let (status, list) = get_json(&app, &token, "/api/v1/tasks").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["key"], "download");

    let (status, task) = get_json(&app, &token, &format!("/api/v1/tasks/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task["status"], "succeeded");
    assert_eq!(task["input"]["key"], "spectra");
    assert_eq!(task["output"]["digest"], "sha256:bbbb");
}

#[tokio::test]
async fn filters_tasks_by_json_field() {
    let (app, _artifacts, storage, token) = seeded_app().await;
    let repo = storage.tasks().unwrap();
    let spectra = repo
        .create(
            "download",
            None,
            ActorId::SYSTEM,
            &json!({"key": "spectra"}),
            at(1_000),
        )
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
        .create(
            "download",
            None,
            ActorId::SYSTEM,
            &json!({"key": "other"}),
            at(1_100),
        )
        .await
        .unwrap();
    repo.finish(other, TaskStatus::Failed, at(1_150), None, Some("boom"))
        .await
        .unwrap();

    // Download history for one artifact key.
    let (status, list) = get_json(
        &app,
        &token,
        "/api/v1/tasks?key=download&input_path=key&input_value=spectra",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["input"]["key"], "spectra");

    // Filter by an output field.
    let (status, list) = get_json(
        &app,
        &token,
        "/api/v1/tasks?output_path=version&output_value=1.0",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    // A path without its value is a bad request.
    let (status, _) = get_json(&app, &token, "/api/v1/tasks?input_path=key").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A malformed path is a bad request.
    let (status, _) = get_json(
        &app,
        &token,
        "/api/v1/tasks?input_path=key;drop&input_value=x",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A structurally invalid path (empty segment) is a bad request, not a 500.
    let (status, _) = get_json(&app, &token, "/api/v1/tasks?input_path=a..b&input_value=x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_rejects_unknown_kind() {
    let (app, _artifacts, _storage, token) = seeded_app().await;
    let (status, _) = post_json(
        &app,
        &token,
        "/api/v1/tasks",
        json!({"key": "nope", "input": {}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_and_cancel_unknown_task_404() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, _) = get_json(&app, &token, "/api/v1/tasks/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = post_json(&app, &token, "/api/v1/tasks/999/cancel", Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn filters_artifacts_and_versions() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    // Substring match on key.
    let (status, json) = get_json(&app, &token, "/api/v1/artifacts?q=spec").await;
    assert_eq!(status, StatusCode::OK);
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["key"], "spectra");

    // Exact version filter on versions.
    let (_, hit) = get_json(
        &app,
        &token,
        "/api/v1/artifacts/spectra/versions?version=0.2.0",
    )
    .await;
    assert_eq!(hit["items"].as_array().unwrap().len(), 2);
    let (_, miss) = get_json(
        &app,
        &token,
        "/api/v1/artifacts/spectra/versions?version=9.9.9",
    )
    .await;
    assert!(miss["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn task_schedule_lifecycle() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    // Empty listing.
    let (status, json) = get_json(&app, &token, "/api/v1/task-schedules").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["items"].as_array().unwrap().is_empty());

    // Create.
    let (status, created) = post_json(
        &app,
        &token,
        "/api/v1/task-schedules",
        json!({
            "key": "download",
            "input": {"key": "spectra"},
            "interval": "PT5M",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["key"], "download");
    assert_eq!(created["interval"], "PT5M");
    assert_eq!(created["input"]["key"], "spectra");
    assert_eq!(created["enabled"], true);
    let id = created["id"].as_str().unwrap();

    // Listing reflects the new row.
    let (_, json) = get_json(&app, &token, "/api/v1/task-schedules").await;
    assert_eq!(json["items"].as_array().unwrap().len(), 1);
    assert_eq!(json["items"][0]["id"], id);

    // Get.
    let (status, fetched) = get_json(&app, &token, &format!("/api/v1/task-schedules/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["key"], "download");

    // Patch only the interval. Enabled stays true.
    let (status, updated) = patch_json(
        &app,
        &token,
        &format!("/api/v1/task-schedules/{id}"),
        json!({"interval": "PT1H"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["interval"], "PT1H");
    assert_eq!(updated["enabled"], true);

    // Patch only enabled. Interval stays as set above.
    let (status, updated) = patch_json(
        &app,
        &token,
        &format!("/api/v1/task-schedules/{id}"),
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["interval"], "PT1H");
    assert_eq!(updated["enabled"], false);

    // Empty patch returns the row unchanged.
    let (status, updated) = patch_json(
        &app,
        &token,
        &format!("/api/v1/task-schedules/{id}"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["interval"], "PT1H");
    assert_eq!(updated["enabled"], false);

    // Delete.
    assert_eq!(
        delete(&app, &token, &format!("/api/v1/task-schedules/{id}")).await,
        StatusCode::NO_CONTENT
    );
    // Second delete is a 404.
    assert_eq!(
        delete(&app, &token, &format!("/api/v1/task-schedules/{id}")).await,
        StatusCode::NOT_FOUND
    );
    let (status, _) = get_json(&app, &token, &format!("/api/v1/task-schedules/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn task_schedule_unknown_returns_404() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let (status, _) = get_json(&app, &token, "/api/v1/task-schedules/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = patch_json(
        &app,
        &token,
        "/api/v1/task-schedules/999",
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn put_then_get_round_trips_blob_bytes() {
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .header("authorization", format!("Bearer {token}"))
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
    let (status, _) = get_json(&app, &token, &location).await;
    assert_eq!(status, StatusCode::OK);

    // Fetch the blob bytes back.
    let blob_uri = format!("/api/v1/artifacts/firmware/versions/{digest}/blob");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&blob_uri)
                .header("authorization", format!("Bearer {token}"))
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
async fn put_rejects_slash_in_key() {
    // Artifact keys must be URL-safe. A `%2F` in the URL decodes to `/`
    // before routing, so the artifact key validator sees `tls/server-cert`
    // and rejects it.
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .header("authorization", format!("Bearer {token}"))
                .uri("/api/v1/artifacts/tls%2Fserver-cert")
                .header(CONTENT_TYPE, "application/pkix-cert")
                .body(Body::from(&b"der-bytes"[..]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn put_rejects_html_media_type_parameters() {
    // Sanitisation rejects media types with parameters so a malicious client
    // cannot inject `text/html; charset=...` and have it echoed back verbatim.
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .header("authorization", format!("Bearer {token}"))
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
                .header("authorization", format!("Bearer {token}"))
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
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let put = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .header("authorization", format!("Bearer {token}"))
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
                .header("authorization", format!("Bearer {token}"))
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
    let (app, _artifacts, _storage, token) = seeded_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/artifacts/firmware/versions/sha256:deadbeef/blob")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Builds an app whose single API key carries `role`, with no pre-seeded
/// artifacts. Used by the authorization tests.
async fn app_with_role(role: Role) -> (Router, String) {
    let root = env::temp_dir().join(format!(
        "aperture-api-{}-{}",
        process::id(),
        uuid::Uuid::new_v4()
    ));
    let _ = fs::remove_dir_all(&root);
    let storage = Storage::open(":memory:").await.unwrap();
    let artifacts = Artifacts::new(storage.clone(), root);

    let mut registry = TaskRegistry::new();
    registry.register(Arc::new(DownloadDefinition::new(artifacts.clone())));
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let auth = aperture_auth::AuthHandle::new(storage.clone())
        .await
        .unwrap();

    let spectra = Spectra::new(
        artifacts.clone(),
        tasks.clone(),
        SpectraConfig::default(),
        ActorId::SYSTEM,
    );

    let password = Password::generate();
    let actor = auth
        .create_user(&role.as_str().parse::<Username>().unwrap(), &password, None)
        .await
        .unwrap();
    let (raw_key, api_key) = auth.create_api_key(actor.id, "key").await.unwrap();
    let subject = aperture_auth::apikey_subject(api_key.id);
    auth.assign_role(&subject, role).await.unwrap();

    let settings = test_settings(&storage);
    let state = AppState::new(
        "test",
        Uuid::nil(),
        storage.clone(),
        spectra,
        tasks,
        settings,
        auth,
    );
    (app(state), raw_key.as_str().to_owned())
}

#[tokio::test]
async fn viewer_is_forbidden_from_user_management() {
    let (app, token) = app_with_role(Role::Viewer).await;

    let (status, _) = get_json(&app, &token, "/api/v1/users").await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let pw = test_password();
    let (status, _) = post_json(
        &app,
        &token,
        "/api/v1/users",
        json!({"username": "new-user", "password": &pw}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_can_create_users() {
    let (app, token) = app_with_role(Role::Admin).await;

    let pw = test_password();
    let (status, _) = post_json(
        &app,
        &token,
        "/api/v1/users",
        json!({"username": "new-user", "password": &pw}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

// --- helpers for the security tests ---

/// Generates a random valid password for tests.
fn test_password() -> String {
    Password::generate().as_str().to_owned()
}

/// Builds a fresh app with no pre-seeded data and returns it alongside the
/// auth handle and storage so tests can create users and keys directly.
async fn fresh_app() -> (Router, aperture_auth::AuthHandle, Storage) {
    let root = env::temp_dir().join(format!(
        "aperture-api-{}-{}",
        process::id(),
        uuid::Uuid::new_v4()
    ));
    let _ = fs::remove_dir_all(&root);
    let storage = Storage::open(":memory:").await.unwrap();
    let artifacts = Artifacts::new(storage.clone(), root);

    let mut registry = TaskRegistry::new();
    registry.register(Arc::new(DownloadDefinition::new(artifacts.clone())));
    let tasks = Tasks::new(storage.tasks().unwrap(), registry);

    let auth = aperture_auth::AuthHandle::new(storage.clone())
        .await
        .unwrap();

    let spectra = Spectra::new(
        artifacts.clone(),
        tasks.clone(),
        SpectraConfig::default(),
        ActorId::SYSTEM,
    );

    let settings = test_settings(&storage);
    let state = AppState::new(
        "test",
        Uuid::nil(),
        storage.clone(),
        spectra,
        tasks,
        settings,
        auth.clone(),
    );
    (app(state), auth, storage)
}

/// Creates a user and returns an API key carrying `role`.
async fn key_for_role(
    auth: &aperture_auth::AuthHandle,
    username: &str,
    role: Role,
) -> (aperture_storage::ActorId, String) {
    let pw = Password::generate();
    let actor = auth
        .create_user(&username.parse::<Username>().unwrap(), &pw, None)
        .await
        .unwrap();
    let (raw, api_key) = auth.create_api_key(actor.id, "k").await.unwrap();
    auth.assign_role(&aperture_auth::apikey_subject(api_key.id), role)
        .await
        .unwrap();
    (actor.id, raw.as_str().to_owned())
}

/// Creates an API key with no role assigned (authenticated but unprivileged).
async fn no_role_key(
    auth: &aperture_auth::AuthHandle,
    username: &str,
) -> (aperture_storage::ActorId, String) {
    let pw = Password::generate();
    let actor = auth
        .create_user(&username.parse::<Username>().unwrap(), &pw, None)
        .await
        .unwrap();
    let (raw, _) = auth.create_api_key(actor.id, "k").await.unwrap();
    (actor.id, raw.as_str().to_owned())
}

/// Logs in and returns the session cookie value, or `None` on non-200.
async fn login(app: &Router, username: &str, password: &str) -> Option<String> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"username": username, "password": password}))
                        .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    if response.status() != StatusCode::OK {
        return None;
    }
    let cookie = response.headers().get(SET_COOKIE)?.to_str().ok()?;
    cookie
        .split(';')
        .next()?
        .strip_prefix("aperture_session=")
        .map(str::to_owned)
}

/// Status of a GET authenticated by a session cookie.
async fn get_with_cookie(app: &Router, cookie: &str, uri: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("cookie", format!("aperture_session={cookie}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn post_with_cookie(app: &Router, cookie: &str, uri: &str, body: Value) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("cookie", format!("aperture_session={cookie}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn post_no_auth(app: &Router, uri: &str, body: Value) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

// --- security tests ---

/// Regression net for the task-schedules class of bug: an authenticated but
/// unprivileged token must be denied on every mutating endpoint. Adding a new
/// resource without a `require()` call makes its mutation succeed here, failing
/// the test. Bodies are kept valid enough to pass JSON extraction so the
/// request reaches the authorization check.
#[tokio::test]
async fn no_role_token_is_denied_on_all_mutations() {
    let (app, auth, _storage) = fresh_app().await;
    let (_id, token) = no_role_key(&auth, "nobody").await;

    // (method, uri, json body)
    let pw = test_password();
    let matrix: [(&str, &str, Value); 10] = [
        ("PUT", "/api/v1/artifacts/firmware", Value::Null),
        (
            "POST",
            "/api/v1/tasks",
            json!({"key": "download", "input": {}}),
        ),
        ("POST", "/api/v1/tasks/1/cancel", Value::Null),
        (
            "POST",
            "/api/v1/task-schedules",
            json!({"key": "download", "input": {}, "interval": "PT5M"}),
        ),
        ("PATCH", "/api/v1/task-schedules/1", json!({})),
        ("DELETE", "/api/v1/task-schedules/1", Value::Null),
        (
            "POST",
            "/api/v1/users",
            json!({"username": "x", "password": &pw}),
        ),
        ("DELETE", "/api/v1/users/1", Value::Null),
        ("POST", "/api/v1/api-keys", json!({"name": "k"})),
        (
            "DELETE",
            "/api/v1/artifacts/firmware/versions/sha256:\
             0000000000000000000000000000000000000000000000000000000000000000",
            Value::Null,
        ),
    ];

    for (method, uri, body) in matrix {
        let bytes = if body.is_null() {
            b"{}".to_vec()
        } else {
            serde_json::to_vec(&body).unwrap()
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(bytes))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} {uri}: expected 403 for no-role token"
        );
    }
}

#[tokio::test]
async fn viewer_cannot_download_artifact_blob() {
    let (app, auth, _storage) = fresh_app().await;
    let (_admin_actor, admin_key) = key_for_role(&auth, "admin", Role::Admin).await;
    let (_viewer_actor, viewer_key) = key_for_role(&auth, "viewer", Role::Viewer).await;

    // Admin stores an artifact (e.g. a secret key).
    let put = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .header("authorization", format!("Bearer {admin_key}"))
                .uri("/api/v1/artifacts/server-key")
                .header(CONTENT_TYPE, "application/octet-stream")
                .body(Body::from(&b"top-secret-key-bytes"[..]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::CREATED);
    let body = to_bytes(put.into_body(), usize::MAX).await.unwrap();
    let digest = serde_json::from_slice::<Value>(&body).unwrap()["digest"]
        .as_str()
        .unwrap()
        .to_owned();

    // Viewer can read the catalog metadata...
    let (status, _) = get_json(
        &app,
        &viewer_key,
        &format!("/api/v1/artifacts/server-key/versions/{digest}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // ...but cannot download the blob content.
    let blob_status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/artifacts/server-key/versions/{digest}/blob"
                ))
                .header("authorization", format!("Bearer {viewer_key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status();
    assert_eq!(blob_status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn change_password_revokes_other_sessions() {
    let (app, auth, _storage) = fresh_app().await;
    let pw = test_password();
    let new_pw = test_password();
    auth.create_user(&"alice".parse().unwrap(), &Password::new(pw.clone()), None)
        .await
        .unwrap();

    let cookie_a = login(&app, "alice", &pw).await.expect("login a");
    let cookie_b = login(&app, "alice", &pw).await.expect("login b");
    // Sanity: both sessions work before the change.
    assert_eq!(
        get_with_cookie(&app, &cookie_a, "/api/v1/version").await,
        StatusCode::OK
    );
    assert_eq!(
        get_with_cookie(&app, &cookie_b, "/api/v1/version").await,
        StatusCode::OK
    );

    let status = post_with_cookie(
        &app,
        &cookie_a,
        "/api/v1/auth/change-password",
        json!({"current_password": &pw, "new_password": &new_pw}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The caller's session survives; the other session is revoked.
    assert_eq!(
        get_with_cookie(&app, &cookie_a, "/api/v1/version").await,
        StatusCode::OK
    );
    assert_eq!(
        get_with_cookie(&app, &cookie_b, "/api/v1/version").await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn change_password_rejects_reuse() {
    let (app, auth, _storage) = fresh_app().await;
    let pw = test_password();
    auth.create_user(&"alice".parse().unwrap(), &Password::new(pw.clone()), None)
        .await
        .unwrap();
    let cookie = login(&app, "alice", &pw).await.expect("login");

    let status = post_with_cookie(
        &app,
        &cookie,
        "/api/v1/auth/change-password",
        json!({"current_password": &pw, "new_password": &pw}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_rate_limited_after_burst() {
    let (app, auth, _storage) = fresh_app().await;
    let correct_pw = test_password();
    let wrong_pw = test_password();
    auth.create_user(
        &"alice".parse().unwrap(),
        &Password::new(correct_pw.clone()),
        None,
    )
    .await
    .unwrap();

    for _ in 0..5 {
        assert!(login(&app, "alice", &wrong_pw).await.is_none());
    }
    // The next attempt is rejected by the limiter before credentials are checked.
    let status = post_no_auth(
        &app,
        "/api/v1/auth/login",
        json!({"username": "alice", "password": &wrong_pw}),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn setup_rejects_invalid_username_and_short_password() {
    let (app, _auth, _storage) = fresh_app().await;

    let pw = test_password();
    let bad_user = post_no_auth(
        &app,
        "/api/v1/auth/setup",
        json!({"username": "bad name", "password": &pw}),
    )
    .await;
    assert!(
        bad_user.is_client_error(),
        "invalid username should be rejected: {bad_user}"
    );

    let short = "x".repeat(5);
    let short_pw = post_no_auth(
        &app,
        "/api/v1/auth/setup",
        json!({"username": "admin", "password": &short}),
    )
    .await;
    assert!(
        short_pw.is_client_error(),
        "short password should be rejected: {short_pw}"
    );
}

#[tokio::test]
async fn admin_cannot_delete_self() {
    let (app, auth, storage) = fresh_app().await;
    let (alice_actor, alice_key) = key_for_role(&auth, "alice", Role::Admin).await;
    let user_id = storage
        .users()
        .unwrap()
        .find_by_actor_id(alice_actor)
        .await
        .unwrap()
        .unwrap()
        .id;

    let status = delete(&app, &alice_key, &format!("/api/v1/users/{user_id}")).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn no_role_token_cannot_delete_others_api_key() {
    let (app, auth, _storage) = fresh_app().await;
    let (_admin_actor, admin_key) = key_for_role(&auth, "admin", Role::Admin).await;
    let (_, api_key_json) =
        post_json(&app, &admin_key, "/api/v1/api-keys", json!({"name": "k"})).await;
    let key_id = api_key_json["id"].as_str().unwrap();

    let (_nobody_actor, nobody_key) = no_role_key(&auth, "nobody").await;
    let status = delete(&app, &nobody_key, &format!("/api/v1/api-keys/{key_id}")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn change_password_rejected_for_api_key_auth() {
    let (app, auth, _storage) = fresh_app().await;
    let (_actor, api_key) = key_for_role(&auth, "alice", Role::Admin).await;

    let pw = test_password();
    let new_pw = test_password();
    let (status, _) = post_json(
        &app,
        &api_key,
        "/api/v1/auth/change-password",
        json!({"current_password": &pw, "new_password": &new_pw}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn current_actor_returns_identity_and_role() {
    let (app, auth, storage) = fresh_app().await;

    // API-key caller: resolves to its owning user actor, carries the key's role.
    let (api_actor, token) = key_for_role(&auth, "alice", Role::Operator).await;
    let alice_id = storage
        .users()
        .unwrap()
        .find_by_actor_id(api_actor)
        .await
        .unwrap()
        .unwrap()
        .id;
    let (status, json) = get_json(&app, &token, "/api/v1/auth/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["actor_id"], api_actor.get().to_string());
    assert_eq!(json["user_id"], alice_id.get().to_string());
    assert_eq!(json["username"], "alice");
    assert_eq!(json["roles"], serde_json::json!(["operator"]));
    assert_eq!(json["must_change_password"], false);

    // Session caller: the user's username and role are populated.
    let pw = test_password();
    let actor = auth
        .create_user(&"carol".parse().unwrap(), &Password::new(pw.clone()), None)
        .await
        .unwrap();
    auth.assign_role(&aperture_auth::actor_subject(actor.id), Role::Admin)
        .await
        .unwrap();
    let carol_id = storage
        .users()
        .unwrap()
        .find_by_actor_id(actor.id)
        .await
        .unwrap()
        .unwrap()
        .id;
    let cookie = login(&app, "carol", &pw).await.expect("login");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/me")
                .header("cookie", format!("aperture_session={cookie}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, json) = read_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["user_id"], carol_id.get().to_string());
    assert_eq!(json["username"], "carol");
    assert_eq!(json["display_name"], "carol");
    assert_eq!(json["roles"], serde_json::json!(["admin"]));
}

#[tokio::test]
async fn current_actor_requires_authentication() {
    let (app, _auth, _storage) = fresh_app().await;

    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn user_avatar_returns_svg_and_requires_auth() {
    let (app, auth, storage) = fresh_app().await;
    let (actor, token) = key_for_role(&auth, "dave", Role::Viewer).await;
    let user_id = storage
        .users()
        .unwrap()
        .find_by_actor_id(actor)
        .await
        .unwrap()
        .unwrap()
        .id;
    let uri = format!("/api/v1/users/{}/avatar", user_id.get());

    // Unauthenticated requests are rejected.
    let status = app
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Authenticated requests get inline SVG plus a strong ETag.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "image/svg+xml"
    );
    // The representation can be reconfigured at runtime, so it is not immutable.
    assert_eq!(
        response.headers().get(CACHE_CONTROL).unwrap(),
        "public, max-age=31536000"
    );
    let etag = response
        .headers()
        .get(ETAG)
        .expect("avatar response carries an ETag")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        etag.starts_with("\"avatar-") && etag.ends_with('"'),
        "expected a strong avatar etag, got {etag}"
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let svg = str::from_utf8(&body).unwrap();
    assert!(svg.starts_with("<svg "), "got: {svg}");
    assert!(svg.ends_with("</svg>"));

    // The same user always renders the same avatar.
    let again = get_bytes(&app, &token, &uri).await;
    assert_eq!(again, body);
}

#[tokio::test]
async fn avatar_style_setting_changes_output_and_etag() {
    let (app, auth, storage) = fresh_app().await;
    let (actor, token) = key_for_role(&auth, "dave", Role::Admin).await;
    let user_id = storage
        .users()
        .unwrap()
        .find_by_actor_id(actor)
        .await
        .unwrap()
        .unwrap()
        .id;
    let uri = format!("/api/v1/users/{}/avatar", user_id.get());

    // Baseline GET with the default (constellation) style.
    let baseline = get_avatar(&app, &token, &uri).await;
    let default_etag = baseline.etag;
    let default_body = baseline.body;

    // Flip the style to "planets" through the settings API.
    let body = serde_json::to_vec(&json!({"value": "planets"})).unwrap();
    let put = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/settings/avatar_style")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::OK);

    // The new representation has a different ETag and SVG body.
    let after = get_avatar(&app, &token, &uri).await;
    assert_ne!(
        after.etag, default_etag,
        "changing the style must change the ETag"
    );
    assert_ne!(
        after.body, default_body,
        "changing the style must change the SVG body"
    );

    // An If-None-Match carrying the stale ETag still returns the new body.
    let stale = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("authorization", format!("Bearer {token}"))
                .header(IF_NONE_MATCH, &default_etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::OK);

    // An If-None-Match carrying the current ETag yields a 304 with an empty
    // body and the same ETag echoed back.
    let fresh = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("authorization", format!("Bearer {token}"))
                .header(IF_NONE_MATCH, &after.etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fresh.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        fresh.headers().get(ETAG).unwrap().to_str().unwrap(),
        after.etag
    );
    let fresh_body = to_bytes(fresh.into_body(), usize::MAX).await.unwrap();
    assert!(fresh_body.is_empty(), "304 body must be empty");
}

struct AvatarResponse {
    etag: String,
    body: bytes::Bytes,
}

async fn get_avatar(app: &Router, token: &str, uri: &str) -> AvatarResponse {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response
        .headers()
        .get(ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    AvatarResponse { etag, body }
}

async fn get_bytes(app: &Router, token: &str, uri: &str) -> bytes::Bytes {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), usize::MAX).await.unwrap()
}
