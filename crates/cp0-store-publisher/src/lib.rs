use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cp0_manifest::AppManifest;
use cp0_package::CApp;
use cp0_store_metadata::{ImageAsset, StoreListing};
use cp0_store_protocol::{
    CATALOG_SCHEMA_VERSION, Catalog, CatalogApp, CatalogDiscovery, MAX_CATALOG_APPS,
    MAX_CATALOG_LIFETIME_SECONDS, RICH_CATALOG_SCHEMA_VERSION, encode_signed_catalog,
    is_valid_https_url, lower_hex, sign_catalog,
};
use cp0_store_transparency::{
    Checkpoint, TRANSPARENCY_SCHEMA_VERSION, TransparencyLeaf, decode_checkpoint, decode_leaf,
    encode_checkpoint, encode_leaf, leaf_hash, lower_hex as transparency_hex,
    merkle_root_from_hashes, sign_checkpoint, verify_log,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

const LEASE_SECONDS: i64 = 300;
const MAX_ATTEMPTS: i16 = 8;
const MAX_TRANSACTION_RETRIES: usize = 3;
const MAX_CHUNK_BYTES: usize = 256 * 1024;
const DEFAULT_CATALOG_LIFETIME_SECONDS: u64 = 7 * 24 * 60 * 60;

#[derive(Debug)]
pub enum PublisherError {
    Configuration(&'static str),
    Database(sqlx::Error),
    Migration(sqlx::migrate::MigrateError),
    Io(io::Error),
    InvalidState(&'static str),
    Package(cp0_package::PackageError),
    Manifest(cp0_manifest::ManifestError),
    Metadata(cp0_store_metadata::ListingError),
    Protocol(cp0_store_protocol::StoreProtocolError),
    Transparency(cp0_store_transparency::TransparencyError),
}

impl fmt::Display for PublisherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => {
                write!(
                    formatter,
                    "invalid Store Publisher configuration: {message}"
                )
            }
            Self::Database(_) => formatter.write_str("Store Publisher database operation failed"),
            Self::Migration(_) => formatter.write_str("Store Publisher database migration failed"),
            Self::Io(_) => formatter.write_str("Store Publisher filesystem operation failed"),
            Self::InvalidState(message) => {
                write!(formatter, "invalid Store publication state: {message}")
            }
            Self::Package(_) => formatter.write_str("Store package validation failed"),
            Self::Manifest(_) => formatter.write_str("Store manifest validation failed"),
            Self::Metadata(_) => formatter.write_str("Store Listing validation failed"),
            Self::Protocol(_) => formatter.write_str("Store Catalog validation failed"),
            Self::Transparency(_) => formatter.write_str("Store transparency validation failed"),
        }
    }
}

impl std::error::Error for PublisherError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Migration(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Package(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::Metadata(error) => Some(error),
            Self::Protocol(error) => Some(error),
            Self::Transparency(error) => Some(error),
            Self::Configuration(_) | Self::InvalidState(_) => None,
        }
    }
}

impl From<sqlx::Error> for PublisherError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<io::Error> for PublisherError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<cp0_package::PackageError> for PublisherError {
    fn from(error: cp0_package::PackageError) -> Self {
        Self::Package(error)
    }
}

impl From<cp0_manifest::ManifestError> for PublisherError {
    fn from(error: cp0_manifest::ManifestError) -> Self {
        Self::Manifest(error)
    }
}

impl From<cp0_store_metadata::ListingError> for PublisherError {
    fn from(error: cp0_store_metadata::ListingError) -> Self {
        Self::Metadata(error)
    }
}

impl From<cp0_store_protocol::StoreProtocolError> for PublisherError {
    fn from(error: cp0_store_protocol::StoreProtocolError) -> Self {
        Self::Protocol(error)
    }
}

