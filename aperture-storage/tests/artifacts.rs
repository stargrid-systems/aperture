use std::{env, fs};

use aperture_storage::{Artifact, ArtifactKind, ArtifactStatus, DownloadStatus, Storage};
use jiff::Timestamp;

fn at(millis: i64) -> Timestamp {
    Timestamp::from_millisecond(millis).unwrap()
}

fn sample(name: &str) -> Artifact {
    Artifact {
        name: name.to_owned(),
        kind: ArtifactKind::Oci,
        source: "ghcr.io/stargrid-systems/spectra:0.2.0".to_owned(),
        digest: Some("sha256:abc".to_owned()),
        media_type: Some("application/vnd.spectra.tar+gzip".to_owned()),
        version: Some("0.2.0".to_owned()),
        size_bytes: Some(1234),
        status: ArtifactStatus::Present,
        downloaded_at: Some(at(1_700_000_000_000)),
        verified_at: None,
    }
}

#[tokio::test]
async fn upsert_get_list_roundtrip() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts();

    assert!(repo.list().await.unwrap().is_empty());

    let artifact = sample("spectra");
    repo.upsert(&artifact).await.unwrap();

    let fetched = repo
        .get("spectra")
        .await
        .unwrap()
        .expect("artifact present");
    assert_eq!(fetched, artifact);

    assert!(repo.get("missing").await.unwrap().is_none());
    assert_eq!(repo.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn upsert_replaces_existing() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts();

    repo.upsert(&sample("spectra")).await.unwrap();

    let mut updated = sample("spectra");
    updated.version = Some("0.3.0".to_owned());
    updated.status = ArtifactStatus::Downloading;
    repo.upsert(&updated).await.unwrap();

    let fetched = repo.get("spectra").await.unwrap().unwrap();
    assert_eq!(fetched.version.as_deref(), Some("0.3.0"));
    assert_eq!(fetched.status, ArtifactStatus::Downloading);
    assert_eq!(repo.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn records_download_history_newest_first() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts();

    let started = at(1_700_000_000_000);
    let source = "ghcr.io/stargrid-systems/spectra:0.2.0";

    let id1 = repo.start_download("spectra", source, started).await.unwrap();
    repo.finish_download(
        id1,
        DownloadStatus::Succeeded,
        started,
        Some("sha256:abc"),
        Some(10),
        None,
    )
    .await
    .unwrap();

    let id2 = repo.start_download("spectra", source, started).await.unwrap();
    repo.finish_download(
        id2,
        DownloadStatus::Failed,
        started,
        None,
        None,
        Some("connection reset"),
    )
    .await
    .unwrap();

    assert!(id2 > id1);

    let history = repo.downloads_for("spectra").await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, id2);
    assert_eq!(history[0].status, DownloadStatus::Failed);
    assert_eq!(history[0].error.as_deref(), Some("connection reset"));
    assert_eq!(history[1].id, id1);
    assert_eq!(history[1].status, DownloadStatus::Succeeded);

    assert!(repo.downloads_for("unknown").await.unwrap().is_empty());
}

#[tokio::test]
async fn lists_running_downloads() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts();

    let started = at(1_700_000_000_000);
    let running = repo.start_download("spectra", "src", started).await.unwrap();
    let done = repo.start_download("firmware", "src", started).await.unwrap();
    repo.finish_download(done, DownloadStatus::Succeeded, started, None, None, None)
        .await
        .unwrap();

    let still_running = repo.list_running().await.unwrap();
    assert_eq!(still_running.len(), 1);
    assert_eq!(still_running[0].id, running);
    assert_eq!(still_running[0].status, DownloadStatus::Running);
}

#[tokio::test]
async fn persists_and_migrations_are_idempotent() {
    let path = env::temp_dir().join("aperture-storage-reopen-test.db");
    let cleanup = || {
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    };
    cleanup();
    let path = path.to_str().unwrap();

    {
        let storage = Storage::open(path).await.unwrap();
        storage
            .artifacts()
            .upsert(&sample("spectra"))
            .await
            .unwrap();
    }
    {
        // Reopening re-runs migrations, which must be a no-op, and still sees data.
        let storage = Storage::open(path).await.unwrap();
        assert!(storage.artifacts().get("spectra").await.unwrap().is_some());
    }

    cleanup();
}
