use std::path::PathBuf;
use std::{env, fs, process};

use aperture_artifacts::{
    Artifact, ArtifactKind, ArtifactStatus, Artifacts, DownloadStatus, Storage,
};
use jiff::Timestamp;

fn temp_root(tag: &str) -> PathBuf {
    let root = env::temp_dir().join(format!("aperture-sync-{tag}-{}", process::id()));
    let _ = fs::remove_dir_all(&root);
    root
}

#[tokio::test]
async fn sync_reconciles_interrupted_downloads() {
    let root = temp_root("interrupted");
    let storage = Storage::open(":memory:").await.unwrap();
    let artifacts = Artifacts::new(storage, root.clone());

    let started = Timestamp::from_millisecond(1_700_000_000_000).unwrap();
    let repo = artifacts.storage().artifacts();

    // A download that was running when the process stopped.
    let id = repo.start_download("spectra", "src", started).await.unwrap();
    // And the matching catalog entry left mid-download.
    repo.upsert(&Artifact {
        name: "spectra".to_owned(),
        kind: ArtifactKind::Oci,
        source: "src".to_owned(),
        digest: None,
        media_type: None,
        version: None,
        size_bytes: None,
        status: ArtifactStatus::Downloading,
        downloaded_at: None,
        verified_at: None,
    })
    .await
    .unwrap();

    artifacts.sync().await.unwrap();

    // The running attempt is now interrupted.
    assert!(repo.list_running().await.unwrap().is_empty());
    let history = repo.downloads_for("spectra").await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, id);
    assert_eq!(history[0].status, DownloadStatus::Interrupted);

    // The stale catalog entry is resolved to failed.
    let entry = repo.get("spectra").await.unwrap().unwrap();
    assert_eq!(entry.status, ArtifactStatus::Failed);

    let _ = fs::remove_dir_all(&root);
}
