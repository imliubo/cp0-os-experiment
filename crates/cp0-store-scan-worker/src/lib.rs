use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cp0_store_metadata::ImageAsset;
use cp0_store_scan::{ScanAsset, ScanDisposition, ScanInput, ScanReport};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

const LEASE_SECONDS: i64 = 60;
const MAX_ATTEMPTS: i16 = 8;
const MAX_CHUNK_BYTES: usize = 256 * 1024;

#[derive(Debug)]
pub enum WorkerError {
    Configuration(&'static str),
    Database(sqlx::Error),
    Migration(sqlx::migrate::MigrateError),
    Io(io::Error),
    InvalidState(&'static str),
}

impl fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => {
                write!(formatter, "invalid scanner configuration: {message}")
            }
            Self::Database(_) => formatter.write_str("scanner database operation failed"),
            Self::Migration(_) => formatter.write_str("scanner database migration failed"),
            Self::Io(_) => formatter.write_str("scanner object operation failed"),
            Self::InvalidState(message) => write!(formatter, "invalid scanner state: {message}"),
        }
    }
}

impl std::error::Error for WorkerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Migration(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Configuration(_) | Self::InvalidState(_) => None,
        }
    }
}

impl From<sqlx::Error> for WorkerError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<io::Error> for WorkerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Idle,
    Completed {
        submission_id: String,
        disposition: ScanDisposition,
    },
    Deferred {
        submission_id: String,
        failed_permanently: bool,
    },
}

#[derive(Clone)]
pub struct ScanWorker {
    pool: PgPool,
    objects: ObjectReader,
    worker_id: Arc<String>,
}

