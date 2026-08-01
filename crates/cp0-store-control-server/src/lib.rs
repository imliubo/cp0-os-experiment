use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path as FilePath, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::rejection::JsonRejection;
use axum::extract::rejection::QueryRejection;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, ETAG, IF_MATCH, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use cp0_store_control::{
    AppRecord, SubmissionSpec, create_submission_request_sha256, is_valid_locale,
    register_app_request_sha256, validate_submission_spec,
};
use cp0_store_metadata::{ImageAsset, ReleaseState, StoreCategory, SubmissionState};
use cp0_store_metrics::{
    AggregateMetricsReport, MAX_METRICS_REPORT_BYTES, WEEK_SECONDS, encode_report, week_start,
};
use cp0_store_protocol::CatalogApp;
use cp0_store_risk::RiskAssessment;
use cp0_store_scan::{ScanDisposition, ScanFinding, ScanReport};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Row, Transaction};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

mod moderation;

pub const MAX_REQUEST_BYTES: usize = 32 * 1024;
pub const MAX_UPLOAD_CHUNK_BYTES: usize = 256 * 1024;
const IDEMPOTENCY_TTL_SECONDS: i64 = 24 * 60 * 60;
const MAX_TRANSACTION_ATTEMPTS: usize = 4;
const DEVICE_CODE_TTL_SECONDS: i64 = 10 * 60;
const DEVICE_POLL_INTERVAL_SECONDS: i16 = 5;
const ACCESS_TOKEN_TTL_SECONDS: i64 = 15 * 60;
const MAX_ACTIVE_DEVICE_AUTHORIZATIONS: i64 = 10_000;
const MFA_STEP_UP_MAX_AGE_SECONDS: i64 = 5 * 60;
const METRICS_BATCH_TTL_SECONDS: i64 = 15 * 24 * 60 * 60;
const MAX_TEAM_MEMBERS: usize = 100;
const MAX_REVIEW_DETAIL_MESSAGES: usize = 6;
const MAX_REVIEW_DETAIL_AUDIT_EVENTS: usize = 32;
const MAX_REVIEW_DETAIL_ASSIGNMENTS: usize = 8;
const MAX_REVIEW_DETAIL_DECISIONS: usize = 8;
const DEVICE_VERIFICATION_URI: &str = "https://developer.cardputerzero.dev/activate";
const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEVICE_CLIENT_ID: &str = "cp0ctl";
const DEVICE_SCOPE: &str = "store.submit";
const OBJECT_GC_LOCK_DOMAIN: &str = "cp0.store-object-gc.v1";
const MAX_GC_FILES: usize = 1_000_000;
const GC_REFERENCE_BATCH_SIZE: usize = 512;
pub const DEFAULT_OBJECT_GC_MINIMUM_AGE_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug)]
struct ContentObjectStore {
    chunks: Arc<PathBuf>,
    temporary: Arc<PathBuf>,
}

impl ContentObjectStore {
    async fn open(root: &FilePath) -> Result<Self, io::Error> {
        if !root.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Store object root must be absolute",
            ));
        }
        tokio::fs::create_dir_all(root).await?;
        restrict_directory(root).await?;
        let root = checked_directory(root).await?;
        let chunks = root.join("chunks");
        let temporary = root.join("temporary");
        tokio::fs::create_dir_all(&chunks).await?;
        tokio::fs::create_dir_all(&temporary).await?;
        restrict_directory(&chunks).await?;
        restrict_directory(&temporary).await?;
        let chunks = checked_directory(&chunks).await?;
        let temporary = checked_directory(&temporary).await?;
        if !chunks.starts_with(&root) || !temporary.starts_with(&root) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Store object directories escape the configured root",
            ));
        }
        Ok(Self {
            chunks: Arc::new(chunks),
            temporary: Arc::new(temporary),
        })
    }

    async fn open_existing(root: &FilePath) -> Result<Self, io::Error> {
        let root = checked_private_directory(root).await?;
        let chunks = checked_private_directory(&root.join("chunks")).await?;
        let temporary = checked_private_directory(&root.join("temporary")).await?;
        if !chunks.starts_with(&root) || !temporary.starts_with(&root) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Store object directories escape the configured root",
            ));
        }
        Ok(Self {
            chunks: Arc::new(chunks),
            temporary: Arc::new(temporary),
        })
    }

    async fn store_chunk(&self, sha256: &str, bytes: &[u8]) -> Result<(), io::Error> {
        if !is_valid_sha256(sha256) || sha256_hex(bytes) != sha256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk digest mismatch",
            ));
        }
        let directory = self.chunks.join(&sha256[..2]);
        tokio::fs::create_dir_all(&directory).await?;
        let directory = checked_directory(&directory).await?;
        if !directory.starts_with(self.chunks.as_ref()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "chunk directory escapes the object root",
            ));
        }
        let destination = directory.join(format!("{sha256}.chunk"));
        if tokio::fs::symlink_metadata(&destination).await.is_ok() {
            self.verify_chunk(sha256, bytes.len()).await?;
            return Ok(());
        }

        let temporary = self
            .temporary
            .join(format!("{}.part", Uuid::new_v4().simple()));
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);

        match tokio::fs::hard_link(&temporary, &destination).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(error);
            }
        }
        tokio::fs::remove_file(&temporary).await?;
        self.verify_chunk(sha256, bytes.len()).await.map(|_| ())
    }

    async fn verify_chunk(
        &self,
        sha256: &str,
        expected_bytes: usize,
    ) -> Result<Vec<u8>, io::Error> {
        if !is_valid_sha256(sha256) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stored chunk identifier is invalid",
            ));
        }
        let path = self
            .chunks
            .join(&sha256[..2])
            .join(format!("{sha256}.chunk"));
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() != expected_bytes as u64
            || expected_bytes > MAX_UPLOAD_CHUNK_BYTES
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stored chunk metadata is invalid",
            ));
        }
        let bytes = tokio::fs::read(path).await?;
        if sha256_hex(&bytes) != sha256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stored chunk digest mismatch",
            ));
        }
        Ok(bytes)
    }
}

#[cfg(unix)]
async fn restrict_directory(path: &FilePath) -> Result<(), io::Error> {
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await
}

#[cfg(not(unix))]
async fn restrict_directory(_path: &FilePath) -> Result<(), io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Store content object storage requires Unix permission semantics",
    ))
}

async fn checked_directory(path: &FilePath) -> Result<PathBuf, io::Error> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Store object path is not a real directory",
        ));
    }
    tokio::fs::canonicalize(path).await
}

#[cfg(unix)]
async fn checked_private_directory(path: &FilePath) -> Result<PathBuf, io::Error> {
    let path = checked_directory(path).await?;
    let metadata = tokio::fs::symlink_metadata(&path).await?;
    if metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Store object directory must not grant group or other access",
        ));
    }
    Ok(path)
}

#[cfg(not(unix))]
async fn checked_private_directory(_path: &FilePath) -> Result<PathBuf, io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Store content object storage requires Unix permission semantics",
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectGcMode {
    DryRun,
    Apply,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObjectGcReport {
    pub mode: &'static str,
    pub observed_unix_seconds: u64,
    pub minimum_age_seconds: u64,
    pub scanned_chunks: u64,
    pub referenced_chunks: u64,
    pub retained_young_chunks: u64,
    pub orphan_chunks: u64,
    pub orphan_chunk_bytes: u64,
    pub deleted_chunks: u64,
    pub deleted_chunk_bytes: u64,
    pub scanned_temporary_files: u64,
    pub retained_young_temporary_files: u64,
    pub stale_temporary_files: u64,
    pub deleted_temporary_files: u64,
}

#[derive(Debug)]
pub enum ObjectGcError {
    Database(sqlx::Error),
    Io(io::Error),
    UnsafeLayout(String),
}

impl fmt::Display for ObjectGcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "object GC database failure: {error}"),
            Self::Io(error) => write!(formatter, "object GC filesystem failure: {error}"),
            Self::UnsafeLayout(reason) => write!(formatter, "unsafe object layout: {reason}"),
        }
    }
}

impl Error for ObjectGcError {}

impl From<sqlx::Error> for ObjectGcError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<io::Error> for ObjectGcError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug)]
struct GcFile {
    path: PathBuf,
    digest: Option<String>,
    bytes: u64,
    modified_unix_seconds: u64,
    device: u64,
    inode: u64,
    links: u64,
}

pub async fn collect_content_objects(
    pool: &PgPool,
    root: impl AsRef<FilePath>,
    mode: ObjectGcMode,
    minimum_age_seconds: u64,
) -> Result<ObjectGcReport, ObjectGcError> {
    if mode == ObjectGcMode::Apply && minimum_age_seconds < DEFAULT_OBJECT_GC_MINIMUM_AGE_SECONDS {
        return Err(ObjectGcError::UnsafeLayout(
            "apply mode requires a minimum age of at least 86400 seconds".to_owned(),
        ));
    }
    if !root.as_ref().is_absolute() {
        return Err(ObjectGcError::UnsafeLayout(
            "Store object root must be absolute".to_owned(),
        ));
    }
    let store = ContentObjectStore::open_existing(root.as_ref()).await?;
    let chunks = inventory_chunks(store.chunks.as_ref()).await?;
    let temporary = inventory_temporary(store.temporary.as_ref()).await?;
    if chunks.len().saturating_add(temporary.len()) > MAX_GC_FILES {
        return Err(ObjectGcError::UnsafeLayout(
            "object inventory exceeds the fixed maintenance bound".to_owned(),
        ));
    }

    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(OBJECT_GC_LOCK_DOMAIN)
        .execute(&mut *transaction)
        .await?;
    let now = database_now(&mut transaction)
        .await
        .map_err(|error| match error {
            TxError::Sql(error) => ObjectGcError::Database(error),
            TxError::Api(_) => {
                ObjectGcError::UnsafeLayout("database clock returned an invalid value".to_owned())
            }
        })?;
    let now = u64::try_from(now).map_err(|_| {
        ObjectGcError::UnsafeLayout("database clock returned an invalid value".to_owned())
    })?;
    let cutoff = now.saturating_sub(minimum_age_seconds);

    let mut referenced = BTreeSet::new();
    for batch in chunks.chunks(GC_REFERENCE_BATCH_SIZE) {
        let digests = batch
            .iter()
            .map(|file| file.digest.clone().expect("chunk inventory has digests"))
            .collect::<Vec<_>>();
        let rows = sqlx::query(
            "SELECT DISTINCT chunk_sha256::TEXT AS chunk_sha256 \
             FROM submission_upload_chunks WHERE chunk_sha256 = ANY($1::CHAR(64)[])",
        )
        .bind(&digests)
        .fetch_all(&mut *transaction)
        .await?;
        referenced.extend(
            rows.into_iter()
                .map(|row| row.get::<String, _>("chunk_sha256")),
        );
    }

    let mut report = ObjectGcReport {
        mode: match mode {
            ObjectGcMode::DryRun => "dry-run",
            ObjectGcMode::Apply => "apply",
        },
        observed_unix_seconds: now,
        minimum_age_seconds,
        scanned_chunks: u64::try_from(chunks.len()).unwrap_or(u64::MAX),
        referenced_chunks: 0,
        retained_young_chunks: 0,
        orphan_chunks: 0,
        orphan_chunk_bytes: 0,
        deleted_chunks: 0,
        deleted_chunk_bytes: 0,
        scanned_temporary_files: u64::try_from(temporary.len()).unwrap_or(u64::MAX),
        retained_young_temporary_files: 0,
        stale_temporary_files: 0,
        deleted_temporary_files: 0,
    };
    let mut orphan_chunks = Vec::new();
    for file in chunks {
        let digest = file.digest.as_deref().expect("chunk inventory has digests");
        if referenced.contains(digest) {
            report.referenced_chunks += 1;
        } else if file.modified_unix_seconds > cutoff {
            report.retained_young_chunks += 1;
        } else {
            report.orphan_chunks += 1;
            report.orphan_chunk_bytes = report.orphan_chunk_bytes.saturating_add(file.bytes);
            orphan_chunks.push(file);
        }
    }
    let mut stale_temporary = Vec::new();
    for file in temporary {
        if file.modified_unix_seconds > cutoff {
            report.retained_young_temporary_files += 1;
        } else {
            report.stale_temporary_files += 1;
            stale_temporary.push(file);
        }
    }

    revalidate_gc_files(&orphan_chunks).await?;
    revalidate_gc_files(&stale_temporary).await?;
    if mode == ObjectGcMode::Apply {
        let mut changed_directories = BTreeSet::new();
        for file in &orphan_chunks {
            if let Err(error) = tokio::fs::remove_file(&file.path).await {
                sync_changed_directories(&changed_directories).await;
                return Err(error.into());
            }
            if let Some(parent) = file.path.parent() {
                changed_directories.insert(parent.to_owned());
            }
            report.deleted_chunks += 1;
            report.deleted_chunk_bytes = report.deleted_chunk_bytes.saturating_add(file.bytes);
        }
        for file in &stale_temporary {
            if let Err(error) = tokio::fs::remove_file(&file.path).await {
                sync_changed_directories(&changed_directories).await;
                return Err(error.into());
            }
            if let Some(parent) = file.path.parent() {
                changed_directories.insert(parent.to_owned());
            }
            report.deleted_temporary_files += 1;
        }
        for directory in changed_directories {
            sync_directory(&directory).await?;
        }
    }
    transaction.commit().await?;
    Ok(report)
}

async fn inventory_chunks(root: &FilePath) -> Result<Vec<GcFile>, ObjectGcError> {
    let mut inventory = Vec::new();
    let mut directories = tokio::fs::read_dir(root).await?;
    while let Some(entry) = directories.next_entry().await? {
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| ObjectGcError::UnsafeLayout("non-UTF-8 chunk prefix".to_owned()))?;
        let metadata = tokio::fs::symlink_metadata(entry.path()).await?;
        if name.len() != 2
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || metadata.file_type().is_symlink()
            || !metadata.is_dir()
        {
            return Err(ObjectGcError::UnsafeLayout(format!(
                "invalid chunk prefix {name:?}"
            )));
        }
        let directory = checked_directory(&entry.path()).await?;
        if !directory.starts_with(root) {
            return Err(ObjectGcError::UnsafeLayout(
                "chunk prefix escapes the object root".to_owned(),
            ));
        }
        let mut files = tokio::fs::read_dir(&directory).await?;
        while let Some(file) = files.next_entry().await? {
            let file_name = file.file_name();
            let file_name = file_name.to_str().ok_or_else(|| {
                ObjectGcError::UnsafeLayout("non-UTF-8 chunk filename".to_owned())
            })?;
            let digest = file_name.strip_suffix(".chunk").ok_or_else(|| {
                ObjectGcError::UnsafeLayout(format!("invalid chunk filename {file_name:?}"))
            })?;
            if !is_valid_sha256(digest) || &digest[..2] != name {
                return Err(ObjectGcError::UnsafeLayout(format!(
                    "chunk filename does not match prefix {file_name:?}"
                )));
            }
            inventory.push(gc_file(file.path(), Some(digest.to_owned())).await?);
            if inventory.len() > MAX_GC_FILES {
                return Err(ObjectGcError::UnsafeLayout(
                    "chunk inventory exceeds the fixed maintenance bound".to_owned(),
                ));
            }
        }
    }
    inventory.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(inventory)
}

async fn inventory_temporary(root: &FilePath) -> Result<Vec<GcFile>, ObjectGcError> {
    let mut inventory = Vec::new();
    let mut files = tokio::fs::read_dir(root).await?;
    while let Some(file) = files.next_entry().await? {
        let file_name = file.file_name();
        let file_name = file_name.to_str().ok_or_else(|| {
            ObjectGcError::UnsafeLayout("non-UTF-8 temporary filename".to_owned())
        })?;
        let identifier = file_name.strip_suffix(".part").ok_or_else(|| {
            ObjectGcError::UnsafeLayout(format!("invalid temporary filename {file_name:?}"))
        })?;
        if identifier.len() != 32
            || !identifier
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(ObjectGcError::UnsafeLayout(format!(
                "invalid temporary filename {file_name:?}"
            )));
        }
        inventory.push(gc_file(file.path(), None).await?);
        if inventory.len() > MAX_GC_FILES {
            return Err(ObjectGcError::UnsafeLayout(
                "temporary inventory exceeds the fixed maintenance bound".to_owned(),
            ));
        }
    }
    inventory.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(inventory)
}

#[cfg(unix)]
async fn gc_file(path: PathBuf, digest: Option<String>) -> Result<GcFile, ObjectGcError> {
    let metadata = tokio::fs::symlink_metadata(&path).await?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ObjectGcError::UnsafeLayout(format!(
            "object is not a regular file: {}",
            path.display()
        )));
    }
    let modified = modified_unix_seconds(&metadata, &path)?;
    Ok(GcFile {
        path,
        digest,
        bytes: metadata.len(),
        modified_unix_seconds: modified,
        device: metadata.dev(),
        inode: metadata.ino(),
        links: metadata.nlink(),
    })
}

#[cfg(not(unix))]
async fn gc_file(_path: PathBuf, _digest: Option<String>) -> Result<GcFile, ObjectGcError> {
    Err(ObjectGcError::UnsafeLayout(
        "object GC requires Unix inode semantics".to_owned(),
    ))
}

fn modified_unix_seconds(
    metadata: &std::fs::Metadata,
    path: &FilePath,
) -> Result<u64, ObjectGcError> {
    metadata
        .modified()
        .map_err(ObjectGcError::Io)?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| {
            ObjectGcError::UnsafeLayout(format!(
                "object modification time predates the Unix epoch: {}",
                path.display()
            ))
        })
}

#[cfg(unix)]
async fn revalidate_gc_files(files: &[GcFile]) -> Result<(), ObjectGcError> {
    for file in files {
        let metadata = tokio::fs::symlink_metadata(&file.path).await?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.dev() != file.device
            || metadata.ino() != file.inode
            || metadata.nlink() != file.links
            || metadata.len() != file.bytes
            || modified_unix_seconds(&metadata, &file.path)? != file.modified_unix_seconds
        {
            return Err(ObjectGcError::UnsafeLayout(format!(
                "object changed during collection: {}",
                file.path.display()
            )));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
async fn revalidate_gc_files(_files: &[GcFile]) -> Result<(), ObjectGcError> {
    Err(ObjectGcError::UnsafeLayout(
        "object GC requires Unix inode semantics".to_owned(),
    ))
}

async fn sync_directory(path: &FilePath) -> Result<(), io::Error> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || std::fs::File::open(path)?.sync_all())
        .await
        .map_err(io::Error::other)?
}

async fn sync_changed_directories(directories: &BTreeSet<PathBuf>) {
    for directory in directories {
        let _ = sync_directory(directory).await;
    }
}

async fn acquire_object_gc_upload_lock(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), TxError> {
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1, 0))")
        .bind(OBJECT_GC_LOCK_DOMAIN)
        .execute(&mut **transaction)
        .await
        .map_err(TxError::Sql)?;
    Ok(())
}

#[derive(Clone)]
pub struct StoreControlService {
    pool: PgPool,
    object_store: Option<ContentObjectStore>,
}

