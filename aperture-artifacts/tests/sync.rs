use std::path::PathBuf;
use std::{env, fs, process};

use aperture_artifacts::{Artifact, Artifacts, ListQuery, Storage, VersionSort};
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
    let artifacts = Artifacts::new(storage, root.clone());
    let repo = artifacts.storage().artifacts();

    // A catalog version whose blob never made it to disk.
    repo.record_version(&Artifact {
        id: 0,
        key: "spectra".to_owned(),
        source: "src".to_owned(),
        digest: "sha256:deadbeef".to_owned(),
        media_type: None,
        version: None,
        size_bytes: 10,
        downloaded_at: Timestamp::from_millisecond(1_700_000_000_000).unwrap(),
        verified_at: None,
    })
    .await
    .unwrap();

    let report = artifacts.sync().await.unwrap();
    assert_eq!(report.removed_entries, 1);

    let versions = repo
        .list_versions("spectra", VersionSort::DownloadedAt, None, None, &ListQuery::default())
        .await
        .unwrap();
    assert!(versions.items.is_empty());

    let _ = fs::remove_dir_all(&root);
}
