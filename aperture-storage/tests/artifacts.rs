use std::{env, fs};

use aperture_storage::{Artifact, DownloadStatus, ListQuery, Order, Storage, VersionSort};
use jiff::Timestamp;

fn at(millis: i64) -> Timestamp {
    Timestamp::from_millisecond(millis).unwrap()
}

fn version(key: &str, digest: &str, downloaded_at: i64) -> Artifact {
    Artifact {
        id: 0,
        key: key.to_owned(),
        source: "ghcr.io/stargrid-systems/spectra:0.2.0".to_owned(),
        digest: digest.to_owned(),
        media_type: Some("application/vnd.spectra.tar+gzip".to_owned()),
        version: Some("0.2.0".to_owned()),
        size_bytes: 1234,
        downloaded_at: at(downloaded_at),
        verified_at: None,
    }
}

#[tokio::test]
async fn record_latest_and_get_version() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    assert!(repo.latest("spectra").await.unwrap().is_none());

    repo.record_version(&version("spectra", "sha256:aaa", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:bbb", 2_000))
        .await
        .unwrap();

    let latest = repo.latest("spectra").await.unwrap().unwrap();
    assert_eq!(latest.digest, "sha256:bbb");

    let specific = repo
        .get_version("spectra", "sha256:aaa")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(specific.digest, "sha256:aaa");

    assert!(
        repo.get_version("spectra", "missing")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn record_version_is_idempotent_per_digest() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    repo.record_version(&version("spectra", "sha256:aaa", 1_000))
        .await
        .unwrap();
    let mut again = version("spectra", "sha256:aaa", 5_000);
    again.version = Some("0.3.0".to_owned());
    repo.record_version(&again).await.unwrap();

    let versions = repo
        .list_versions(
            "spectra",
            VersionSort::DownloadedAt,
            None,
            None,
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(versions.items.len(), 1);
    assert_eq!(versions.items[0].version.as_deref(), Some("0.3.0"));
    assert_eq!(versions.items[0].downloaded_at, at(5_000));
}

#[tokio::test]
async fn list_keys_returns_latest_and_count() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    repo.record_version(&version("spectra", "sha256:aaa", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:bbb", 2_000))
        .await
        .unwrap();
    repo.record_version(&version("firmware", "sha256:ccc", 1_500))
        .await
        .unwrap();

    let keys = repo.list_keys(None, &ListQuery::default()).await.unwrap();
    assert_eq!(keys.items.len(), 2);
    // Ordered by key ascending.
    assert_eq!(keys.items[0].latest.key, "firmware");
    assert_eq!(keys.items[0].version_count, 1);
    assert_eq!(keys.items[1].latest.key, "spectra");
    assert_eq!(keys.items[1].version_count, 2);
    assert_eq!(keys.items[1].latest.digest, "sha256:bbb");
}

#[tokio::test]
async fn list_keys_paginates_with_cursor() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    for key in ["a", "b", "c"] {
        repo.record_version(&version(key, &format!("sha256:{key}"), 1_000))
            .await
            .unwrap();
    }

    let first = repo
        .list_keys(
            None,
            &ListQuery {
                limit: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        first
            .items
            .iter()
            .map(|k| k.latest.key.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    let cursor = first.next_cursor.expect("more pages");

    let second = repo
        .list_keys(
            None,
            &ListQuery {
                limit: Some(2),
                cursor: Some(cursor),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        second
            .items
            .iter()
            .map(|k| k.latest.key.as_str())
            .collect::<Vec<_>>(),
        ["c"]
    );
    assert!(second.next_cursor.is_none());

    // Page back from the second page using its prev cursor.
    let back = repo
        .list_keys(
            None,
            &ListQuery {
                limit: Some(2),
                cursor: Some(second.prev_cursor.expect("a previous page")),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        back.items
            .iter()
            .map(|k| k.latest.key.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(back.next_cursor.is_some());
    assert!(back.prev_cursor.is_none());
}

#[tokio::test]
async fn list_keys_q_treats_wildcards_literally() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    repo.record_version(&version("a_b", "sha256:1", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("axb", "sha256:2", 1_000))
        .await
        .unwrap();

    // `_` must match literally, not as a single-char wildcard.
    let hits = repo
        .list_keys(Some("a_b"), &ListQuery::default())
        .await
        .unwrap();
    assert_eq!(
        hits.items
            .iter()
            .map(|k| k.latest.key.as_str())
            .collect::<Vec<_>>(),
        ["a_b"]
    );
}

#[tokio::test]
async fn list_versions_sorts_and_paginates() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    for (digest, ts) in [
        ("sha256:a", 1_000),
        ("sha256:b", 3_000),
        ("sha256:c", 2_000),
    ] {
        repo.record_version(&version("spectra", digest, ts))
            .await
            .unwrap();
    }

    // Default order is downloaded_at descending.
    let first = repo
        .list_versions(
            "spectra",
            VersionSort::DownloadedAt,
            None,
            None,
            &ListQuery {
                limit: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        first
            .items
            .iter()
            .map(|v| v.digest.as_str())
            .collect::<Vec<_>>(),
        ["sha256:b", "sha256:c"]
    );
    let cursor = first.next_cursor.expect("more pages");

    let second = repo
        .list_versions(
            "spectra",
            VersionSort::DownloadedAt,
            None,
            None,
            &ListQuery {
                limit: Some(2),
                cursor: Some(cursor),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        second
            .items
            .iter()
            .map(|v| v.digest.as_str())
            .collect::<Vec<_>>(),
        ["sha256:a"]
    );
}

#[tokio::test]
async fn delete_version_removes_only_that_version() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    repo.record_version(&version("spectra", "sha256:aaa", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:bbb", 2_000))
        .await
        .unwrap();

    repo.delete_version("spectra", "sha256:aaa").await.unwrap();

    let versions = repo
        .list_versions(
            "spectra",
            VersionSort::DownloadedAt,
            None,
            None,
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(versions.items.len(), 1);
    assert_eq!(versions.items[0].digest, "sha256:bbb");
}

#[tokio::test]
async fn lists_downloads_with_filters_and_history() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    let started = at(1_700_000_000_000);
    let source = "ghcr.io/stargrid-systems/spectra:0.2.0";

    let id1 = repo
        .start_download("spectra", source, started)
        .await
        .unwrap();
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

    let id2 = repo
        .start_download("spectra", source, started)
        .await
        .unwrap();
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

    // Newest first across all downloads.
    let all = repo
        .list_downloads(None, None, &ListQuery::default())
        .await
        .unwrap();
    assert_eq!(all.items.len(), 2);
    assert_eq!(all.items[0].id, id2);
    assert_eq!(all.items[0].status, DownloadStatus::Failed);

    // Filter by status.
    let failed = repo
        .list_downloads(Some(DownloadStatus::Failed), None, &ListQuery::default())
        .await
        .unwrap();
    assert_eq!(failed.items.len(), 1);
    assert_eq!(failed.items[0].id, id2);

    // Oldest first with explicit order.
    let oldest = repo
        .list_downloads(
            None,
            Some("spectra"),
            &ListQuery {
                order: Some(Order::Asc),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(oldest.items[0].id, id1);

    let history = repo.downloads_for("spectra").await.unwrap();
    assert_eq!(history.len(), 2);
    assert!(repo.downloads_for("unknown").await.unwrap().is_empty());
}

#[tokio::test]
async fn lists_running_downloads() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    let started = at(1_700_000_000_000);
    let running = repo
        .start_download("spectra", "src", started)
        .await
        .unwrap();
    let done = repo
        .start_download("firmware", "src", started)
        .await
        .unwrap();
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
            .unwrap()
            .record_version(&version("spectra", "sha256:aaa", 1_000))
            .await
            .unwrap();
    }
    {
        // Reopening re-runs migrations, which must be a no-op, and still sees data.
        let storage = Storage::open(path).await.unwrap();
        assert!(
            storage
                .artifacts()
                .unwrap()
                .latest("spectra")
                .await
                .unwrap()
                .is_some()
        );
    }

    cleanup();
}
