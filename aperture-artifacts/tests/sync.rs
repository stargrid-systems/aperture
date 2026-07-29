use std::path::PathBuf;
use std::{env, fs, process};

use aperture_artifacts::{Artifact, ArtifactKey, Artifacts, ListQuery, Storage, VersionSort};
use aperture_storage::DbId;
use jiff::Timestamp;

fn temp_root(tag: &str) -> PathBuf {
    let root = env::temp_dir().join(format!("aperture-sync-{tag}-{}", process::id()));
    let _ = fs::remove_dir_all(&root);
    root
}

#[tokio::test]
async fn sync_removes_versions_without_blobs() {
    let root = temp_root("orphan-version");
    let storage = Storage::open(":memory:").await.unwrap();
    let artifacts = Artifacts::new(storage.clone(), root.clone());
    let repo = storage.artifacts().unwrap();

    // A catalog version whose blob never made it to disk.
    repo.record_version(&Artifact {
        id: DbId::from(0),
        key: ArtifactKey::new("spectra").unwrap(),
        source: "src".to_owned(),
        digest: "sha256:deadbeef".parse().unwrap(),
        media_type: None,
        version: None,
        size_bytes: 10,
        downloaded_at: Timestamp::from_microsecond(1_700_000_000_000).unwrap(),
        verified_at: None,
    })
    .await
    .unwrap();

    let report = artifacts.sync().await.unwrap();
    assert_eq!(report.removed_entries, 1);

    let versions = repo
        .list_versions(
            &ArtifactKey::new("spectra").unwrap(),
            VersionSort::DownloadedAt,
            None,
            None,
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert!(versions.items.is_empty());

    let _ = fs::remove_dir_all(&root);
}
