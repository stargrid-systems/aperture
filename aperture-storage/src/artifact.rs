//! Artifact catalog: the manager's record of downloaded components and the
//! history of fetch attempts.

use jiff::Timestamp;
use turso::{Connection, Row, Value, params_from_iter};

use crate::error::{Result, StorageError, database};
use crate::macros::sql;

/// What kind of component an artifact is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// An OCI image (spectra, firmware).
    Oci,
    /// A host tool binary (for example avrdude).
    Tool,
}

impl ArtifactKind {
    fn as_db(self) -> &'static str {
        match self {
            Self::Oci => "oci",
            Self::Tool => "tool",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "oci" => Ok(Self::Oci),
            "tool" => Ok(Self::Tool),
            other => Err(StorageError::Decode(format!(
                "unknown artifact kind {other:?}"
            ))),
        }
    }
}

/// Download and availability state of an artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactStatus {
    /// A download is in progress.
    Downloading,
    /// The artifact is present and usable.
    Present,
    /// The last download attempt failed.
    Failed,
}

impl ArtifactStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Downloading => "downloading",
            Self::Present => "present",
            Self::Failed => "failed",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "downloading" => Ok(Self::Downloading),
            "present" => Ok(Self::Present),
            "failed" => Ok(Self::Failed),
            other => Err(StorageError::Decode(format!(
                "unknown artifact status {other:?}"
            ))),
        }
    }
}

/// A catalog entry describing the current state of a managed component.
#[derive(Debug, Clone, PartialEq)]
pub struct Artifact {
    /// Logical name, for example `spectra` or `tool/avrdude`.
    pub name: String,
    /// What kind of component this is.
    pub kind: ArtifactKind,
    /// Where it came from (an image reference or a URL).
    pub source: String,
    /// Content digest of the stored blob, if known.
    pub digest: Option<String>,
    /// OCI media type, if applicable.
    pub media_type: Option<String>,
    /// Human-readable version, if known.
    pub version: Option<String>,
    /// Size of the stored blob in bytes.
    pub size_bytes: Option<i64>,
    /// Current availability state.
    pub status: ArtifactStatus,
    /// When the artifact was last downloaded.
    pub downloaded_at: Option<Timestamp>,
    /// When the artifact was last verified.
    pub verified_at: Option<Timestamp>,
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
    /// Name of the artifact this attempt is for.
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

    /// Inserts a catalog entry, replacing any existing entry with the same
    /// name.
    pub async fn upsert(&self, artifact: &Artifact) -> Result<()> {
        let params = params_from_iter([
            Value::Text(artifact.name.clone()),
            Value::Text(artifact.kind.as_db().to_owned()),
            Value::Text(artifact.source.clone()),
            text_or_null(&artifact.digest),
            text_or_null(&artifact.media_type),
            text_or_null(&artifact.version),
            int_or_null(artifact.size_bytes),
            Value::Text(artifact.status.as_db().to_owned()),
            ts_or_null(artifact.downloaded_at),
            ts_or_null(artifact.verified_at),
        ]);
        self.connection
            .execute(
                sql!(
                    INSERT OR REPLACE INTO artifacts
                    (name, kind, source, digest, media_type, version, size_bytes, status, downloaded_at, verified_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ),
                params,
            )
            .await
            .map_err(database)?;
        Ok(())
    }

    /// Removes the catalog entry for `name`. Does nothing if it is absent.
    pub async fn delete(&self, name: &str) -> Result<()> {
        self.connection
            .execute(
                sql!(DELETE FROM artifacts WHERE name = ?1),
                params_from_iter([Value::Text(name.to_owned())]),
            )
            .await
            .map_err(database)?;
        Ok(())
    }

    /// Returns the catalog entry for `name`, if present.
    pub async fn get(&self, name: &str) -> Result<Option<Artifact>> {
        let mut rows = self
            .connection
            .query(
                sql!(
                    SELECT name, kind, source, digest, media_type, version, size_bytes, status, downloaded_at, verified_at
                    FROM artifacts WHERE name = ?1
                ),
                params_from_iter([Value::Text(name.to_owned())]),
            )
            .await
            .map_err(database)?;
        match rows.next().await.map_err(database)? {
            Some(row) => Ok(Some(row_to_artifact(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists all catalog entries ordered by name.
    pub async fn list(&self) -> Result<Vec<Artifact>> {
        let mut rows = self
            .connection
            .query(
                sql!(
                    SELECT name, kind, source, digest, media_type, version, size_bytes, status, downloaded_at, verified_at
                    FROM artifacts ORDER BY name
                ),
                (),
            )
            .await
            .map_err(database)?;
        let mut artifacts = Vec::new();
        while let Some(row) = rows.next().await.map_err(database)? {
            artifacts.push(row_to_artifact(&row)?);
        }
        Ok(artifacts)
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
        let mut rows = self
            .connection
            .query(
                sql!(
                    SELECT id, artifact, started_at, finished_at, status, digest, size_bytes, source, error
                    FROM artifact_downloads WHERE status = ?1 ORDER BY id
                ),
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

    /// Lists the download history for `artifact`, newest first.
    pub async fn downloads_for(&self, artifact: &str) -> Result<Vec<Download>> {
        let mut rows = self
            .connection
            .query(
                sql!(
                    SELECT id, artifact, started_at, finished_at, status, digest, size_bytes, source, error
                    FROM artifact_downloads WHERE artifact = ?1 ORDER BY id DESC
                ),
                params_from_iter([Value::Text(artifact.to_owned())]),
            )
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
        name: req_text(row, 0)?,
        kind: ArtifactKind::from_db(&req_text(row, 1)?)?,
        source: req_text(row, 2)?,
        digest: opt_text(row, 3)?,
        media_type: opt_text(row, 4)?,
        version: opt_text(row, 5)?,
        size_bytes: opt_int(row, 6)?,
        status: ArtifactStatus::from_db(&req_text(row, 7)?)?,
        downloaded_at: opt_ts(row, 8)?,
        verified_at: opt_ts(row, 9)?,
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