impl From<cp0_store_transparency::TransparencyError> for PublisherError {
    fn from(error: cp0_store_transparency::TransparencyError) -> Self {
        Self::Transparency(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Idle,
    Published {
        event_id: String,
        release_id: String,
        catalog_sequence: u64,
        app_count: usize,
    },
    Superseded {
        event_id: String,
        release_id: String,
        catalog_sequence: u64,
    },
    Deferred {
        event_id: String,
        release_id: String,
        failed_permanently: bool,
    },
}

#[derive(Clone)]
pub struct StorePublisher {
    pool: PgPool,
    objects: ObjectReader,
    origin: PublicationRoot,
    signer: Arc<StoreSigner>,
    worker_id: Arc<String>,
    base_url: Arc<String>,
    catalog_lifetime_seconds: u64,
}

impl StorePublisher {
    pub async fn open(
        pool: PgPool,
        object_root: impl AsRef<Path>,
        origin_root: impl AsRef<Path>,
        signing_key_path: impl AsRef<Path>,
        base_url: impl Into<String>,
        worker_id: impl Into<String>,
    ) -> Result<Self, PublisherError> {
        Self::open_with_lifetime(
            pool,
            object_root,
            origin_root,
            signing_key_path,
            base_url,
            worker_id,
            DEFAULT_CATALOG_LIFETIME_SECONDS,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn open_with_lifetime(
        pool: PgPool,
        object_root: impl AsRef<Path>,
        origin_root: impl AsRef<Path>,
        signing_key_path: impl AsRef<Path>,
        base_url: impl Into<String>,
        worker_id: impl Into<String>,
        catalog_lifetime_seconds: u64,
    ) -> Result<Self, PublisherError> {
        let worker_id = worker_id.into();
        if !valid_worker_id(&worker_id) {
            return Err(PublisherError::Configuration("worker ID is invalid"));
        }
        if !(1..=MAX_CATALOG_LIFETIME_SECONDS).contains(&catalog_lifetime_seconds) {
            return Err(PublisherError::Configuration(
                "Catalog lifetime is outside protocol limits",
            ));
        }
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        if !is_valid_https_url(&format!("{base_url}/catalog.json")) {
            return Err(PublisherError::Configuration(
                "public Store base URL is invalid",
            ));
        }
        let publisher = Self {
            pool,
            objects: ObjectReader::open(object_root.as_ref()).await?,
            origin: PublicationRoot::open(origin_root.as_ref()).await?,
            signer: Arc::new(StoreSigner::open(signing_key_path.as_ref())?),
            worker_id: Arc::new(worker_id),
            base_url: Arc::new(base_url),
            catalog_lifetime_seconds,
        };
        publisher.verify_transparency_log().await?;
        publisher.reconcile_current().await?;
        Ok(publisher)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn store_public_key(&self) -> [u8; 32] {
        self.signer.public_key
    }

    pub async fn run_once(&self) -> Result<RunOutcome, PublisherError> {
        let mut retries = 0;
        loop {
            match self.run_once_inner().await {
                Err(error)
                    if retries < MAX_TRANSACTION_RETRIES
                        && is_retryable_transaction_error(&error) =>
                {
                    retries += 1;
                    tokio::task::yield_now().await;
                }
                outcome => return outcome,
            }
        }
    }

    async fn run_once_inner(&self) -> Result<RunOutcome, PublisherError> {
        self.reconcile_current().await?;
        self.enqueue_one().await?;
        if let Some(expired) = self.recover_expired_leases().await? {
            self.finish_failed(&expired, "lease-exhausted").await?;
            return Ok(RunOutcome::Deferred {
                event_id: expired.event_id,
                release_id: expired.release_id,
                failed_permanently: true,
            });
        }
        let Some(job) = self.claim_one().await? else {
            return Ok(RunOutcome::Idle);
        };
        let prepared = match self.prepare_publication(&job).await {
            Ok(Preparation::Ready(prepared)) => prepared,
            Ok(Preparation::Superseded) => {
                self.finish_superseded(&job).await?;
                return Ok(RunOutcome::Superseded {
                    event_id: job.event_id,
                    release_id: job.release_id,
                    catalog_sequence: job.catalog_sequence,
                });
            }
            Err(error) => {
                let (code, permanent) = classify_preparation_error(&error);
                if permanent || job.attempts >= MAX_ATTEMPTS {
                    self.finish_failed(&job, code).await?;
                } else {
                    self.defer_job(&job, code).await?;
                }
                return Ok(RunOutcome::Deferred {
                    event_id: job.event_id,
                    release_id: job.release_id,
                    failed_permanently: permanent || job.attempts >= MAX_ATTEMPTS,
                });
            }
        };
        if let Err(error) = self.origin.write_generation(&prepared).await {
            eprintln!("Store Publisher deferred an origin write: {error:?}");
            let permanent = job.attempts >= MAX_ATTEMPTS;
            if permanent {
                self.finish_failed(&job, "origin-unavailable").await?;
            } else {
                self.defer_job(&job, "origin-unavailable").await?;
            }
            return Ok(RunOutcome::Deferred {
                event_id: job.event_id,
                release_id: job.release_id,
                failed_permanently: permanent,
            });
        }
        match self.commit_publication(&job, &prepared).await? {
            CommitOutcome::Superseded => {
                return Ok(RunOutcome::Superseded {
                    event_id: job.event_id,
                    release_id: job.release_id,
                    catalog_sequence: job.catalog_sequence,
                });
            }
            CommitOutcome::Published => {}
        }
        self.origin
            .verify_committed_generation(
                job.catalog_sequence,
                &prepared.catalog_encoded,
                &prepared.transparency.leaf_encoded,
                &prepared.transparency.checkpoint_encoded,
                &prepared.store_public_key,
            )
            .await?;
        self.origin
            .switch_current(job.catalog_sequence, &prepared.catalog_encoded)
            .await?;
        Ok(RunOutcome::Published {
            event_id: job.event_id,
            release_id: job.release_id,
            catalog_sequence: job.catalog_sequence,
            app_count: prepared.app_count,
        })
    }

    pub async fn reconcile_current(&self) -> Result<(), PublisherError> {
        let snapshot = sqlx::query(
            "SELECT snapshot.sequence, snapshot.encoded_catalog, leaf.encoded_leaf, \
             checkpoint.encoded_checkpoint FROM store_catalog_snapshots snapshot \
             JOIN store_transparency_leaves leaf ON leaf.catalog_sequence = snapshot.sequence \
             JOIN store_transparency_checkpoints checkpoint \
               ON checkpoint.catalog_sequence = snapshot.sequence \
             ORDER BY snapshot.sequence DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(snapshot) = snapshot else {
            return self.origin.require_no_current().await;
        };
        let sequence = positive_u64(snapshot.get::<i64, _>("sequence"))?;
        let encoded: Vec<u8> = snapshot.get("encoded_catalog");
        let leaf: Vec<u8> = snapshot.get("encoded_leaf");
        let checkpoint: Vec<u8> = snapshot.get("encoded_checkpoint");
        self.origin
            .verify_committed_generation(
                sequence,
                &encoded,
                &leaf,
                &checkpoint,
                &self.signer.public_key,
            )
            .await?;
        self.origin.switch_current(sequence, &encoded).await
    }

    async fn verify_transparency_log(&self) -> Result<(), PublisherError> {
        let snapshot_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM store_catalog_snapshots")
                .fetch_one(&self.pool)
                .await?;
        let leaf_rows = sqlx::query(
            "SELECT leaf.tree_index, leaf.catalog_sequence, leaf.leaf_sha256, leaf.encoded_leaf, \
             snapshot.catalog_sha256, snapshot.catalog_bytes, snapshot.store_key_id, \
             snapshot.published_unix_seconds, snapshot.source_event_id, \
             snapshot.source_release_id, job.job_kind, job.source_state \
             FROM store_transparency_leaves leaf \
             JOIN store_catalog_snapshots snapshot ON snapshot.sequence = leaf.catalog_sequence \
             JOIN store_publication_jobs job ON job.event_id = snapshot.source_event_id \
             ORDER BY leaf.tree_index",
        )
        .fetch_all(&self.pool)
        .await?;
        let checkpoint_rows = sqlx::query(
            "SELECT tree_size, catalog_sequence, root_sha256, store_key_id, encoded_checkpoint \
             FROM store_transparency_checkpoints ORDER BY tree_size",
        )
        .fetch_all(&self.pool)
        .await?;
        if snapshot_count != leaf_rows.len() as i64 || checkpoint_rows.len() != leaf_rows.len() {
            return Err(PublisherError::InvalidState(
                "Catalog snapshots and transparency log are not one-to-one",
            ));
        }
        if leaf_rows.is_empty() {
            return Ok(());
        }
        let mut leaves = Vec::with_capacity(leaf_rows.len());
        for (index, row) in leaf_rows.into_iter().enumerate() {
            if row.get::<i64, _>("tree_index") != index as i64 {
                return Err(PublisherError::InvalidState(
                    "transparency log indices are not contiguous",
                ));
            }
            let leaf = decode_leaf(&row.get::<Vec<u8>, _>("encoded_leaf"))?;
            let expected_catalog_bytes = u32::try_from(row.get::<i32, _>("catalog_bytes"))
                .map_err(|_| PublisherError::InvalidState("stored Catalog size is invalid"))?;
            if leaf.tree_index != index as u64
                || leaf.catalog_sequence != positive_u64(row.get::<i64, _>("catalog_sequence"))?
                || leaf.catalog_sha256 != row.get::<String, _>("catalog_sha256")
                || leaf.catalog_bytes != expected_catalog_bytes
                || leaf.store_key_id != row.get::<String, _>("store_key_id")
                || leaf.published_unix_seconds
                    != positive_u64(row.get::<i64, _>("published_unix_seconds"))?
                || leaf.source_event_id != row.get::<String, _>("source_event_id")
                || leaf.source_release_id != row.get::<String, _>("source_release_id")
                || leaf.job_kind != row.get::<String, _>("job_kind")
                || leaf.release_state != row.get::<String, _>("source_state")
                || transparency_hex(&leaf_hash(&leaf)?) != row.get::<String, _>("leaf_sha256")
            {
                return Err(PublisherError::InvalidState(
                    "transparency leaf does not match its Catalog snapshot",
                ));
            }
            leaves.push(leaf);
        }
        for (index, row) in checkpoint_rows.into_iter().enumerate() {
            let expected_tree_size = index + 1;
            if row.get::<i64, _>("tree_size") != expected_tree_size as i64 {
                return Err(PublisherError::InvalidState(
                    "transparency checkpoint sizes are not contiguous",
                ));
            }
            let checkpoint = decode_checkpoint(&row.get::<Vec<u8>, _>("encoded_checkpoint"))?;
            let latest = &leaves[index];
            if checkpoint.checkpoint.tree_size != expected_tree_size as u64
                || checkpoint.checkpoint.latest_catalog_sequence != latest.catalog_sequence
                || checkpoint.checkpoint.issued_unix_seconds != latest.published_unix_seconds
                || checkpoint.checkpoint.root_sha256 != row.get::<String, _>("root_sha256")
                || checkpoint.key_id != row.get::<String, _>("store_key_id")
                || latest.catalog_sequence != positive_u64(row.get::<i64, _>("catalog_sequence"))?
            {
                return Err(PublisherError::InvalidState(
                    "transparency checkpoint does not match its database record",
                ));
            }
            verify_log(
                &checkpoint,
                &self.signer.public_key,
                &leaves[..expected_tree_size],
            )?;
        }
        Ok(())
    }

    async fn enqueue_one(&self) -> Result<bool, PublisherError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let row = sqlx::query(
            "SELECT event_id, topic, aggregate_id, aggregate_version, payload \
             FROM outbox_events WHERE topic IN ('release.publish-requested', 'catalog.rebuild-requested') \
             AND aggregate_kind = 'release' AND published_unix_seconds IS NULL \
             ORDER BY created_unix_seconds, event_id FOR UPDATE SKIP LOCKED LIMIT 1",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(false);
        };
        let event_id: String = row.get("event_id");
        let topic: String = row.get("topic");
        let release_id: String = row.get("aggregate_id");
        let source_resource_version: i64 = row.get("aggregate_version");
        let payload: Value = row.get("payload");
        if payload.get("release_id").and_then(Value::as_str) != Some(&release_id) {
            return Err(PublisherError::InvalidState(
                "publication event release identity is invalid",
            ));
        }
        let source_state =
            payload
                .get("state")
                .and_then(Value::as_str)
                .ok_or(PublisherError::InvalidState(
                    "publication event state is missing",
                ))?;
        let job_kind = match topic.as_str() {
            "release.publish-requested" if source_state == "publishing" => "publish-release",
            "catalog.rebuild-requested"
                if matches!(source_state, "published" | "paused" | "removed") =>
            {
                "rebuild-catalog"
            }
            _ => {
                return Err(PublisherError::InvalidState(
                    "publication event topic and state do not match",
                ));
            }
        };
        let now = database_now(&mut transaction).await?;
        sqlx::query(
            "INSERT INTO store_publication_jobs (event_id, release_id, job_kind, \
             source_resource_version, source_state, state, created_unix_seconds) \
             VALUES ($1, $2, $3, $4, $5, 'queued', $6) ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(&event_id)
        .bind(&release_id)
        .bind(job_kind)
        .bind(source_resource_version)
        .bind(source_state)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let changed = sqlx::query(
            "UPDATE outbox_events SET published_unix_seconds = $1, attempts = attempts + 1 \
             WHERE event_id = $2 AND published_unix_seconds IS NULL",
        )
        .bind(now)
        .bind(&event_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(PublisherError::InvalidState(
                "publication event dispatch was lost",
            ));
        }
        transaction.commit().await?;
        Ok(true)
    }

    async fn recover_expired_leases(&self) -> Result<Option<PublicationJob>, PublisherError> {
        sqlx::query(
            "UPDATE store_publication_jobs SET state = 'queued', lease_token = NULL, \
             leased_until_unix_seconds = NULL, last_error_code = 'lease-expired' \
             WHERE state = 'running' AND attempts < 8 AND leased_until_unix_seconds <= \
             EXTRACT(EPOCH FROM clock_timestamp())::BIGINT",
        )
        .execute(&self.pool)
        .await?;
        let row = sqlx::query(
            "SELECT event_id, release_id, job_kind, source_resource_version, source_state, \
             lease_token, attempts, catalog_sequence, published_unix_seconds, expires_unix_seconds \
             FROM store_publication_jobs WHERE state = 'running' AND attempts >= 8 AND \
             leased_until_unix_seconds <= EXTRACT(EPOCH FROM clock_timestamp())::BIGINT \
             ORDER BY created_unix_seconds, event_id LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(publication_job_from_row).transpose()
    }

    async fn claim_one(&self) -> Result<Option<PublicationJob>, PublisherError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let last_sequence: i64 = sqlx::query_scalar(
            "SELECT last_sequence FROM catalog_sequence WHERE singleton = TRUE FOR UPDATE",
        )
        .fetch_one(&mut *transaction)
        .await?;
        let running: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM store_publication_jobs WHERE state = 'running')",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if running {
            transaction.commit().await?;
            return Ok(None);
        }
        let row = sqlx::query(
            "SELECT event_id, attempts, catalog_sequence, published_unix_seconds, expires_unix_seconds \
             FROM store_publication_jobs WHERE state = 'queued' AND attempts < 8 \
             ORDER BY created_unix_seconds, event_id FOR UPDATE SKIP LOCKED LIMIT 1",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let event_id: String = row.get("event_id");
        let attempts: i16 = row.get("attempts");
        let now = database_now(&mut transaction).await?;
        let (sequence, published, expires) = if attempts == 0 {
            let sequence = last_sequence
                .checked_add(1)
                .ok_or(PublisherError::InvalidState("Catalog sequence overflow"))?;
            sqlx::query("UPDATE catalog_sequence SET last_sequence = $1 WHERE singleton = TRUE")
                .bind(sequence)
                .execute(&mut *transaction)
                .await?;
            let expires = now
                .checked_add(i64::try_from(self.catalog_lifetime_seconds).map_err(|_| {
                    PublisherError::InvalidState("Catalog lifetime cannot be represented")
                })?)
                .ok_or(PublisherError::InvalidState("Catalog expiry overflow"))?;
            (sequence, now, expires)
        } else {
            (
                row.get::<Option<i64>, _>("catalog_sequence")
                    .ok_or(PublisherError::InvalidState("reserved sequence is missing"))?,
                row.get::<Option<i64>, _>("published_unix_seconds").ok_or(
                    PublisherError::InvalidState("reserved timestamp is missing"),
                )?,
                row.get::<Option<i64>, _>("expires_unix_seconds")
                    .ok_or(PublisherError::InvalidState("reserved expiry is missing"))?,
            )
        };
        let lease_token = prefixed_uuid("lease_");
        let claimed = sqlx::query(
            "UPDATE store_publication_jobs SET state = 'running', lease_token = $1, \
             leased_until_unix_seconds = $2, attempts = attempts + 1, catalog_sequence = $3, \
             published_unix_seconds = $4, expires_unix_seconds = $5, last_error_code = NULL \
             WHERE event_id = $6 AND state = 'queued' \
             RETURNING event_id, release_id, job_kind, source_resource_version, source_state, \
             lease_token, attempts, catalog_sequence, published_unix_seconds, expires_unix_seconds",
        )
        .bind(&lease_token)
        .bind(now + LEASE_SECONDS)
        .bind(sequence)
        .bind(published)
        .bind(expires)
        .bind(&event_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(PublisherError::InvalidState("publication claim was lost"))?;
        let job = publication_job_from_row(claimed)?;
        transaction.commit().await?;
        Ok(Some(job))
    }

    async fn prepare_publication(
        &self,
        job: &PublicationJob,
    ) -> Result<Preparation, PublisherError> {
        let release = self.load_release_source(job).await?;
        let Some(release) = release else {
            return Ok(Preparation::Superseded);
        };
        let target = if job.job_kind == "publish-release" {
            Some(self.prepare_package(job, &release).await?)
        } else {
            None
        };
        let mut projected = BTreeMap::<String, ProjectedApp>::new();
        let rows = sqlx::query(
            "SELECT release.state, artifact.catalog_sequence, artifact.catalog_app \
             FROM store_package_artifacts artifact \
             JOIN releases release ON release.release_id = artifact.release_id \
             ORDER BY artifact.catalog_sequence",
        )
        .fetch_all(&self.pool)
        .await?;
        for row in rows {
            let app_value: Value = row.get("catalog_app");
            let app: CatalogApp = serde_json::from_value(app_value)
                .map_err(|_| PublisherError::InvalidState("stored Catalog app is invalid"))?;
            app.validate()?;
            projected.insert(
                app.app_id.clone(),
                ProjectedApp {
                    sequence: positive_u64(row.get::<i64, _>("catalog_sequence"))?,
                    state: row.get("state"),
                    app,
                },
            );
        }
        if let Some(artifact) = &target {
            projected.insert(
                artifact.app.app_id.clone(),
                ProjectedApp {
                    sequence: job.catalog_sequence,
                    state: "published".to_owned(),
                    app: artifact.app.clone(),
                },
            );
        }
        let mut apps = projected
            .into_values()
            .filter(|projection| projection.state == "published")
            .map(|projection| {
                let _ = projection.sequence;
                projection.app
            })
            .collect::<Vec<_>>();
        if apps.len() > MAX_CATALOG_APPS {
            return Err(PublisherError::InvalidState(
                "Catalog application bound was exceeded",
            ));
        }
        let schema_version = catalog_schema_for_projection(&mut apps);
        let catalog = Catalog {
            schema_version,
            sequence: job.catalog_sequence,
            published_unix_seconds: job.published_unix_seconds,
            expires_unix_seconds: job.expires_unix_seconds,
            apps,
        };
        let signed = sign_catalog(catalog, &self.signer.secret)?;
        let catalog_encoded = encode_signed_catalog(&signed)?;
        let catalog_sha256 = sha256_hex(&catalog_encoded);
        let transparency = self
            .prepare_transparency(job, &catalog_sha256, catalog_encoded.len())
            .await?;
        Ok(Preparation::Ready(Box::new(PreparedPublication {
            catalog_sha256,
            catalog_encoded,
            catalog_relative_path: catalog_relative_path(job.catalog_sequence),
            app_count: signed.catalog.apps.len(),
            package: target,
            store_public_key: self.signer.public_key,
            store_key_id: self.signer.key_id.clone(),
            transparency,
        })))
    }

    async fn prepare_transparency(
        &self,
        job: &PublicationJob,
        catalog_sha256: &str,
        catalog_bytes: usize,
    ) -> Result<PreparedTransparency, PublisherError> {
        let rows = sqlx::query(
            "SELECT tree_index, leaf_sha256, encoded_leaf \
             FROM store_transparency_leaves ORDER BY tree_index",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut hashes = Vec::with_capacity(rows.len() + 1);
        for (index, row) in rows.into_iter().enumerate() {
            if row.get::<i64, _>("tree_index") != index as i64 {
                return Err(PublisherError::InvalidState(
                    "transparency log indices are not contiguous",
                ));
            }
            let leaf = decode_leaf(&row.get::<Vec<u8>, _>("encoded_leaf"))?;
            let hash = leaf_hash(&leaf)?;
            if transparency_hex(&hash) != row.get::<String, _>("leaf_sha256") {
                return Err(PublisherError::InvalidState(
                    "stored transparency leaf digest does not match",
                ));
            }
            hashes.push(hash);
        }
        let leaf = TransparencyLeaf {
            schema_version: TRANSPARENCY_SCHEMA_VERSION,
            tree_index: hashes.len() as u64,
            catalog_sequence: job.catalog_sequence,
            catalog_sha256: catalog_sha256.to_owned(),
            catalog_bytes: u32::try_from(catalog_bytes).map_err(|_| {
                PublisherError::InvalidState("Catalog size cannot be represented in transparency")
            })?,
            store_key_id: self.signer.key_id.clone(),
            published_unix_seconds: job.published_unix_seconds,
            source_event_id: job.event_id.clone(),
            source_release_id: job.release_id.clone(),
            job_kind: job.job_kind.clone(),
            release_state: job.source_state.clone(),
        };
        let leaf_encoded = encode_leaf(&leaf)?;
        let leaf_hash = leaf_hash(&leaf)?;
        hashes.push(leaf_hash);
        let root = merkle_root_from_hashes(&hashes);
        let checkpoint = sign_checkpoint(
            Checkpoint {
                schema_version: TRANSPARENCY_SCHEMA_VERSION,
                tree_size: hashes.len() as u64,
                root_sha256: transparency_hex(&root),
                latest_catalog_sequence: job.catalog_sequence,
                issued_unix_seconds: job.published_unix_seconds,
            },
            &self.signer.secret,
        )?;
        let checkpoint_encoded = encode_checkpoint(&checkpoint)?;
        Ok(PreparedTransparency {
            tree_index: leaf.tree_index,
            leaf_sha256: transparency_hex(&leaf_hash),
            leaf_encoded,
            tree_size: checkpoint.checkpoint.tree_size,
            root_sha256: checkpoint.checkpoint.root_sha256,
            checkpoint_encoded,
        })
    }

    async fn load_release_source(
        &self,
        job: &PublicationJob,
    ) -> Result<Option<ReleaseSource>, PublisherError> {
        let row = sqlx::query(
            "SELECT release.submission_id, release.app_id, release.version, release.state, \
             release.resource_version, submission.state AS submission_state, \
             submission.package_sha256, submission.package_bytes, submission.listing_sha256, \
             submission.listing_bytes, submission.assets, submission.finalized_content_sha256, \
             app.owner_team_id, app.default_locale, team.name AS developer_name, \
             EXISTS (SELECT 1 FROM review_decisions decision \
                     WHERE decision.submission_id = submission.submission_id \
                       AND decision.decision = 'approved') AS approved_decision \
             FROM releases release \
             JOIN submissions submission ON submission.submission_id = release.submission_id \
             JOIN apps app ON app.app_id = release.app_id \
             JOIN teams team ON team.team_id = app.owner_team_id \
             WHERE release.release_id = $1",
        )
        .bind(&job.release_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PublisherError::InvalidState(
            "publication Release is missing",
        ))?;
        let state: String = row.get("state");
        let resource_version = positive_u64(row.get::<i64, _>("resource_version"))?;
        if state != job.source_state || resource_version != job.source_resource_version {
            return Ok(None);
        }
        if job.job_kind == "publish-release"
            && (row.get::<String, _>("submission_state") != "approved"
                || !row.get::<bool, _>("approved_decision"))
        {
            return Err(PublisherError::InvalidState(
                "publication source is not approved",
            ));
        }
        Ok(Some(ReleaseSource {
            submission_id: row.get("submission_id"),
            app_id: row.get("app_id"),
            version: row.get("version"),
            package_sha256: row.get("package_sha256"),
            package_bytes: positive_size(row.get::<i64, _>("package_bytes"))?,
            listing_sha256: row.get("listing_sha256"),
            listing_bytes: positive_size(row.get::<i64, _>("listing_bytes"))?,
            assets: serde_json::from_value(row.get("assets"))
                .map_err(|_| PublisherError::InvalidState("submission assets are invalid"))?,
            finalized_content_sha256: row
                .get::<Option<String>, _>("finalized_content_sha256")
                .ok_or(PublisherError::InvalidState(
                    "submission content digest is missing",
                ))?,
            owner_team_id: row.get("owner_team_id"),
            default_locale: row.get("default_locale"),
            developer_name: row.get("developer_name"),
        }))
    }

    async fn prepare_package(
        &self,
        job: &PublicationJob,
        release: &ReleaseSource,
    ) -> Result<PreparedPackage, PublisherError> {
        if release.assets.len() < 2 || release.assets.len() > 6 {
            return Err(PublisherError::InvalidState(
                "submission asset count is invalid",
            ));
        }
        if cp0_store_scan::submission_content_sha256(
            &release.package_sha256,
            &release.listing_sha256,
            &release.assets,
        ) != release.finalized_content_sha256
        {
            return Err(PublisherError::InvalidState(
                "submission content digest does not match",
            ));
        }
        let package_encoded = self
            .load_part(
                &release.submission_id,
                "package",
                &release.package_sha256,
                release.package_bytes,
            )
            .await?;
        let listing_encoded = self
            .load_part(
                &release.submission_id,
                "listing",
                &release.listing_sha256,
                release.listing_bytes,
            )
            .await?;
        for (index, descriptor) in release.assets.iter().enumerate() {
            self.load_part(
                &release.submission_id,
                &format!("asset-{index}"),
                &descriptor.sha256,
                usize::try_from(descriptor.bytes)
                    .map_err(|_| PublisherError::InvalidState("asset size is invalid"))?,
            )
            .await?;
        }

        let mut package = CApp::decode(&package_encoded)?;
        package.verify_developer_signature()?;
        if package.store_key_id().is_some() {
            return Err(PublisherError::InvalidState(
                "developer package already has a Store signature",
            ));
        }
        let developer_key = package
            .developer_public_key()
            .ok_or(PublisherError::InvalidState(
                "developer signature key is missing",
            ))?;
        let key_active: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM developer_keys WHERE team_id = $1 AND state = 'active' \
             AND public_key = $2 AND fingerprint_sha256 = $3)",
        )
        .bind(&release.owner_team_id)
        .bind(developer_key.to_vec())
        .bind(sha256_hex(&developer_key))
        .fetch_one(&self.pool)
        .await?;
        if !key_active {
            return Err(PublisherError::InvalidState(
                "developer signing key is no longer active",
            ));
        }
        let manifest_encoded = package
            .entry("app.json")
            .ok_or(PublisherError::InvalidState("package manifest is missing"))?;
        let manifest = cp0_manifest::parse_and_validate(manifest_encoded)?;
        let listing = cp0_store_metadata::parse_and_validate(&listing_encoded)?;
        validate_release_metadata(release, &manifest, &listing)?;
        package.sign_store(&self.signer.secret)?;
        let encoded = package.encode()?;
        let package_sha256 = sha256_hex(&encoded);
        let relative_path = package_relative_path(job.catalog_sequence, &job.release_id);
        let package_url = format!("{}/{relative_path}", self.base_url);
        if !is_valid_https_url(&package_url) {
            return Err(PublisherError::InvalidState(
                "generated package URL is invalid",
            ));
        }
        let localized = listing
            .localizations
            .iter()
            .find(|localized| localized.locale == listing.default_locale)
            .ok_or(PublisherError::InvalidState(
                "default Listing localization is missing",
            ))?;
        let mut permissions = manifest
            .permissions
            .iter()
            .map(|request| request.name)
            .collect::<Vec<_>>();
        permissions.sort_by_key(|permission| permission.as_str());
        let app = CatalogApp {
            app_id: release.app_id.clone(),
            name: manifest.name,
            version: release.version.clone(),
            sdk_version: manifest.sdk_version,
            summary: localized.subtitle.clone(),
            package_url,
            package_sha256: package_sha256.clone(),
            package_bytes: encoded.len() as u64,
            permissions,
            discovery: Some(CatalogDiscovery {
                developer: release.developer_name.clone(),
                subtitle: localized.subtitle.clone(),
                category: listing.category,
                keywords: localized.keywords.clone(),
                age_rating: listing.age_rating,
                privacy_url: listing.privacy_url.clone(),
                support_url: listing.support_url.clone(),
            }),
        };
        app.validate()?;
        Ok(PreparedPackage {
            release_id: job.release_id.clone(),
            submission_id: release.submission_id.clone(),
            relative_path,
            package_sha256,
            encoded,
            app,
        })
    }

    async fn load_part(
        &self,
        submission_id: &str,
        part_name: &str,
        expected_sha256: &str,
        expected_bytes: usize,
    ) -> Result<Vec<u8>, PublisherError> {
        let row = sqlx::query(
            "SELECT expected_sha256, expected_bytes, received_bytes \
             FROM submission_upload_parts WHERE submission_id = $1 AND part_name = $2",
        )
        .bind(submission_id)
        .bind(part_name)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PublisherError::InvalidState("submission part is missing"))?;
        if row.get::<String, _>("expected_sha256") != expected_sha256
            || positive_size(row.get::<i64, _>("expected_bytes"))? != expected_bytes
            || positive_size(row.get::<i64, _>("received_bytes"))? != expected_bytes
        {
            return Err(PublisherError::InvalidState(
                "submission part descriptor does not match",
            ));
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
                .map_err(|_| PublisherError::InvalidState("chunk offset is invalid"))?;
            let bytes = usize::try_from(row.get::<i32, _>("chunk_bytes"))
                .map_err(|_| PublisherError::InvalidState("chunk size is invalid"))?;
            let sha256: String = row.get("chunk_sha256");
            if offset != encoded.len() || bytes == 0 || bytes > MAX_CHUNK_BYTES {
                return Err(PublisherError::InvalidState("chunk descriptor is invalid"));
            }
            encoded.extend_from_slice(&self.objects.read_chunk(&sha256, bytes).await?);
            if encoded.len() > expected_bytes {
                return Err(PublisherError::InvalidState(
                    "submission part exceeds its bound",
                ));
            }
        }
        if encoded.len() != expected_bytes || sha256_hex(&encoded) != expected_sha256 {
            return Err(PublisherError::InvalidState(
                "submission part digest does not match",
            ));
        }
        Ok(encoded)
    }

    async fn commit_publication(
        &self,
        job: &PublicationJob,
        prepared: &PreparedPublication,
    ) -> Result<CommitOutcome, PublisherError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        lock_active_job(&mut transaction, job).await?;
        let release_row = sqlx::query(
            "SELECT state, resource_version FROM releases WHERE release_id = $1 FOR UPDATE",
        )
        .bind(&job.release_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(PublisherError::InvalidState(
            "publication Release is missing",
        ))?;
        if release_row.get::<String, _>("state") != job.source_state
            || positive_u64(release_row.get::<i64, _>("resource_version"))?
                != job.source_resource_version
        {
            mark_job_superseded(&mut transaction, job, "source-superseded").await?;
            append_catalog_event(
                &mut transaction,
                &self.worker_id,
                job,
                "catalog.publication-superseded",
                "catalog.publication-superseded",
                &prepared.catalog_sha256,
                json!({
                    "release_id": job.release_id,
                    "catalog_sequence": job.catalog_sequence
                }),
            )
            .await?;
            transaction.commit().await?;
            return Ok(CommitOutcome::Superseded);
        }
        let now = database_now(&mut transaction).await?;
        if let Some(package) = &prepared.package {
            sqlx::query(
                "INSERT INTO store_package_artifacts (release_id, submission_id, catalog_sequence, \
                 package_sha256, package_bytes, relative_path, store_key_id, catalog_app, \
                 created_unix_seconds) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(&package.release_id)
            .bind(&package.submission_id)
            .bind(i64_from_u64(job.catalog_sequence)?)
            .bind(&package.package_sha256)
            .bind(i64::try_from(package.encoded.len()).map_err(|_| {
                PublisherError::InvalidState("signed package size cannot be represented")
            })?)
            .bind(&package.relative_path)
            .bind(&prepared.store_key_id)
            .bind(
                serde_json::to_value(&package.app)
                    .map_err(|_| PublisherError::InvalidState("Catalog app cannot be encoded"))?,
            )
            .bind(now)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            "INSERT INTO store_catalog_snapshots (sequence, source_event_id, source_release_id, \
             catalog_sha256, catalog_bytes, relative_path, store_key_id, app_count, \
             published_unix_seconds, expires_unix_seconds, encoded_catalog, created_unix_seconds) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(i64_from_u64(job.catalog_sequence)?)
        .bind(&job.event_id)
        .bind(&job.release_id)
        .bind(&prepared.catalog_sha256)
        .bind(
            i32::try_from(prepared.catalog_encoded.len())
                .map_err(|_| PublisherError::InvalidState("Catalog size cannot be represented"))?,
        )
        .bind(&prepared.catalog_relative_path)
        .bind(&prepared.store_key_id)
        .bind(
            i16::try_from(prepared.app_count).map_err(|_| {
                PublisherError::InvalidState("Catalog app count cannot be represented")
            })?,
        )
        .bind(i64_from_u64(job.published_unix_seconds)?)
        .bind(i64_from_u64(job.expires_unix_seconds)?)
        .bind(&prepared.catalog_encoded)
        .bind(now)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "INSERT INTO store_transparency_leaves (tree_index, catalog_sequence, leaf_sha256, \
             encoded_leaf, created_unix_seconds) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(i64_from_u64(prepared.transparency.tree_index)?)
        .bind(i64_from_u64(job.catalog_sequence)?)
        .bind(&prepared.transparency.leaf_sha256)
        .bind(&prepared.transparency.leaf_encoded)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO store_transparency_checkpoints (tree_size, catalog_sequence, \
             root_sha256, store_key_id, encoded_checkpoint, created_unix_seconds) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(i64_from_u64(prepared.transparency.tree_size)?)
        .bind(i64_from_u64(job.catalog_sequence)?)
        .bind(&prepared.transparency.root_sha256)
        .bind(&prepared.store_key_id)
        .bind(&prepared.transparency.checkpoint_encoded)
        .bind(now)
        .execute(&mut *transaction)
        .await?;

        if job.job_kind == "publish-release" {
            let new_version =
                job.source_resource_version
                    .checked_add(1)
                    .ok_or(PublisherError::InvalidState(
                        "Release resource version overflow",
                    ))?;
            let changed = sqlx::query(
                "UPDATE releases SET state = 'published', catalog_sequence = $1, \
                 resource_version = $2 WHERE release_id = $3 AND state = 'publishing' \
                 AND resource_version = $4",
            )
            .bind(i64_from_u64(job.catalog_sequence)?)
            .bind(i64_from_u64(new_version)?)
            .bind(&job.release_id)
            .bind(i64_from_u64(job.source_resource_version)?)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if changed != 1 {
                return Err(PublisherError::InvalidState(
                    "publication Release update was lost",
                ));
            }
            append_release_published(
                &mut transaction,
                &self.worker_id,
                job,
                new_version,
                &prepared.catalog_sha256,
                now,
            )
            .await?;
        }
        let completed = sqlx::query(
            "UPDATE store_publication_jobs SET state = 'completed', lease_token = NULL, \
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
            return Err(PublisherError::InvalidState(
                "publication completion lease was lost",
            ));
        }
        append_catalog_event(
            &mut transaction,
            &self.worker_id,
            job,
            "catalog.published",
            "catalog.published",
            &prepared.catalog_sha256,
            json!({
                "release_id": job.release_id,
                "catalog_sequence": job.catalog_sequence,
                "catalog_sha256": prepared.catalog_sha256,
                "app_count": prepared.app_count
            }),
        )
        .await?;
        transaction.commit().await?;
        Ok(CommitOutcome::Published)
    }

    async fn defer_job(&self, job: &PublicationJob, code: &str) -> Result<(), PublisherError> {
        if !valid_error_code(code) {
            return Err(PublisherError::InvalidState(
                "publication error code is invalid",
            ));
        }
        let changed = sqlx::query(
            "UPDATE store_publication_jobs SET state = 'queued', lease_token = NULL, \
             leased_until_unix_seconds = NULL, last_error_code = $1 \
             WHERE event_id = $2 AND state = 'running' AND lease_token = $3",
        )
        .bind(code)
        .bind(&job.event_id)
        .bind(&job.lease_token)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(PublisherError::InvalidState(
                "publication defer lease was lost",
            ));
        }
        Ok(())
    }

    async fn finish_superseded(&self, job: &PublicationJob) -> Result<(), PublisherError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        lock_active_job(&mut transaction, job).await?;
        mark_job_superseded(&mut transaction, job, "source-superseded").await?;
        append_catalog_event(
            &mut transaction,
            &self.worker_id,
            job,
            "catalog.publication-superseded",
            "catalog.publication-superseded",
            &sha256_hex(job.event_id.as_bytes()),
            json!({
                "release_id": job.release_id,
                "catalog_sequence": job.catalog_sequence
            }),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn finish_failed(&self, job: &PublicationJob, code: &str) -> Result<(), PublisherError> {
        if !valid_error_code(code) {
            return Err(PublisherError::InvalidState(
                "publication error code is invalid",
            ));
        }
        let mut transaction = begin_serializable(&self.pool).await?;
        lock_active_job(&mut transaction, job).await?;
        let release = sqlx::query(
            "SELECT state, resource_version FROM releases WHERE release_id = $1 FOR UPDATE",
        )
        .bind(&job.release_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(PublisherError::InvalidState(
            "publication Release is missing",
        ))?;
        let release_matches = release.get::<String, _>("state") == job.source_state
            && positive_u64(release.get::<i64, _>("resource_version"))?
                == job.source_resource_version;
        if !release_matches {
            mark_job_superseded(&mut transaction, job, "source-superseded").await?;
            transaction.commit().await?;
            return Ok(());
        }
        let now = database_now(&mut transaction).await?;
        if job.job_kind == "publish-release" {
            let new_version =
                job.source_resource_version
                    .checked_add(1)
                    .ok_or(PublisherError::InvalidState(
                        "Release resource version overflow",
                    ))?;
            sqlx::query(
                "UPDATE releases SET state = 'publish-failed', resource_version = $1 \
                 WHERE release_id = $2 AND state = 'publishing' AND resource_version = $3",
            )
            .bind(i64_from_u64(new_version)?)
            .bind(&job.release_id)
            .bind(i64_from_u64(job.source_resource_version)?)
            .execute(&mut *transaction)
            .await?;
            append_release_failed(
                &mut transaction,
                &self.worker_id,
                job,
                new_version,
                code,
                now,
            )
            .await?;
        }
        let changed = sqlx::query(
            "UPDATE store_publication_jobs SET state = 'failed', lease_token = NULL, \
             leased_until_unix_seconds = NULL, last_error_code = $1, completed_unix_seconds = $2 \
             WHERE event_id = $3 AND state = 'running' AND lease_token = $4",
        )
        .bind(code)
        .bind(now)
        .bind(&job.event_id)
        .bind(&job.lease_token)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(PublisherError::InvalidState(
                "publication failure lease was lost",
            ));
        }
        let failure_sha256 = sha256_hex(format!("{}\0{code}", job.event_id).as_bytes());
        append_catalog_event(
            &mut transaction,
            &self.worker_id,
            job,
            "catalog.publish-failed",
            "catalog.publish-failed",
            &failure_sha256,
            json!({
                "release_id": job.release_id,
                "catalog_sequence": job.catalog_sequence,
                "error_code": code
            }),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

fn catalog_schema_for_projection(apps: &mut [CatalogApp]) -> u32 {
    if apps.iter().all(|app| app.discovery.is_some()) {
        RICH_CATALOG_SCHEMA_VERSION
    } else {
        for app in apps {
            app.discovery = None;
        }
        CATALOG_SCHEMA_VERSION
    }
}

#[derive(Clone)]
struct ObjectReader {
    chunks: Arc<PathBuf>,
}

impl ObjectReader {
    async fn open(root: &Path) -> Result<Self, PublisherError> {
        if !root.is_absolute() {
            return Err(PublisherError::Configuration(
                "submission object root must be absolute",
            ));
        }
        let root = checked_directory(root).await?;
        let chunks = checked_directory(&root.join("chunks")).await?;
        if !chunks.starts_with(&root) {
            return Err(PublisherError::Configuration(
                "submission object directory escapes its root",
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
    ) -> Result<Vec<u8>, PublisherError> {
        if !valid_sha256(sha256) || expected_bytes == 0 || expected_bytes > MAX_CHUNK_BYTES {
            return Err(PublisherError::InvalidState(
                "chunk request is outside limits",
            ));
        }
        let path = self
            .chunks
            .join(&sha256[..2])
            .join(format!("{sha256}.chunk"));
        let digest = sha256.to_owned();
        tokio::task::spawn_blocking(move || read_verified_file(&path, expected_bytes, &digest))
            .await
            .map_err(|_| PublisherError::InvalidState("object reader task failed"))?
            .map_err(PublisherError::Io)
    }
}

#[derive(Clone)]
struct PublicationRoot {
    root: Arc<PathBuf>,
    generations: Arc<PathBuf>,
}

impl PublicationRoot {
    async fn open(root: &Path) -> Result<Self, PublisherError> {
        if !root.is_absolute() {
            return Err(PublisherError::Configuration(
                "publication root must be absolute",
            ));
        }
        let root = checked_directory(root).await?;
        let generations = root.join("generations");
        match tokio::fs::symlink_metadata(&generations).await {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(PublisherError::Configuration(
                    "publication generations path is not a real directory",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tokio::fs::create_dir(&generations).await?;
                tokio::fs::set_permissions(&generations, fs::Permissions::from_mode(0o755)).await?;
            }
            Err(error) => return Err(error.into()),
        }
        let generations = tokio::fs::canonicalize(&generations).await?;
        if !generations.starts_with(&root) {
            return Err(PublisherError::Configuration(
                "publication generations directory escapes its root",
            ));
        }
        Ok(Self {
            root: Arc::new(root),
            generations: Arc::new(generations),
        })
    }

    async fn write_generation(&self, prepared: &PreparedPublication) -> Result<(), PublisherError> {
        let root = self.root.as_ref().clone();
        let generations = self.generations.as_ref().clone();
        let prepared = prepared.clone();
        tokio::task::spawn_blocking(move || write_generation_sync(&root, &generations, &prepared))
            .await
            .map_err(|_| PublisherError::InvalidState("publication writer task failed"))?
            .map_err(PublisherError::Io)
    }

    async fn verify_catalog(&self, sequence: u64, expected: &[u8]) -> Result<(), PublisherError> {
        let path = self
            .generations
            .join(sequence.to_string())
            .join("catalog.json");
        let expected = expected.to_vec();
        tokio::task::spawn_blocking(move || verify_exact_file(&path, &expected))
            .await
            .map_err(|_| PublisherError::InvalidState("Catalog verifier task failed"))?
            .map_err(PublisherError::Io)
    }

    async fn verify_committed_generation(
        &self,
        sequence: u64,
        expected_catalog: &[u8],
        expected_leaf: &[u8],
        expected_checkpoint: &[u8],
        expected_public_key: &[u8; 32],
    ) -> Result<(), PublisherError> {
        let directory = self.generations.join(sequence.to_string());
        let expected_catalog = expected_catalog.to_vec();
        let expected_leaf = expected_leaf.to_vec();
        let expected_checkpoint = expected_checkpoint.to_vec();
        let expected_public_key = *expected_public_key;
        tokio::task::spawn_blocking(move || {
            verify_committed_generation_sync(
                &directory,
                &expected_catalog,
                &expected_leaf,
                &expected_checkpoint,
                &expected_public_key,
            )
        })
        .await
        .map_err(|_| PublisherError::InvalidState("generation verifier task failed"))?
        .map_err(PublisherError::Io)
    }

    async fn require_no_current(&self) -> Result<(), PublisherError> {
        match tokio::fs::symlink_metadata(self.root.join("current")).await {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Ok(_) => Err(PublisherError::InvalidState(
                "publication pointer exists without a committed Catalog snapshot",
            )),
            Err(error) => Err(error.into()),
        }
    }

    async fn switch_current(
        &self,
        sequence: u64,
        expected_catalog: &[u8],
    ) -> Result<(), PublisherError> {
        self.verify_catalog(sequence, expected_catalog).await?;
        let root = self.root.as_ref().clone();
        tokio::task::spawn_blocking(move || switch_current_sync(&root, sequence))
            .await
            .map_err(|_| PublisherError::InvalidState("current pointer task failed"))?
            .map_err(PublisherError::Io)
    }
}

struct StoreSigner {
    secret: [u8; 32],
    public_key: [u8; 32],
    key_id: String,
}

impl StoreSigner {
    fn open(path: &Path) -> Result<Self, PublisherError> {
        if !path.is_absolute() {
            return Err(PublisherError::Configuration(
                "Store signing key path must be absolute",
            ));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != 32 || metadata.mode() & 0o077 != 0 {
            return Err(PublisherError::Configuration(
                "Store signing key must be a 32-byte regular file with mode 0600 or stricter",
            ));
        }
        let mut secret = [0_u8; 32];
        file.read_exact(&mut secret)?;
        let mut extra = [0_u8; 1];
        if file.read(&mut extra)? != 0 {
            return Err(PublisherError::Configuration(
                "Store signing key has trailing bytes",
            ));
        }
        let public_key = cp0_package::public_key(&secret);
        Ok(Self {
            key_id: lower_hex(&cp0_package::key_id(&public_key)),
            secret,
            public_key,
        })
    }
}

impl Drop for StoreSigner {
    fn drop(&mut self) {
        self.secret.fill(0);
    }
}

#[derive(Clone)]
struct PreparedPublication {
    catalog_encoded: Vec<u8>,
    catalog_sha256: String,
    catalog_relative_path: String,
    app_count: usize,
    package: Option<PreparedPackage>,
    store_public_key: [u8; 32],
    store_key_id: String,
    transparency: PreparedTransparency,
}

#[derive(Clone)]
struct PreparedTransparency {
    tree_index: u64,
    leaf_sha256: String,
    leaf_encoded: Vec<u8>,
    tree_size: u64,
    root_sha256: String,
    checkpoint_encoded: Vec<u8>,
}

#[derive(Clone)]
struct PreparedPackage {
    release_id: String,
    submission_id: String,
    relative_path: String,
    package_sha256: String,
    encoded: Vec<u8>,
    app: CatalogApp,
}

enum Preparation {
    Ready(Box<PreparedPublication>),
    Superseded,
}

enum CommitOutcome {
    Published,
    Superseded,
}

struct ProjectedApp {
    sequence: u64,
    state: String,
    app: CatalogApp,
}

struct ReleaseSource {
    submission_id: String,
    app_id: String,
    version: String,
    package_sha256: String,
    package_bytes: usize,
    listing_sha256: String,
    listing_bytes: usize,
    assets: Vec<ImageAsset>,
    finalized_content_sha256: String,
    owner_team_id: String,
    default_locale: String,
    developer_name: String,
}

struct PublicationJob {
    event_id: String,
    release_id: String,
    job_kind: String,
    source_resource_version: u64,
    source_state: String,
    lease_token: String,
    attempts: i16,
    catalog_sequence: u64,
    published_unix_seconds: u64,
    expires_unix_seconds: u64,
}

pub async fn connect(database_url: &str, max_connections: u32) -> Result<PgPool, PublisherError> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await
        .map_err(PublisherError::Database)
}

/// Applies the complete Store schema, including the isolated publication queue.
pub async fn migrate(pool: &PgPool) -> Result<(), PublisherError> {
    sqlx::migrate!("../cp0-store-control-server/migrations")
        .run(pool)
        .await
        .map_err(PublisherError::Migration)
}

fn validate_release_metadata(
    release: &ReleaseSource,
    manifest: &AppManifest,
    listing: &StoreListing,
) -> Result<(), PublisherError> {
    if manifest.id != release.app_id
        || manifest.version != release.version
        || listing.app_id != release.app_id
        || listing.version != release.version
        || listing.default_locale != release.default_locale
    {
        return Err(PublisherError::InvalidState(
            "package, Listing and Release identities differ",
        ));
    }
    let localized = listing
        .localizations
        .iter()
        .find(|localized| localized.locale == listing.default_locale)
        .ok_or(PublisherError::InvalidState(
            "default Listing localization is missing",
        ))?;
    if localized.name != manifest.name {
        return Err(PublisherError::InvalidState(
            "package and Listing names differ",
        ));
    }
    let mut listed_assets = Vec::with_capacity(1 + listing.screenshots.len());
    listed_assets.push(&listing.icon);
    listed_assets.extend(listing.screenshots.iter());
    if listed_assets.len() != release.assets.len()
        || listed_assets
            .iter()
            .zip(&release.assets)
            .any(|(listed, stored)| *listed != stored)
    {
        return Err(PublisherError::InvalidState(
            "Listing assets differ from the approved Submission",
        ));
    }
    Ok(())
}

async fn lock_active_job(
    transaction: &mut Transaction<'_, Postgres>,
    job: &PublicationJob,
) -> Result<(), PublisherError> {
    let row = sqlx::query(
        "SELECT state, lease_token, catalog_sequence, source_resource_version, source_state \
         FROM store_publication_jobs WHERE event_id = $1 FOR UPDATE",
    )
    .bind(&job.event_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(PublisherError::InvalidState("publication job is missing"))?;
    if row.get::<String, _>("state") != "running"
        || row.get::<Option<String>, _>("lease_token").as_deref() != Some(&job.lease_token)
        || row.get::<Option<i64>, _>("catalog_sequence")
            != Some(i64_from_u64(job.catalog_sequence)?)
        || positive_u64(row.get::<i64, _>("source_resource_version"))?
            != job.source_resource_version
        || row.get::<String, _>("source_state") != job.source_state
    {
        return Err(PublisherError::InvalidState("publication lease is stale"));
    }
    Ok(())
}

async fn mark_job_superseded(
    transaction: &mut Transaction<'_, Postgres>,
    job: &PublicationJob,
    code: &str,
) -> Result<(), PublisherError> {
    let now = database_now(transaction).await?;
    let changed = sqlx::query(
        "UPDATE store_publication_jobs SET state = 'superseded', lease_token = NULL, \
         leased_until_unix_seconds = NULL, last_error_code = $1, completed_unix_seconds = $2 \
         WHERE event_id = $3 AND state = 'running' AND lease_token = $4",
    )
    .bind(code)
    .bind(now)
    .bind(&job.event_id)
    .bind(&job.lease_token)
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(PublisherError::InvalidState(
            "superseded publication lease was lost",
        ));
    }
    Ok(())
}

async fn append_release_published(
    transaction: &mut Transaction<'_, Postgres>,
    worker_id: &str,
    job: &PublicationJob,
    resource_version: u64,
    catalog_sha256: &str,
    now: i64,
) -> Result<(), PublisherError> {
    let request_id = prefixed_uuid("publishreq_");
    let key_sha256 = sha256_hex(job.event_id.as_bytes());
    sqlx::query(
        "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
         object_id, before_state, after_state, resource_version, request_id, request_sha256, \
         idempotency_key_sha256) VALUES ($1, $2, 'release.published', 'release', $3, \
         'publishing', 'published', $4, $5, $6, $7)",
    )
    .bind(now)
    .bind(worker_id)
    .bind(&job.release_id)
    .bind(i64_from_u64(resource_version)?)
    .bind(&request_id)
    .bind(catalog_sha256)
    .bind(&key_sha256)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds) \
         VALUES ($1, 'release.published', 'release', $2, $3, $4, $5, $6)",
    )
    .bind(prefixed_uuid("evt_"))
    .bind(&job.release_id)
    .bind(i64_from_u64(resource_version)?)
    .bind(catalog_sha256)
    .bind(json!({
        "release_id": job.release_id,
        "catalog_sequence": job.catalog_sequence,
        "catalog_sha256": catalog_sha256
    }))
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn append_release_failed(
    transaction: &mut Transaction<'_, Postgres>,
    worker_id: &str,
    job: &PublicationJob,
    resource_version: u64,
    code: &str,
    now: i64,
) -> Result<(), PublisherError> {
    let failure_sha256 = sha256_hex(format!("{}\0{code}", job.event_id).as_bytes());
    let request_id = prefixed_uuid("publishreq_");
    let key_sha256 = sha256_hex(job.event_id.as_bytes());
    sqlx::query(
        "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
         object_id, before_state, after_state, resource_version, request_id, request_sha256, \
         idempotency_key_sha256) VALUES ($1, $2, 'release.publish-failed', 'release', $3, \
         'publishing', 'publish-failed', $4, $5, $6, $7)",
    )
    .bind(now)
    .bind(worker_id)
    .bind(&job.release_id)
    .bind(i64_from_u64(resource_version)?)
    .bind(&request_id)
    .bind(&failure_sha256)
    .bind(&key_sha256)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds) \
         VALUES ($1, 'release.publish-failed', 'release', $2, $3, $4, $5, $6)",
    )
    .bind(prefixed_uuid("evt_"))
    .bind(&job.release_id)
    .bind(i64_from_u64(resource_version)?)
    .bind(&failure_sha256)
    .bind(json!({"release_id": job.release_id, "error_code": code}))
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn append_catalog_event(
    transaction: &mut Transaction<'_, Postgres>,
    worker_id: &str,
    job: &PublicationJob,
    action: &str,
    topic: &str,
    request_sha256: &str,
    payload: Value,
) -> Result<(), PublisherError> {
    let now = database_now(transaction).await?;
    let request_id = prefixed_uuid("catalogreq_");
    let key_sha256 = sha256_hex(job.event_id.as_bytes());
    let after_state = match action {
        "catalog.published" => "published",
        "catalog.publish-failed" => "failed",
        "catalog.publication-superseded" => "superseded",
        _ => {
            return Err(PublisherError::InvalidState(
                "Catalog audit action is invalid",
            ));
        }
    };
    sqlx::query(
        "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
         object_id, before_state, after_state, resource_version, request_id, request_sha256, \
         idempotency_key_sha256) VALUES ($1, $2, $3, 'catalog', $4, NULL, $5, \
         $6, $7, $8, $9)",
    )
    .bind(now)
    .bind(worker_id)
    .bind(action)
    .bind(job.catalog_sequence.to_string())
    .bind(after_state)
    .bind(i64_from_u64(job.catalog_sequence)?)
    .bind(&request_id)
    .bind(request_sha256)
    .bind(&key_sha256)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds) \
         VALUES ($1, $2, 'catalog', $3, $4, $5, $6, $7)",
    )
    .bind(prefixed_uuid("evt_"))
    .bind(topic)
    .bind(job.catalog_sequence.to_string())
    .bind(i64_from_u64(job.catalog_sequence)?)
    .bind(request_sha256)
    .bind(payload)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn write_generation_sync(
    root: &Path,
    generations: &Path,
    prepared: &PreparedPublication,
) -> io::Result<()> {
    let signed = cp0_store_protocol::decode_signed_catalog(&prepared.catalog_encoded)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Catalog encoding is invalid"))?;
    let sequence = signed.catalog.sequence;
    let final_directory = generations.join(sequence.to_string());
    if final_directory.exists() {
        return verify_generation_sync(&final_directory, prepared);
    }
    let temporary = generations.join(format!(".tmp-{sequence}-{}", Uuid::new_v4().simple()));
    fs::create_dir(&temporary)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
    if let Some(package) = &prepared.package {
        let package_directory = temporary.join("packages");
        fs::create_dir(&package_directory)?;
        fs::set_permissions(&package_directory, fs::Permissions::from_mode(0o755))?;
        write_new_synced(
            &package_directory.join(format!("{}.capp", package.release_id)),
            &package.encoded,
            0o444,
        )?;
        sync_directory(&package_directory)?;
    }
    write_new_synced(
        &temporary.join("catalog.json"),
        &prepared.catalog_encoded,
        0o444,
    )?;
    let transparency_directory = temporary.join("transparency");
    fs::create_dir(&transparency_directory)?;
    fs::set_permissions(&transparency_directory, fs::Permissions::from_mode(0o755))?;
    write_new_synced(
        &transparency_directory.join("leaf.json"),
        &prepared.transparency.leaf_encoded,
        0o444,
    )?;
    write_new_synced(
        &transparency_directory.join("checkpoint.json"),
        &prepared.transparency.checkpoint_encoded,
        0o444,
    )?;
    sync_directory(&transparency_directory)?;
    write_new_synced(
        &temporary.join("store.pub"),
        &prepared.store_public_key,
        0o444,
    )?;
    sync_directory(&temporary)?;
    match fs::rename(&temporary, &final_directory) {
        Ok(()) => {
            sync_directory(generations)?;
            verify_generation_sync(&final_directory, prepared)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            verify_generation_sync(&final_directory, prepared)
        }
        Err(error) => Err(error),
    }
    .and_then(|result| {
        if !final_directory.starts_with(root) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "generation escaped publication root",
            ));
        }
        Ok(result)
    })
}

fn verify_generation_sync(directory: &Path, prepared: &PreparedPublication) -> io::Result<()> {
    let metadata = fs::symlink_metadata(directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "generation is not a real directory",
        ));
    }
    verify_exact_file(&directory.join("catalog.json"), &prepared.catalog_encoded)?;
    verify_exact_file(&directory.join("store.pub"), &prepared.store_public_key)?;
    let transparency_directory = directory.join("transparency");
    let transparency_metadata = fs::symlink_metadata(&transparency_directory)?;
    if transparency_metadata.file_type().is_symlink() || !transparency_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "transparency path is not a real directory",
        ));
    }
    verify_exact_file(
        &transparency_directory.join("leaf.json"),
        &prepared.transparency.leaf_encoded,
    )?;
    verify_exact_file(
        &transparency_directory.join("checkpoint.json"),
        &prepared.transparency.checkpoint_encoded,
    )?;
    if let Some(package) = &prepared.package {
        let package_directory = directory.join("packages");
        verify_exact_file(
            &package_directory.join(format!("{}.capp", package.release_id)),
            &package.encoded,
        )?;
        fs::set_permissions(package_directory, fs::Permissions::from_mode(0o555))?;
    }
    fs::set_permissions(transparency_directory, fs::Permissions::from_mode(0o555))?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o555))?;
    Ok(())
}