impl StoreControlService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            object_store: None,
        }
    }

    pub async fn with_object_root(
        pool: PgPool,
        root: impl AsRef<FilePath>,
    ) -> Result<Self, io::Error> {
        Ok(Self {
            pool,
            object_store: Some(ContentObjectStore::open(root.as_ref()).await?),
        })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn object_store(&self) -> Result<&ContentObjectStore, ApiError> {
        self.object_store.as_ref().ok_or_else(ApiError::unavailable)
    }

    async fn create_device_authorization(
        &self,
        request: &DeviceCodeRequest,
    ) -> Result<DeviceCodeResponse, ApiError> {
        if request.client_id != DEVICE_CLIENT_ID || request.scope != DEVICE_SCOPE {
            return Err(ApiError::invalid_request());
        }
        for _ in 0..MAX_TRANSACTION_ATTEMPTS {
            let device_code = random_secret("cp0_dc_");
            let user_code = random_user_code();
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|_| ApiError::unavailable())?;
            sqlx::query(
                "SELECT pg_advisory_xact_lock(\
                 hashtextextended('cp0.oauth-device.active-cap.v1', 0))",
            )
            .execute(&mut *transaction)
            .await
            .map_err(|_| ApiError::unavailable())?;
            let now: i64 =
                sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(|_| ApiError::unavailable())?;
            let active: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM oauth_device_authorizations \
                 WHERE state = 'pending' AND expires_unix_seconds > $1",
            )
            .bind(now)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| ApiError::unavailable())?;
            if active >= MAX_ACTIVE_DEVICE_AUTHORIZATIONS {
                return Err(ApiError::unavailable());
            }
            let inserted = sqlx::query(
                "INSERT INTO oauth_device_authorizations (device_code_sha256, user_code, \
                 client_id, scopes, state, requested_unix_seconds, expires_unix_seconds, \
                 poll_interval_seconds, next_poll_unix_seconds) \
                 VALUES ($1, $2, $3, ARRAY[$4], 'pending', $5, $6, $7, $8) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(sha256_hex(device_code.as_bytes()))
            .bind(&user_code)
            .bind(DEVICE_CLIENT_ID)
            .bind(DEVICE_SCOPE)
            .bind(now)
            .bind(now + DEVICE_CODE_TTL_SECONDS)
            .bind(DEVICE_POLL_INTERVAL_SECONDS)
            .bind(now + i64::from(DEVICE_POLL_INTERVAL_SECONDS))
            .execute(&mut *transaction)
            .await
            .map_err(|_| ApiError::unavailable())?
            .rows_affected();
            if inserted == 1 {
                transaction
                    .commit()
                    .await
                    .map_err(|_| ApiError::unavailable())?;
                return Ok(DeviceCodeResponse {
                    device_code,
                    user_code,
                    verification_uri: DEVICE_VERIFICATION_URI,
                    expires_in: DEVICE_CODE_TTL_SECONDS as u64,
                    interval: DEVICE_POLL_INTERVAL_SECONDS as u64,
                });
            }
        }
        Err(ApiError::unavailable())
    }

    async fn decide_device_authorization(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        request: &DeviceAuthorizationDecisionRequest,
    ) -> Result<(), ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .decide_device_authorization_once(&token_sha256, &key_sha256, request_id, request)
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(ApiError::from_transaction(error)),
                Ok(()) => return Ok(()),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn decide_device_authorization_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        request: &DeviceAuthorizationDecisionRequest,
    ) -> Result<(), TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        require_device_authorization(&identity)?;
        let now = database_now(&mut transaction).await?;
        let request_sha256 = oauth_decision_request_sha256(request);
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::NO_CONTENT.as_u16() as i16 && body == json!({}) =>
            {
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(());
            }
            IdempotencyReservation::Replay { .. } => return Err(ApiError::internal().into()),
            IdempotencyReservation::Fresh => {}
        }
        let row = sqlx::query(
            "SELECT device_code_sha256, state, expires_unix_seconds \
             FROM oauth_device_authorizations WHERE user_code = $1 FOR UPDATE",
        )
        .bind(&request.user_code)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
        if row.get::<i64, _>("expires_unix_seconds") <= now {
            return Err(ApiError::expired_token().into());
        }
        if row.get::<String, _>("state") != "pending" {
            return Err(ApiError::conflict().into());
        }
        let next_state = match request.decision.as_str() {
            "approve" => "approved",
            "deny" => "denied",
            _ => return Err(ApiError::invalid_request().into()),
        };
        let device_code_sha256: String = row.get("device_code_sha256");
        let changed = sqlx::query(
            "UPDATE oauth_device_authorizations SET state = $1, member_id = $2, \
             decided_unix_seconds = $3 WHERE device_code_sha256 = $4 AND state = 'pending'",
        )
        .bind(next_state)
        .bind(&identity.member_id)
        .bind(now)
        .bind(&device_code_sha256)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .rows_affected();
        if changed != 1 {
            return Err(ApiError::conflict().into());
        }
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            StatusCode::NO_CONTENT,
            &json!({}),
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.member_id,
                action: "oauth-device.decided",
                topic: "oauth-device.decided",
                object_kind: "oauth-device",
                object_id: &device_code_sha256,
                before_state: Some("pending"),
                after_state: Some(next_state),
                resource_version: 1,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "device_code_sha256": device_code_sha256,
                    "decision": request.decision,
                    "member_id": identity.member_id
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)
    }

    async fn exchange_device_authorization(
        &self,
        request: &DeviceTokenRequest,
    ) -> Result<DeviceTokenResponse, ApiError> {
        if request.grant_type != DEVICE_GRANT_TYPE
            || request.client_id != DEVICE_CLIENT_ID
            || !is_valid_device_code(&request.device_code)
        {
            return Err(ApiError::invalid_request());
        }
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self.exchange_device_authorization_once(request).await {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(ApiError::from_transaction(error)),
                Ok(response) => return Ok(response),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn exchange_device_authorization_once(
        &self,
        request: &DeviceTokenRequest,
    ) -> Result<DeviceTokenResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let now = database_now(&mut transaction).await?;
        let device_code_sha256 = sha256_hex(request.device_code.as_bytes());
        let row = sqlx::query(
            "SELECT state, member_id, expires_unix_seconds, poll_interval_seconds, \
             next_poll_unix_seconds FROM oauth_device_authorizations \
             WHERE device_code_sha256 = $1 AND client_id = $2 FOR UPDATE",
        )
        .bind(&device_code_sha256)
        .bind(&request.client_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::expired_token)?;
        if row.get::<i64, _>("expires_unix_seconds") <= now {
            return Err(ApiError::expired_token().into());
        }
        let state: String = row.get("state");
        if state == "pending" {
            let interval: i16 = row.get("poll_interval_seconds");
            let too_soon = now < row.get::<i64, _>("next_poll_unix_seconds");
            let next_interval = if too_soon {
                (interval + 5).min(30)
            } else {
                interval
            };
            sqlx::query(
                "UPDATE oauth_device_authorizations SET poll_interval_seconds = $1, \
                 last_poll_unix_seconds = $2, next_poll_unix_seconds = $3 \
                 WHERE device_code_sha256 = $4 AND state = 'pending'",
            )
            .bind(next_interval)
            .bind(now)
            .bind(now + i64::from(next_interval))
            .bind(&device_code_sha256)
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;
            transaction.commit().await.map_err(TxError::Sql)?;
            return Err(if too_soon {
                ApiError::slow_down()
            } else {
                ApiError::authorization_pending()
            }
            .into());
        }
        if state != "approved" {
            return Err(ApiError::access_denied().into());
        }
        let member_id: String = row
            .get::<Option<String>, _>("member_id")
            .ok_or_else(ApiError::access_denied)?;
        let member = sqlx::query(
            "SELECT role, two_factor_enabled FROM team_members \
             WHERE member_id = $1 AND membership_state = 'active'",
        )
        .bind(&member_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::access_denied)?;
        if !matches!(
            member.get::<String, _>("role").as_str(),
            "owner" | "developer"
        ) || !member.get::<bool, _>("two_factor_enabled")
        {
            return Err(ApiError::access_denied().into());
        }
        let access_token = random_secret("cp0_at_");
        let access_token_sha256 = sha256_hex(access_token.as_bytes());
        sqlx::query(
            "INSERT INTO access_tokens (token_sha256, member_id, scopes, \
             expires_unix_seconds, revoked, created_unix_seconds) \
             VALUES ($1, $2, ARRAY[$3], $4, FALSE, $5)",
        )
        .bind(&access_token_sha256)
        .bind(&member_id)
        .bind(DEVICE_SCOPE)
        .bind(now + ACCESS_TOKEN_TTL_SECONDS)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        let changed = sqlx::query(
            "UPDATE oauth_device_authorizations SET state = 'consumed', \
             consumed_unix_seconds = $1, issued_token_sha256 = $2 \
             WHERE device_code_sha256 = $3 AND state = 'approved'",
        )
        .bind(now)
        .bind(&access_token_sha256)
        .bind(&device_code_sha256)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .rows_affected();
        if changed != 1 {
            return Err(ApiError::access_denied().into());
        }
        let request_sha256 = oauth_exchange_request_sha256(&device_code_sha256);
        let exchange_request_id = request_id();
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &member_id,
                action: "oauth-device.consumed",
                topic: "oauth-device.consumed",
                object_kind: "oauth-device",
                object_id: &device_code_sha256,
                before_state: Some("approved"),
                after_state: Some("consumed"),
                resource_version: 2,
                request_id: &exchange_request_id,
                request_sha256: &request_sha256,
                key_sha256: &request_sha256,
                payload: json!({
                    "device_code_sha256": device_code_sha256,
                    "member_id": member_id,
                    "scope": DEVICE_SCOPE
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(DeviceTokenResponse {
            access_token,
            token_type: "Bearer",
            expires_in: ACCESS_TOKEN_TTL_SECONDS as u64,
            scope: DEVICE_SCOPE,
        })
    }

    async fn get_team(&self, token: &str, team_id: &str) -> Result<TeamResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate(&mut transaction, &token_sha256)
            .await
            .map_err(ApiError::from_transaction)?;
        require_team_read(&identity)?;
        if identity.team_id != team_id {
            return Err(ApiError::not_found());
        }
        let team = load_team(&mut transaction, team_id, false)
            .await
            .map_err(ApiError::from_transaction)?;
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;
        Ok(team)
    }

    #[allow(clippy::too_many_arguments)]
    async fn set_team_member_role(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        team_id: &str,
        member_id: &str,
        expected_version: u64,
        request: &SetTeamMemberRoleRequest,
    ) -> Result<TeamResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .set_team_member_role_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    team_id,
                    member_id,
                    expected_version,
                    request,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(ApiError::from_transaction(error)),
                Ok(team) => return Ok(team),
            }
        }
        Err(ApiError::unavailable())
    }

    #[allow(clippy::too_many_arguments)]
    async fn set_team_member_role_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        team_id: &str,
        member_id: &str,
        expected_version: u64,
        request: &SetTeamMemberRoleRequest,
    ) -> Result<TeamResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        let now = database_now(&mut transaction).await?;
        require_team_write(&identity, now)?;
        let expected_version_text = expected_version.to_string();
        let request_sha256 = mutation_request_sha256(
            "team.member-role.v1",
            &[team_id, member_id, &expected_version_text, &request.role],
        );
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::OK.as_u16() as i16 =>
            {
                let team = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(team);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }
        if identity.team_id != team_id {
            return Err(ApiError::not_found().into());
        }
        let current_team = load_team(&mut transaction, team_id, true).await?;
        if current_team.resource_version != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        let target = sqlx::query(
            "SELECT role, resource_version FROM team_members \
             WHERE member_id = $1 AND team_id = $2 AND membership_state = 'active' FOR UPDATE",
        )
        .bind(member_id)
        .bind(team_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
        let before_role: String = target.get("role");
        if before_role == request.role {
            return Err(ApiError::invalid_transition().into());
        }
        if before_role == "owner" && request.role != "owner" {
            let owner_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM team_members \
                 WHERE team_id = $1 AND role = 'owner' AND membership_state = 'active'",
            )
            .bind(team_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;
            if owner_count <= 1 {
                return Err(ApiError::conflict().into());
            }
        }
        let member_version = u64::try_from(target.get::<i64, _>("resource_version"))
            .map_err(|_| ApiError::internal())?
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        sqlx::query(
            "UPDATE team_members SET role = $1, resource_version = $2 WHERE member_id = $3",
        )
        .bind(&request.role)
        .bind(i64::try_from(member_version).map_err(|_| ApiError::internal())?)
        .bind(member_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        let team_version = expected_version
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        sqlx::query("UPDATE teams SET resource_version = $1 WHERE team_id = $2")
            .bind(i64::try_from(team_version).map_err(|_| ApiError::internal())?)
            .bind(team_id)
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;
        sqlx::query("UPDATE access_tokens SET revoked = TRUE WHERE member_id = $1 AND NOT revoked")
            .bind(member_id)
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;

        let team = load_team(&mut transaction, team_id, false).await?;
        let response_body = serde_json::to_value(&team).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            StatusCode::OK,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.member_id,
                action: "team.member-role-changed",
                topic: "team.member-role-changed",
                object_kind: "team",
                object_id: team_id,
                before_state: None,
                after_state: None,
                resource_version: team_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "team_id": team_id,
                    "member_id": member_id,
                    "before_role": before_role,
                    "after_role": request.role,
                    "member_resource_version": member_version
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(team)
    }

    #[allow(clippy::too_many_arguments)]
    async fn remove_team_member(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        team_id: &str,
        member_id: &str,
        expected_version: u64,
    ) -> Result<TeamResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .remove_team_member_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    team_id,
                    member_id,
                    expected_version,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(ApiError::from_transaction(error)),
                Ok(team) => return Ok(team),
            }
        }
        Err(ApiError::unavailable())
    }

    #[allow(clippy::too_many_arguments)]
    async fn remove_team_member_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        team_id: &str,
        member_id: &str,
        expected_version: u64,
    ) -> Result<TeamResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        let now = database_now(&mut transaction).await?;
        require_team_write(&identity, now)?;
        let expected_version_text = expected_version.to_string();
        let request_sha256 = mutation_request_sha256(
            "team.member-remove.v1",
            &[team_id, member_id, &expected_version_text],
        );
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::OK.as_u16() as i16 =>
            {
                let team = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(team);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }
        if identity.team_id != team_id {
            return Err(ApiError::not_found().into());
        }
        let current_team = load_team(&mut transaction, team_id, true).await?;
        if current_team.resource_version != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        let target = sqlx::query(
            "SELECT role, membership_state, resource_version FROM team_members \
             WHERE member_id = $1 AND team_id = $2 AND membership_state <> 'removed' FOR UPDATE",
        )
        .bind(member_id)
        .bind(team_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
        let before_role: String = target.get("role");
        let before_state: String = target.get("membership_state");
        if before_role == "owner" && before_state == "active" {
            let owner_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM team_members \
                 WHERE team_id = $1 AND role = 'owner' AND membership_state = 'active'",
            )
            .bind(team_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;
            if owner_count <= 1 {
                return Err(ApiError::conflict().into());
            }
        }
        let member_version = u64::try_from(target.get::<i64, _>("resource_version"))
            .map_err(|_| ApiError::internal())?
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        let changed = sqlx::query(
            "UPDATE team_members SET membership_state = 'removed', removed_unix_seconds = $1, \
             resource_version = $2 WHERE member_id = $3 AND team_id = $4 \
             AND membership_state = $5",
        )
        .bind(now)
        .bind(i64::try_from(member_version).map_err(|_| ApiError::internal())?)
        .bind(member_id)
        .bind(team_id)
        .bind(&before_state)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .rows_affected();
        if changed != 1 {
            return Err(ApiError::not_found().into());
        }
        let team_version = expected_version
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        sqlx::query("UPDATE teams SET resource_version = $1 WHERE team_id = $2")
            .bind(i64::try_from(team_version).map_err(|_| ApiError::internal())?)
            .bind(team_id)
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;
        sqlx::query("UPDATE access_tokens SET revoked = TRUE WHERE member_id = $1 AND NOT revoked")
            .bind(member_id)
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;

        let team = load_team(&mut transaction, team_id, false).await?;
        let response_body = serde_json::to_value(&team).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            StatusCode::OK,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.member_id,
                action: "team.member-removed",
                topic: "team.member-removed",
                object_kind: "team",
                object_id: team_id,
                before_state: Some(&before_state),
                after_state: Some("removed"),
                resource_version: team_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "team_id": team_id,
                    "member_id": member_id,
                    "before_role": before_role,
                    "member_resource_version": member_version,
                    "removed_unix_seconds": now
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(team)
    }

    #[allow(clippy::too_many_arguments)]
    async fn set_team_member_state(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        team_id: &str,
        member_id: &str,
        expected_version: u64,
        action: TeamMemberStateAction,
    ) -> Result<TeamResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .set_team_member_state_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    team_id,
                    member_id,
                    expected_version,
                    action,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(ApiError::from_transaction(error)),
                Ok(team) => return Ok(team),
            }
        }
        Err(ApiError::unavailable())
    }

    #[allow(clippy::too_many_arguments)]
    async fn set_team_member_state_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        team_id: &str,
        member_id: &str,
        expected_version: u64,
        action: TeamMemberStateAction,
    ) -> Result<TeamResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        let now = database_now(&mut transaction).await?;
        require_team_write(&identity, now)?;
        let expected_version_text = expected_version.to_string();
        let request_sha256 = mutation_request_sha256(
            action.request_domain(),
            &[team_id, member_id, &expected_version_text],
        );
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::OK.as_u16() as i16 =>
            {
                let team = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(team);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }
        if identity.team_id != team_id {
            return Err(ApiError::not_found().into());
        }
        let current_team = load_team(&mut transaction, team_id, true).await?;
        if current_team.resource_version != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        let target = sqlx::query(
            "SELECT role, membership_state, resource_version FROM team_members \
             WHERE member_id = $1 AND team_id = $2 AND membership_state <> 'removed' FOR UPDATE",
        )
        .bind(member_id)
        .bind(team_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
        let role: String = target.get("role");
        let before_state: String = target.get("membership_state");
        if before_state != action.before_state() {
            return Err(ApiError::invalid_transition().into());
        }
        if action == TeamMemberStateAction::Suspend && role == "owner" {
            let owner_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM team_members \
                 WHERE team_id = $1 AND role = 'owner' AND membership_state = 'active'",
            )
            .bind(team_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;
            if owner_count <= 1 {
                return Err(ApiError::conflict().into());
            }
        }
        let member_version = u64::try_from(target.get::<i64, _>("resource_version"))
            .map_err(|_| ApiError::internal())?
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        let changed = sqlx::query(
            "UPDATE team_members SET membership_state = $1, resource_version = $2 \
             WHERE member_id = $3 AND team_id = $4 AND membership_state = $5",
        )
        .bind(action.after_state())
        .bind(i64::try_from(member_version).map_err(|_| ApiError::internal())?)
        .bind(member_id)
        .bind(team_id)
        .bind(action.before_state())
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .rows_affected();
        if changed != 1 {
            return Err(ApiError::invalid_transition().into());
        }
        let team_version = expected_version
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        sqlx::query("UPDATE teams SET resource_version = $1 WHERE team_id = $2")
            .bind(i64::try_from(team_version).map_err(|_| ApiError::internal())?)
            .bind(team_id)
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;
        sqlx::query("UPDATE access_tokens SET revoked = TRUE WHERE member_id = $1 AND NOT revoked")
            .bind(member_id)
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;

        let team = load_team(&mut transaction, team_id, false).await?;
        let response_body = serde_json::to_value(&team).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            StatusCode::OK,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.member_id,
                action: action.event_name(),
                topic: action.event_name(),
                object_kind: "team",
                object_id: team_id,
                before_state: Some(action.before_state()),
                after_state: Some(action.after_state()),
                resource_version: team_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "team_id": team_id,
                    "member_id": member_id,
                    "role": role,
                    "before_state": action.before_state(),
                    "after_state": action.after_state(),
                    "member_resource_version": member_version
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(team)
    }

    async fn create_app(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        request: &CreateAppRequest,
    ) -> Result<AppRecord, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());

        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .create_app_once(&token_sha256, &key_sha256, request_id, request)
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    tokio::time::sleep(Duration::from_millis(5 * (attempt as u64 + 1))).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(app) => return Ok(app),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn create_app_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        request: &CreateAppRequest,
    ) -> Result<AppRecord, TxError> {
        let mut transaction = self.pool.begin().await.map_err(TxError::Sql)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;

        let identity = authenticate(&mut transaction, token_sha256).await?;
        if !matches!(identity.role.as_str(), "owner" | "developer") {
            return Err(ApiError::forbidden().into());
        }
        if !identity.two_factor_enabled {
            return Err(ApiError::two_factor_required().into());
        }
        if !identity.has_any_scope(&["store.apps.write", "store.control"]) {
            return Err(ApiError::forbidden().into());
        }

        let request_sha256 = register_app_request_sha256(
            &identity.team_id,
            &request.app_id,
            &request.default_locale,
        );
        let now = database_now(&mut transaction).await?;

        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::CREATED.as_u16() as i16 =>
            {
                let app = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(app);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }

        let inserted = sqlx::query(
            "INSERT INTO apps (app_id, owner_team_id, default_locale, resource_version, created_unix_seconds) \
             VALUES ($1, $2, $3, 1, $4) \
             ON CONFLICT (app_id) DO NOTHING",
        )
        .bind(&request.app_id)
        .bind(&identity.team_id)
        .bind(&request.default_locale)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .rows_affected();
        if inserted != 1 {
            return Err(ApiError::conflict().into());
        }

        let app = AppRecord {
            app_id: request.app_id.clone(),
            owner_team_id: identity.team_id,
            default_locale: request.default_locale.clone(),
            resource_version: 1,
        };
        let response_body = serde_json::to_value(&app).map_err(|_| ApiError::internal())?;

        sqlx::query(
            "UPDATE idempotency_records \
             SET response_status = 201, response_body = $1 \
             WHERE actor_id = $2 AND key_sha256 = $3",
        )
        .bind(&response_body)
        .bind(&identity.member_id)
        .bind(key_sha256)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        sqlx::query(
            "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
             object_id, before_state, after_state, resource_version, request_id, request_sha256, \
             idempotency_key_sha256) \
             VALUES ($1, $2, 'app.registered', 'app', $3, NULL, NULL, 1, $4, $5, $6)",
        )
        .bind(now)
        .bind(&identity.member_id)
        .bind(&app.app_id)
        .bind(request_id)
        .bind(&request_sha256)
        .bind(key_sha256)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        sqlx::query(
            "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
             aggregate_version, request_sha256, payload, created_unix_seconds) \
             VALUES ($1, 'app.registered', 'app', $2, 1, $3, $4, $5)",
        )
        .bind(prefixed_uuid("evt_"))
        .bind(&app.app_id)
        .bind(&request_sha256)
        .bind(json!({"app_id": app.app_id, "owner_team_id": app.owner_team_id}))
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(app)
    }

    async fn get_app(&self, token: &str, app_id: &str) -> Result<AppRecord, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate(&mut transaction, &token_sha256)
            .await
            .map_err(ApiError::from_transaction)?;
        if !identity.has_any_scope(&[
            "store.apps.read",
            "store.apps.write",
            "store.submit",
            "store.control",
        ]) {
            return Err(ApiError::forbidden());
        }

        let row = sqlx::query(
            "SELECT app_id, owner_team_id, default_locale, resource_version \
             FROM apps WHERE app_id = $1 AND owner_team_id = $2",
        )
        .bind(app_id)
        .bind(&identity.team_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?
        .ok_or_else(ApiError::not_found)?;
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;

        Ok(AppRecord {
            app_id: row.get("app_id"),
            owner_team_id: row.get("owner_team_id"),
            default_locale: row.get("default_locale"),
            resource_version: row_version(&row)?,
        })
    }

    async fn create_submission(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        app_id: &str,
        request: &CreateSubmissionRequest,
    ) -> Result<SubmissionResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .create_submission_once(&token_sha256, &key_sha256, request_id, app_id, request)
                .await
            {
                Err(TxError::Sql(error))
                    if is_retryable_transaction_error(&error)
                        || is_submission_revision_conflict(&error) =>
                {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(submission) => return Ok(submission),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn create_submission_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        app_id: &str,
        request: &CreateSubmissionRequest,
    ) -> Result<SubmissionResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        require_developer_write(&identity)?;

        let owner_team_id: String =
            sqlx::query_scalar("SELECT owner_team_id FROM apps WHERE app_id = $1 FOR UPDATE")
                .bind(app_id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(TxError::Sql)?
                .ok_or_else(ApiError::not_found)?;
        if owner_team_id != identity.team_id {
            return Err(ApiError::forbidden().into());
        }

        let spec = request.spec();
        validate_submission_spec(&spec).map_err(|_| ApiError::invalid_request())?;
        let request_sha256 = create_submission_request_sha256(app_id, &spec);
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::CREATED.as_u16() as i16 =>
            {
                let submission = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(submission);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }

        let revision: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(revision), 0) + 1 FROM submissions \
             WHERE app_id = $1 AND version = $2",
        )
        .bind(app_id)
        .bind(&request.version)
        .fetch_one(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        let submission_id = prefixed_uuid("sub_");
        let assets = serde_json::to_value(&request.assets).map_err(|_| ApiError::internal())?;
        sqlx::query(
            "INSERT INTO submissions (submission_id, app_id, version, revision, state, \
             package_sha256, package_bytes, listing_sha256, listing_bytes, assets, \
             resource_version, created_unix_seconds) \
             VALUES ($1, $2, $3, $4, 'uploading', $5, $6, $7, $8, $9, 1, $10)",
        )
        .bind(&submission_id)
        .bind(app_id)
        .bind(&request.version)
        .bind(revision)
        .bind(&request.package_sha256)
        .bind(i64::try_from(request.package_bytes).map_err(|_| ApiError::invalid_request())?)
        .bind(&request.listing_sha256)
        .bind(i64::try_from(request.listing_bytes).map_err(|_| ApiError::invalid_request())?)
        .bind(&assets)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        insert_upload_part(
            &mut transaction,
            &submission_id,
            "package",
            &request.package_sha256,
            request.package_bytes,
        )
        .await?;
        insert_upload_part(
            &mut transaction,
            &submission_id,
            "listing",
            &request.listing_sha256,
            request.listing_bytes,
        )
        .await?;
        for (index, asset) in request.assets.iter().enumerate() {
            insert_upload_part(
                &mut transaction,
                &submission_id,
                &format!("asset-{index}"),
                &asset.sha256,
                asset.bytes,
            )
            .await?;
        }

        let submission = SubmissionResponse {
            submission_id: submission_id.clone(),
            app_id: app_id.to_owned(),
            version: request.version.clone(),
            revision: u32::try_from(revision).map_err(|_| ApiError::internal())?,
            state: SubmissionState::Uploading,
            package_sha256: request.package_sha256.clone(),
            listing_sha256: request.listing_sha256.clone(),
            assets: request.assets.clone(),
            resource_version: 1,
            created_unix_seconds: u64::try_from(now).map_err(|_| ApiError::internal())?,
        };
        let response_body = serde_json::to_value(&submission).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            StatusCode::CREATED,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.member_id,
                action: "submission.created",
                topic: "submission.created",
                object_kind: "submission",
                object_id: &submission_id,
                before_state: None,
                after_state: Some("uploading"),
                resource_version: 1,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "submission_id": submission_id,
                    "app_id": app_id,
                    "version": request.version,
                    "revision": revision
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(submission)
    }

    async fn get_submission(
        &self,
        token: &str,
        submission_id: &str,
    ) -> Result<SubmissionResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate(&mut transaction, &token_sha256)
            .await
            .map_err(ApiError::from_transaction)?;
        if !identity.has_any_scope(&["store.submit", "store.control"]) {
            return Err(ApiError::forbidden());
        }
        let stored = load_submission(&mut transaction, submission_id, &identity.team_id, false)
            .await
            .map_err(ApiError::from_transaction)?;
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;
        Ok(stored.response)
    }

    async fn upload_submission_part(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        upload: UploadMutation<'_>,
    ) -> Result<u64, ApiError> {
        let object_store = self.object_store()?.clone();
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .upload_submission_part_once(
                    &object_store,
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    upload,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(version) => return Ok(version),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn upload_submission_part_once(
        &self,
        object_store: &ContentObjectStore,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        upload: UploadMutation<'_>,
    ) -> Result<u64, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        require_developer_write(&identity)?;
        let request_sha256 = mutation_request_sha256(
            "submission.part.upload.v1",
            &[
                upload.submission_id,
                upload.part_name,
                &upload.range.start.to_string(),
                &upload.range.end.to_string(),
                &upload.range.total.to_string(),
                upload.chunk_sha256,
                &upload.expected_version.to_string(),
            ],
        );
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::NO_CONTENT.as_u16() as i16 =>
            {
                let version = body
                    .get("resource_version")
                    .and_then(Value::as_u64)
                    .ok_or_else(ApiError::internal)?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(version);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }

        let stored = load_submission(
            &mut transaction,
            upload.submission_id,
            &identity.team_id,
            true,
        )
        .await?;
        if stored.response.resource_version != upload.expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        if stored.response.state != SubmissionState::Uploading {
            return Err(ApiError::invalid_transition().into());
        }
        let part =
            load_upload_part(&mut transaction, upload.submission_id, upload.part_name).await?;
        if part.expected_bytes != upload.range.total || part.received_bytes != upload.range.start {
            return Err(ApiError::upload_range_conflict().into());
        }
        if sha256_hex(upload.body) != upload.chunk_sha256 {
            return Err(ApiError::digest_mismatch().into());
        }
        acquire_object_gc_upload_lock(&mut transaction).await?;
        object_store
            .store_chunk(upload.chunk_sha256, upload.body)
            .await
            .map_err(|_| ApiError::unavailable())?;

        sqlx::query(
            "INSERT INTO submission_upload_chunks (submission_id, part_name, chunk_offset, \
             chunk_bytes, chunk_sha256, created_unix_seconds) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(upload.submission_id)
        .bind(upload.part_name)
        .bind(i64::try_from(upload.range.start).map_err(|_| ApiError::invalid_request())?)
        .bind(i32::try_from(upload.body.len()).map_err(|_| ApiError::invalid_request())?)
        .bind(upload.chunk_sha256)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        let received_bytes = upload
            .range
            .end
            .checked_add(1)
            .ok_or_else(ApiError::invalid_request)?;
        sqlx::query(
            "UPDATE submission_upload_parts SET received_bytes = $1 \
             WHERE submission_id = $2 AND part_name = $3",
        )
        .bind(i64::try_from(received_bytes).map_err(|_| ApiError::invalid_request())?)
        .bind(upload.submission_id)
        .bind(upload.part_name)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        let resource_version = upload
            .expected_version
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        sqlx::query("UPDATE submissions SET resource_version = $1 WHERE submission_id = $2")
            .bind(i64::try_from(resource_version).map_err(|_| ApiError::internal())?)
            .bind(upload.submission_id)
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;
        let response_body = json!({"resource_version": resource_version});
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            StatusCode::NO_CONTENT,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.member_id,
                action: "submission.part-uploaded",
                topic: "submission.part-uploaded",
                object_kind: "submission",
                object_id: upload.submission_id,
                before_state: Some("uploading"),
                after_state: Some("uploading"),
                resource_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "submission_id": upload.submission_id,
                    "part_name": upload.part_name,
                    "offset": upload.range.start,
                    "bytes": upload.body.len(),
                    "chunk_sha256": upload.chunk_sha256
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(resource_version)
    }

    async fn finalize_submission(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        submission_id: &str,
        expected_version: u64,
        content_sha256: &str,
    ) -> Result<SubmissionResponse, ApiError> {
        let object_store = self.object_store()?.clone();
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .finalize_submission_once(
                    &object_store,
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    submission_id,
                    expected_version,
                    content_sha256,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(submission) => return Ok(submission),
            }
        }
        Err(ApiError::unavailable())
    }

    #[allow(clippy::too_many_arguments)]
    async fn finalize_submission_once(
        &self,
        object_store: &ContentObjectStore,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        submission_id: &str,
        expected_version: u64,
        content_sha256: &str,
    ) -> Result<SubmissionResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        require_developer_write(&identity)?;
        let request_sha256 = mutation_request_sha256(
            "submission.finalize.v1",
            &[submission_id, content_sha256, &expected_version.to_string()],
        );
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::ACCEPTED.as_u16() as i16 =>
            {
                let submission = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(submission);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }

        let mut stored =
            load_submission(&mut transaction, submission_id, &identity.team_id, true).await?;
        if stored.response.resource_version != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        if stored.response.state != SubmissionState::Uploading {
            return Err(ApiError::invalid_transition().into());
        }
        let parts = load_upload_parts(&mut transaction, submission_id).await?;
        if parts.len() != stored.response.assets.len() + 2
            || !upload_parts_match_submission(&stored, &parts)
            || parts
                .iter()
                .any(|part| part.received_bytes != part.expected_bytes)
        {
            return Err(ApiError::upload_range_conflict().into());
        }
        for part in &parts {
            verify_uploaded_part(object_store, &mut transaction, submission_id, part).await?;
        }
        let computed_content_sha256 = submission_content_sha256(
            &stored.response.package_sha256,
            &stored.response.listing_sha256,
            &stored.response.assets,
        );
        if computed_content_sha256 != content_sha256 {
            return Err(ApiError::digest_mismatch().into());
        }

        let resource_version = expected_version
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        sqlx::query(
            "UPDATE submissions SET state = 'processing', resource_version = $1, \
             finalized_content_sha256 = $2 WHERE submission_id = $3",
        )
        .bind(i64::try_from(resource_version).map_err(|_| ApiError::internal())?)
        .bind(content_sha256)
        .bind(submission_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        stored.response.state = SubmissionState::Processing;
        stored.response.resource_version = resource_version;
        let response_body =
            serde_json::to_value(&stored.response).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            StatusCode::ACCEPTED,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.member_id,
                action: "submission.finalized",
                topic: "submission.scan-requested",
                object_kind: "submission",
                object_id: submission_id,
                before_state: Some("uploading"),
                after_state: Some("processing"),
                resource_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "submission_id": submission_id,
                    "app_id": stored.response.app_id,
                    "version": stored.response.version,
                    "revision": stored.response.revision,
                    "content_sha256": content_sha256
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(stored.response)
    }

    async fn withdraw_submission(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        submission_id: &str,
        expected_version: u64,
    ) -> Result<SubmissionResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .withdraw_submission_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    submission_id,
                    expected_version,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(ApiError::from_transaction(error)),
                Ok(submission) => return Ok(submission),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn withdraw_submission_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        submission_id: &str,
        expected_version: u64,
    ) -> Result<SubmissionResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        require_developer_write(&identity)?;
        let expected_version_text = expected_version.to_string();
        let request_sha256 = mutation_request_sha256(
            "submission.withdraw.v1",
            &[submission_id, &expected_version_text],
        );
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::OK.as_u16() as i16 =>
            {
                let submission = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(submission);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }

        let mut stored =
            load_submission(&mut transaction, submission_id, &identity.team_id, true).await?;
        if stored.response.resource_version != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        if !stored
            .response
            .state
            .can_transition_to(SubmissionState::Withdrawn)
        {
            return Err(ApiError::invalid_transition().into());
        }
        let before_state = stored.response.state.as_str();
        let resource_version = expected_version
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        let changed = sqlx::query(
            "UPDATE submissions SET state = 'withdrawn', resource_version = $1 \
             WHERE submission_id = $2 AND resource_version = $3",
        )
        .bind(i64::try_from(resource_version).map_err(|_| ApiError::internal())?)
        .bind(submission_id)
        .bind(i64::try_from(expected_version).map_err(|_| ApiError::internal())?)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .rows_affected();
        if changed != 1 {
            return Err(ApiError::precondition_failed().into());
        }

        sqlx::query(
            "UPDATE submission_scan_jobs SET state = 'cancelled', lease_token = NULL, \
             leased_until_unix_seconds = NULL, last_error_code = 'submission-withdrawn', \
             completed_unix_seconds = $1 WHERE submission_id = $2 AND state IN ('queued', 'running')",
        )
        .bind(now)
        .bind(submission_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        sqlx::query(
            "UPDATE review_assignments SET state = 'cancelled', completed_unix_seconds = $1 \
             WHERE submission_id = $2 AND state = 'active'",
        )
        .bind(now)
        .bind(submission_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        sqlx::query(
            "UPDATE outbox_events SET published_unix_seconds = $1, attempts = attempts + 1 \
             WHERE topic = 'submission.scan-requested' AND aggregate_kind = 'submission' \
               AND aggregate_id = $2 AND published_unix_seconds IS NULL",
        )
        .bind(now)
        .bind(submission_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        stored.response.state = SubmissionState::Withdrawn;
        stored.response.resource_version = resource_version;
        let response_body =
            serde_json::to_value(&stored.response).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            StatusCode::OK,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.member_id,
                action: "submission.withdrawn",
                topic: "submission.withdrawn",
                object_kind: "submission",
                object_id: submission_id,
                before_state: Some(before_state),
                after_state: Some("withdrawn"),
                resource_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "submission_id": submission_id,
                    "app_id": stored.response.app_id,
                    "version": stored.response.version,
                    "revision": stored.response.revision
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(stored.response)
    }

    async fn list_review_queue(
        &self,
        token: &str,
        cursor: Option<ReviewCursor>,
        limit: usize,
    ) -> Result<ReviewQueueResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate_reviewer(&mut transaction, &token_sha256)
            .await
            .map_err(ApiError::from_transaction)?;
        require_reviewer_write(&identity)?;
        let (cursor_time, cursor_id) = cursor
            .map(|cursor| (cursor.created_unix_seconds, cursor.submission_id))
            .unwrap_or((0, String::new()));
        let rows = sqlx::query(
            "SELECT submission.submission_id, submission.app_id, submission.version, \
             submission.revision, submission.state, submission.package_sha256, \
             submission.package_bytes, submission.listing_sha256, submission.listing_bytes, \
             submission.assets, submission.resource_version, submission.created_unix_seconds, \
             review_metadata.name AS review_name, review_metadata.category AS review_category, \
             team.name AS developer_name, \
             current_assignment.assignment_kind AS current_assignment_kind, \
             CASE WHEN submission.state = 'ready-for-review' THEN 'primary' \
                  WHEN submission.state = 'pending-secondary-review' THEN 'secondary' \
                  ELSE current_assignment.assignment_kind END AS review_stage, \
             current_risk.policy_version AS risk_policy_version, \
             current_risk.tier AS risk_tier, current_risk.reason_codes AS risk_reason_codes \
             FROM submissions submission \
             JOIN submission_scan_results scan_result \
               ON scan_result.submission_id = submission.submission_id \
             JOIN submission_review_metadata review_metadata \
               ON review_metadata.submission_id = submission.submission_id AND \
                  review_metadata.scan_id = scan_result.scan_id \
             JOIN apps app ON app.app_id = submission.app_id \
             JOIN teams team ON team.team_id = app.owner_team_id \
             JOIN LATERAL ( \
               SELECT policy_version, tier, reason_codes \
               FROM submission_risk_assessments assessment \
               WHERE assessment.scan_id = scan_result.scan_id \
               ORDER BY policy_version DESC LIMIT 1 \
             ) current_risk ON TRUE \
             LEFT JOIN LATERAL ( \
               SELECT assignment_kind FROM review_assignments assignment \
               WHERE assignment.submission_id = submission.submission_id \
                 AND assignment.reviewer_id = $1 AND assignment.state = 'active' \
             ) current_assignment ON TRUE \
             WHERE (submission.state = 'ready-for-review' OR \
               (submission.state = 'pending-secondary-review' AND NOT EXISTS ( \
                 SELECT 1 FROM review_assignments assignment \
                 WHERE assignment.submission_id = submission.submission_id \
                   AND assignment.reviewer_id = $1)) OR \
               (submission.state = 'in-review' AND current_assignment.assignment_kind IS NOT NULL)) AND \
               (submission.created_unix_seconds, submission.submission_id) > ($2, $3) \
             ORDER BY submission.created_unix_seconds, submission.submission_id LIMIT $4",
        )
        .bind(&identity.reviewer_id)
        .bind(cursor_time)
        .bind(cursor_id)
        .bind(i64::try_from(limit + 1).map_err(|_| ApiError::invalid_request())?)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;

        let has_more = rows.len() > limit;
        let mut items = rows
            .iter()
            .take(limit)
            .map(|row| {
                let assigned_to_caller = row
                    .get::<Option<String>, _>("current_assignment_kind")
                    .is_some();
                Ok(ReviewQueueItemResponse {
                    submission: stored_submission_from_row(row)?.response,
                    app: review_app_from_row(row)?,
                    review_stage: row.get("review_stage"),
                    assigned_to_caller,
                    risk: risk_assessment_from_row(row)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = if has_more {
            items.last().map(|item| {
                encode_review_cursor(
                    item.submission.created_unix_seconds,
                    &item.submission.submission_id,
                )
            })
        } else {
            None
        };
        items.shrink_to_fit();
        Ok(ReviewQueueResponse { items, next_cursor })
    }

    async fn get_review_submission_detail(
        &self,
        token: &str,
        submission_id: &str,
    ) -> Result<ReviewSubmissionDetailResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate_reviewer(&mut transaction, &token_sha256)
            .await
            .map_err(ApiError::from_transaction)?;
        require_reviewer_write(&identity)?;
        let row = sqlx::query(
            "SELECT submission.submission_id, submission.app_id, submission.version, \
             submission.revision, submission.state, submission.package_sha256, \
             submission.package_bytes, submission.listing_sha256, submission.listing_bytes, \
             submission.assets, submission.resource_version, submission.created_unix_seconds, \
             review_metadata.name AS review_name, review_metadata.category AS review_category, \
             team.name AS developer_name, scan_result.scan_id, scan_result.scanner_version, \
             scan_result.report, scan_result.report_sha256, \
             caller_assignment.assignment_kind AS caller_assignment_kind, \
             caller_assignment.state AS caller_assignment_state, \
             CASE WHEN submission.state = 'ready-for-review' THEN 'primary' \
                  WHEN submission.state = 'pending-secondary-review' THEN 'secondary' \
                  ELSE caller_assignment.assignment_kind END AS review_stage, \
             current_risk.policy_version AS risk_policy_version, \
             current_risk.tier AS risk_tier, current_risk.reason_codes AS risk_reason_codes \
             FROM submissions submission \
             JOIN submission_scan_results scan_result \
               ON scan_result.submission_id = submission.submission_id \
             JOIN submission_review_metadata review_metadata \
               ON review_metadata.submission_id = submission.submission_id AND \
                  review_metadata.scan_id = scan_result.scan_id \
             JOIN apps app ON app.app_id = submission.app_id \
             JOIN teams team ON team.team_id = app.owner_team_id \
             JOIN LATERAL ( \
               SELECT policy_version, tier, reason_codes \
               FROM submission_risk_assessments assessment \
               WHERE assessment.scan_id = scan_result.scan_id \
               ORDER BY policy_version DESC LIMIT 1 \
             ) current_risk ON TRUE \
             LEFT JOIN LATERAL ( \
               SELECT assignment_kind, state FROM review_assignments assignment \
               WHERE assignment.submission_id = submission.submission_id AND \
                     assignment.reviewer_id = $1 \
               ORDER BY (state = 'active') DESC, created_unix_seconds DESC, assignment_id DESC \
               LIMIT 1 \
             ) caller_assignment ON TRUE \
             WHERE submission.submission_id = $2 AND ( \
               submission.state = 'ready-for-review' OR \
               (submission.state = 'pending-secondary-review' AND NOT EXISTS ( \
                 SELECT 1 FROM review_assignments assignment \
                 WHERE assignment.submission_id = submission.submission_id AND \
                       assignment.reviewer_id = $1 \
               )) OR caller_assignment.assignment_kind IS NOT NULL \
             )",
        )
        .bind(&identity.reviewer_id)
        .bind(submission_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?
        .ok_or_else(ApiError::not_found)?;

        let risk = risk_assessment_from_row(&row)?;
        let report_value: Value = row.get("report");
        let report: ScanReport =
            serde_json::from_value(report_value).map_err(|_| ApiError::internal())?;
        let report_sha256: String = row.get("report_sha256");
        if report.disposition != ScanDisposition::ReadyForReview
            || report.risk.as_ref() != Some(&risk)
            || report.scanner_version != row.get::<String, _>("scanner_version")
            || cp0_store_scan::report_sha256(&report).map_err(|_| ApiError::internal())?
                != report_sha256
        {
            return Err(ApiError::internal());
        }

        let mut message_rows = sqlx::query(
            "SELECT message.message_id, message.actor_id, message.actor_kind, message.body, \
             message.created_unix_seconds, \
             CASE WHEN message.actor_kind = 'reviewer' THEN reviewer.email \
                  ELSE team.name END AS actor_label \
             FROM review_messages message \
             LEFT JOIN reviewers reviewer ON message.actor_kind = 'reviewer' AND \
                  reviewer.reviewer_id = message.actor_id \
             LEFT JOIN team_members member ON message.actor_kind = 'developer' AND \
                  member.member_id = message.actor_id \
             LEFT JOIN teams team ON team.team_id = member.team_id \
             WHERE message.submission_id = $1 \
             ORDER BY message.created_unix_seconds DESC, message.message_id DESC LIMIT $2",
        )
        .bind(submission_id)
        .bind(i64::try_from(MAX_REVIEW_DETAIL_MESSAGES + 1).map_err(|_| ApiError::internal())?)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        let messages_truncated = message_rows.len() > MAX_REVIEW_DETAIL_MESSAGES;
        message_rows.truncate(MAX_REVIEW_DETAIL_MESSAGES);
        message_rows.reverse();
        let messages = message_rows
            .into_iter()
            .map(|row| {
                Ok(ReviewDetailMessageResponse {
                    message_id: row.get("message_id"),
                    actor_id: row.get("actor_id"),
                    actor_kind: row.get("actor_kind"),
                    actor_label: row
                        .get::<Option<String>, _>("actor_label")
                        .ok_or_else(ApiError::internal)?,
                    body: row.get("body"),
                    created_unix_seconds: u64::try_from(row.get::<i64, _>("created_unix_seconds"))
                        .map_err(|_| ApiError::internal())?,
                })
            })
            .collect::<Result<Vec<_>, ApiError>>()?;

        let assignment_rows = sqlx::query(
            "SELECT assignment.assignment_id, assignment.reviewer_id, reviewer.email, \
             reviewer.role, assignment.assignment_kind, assignment.state, \
             assignment.created_unix_seconds, assignment.completed_unix_seconds \
             FROM review_assignments assignment \
             JOIN reviewers reviewer ON reviewer.reviewer_id = assignment.reviewer_id \
             WHERE assignment.submission_id = $1 \
             ORDER BY assignment.created_unix_seconds, assignment.assignment_id LIMIT $2",
        )
        .bind(submission_id)
        .bind(i64::try_from(MAX_REVIEW_DETAIL_ASSIGNMENTS + 1).map_err(|_| ApiError::internal())?)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        if assignment_rows.len() > MAX_REVIEW_DETAIL_ASSIGNMENTS {
            return Err(ApiError::internal());
        }
        let assignments = assignment_rows
            .into_iter()
            .map(|row| review_assignment_from_row(&row))
            .collect::<Result<Vec<_>, _>>()?;

        let decision_rows = sqlx::query(
            "SELECT decision.decision_id, decision.reviewer_id, reviewer.email, \
             decision.decision, decision.reason_codes, decision.note, \
             decision.created_unix_seconds, decision.assignment_id \
             FROM review_decisions decision \
             JOIN reviewers reviewer ON reviewer.reviewer_id = decision.reviewer_id \
             WHERE decision.submission_id = $1 \
             ORDER BY decision.created_unix_seconds, decision.decision_id LIMIT $2",
        )
        .bind(submission_id)
        .bind(i64::try_from(MAX_REVIEW_DETAIL_DECISIONS + 1).map_err(|_| ApiError::internal())?)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        if decision_rows.len() > MAX_REVIEW_DETAIL_DECISIONS {
            return Err(ApiError::internal());
        }
        let decisions = decision_rows
            .into_iter()
            .map(|row| review_decision_from_row(&row))
            .collect::<Result<Vec<_>, _>>()?;

        let mut audit_rows = sqlx::query(
            "SELECT sequence, occurred_unix_seconds, actor_id, action, before_state, after_state, \
             resource_version FROM audit_events \
             WHERE object_kind = 'submission' AND object_id = $1 \
             ORDER BY sequence DESC LIMIT $2",
        )
        .bind(submission_id)
        .bind(i64::try_from(MAX_REVIEW_DETAIL_AUDIT_EVENTS + 1).map_err(|_| ApiError::internal())?)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        let audit_truncated = audit_rows.len() > MAX_REVIEW_DETAIL_AUDIT_EVENTS;
        audit_rows.truncate(MAX_REVIEW_DETAIL_AUDIT_EVENTS);
        audit_rows.reverse();
        let audit = audit_rows
            .into_iter()
            .map(|row| review_audit_from_row(&row))
            .collect::<Result<Vec<_>, _>>()?;

        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let submission = stored_submission_from_row(&row)?.response;
        let review_stage = row
            .get::<Option<String>, _>("review_stage")
            .ok_or_else(ApiError::internal)?;
        let assigned_to_caller = row
            .get::<Option<String>, _>("caller_assignment_state")
            .is_some_and(|state| state == "active");
        Ok(ReviewSubmissionDetailResponse {
            submission,
            app: review_app_from_row(&row)?,
            review_stage,
            assigned_to_caller,
            risk,
            scan: ReviewScanResponse {
                scan_id: row.get("scan_id"),
                scanner_version: report.scanner_version,
                report_sha256,
                developer_key_sha256: report.developer_key_sha256,
                imports: report.imports,
                permissions: report.permissions,
                findings: report.findings,
            },
            assignments,
            decisions,
            messages,
            messages_truncated,
            audit,
            audit_truncated,
        })
    }

    async fn begin_review(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        submission_id: &str,
        expected_version: u64,
    ) -> Result<SubmissionResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .begin_review_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    submission_id,
                    expected_version,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(submission) => return Ok(submission),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn begin_review_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        submission_id: &str,
        expected_version: u64,
    ) -> Result<SubmissionResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate_reviewer(&mut transaction, token_sha256).await?;
        require_reviewer_write(&identity)?;
        let request_sha256 = mutation_request_sha256(
            "submission.review.begin.v1",
            &[submission_id, &expected_version.to_string()],
        );
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            &identity.reviewer_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::OK.as_u16() as i16 =>
            {
                let submission = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(submission);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }

        let mut stored = load_review_submission(&mut transaction, submission_id, true).await?;
        if stored.response.resource_version != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        let (assignment_kind, before_state) = match stored.response.state {
            SubmissionState::ReadyForReview => ("primary", "ready-for-review"),
            SubmissionState::PendingSecondaryReview => {
                let primary_reviewer: String = sqlx::query_scalar(
                    "SELECT reviewer_id FROM review_assignments \
                     WHERE submission_id = $1 AND assignment_kind = 'primary' \
                       AND state = 'completed'",
                )
                .bind(submission_id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(TxError::Sql)?
                .ok_or_else(ApiError::invalid_transition)?;
                if primary_reviewer == identity.reviewer_id {
                    return Err(ApiError::forbidden().into());
                }
                ("secondary", "pending-secondary-review")
            }
            _ => return Err(ApiError::invalid_transition().into()),
        };
        let resource_version = expected_version
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        sqlx::query(
            "INSERT INTO review_assignments (assignment_id, submission_id, reviewer_id, \
             assignment_kind, state, source_resource_version, created_unix_seconds) \
             VALUES ($1, $2, $3, $4, 'active', $5, $6)",
        )
        .bind(prefixed_uuid("assignment_"))
        .bind(submission_id)
        .bind(&identity.reviewer_id)
        .bind(assignment_kind)
        .bind(i64::try_from(expected_version).map_err(|_| ApiError::internal())?)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        sqlx::query(
            "UPDATE submissions SET state = 'in-review', resource_version = $1 \
             WHERE submission_id = $2",
        )
        .bind(i64::try_from(resource_version).map_err(|_| ApiError::internal())?)
        .bind(submission_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        stored.response.state = SubmissionState::InReview;
        stored.response.resource_version = resource_version;
        let response_body =
            serde_json::to_value(&stored.response).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.reviewer_id,
            key_sha256,
            StatusCode::OK,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.reviewer_id,
                action: "submission.review-begun",
                topic: "submission.review-begun",
                object_kind: "submission",
                object_id: submission_id,
                before_state: Some(before_state),
                after_state: Some("in-review"),
                resource_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "submission_id": submission_id,
                    "reviewer_id": identity.reviewer_id,
                    "assignment_kind": assignment_kind
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(stored.response)
    }

    #[allow(clippy::too_many_arguments)]
    async fn decide_review(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        submission_id: &str,
        expected_version: u64,
        request: &ReviewDecisionRequest,
    ) -> Result<SubmissionResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .decide_review_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    submission_id,
                    expected_version,
                    request,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(submission) => return Ok(submission),
            }
        }
        Err(ApiError::unavailable())
    }

    #[allow(clippy::too_many_arguments)]
    async fn decide_review_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        submission_id: &str,
        expected_version: u64,
        request: &ReviewDecisionRequest,
    ) -> Result<SubmissionResponse, TxError> {
        validate_review_decision(request)?;
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate_reviewer(&mut transaction, token_sha256).await?;
        require_reviewer_write(&identity)?;
        let reason_codes = request.reason_codes.join("\0");
        let request_sha256 = mutation_request_sha256(
            "submission.review.decision.v1",
            &[
                submission_id,
                &expected_version.to_string(),
                &request.decision,
                &reason_codes,
                &request.note,
            ],
        );
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            &identity.reviewer_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::CREATED.as_u16() as i16 =>
            {
                let submission = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(submission);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }

        let mut stored = load_review_submission(&mut transaction, submission_id, true).await?;
        if stored.response.resource_version != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        if stored.response.state != SubmissionState::InReview {
            return Err(ApiError::invalid_transition().into());
        }
        let assignment = sqlx::query(
            "SELECT assignment_id, assignment_kind FROM review_assignments \
             WHERE submission_id = $1 AND reviewer_id = $2 AND state = 'active' FOR UPDATE",
        )
        .bind(submission_id)
        .bind(&identity.reviewer_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::forbidden)?;
        let assignment_id: String = assignment.get("assignment_id");
        let assignment_kind: String = assignment.get("assignment_kind");
        let next_state = match (assignment_kind.as_str(), request.decision.as_str()) {
            ("primary", "approved") => SubmissionState::PendingSecondaryReview,
            ("secondary", "approved") => SubmissionState::Approved,
            (_, decision) => review_decision_state(decision)?,
        };
        let decision_id = prefixed_uuid("decision_");
        sqlx::query(
            "INSERT INTO review_decisions (decision_id, submission_id, reviewer_id, decision, \
             reason_codes, note, created_unix_seconds, assignment_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&decision_id)
        .bind(submission_id)
        .bind(&identity.reviewer_id)
        .bind(&request.decision)
        .bind(&request.reason_codes)
        .bind(&request.note)
        .bind(now)
        .bind(&assignment_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        sqlx::query(
            "UPDATE review_assignments SET state = 'completed', completed_unix_seconds = $1 \
             WHERE assignment_id = $2",
        )
        .bind(now)
        .bind(&assignment_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        let resource_version = expected_version
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        sqlx::query(
            "UPDATE submissions SET state = $1, resource_version = $2 WHERE submission_id = $3",
        )
        .bind(next_state.as_str())
        .bind(i64::try_from(resource_version).map_err(|_| ApiError::internal())?)
        .bind(submission_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        stored.response.state = next_state;
        stored.response.resource_version = resource_version;
        let response_body =
            serde_json::to_value(&stored.response).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.reviewer_id,
            key_sha256,
            StatusCode::CREATED,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.reviewer_id,
                action: "submission.review-decided",
                topic: "submission.review-decided",
                object_kind: "submission",
                object_id: submission_id,
                before_state: Some("in-review"),
                after_state: Some(next_state.as_str()),
                resource_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "decision_id": decision_id,
                    "submission_id": submission_id,
                    "reviewer_id": identity.reviewer_id,
                    "assignment_kind": assignment_kind,
                    "decision": request.decision,
                    "reason_codes": request.reason_codes
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(stored.response)
    }

    async fn create_release(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        request: &CreateReleaseRequest,
    ) -> Result<ReleaseResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .create_release_once(&token_sha256, &key_sha256, request_id, request)
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(release) => return Ok(release),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn create_release_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        request: &CreateReleaseRequest,
    ) -> Result<ReleaseResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        require_release_write(&identity)?;
        let rollout_percent = request.rollout_percent.to_string();
        let request_sha256 = mutation_request_sha256(
            "release.create.v1",
            &[&request.submission_id, &rollout_percent],
        );
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::CREATED.as_u16() as i16 =>
            {
                let release = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(release);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }

        let submission = load_submission(
            &mut transaction,
            &request.submission_id,
            &identity.team_id,
            true,
        )
        .await?;
        if submission.response.state != SubmissionState::Approved {
            return Err(ApiError::invalid_transition().into());
        }

        let release = ReleaseResponse {
            release_id: prefixed_uuid("rel_"),
            submission_id: submission.response.submission_id,
            app_id: submission.response.app_id,
            version: submission.response.version,
            state: ReleaseState::Ready,
            rollout_percent: request.rollout_percent,
            scheduled_unix_seconds: None,
            catalog_sequence: None,
            resource_version: 1,
        };
        let inserted = sqlx::query(
            "INSERT INTO releases (release_id, submission_id, app_id, version, state, \
             rollout_percent, scheduled_unix_seconds, catalog_sequence, resource_version, \
             created_unix_seconds) VALUES ($1, $2, $3, $4, 'ready', $5, NULL, NULL, 1, $6) \
             ON CONFLICT (submission_id) DO NOTHING",
        )
        .bind(&release.release_id)
        .bind(&release.submission_id)
        .bind(&release.app_id)
        .bind(&release.version)
        .bind(i16::from(release.rollout_percent))
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .rows_affected();
        if inserted != 1 {
            return Err(ApiError::conflict().into());
        }

        let response_body = serde_json::to_value(&release).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            StatusCode::CREATED,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.member_id,
                action: "release.created",
                topic: "release.created",
                object_kind: "release",
                object_id: &release.release_id,
                before_state: None,
                after_state: Some(ReleaseState::Ready.as_str()),
                resource_version: release.resource_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "release_id": release.release_id,
                    "submission_id": release.submission_id,
                    "app_id": release.app_id,
                    "version": release.version,
                    "rollout_percent": release.rollout_percent
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(release)
    }

    async fn get_release(
        &self,
        token: &str,
        release_id: &str,
    ) -> Result<ReleaseResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate(&mut transaction, &token_sha256)
            .await
            .map_err(ApiError::from_transaction)?;
        require_release_read(&identity)?;
        let release = load_release(&mut transaction, release_id, &identity.team_id, false)
            .await
            .map_err(ApiError::from_transaction)?;
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;
        Ok(release)
    }

    #[allow(clippy::too_many_arguments)]
    async fn mutate_release(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        release_id: &str,
        expected_version: u64,
        action: &ReleaseAction,
    ) -> Result<ReleaseResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .mutate_release_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    release_id,
                    expected_version,
                    action,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(release) => return Ok(release),
            }
        }
        Err(ApiError::unavailable())
    }

    #[allow(clippy::too_many_arguments)]
    async fn mutate_release_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        release_id: &str,
        expected_version: u64,
        action: &ReleaseAction,
    ) -> Result<ReleaseResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        require_release_write(&identity)?;
        let request_sha256 = release_mutation_request_sha256(release_id, expected_version, action);
        let now = database_now(&mut transaction).await?;
        let response_status = action.response_status();
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == response_status.as_u16() as i16 =>
            {
                let release = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(release);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }
        if matches!(
            action,
            ReleaseAction::Schedule {
                publish_unix_seconds
            } if *publish_unix_seconds <= u64::try_from(now).map_err(|_| ApiError::internal())?
        ) {
            return Err(ApiError::invalid_request().into());
        }

        let mut release =
            load_release(&mut transaction, release_id, &identity.team_id, true).await?;
        if release.resource_version != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        let (target, event_action, topic) = release_action_transition(action);
        if !release.state.can_transition_to(target) {
            return Err(ApiError::invalid_transition().into());
        }
        let before = release.state;
        release.state = target;
        release.scheduled_unix_seconds = match action {
            ReleaseAction::Schedule {
                publish_unix_seconds,
            } => Some(*publish_unix_seconds),
            _ => None,
        };
        release.resource_version = expected_version
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        sqlx::query(
            "UPDATE releases SET state = $1, scheduled_unix_seconds = $2, resource_version = $3 \
             WHERE release_id = $4",
        )
        .bind(release.state.as_str())
        .bind(
            release
                .scheduled_unix_seconds
                .map(i64::try_from)
                .transpose()
                .map_err(|_| ApiError::invalid_request())?,
        )
        .bind(i64::try_from(release.resource_version).map_err(|_| ApiError::internal())?)
        .bind(release_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        let details = match action {
            ReleaseAction::Schedule {
                publish_unix_seconds,
            } => json!({"publish_unix_seconds": publish_unix_seconds}),
            ReleaseAction::Remove { reason_code, note } => {
                json!({"reason_code": reason_code, "note": note})
            }
            _ => json!({}),
        };
        sqlx::query(
            "INSERT INTO release_operations (operation_id, release_id, actor_id, action, \
             before_state, after_state, resource_version, request_sha256, details, \
             created_unix_seconds) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(prefixed_uuid("releaseop_"))
        .bind(release_id)
        .bind(&identity.member_id)
        .bind(action.name())
        .bind(before.as_str())
        .bind(release.state.as_str())
        .bind(i64::try_from(release.resource_version).map_err(|_| ApiError::internal())?)
        .bind(&request_sha256)
        .bind(details)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        let response_body = serde_json::to_value(&release).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            response_status,
            &response_body,
        )
        .await?;
        let mut payload = json!({
            "release_id": release.release_id,
            "app_id": release.app_id,
            "version": release.version,
            "state": release.state
        });
        if let ReleaseAction::Schedule {
            publish_unix_seconds,
        } = action
        {
            payload["publish_unix_seconds"] = json!(publish_unix_seconds);
        }
        if let ReleaseAction::Remove { reason_code, .. } = action {
            payload["reason_code"] = json!(reason_code);
        }
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.member_id,
                action: event_action,
                topic,
                object_kind: "release",
                object_id: release_id,
                before_state: Some(before.as_str()),
                after_state: Some(release.state.as_str()),
                resource_version: release.resource_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload,
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(release)
    }

    async fn get_today_editorial(&self, token: &str) -> Result<EditorialLayoutResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate_store_operator(&mut transaction, &token_sha256)
            .await
            .map_err(ApiError::from_transaction)?;
        require_editorial_access(&identity)?;
        let layout = load_editorial_layout(&mut transaction, false)
            .await
            .map_err(ApiError::from_transaction)?;
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;
        Ok(layout)
    }

    async fn list_editorial_releases(
        &self,
        token: &str,
        cursor: Option<EditorialReleaseCursor>,
        limit: usize,
    ) -> Result<EditorialReleaseListResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate_store_operator(&mut transaction, &token_sha256)
            .await
            .map_err(ApiError::from_transaction)?;
        require_editorial_access(&identity)?;
        let (after_sequence, after_release_id) = cursor
            .map(|cursor| (cursor.catalog_sequence, cursor.release_id))
            .unwrap_or((0, String::new()));
        let rows = sqlx::query(
            "SELECT release.release_id, release.app_id, release.version, \
             artifact.catalog_sequence, artifact.catalog_app \
             FROM releases release \
             JOIN submissions submission ON submission.submission_id = release.submission_id \
             JOIN store_package_artifacts artifact ON artifact.release_id = release.release_id \
             WHERE release.state = 'published' AND submission.state = 'approved' \
               AND submission.app_id = release.app_id AND submission.version = release.version \
               AND release.catalog_sequence = artifact.catalog_sequence \
               AND (artifact.catalog_sequence, release.release_id) > ($1, $2) \
               AND NOT EXISTS ( \
                 SELECT 1 FROM store_package_artifacts newer_artifact \
                 JOIN releases newer_release \
                   ON newer_release.release_id = newer_artifact.release_id \
                 JOIN submissions newer_submission \
                   ON newer_submission.submission_id = newer_release.submission_id \
                 WHERE newer_release.app_id = release.app_id \
                   AND newer_release.state = 'published' \
                   AND newer_submission.state = 'approved' \
                   AND newer_submission.app_id = newer_release.app_id \
                   AND newer_submission.version = newer_release.version \
                   AND newer_release.catalog_sequence = newer_artifact.catalog_sequence \
                   AND newer_artifact.catalog_sequence > artifact.catalog_sequence \
               ) \
             ORDER BY artifact.catalog_sequence, release.release_id LIMIT $3",
        )
        .bind(after_sequence)
        .bind(after_release_id)
        .bind(i64::try_from(limit + 1).map_err(|_| ApiError::invalid_request())?)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;

        let has_more = rows.len() > limit;
        let mut items = rows
            .iter()
            .take(limit)
            .map(|row| {
                let release_id: String = row.get("release_id");
                let app_id: String = row.get("app_id");
                let version: String = row.get("version");
                let catalog_sequence = u64::try_from(row.get::<i64, _>("catalog_sequence"))
                    .map_err(|_| ApiError::internal())?;
                let catalog: CatalogApp = serde_json::from_value(row.get("catalog_app"))
                    .map_err(|_| ApiError::internal())?;
                catalog.validate().map_err(|_| ApiError::internal())?;
                if catalog.app_id != app_id || catalog.version != version {
                    return Err(ApiError::internal());
                }
                Ok(EditorialReleaseResponse {
                    release_id,
                    app_id,
                    name: catalog.name,
                    version,
                    category: catalog.discovery.map(|discovery| discovery.category),
                    catalog_sequence,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = has_more.then(|| items.last()).flatten().map(|release| {
            encode_editorial_release_cursor(release.catalog_sequence, &release.release_id)
        });
        items.shrink_to_fit();
        Ok(EditorialReleaseListResponse { items, next_cursor })
    }

    async fn replace_today_editorial(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        expected_version: Option<u64>,
        request: &EditorialLayoutRequest,
    ) -> Result<EditorialLayoutResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .replace_today_editorial_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    expected_version,
                    request,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(layout) => return Ok(layout),
            }
        }
        Err(ApiError::unavailable())
    }

    #[allow(clippy::too_many_arguments)]
    async fn replace_today_editorial_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        expected_version: Option<u64>,
        request: &EditorialLayoutRequest,
    ) -> Result<EditorialLayoutResponse, TxError> {
        validate_editorial_request(request)?;
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate_store_operator(&mut transaction, token_sha256).await?;
        require_editorial_access(&identity)?;
        let request_sha256 = editorial_request_sha256(expected_version, request)?;
        let now = database_now(&mut transaction).await?;
        let response_status = if expected_version.is_some() {
            StatusCode::OK
        } else {
            StatusCode::CREATED
        };
        match reserve_idempotency(
            &mut transaction,
            &identity.operator_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == response_status.as_u16() as i16 =>
            {
                let layout = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(layout);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }

        let current = sqlx::query_scalar::<_, i64>(
            "SELECT resource_version FROM store_editorial_layouts \
             WHERE layout_id = 'today' FOR UPDATE",
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .map(|version| u64::try_from(version).map_err(|_| ApiError::internal()))
        .transpose()?;
        let resource_version = match (expected_version, current) {
            (None, None) => 1,
            (None, Some(_)) => return Err(ApiError::conflict().into()),
            (Some(_), None) => return Err(ApiError::not_found().into()),
            (Some(expected), Some(current)) if expected != current => {
                return Err(ApiError::precondition_failed().into());
            }
            (Some(expected), Some(_)) => expected.checked_add(1).ok_or_else(ApiError::internal)?,
        };
        let resolved = resolve_editorial_layout(&mut transaction, request).await?;
        let layout = EditorialLayoutResponse {
            layout_id: "today".into(),
            headline: request.headline.clone(),
            featured: resolved.featured.clone(),
            collections: resolved.collections.clone(),
            resource_version,
            updated_unix_seconds: u64::try_from(now).map_err(|_| ApiError::internal())?,
        };
        if expected_version.is_none() {
            sqlx::query(
                "INSERT INTO store_editorial_layouts (layout_id, headline, featured_release_id, \
                 featured_app_id, collections, resource_version, created_unix_seconds, \
                 updated_unix_seconds) VALUES ('today', $1, $2, $3, $4, 1, $5, $5)",
            )
            .bind(&layout.headline)
            .bind(&layout.featured.release_id)
            .bind(&layout.featured.app_id)
            .bind(&resolved.collections_json)
            .bind(now)
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;
        } else {
            sqlx::query(
                "UPDATE store_editorial_layouts SET headline = $1, featured_release_id = $2, \
                 featured_app_id = $3, collections = $4, resource_version = $5, \
                 updated_unix_seconds = $6 WHERE layout_id = 'today'",
            )
            .bind(&layout.headline)
            .bind(&layout.featured.release_id)
            .bind(&layout.featured.app_id)
            .bind(&resolved.collections_json)
            .bind(i64::try_from(resource_version).map_err(|_| ApiError::internal())?)
            .bind(now)
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;
        }
        sqlx::query(
            "INSERT INTO store_editorial_revisions (layout_id, resource_version, operator_id, \
             headline, featured_release_id, featured_app_id, collections, request_sha256, \
             created_unix_seconds) VALUES ('today', $1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(i64::try_from(resource_version).map_err(|_| ApiError::internal())?)
        .bind(&identity.operator_id)
        .bind(&layout.headline)
        .bind(&layout.featured.release_id)
        .bind(&layout.featured.app_id)
        .bind(&resolved.collections_json)
        .bind(&request_sha256)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        let response_body = serde_json::to_value(&layout).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.operator_id,
            key_sha256,
            response_status,
            &response_body,
        )
        .await?;
        append_editorial_mutation(
            &mut transaction,
            EditorialMutationEvent {
                now,
                operator_id: &identity.operator_id,
                action: if expected_version.is_some() {
                    "editorial.today-updated"
                } else {
                    "editorial.today-created"
                },
                resource_version,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                featured_release_id: &layout.featured.release_id,
                featured_app_id: &layout.featured.app_id,
                featured_release_version: resolved.featured_release_version,
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(layout)
    }

    async fn post_review_message(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        submission_id: &str,
        request: &ReviewMessageRequest,
    ) -> Result<ReviewMessageResponse, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .post_review_message_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    submission_id,
                    request,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(message) => return Ok(message),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn post_review_message_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        submission_id: &str,
        request: &ReviewMessageRequest,
    ) -> Result<ReviewMessageResponse, TxError> {
        validate_review_text(&request.body, false)?;
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate_message_actor(&mut transaction, token_sha256).await?;
        match &identity {
            MessageIdentity::Developer(developer) => require_developer_write(developer)?,
            MessageIdentity::Reviewer(reviewer) => require_reviewer_write(reviewer)?,
        }
        let request_sha256 = mutation_request_sha256(
            "submission.review.message.v1",
            &[submission_id, &request.body],
        );
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            identity.actor_id(),
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::CREATED.as_u16() as i16 =>
            {
                let message = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(message);
            }
            IdempotencyReservation::Replay { .. } => {
                return Err(ApiError::internal().into());
            }
        }

        let stored = match &identity {
            MessageIdentity::Developer(developer) => {
                load_submission(&mut transaction, submission_id, &developer.team_id, true).await?
            }
            MessageIdentity::Reviewer(reviewer) => {
                let stored = load_review_submission(&mut transaction, submission_id, true).await?;
                let assigned: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM review_assignments \
                     WHERE submission_id = $1 AND reviewer_id = $2)",
                )
                .bind(submission_id)
                .bind(&reviewer.reviewer_id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(TxError::Sql)?;
                if !assigned {
                    return Err(ApiError::forbidden().into());
                }
                stored
            }
        };
        if !matches!(
            stored.response.state,
            SubmissionState::ReadyForReview
                | SubmissionState::InReview
                | SubmissionState::PendingSecondaryReview
                | SubmissionState::NeedsChanges
                | SubmissionState::Approved
                | SubmissionState::Rejected
        ) {
            return Err(ApiError::invalid_transition().into());
        }

        let message = ReviewMessageResponse {
            message_id: prefixed_uuid("msg_"),
            submission_id: submission_id.to_owned(),
            actor_id: identity.actor_id().to_owned(),
            body: request.body.clone(),
            created_unix_seconds: u64::try_from(now).map_err(|_| ApiError::internal())?,
        };
        sqlx::query(
            "INSERT INTO review_messages (message_id, submission_id, actor_id, actor_kind, body, \
             created_unix_seconds) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&message.message_id)
        .bind(submission_id)
        .bind(&message.actor_id)
        .bind(identity.actor_kind())
        .bind(&message.body)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        let response_body = serde_json::to_value(&message).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            identity.actor_id(),
            key_sha256,
            StatusCode::CREATED,
            &response_body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: identity.actor_id(),
                action: "review-message.created",
                topic: "review-message.created",
                object_kind: "review-message",
                object_id: &message.message_id,
                before_state: None,
                after_state: None,
                resource_version: 1,
                request_id,
                request_sha256: &request_sha256,
                key_sha256,
                payload: json!({
                    "message_id": message.message_id,
                    "submission_id": submission_id,
                    "actor_id": message.actor_id,
                    "actor_kind": identity.actor_kind()
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(message)
    }

    async fn ingest_aggregate_metrics(
        &self,
        report: &AggregateMetricsReport,
    ) -> Result<(), ApiError> {
        report.validate().map_err(|_| ApiError::invalid_request())?;
        let encoded = encode_report(report).map_err(|_| ApiError::invalid_request())?;
        let report_sha256 = sha256_hex(&encoded);
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let now = database_now(&mut transaction)
            .await
            .map_err(ApiError::from_transaction)?;
        sqlx::query("DELETE FROM store_metric_batches WHERE expires_unix_seconds <= $1")
            .bind(now)
            .execute(&mut *transaction)
            .await
            .map_err(|_| ApiError::unavailable())?;

        let existing = sqlx::query_scalar::<_, String>(
            "SELECT report_sha256 FROM store_metric_batches WHERE batch_id = $1",
        )
        .bind(&report.batch_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        if let Some(existing) = existing {
            if existing != report_sha256 {
                return Err(ApiError::idempotency_conflict());
            }
            transaction
                .commit()
                .await
                .map_err(|_| ApiError::unavailable())?;
            return Ok(());
        }

        let now = u64::try_from(now).map_err(|_| ApiError::internal())?;
        let current_week = week_start(now);
        if report.week_start_unix_seconds.checked_add(WEEK_SECONDS) != Some(current_week) {
            return Err(ApiError::invalid_request());
        }
        for record in &report.records {
            let published: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM store_package_artifacts artifact \
                 JOIN releases release ON release.release_id = artifact.release_id \
                 WHERE release.app_id = $1 AND release.version = $2 \
                   AND artifact.catalog_app->>'app_id' = $1 \
                   AND artifact.catalog_app->>'version' = $2)",
            )
            .bind(&record.app_id)
            .bind(&record.version)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| ApiError::unavailable())?;
            if !published {
                return Err(ApiError::invalid_request());
            }
        }

        let inserted = sqlx::query(
            "INSERT INTO store_metric_batches (batch_id, week_start_unix_seconds, \
             report_sha256, received_unix_seconds, expires_unix_seconds) \
             VALUES ($1, $2, $3, $4, $4 + $5) ON CONFLICT (batch_id) DO NOTHING",
        )
        .bind(&report.batch_id)
        .bind(
            i64::try_from(report.week_start_unix_seconds)
                .map_err(|_| ApiError::invalid_request())?,
        )
        .bind(&report_sha256)
        .bind(i64::try_from(now).map_err(|_| ApiError::internal())?)
        .bind(METRICS_BATCH_TTL_SECONDS)
        .execute(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        if inserted.rows_affected() == 0 {
            let existing = sqlx::query_scalar::<_, String>(
                "SELECT report_sha256 FROM store_metric_batches WHERE batch_id = $1",
            )
            .bind(&report.batch_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| ApiError::unavailable())?;
            if existing != report_sha256 {
                return Err(ApiError::idempotency_conflict());
            }
            transaction
                .commit()
                .await
                .map_err(|_| ApiError::unavailable())?;
            return Ok(());
        }

        for record in &report.records {
            sqlx::query(
                "INSERT INTO store_metric_aggregates (week_start_unix_seconds, app_id, version, \
                 batch_count, install_count, launch_count, crash_count, updated_unix_seconds) \
                 VALUES ($1, $2, $3, 1, $4, $5, $6, $7) \
                 ON CONFLICT (week_start_unix_seconds, app_id, version) DO UPDATE SET \
                 batch_count = store_metric_aggregates.batch_count + 1, \
                 install_count = store_metric_aggregates.install_count + EXCLUDED.install_count, \
                 launch_count = store_metric_aggregates.launch_count + EXCLUDED.launch_count, \
                 crash_count = store_metric_aggregates.crash_count + EXCLUDED.crash_count, \
                 updated_unix_seconds = EXCLUDED.updated_unix_seconds",
            )
            .bind(
                i64::try_from(report.week_start_unix_seconds)
                    .map_err(|_| ApiError::invalid_request())?,
            )
            .bind(&record.app_id)
            .bind(&record.version)
            .bind(i64::from(record.installs))
            .bind(i64::from(record.launches))
            .bind(i64::from(record.crashes))
            .bind(i64::try_from(now).map_err(|_| ApiError::internal())?)
            .execute(&mut *transaction)
            .await
            .map_err(|_| ApiError::unavailable())?;
        }
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())
    }
}

pub async fn connect(database_url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await
}

/// Applies the App, Submission, scanner, review, and Release control schemas.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!().run(pool).await
}

