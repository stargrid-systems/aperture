//! Artifact catalog: the manager's record of stored component versions and the
//! history of fetch attempts.
//!
//! The catalog holds one row per stored version, identified by `(key, digest)`.
//! A key can have several versions, kept around for rollback. Rows are only
//! ever written once a version's blob is materialized, so every row is present
//! and usable.

use jiff::Timestamp;
use turso::{Connection, Row, params_from_iter};

use crate::error::{Result, StorageError};
use crate::id::DbId;
use crate::macros::sql;
use crate::page::{CursorValue, Keyset, ListQuery, Order, Page, Paginator};
use crate::query::Filters;
use crate::sql::{Columns, ToSql, get};

mod col {
    pub const DIGEST: &str = "digest";
    pub const DOWNLOADED_AT: &str = "downloaded_at";
    pub const ID: &str = "id";
    pub const KEY: &str = "key";
    pub const MEDIA_TYPE: &str = "media_type";
    pub const SIZE_BYTES: &str = "size_bytes";
    pub const SOURCE: &str = "source";
    pub const VERIFIED_AT: &str = "verified_at";
    pub const VERSION: &str = "version";
}

/// Columns selected for an [`Artifact`], in [`row_to_artifact`] order.
const ARTIFACT_COLUMNS: Columns = Columns::new(&[
    col::ID,
    col::KEY,
    col::SOURCE,
    col::DIGEST,
    col::MEDIA_TYPE,
    col::VERSION,
    col::SIZE_BYTES,
    col::DOWNLOADED_AT,
    col::VERIFIED_AT,
]);

/// A stored version of an artifact. Every row maps to a materialized blob.
#[derive(Debug, Clone, PartialEq)]
pub struct Artifact {
    /// Store-assigned id. Ignored by [`ArtifactRepository::record_version`].
    pub id: DbId,
    /// Logical key, for example `spectra` or `tool/avrdude`.
    pub key: String,
    /// Where it came from (an image reference or a URL).
    pub source: String,
    /// Content digest of the stored blob.
    pub digest: String,
    /// OCI media type, if applicable.
    pub media_type: Option<String>,
    /// Human-readable version, if known.
    pub version: Option<String>,
    /// Size of the stored blob in bytes.
    pub size_bytes: u64,
    /// When this version was downloaded.
    pub downloaded_at: Timestamp,
    /// When this version was last verified.
    pub verified_at: Option<Timestamp>,
}

/// A distinct key with its newest version and how many versions are stored.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactKey {
    /// The newest stored version for this key.
    pub latest: Artifact,
    /// How many versions of this key are stored.
    pub version_count: u64,
}

/// Field a version listing is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSort {
    /// When the version was downloaded.
    DownloadedAt,
    /// Stored blob size.
    SizeBytes,
}

/// Repository over the artifact catalog.
pub struct ArtifactRepository {
    connection: Connection,
}