fn verify_committed_generation_sync(
    directory: &Path,
    expected_catalog: &[u8],
    expected_leaf: &[u8],
    expected_checkpoint: &[u8],
    expected_public_key: &[u8; 32],
) -> io::Result<()> {
    let metadata = fs::symlink_metadata(directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "committed generation is not a real directory",
        ));
    }
    let transparency_directory = directory.join("transparency");
    let transparency_metadata = fs::symlink_metadata(&transparency_directory)?;
    if transparency_metadata.file_type().is_symlink() || !transparency_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "committed transparency path is not a real directory",
        ));
    }
    verify_exact_file(&directory.join("catalog.json"), expected_catalog)?;
    verify_exact_file(&transparency_directory.join("leaf.json"), expected_leaf)?;
    verify_exact_file(
        &transparency_directory.join("checkpoint.json"),
        expected_checkpoint,
    )?;
    verify_exact_file(&directory.join("store.pub"), expected_public_key)
}

fn switch_current_sync(root: &Path, sequence: u64) -> io::Result<()> {
    let current = root.join("current");
    let target = PathBuf::from("generations").join(sequence.to_string());
    match fs::symlink_metadata(&current) {
        Ok(metadata) if !metadata.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "current publication pointer is not a symbolic link",
            ));
        }
        Ok(_) if fs::read_link(&current)? == target => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let temporary = root.join(format!(".current-{}", Uuid::new_v4().simple()));
    symlink(&target, &temporary)?;
    fs::rename(&temporary, &current)?;
    sync_directory(root)
}