pub fn router(service: StoreControlService) -> Router {
    Router::new()
        .route(
            "/reports/v1/content",
            post(moderation::post_content_report).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/metrics/v1/aggregate",
            post(post_aggregate_metrics).layer(DefaultBodyLimit::max(MAX_METRICS_REPORT_BYTES)),
        )
        .route(
            "/oauth/device/code",
            post(post_device_code).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/oauth/device/authorize",
            post(post_device_authorization).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/oauth/token",
            post(post_device_token).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route("/v1/teams/{team_id}", get(get_team))
        .route(
            "/v1/teams/{team_id}/members/{member_action}",
            post(mutate_team_member).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/v1/apps",
            post(post_app).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route("/v1/apps/{app_id}", get(get_app))
        .route(
            "/v1/apps/{app_id}/submissions",
            post(post_submission).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/v1/submissions/{submission_action}",
            get(get_submission)
                .post(mutate_submission)
                .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/v1/submissions/{submission_id}/parts/{part_name}",
            put(put_submission_part).layer(DefaultBodyLimit::max(MAX_UPLOAD_CHUNK_BYTES)),
        )
        .route(
            "/v1/submissions/{submission_id}/messages",
            post(post_review_message).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route("/v1/review/submissions", get(list_review_queue))
        .route(
            "/v1/review/submissions/{submission_action}",
            get(get_review_submission_detail)
                .post(begin_review)
                .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/v1/review/submissions/{submission_id}/decisions",
            post(decide_review).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/v1/releases",
            post(post_release).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/v1/releases/{release_action}",
            get(get_release)
                .post(mutate_release)
                .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route("/v1/editorial/releases", get(list_editorial_releases))
        .route(
            "/v1/editorial/today",
            get(get_today_editorial)
                .post(post_today_editorial)
                .put(put_today_editorial)
                .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/v1/moderation/reports",
            get(moderation::list_moderation_reports),
        )
        .route(
            "/v1/moderation/reports/{report_action}",
            post(moderation::decide_content_report).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/v1/apps/{app_id}/moderation-notices",
            get(moderation::list_developer_notices),
        )
        .route(
            "/v1/moderation/notices/{notice_action}",
            post(moderation::appeal_notice).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/v1/moderation/appeals/{appeal_action}",
            post(moderation::decide_appeal).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(fallback)
        .with_state(service)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceCodeRequest {
    client_id: String,
    scope: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct MetricsAcceptedResponse {
    accepted: bool,
    batch_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: &'static str,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceAuthorizationDecisionRequest {
    user_code: String,
    decision: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceTokenRequest {
    grant_type: String,
    device_code: String,
    client_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceTokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: u64,
    scope: &'static str,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetTeamMemberRoleRequest {
    role: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TeamMemberStateAction {
    Suspend,
    Restore,
}

impl TeamMemberStateAction {
    const fn before_state(self) -> &'static str {
        match self {
            Self::Suspend => "active",
            Self::Restore => "suspended",
        }
    }

    const fn after_state(self) -> &'static str {
        match self {
            Self::Suspend => "suspended",
            Self::Restore => "active",
        }
    }

    const fn request_domain(self) -> &'static str {
        match self {
            Self::Suspend => "team.member-suspend.v1",
            Self::Restore => "team.member-restore.v1",
        }
    }

    const fn event_name(self) -> &'static str {
        match self {
            Self::Suspend => "team.member-suspended",
            Self::Restore => "team.member-restored",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamMemberResponse {
    member_id: String,
    email: String,
    role: String,
    membership_state: String,
    two_factor_enabled: bool,
    resource_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamResponse {
    team_id: String,
    name: String,
    members: Vec<TeamMemberResponse>,
    resource_version: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateAppRequest {
    app_id: String,
    default_locale: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSubmissionRequest {
    version: String,
    package_sha256: String,
    package_bytes: u64,
    listing_sha256: String,
    listing_bytes: u64,
    assets: Vec<ImageAsset>,
}

impl CreateSubmissionRequest {
    fn spec(&self) -> SubmissionSpec {
        SubmissionSpec {
            version: self.version.clone(),
            package_sha256: self.package_sha256.clone(),
            package_bytes: self.package_bytes,
            listing_sha256: self.listing_sha256.clone(),
            listing_bytes: self.listing_bytes,
            assets: self.assets.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FinalizeSubmissionRequest {
    content_sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewMessageRequest {
    body: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewDecisionRequest {
    decision: String,
    reason_codes: Vec<String>,
    note: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewQueueQuery {
    cursor: Option<String>,
    limit: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditorialReleaseQuery {
    cursor: Option<String>,
    limit: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateReleaseRequest {
    submission_id: String,
    rollout_percent: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleReleaseRequest {
    publish_unix_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemovalRequest {
    reason_code: String,
    note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditorialCollectionRequest {
    title: String,
    release_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditorialLayoutRequest {
    headline: String,
    featured_release_id: String,
    collections: Vec<EditorialCollectionRequest>,
}

struct ReviewCursor {
    created_unix_seconds: i64,
    submission_id: String,
}

struct EditorialReleaseCursor {
    catalog_sequence: i64,
    release_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmissionResponse {
    submission_id: String,
    app_id: String,
    version: String,
    revision: u32,
    state: SubmissionState,
    package_sha256: String,
    listing_sha256: String,
    assets: Vec<ImageAsset>,
    resource_version: u64,
    created_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewMessageResponse {
    message_id: String,
    submission_id: String,
    actor_id: String,
    body: String,
    created_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReviewQueueResponse {
    items: Vec<ReviewQueueItemResponse>,
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewQueueItemResponse {
    submission: SubmissionResponse,
    app: ReviewAppResponse,
    review_stage: String,
    assigned_to_caller: bool,
    risk: RiskAssessment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewAppResponse {
    name: String,
    developer_name: String,
    category: StoreCategory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewScanResponse {
    scan_id: String,
    scanner_version: String,
    report_sha256: String,
    developer_key_sha256: Option<String>,
    imports: Vec<String>,
    permissions: Vec<String>,
    findings: Vec<ScanFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewAssignmentResponse {
    assignment_id: String,
    reviewer_id: String,
    reviewer_email: String,
    reviewer_role: String,
    assignment_kind: String,
    state: String,
    created_unix_seconds: u64,
    completed_unix_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewDecisionRecordResponse {
    decision_id: String,
    reviewer_id: String,
    reviewer_email: String,
    decision: String,
    reason_codes: Vec<String>,
    note: String,
    created_unix_seconds: u64,
    assignment_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewDetailMessageResponse {
    message_id: String,
    actor_id: String,
    actor_kind: String,
    actor_label: String,
    body: String,
    created_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewAuditResponse {
    sequence: u64,
    occurred_unix_seconds: u64,
    actor_id: String,
    action: String,
    before_state: Option<String>,
    after_state: Option<String>,
    resource_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewSubmissionDetailResponse {
    submission: SubmissionResponse,
    app: ReviewAppResponse,
    review_stage: String,
    assigned_to_caller: bool,
    risk: RiskAssessment,
    scan: ReviewScanResponse,
    assignments: Vec<ReviewAssignmentResponse>,
    decisions: Vec<ReviewDecisionRecordResponse>,
    messages: Vec<ReviewDetailMessageResponse>,
    messages_truncated: bool,
    audit: Vec<ReviewAuditResponse>,
    audit_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseResponse {
    release_id: String,
    submission_id: String,
    app_id: String,
    version: String,
    state: ReleaseState,
    rollout_percent: u8,
    scheduled_unix_seconds: Option<u64>,
    catalog_sequence: Option<u64>,
    resource_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditorialItemResponse {
    release_id: String,
    app_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditorialCollectionResponse {
    title: String,
    items: Vec<EditorialItemResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditorialLayoutResponse {
    layout_id: String,
    headline: String,
    featured: EditorialItemResponse,
    collections: Vec<EditorialCollectionResponse>,
    resource_version: u64,
    updated_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct EditorialReleaseResponse {
    release_id: String,
    app_id: String,
    name: String,
    version: String,
    category: Option<StoreCategory>,
    catalog_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct EditorialReleaseListResponse {
    items: Vec<EditorialReleaseResponse>,
    next_cursor: Option<String>,
}

struct ResolvedEditorialLayout {
    featured: EditorialItemResponse,
    featured_release_version: u64,
    collections: Vec<EditorialCollectionResponse>,
    collections_json: Value,
}

enum ReleaseAction {
    Schedule { publish_unix_seconds: u64 },
    Publish,
    Pause,
    Resume,
    Remove { reason_code: String, note: String },
}

impl ReleaseAction {
    const fn name(&self) -> &'static str {
        match self {
            Self::Schedule { .. } => "schedule",
            Self::Publish => "publish",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Remove { .. } => "remove",
        }
    }

    const fn response_status(&self) -> StatusCode {
        match self {
            Self::Publish => StatusCode::ACCEPTED,
            _ => StatusCode::OK,
        }
    }
}

struct StoredSubmission {
    response: SubmissionResponse,
    package_bytes: u64,
    listing_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
struct UploadRange {
    start: u64,
    end: u64,
    total: u64,
}

#[derive(Clone, Copy)]
struct UploadMutation<'a> {
    submission_id: &'a str,
    part_name: &'a str,
    expected_version: u64,
    range: UploadRange,
    chunk_sha256: &'a str,
    body: &'a [u8],
}

struct UploadPart {
    name: String,
    expected_sha256: String,
    expected_bytes: u64,
    received_bytes: u64,
}

struct StoredChunk {
    offset: u64,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct Problem {
    r#type: String,
    title: &'static str,
    status: u16,
    code: &'static str,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
enum ApiErrorKind {
    InvalidRequest,
    AuthorizationPending,
    SlowDown,
    AccessDenied,
    ExpiredToken,
    Unauthorized,
    Forbidden,
    TwoFactorRequired,
    StepUpRequired,
    NotFound,
    Conflict,
    IdempotencyConflict,
    PayloadTooLarge,
    MethodNotAllowed,
    PreconditionFailed,
    InvalidTransition,
    UploadRangeConflict,
    DigestMismatch,
    Internal,
    Unavailable,
}

#[derive(Debug)]
struct ApiError {
    kind: ApiErrorKind,
}

impl ApiError {
    const fn invalid_request() -> Self {
        Self {
            kind: ApiErrorKind::InvalidRequest,
        }
    }

    const fn authorization_pending() -> Self {
        Self {
            kind: ApiErrorKind::AuthorizationPending,
        }
    }

    const fn slow_down() -> Self {
        Self {
            kind: ApiErrorKind::SlowDown,
        }
    }

    const fn access_denied() -> Self {
        Self {
            kind: ApiErrorKind::AccessDenied,
        }
    }

    const fn expired_token() -> Self {
        Self {
            kind: ApiErrorKind::ExpiredToken,
        }
    }

    const fn unauthorized() -> Self {
        Self {
            kind: ApiErrorKind::Unauthorized,
        }
    }

    const fn forbidden() -> Self {
        Self {
            kind: ApiErrorKind::Forbidden,
        }
    }

    const fn two_factor_required() -> Self {
        Self {
            kind: ApiErrorKind::TwoFactorRequired,
        }
    }

    const fn step_up_required() -> Self {
        Self {
            kind: ApiErrorKind::StepUpRequired,
        }
    }

    const fn not_found() -> Self {
        Self {
            kind: ApiErrorKind::NotFound,
        }
    }

    const fn conflict() -> Self {
        Self {
            kind: ApiErrorKind::Conflict,
        }
    }

    const fn idempotency_conflict() -> Self {
        Self {
            kind: ApiErrorKind::IdempotencyConflict,
        }
    }

    const fn payload_too_large() -> Self {
        Self {
            kind: ApiErrorKind::PayloadTooLarge,
        }
    }

    const fn method_not_allowed() -> Self {
        Self {
            kind: ApiErrorKind::MethodNotAllowed,
        }
    }

    const fn precondition_failed() -> Self {
        Self {
            kind: ApiErrorKind::PreconditionFailed,
        }
    }

    const fn invalid_transition() -> Self {
        Self {
            kind: ApiErrorKind::InvalidTransition,
        }
    }

    const fn upload_range_conflict() -> Self {
        Self {
            kind: ApiErrorKind::UploadRangeConflict,
        }
    }

    const fn digest_mismatch() -> Self {
        Self {
            kind: ApiErrorKind::DigestMismatch,
        }
    }

    const fn internal() -> Self {
        Self {
            kind: ApiErrorKind::Internal,
        }
    }

    const fn unavailable() -> Self {
        Self {
            kind: ApiErrorKind::Unavailable,
        }
    }

    fn from_transaction(error: TxError) -> Self {
        match error {
            TxError::Api(error) => error,
            TxError::Sql(_) => Self::unavailable(),
        }
    }

    fn response(self, request_id: String) -> Response {
        let (status, code, title, detail) = match self.kind {
            ApiErrorKind::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                "invalid-request",
                "Invalid request",
                Some("The request does not match the Store API contract."),
            ),
            ApiErrorKind::AuthorizationPending => (
                StatusCode::BAD_REQUEST,
                "authorization-pending",
                "Authorization pending",
                Some("The developer has not completed device authorization."),
            ),
            ApiErrorKind::SlowDown => (
                StatusCode::BAD_REQUEST,
                "slow-down",
                "Polling too quickly",
                Some("Increase the polling interval before retrying."),
            ),
            ApiErrorKind::AccessDenied => (
                StatusCode::BAD_REQUEST,
                "access-denied",
                "Authorization denied",
                Some("The device authorization was denied or is no longer permitted."),
            ),
            ApiErrorKind::ExpiredToken => (
                StatusCode::BAD_REQUEST,
                "expired-token",
                "Device code expired",
                Some("Start a new device authorization request."),
            ),
            ApiErrorKind::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authorization required",
                Some("The access token is missing, expired, revoked, or invalid."),
            ),
            ApiErrorKind::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "Operation not permitted",
                Some(
                    "Current team membership, role, or token scope does not allow this operation.",
                ),
            ),
            ApiErrorKind::TwoFactorRequired => (
                StatusCode::FORBIDDEN,
                "two-factor-required",
                "Two-factor authentication required",
                Some("Enable two-factor authentication before changing Store resources."),
            ),
            ApiErrorKind::StepUpRequired => (
                StatusCode::FORBIDDEN,
                "step-up-required",
                "Recent authentication required",
                Some("Complete a fresh multi-factor authentication challenge before retrying."),
            ),
            ApiErrorKind::NotFound => (
                StatusCode::NOT_FOUND,
                "not-found",
                "Resource not found",
                None,
            ),
            ApiErrorKind::Conflict => (
                StatusCode::CONFLICT,
                "conflict",
                "Resource conflict",
                Some("The requested resource conflicts with an existing immutable identity."),
            ),
            ApiErrorKind::IdempotencyConflict => (
                StatusCode::CONFLICT,
                "idempotency-conflict",
                "Idempotency conflict",
                Some("This idempotency key was already used for another request."),
            ),
            ApiErrorKind::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload-too-large",
                "Request body too large",
                Some("The request body exceeds the limit for this operation."),
            ),
            ApiErrorKind::MethodNotAllowed => (
                StatusCode::METHOD_NOT_ALLOWED,
                "method-not-allowed",
                "Method not allowed",
                None,
            ),
            ApiErrorKind::PreconditionFailed => (
                StatusCode::PRECONDITION_FAILED,
                "precondition-failed",
                "Resource version changed",
                Some("Read the resource again before retrying this operation."),
            ),
            ApiErrorKind::InvalidTransition => (
                StatusCode::CONFLICT,
                "invalid-transition",
                "Invalid state transition",
                Some("The resource is not in a state that allows this operation."),
            ),
            ApiErrorKind::UploadRangeConflict => (
                StatusCode::CONFLICT,
                "upload-range-conflict",
                "Upload range conflict",
                Some("Upload the next contiguous range with the current ETag."),
            ),
            ApiErrorKind::DigestMismatch => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "digest-mismatch",
                "Content digest mismatch",
                Some(
                    "Uploaded bytes or the finalized content digest do not match the declaration.",
                ),
            ),
            ApiErrorKind::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "Internal server error",
                None,
            ),
            ApiErrorKind::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service-unavailable",
                "Service temporarily unavailable",
                Some("Retry this request with the same idempotency key."),
            ),
        };
        let problem = Problem {
            r#type: format!("https://cardputerzero.dev/problems/{code}"),
            title,
            status: status.as_u16(),
            code,
            request_id,
            detail,
        };
        let response_request_id = problem.request_id.clone();
        let mut response = (status, Json(problem)).into_response();
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer realm=\"cardputerzero-store\""),
            );
        }
        if let Ok(value) = HeaderValue::from_str(&response_request_id) {
            response.headers_mut().insert("x-request-id", value);
        }
        response
    }
}

enum TxError {
    Api(ApiError),
    Sql(sqlx::Error),
}

enum IdempotencyReservation {
    Fresh,
    Replay { status: i16, body: Value },
}

impl From<ApiError> for TxError {
    fn from(value: ApiError) -> Self {
        Self::Api(value)
    }
}

#[derive(Debug)]
struct Identity {
    member_id: String,
    team_id: String,
    role: String,
    two_factor_enabled: bool,
    mfa_authenticated_unix_seconds: Option<i64>,
    scopes: Vec<String>,
}

impl Identity {
    fn has_any_scope(&self, expected: &[&str]) -> bool {
        self.scopes
            .iter()
            .any(|scope| expected.contains(&scope.as_str()))
    }
}

#[derive(Debug)]
struct ReviewerIdentity {
    reviewer_id: String,
    role: String,
    two_factor_enabled: bool,
    scopes: Vec<String>,
}

impl ReviewerIdentity {
    fn has_scope(&self, expected: &str) -> bool {
        self.scopes.iter().any(|scope| scope == expected)
    }
}

#[derive(Debug)]
struct StoreOperatorIdentity {
    operator_id: String,
    role: String,
    two_factor_enabled: bool,
    scopes: Vec<String>,
}

impl StoreOperatorIdentity {
    fn has_scope(&self, expected: &str) -> bool {
        self.scopes.iter().any(|scope| scope == expected)
    }
}

enum MessageIdentity {
    Developer(Identity),
    Reviewer(ReviewerIdentity),
}

impl MessageIdentity {
    fn actor_id(&self) -> &str {
        match self {
            Self::Developer(identity) => &identity.member_id,
            Self::Reviewer(identity) => &identity.reviewer_id,
        }
    }

    const fn actor_kind(&self) -> &'static str {
        match self {
            Self::Developer(_) => "developer",
            Self::Reviewer(_) => "reviewer",
        }
    }
}

struct MutationEvent<'a> {
    now: i64,
    actor_id: &'a str,
    action: &'a str,
    topic: &'a str,
    object_kind: &'a str,
    object_id: &'a str,
    before_state: Option<&'a str>,
    after_state: Option<&'a str>,
    resource_version: u64,
    request_id: &'a str,
    request_sha256: &'a str,
    key_sha256: &'a str,
    payload: Value,
}

struct EditorialMutationEvent<'a> {
    now: i64,
    operator_id: &'a str,
    action: &'a str,
    resource_version: u64,
    request_id: &'a str,
    request_sha256: &'a str,
    key_sha256: &'a str,
    featured_release_id: &'a str,
    featured_app_id: &'a str,
    featured_release_version: u64,
}

async fn begin_serializable(pool: &PgPool) -> Result<Transaction<'_, Postgres>, TxError> {
    let mut transaction = pool.begin().await.map_err(TxError::Sql)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
    Ok(transaction)
}

async fn retry_delay(attempt: usize) {
    tokio::time::sleep(Duration::from_millis(5 * (attempt as u64 + 1))).await;
}

fn require_developer_write(identity: &Identity) -> Result<(), ApiError> {
    if !matches!(identity.role.as_str(), "owner" | "developer")
        || !identity.has_any_scope(&["store.submit", "store.control"])
    {
        return Err(ApiError::forbidden());
    }
    if !identity.two_factor_enabled {
        return Err(ApiError::two_factor_required());
    }
    Ok(())
}

fn require_device_authorization(identity: &Identity) -> Result<(), ApiError> {
    if !matches!(identity.role.as_str(), "owner" | "developer")
        || !identity.has_any_scope(&[DEVICE_SCOPE])
    {
        return Err(ApiError::forbidden());
    }
    if !identity.two_factor_enabled {
        return Err(ApiError::two_factor_required());
    }
    Ok(())
}

fn require_team_read(identity: &Identity) -> Result<(), ApiError> {
    if !identity.has_any_scope(&["store.teams.read", "store.teams.write", "store.control"]) {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

fn require_team_write(identity: &Identity, now: i64) -> Result<(), ApiError> {
    if identity.role != "owner" || !identity.has_any_scope(&["store.teams.write", "store.control"])
    {
        return Err(ApiError::forbidden());
    }
    if !identity.two_factor_enabled {
        return Err(ApiError::two_factor_required());
    }
    if !identity
        .mfa_authenticated_unix_seconds
        .is_some_and(|authenticated| {
            authenticated <= now && authenticated >= now - MFA_STEP_UP_MAX_AGE_SECONDS
        })
    {
        return Err(ApiError::step_up_required());
    }
    Ok(())
}

fn require_release_read(identity: &Identity) -> Result<(), ApiError> {
    if !matches!(identity.role.as_str(), "owner" | "release-manager")
        || !identity.has_any_scope(&["store.release", "store.control"])
    {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

fn require_release_write(identity: &Identity) -> Result<(), ApiError> {
    require_release_read(identity)?;
    if !identity.two_factor_enabled {
        return Err(ApiError::two_factor_required());
    }
    Ok(())
}

fn require_reviewer_write(identity: &ReviewerIdentity) -> Result<(), ApiError> {
    if !matches!(
        identity.role.as_str(),
        "reviewer" | "senior-reviewer" | "admin"
    ) || !identity.has_scope("store.review")
    {
        return Err(ApiError::forbidden());
    }
    if !identity.two_factor_enabled {
        return Err(ApiError::two_factor_required());
    }
    Ok(())
}

fn require_editorial_access(identity: &StoreOperatorIdentity) -> Result<(), ApiError> {
    if !matches!(identity.role.as_str(), "editor" | "admin")
        || !identity.has_scope("store.editorial")
    {
        return Err(ApiError::forbidden());
    }
    if !identity.two_factor_enabled {
        return Err(ApiError::two_factor_required());
    }
    Ok(())
}

async fn complete_idempotency(
    transaction: &mut Transaction<'_, Postgres>,
    actor_id: &str,
    key_sha256: &str,
    status: StatusCode,
    body: &Value,
) -> Result<(), TxError> {
    let affected = sqlx::query(
        "UPDATE idempotency_records SET response_status = $1, response_body = $2 \
         WHERE actor_id = $3 AND key_sha256 = $4 AND response_status IS NULL",
    )
    .bind(status.as_u16() as i16)
    .bind(body)
    .bind(actor_id)
    .bind(key_sha256)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?
    .rows_affected();
    if affected != 1 {
        return Err(ApiError::internal().into());
    }
    Ok(())
}

async fn append_mutation(
    transaction: &mut Transaction<'_, Postgres>,
    event: MutationEvent<'_>,
) -> Result<(), TxError> {
    let version = i64::try_from(event.resource_version).map_err(|_| ApiError::internal())?;
    sqlx::query(
        "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
         object_id, before_state, after_state, resource_version, request_id, request_sha256, \
         idempotency_key_sha256) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(event.now)
    .bind(event.actor_id)
    .bind(event.action)
    .bind(event.object_kind)
    .bind(event.object_id)
    .bind(event.before_state)
    .bind(event.after_state)
    .bind(version)
    .bind(event.request_id)
    .bind(event.request_sha256)
    .bind(event.key_sha256)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(prefixed_uuid("evt_"))
    .bind(event.topic)
    .bind(event.object_kind)
    .bind(event.object_id)
    .bind(version)
    .bind(event.request_sha256)
    .bind(event.payload)
    .bind(event.now)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    Ok(())
}

async fn append_editorial_mutation(
    transaction: &mut Transaction<'_, Postgres>,
    event: EditorialMutationEvent<'_>,
) -> Result<(), TxError> {
    let layout_version = i64::try_from(event.resource_version).map_err(|_| ApiError::internal())?;
    let release_version =
        i64::try_from(event.featured_release_version).map_err(|_| ApiError::internal())?;
    sqlx::query(
        "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
         object_id, before_state, after_state, resource_version, request_id, request_sha256, \
         idempotency_key_sha256) VALUES ($1, $2, $3, 'editorial', 'today', NULL, 'active', \
         $4, $5, $6, $7)",
    )
    .bind(event.now)
    .bind(event.operator_id)
    .bind(event.action)
    .bind(layout_version)
    .bind(event.request_id)
    .bind(event.request_sha256)
    .bind(event.key_sha256)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds) \
         VALUES ($1, 'catalog.rebuild-requested', 'release', $2, $3, $4, $5, $6)",
    )
    .bind(prefixed_uuid("evt_"))
    .bind(event.featured_release_id)
    .bind(release_version)
    .bind(event.request_sha256)
    .bind(json!({
        "release_id": event.featured_release_id,
        "app_id": event.featured_app_id,
        "state": "published",
        "editorial_resource_version": event.resource_version
    }))
    .bind(event.now)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    Ok(())
}

async fn insert_upload_part(
    transaction: &mut Transaction<'_, Postgres>,
    submission_id: &str,
    part_name: &str,
    expected_sha256: &str,
    expected_bytes: u64,
) -> Result<(), TxError> {
    sqlx::query(
        "INSERT INTO submission_upload_parts (submission_id, part_name, expected_sha256, \
         expected_bytes, received_bytes) VALUES ($1, $2, $3, $4, 0)",
    )
    .bind(submission_id)
    .bind(part_name)
    .bind(expected_sha256)
    .bind(i64::try_from(expected_bytes).map_err(|_| ApiError::invalid_request())?)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    Ok(())
}

async fn load_submission(
    transaction: &mut Transaction<'_, Postgres>,
    submission_id: &str,
    team_id: &str,
    lock: bool,
) -> Result<StoredSubmission, TxError> {
    let sql = if lock {
        "SELECT submission.submission_id, submission.app_id, submission.version, \
         submission.revision, submission.state, submission.package_sha256, \
         submission.package_bytes, submission.listing_sha256, submission.listing_bytes, \
         submission.assets, submission.resource_version, submission.created_unix_seconds \
         FROM submissions submission JOIN apps app ON app.app_id = submission.app_id \
         WHERE submission.submission_id = $1 AND app.owner_team_id = $2 \
         FOR UPDATE OF submission"
    } else {
        "SELECT submission.submission_id, submission.app_id, submission.version, \
         submission.revision, submission.state, submission.package_sha256, \
         submission.package_bytes, submission.listing_sha256, submission.listing_bytes, \
         submission.assets, submission.resource_version, submission.created_unix_seconds \
         FROM submissions submission JOIN apps app ON app.app_id = submission.app_id \
         WHERE submission.submission_id = $1 AND app.owner_team_id = $2"
    };
    let row = sqlx::query(sql)
        .bind(submission_id)
        .bind(team_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
    stored_submission_from_row(&row).map_err(Into::into)
}

async fn load_review_submission(
    transaction: &mut Transaction<'_, Postgres>,
    submission_id: &str,
    lock: bool,
) -> Result<StoredSubmission, TxError> {
    let sql = if lock {
        "SELECT submission_id, app_id, version, revision, state, package_sha256, \
         package_bytes, listing_sha256, listing_bytes, assets, resource_version, \
         created_unix_seconds FROM submissions WHERE submission_id = $1 FOR UPDATE"
    } else {
        "SELECT submission_id, app_id, version, revision, state, package_sha256, \
         package_bytes, listing_sha256, listing_bytes, assets, resource_version, \
         created_unix_seconds FROM submissions WHERE submission_id = $1"
    };
    let row = sqlx::query(sql)
        .bind(submission_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
    stored_submission_from_row(&row).map_err(Into::into)
}

async fn load_release(
    transaction: &mut Transaction<'_, Postgres>,
    release_id: &str,
    team_id: &str,
    lock: bool,
) -> Result<ReleaseResponse, TxError> {
    let sql = if lock {
        "SELECT release.release_id, release.submission_id, release.app_id, release.version, \
         release.state, release.rollout_percent, release.scheduled_unix_seconds, \
         release.catalog_sequence, release.resource_version FROM releases release \
         JOIN apps app ON app.app_id = release.app_id \
         WHERE release.release_id = $1 AND app.owner_team_id = $2 FOR UPDATE OF release"
    } else {
        "SELECT release.release_id, release.submission_id, release.app_id, release.version, \
         release.state, release.rollout_percent, release.scheduled_unix_seconds, \
         release.catalog_sequence, release.resource_version FROM releases release \
         JOIN apps app ON app.app_id = release.app_id \
         WHERE release.release_id = $1 AND app.owner_team_id = $2"
    };
    let row = sqlx::query(sql)
        .bind(release_id)
        .bind(team_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
    release_from_row(&row).map_err(Into::into)
}

async fn load_editorial_layout(
    transaction: &mut Transaction<'_, Postgres>,
    lock: bool,
) -> Result<EditorialLayoutResponse, TxError> {
    let sql = if lock {
        "SELECT headline, featured_release_id, featured_app_id, collections, \
         resource_version, updated_unix_seconds FROM store_editorial_layouts \
         WHERE layout_id = 'today' FOR UPDATE"
    } else {
        "SELECT headline, featured_release_id, featured_app_id, collections, \
         resource_version, updated_unix_seconds FROM store_editorial_layouts \
         WHERE layout_id = 'today'"
    };
    let row = sqlx::query(sql)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
    let collections: Value = row.get("collections");
    Ok(EditorialLayoutResponse {
        layout_id: "today".into(),
        headline: row.get("headline"),
        featured: EditorialItemResponse {
            release_id: row.get("featured_release_id"),
            app_id: row.get("featured_app_id"),
        },
        collections: serde_json::from_value(collections).map_err(|_| ApiError::internal())?,
        resource_version: row_version(&row)?,
        updated_unix_seconds: u64::try_from(row.get::<i64, _>("updated_unix_seconds"))
            .map_err(|_| ApiError::internal())?,
    })
}

async fn resolve_editorial_layout(
    transaction: &mut Transaction<'_, Postgres>,
    request: &EditorialLayoutRequest,
) -> Result<ResolvedEditorialLayout, TxError> {
    let release_ids = std::iter::once(request.featured_release_id.as_str())
        .chain(
            request
                .collections
                .iter()
                .flat_map(|collection| collection.release_ids.iter().map(String::as_str)),
        )
        .collect::<BTreeSet<_>>();
    let mut releases = BTreeMap::new();
    for release_id in release_ids {
        let row = sqlx::query(
            "SELECT release.app_id, release.resource_version FROM releases release \
             JOIN submissions submission ON submission.submission_id = release.submission_id \
             WHERE release.release_id = $1 AND release.state = 'published' \
               AND submission.state = 'approved' AND submission.app_id = release.app_id \
               AND submission.version = release.version FOR SHARE OF release, submission",
        )
        .bind(release_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::invalid_transition)?;
        let app_id: String = row.get("app_id");
        let version = u64::try_from(row.get::<i64, _>("resource_version"))
            .map_err(|_| ApiError::internal())?;
        releases.insert(release_id.to_owned(), (app_id, version));
    }
    let (featured_app_id, featured_release_version) = releases
        .get(&request.featured_release_id)
        .cloned()
        .ok_or_else(ApiError::internal)?;
    let featured = EditorialItemResponse {
        release_id: request.featured_release_id.clone(),
        app_id: featured_app_id.clone(),
    };
    let mut app_ids = BTreeSet::from([featured_app_id]);
    let mut collections = Vec::with_capacity(request.collections.len());
    for collection in &request.collections {
        let mut items = Vec::with_capacity(collection.release_ids.len());
        for release_id in &collection.release_ids {
            let (app_id, _) = releases.get(release_id).ok_or_else(ApiError::internal)?;
            if !app_ids.insert(app_id.clone()) {
                return Err(ApiError::invalid_request().into());
            }
            items.push(EditorialItemResponse {
                release_id: release_id.clone(),
                app_id: app_id.clone(),
            });
        }
        collections.push(EditorialCollectionResponse {
            title: collection.title.clone(),
            items,
        });
    }
    let collections_json = serde_json::to_value(&collections).map_err(|_| ApiError::internal())?;
    Ok(ResolvedEditorialLayout {
        featured,
        featured_release_version,
        collections,
        collections_json,
    })
}

async fn load_team(
    transaction: &mut Transaction<'_, Postgres>,
    team_id: &str,
    lock: bool,
) -> Result<TeamResponse, TxError> {
    let sql = if lock {
        "SELECT team_id, name, resource_version FROM teams WHERE team_id = $1 FOR UPDATE"
    } else {
        "SELECT team_id, name, resource_version FROM teams WHERE team_id = $1"
    };
    let team = sqlx::query(sql)
        .bind(team_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
    let rows = sqlx::query(
        "SELECT member_id, email, role, membership_state, two_factor_enabled, resource_version \
         FROM team_members WHERE team_id = $1 AND membership_state <> 'removed' \
         ORDER BY member_id LIMIT 101",
    )
    .bind(team_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    if rows.len() > MAX_TEAM_MEMBERS {
        return Err(ApiError::internal().into());
    }
    let members = rows
        .into_iter()
        .map(|row| {
            Ok(TeamMemberResponse {
                member_id: row.get("member_id"),
                email: row.get("email"),
                role: row.get("role"),
                membership_state: row.get("membership_state"),
                two_factor_enabled: row.get("two_factor_enabled"),
                resource_version: row_version(&row)?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(TeamResponse {
        team_id: team.get("team_id"),
        name: team.get("name"),
        members,
        resource_version: row_version(&team)?,
    })
}

fn release_from_row(row: &sqlx::postgres::PgRow) -> Result<ReleaseResponse, ApiError> {
    Ok(ReleaseResponse {
        release_id: row.get("release_id"),
        submission_id: row.get("submission_id"),
        app_id: row.get("app_id"),
        version: row.get("version"),
        state: parse_release_state(row.get("state"))?,
        rollout_percent: u8::try_from(row.get::<i16, _>("rollout_percent"))
            .map_err(|_| ApiError::internal())?,
        scheduled_unix_seconds: row
            .get::<Option<i64>, _>("scheduled_unix_seconds")
            .map(u64::try_from)
            .transpose()
            .map_err(|_| ApiError::internal())?,
        catalog_sequence: row
            .get::<Option<i64>, _>("catalog_sequence")
            .map(u64::try_from)
            .transpose()
            .map_err(|_| ApiError::internal())?,
        resource_version: row_version(row)?,
    })
}

fn stored_submission_from_row(row: &sqlx::postgres::PgRow) -> Result<StoredSubmission, ApiError> {
    let assets: Value = row.get("assets");
    Ok(StoredSubmission {
        response: SubmissionResponse {
            submission_id: row.get("submission_id"),
            app_id: row.get("app_id"),
            version: row.get("version"),
            revision: u32::try_from(row.get::<i32, _>("revision"))
                .map_err(|_| ApiError::internal())?,
            state: parse_submission_state(row.get("state"))?,
            package_sha256: row.get("package_sha256"),
            listing_sha256: row.get("listing_sha256"),
            assets: serde_json::from_value(assets).map_err(|_| ApiError::internal())?,
            resource_version: u64::try_from(row.get::<i64, _>("resource_version"))
                .map_err(|_| ApiError::internal())?,
            created_unix_seconds: u64::try_from(row.get::<i64, _>("created_unix_seconds"))
                .map_err(|_| ApiError::internal())?,
        },
        package_bytes: u64::try_from(row.get::<i64, _>("package_bytes"))
            .map_err(|_| ApiError::internal())?,
        listing_bytes: u64::try_from(row.get::<i64, _>("listing_bytes"))
            .map_err(|_| ApiError::internal())?,
    })
}

fn risk_assessment_from_row(row: &sqlx::postgres::PgRow) -> Result<RiskAssessment, ApiError> {
    let policy_version = u16::try_from(row.get::<i16, _>("risk_policy_version"))
        .map_err(|_| ApiError::internal())?;
    let tier: String = row.get("risk_tier");
    let reasons: Value = row.get("risk_reason_codes");
    serde_json::from_value(json!({
        "policy_version": policy_version,
        "tier": tier,
        "reasons": reasons,
    }))
    .map_err(|_| ApiError::internal())
}

fn review_app_from_row(row: &sqlx::postgres::PgRow) -> Result<ReviewAppResponse, ApiError> {
    let category: String = row.get("review_category");
    Ok(ReviewAppResponse {
        name: row.get("review_name"),
        developer_name: row.get("developer_name"),
        category: serde_json::from_value(json!(category)).map_err(|_| ApiError::internal())?,
    })
}

fn review_assignment_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<ReviewAssignmentResponse, ApiError> {
    Ok(ReviewAssignmentResponse {
        assignment_id: row.get("assignment_id"),
        reviewer_id: row.get("reviewer_id"),
        reviewer_email: row.get("email"),
        reviewer_role: row.get("role"),
        assignment_kind: row.get("assignment_kind"),
        state: row.get("state"),
        created_unix_seconds: u64::try_from(row.get::<i64, _>("created_unix_seconds"))
            .map_err(|_| ApiError::internal())?,
        completed_unix_seconds: row
            .get::<Option<i64>, _>("completed_unix_seconds")
            .map(u64::try_from)
            .transpose()
            .map_err(|_| ApiError::internal())?,
    })
}

fn review_decision_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<ReviewDecisionRecordResponse, ApiError> {
    Ok(ReviewDecisionRecordResponse {
        decision_id: row.get("decision_id"),
        reviewer_id: row.get("reviewer_id"),
        reviewer_email: row.get("email"),
        decision: row.get("decision"),
        reason_codes: row.get("reason_codes"),
        note: row.get("note"),
        created_unix_seconds: u64::try_from(row.get::<i64, _>("created_unix_seconds"))
            .map_err(|_| ApiError::internal())?,
        assignment_id: row.get("assignment_id"),
    })
}

fn review_audit_from_row(row: &sqlx::postgres::PgRow) -> Result<ReviewAuditResponse, ApiError> {
    Ok(ReviewAuditResponse {
        sequence: u64::try_from(row.get::<i64, _>("sequence")).map_err(|_| ApiError::internal())?,
        occurred_unix_seconds: u64::try_from(row.get::<i64, _>("occurred_unix_seconds"))
            .map_err(|_| ApiError::internal())?,
        actor_id: row.get("actor_id"),
        action: row.get("action"),
        before_state: row.get("before_state"),
        after_state: row.get("after_state"),
        resource_version: row_version(row)?,
    })
}

fn parse_submission_state(value: String) -> Result<SubmissionState, ApiError> {
    match value.as_str() {
        "draft" => Ok(SubmissionState::Draft),
        "uploading" => Ok(SubmissionState::Uploading),
        "processing" => Ok(SubmissionState::Processing),
        "ready-for-review" => Ok(SubmissionState::ReadyForReview),
        "in-review" => Ok(SubmissionState::InReview),
        "pending-secondary-review" => Ok(SubmissionState::PendingSecondaryReview),
        "needs-changes" => Ok(SubmissionState::NeedsChanges),
        "approved" => Ok(SubmissionState::Approved),
        "rejected" => Ok(SubmissionState::Rejected),
        "withdrawn" => Ok(SubmissionState::Withdrawn),
        _ => Err(ApiError::internal()),
    }
}

fn parse_release_state(value: String) -> Result<ReleaseState, ApiError> {
    match value.as_str() {
        "ready" => Ok(ReleaseState::Ready),
        "scheduled" => Ok(ReleaseState::Scheduled),
        "publishing" => Ok(ReleaseState::Publishing),
        "publish-failed" => Ok(ReleaseState::PublishFailed),
        "published" => Ok(ReleaseState::Published),
        "paused" => Ok(ReleaseState::Paused),
        "removed" => Ok(ReleaseState::Removed),
        _ => Err(ApiError::internal()),
    }
}

async fn load_upload_part(
    transaction: &mut Transaction<'_, Postgres>,
    submission_id: &str,
    part_name: &str,
) -> Result<UploadPart, TxError> {
    let row = sqlx::query(
        "SELECT part_name, expected_sha256, expected_bytes, received_bytes \
         FROM submission_upload_parts WHERE submission_id = $1 AND part_name = $2 FOR UPDATE",
    )
    .bind(submission_id)
    .bind(part_name)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(TxError::Sql)?
    .ok_or_else(ApiError::not_found)?;
    upload_part_from_row(&row).map_err(Into::into)
}

async fn load_upload_parts(
    transaction: &mut Transaction<'_, Postgres>,
    submission_id: &str,
) -> Result<Vec<UploadPart>, TxError> {
    let rows = sqlx::query(
        "SELECT part_name, expected_sha256, expected_bytes, received_bytes \
         FROM submission_upload_parts WHERE submission_id = $1 ORDER BY part_name FOR UPDATE",
    )
    .bind(submission_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    rows.iter()
        .map(upload_part_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn upload_part_from_row(row: &sqlx::postgres::PgRow) -> Result<UploadPart, ApiError> {
    Ok(UploadPart {
        name: row.get("part_name"),
        expected_sha256: row.get("expected_sha256"),
        expected_bytes: u64::try_from(row.get::<i64, _>("expected_bytes"))
            .map_err(|_| ApiError::internal())?,
        received_bytes: u64::try_from(row.get::<i64, _>("received_bytes"))
            .map_err(|_| ApiError::internal())?,
    })
}

fn upload_parts_match_submission(stored: &StoredSubmission, parts: &[UploadPart]) -> bool {
    let matches_part = |name: &str, sha256: &str, bytes: u64| {
        parts.iter().any(|part| {
            part.name == name && part.expected_sha256 == sha256 && part.expected_bytes == bytes
        })
    };
    matches_part(
        "package",
        &stored.response.package_sha256,
        stored.package_bytes,
    ) && matches_part(
        "listing",
        &stored.response.listing_sha256,
        stored.listing_bytes,
    ) && stored
        .response
        .assets
        .iter()
        .enumerate()
        .all(|(index, asset)| matches_part(&format!("asset-{index}"), &asset.sha256, asset.bytes))
}

async fn verify_uploaded_part(
    object_store: &ContentObjectStore,
    transaction: &mut Transaction<'_, Postgres>,
    submission_id: &str,
    part: &UploadPart,
) -> Result<(), TxError> {
    let rows = sqlx::query(
        "SELECT chunk_offset, chunk_bytes, chunk_sha256 FROM submission_upload_chunks \
         WHERE submission_id = $1 AND part_name = $2 ORDER BY chunk_offset",
    )
    .bind(submission_id)
    .bind(&part.name)
    .fetch_all(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    let chunks = rows
        .iter()
        .map(|row| {
            Ok(StoredChunk {
                offset: u64::try_from(row.get::<i64, _>("chunk_offset"))
                    .map_err(|_| ApiError::internal())?,
                bytes: usize::try_from(row.get::<i32, _>("chunk_bytes"))
                    .map_err(|_| ApiError::internal())?,
                sha256: row.get("chunk_sha256"),
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let mut offset = 0_u64;
    let mut hasher = Sha256::new();
    for chunk in chunks {
        if chunk.offset != offset {
            return Err(ApiError::digest_mismatch().into());
        }
        let bytes = object_store
            .verify_chunk(&chunk.sha256, chunk.bytes)
            .await
            .map_err(|_| ApiError::unavailable())?;
        hasher.update(&bytes);
        offset = offset
            .checked_add(chunk.bytes as u64)
            .ok_or_else(ApiError::internal)?;
    }
    if offset != part.expected_bytes || lower_hex(&hasher.finalize()) != part.expected_sha256 {
        return Err(ApiError::digest_mismatch().into());
    }
    Ok(())
}

fn submission_content_sha256(
    package_sha256: &str,
    listing_sha256: &str,
    assets: &[ImageAsset],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"CardputerZero Store submission content v1\0");
    hash_field(&mut hasher, package_sha256.as_bytes());
    hash_field(&mut hasher, listing_sha256.as_bytes());
    for asset in assets {
        hash_field(&mut hasher, asset.path.as_bytes());
        hash_field(&mut hasher, asset.sha256.as_bytes());
        hasher.update(asset.bytes.to_be_bytes());
        hasher.update(asset.width.to_be_bytes());
        hasher.update(asset.height.to_be_bytes());
    }
    lower_hex(&hasher.finalize())
}

fn mutation_request_sha256(domain: &str, fields: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, domain.as_bytes());
    for field in fields {
        hash_field(&mut hasher, field.as_bytes());
    }
    lower_hex(&hasher.finalize())
}

fn editorial_request_sha256(
    expected_version: Option<u64>,
    request: &EditorialLayoutRequest,
) -> Result<String, ApiError> {
    let encoded = serde_json::to_vec(request).map_err(|_| ApiError::internal())?;
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, b"editorial.today.replace.v1");
    hash_field(
        &mut hasher,
        expected_version
            .map_or_else(|| "create".into(), |version| version.to_string())
            .as_bytes(),
    );
    hash_field(&mut hasher, &encoded);
    Ok(lower_hex(&hasher.finalize()))
}

fn validate_editorial_request(request: &EditorialLayoutRequest) -> Result<(), ApiError> {
    if !valid_editorial_text(&request.headline, 48)
        || !is_valid_release_id(&request.featured_release_id)
        || !(1..=2).contains(&request.collections.len())
    {
        return Err(ApiError::invalid_request());
    }
    let mut releases = BTreeSet::from([request.featured_release_id.as_str()]);
    let mut titles = BTreeSet::new();
    for collection in &request.collections {
        if !valid_editorial_text(&collection.title, 32)
            || !titles.insert(collection.title.as_str())
            || !(1..=4).contains(&collection.release_ids.len())
            || collection
                .release_ids
                .iter()
                .any(|release_id| !is_valid_release_id(release_id) || !releases.insert(release_id))
        {
            return Err(ApiError::invalid_request());
        }
    }
    Ok(())
}

fn valid_editorial_text(value: &str, maximum_chars: usize) -> bool {
    let chars = value.chars().count();
    (1..=maximum_chars).contains(&chars)
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn oauth_decision_request_sha256(request: &DeviceAuthorizationDecisionRequest) -> String {
    mutation_request_sha256(
        "oauth-device.decision.v1",
        &[&request.user_code, &request.decision],
    )
}

fn oauth_exchange_request_sha256(device_code_sha256: &str) -> String {
    mutation_request_sha256(
        "oauth-device.exchange.v1",
        &[device_code_sha256, DEVICE_CLIENT_ID],
    )
}

fn release_mutation_request_sha256(
    release_id: &str,
    expected_version: u64,
    action: &ReleaseAction,
) -> String {
    let expected_version = expected_version.to_string();
    match action {
        ReleaseAction::Schedule {
            publish_unix_seconds,
        } => {
            let publish_unix_seconds = publish_unix_seconds.to_string();
            mutation_request_sha256(
                "release.mutate.v1",
                &[
                    release_id,
                    &expected_version,
                    action.name(),
                    &publish_unix_seconds,
                ],
            )
        }
        ReleaseAction::Remove { reason_code, note } => mutation_request_sha256(
            "release.mutate.v1",
            &[
                release_id,
                &expected_version,
                action.name(),
                reason_code,
                note,
            ],
        ),
        _ => mutation_request_sha256(
            "release.mutate.v1",
            &[release_id, &expected_version, action.name()],
        ),
    }
}

fn release_action_transition(action: &ReleaseAction) -> (ReleaseState, &'static str, &'static str) {
    match action {
        ReleaseAction::Schedule { .. } => (
            ReleaseState::Scheduled,
            "release.scheduled",
            "release.scheduled",
        ),
        ReleaseAction::Publish => (
            ReleaseState::Publishing,
            "release.publish-requested",
            "release.publish-requested",
        ),
        ReleaseAction::Pause => (
            ReleaseState::Paused,
            "release.paused",
            "catalog.rebuild-requested",
        ),
        ReleaseAction::Resume => (
            ReleaseState::Published,
            "release.resumed",
            "catalog.rebuild-requested",
        ),
        ReleaseAction::Remove { .. } => (
            ReleaseState::Removed,
            "release.removed",
            "catalog.rebuild-requested",
        ),
    }
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn authenticate(
    transaction: &mut Transaction<'_, Postgres>,
    token_sha256: &str,
) -> Result<Identity, TxError> {
    lookup_developer_identity(transaction, token_sha256)
        .await?
        .ok_or_else(|| ApiError::unauthorized().into())
}

async fn lookup_developer_identity(
    transaction: &mut Transaction<'_, Postgres>,
    token_sha256: &str,
) -> Result<Option<Identity>, TxError> {
    let row = sqlx::query(
        "SELECT member.member_id, member.team_id, member.role, member.two_factor_enabled, \
         token.mfa_authenticated_unix_seconds, token.scopes \
         FROM access_tokens token \
         JOIN team_members member ON member.member_id = token.member_id \
         WHERE token.token_sha256 = $1 AND NOT token.revoked \
           AND member.membership_state = 'active' \
           AND token.expires_unix_seconds > EXTRACT(EPOCH FROM clock_timestamp())::BIGINT",
    )
    .bind(token_sha256)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;

    Ok(row.map(|row| Identity {
        member_id: row.get("member_id"),
        team_id: row.get("team_id"),
        role: row.get("role"),
        two_factor_enabled: row.get("two_factor_enabled"),
        mfa_authenticated_unix_seconds: row.get("mfa_authenticated_unix_seconds"),
        scopes: row.get("scopes"),
    }))
}

async fn authenticate_reviewer(
    transaction: &mut Transaction<'_, Postgres>,
    token_sha256: &str,
) -> Result<ReviewerIdentity, TxError> {
    lookup_reviewer_identity(transaction, token_sha256)
        .await?
        .ok_or_else(|| ApiError::unauthorized().into())
}

async fn lookup_reviewer_identity(
    transaction: &mut Transaction<'_, Postgres>,
    token_sha256: &str,
) -> Result<Option<ReviewerIdentity>, TxError> {
    let row = sqlx::query(
        "SELECT reviewer.reviewer_id, reviewer.role, reviewer.two_factor_enabled, token.scopes \
         FROM reviewer_access_tokens token \
         JOIN reviewers reviewer ON reviewer.reviewer_id = token.reviewer_id \
         LEFT JOIN workforce_sessions session \
           ON session.session_sha256 = token.workforce_session_sha256 \
         LEFT JOIN workforce_identity_links link ON link.link_id = session.link_id \
         WHERE token.token_sha256 = $1 AND NOT token.revoked AND reviewer.state = 'active' \
           AND token.expires_unix_seconds > EXTRACT(EPOCH FROM clock_timestamp())::BIGINT \
           AND (token.workforce_session_sha256 IS NULL OR ( \
             session.state = 'active' AND session.audience = 'review' \
             AND session.idle_expires_unix_seconds > \
               EXTRACT(EPOCH FROM clock_timestamp())::BIGINT \
             AND session.absolute_expires_unix_seconds > \
               EXTRACT(EPOCH FROM clock_timestamp())::BIGINT \
             AND link.state = 'active' AND link.reviewer_id = token.reviewer_id \
             AND link.operator_id IS NULL \
             AND token.created_unix_seconds >= session.created_unix_seconds \
             AND token.expires_unix_seconds <= LEAST( \
               session.idle_expires_unix_seconds, session.absolute_expires_unix_seconds, \
               token.created_unix_seconds + 300)))",
    )
    .bind(token_sha256)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;

    Ok(row.map(|row| ReviewerIdentity {
        reviewer_id: row.get("reviewer_id"),
        role: row.get("role"),
        two_factor_enabled: row.get("two_factor_enabled"),
        scopes: row.get("scopes"),
    }))
}

async fn authenticate_store_operator(
    transaction: &mut Transaction<'_, Postgres>,
    token_sha256: &str,
) -> Result<StoreOperatorIdentity, TxError> {
    let row = sqlx::query(
        "SELECT operator.operator_id, operator.role, operator.two_factor_enabled, token.scopes \
         FROM store_operator_access_tokens token \
         JOIN store_operators operator ON operator.operator_id = token.operator_id \
         LEFT JOIN workforce_sessions session \
           ON session.session_sha256 = token.workforce_session_sha256 \
         LEFT JOIN workforce_identity_links link ON link.link_id = session.link_id \
         WHERE token.token_sha256 = $1 AND NOT token.revoked AND operator.state = 'active' \
           AND token.expires_unix_seconds > EXTRACT(EPOCH FROM clock_timestamp())::BIGINT \
           AND (token.workforce_session_sha256 IS NULL OR ( \
             session.state = 'active' AND session.audience = 'operations' \
             AND session.idle_expires_unix_seconds > \
               EXTRACT(EPOCH FROM clock_timestamp())::BIGINT \
             AND session.absolute_expires_unix_seconds > \
               EXTRACT(EPOCH FROM clock_timestamp())::BIGINT \
             AND link.state = 'active' AND link.operator_id = token.operator_id \
             AND link.reviewer_id IS NULL \
             AND token.created_unix_seconds >= session.created_unix_seconds \
             AND token.expires_unix_seconds <= LEAST( \
               session.idle_expires_unix_seconds, session.absolute_expires_unix_seconds, \
               token.created_unix_seconds + 300)))",
    )
    .bind(token_sha256)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(TxError::Sql)?
    .ok_or_else(ApiError::unauthorized)?;
    Ok(StoreOperatorIdentity {
        operator_id: row.get("operator_id"),
        role: row.get("role"),
        two_factor_enabled: row.get("two_factor_enabled"),
        scopes: row.get("scopes"),
    })
}

async fn authenticate_message_actor(
    transaction: &mut Transaction<'_, Postgres>,
    token_sha256: &str,
) -> Result<MessageIdentity, TxError> {
    let developer = lookup_developer_identity(transaction, token_sha256).await?;
    let reviewer = lookup_reviewer_identity(transaction, token_sha256).await?;
    match (developer, reviewer) {
        (Some(identity), None) => Ok(MessageIdentity::Developer(identity)),
        (None, Some(identity)) => Ok(MessageIdentity::Reviewer(identity)),
        _ => Err(ApiError::unauthorized().into()),
    }
}

async fn database_now(transaction: &mut Transaction<'_, Postgres>) -> Result<i64, TxError> {
    sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(&mut **transaction)
        .await
        .map_err(TxError::Sql)
}

async fn reserve_idempotency(
    transaction: &mut Transaction<'_, Postgres>,
    actor_id: &str,
    key_sha256: &str,
    request_sha256: &str,
    now: i64,
) -> Result<IdempotencyReservation, TxError> {
    let inserted = sqlx::query(
        "INSERT INTO idempotency_records (actor_id, key_sha256, request_sha256, \
         created_unix_seconds, expires_unix_seconds) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (actor_id, key_sha256) DO NOTHING",
    )
    .bind(actor_id)
    .bind(key_sha256)
    .bind(request_sha256)
    .bind(now)
    .bind(now + IDEMPOTENCY_TTL_SECONDS)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?
    .rows_affected();
    if inserted == 1 {
        return Ok(IdempotencyReservation::Fresh);
    }

    let row = sqlx::query(
        "SELECT request_sha256, response_status, response_body, expires_unix_seconds \
         FROM idempotency_records WHERE actor_id = $1 AND key_sha256 = $2 FOR UPDATE",
    )
    .bind(actor_id)
    .bind(key_sha256)
    .fetch_one(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;

    let expires: i64 = row.get("expires_unix_seconds");
    if expires <= now {
        sqlx::query("DELETE FROM idempotency_records WHERE actor_id = $1 AND key_sha256 = $2")
            .bind(actor_id)
            .bind(key_sha256)
            .execute(&mut **transaction)
            .await
            .map_err(TxError::Sql)?;
        sqlx::query(
            "INSERT INTO idempotency_records (actor_id, key_sha256, request_sha256, \
             created_unix_seconds, expires_unix_seconds) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(actor_id)
        .bind(key_sha256)
        .bind(request_sha256)
        .bind(now)
        .bind(now + IDEMPOTENCY_TTL_SECONDS)
        .execute(&mut **transaction)
        .await
        .map_err(TxError::Sql)?;
        return Ok(IdempotencyReservation::Fresh);
    }

    let existing_request: String = row.get("request_sha256");
    if existing_request != request_sha256 {
        return Err(ApiError::idempotency_conflict().into());
    }
    let status: Option<i16> = row.get("response_status");
    let body: Option<Value> = row.get("response_body");
    match (status, body) {
        (Some(status), Some(body)) => Ok(IdempotencyReservation::Replay { status, body }),
        _ => Err(ApiError::unavailable().into()),
    }
}

async fn post_aggregate_metrics(
    State(service): State<StoreControlService>,
    payload: Result<Json<AggregateMetricsReport>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    let Json(report) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            return metrics_error_response(json_rejection(rejection), request_id);
        }
    };
    match service.ingest_aggregate_metrics(&report).await {
        Ok(()) => metrics_json_response(
            StatusCode::ACCEPTED,
            MetricsAcceptedResponse {
                accepted: true,
                batch_id: report.batch_id,
            },
            request_id,
        ),
        Err(error) => metrics_error_response(error, request_id),
    }
}

async fn post_device_code(
    State(service): State<StoreControlService>,
    payload: Result<Json<DeviceCodeRequest>, JsonRejection>,
) -> Response {
    let response_request_id = request_id();
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            return oauth_error_response(json_rejection(rejection), response_request_id);
        }
    };
    match service.create_device_authorization(&request).await {
        Ok(response) => oauth_json_response(StatusCode::OK, response, response_request_id),
        Err(error) => oauth_error_response(error, response_request_id),
    }
}

async fn post_device_authorization(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
    payload: Result<Json<DeviceAuthorizationDecisionRequest>, JsonRejection>,
) -> Response {
    let response_request_id = request_id();
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return oauth_error_response(error, response_request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return oauth_error_response(error, response_request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            return oauth_error_response(json_rejection(rejection), response_request_id);
        }
    };
    if !is_valid_user_code(&request.user_code)
        || !matches!(request.decision.as_str(), "approve" | "deny")
    {
        return oauth_error_response(ApiError::invalid_request(), response_request_id);
    }
    match service
        .decide_device_authorization(&token, &idempotency_key, &response_request_id, &request)
        .await
    {
        Ok(()) => oauth_empty_response(response_request_id),
        Err(error) => oauth_error_response(error, response_request_id),
    }
}

async fn post_device_token(
    State(service): State<StoreControlService>,
    payload: Result<Json<DeviceTokenRequest>, JsonRejection>,
) -> Response {
    let response_request_id = request_id();
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            return oauth_error_response(json_rejection(rejection), response_request_id);
        }
    };
    match service.exchange_device_authorization(&request).await {
        Ok(response) => oauth_json_response(StatusCode::OK, response, response_request_id),
        Err(error) => oauth_error_response(error, response_request_id),
    }
}

async fn get_team(
    State(service): State<StoreControlService>,
    Path(team_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id();
    if !is_valid_team_id(&team_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service.get_team(&token, &team_id).await {
        Ok(team) => {
            let version = team.resource_version;
            resource_response(StatusCode::OK, team, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn mutate_team_member(
    State(service): State<StoreControlService>,
    Path((team_id, member_action)): Path<(String, String)>,
    headers: HeaderMap,
    payload: Result<Bytes, BytesRejection>,
) -> Response {
    let request_id = request_id();
    let Some((member_id, action)) = member_action.split_once(':') else {
        return ApiError::invalid_request().response(request_id);
    };
    if !is_valid_team_id(&team_id) || !is_valid_member_id(member_id) || action.contains(':') {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let expected_version = match expected_version(&headers) {
        Ok(value) => value,
        Err(error) => return error.response(request_id),
    };
    let body = match payload {
        Ok(body) => body,
        Err(rejection) => {
            let error = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                ApiError::payload_too_large()
            } else {
                ApiError::invalid_request()
            };
            return error.response(request_id);
        }
    };
    let result = match action {
        "set-role" => {
            if let Err(error) = require_json_content_type(&headers) {
                return error.response(request_id);
            }
            let request: SetTeamMemberRoleRequest = match serde_json::from_slice(&body) {
                Ok(request) => request,
                Err(_) => return ApiError::invalid_request().response(request_id),
            };
            if !is_valid_team_role(&request.role) {
                return ApiError::invalid_request().response(request_id);
            }
            service
                .set_team_member_role(
                    &token,
                    &idempotency_key,
                    &request_id,
                    &team_id,
                    member_id,
                    expected_version,
                    &request,
                )
                .await
        }
        "remove" if body.is_empty() => {
            service
                .remove_team_member(
                    &token,
                    &idempotency_key,
                    &request_id,
                    &team_id,
                    member_id,
                    expected_version,
                )
                .await
        }
        "suspend" if body.is_empty() => {
            service
                .set_team_member_state(
                    &token,
                    &idempotency_key,
                    &request_id,
                    &team_id,
                    member_id,
                    expected_version,
                    TeamMemberStateAction::Suspend,
                )
                .await
        }
        "restore" if body.is_empty() => {
            service
                .set_team_member_state(
                    &token,
                    &idempotency_key,
                    &request_id,
                    &team_id,
                    member_id,
                    expected_version,
                    TeamMemberStateAction::Restore,
                )
                .await
        }
        _ => return ApiError::invalid_request().response(request_id),
    };
    match result {
        Ok(team) => {
            let version = team.resource_version;
            resource_response(StatusCode::OK, team, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn post_app(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
    payload: Result<Json<CreateAppRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            let error = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                ApiError::payload_too_large()
            } else {
                ApiError::invalid_request()
            };
            return error.response(request_id);
        }
    };
    if !cp0_manifest::is_valid_app_id(&request.app_id) || !is_valid_locale(&request.default_locale)
    {
        return ApiError::invalid_request().response(request_id);
    }

    match service
        .create_app(&token, &idempotency_key, &request_id, &request)
        .await
    {
        Ok(app) => {
            let version = app.resource_version;
            resource_response(StatusCode::CREATED, app, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn get_app(
    State(service): State<StoreControlService>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id();
    if !cp0_manifest::is_valid_app_id(&app_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service.get_app(&token, &app_id).await {
        Ok(app) => {
            let version = app.resource_version;
            resource_response(StatusCode::OK, app, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn post_submission(
    State(service): State<StoreControlService>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<CreateSubmissionRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    if !cp0_manifest::is_valid_app_id(&app_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return json_rejection(rejection).response(request_id),
    };
    if validate_submission_spec(&request.spec()).is_err() {
        return ApiError::invalid_request().response(request_id);
    }
    match service
        .create_submission(&token, &idempotency_key, &request_id, &app_id, &request)
        .await
    {
        Ok(submission) => {
            let version = submission.resource_version;
            resource_response(StatusCode::CREATED, submission, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn get_submission(
    State(service): State<StoreControlService>,
    Path(submission_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id();
    if !is_valid_submission_id(&submission_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service.get_submission(&token, &submission_id).await {
        Ok(submission) => {
            let version = submission.resource_version;
            resource_response(StatusCode::OK, submission, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn put_submission_part(
    State(service): State<StoreControlService>,
    Path((submission_id, part_name)): Path<(String, String)>,
    headers: HeaderMap,
    payload: Result<Bytes, BytesRejection>,
) -> Response {
    let request_id = request_id();
    if !is_valid_submission_id(&submission_id)
        || !valid_part_name(&part_name)
        || headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            != Some("application/octet-stream")
    {
        return ApiError::invalid_request().response(request_id);
    }
    let body = match payload {
        Ok(body) => body,
        Err(rejection) => {
            let error = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                ApiError::payload_too_large()
            } else {
                ApiError::invalid_request()
            };
            return error.response(request_id);
        }
    };
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let expected_version = match expected_version(&headers) {
        Ok(value) => value,
        Err(error) => return error.response(request_id),
    };
    let chunk_sha256 = match content_sha256(&headers) {
        Ok(value) => value,
        Err(error) => return error.response(request_id),
    };
    let range = match upload_range(&headers, body.len()) {
        Ok(value) => value,
        Err(error) => return error.response(request_id),
    };
    if sha256_hex(&body) != chunk_sha256 {
        return ApiError::digest_mismatch().response(request_id);
    }
    match service
        .upload_submission_part(
            &token,
            &idempotency_key,
            &request_id,
            UploadMutation {
                submission_id: &submission_id,
                part_name: &part_name,
                expected_version,
                range,
                chunk_sha256: &chunk_sha256,
                body: &body,
            },
        )
        .await
    {
        Ok(version) => empty_resource_response(version, request_id),
        Err(error) => error.response(request_id),
    }
}

async fn mutate_submission(
    State(service): State<StoreControlService>,
    Path(submission_action): Path<String>,
    headers: HeaderMap,
    payload: Result<Bytes, BytesRejection>,
) -> Response {
    let request_id = request_id();
    let Some((submission_id, action)) = submission_action.split_once(':') else {
        return ApiError::invalid_request().response(request_id);
    };
    if !is_valid_submission_id(submission_id) || action.contains(':') {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let expected_version = match expected_version(&headers) {
        Ok(value) => value,
        Err(error) => return error.response(request_id),
    };
    let body = match payload {
        Ok(body) => body,
        Err(rejection) => {
            let error = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                ApiError::payload_too_large()
            } else {
                ApiError::invalid_request()
            };
            return error.response(request_id);
        }
    };
    let result = match action {
        "finalize" => {
            if require_json_content_type(&headers).is_err() {
                return ApiError::invalid_request().response(request_id);
            }
            let request: FinalizeSubmissionRequest = match serde_json::from_slice(&body) {
                Ok(request) => request,
                Err(_) => return ApiError::invalid_request().response(request_id),
            };
            if !is_valid_sha256(&request.content_sha256) {
                return ApiError::invalid_request().response(request_id);
            }
            service
                .finalize_submission(
                    &token,
                    &idempotency_key,
                    &request_id,
                    submission_id,
                    expected_version,
                    &request.content_sha256,
                )
                .await
                .map(|submission| (StatusCode::ACCEPTED, submission))
        }
        "withdraw" if body.is_empty() => service
            .withdraw_submission(
                &token,
                &idempotency_key,
                &request_id,
                submission_id,
                expected_version,
            )
            .await
            .map(|submission| (StatusCode::OK, submission)),
        _ => return ApiError::invalid_request().response(request_id),
    };
    match result {
        Ok((status, submission)) => {
            let version = submission.resource_version;
            resource_response(status, submission, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn list_review_queue(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
    query: Result<Query<ReviewQueueQuery>, QueryRejection>,
) -> Response {
    let request_id = request_id();
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return ApiError::invalid_request().response(request_id),
    };
    let limit = usize::from(query.limit.unwrap_or(25));
    if !(1..=50).contains(&limit) {
        return ApiError::invalid_request().response(request_id);
    }
    let cursor = match query.cursor.as_deref().map(parse_review_cursor).transpose() {
        Ok(cursor) => cursor,
        Err(error) => return error.response(request_id),
    };
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service.list_review_queue(&token, cursor, limit).await {
        Ok(queue) => json_response(StatusCode::OK, queue, request_id),
        Err(error) => error.response(request_id),
    }
}

async fn get_review_submission_detail(
    State(service): State<StoreControlService>,
    Path(submission_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id();
    if !is_valid_submission_id(&submission_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service
        .get_review_submission_detail(&token, &submission_id)
        .await
    {
        Ok(detail) => {
            let version = detail.submission.resource_version;
            resource_response(StatusCode::OK, detail, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn begin_review(
    State(service): State<StoreControlService>,
    Path(submission_action): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id();
    let Some(submission_id) = submission_action.strip_suffix(":begin") else {
        return ApiError::invalid_request().response(request_id);
    };
    if !is_valid_submission_id(submission_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let expected_version = match expected_version(&headers) {
        Ok(value) => value,
        Err(error) => return error.response(request_id),
    };
    match service
        .begin_review(
            &token,
            &idempotency_key,
            &request_id,
            submission_id,
            expected_version,
        )
        .await
    {
        Ok(submission) => {
            let version = submission.resource_version;
            resource_response(StatusCode::OK, submission, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn decide_review(
    State(service): State<StoreControlService>,
    Path(submission_id): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<ReviewDecisionRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    if !is_valid_submission_id(&submission_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let expected_version = match expected_version(&headers) {
        Ok(value) => value,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return json_rejection(rejection).response(request_id),
    };
    if let Err(error) = validate_review_decision(&request) {
        return error.response(request_id);
    }
    match service
        .decide_review(
            &token,
            &idempotency_key,
            &request_id,
            &submission_id,
            expected_version,
            &request,
        )
        .await
    {
        Ok(submission) => {
            let version = submission.resource_version;
            resource_response(StatusCode::CREATED, submission, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn post_review_message(
    State(service): State<StoreControlService>,
    Path(submission_id): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<ReviewMessageRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    if !is_valid_submission_id(&submission_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return json_rejection(rejection).response(request_id),
    };
    if let Err(error) = validate_review_text(&request.body, false) {
        return error.response(request_id);
    }
    match service
        .post_review_message(
            &token,
            &idempotency_key,
            &request_id,
            &submission_id,
            &request,
        )
        .await
    {
        Ok(message) => json_response(StatusCode::CREATED, message, request_id),
        Err(error) => error.response(request_id),
    }
}

async fn post_release(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
    payload: Result<Json<CreateReleaseRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return json_rejection(rejection).response(request_id),
    };
    if !is_valid_submission_id(&request.submission_id)
        || !(1..=100).contains(&request.rollout_percent)
    {
        return ApiError::invalid_request().response(request_id);
    }
    match service
        .create_release(&token, &idempotency_key, &request_id, &request)
        .await
    {
        Ok(release) => {
            let version = release.resource_version;
            resource_response(StatusCode::CREATED, release, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn get_today_editorial(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id();
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service.get_today_editorial(&token).await {
        Ok(layout) => {
            let version = layout.resource_version;
            resource_response(StatusCode::OK, layout, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn list_editorial_releases(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
    query: Result<Query<EditorialReleaseQuery>, QueryRejection>,
) -> Response {
    let request_id = request_id();
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return ApiError::invalid_request().response(request_id),
    };
    let limit = usize::from(query.limit.unwrap_or(25));
    if !(1..=50).contains(&limit) {
        return ApiError::invalid_request().response(request_id);
    }
    let cursor = match query
        .cursor
        .as_deref()
        .map(parse_editorial_release_cursor)
        .transpose()
    {
        Ok(cursor) => cursor,
        Err(error) => return error.response(request_id),
    };
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service.list_editorial_releases(&token, cursor, limit).await {
        Ok(releases) => json_response(StatusCode::OK, releases, request_id),
        Err(error) => error.response(request_id),
    }
}

async fn post_today_editorial(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
    payload: Result<Json<EditorialLayoutRequest>, JsonRejection>,
) -> Response {
    if headers.contains_key(IF_MATCH) {
        return ApiError::invalid_request().response(request_id());
    }
    replace_today_editorial_request(service, headers, payload, None, StatusCode::CREATED).await
}

async fn put_today_editorial(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
    payload: Result<Json<EditorialLayoutRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    let expected_version = match expected_version(&headers) {
        Ok(version) => version,
        Err(error) => return error.response(request_id),
    };
    replace_today_editorial_request_with_id(
        service,
        headers,
        payload,
        Some(expected_version),
        StatusCode::OK,
        request_id,
    )
    .await
}

async fn replace_today_editorial_request(
    service: StoreControlService,
    headers: HeaderMap,
    payload: Result<Json<EditorialLayoutRequest>, JsonRejection>,
    expected_version: Option<u64>,
    status: StatusCode,
) -> Response {
    let request_id = request_id();
    replace_today_editorial_request_with_id(
        service,
        headers,
        payload,
        expected_version,
        status,
        request_id,
    )
    .await
}

async fn replace_today_editorial_request_with_id(
    service: StoreControlService,
    headers: HeaderMap,
    payload: Result<Json<EditorialLayoutRequest>, JsonRejection>,
    expected_version: Option<u64>,
    status: StatusCode,
    request_id: String,
) -> Response {
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return json_rejection(rejection).response(request_id),
    };
    match service
        .replace_today_editorial(
            &token,
            &idempotency_key,
            &request_id,
            expected_version,
            &request,
        )
        .await
    {
        Ok(layout) => {
            let version = layout.resource_version;
            resource_response(status, layout, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn get_release(
    State(service): State<StoreControlService>,
    Path(release_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id();
    if !is_valid_release_id(&release_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service.get_release(&token, &release_id).await {
        Ok(release) => {
            let version = release.resource_version;
            resource_response(StatusCode::OK, release, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn mutate_release(
    State(service): State<StoreControlService>,
    Path(release_action): Path<String>,
    headers: HeaderMap,
    payload: Result<Bytes, BytesRejection>,
) -> Response {
    let request_id = request_id();
    let Some((release_id, action_name)) = release_action.split_once(':') else {
        return ApiError::invalid_request().response(request_id);
    };
    if !is_valid_release_id(release_id) || action_name.contains(':') {
        return ApiError::invalid_request().response(request_id);
    }
    let body = match payload {
        Ok(body) => body,
        Err(rejection) => {
            let error = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                ApiError::payload_too_large()
            } else {
                ApiError::invalid_request()
            };
            return error.response(request_id);
        }
    };
    let action = match parse_release_action(action_name, &headers, &body) {
        Ok(action) => action,
        Err(error) => return error.response(request_id),
    };
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let expected_version = match expected_version(&headers) {
        Ok(value) => value,
        Err(error) => return error.response(request_id),
    };
    let response_status = action.response_status();
    match service
        .mutate_release(
            &token,
            &idempotency_key,
            &request_id,
            release_id,
            expected_version,
            &action,
        )
        .await
    {
        Ok(release) => {
            let version = release.resource_version;
            resource_response(response_status, release, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

fn parse_release_action(
    action_name: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<ReleaseAction, ApiError> {
    match action_name {
        "schedule" => {
            require_json_content_type(headers)?;
            let request: ScheduleReleaseRequest =
                serde_json::from_slice(body).map_err(|_| ApiError::invalid_request())?;
            if request.publish_unix_seconds == 0 || request.publish_unix_seconds > i64::MAX as u64 {
                return Err(ApiError::invalid_request());
            }
            Ok(ReleaseAction::Schedule {
                publish_unix_seconds: request.publish_unix_seconds,
            })
        }
        "publish" if body.is_empty() => Ok(ReleaseAction::Publish),
        "pause" if body.is_empty() => Ok(ReleaseAction::Pause),
        "resume" if body.is_empty() => Ok(ReleaseAction::Resume),
        "remove" => {
            require_json_content_type(headers)?;
            let request: RemovalRequest =
                serde_json::from_slice(body).map_err(|_| ApiError::invalid_request())?;
            if !is_valid_reason_code(&request.reason_code)
                || validate_review_text(&request.note, false).is_err()
            {
                return Err(ApiError::invalid_request());
            }
            Ok(ReleaseAction::Remove {
                reason_code: request.reason_code,
                note: request.note,
            })
        }
        _ => Err(ApiError::invalid_request()),
    }
}

fn require_json_content_type(headers: &HeaderMap) -> Result<(), ApiError> {
    let is_json = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    if is_json {
        Ok(())
    } else {
        Err(ApiError::invalid_request())
    }
}

fn json_rejection(rejection: JsonRejection) -> ApiError {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::payload_too_large()
    } else {
        ApiError::invalid_request()
    }
}

fn json_response<T: Serialize>(status: StatusCode, resource: T, request_id: String) -> Response {
    let mut response = (status, Json(resource)).into_response();
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

fn oauth_json_response<T: Serialize>(
    status: StatusCode,
    resource: T,
    request_id: String,
) -> Response {
    let mut response = json_response(status, resource, request_id);
    add_oauth_cache_headers(&mut response);
    response
}

fn metrics_json_response<T: Serialize>(
    status: StatusCode,
    resource: T,
    request_id: String,
) -> Response {
    let mut response = json_response(status, resource, request_id);
    add_oauth_cache_headers(&mut response);
    response
}

fn metrics_error_response(error: ApiError, request_id: String) -> Response {
    let mut response = error.response(request_id);
    add_oauth_cache_headers(&mut response);
    response
}

fn oauth_empty_response(request_id: String) -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    add_oauth_cache_headers(&mut response);
    response
}

fn oauth_error_response(error: ApiError, request_id: String) -> Response {
    let mut response = error.response(request_id);
    add_oauth_cache_headers(&mut response);
    response
}

fn add_oauth_cache_headers(response: &mut Response) {
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("pragma", HeaderValue::from_static("no-cache"));
}

async fn fallback() -> Response {
    ApiError::not_found().response(request_id())
}

async fn method_not_allowed() -> Response {
    ApiError::method_not_allowed().response(request_id())
}

fn resource_response<T: Serialize>(
    status: StatusCode,
    resource: T,
    resource_version: u64,
    request_id: String,
) -> Response {
    let etag = format!("\"{resource_version}\"");
    let mut response = (status, Json(resource)).into_response();
    if let Ok(value) = HeaderValue::from_str(&etag) {
        response.headers_mut().insert(ETAG, value);
    }
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

fn empty_resource_response(resource_version: u64, request_id: String) -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&format!("\"{resource_version}\"")) {
        response.headers_mut().insert(ETAG, value);
    }
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

fn bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(ApiError::unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .filter(|token| {
            (32..=512).contains(&token.len()) && token.bytes().all(|byte| byte.is_ascii_graphic())
        })
        .ok_or_else(ApiError::unauthorized)?;
    Ok(token.to_owned())
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|key| {
            (16..=128).contains(&key.len())
                && key.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-')
                })
        })
        .ok_or_else(ApiError::invalid_request)?;
    Ok(key.to_owned())
}

fn expected_version(headers: &HeaderMap) -> Result<u64, ApiError> {
    let value = headers
        .get(IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix('"'))
        .and_then(|value| value.strip_suffix('"'))
        .filter(|value| is_canonical_number(value))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(ApiError::invalid_request)?;
    Ok(value)
}

fn content_sha256(headers: &HeaderMap) -> Result<String, ApiError> {
    headers
        .get("content-sha256")
        .and_then(|value| value.to_str().ok())
        .filter(|value| is_valid_sha256(value))
        .map(str::to_owned)
        .ok_or_else(ApiError::invalid_request)
}

fn upload_range(headers: &HeaderMap, body_bytes: usize) -> Result<UploadRange, ApiError> {
    let value = headers
        .get("content-range")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("bytes "))
        .ok_or_else(ApiError::invalid_request)?;
    let (range, total) = value
        .split_once('/')
        .ok_or_else(ApiError::invalid_request)?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(ApiError::invalid_request)?;
    if !is_canonical_number(start) || !is_canonical_number(end) || !is_canonical_number(total) {
        return Err(ApiError::invalid_request());
    }
    let start = start
        .parse::<u64>()
        .map_err(|_| ApiError::invalid_request())?;
    let end = end
        .parse::<u64>()
        .map_err(|_| ApiError::invalid_request())?;
    let total = total
        .parse::<u64>()
        .map_err(|_| ApiError::invalid_request())?;
    let declared_bytes = end
        .checked_sub(start)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(ApiError::invalid_request)?;
    if body_bytes == 0
        || body_bytes > MAX_UPLOAD_CHUNK_BYTES
        || declared_bytes != body_bytes as u64
        || total == 0
        || end >= total
    {
        return Err(ApiError::invalid_request());
    }
    Ok(UploadRange { start, end, total })
}

fn is_canonical_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_valid_device_code(value: &str) -> bool {
    value.strip_prefix("cp0_dc_").is_some_and(|suffix| {
        suffix.len() == 64
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn is_valid_user_code(value: &str) -> bool {
    value.len() == 14
        && value.bytes().enumerate().all(|(index, byte)| match index {
            4 | 9 => byte == b'-',
            _ => byte.is_ascii_hexdigit() && !byte.is_ascii_lowercase(),
        })
}

fn is_valid_submission_id(value: &str) -> bool {
    valid_prefixed_hex_id(value, "sub_")
}

fn is_valid_team_id(value: &str) -> bool {
    valid_prefixed_hex_id(value, "team_")
}

fn is_valid_member_id(value: &str) -> bool {
    valid_prefixed_hex_id(value, "member_")
}

fn is_valid_team_role(value: &str) -> bool {
    matches!(value, "owner" | "developer" | "release-manager" | "viewer")
}

fn is_valid_release_id(value: &str) -> bool {
    valid_prefixed_hex_id(value, "rel_")
}

fn validate_review_decision(request: &ReviewDecisionRequest) -> Result<(), ApiError> {
    review_decision_state(&request.decision)?;
    if request.reason_codes.len() > 16
        || request
            .reason_codes
            .iter()
            .any(|code| !is_valid_reason_code(code))
        || request
            .reason_codes
            .iter()
            .enumerate()
            .any(|(index, code)| request.reason_codes[..index].contains(code))
    {
        return Err(ApiError::invalid_request());
    }
    let requires_explanation = request.decision != "approved";
    if requires_explanation && request.reason_codes.is_empty() {
        return Err(ApiError::invalid_request());
    }
    validate_review_text(&request.note, !requires_explanation)
}

fn review_decision_state(value: &str) -> Result<SubmissionState, ApiError> {
    match value {
        "needs-changes" => Ok(SubmissionState::NeedsChanges),
        "approved" => Ok(SubmissionState::Approved),
        "rejected" => Ok(SubmissionState::Rejected),
        _ => Err(ApiError::invalid_request()),
    }
}

fn validate_review_text(value: &str, allow_empty: bool) -> Result<(), ApiError> {
    let characters = value.chars().count();
    if characters > 2000
        || (!allow_empty && characters == 0)
        || value.trim() != value
        || value.chars().any(|character| {
            character == '\0' || (character.is_control() && !matches!(character, '\n' | '\t'))
        })
    {
        return Err(ApiError::invalid_request());
    }
    Ok(())
}

fn is_valid_reason_code(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn encode_review_cursor(created_unix_seconds: u64, submission_id: &str) -> String {
    format!("{created_unix_seconds:016x}.{submission_id}")
}

fn parse_review_cursor(value: &str) -> Result<ReviewCursor, ApiError> {
    if value.len() != 53 {
        return Err(ApiError::invalid_request());
    }
    let (timestamp, submission_id) = value
        .split_once('.')
        .ok_or_else(ApiError::invalid_request)?;
    let created_unix_seconds = u64::from_str_radix(timestamp, 16)
        .ok()
        .filter(|value| *value > 0 && *value <= i64::MAX as u64)
        .ok_or_else(ApiError::invalid_request)?;
    if !timestamp
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || !is_valid_submission_id(submission_id)
        || encode_review_cursor(created_unix_seconds, submission_id) != value
    {
        return Err(ApiError::invalid_request());
    }
    Ok(ReviewCursor {
        created_unix_seconds: created_unix_seconds as i64,
        submission_id: submission_id.to_owned(),
    })
}

fn encode_editorial_release_cursor(catalog_sequence: u64, release_id: &str) -> String {
    format!("{catalog_sequence:016x}.{release_id}")
}

fn parse_editorial_release_cursor(value: &str) -> Result<EditorialReleaseCursor, ApiError> {
    if value.len() != 53 {
        return Err(ApiError::invalid_request());
    }
    let (sequence, release_id) = value
        .split_once('.')
        .ok_or_else(ApiError::invalid_request)?;
    let catalog_sequence = u64::from_str_radix(sequence, 16)
        .ok()
        .filter(|value| *value > 0 && *value <= i64::MAX as u64)
        .ok_or_else(ApiError::invalid_request)?;
    if !sequence
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || !is_valid_release_id(release_id)
        || encode_editorial_release_cursor(catalog_sequence, release_id) != value
    {
        return Err(ApiError::invalid_request());
    }
    Ok(EditorialReleaseCursor {
        catalog_sequence: catalog_sequence as i64,
        release_id: release_id.to_owned(),
    })
}

fn valid_part_name(value: &str) -> bool {
    matches!(value, "package" | "listing")
        || value
            .strip_prefix("asset-")
            .is_some_and(|suffix| matches!(suffix, "0" | "1" | "2" | "3" | "4" | "5"))
}

fn valid_prefixed_hex_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn row_version(row: &sqlx::postgres::PgRow) -> Result<u64, ApiError> {
    let value: i64 = row.get("resource_version");
    u64::try_from(value).map_err(|_| ApiError::internal())
}

fn request_id() -> String {
    prefixed_uuid("req_")
}

fn prefixed_uuid(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().simple())
}

fn random_secret(prefix: &str) -> String {
    format!(
        "{prefix}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn random_user_code() -> String {
    let random = Uuid::new_v4().simple().to_string().to_ascii_uppercase();
    format!("{}-{}-{}", &random[0..4], &random[4..8], &random[8..12])
}

fn sha256_hex(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_retryable_transaction_error(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| matches!(code.as_ref(), "40001" | "40P01"))
}

fn is_submission_revision_conflict(error: &sqlx::Error) -> bool {
    error.as_database_error().is_some_and(|error| {
        error.code().as_deref() == Some("23505")
            && error.constraint() == Some("submissions_app_id_version_revision_key")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_validators_reject_ambiguous_credentials() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer token-with-safe-bytes-1234567890"),
        );
        headers.insert(
            "idempotency-key",
            HeaderValue::from_static("request-key-0001"),
        );
        assert!(bearer_token(&headers).is_ok());
        assert!(idempotency_key(&headers).is_ok());

        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("bearer token-with-safe-bytes-1234567890"),
        );
        assert!(bearer_token(&headers).is_err());
        headers.insert("idempotency-key", HeaderValue::from_static("short"));
        assert!(idempotency_key(&headers).is_err());
    }

    #[test]
    fn retry_classifier_is_closed_by_default() {
        let error = sqlx::Error::RowNotFound;
        assert!(!is_retryable_transaction_error(&error));
    }

    #[test]
    fn review_contract_rejects_ambiguous_input() {
        let submission_id = "sub_0123456789abcdef0123456789abcdef";
        let cursor = encode_review_cursor(1_700_000_000, submission_id);
        let parsed = parse_review_cursor(&cursor).unwrap();
        assert_eq!(parsed.created_unix_seconds, 1_700_000_000);
        assert_eq!(parsed.submission_id, submission_id);
        assert!(parse_review_cursor(&cursor.to_uppercase()).is_err());
        assert!(
            parse_review_cursor("0000000000000000.sub_0123456789abcdef0123456789abcdef").is_err()
        );

        let approved = ReviewDecisionRequest {
            decision: "approved".to_owned(),
            reason_codes: Vec::new(),
            note: String::new(),
        };
        assert!(validate_review_decision(&approved).is_ok());
        let duplicate = ReviewDecisionRequest {
            decision: "needs-changes".to_owned(),
            reason_codes: vec!["privacy".to_owned(), "privacy".to_owned()],
            note: "Explain the issue.".to_owned(),
        };
        assert!(validate_review_decision(&duplicate).is_err());
        let missing_explanation = ReviewDecisionRequest {
            decision: "rejected".to_owned(),
            reason_codes: vec!["malware".to_owned()],
            note: String::new(),
        };
        assert!(validate_review_decision(&missing_explanation).is_err());
        assert!(validate_review_text(&"x".repeat(2000), false).is_ok());
        assert!(validate_review_text(&"x".repeat(2001), false).is_err());
    }

    #[test]
    fn release_contract_rejects_ambiguous_actions_and_bodies() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        let schedule = br#"{"publish_unix_seconds":1700000000}"#;
        assert!(matches!(
            parse_release_action("schedule", &headers, schedule).unwrap(),
            ReleaseAction::Schedule {
                publish_unix_seconds: 1_700_000_000
            }
        ));
        assert!(parse_release_action("publish", &headers, b"").is_ok());
        assert!(parse_release_action("publish", &headers, b"{}").is_err());
        assert!(parse_release_action("publish-extra", &headers, b"").is_err());
        assert!(parse_release_action("schedule", &headers, b"{}").is_err());
        assert!(is_valid_release_id("rel_0123456789abcdef0123456789abcdef"));
        assert!(!is_valid_release_id("rel_0123456789ABCDEF0123456789ABCDEF"));
    }

    #[test]
    fn editorial_release_cursor_is_canonical_and_bounded() {
        let release_id = "rel_0123456789abcdef0123456789abcdef";
        let cursor = encode_editorial_release_cursor(901, release_id);
        let parsed = parse_editorial_release_cursor(&cursor).unwrap();
        assert_eq!(parsed.catalog_sequence, 901);
        assert_eq!(parsed.release_id, release_id);
        assert!(parse_editorial_release_cursor(&cursor.to_uppercase()).is_err());
        assert!(
            parse_editorial_release_cursor("0000000000000000.rel_0123456789abcdef0123456789abcdef")
                .is_err()
        );
        assert!(
            parse_editorial_release_cursor(
                "0000000000000385.release_0123456789abcdef0123456789abcdef"
            )
            .is_err()
        );
    }

    #[test]
    fn parses_only_canonical_versions_and_ranges() {
        let mut headers = HeaderMap::new();
        headers.insert(IF_MATCH, HeaderValue::from_static("\"17\""));
        headers.insert(
            "content-range",
            HeaderValue::from_static("bytes 262144-262160/262161"),
        );
        assert_eq!(expected_version(&headers).unwrap(), 17);
        let range = upload_range(&headers, 17).unwrap();
        assert_eq!(range.start, 262_144);
        assert_eq!(range.end, 262_160);
        assert_eq!(range.total, 262_161);

        headers.insert(IF_MATCH, HeaderValue::from_static("W/\"17\""));
        assert!(expected_version(&headers).is_err());
        headers.insert("content-range", HeaderValue::from_static("bytes 01-17/18"));
        assert!(upload_range(&headers, 17).is_err());
    }

    #[tokio::test]
    async fn object_store_rejects_relative_roots() {
        let error = ContentObjectStore::open(FilePath::new("relative-store"))
            .await
            .expect_err("relative object root must fail closed");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
