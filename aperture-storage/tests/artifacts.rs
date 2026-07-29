use std::{env, fs};

use aperture_storage::{
    Artifact, ArtifactKey, DbId, Digest, ListQuery, MediaType, Storage, VersionSort,
};
use jiff::Timestamp;

fn at(micros: i64) -> Timestamp {
    Timestamp::from_microsecond(micros).unwrap()
}

fn key(s: &'static str) -> ArtifactKey {
    ArtifactKey::new(s).unwrap()
}

fn digest(s: &str) -> Digest {
    s.parse().unwrap()
}

fn mt(s: &str) -> MediaType {
    s.parse().unwrap()
}

fn version(key_str: &'static str, digest_str: &str, downloaded_at: i64) -> Artifact {
    Artifact {
        id: DbId::from(0),
        key: key(key_str),
        source: "ghcr.io/stargrid-systems/spectra:0.2.0".to_owned(),
        digest: digest(digest_str),
        media_type: Some(mt("application/vnd.spectra.tar+gzip")),
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

    assert!(repo.latest(&key("spectra")).await.unwrap().is_none());

    repo.record_version(&version("spectra", "sha256:aaaa", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:bbbb", 2_000))
        .await
        .unwrap();

    let latest = repo.latest(&key("spectra")).await.unwrap().unwrap();
    assert_eq!(latest.digest, digest("sha256:bbbb"));

    let specific = repo
        .get_version(&key("spectra"), &digest("sha256:aaaa"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(specific.digest, digest("sha256:aaaa"));

    assert!(
        repo.get_version(&key("spectra"), &digest("sha256:dead"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn record_version_is_idempotent_per_digest() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    repo.record_version(&version("spectra", "sha256:aaaa", 1_000))
        .await
        .unwrap();
    let mut again = version("spectra", "sha256:aaaa", 5_000);
    again.version = Some("0.3.0".to_owned());
    repo.record_version(&again).await.unwrap();

    let versions = repo
        .list_versions(
            &key("spectra"),
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

    repo.record_version(&version("spectra", "sha256:aaaa", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:bbbb", 2_000))
        .await
        .unwrap();
    repo.record_version(&version("firmware", "sha256:cccc", 1_500))
        .await
        .unwrap();

    let keys = repo.list_keys(None, &ListQuery::default()).await.unwrap();
    assert_eq!(keys.items.len(), 2);
    // Ordered by key ascending.
    assert_eq!(keys.items[0].latest.key, key("firmware"));
    assert_eq!(keys.items[0].version_count, 1);
    assert_eq!(keys.items[1].latest.key, key("spectra"));
    assert_eq!(keys.items[1].version_count, 2);
    assert_eq!(keys.items[1].latest.digest, digest("sha256:bbbb"));
}

#[tokio::test]
async fn list_keys_paginates_with_cursor() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    for key in ["a", "b", "c"] {
        repo.record_version(&version(key, &format!("sha256:{key}{key}"), 1_000))
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

    repo.record_version(&version("a_b", "sha256:11", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("axb", "sha256:22", 1_000))
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
        ("sha256:aa", 1_000),
        ("sha256:bb", 3_000),
        ("sha256:cc", 2_000),
    ] {
        repo.record_version(&version("spectra", digest, ts))
            .await
            .unwrap();
    }

    // Default order is downloaded_at descending.
    let first = repo
        .list_versions(
            &key("spectra"),
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
            .map(|v| v.digest.to_string())
            .collect::<Vec<_>>(),
        ["sha256:bb", "sha256:cc"]
    );
    let cursor = first.next_cursor.expect("more pages");

    let second = repo
        .list_versions(
            &key("spectra"),
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
            .map(|v| v.digest.to_string())
            .collect::<Vec<_>>(),
        ["sha256:aa"]
    );
}

#[tokio::test]
async fn delete_version_removes_only_that_version() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.artifacts().unwrap();

    repo.record_version(&version("spectra", "sha256:aaaa", 1_000))
        .await
        .unwrap();
    repo.record_version(&version("spectra", "sha256:bbbb", 2_000))
        .await
        .unwrap();

    repo.delete_version(&key("spectra"), &digest("sha256:aaaa"))
        .await
        .unwrap();

    let versions = repo
        .list_versions(
            &key("spectra"),
            VersionSort::DownloadedAt,
            None,
            None,
            &ListQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(versions.items.len(), 1);
    assert_eq!(versions.items[0].digest, digest("sha256:bbbb"));
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
            .record_version(&version("spectra", "sha256:aaaa", 1_000))
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
                .latest(&key("spectra"))
                .await
                .unwrap()
                .is_some()
        );
    }

    cleanup();
}
