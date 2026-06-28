//! Artifact catalog: the manager's record of stored component versions and the
//! history of fetch attempts.
//!
//! The catalog holds one row per stored version, identified by `(key, digest)`.
//! A key can have several versions, kept around for rollback. Rows are only ever
//! written once a version's blob is materialized, so every row is present and
//! usable. In-flight and failed attempts live in `artifact_downloads`.

use jiff::Timestamp;
use turso::{Connection, Row, Value, params_from_iter};

use crate::error::{Result, StorageError, database};
use crate::macros::sql;
use crate::page::{CursorValue, Filters, Keyset, ListQuery, Order, Page, Paginator};

/// Columns selected for an [`Artifact`], in [`row_to_artifact`] order.
const ARTIFACT_COLUMNS: &str =
    "id, key, source, digest, media_type, version, size_bytes, downloaded_at, verified_at";

/// Columns selected for a [`Download`], in [`row_to_download`] order.
const DOWNLOAD_COLUMNS: &str =
    "id, artifact, started_at, finished_at, status, digest, size_bytes, source, error";

/// A stored version of an artifact. Every row maps to a materialized blob.
#[derive(Debug, Clone, PartialEq)]
pub struct Artifact {
    /// Store-assigned id. Ignored by [`ArtifactRepository::record_version`].
    pub id: i64,
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
    pub size_bytes: i64,
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
    pub version_count: i64,
}

/// Field a version listing is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSort {
    /// When the version was downloaded.
    DownloadedAt,
    /// Stored blob size.
    SizeBytes,
}

/// Lifecycle state of a single download attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    /// The attempt is in progress.
    Running,
    /// The attempt completed successfully.
    Succeeded,
    /// The attempt failed.
    Failed,
    /// The attempt was still running when the process stopped.
    Interrupted,
}

impl DownloadStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            other => Err(StorageError::Decode(format!(
                "unknown download status {other:?}"
            ))),
        }
    }
}

/// A recorded download attempt, including its assigned id.
#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    /// Store-assigned id.
    pub id: i64,
    /// Key of the artifact this attempt is for.
    pub artifact: String,
    /// When the attempt started.
    pub started_at: Timestamp,
    /// When the attempt finished, if it did.
    pub finished_at: Option<Timestamp>,
    /// The lifecycle state.
    pub status: DownloadStatus,
    /// Resolved content digest, if the attempt got that far.
    pub digest: Option<String>,
    /// Number of bytes transferred.
    pub size_bytes: Option<i64>,
    /// Where the attempt fetched from.
    pub source: String,
    /// Failure detail, if any.
    pub error: Option<String>,
}

/// Repository over the artifact catalog and its download history.
pub struct ArtifactRepository {
    connection: Connection,
}