impl ScanWorker {
    pub async fn open(
        pool: PgPool,
        object_root: impl AsRef<Path>,
        worker_id: impl Into<String>,
    ) -> Result<Self, WorkerError> {
        let worker_id = worker_id.into();
        if !valid_worker_id(&worker_id) {
            return Err(WorkerError::Configuration("worker ID is invalid"));
        }
        Ok(Self {
            pool,
            objects: ObjectReader::open(object_root.as_ref()).await?,
            worker_id: Arc::new(worker_id),
        })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn run_once(&self) -> Result<RunOutcome, WorkerError> {
        self.enqueue_one().await?;
        self.recover_expired_leases().await?;
        let Some(job) = self.claim_one().await? else {
            return Ok(RunOutcome::Idle);
        };
        let bundle = match self.load_bundle(&job).await {
            Ok(bundle) => bundle,
            Err(error) => {
                let failed_permanently = self.defer_job(&job, "object-unavailable").await?;
                let _ = error;
                return Ok(RunOutcome::Deferred {
                    submission_id: job.submission_id,
                    failed_permanently,
                });
            }
        };
        let assets = bundle
            .assets
            .iter()
            .map(|asset| ScanAsset {
                descriptor: &asset.descriptor,
                encoded: &asset.encoded,
            })
            .collect::<Vec<_>>();
        let report = cp0_store_scan::scan(&ScanInput {
            expected_app_id: &bundle.app_id,
            expected_version: &bundle.version,
            expected_default_locale: &bundle.default_locale,
            package: &bundle.package,
            listing: &bundle.listing,
            assets: &assets,
            trusted_developer_keys: &bundle.trusted_developer_keys,
        });
        let disposition = report.disposition;
        if let Err(error) = self.complete_job(&job, &report).await {
            let _ = self.defer_job(&job, "commit-failed").await;
            return Err(error);
        }
        Ok(RunOutcome::Completed {
            submission_id: job.submission_id,
            disposition,
        })
    }

    async fn enqueue_one(&self) -> Result<bool, WorkerError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let row = sqlx::query(
            "SELECT event_id, aggregate_id, aggregate_version, payload \
             FROM outbox_events WHERE topic = 'submission.scan-requested' \
             AND aggregate_kind = 'submission' AND published_unix_seconds IS NULL \
             ORDER BY created_unix_seconds, event_id FOR UPDATE SKIP LOCKED LIMIT 1",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(false);
        };
        let event_id: String = row.get("event_id");
        let submission_id: String = row.get("aggregate_id");
        let source_resource_version: i64 = row.get("aggregate_version");
        let payload: Value = row.get("payload");
        let content_sha256 = payload
            .get("content_sha256")
            .and_then(Value::as_str)
            .filter(|value| valid_sha256(value))
            .ok_or(WorkerError::InvalidState("scan event digest is invalid"))?;
        if payload.get("submission_id").and_then(Value::as_str) != Some(&submission_id) {
            return Err(WorkerError::InvalidState(
                "scan event submission identity is invalid",
            ));
        }
        let now = database_now(&mut transaction).await?;
        sqlx::query(
            "INSERT INTO submission_scan_jobs (event_id, submission_id, source_resource_version, \
             source_content_sha256, state, created_unix_seconds) \
             VALUES ($1, $2, $3, $4, 'queued', $5) ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(&event_id)
        .bind(&submission_id)
        .bind(source_resource_version)
        .bind(content_sha256)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let updated = sqlx::query(
            "UPDATE outbox_events SET published_unix_seconds = $1, attempts = attempts + 1 \
             WHERE event_id = $2 AND published_unix_seconds IS NULL",
        )
        .bind(now)
        .bind(&event_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(WorkerError::InvalidState("scan event claim was lost"));
        }
        transaction.commit().await?;
        Ok(true)
    }

    async fn recover_expired_leases(&self) -> Result<(), WorkerError> {
        sqlx::query(
            "UPDATE submission_scan_jobs SET \
             state = CASE WHEN attempts >= 8 THEN 'failed' ELSE 'queued' END, \
             lease_token = NULL, leased_until_unix_seconds = NULL, \
             last_error_code = CASE WHEN attempts >= 8 THEN 'lease-exhausted' ELSE 'lease-expired' END, \
             completed_unix_seconds = CASE WHEN attempts >= 8 \
                 THEN EXTRACT(EPOCH FROM clock_timestamp())::BIGINT ELSE NULL END \
             WHERE state = 'running' AND leased_until_unix_seconds <= \
             EXTRACT(EPOCH FROM clock_timestamp())::BIGINT",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn claim_one(&self) -> Result<Option<ScanJob>, WorkerError> {
        let lease_token = prefixed_uuid("lease_");
        let row = sqlx::query(
            "WITH candidate AS ( \
                 SELECT event_id FROM submission_scan_jobs \
                 WHERE state = 'queued' AND attempts < 8 \
                 ORDER BY created_unix_seconds, event_id FOR UPDATE SKIP LOCKED LIMIT 1 \
             ) UPDATE submission_scan_jobs job SET state = 'running', lease_token = $1, \
                 leased_until_unix_seconds = EXTRACT(EPOCH FROM clock_timestamp())::BIGINT + $2, \
                 attempts = attempts + 1, last_error_code = NULL \
             FROM candidate WHERE job.event_id = candidate.event_id \
             RETURNING job.event_id, job.submission_id, job.source_resource_version, \
                 job.source_content_sha256, job.lease_token, job.attempts",
        )
        .bind(&lease_token)
        .bind(LEASE_SECONDS)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(ScanJob {
                event_id: row.get("event_id"),
                submission_id: row.get("submission_id"),
                source_resource_version: u64::try_from(
                    row.get::<i64, _>("source_resource_version"),
                )
                .map_err(|_| WorkerError::InvalidState("scan resource version is invalid"))?,
                source_content_sha256: row.get("source_content_sha256"),
                lease_token: row.get("lease_token"),
                attempts: row.get("attempts"),
            })
        })
        .transpose()
    }

    async fn load_bundle(&self, job: &ScanJob) -> Result<ScanBundle, WorkerError> {
        let row = sqlx::query(
            "SELECT submission.app_id, submission.version, submission.state, \
             submission.package_sha256, submission.package_bytes, submission.listing_sha256, \
             submission.listing_bytes, submission.assets, submission.resource_version, \
             submission.finalized_content_sha256, app.owner_team_id, app.default_locale \
             FROM submissions submission JOIN apps app ON app.app_id = submission.app_id \
             WHERE submission.submission_id = $1",
        )
        .bind(&job.submission_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(WorkerError::InvalidState("scan submission is missing"))?;
        let state: String = row.get("state");
        let resource_version = u64::try_from(row.get::<i64, _>("resource_version"))
            .map_err(|_| WorkerError::InvalidState("scan resource version is invalid"))?;
        let finalized_content_sha256: Option<String> = row.get("finalized_content_sha256");
        if state != "processing"
            || resource_version != job.source_resource_version
            || finalized_content_sha256.as_deref() != Some(&job.source_content_sha256)
        {
            return Err(WorkerError::InvalidState("scan source is stale"));
        }
        let app_id: String = row.get("app_id");
        let version: String = row.get("version");
        let default_locale: String = row.get("default_locale");
        let package_sha256: String = row.get("package_sha256");
        let package_bytes = positive_size(row.get::<i64, _>("package_bytes"))?;
        let listing_sha256: String = row.get("listing_sha256");
        let listing_bytes = positive_size(row.get::<i64, _>("listing_bytes"))?;
        let assets_value: Value = row.get("assets");
        let asset_descriptors: Vec<ImageAsset> = serde_json::from_value(assets_value)
            .map_err(|_| WorkerError::InvalidState("scan asset descriptors are invalid"))?;
        if !(2..=6).contains(&asset_descriptors.len()) {
            return Err(WorkerError::InvalidState("scan asset count is invalid"));
        }
        if cp0_store_scan::submission_content_sha256(
            &package_sha256,
            &listing_sha256,
            &asset_descriptors,
        ) != job.source_content_sha256
        {
            return Err(WorkerError::InvalidState("scan content digest is invalid"));
        }
        let package = self
            .load_part(
                &job.submission_id,
                "package",
                &package_sha256,
                package_bytes,
            )
            .await?;
        let listing = self
            .load_part(
                &job.submission_id,
                "listing",
                &listing_sha256,
                listing_bytes,
            )
            .await?;
        let mut assets = Vec::with_capacity(asset_descriptors.len());
        for (index, descriptor) in asset_descriptors.into_iter().enumerate() {
            let encoded = self
                .load_part(
                    &job.submission_id,
                    &format!("asset-{index}"),
                    &descriptor.sha256,
                    usize::try_from(descriptor.bytes)
                        .map_err(|_| WorkerError::InvalidState("scan asset size is invalid"))?,
                )
                .await?;
            assets.push(OwnedAsset {
                descriptor,
                encoded,
            });
        }
        let owner_team_id: String = row.get("owner_team_id");
        let key_rows = sqlx::query(
            "SELECT public_key, fingerprint_sha256 FROM developer_keys \
             WHERE team_id = $1 AND state = 'active' \
             ORDER BY key_id",
        )
        .bind(owner_team_id)
        .fetch_all(&self.pool)
        .await?;
        let trusted_developer_keys = key_rows
            .into_iter()
            .map(|row| {
                let encoded: Vec<u8> = row.get("public_key");
                let fingerprint: String = row.get("fingerprint_sha256");
                if sha256_hex(&encoded) != fingerprint {
                    return Err(WorkerError::InvalidState(
                        "developer key fingerprint is invalid",
                    ));
                }
                encoded
                    .try_into()
                    .map_err(|_| WorkerError::InvalidState("developer key length is invalid"))
            })
            .collect::<Result<Vec<[u8; 32]>, _>>()?;
        Ok(ScanBundle {
            app_id,
            version,
            default_locale,
            package,
            listing,
            assets,
            trusted_developer_keys,
        })
    }

    async fn load_part(
        &self,
        submission_id: &str,
        part_name: &str,
        expected_sha256: &str,
        expected_bytes: usize,
    ) -> Result<Vec<u8>, WorkerError> {
        let row = sqlx::query(
            "SELECT expected_sha256, expected_bytes, received_bytes \
             FROM submission_upload_parts WHERE submission_id = $1 AND part_name = $2",
        )
        .bind(submission_id)
        .bind(part_name)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(WorkerError::InvalidState("scan upload part is missing"))?;
        let stored_sha256: String = row.get("expected_sha256");
        let stored_bytes = positive_size(row.get::<i64, _>("expected_bytes"))?;
        let received_bytes = positive_size(row.get::<i64, _>("received_bytes"))?;
        if stored_sha256 != expected_sha256
            || stored_bytes != expected_bytes
            || received_bytes != expected_bytes
        {
            return Err(WorkerError::InvalidState("scan upload part is incomplete"));
        }
        let rows = sqlx::query(
            "SELECT chunk_offset, chunk_bytes, chunk_sha256 FROM submission_upload_chunks \
             WHERE submission_id = $1 AND part_name = $2 ORDER BY chunk_offset",
        )
        .bind(submission_id)
        .bind(part_name)
        .fetch_all(&self.pool)
        .await?;
        let mut encoded = Vec::with_capacity(expected_bytes);
        for row in rows {
            let offset = usize::try_from(row.get::<i64, _>("chunk_offset"))
                .map_err(|_| WorkerError::InvalidState("scan chunk offset is invalid"))?;
            let chunk_bytes = usize::try_from(row.get::<i32, _>("chunk_bytes"))
                .map_err(|_| WorkerError::InvalidState("scan chunk size is invalid"))?;
            let chunk_sha256: String = row.get("chunk_sha256");
            if offset != encoded.len()
                || chunk_bytes == 0
                || chunk_bytes > MAX_CHUNK_BYTES
                || !valid_sha256(&chunk_sha256)
            {
                return Err(WorkerError::InvalidState(
                    "scan chunk descriptor is invalid",
                ));
            }
            let chunk = self.objects.read_chunk(&chunk_sha256, chunk_bytes).await?;
            encoded.extend_from_slice(&chunk);
            if encoded.len() > expected_bytes {
                return Err(WorkerError::InvalidState(
                    "scan upload part exceeds its bound",
                ));
            }
        }
        if encoded.len() != expected_bytes || sha256_hex(&encoded) != expected_sha256 {
            return Err(WorkerError::InvalidState(
                "scan upload part digest is invalid",
            ));
        }
        Ok(encoded)
    }

    async fn complete_job(&self, job: &ScanJob, report: &ScanReport) -> Result<(), WorkerError> {
        let report_value = serde_json::to_value(report)
            .map_err(|_| WorkerError::InvalidState("scan report cannot be encoded"))?;
        let report_encoded = serde_json::to_vec(report)
            .map_err(|_| WorkerError::InvalidState("scan report cannot be encoded"))?;
        if report_encoded.len() > 32 * 1024 || report.findings.len() > cp0_store_scan::MAX_FINDINGS
        {
            return Err(WorkerError::InvalidState("scan report exceeds its bound"));
        }
        let report_sha256 = cp0_store_scan::report_sha256(report)
            .map_err(|_| WorkerError::InvalidState("scan report cannot be hashed"))?;
        let outcome = report.disposition.as_submission_state();
        let mut transaction = begin_serializable(&self.pool).await?;
        let job_row = sqlx::query(
            "SELECT state, lease_token, source_resource_version, source_content_sha256 \
             FROM submission_scan_jobs WHERE event_id = $1 FOR UPDATE",
        )
        .bind(&job.event_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(WorkerError::InvalidState("scan job is missing"))?;
        if job_row.get::<String, _>("state") != "running"
            || job_row.get::<Option<String>, _>("lease_token").as_deref() != Some(&job.lease_token)
            || u64::try_from(job_row.get::<i64, _>("source_resource_version")).ok()
                != Some(job.source_resource_version)
            || job_row.get::<String, _>("source_content_sha256") != job.source_content_sha256
        {
            return Err(WorkerError::InvalidState("scan lease is stale"));
        }
        let submission_row = sqlx::query(
            "SELECT state, resource_version, finalized_content_sha256 FROM submissions \
             WHERE submission_id = $1 FOR UPDATE",
        )
        .bind(&job.submission_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(WorkerError::InvalidState("scan submission is missing"))?;
        if submission_row.get::<String, _>("state") != "processing"
            || u64::try_from(submission_row.get::<i64, _>("resource_version")).ok()
                != Some(job.source_resource_version)
            || submission_row
                .get::<Option<String>, _>("finalized_content_sha256")
                .as_deref()
                != Some(&job.source_content_sha256)
        {
            return Err(WorkerError::InvalidState(
                "scan source changed before commit",
            ));
        }
        let now = database_now(&mut transaction).await?;
        let new_version = job
            .source_resource_version
            .checked_add(1)
            .ok_or(WorkerError::InvalidState("scan resource version overflow"))?;
        let scan_id = prefixed_uuid("scan_");
        sqlx::query(
            "INSERT INTO submission_scan_results (scan_id, event_id, submission_id, \
             source_resource_version, source_content_sha256, outcome, scanner_version, report, \
             report_sha256, created_unix_seconds) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&scan_id)
        .bind(&job.event_id)
        .bind(&job.submission_id)
        .bind(
            i64::try_from(job.source_resource_version)
                .map_err(|_| WorkerError::InvalidState("scan resource version is invalid"))?,
        )
        .bind(&job.source_content_sha256)
        .bind(outcome)
        .bind(&report.scanner_version)
        .bind(&report_value)
        .bind(&report_sha256)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let changed = sqlx::query(
            "UPDATE submissions SET state = $1, resource_version = $2 \
             WHERE submission_id = $3 AND state = 'processing' AND resource_version = $4",
        )
        .bind(outcome)
        .bind(
            i64::try_from(new_version)
                .map_err(|_| WorkerError::InvalidState("scan resource version is invalid"))?,
        )
        .bind(&job.submission_id)
        .bind(
            i64::try_from(job.source_resource_version)
                .map_err(|_| WorkerError::InvalidState("scan resource version is invalid"))?,
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(WorkerError::InvalidState("scan source update was lost"));
        }
        let completed = sqlx::query(
            "UPDATE submission_scan_jobs SET state = 'completed', lease_token = NULL, \
             leased_until_unix_seconds = NULL, completed_unix_seconds = $1 \
             WHERE event_id = $2 AND state = 'running' AND lease_token = $3",
        )
        .bind(now)
        .bind(&job.event_id)
        .bind(&job.lease_token)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if completed != 1 {
            return Err(WorkerError::InvalidState("scan completion lease was lost"));
        }
        let request_id = prefixed_uuid("scanreq_");
        let key_sha256 = sha256_hex(job.event_id.as_bytes());
        sqlx::query(
            "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
             object_id, before_state, after_state, resource_version, request_id, request_sha256, \
             idempotency_key_sha256) VALUES ($1, $2, 'submission.scan-completed', 'submission', \
             $3, 'processing', $4, $5, $6, $7, $8)",
        )
        .bind(now)
        .bind(self.worker_id.as_str())
        .bind(&job.submission_id)
        .bind(outcome)
        .bind(
            i64::try_from(new_version)
                .map_err(|_| WorkerError::InvalidState("scan resource version is invalid"))?,
        )
        .bind(&request_id)
        .bind(&report_sha256)
        .bind(&key_sha256)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
             aggregate_version, request_sha256, payload, created_unix_seconds) \
             VALUES ($1, 'submission.scan-completed', 'submission', $2, $3, $4, $5, $6)",
        )
        .bind(prefixed_uuid("evt_"))
        .bind(&job.submission_id)
        .bind(
            i64::try_from(new_version)
                .map_err(|_| WorkerError::InvalidState("scan resource version is invalid"))?,
        )
        .bind(&report_sha256)
        .bind(json!({
            "submission_id": job.submission_id,
            "scan_id": scan_id,
            "outcome": outcome,
            "report_sha256": report_sha256
        }))
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn defer_job(&self, job: &ScanJob, code: &str) -> Result<bool, WorkerError> {
        if !valid_error_code(code) {
            return Err(WorkerError::InvalidState("scan error code is invalid"));
        }
        let failed = job.attempts >= MAX_ATTEMPTS;
        let state = if failed { "failed" } else { "queued" };
        let now = if failed {
            Some(current_database_time(&self.pool).await?)
        } else {
            None
        };
        let updated = sqlx::query(
            "UPDATE submission_scan_jobs SET state = $1, lease_token = NULL, \
             leased_until_unix_seconds = NULL, last_error_code = $2, completed_unix_seconds = $3 \
             WHERE event_id = $4 AND state = 'running' AND lease_token = $5",
        )
        .bind(state)
        .bind(code)
        .bind(now)
        .bind(&job.event_id)
        .bind(&job.lease_token)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(WorkerError::InvalidState("scan lease was lost"));
        }
        Ok(failed)
    }
}

#[derive(Clone)]
struct ObjectReader {
    chunks: Arc<PathBuf>,
}

impl ObjectReader {
    async fn open(root: &Path) -> Result<Self, WorkerError> {
        if !root.is_absolute() {
            return Err(WorkerError::Configuration("object root must be absolute"));
        }
        let root = checked_directory(root).await?;
        let chunks = checked_directory(&root.join("chunks")).await?;
        if !chunks.starts_with(&root) {
            return Err(WorkerError::Configuration(
                "object directory escapes its root",
            ));
        }
        Ok(Self {
            chunks: Arc::new(chunks),
        })
    }

    async fn read_chunk(
        &self,
        sha256: &str,
        expected_bytes: usize,
    ) -> Result<Vec<u8>, WorkerError> {
        if !valid_sha256(sha256) || expected_bytes == 0 || expected_bytes > MAX_CHUNK_BYTES {
            return Err(WorkerError::InvalidState("chunk request is invalid"));
        }
        let path = self
            .chunks
            .join(&sha256[..2])
            .join(format!("{sha256}.chunk"));
        let expected_sha256 = sha256.to_owned();
        tokio::task::spawn_blocking(move || {
            let mut file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(path)?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() != expected_bytes as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "object metadata is invalid",
                ));
            }
            let mut encoded = Vec::with_capacity(expected_bytes);
            file.by_ref()
                .take(expected_bytes as u64 + 1)
                .read_to_end(&mut encoded)?;
            if encoded.len() != expected_bytes || sha256_hex(&encoded) != expected_sha256 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "object digest is invalid",
                ));
            }
            Ok(encoded)
        })
        .await
        .map_err(|_| WorkerError::InvalidState("object reader task failed"))?
        .map_err(WorkerError::Io)
    }
}