fn write_new_synced(path: &Path, encoded: &[u8], mode: u32) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(encoded)?;
    file.sync_all()?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

fn verify_exact_file(path: &Path, expected: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != expected.len() as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "published artifact metadata differs",
        ));
    }
    let mut encoded = Vec::with_capacity(expected.len());
    Read::by_ref(&mut file)
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut encoded)?;
    if encoded != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "published artifact bytes differ",
        ));
    }
    Ok(())
}

fn read_verified_file(path: &Path, expected_bytes: usize, digest: &str) -> io::Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != expected_bytes as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "content object metadata differs",
        ));
    }
    let mut encoded = Vec::with_capacity(expected_bytes);
    Read::by_ref(&mut file)
        .take(expected_bytes as u64 + 1)
        .read_to_end(&mut encoded)?;
    if encoded.len() != expected_bytes || sha256_hex(&encoded) != digest {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "content object digest differs",
        ));
    }
    Ok(encoded)
}

fn sync_directory(path: &Path) -> io::Result<()> {
    let directory = File::open(path)?;
    match directory.sync_all() {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::EINVAL) | Some(libc::ENOTSUP)
            ) || (cfg!(target_os = "macos")
                && error.kind() == io::ErrorKind::PermissionDenied) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn classify_preparation_error(error: &PublisherError) -> (&'static str, bool) {
    match error {
        PublisherError::Io(_) => ("object-unavailable", false),
        PublisherError::Database(_) | PublisherError::Migration(_) => {
            ("database-unavailable", false)
        }
        PublisherError::Configuration(_) => ("publisher-misconfigured", true),
        PublisherError::InvalidState(_) => ("source-invalid", true),
        PublisherError::Package(_)
        | PublisherError::Manifest(_)
        | PublisherError::Metadata(_)
        | PublisherError::Protocol(_)
        | PublisherError::Transparency(_) => ("content-invalid", true),
    }
}