impl ArtifactRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Records a stored version. If `(key, digest)` already exists its metadata
    /// is refreshed. The `id` field of `artifact` is ignored.
    pub async fn record_version(&self, artifact: &Artifact) -> Result<()> {
        let params = params_from_iter([
            Value::Text(artifact.key.clone()),
            Value::Text(artifact.source.clone()),
            Value::Text(artifact.digest.clone()),
            text_or_null(&artifact.media_type),
            text_or_null(&artifact.version),
            Value::Integer(artifact.size_bytes),
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
    pub async fn latest(&self, key: &str) -> Result<Option<Artifact>> {
        let sql = sql!(
            "SELECT {cols} FROM artifacts WHERE key = ?1 \
             ORDER BY downloaded_at DESC, id DESC LIMIT 1",
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
    pub async fn get_version(&self, key: &str, digest: &str) -> Result<Option<Artifact>> {
        let sql = sql!(
            "SELECT {cols} FROM artifacts WHERE key = ?1 AND digest = ?2",
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
    pub async fn get_key(&self, key: &str) -> Result<Option<ArtifactKey>> {
        let sql = sql!(
            "SELECT {cols}, \
             (SELECT COUNT(*) FROM artifacts c WHERE c.key = a.key) AS version_count \
             FROM artifacts a \
             WHERE a.key = ?1 AND a.id = (SELECT b.id FROM artifacts b WHERE b.key = a.key \
                ORDER BY b.downloaded_at DESC, b.id DESC LIMIT 1)",
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
    /// Ordered by key, ascending by default. `q` matches a substring of the key.
    pub async fn list_keys(&self, q: Option<&str>, query: &ListQuery) -> Result<Page<ArtifactKey>> {
        let paginator = Paginator::new(query, Order::Asc)?;
        let keyset = Keyset::unique("a.key", paginator.query_order());

        let mut filters = Filters::new();
        // The latest-version predicate keeps one row per key.
        filters.raw(
            "a.id = (SELECT b.id FROM artifacts b WHERE b.key = a.key \
             ORDER BY b.downloaded_at DESC, b.id DESC LIMIT 1)",
        );
        filters.like("a.key", q);
        filters.keyset(&keyset, &paginator);

        let sql = sql!(
            "SELECT {cols}, \
             (SELECT COUNT(*) FROM artifacts c WHERE c.key = a.key) AS version_count \
             FROM artifacts a {where_clause} ORDER BY {order} LIMIT {limit}",
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
            (CursorValue::Text(key.latest.key.clone()), key.latest.id)
        }))
    }

    /// Lists the stored versions of `key`. Ordered by `sort`, descending by
    /// default. Optionally filtered by exact `media_type` and `version`.
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

        let sql = sql!(
            "SELECT {cols} FROM artifacts {where_clause} ORDER BY {order} LIMIT {limit}",
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
                VersionSort::SizeBytes => artifact.size_bytes,
            };
            (CursorValue::Int(value), artifact.id)
        }))
    }

    /// Lists every stored version. For internal reconciliation, not paginated.
    pub async fn all_versions(&self) -> Result<Vec<Artifact>> {
        let sql = sql!("SELECT {cols} FROM artifacts ORDER BY id", cols = ARTIFACT_COLUMNS);
        let mut rows = self.connection.query(&sql, ()).await.map_err(database)?;
        let mut artifacts = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            artifacts.push(row_to_artifact(&row)?);
        }
        Ok(artifacts)
    }

    /// Removes the `(key, digest)` version. Does nothing if it is absent.
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

    /// Records the start of a download attempt and returns its assigned id.
    /// The row begins in the [`DownloadStatus::Running`] state.
    pub async fn start_download(
        &self,
        artifact: &str,
        source: &str,
        started_at: Timestamp,
    ) -> Result<i64> {
        let params = params_from_iter([
            Value::Text(artifact.to_owned()),
            Value::Integer(started_at.as_millisecond()),
            Value::Text(DownloadStatus::Running.as_db().to_owned()),
            Value::Text(source.to_owned()),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT INTO artifact_downloads (artifact, started_at, status, source)
                    VALUES (?1, ?2, ?3, ?4)
                ),
                params,
            )
            .await
            .map_err(database)?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Records the outcome of the download attempt with `id`.
    pub async fn finish_download(
        &self,
        id: i64,
        status: DownloadStatus,
        finished_at: Timestamp,
        digest: Option<&str>,
        size_bytes: Option<i64>,
        error: Option<&str>,
    ) -> Result<()> {
        let params = params_from_iter([
            Value::Text(status.as_db().to_owned()),
            Value::Integer(finished_at.as_millisecond()),
            text_ref_or_null(digest),
            int_or_null(size_bytes),
            text_ref_or_null(error),
            Value::Integer(id),
        ]);
        self.connection
            .execute(
                sql!(
                    UPDATE artifact_downloads
                    SET status = ?1, finished_at = ?2, digest = ?3, size_bytes = ?4, error = ?5
                    WHERE id = ?6
                ),
                params,
            )
            .await
            .map_err(database)?;
        Ok(())
    }

    /// Lists attempts still in the [`DownloadStatus::Running`] state. After a
    /// clean start these are leftovers from a process that stopped mid-download.
    pub async fn list_running(&self) -> Result<Vec<Download>> {
        let sql = sql!(
            "SELECT {cols} FROM artifact_downloads WHERE status = ?1 ORDER BY id",
            cols = DOWNLOAD_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                params_from_iter([Value::Text(DownloadStatus::Running.as_db().to_owned())]),
            )
            .await
            .map_err(database)?;
        let mut downloads = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            downloads.push(row_to_download(&row)?);
        }
        Ok(downloads)
    }

    /// Lists download attempts, newest first, optionally filtered by `status`
    /// and artifact `key`.
    pub async fn list_downloads(
        &self,
        status: Option<DownloadStatus>,
        key: Option<&str>,
        query: &ListQuery,
    ) -> Result<Page<Download>> {
        let paginator = Paginator::new(query, Order::Desc)?;
        let keyset = Keyset::unique("id", paginator.query_order());

        let mut filters = Filters::new();
        filters.eq_text("status", status.map(DownloadStatus::as_db));
        filters.eq_text("artifact", key);
        filters.keyset(&keyset, &paginator);

        let sql = sql!(
            "SELECT {cols} FROM artifact_downloads {where_clause} ORDER BY {order} LIMIT {limit}",
            cols = DOWNLOAD_COLUMNS,
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
            items.push(row_to_download(&row)?);
        }
        Ok(paginator.finish(items, |download| {
            (CursorValue::Int(download.id), download.id)
        }))
    }

    /// Lists the download history for `artifact`, newest first.
    pub async fn downloads_for(&self, artifact: &str) -> Result<Vec<Download>> {
        let sql = sql!(
            "SELECT {cols} FROM artifact_downloads WHERE artifact = ?1 ORDER BY id DESC",
            cols = DOWNLOAD_COLUMNS,
        );
        let mut rows = self
            .connection
            .query(&sql, params_from_iter([Value::Text(artifact.to_owned())]))
            .await
            .map_err(database)?;
        let mut downloads = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            downloads.push(row_to_download(&row)?);
        }
        Ok(downloads)
    }
}