impl ArtifactRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Records a stored version. If `(key, digest)` already exists its metadata
    /// is refreshed. The `id` field of `artifact` is ignored.
    #[tracing::instrument(level = "info", skip(self, artifact))]
    pub async fn record_version(&self, artifact: &Artifact) -> Result<()> {
        let params = params_from_iter([
            artifact.key.to_sql(),
            artifact.source.to_sql(),
            artifact.digest.to_sql(),
            artifact.media_type.to_sql(),
            artifact.version.to_sql(),
            artifact.size_bytes.to_sql(),
            artifact.downloaded_at.to_sql(),
            artifact.verified_at.to_sql(),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO artifacts
                    (key, source, digest, media_type, version, size_bytes, downloaded_at, verified_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                    ON CONFLICT (key, digest) DO UPDATE SET
                        source = excluded.source,
                        media_type = excluded.media_type,
                        version = excluded.version,
                        size_bytes = excluded.size_bytes,
                        downloaded_at = excluded.downloaded_at,
                        verified_at = excluded.verified_at
                ),
                params,
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Returns the newest stored version of `key`, if any.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn latest(&self, key: &str) -> Result<Option<Artifact>> {
        let sql = format!(
            sql!(
                SELECT {cols} FROM artifacts WHERE key = ?1
                ORDER BY downloaded_at DESC, id DESC LIMIT 1
            ),
            cols = ARTIFACT_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([key.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(row_to_artifact(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns the `(key, digest)` version, if stored.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get_version(&self, key: &str, digest: &str) -> Result<Option<Artifact>> {
        let sql = format!(
            sql!(SELECT {cols} FROM artifacts WHERE key = ?1 AND digest = ?2),
            cols = ARTIFACT_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([key.to_sql(), digest.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(row_to_artifact(&row)?)),
            None => Ok(None),
        }
    }

    /// Returns `key` with its newest version and version count, if it exists.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn get_key(&self, key: &str) -> Result<Option<ArtifactKey>> {
        let sql = format!(
            sql!(
                SELECT {cols},
                (SELECT COUNT(*) FROM artifacts c WHERE c.key = a.key) AS version_count
                FROM artifacts a
                WHERE a.key = ?1 AND a.id = (SELECT b.id FROM artifacts b WHERE b.key = a.key
                    ORDER BY b.downloaded_at DESC, b.id DESC LIMIT 1)
            ),
            cols = ARTIFACT_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([key.to_sql()]))
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(Some(row_to_artifact_key(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists distinct keys, each with its newest version and version count.
    /// Ordered by key, ascending by default. `q` matches a substring of the
    /// key.
    #[tracing::instrument(level = "info", skip(self, query))]
    pub async fn list_keys(&self, q: Option<&str>, query: &ListQuery) -> Result<Page<ArtifactKey>> {
        let paginator = Paginator::new(query, Order::Asc)?;
        let keyset = Keyset::unique("a.key", paginator.query_order());

        let mut filters = Filters::new();
        // The latest-version predicate keeps one row per key.
        filters.raw(
            "a.id = (SELECT b.id FROM artifacts b WHERE b.key = a.key ORDER BY b.downloaded_at \
             DESC, b.id DESC LIMIT 1)",
        );
        filters.like_opt("a.key", q);
        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(
                SELECT {cols},
                (SELECT COUNT(*) FROM artifacts c WHERE c.key = a.key) AS version_count
                FROM artifacts a {where_clause} ORDER BY {order} LIMIT {limit}
            ),
            cols = ARTIFACT_COLUMNS,
            where_clause = filters.where_clause(),
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );

        let mut rows = self
            .connection
            .query(&sql, params_from_iter(filters.into_params()))
            .await
            .map_err(StorageError::from_turso)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            items.push(row_to_artifact_key(&row)?);
        }
        Ok(paginator.finish(items, |key| {
            (
                CursorValue::Text(key.latest.key.clone()),
                key.latest.id.get(),
            )
        }))
    }

    /// Lists the stored versions of `key`. Ordered by `sort`, descending by
    /// default. Optionally filtered by exact `media_type` and `version`.
    #[tracing::instrument(level = "info", skip(self, query))]
    pub async fn list_versions(
        &self,
        key: &str,
        sort: VersionSort,
        media_type: Option<&str>,
        version: Option<&str>,
        query: &ListQuery,
    ) -> Result<Page<Artifact>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let column = match sort {
            VersionSort::DownloadedAt => col::DOWNLOADED_AT,
            VersionSort::SizeBytes => col::SIZE_BYTES,
        };
        let keyset = Keyset::with_id(column, paginator.query_order());

        let mut filters = Filters::new();
        filters.eq_text(col::KEY, key);
        filters.eq_text_opt(col::MEDIA_TYPE, media_type);
        filters.eq_text_opt(col::VERSION, version);
        filters.keyset(&keyset, &paginator);

        let sql = format!(
            sql!(SELECT {cols} FROM artifacts {where_clause} ORDER BY {order} LIMIT {limit}),
            cols = ARTIFACT_COLUMNS,
            where_clause = filters.where_clause(),
            order = keyset.order_by(),
            limit = paginator.fetch_limit(),
        );

        let mut rows = self
            .connection
            .query(&sql, params_from_iter(filters.into_params()))
            .await
            .map_err(StorageError::from_turso)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            items.push(row_to_artifact(&row)?);
        }
        Ok(paginator.finish(items, |artifact| {
            let value = match sort {
                VersionSort::DownloadedAt => artifact.downloaded_at.as_microsecond(),
                VersionSort::SizeBytes => artifact.size_bytes as i64,
            };
            (CursorValue::Int(value), artifact.id.get())
        }))
    }

    /// Lists every stored version. For internal reconciliation, not paginated.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn all_versions(&self) -> Result<Vec<Artifact>> {
        let sql = format!(
            sql!(SELECT {cols} FROM artifacts ORDER BY id),
            cols = ARTIFACT_COLUMNS
        );
        let mut rows = self
            .connection
            .query(&sql, ())
            .await
            .map_err(StorageError::from_turso)?;
        let mut artifacts = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            artifacts.push(row_to_artifact(&row)?);
        }
        Ok(artifacts)
    }

    /// Removes the `(key, digest)` version. Does nothing if it is absent.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn delete_version(&self, key: &str, digest: &str) -> Result<()> {
        self.connection
            .execute(
                sql!(DELETE FROM artifacts WHERE key = ?1 AND digest = ?2),
                params_from_iter([key.to_sql(), digest.to_sql()]),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }
}

fn row_to_artifact(row: &Row) -> Result<Artifact> {
    Ok(Artifact {
        id: ARTIFACT_COLUMNS.extract(row, col::ID)?,
        key: ARTIFACT_COLUMNS.extract(row, col::KEY)?,
        source: ARTIFACT_COLUMNS.extract(row, col::SOURCE)?,
        digest: ARTIFACT_COLUMNS.extract(row, col::DIGEST)?,
        media_type: ARTIFACT_COLUMNS.extract(row, col::MEDIA_TYPE)?,
        version: ARTIFACT_COLUMNS.extract(row, col::VERSION)?,
        size_bytes: ARTIFACT_COLUMNS.extract(row, col::SIZE_BYTES)?,
        downloaded_at: ARTIFACT_COLUMNS.extract(row, col::DOWNLOADED_AT)?,
        verified_at: ARTIFACT_COLUMNS.extract(row, col::VERIFIED_AT)?,
    })
}

fn row_to_artifact_key(row: &Row) -> Result<ArtifactKey> {
    Ok(ArtifactKey {
        latest: row_to_artifact(row)?,
        version_count: get(row, ARTIFACT_COLUMNS.len())?,
    })
}
