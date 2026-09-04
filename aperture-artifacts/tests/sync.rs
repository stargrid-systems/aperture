use std::path::PathBuf;
use std::{env, fs, process};

use aperture_artifacts::{
    Artifact, ArtifactKey, ArtifactOrphanRemoved, ArtifactRemoved, Artifacts, ListQuery, Storage,
    VersionSort,
};
use aperture_events::{Delivery, EventBus};
use aperture_storage::{ActorId, ArtifactId};
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
    let event_bus = EventBus::new();
    let artifacts = Artifacts::new(storage.clone(), root.clone(), event_bus.clone());
    let repo = storage.artifacts().unwrap();
    let mut rx = event_bus.subscribe_typed::<ArtifactRemoved>();

    // A catalog version whose blob never made it to disk.
    repo.record_version(&Artifact {
        id: ArtifactId::from(0),
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

    let event = match rx.recv().await.expect("sync emits a removal event") {
        Delivery::Event(event) => event,
        Delivery::Lagged(n) => panic!("unexpected lag report: {n}"),
    };
    assert_eq!(event.payload.key, "spectra");
    assert_eq!(
        event.actor,
        ActorId::SYSTEM,
        "sync removals are system-initiated",
    );

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn sync_removes_blobs_without_entries() {
    let root = temp_root("orphan-blob");
    let storage = Storage::open(":memory:").await.unwrap();
    let event_bus = EventBus::new();
    let artifacts = Artifacts::new(storage, root.clone(), event_bus.clone());
    let mut rx = event_bus.subscribe_typed::<ArtifactOrphanRemoved>();

    // A blob no catalog entry references.
    let blob = root.join("blobs").join("sha256").join("cafe");
    fs::create_dir_all(blob.parent().unwrap()).unwrap();
    fs::write(&blob, b"orphan").unwrap();

    let report = artifacts.sync().await.unwrap();
    assert_eq!(report.removed_blobs, 1);
    assert!(!blob.exists());

    let event = match rx.recv().await.expect("sync emits a removal event") {
        Delivery::Event(event) => event,
        Delivery::Lagged(n) => panic!("unexpected lag report: {n}"),
    };
    // An orphan blob has no key, so the digest identifies it, on its own
    // event kind instead of polluting ArtifactRemoved's key field.
    assert_eq!(event.payload.digest, "sha256:cafe");
    assert_eq!(event.actor, ActorId::SYSTEM);

    let _ = fs::remove_dir_all(&root);
}