fn text_or_null(value: &Option<String>) -> Value {
    match value {
        Some(text) => Value::Text(text.clone()),
        None => Value::Null,
    }
}

fn text_ref_or_null(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::Text(text.to_owned()),
        None => Value::Null,
    }
}

fn int_or_null(value: Option<i64>) -> Value {
    match value {
        Some(int) => Value::Integer(int),
        None => Value::Null,
    }
}

fn ts_or_null(value: Option<Timestamp>) -> Value {
    match value {
        Some(timestamp) => Value::Integer(timestamp.as_millisecond()),
        None => Value::Null,
    }
}

fn req_text(row: &Row, idx: usize) -> Result<String> {
    match row.get_value(idx).map_err(database)? {
        Value::Text(text) => Ok(text),
        other => Err(StorageError::Decode(format!(
            "expected text at column {idx}, found {other:?}"
        ))),
    }
}

fn opt_text(row: &Row, idx: usize) -> Result<Option<String>> {
    match row.get_value(idx).map_err(database)? {
        Value::Null => Ok(None),
        Value::Text(text) => Ok(Some(text)),
        other => Err(StorageError::Decode(format!(
            "expected text or null at column {idx}, found {other:?}"
        ))),
    }
}

fn req_int(row: &Row, idx: usize) -> Result<i64> {
    match row.get_value(idx).map_err(database)? {
        Value::Integer(int) => Ok(int),
        other => Err(StorageError::Decode(format!(
            "expected integer at column {idx}, found {other:?}"
        ))),
    }
}

fn opt_int(row: &Row, idx: usize) -> Result<Option<i64>> {
    match row.get_value(idx).map_err(database)? {
        Value::Null => Ok(None),
        Value::Integer(int) => Ok(Some(int)),
        other => Err(StorageError::Decode(format!(
            "expected integer or null at column {idx}, found {other:?}"
        ))),
    }
}

fn req_ts(row: &Row, idx: usize) -> Result<Timestamp> {
    ts_from_millis(req_int(row, idx)?)
}

fn opt_ts(row: &Row, idx: usize) -> Result<Option<Timestamp>> {
    match opt_int(row, idx)? {
        Some(millis) => Ok(Some(ts_from_millis(millis)?)),
        None => Ok(None),
    }
}

fn ts_from_millis(millis: i64) -> Result<Timestamp> {
    Timestamp::from_millisecond(millis)
        .map_err(|err| StorageError::Decode(format!("invalid timestamp {millis}: {err}")))
}

fn row_to_artifact(row: &Row) -> Result<Artifact> {
    Ok(Artifact {
        id: req_int(row, 0)?,
        key: req_text(row, 1)?,
        source: req_text(row, 2)?,
        digest: req_text(row, 3)?,
        media_type: opt_text(row, 4)?,
        version: opt_text(row, 5)?,
        size_bytes: req_int(row, 6)?,
        downloaded_at: req_ts(row, 7)?,
        verified_at: opt_ts(row, 8)?,
    })
}

fn row_to_artifact_key(row: &Row) -> Result<ArtifactKey> {
    Ok(ArtifactKey {
        latest: row_to_artifact(row)?,
        version_count: req_int(row, 9)?,
    })
}

fn row_to_download(row: &Row) -> Result<Download> {
    Ok(Download {
        id: req_int(row, 0)?,
        artifact: req_text(row, 1)?,
        started_at: req_ts(row, 2)?,
        finished_at: opt_ts(row, 3)?,
        status: DownloadStatus::from_db(&req_text(row, 4)?)?,
        digest: opt_text(row, 5)?,
        size_bytes: opt_int(row, 6)?,
        source: req_text(row, 7)?,
        error: opt_text(row, 8)?,
    })
}