fn is_retryable_transaction_error(error: &PublisherError) -> bool {
    matches!(
        error,
        PublisherError::Database(sqlx::Error::Database(database_error))
            if matches!(database_error.code().as_deref(), Some("40001" | "40P01"))
    )
}

fn publication_job_from_row(row: sqlx::postgres::PgRow) -> Result<PublicationJob, PublisherError> {
    Ok(PublicationJob {
        event_id: row.get("event_id"),
        release_id: row.get("release_id"),
        job_kind: row.get("job_kind"),
        source_resource_version: positive_u64(row.get::<i64, _>("source_resource_version"))?,
        source_state: row.get("source_state"),
        lease_token: row.get::<Option<String>, _>("lease_token").ok_or(
            PublisherError::InvalidState("publication lease token is missing"),
        )?,
        attempts: row.get("attempts"),
        catalog_sequence: positive_u64(
            row.get::<Option<i64>, _>("catalog_sequence")
                .ok_or(PublisherError::InvalidState("Catalog sequence is missing"))?,
        )?,
        published_unix_seconds: positive_u64(
            row.get::<Option<i64>, _>("published_unix_seconds").ok_or(
                PublisherError::InvalidState("publication timestamp is missing"),
            )?,
        )?,
        expires_unix_seconds: positive_u64(
            row.get::<Option<i64>, _>("expires_unix_seconds")
                .ok_or(PublisherError::InvalidState("Catalog expiry is missing"))?,
        )?,
    })
}

