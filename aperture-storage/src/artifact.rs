//! Artifact catalog: the manager's record of stored component versions and the
//! history of fetch attempts.
//!
//! The catalog holds one row per stored version, identified by `(key, digest)`.
//! A key can have several versions, kept around for rollback. Rows are only
//! ever written once a version's blob is materialized, so every row is present
//! and usable.

use jiff::Timestamp;
use turso::{Connection, Row, Value, params_from_iter};

use crate::error::{Result, database};
use crate::id::DbId;
use crate::macros::sql;
use crate::page::{CursorValue, Filters, Keyset, ListQuery, Order, Page, Paginator};
use crate::row::{opt_text, opt_ts, req_int, req_text, req_ts, req_u64, text_or_null, ts_or_null};

/// Columns selected for an [`Artifact`], in [`row_to_artifact`] order.
const ARTIFACT_COLUMNS: &str =
    "id, key, source, digest, media_type, version, size_bytes, downloaded_at, verified_at";

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
            Value::Text(artifact.key.clone()),
            Value::Text(artifact.source.clone()),
            Value::Text(artifact.digest.clone()),
            text_or_null(&artifact.media_type),
            text_or_null(&artifact.version),
            Value::Integer(artifact.size_bytes as i64),
            Value::Integer(artifact.downloaded_at.as_millisecond()),
            ts_or_null(artifact.verified_at),
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
            .map_err(database)?;
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
            .query(&sql, params_from_iter([Value::Text(key.to_owned())]))
            .await
            .map_err(database)?;
        match rows.next().await.map_err(database)? {
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
            .query(
                &sql,
                params_from_iter([Value::Text(key.to_owned()), Value::Text(digest.to_owned())]),
            )
            .await
            .map_err(database)?;
        match rows.next().await.map_err(database)? {
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
            .query(&sql, params_from_iter([Value::Text(key.to_owned())]))
            .await
            .map_err(database)?;
        match rows.next().await.map_err(database)? {
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
        filters.like("a.key", q);
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
            .map_err(database)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
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
            VersionSort::DownloadedAt => "downloaded_at",
            VersionSort::SizeBytes => "size_bytes",
        };
        let keyset = Keyset::with_id(column, paginator.query_order());

        let mut filters = Filters::new();
        filters.eq_text("key", Some(key));
        filters.eq_text("media_type", media_type);
        filters.eq_text("version", version);
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
            .map_err(database)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            items.push(row_to_artifact(&row)?);
        }
        Ok(paginator.finish(items, |artifact| {
            let value = match sort {
                VersionSort::DownloadedAt => artifact.downloaded_at.as_millisecond(),
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
        let mut rows = self.connection.query(&sql, ()).await.map_err(database)?;
        let mut artifacts = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
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
                params_from_iter([Value::Text(key.to_owned()), Value::Text(digest.to_owned())]),
            )
            .await
            .map_err(database)?;
        Ok(())
    }
}

fn row_to_artifact(row: &Row) -> Result<Artifact> {
    Ok(Artifact {
        id: DbId::from(req_int(row, 0)?),
        key: req_text(row, 1)?,
        source: req_text(row, 2)?,
        digest: req_text(row, 3)?,
        media_type: opt_text(row, 4)?,
        version: opt_text(row, 5)?,
        size_bytes: req_u64(row, 6)?,
        downloaded_at: req_ts(row, 7)?,
        verified_at: opt_ts(row, 8)?,
    })
}

fn row_to_artifact_key(row: &Row) -> Result<ArtifactKey> {
    Ok(ArtifactKey {
        latest: row_to_artifact(row)?,
        version_count: req_u64(row, 9)?,
    })
}