struct ScanJob {
    event_id: String,
    submission_id: String,
    source_resource_version: u64,
    source_content_sha256: String,
    lease_token: String,
    attempts: i16,
}

struct ScanBundle {
    app_id: String,
    version: String,
    default_locale: String,
    package: Vec<u8>,
    listing: Vec<u8>,
    assets: Vec<OwnedAsset>,
    trusted_developer_keys: Vec<[u8; 32]>,
}

struct OwnedAsset {
    descriptor: ImageAsset,
    encoded: Vec<u8>,
}

pub async fn connect(database_url: &str, max_connections: u32) -> Result<PgPool, WorkerError> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await
        .map_err(WorkerError::Database)
}

/// Applies the complete Store schema, including reviewer isolation and Release control.
pub async fn migrate(pool: &PgPool) -> Result<(), WorkerError> {
    sqlx::migrate!("../cp0-store-control-server/migrations")
        .run(pool)
        .await
        .map_err(WorkerError::Migration)
}

async fn begin_serializable(pool: &PgPool) -> Result<Transaction<'_, Postgres>, WorkerError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await?;
    Ok(transaction)
}

async fn database_now(transaction: &mut Transaction<'_, Postgres>) -> Result<i64, WorkerError> {
    sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(&mut **transaction)
        .await
        .map_err(WorkerError::Database)
}

async fn current_database_time(pool: &PgPool) -> Result<i64, WorkerError> {
    sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(pool)
        .await
        .map_err(WorkerError::Database)
}

async fn checked_directory(path: &Path) -> Result<PathBuf, WorkerError> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(WorkerError::Configuration(
            "object path is not a real directory",
        ));
    }
    tokio::fs::canonicalize(path).await.map_err(WorkerError::Io)
}

fn positive_size(value: i64) -> Result<usize, WorkerError> {
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(WorkerError::InvalidState("object size is invalid"))
}

fn valid_worker_id(value: &str) -> bool {
    (3..=64).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn valid_error_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn prefixed_uuid(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().simple())
}

fn sha256_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