async fn begin_serializable(pool: &PgPool) -> Result<Transaction<'_, Postgres>, PublisherError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await?;
    Ok(transaction)
}

async fn database_now(transaction: &mut Transaction<'_, Postgres>) -> Result<i64, PublisherError> {
    sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(&mut **transaction)
        .await
        .map_err(PublisherError::Database)
}

async fn checked_directory(path: &Path) -> Result<PathBuf, PublisherError> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PublisherError::Configuration(
            "configured path is not a real directory",
        ));
    }
    tokio::fs::canonicalize(path)
        .await
        .map_err(PublisherError::Io)
}

fn positive_size(value: i64) -> Result<usize, PublisherError> {
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PublisherError::InvalidState("object size is invalid"))
}

fn positive_u64(value: i64) -> Result<u64, PublisherError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PublisherError::InvalidState(
            "positive database value is invalid",
        ))
}

fn i64_from_u64(value: u64) -> Result<i64, PublisherError> {
    i64::try_from(value).map_err(|_| PublisherError::InvalidState("value exceeds database range"))
}

fn package_relative_path(sequence: u64, release_id: &str) -> String {
    format!("generations/{sequence}/packages/{release_id}.capp")
}

fn catalog_relative_path(sequence: u64) -> String {
    format!("generations/{sequence}/catalog.json")
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

#[cfg(test)]
mod tests {
    use super::*;
    use cp0_store_metadata::{AgeRating, StoreCategory};

    fn projected_app(discovery: bool) -> CatalogApp {
        let summary = "A migration-safe discovery fixture".to_owned();
        CatalogApp {
            app_id: "dev.cardputerzero.publisher".into(),
            name: "Publisher".into(),
            version: "1.0.0".into(),
            sdk_version: "1.0".into(),
            summary: summary.clone(),
            package_url: "https://store.example.com/publisher.capp".into(),
            package_sha256: "11".repeat(32),
            package_bytes: 4096,
            permissions: Vec::new(),
            discovery: discovery.then_some(CatalogDiscovery {
                developer: "CardputerZero Labs".into(),
                subtitle: summary,
                category: StoreCategory::Utilities,
                keywords: vec!["publisher".into()],
                age_rating: AgeRating::FourPlus,
                privacy_url: "https://example.com/privacy".into(),
                support_url: "https://example.com/support".into(),
            }),
        }
    }

    #[test]
    fn publication_paths_are_stable() {
        assert_eq!(catalog_relative_path(42), "generations/42/catalog.json");
        assert_eq!(
            package_relative_path(42, "rel_11111111111111111111111111111111"),
            "generations/42/packages/rel_11111111111111111111111111111111.capp"
        );
    }

    #[test]
    fn mixed_legacy_projection_stays_pure_v1_until_every_artifact_is_rich() {
        let mut rich = vec![projected_app(true)];
        assert_eq!(
            catalog_schema_for_projection(&mut rich),
            RICH_CATALOG_SCHEMA_VERSION
        );
        assert!(rich[0].discovery.is_some());

        let mut mixed = vec![projected_app(true), projected_app(false)];
        assert_eq!(
            catalog_schema_for_projection(&mut mixed),
            CATALOG_SCHEMA_VERSION
        );
        assert!(mixed.iter().all(|app| app.discovery.is_none()));
    }

    #[test]
    fn worker_and_error_identifiers_are_bounded() {
        assert!(valid_worker_id("publisher-primary"));
        assert!(!valid_worker_id("Publisher"));
        assert!(valid_error_code("source-invalid"));
        assert!(!valid_error_code("source_invalid"));
    }
}
