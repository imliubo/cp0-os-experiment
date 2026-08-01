use std::path::{Path, PathBuf};

use cp0_manifest::{
    AppManifest, DisplayMode, Permission, PermissionRequest, ResourceLimits, Runtime,
};
use cp0_package::{CApp, PackageEntry};
use cp0_store_metadata::{AgeRating, ImageAsset, LocalizedListing, StoreCategory, StoreListing};
use cp0_store_scan::{ScanDisposition, submission_content_sha256};
use cp0_store_scan_worker::{RunOutcome, ScanWorker, connect, migrate};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

const TEAM_ID: &str = "team_11111111111111111111111111111111";
const KEY_ID: &str = "key_11111111111111111111111111111111";
const APP_ID: &str = "dev.example.scan";

#[tokio::test]
#[ignore = "requires CP0_STORE_TEST_DATABASE_URL"]
async fn postgres_scan_worker_acceptance() {
    let Ok(database_url) = std::env::var("CP0_STORE_TEST_DATABASE_URL") else {
        return;
    };
    let pool = connect(&database_url, 8).await.unwrap();
    migrate(&pool).await.unwrap();
    reset_database(&pool).await;
    let object_root = test_object_root();
    if tokio::fs::symlink_metadata(&object_root).await.is_ok() {
        tokio::fs::remove_dir_all(&object_root).await.unwrap();
    }
    tokio::fs::create_dir_all(object_root.join("chunks"))
        .await
        .unwrap();

    let fixture = Fixture::new("1.0.0");
    seed_identity(&pool, &fixture.developer_key).await;
    let accepted = seed_submission(
        &pool,
        &object_root,
        "sub_11111111111111111111111111111111",
        "evt_11111111111111111111111111111111",
        1,
        &fixture,
        SeedMode::Pending,
    )
    .await;
    let worker = ScanWorker::open(pool.clone(), &object_root, "scanner-acceptance")
        .await
        .unwrap();
    assert_eq!(
        worker.run_once().await.unwrap(),
        RunOutcome::Completed {
            submission_id: accepted.submission_id.clone(),
            disposition: ScanDisposition::ReadyForReview,
        }
    );
    assert_submission(&pool, &accepted.submission_id, "ready-for-review", 3).await;
    assert_eq!(count(&pool, "submission_scan_results").await, 1);
    assert_eq!(count(&pool, "submission_risk_assessments").await, 1);
    let risk = sqlx::query(
        "SELECT policy_version, tier, reason_codes FROM submission_risk_assessments \
         WHERE submission_id = $1",
    )
    .bind(&accepted.submission_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(risk.get::<i16, _>("policy_version"), 1);
    assert_eq!(risk.get::<String, _>("tier"), "standard");
    assert_eq!(risk.get::<Value, _>("reason_codes"), json!([]));
    assert_eq!(count(&pool, "audit_events").await, 1);
    assert!(matches!(worker.run_once().await.unwrap(), RunOutcome::Idle));

    let result_mutation = sqlx::query(
        "UPDATE submission_scan_results SET outcome = 'rejected' WHERE submission_id = $1",
    )
    .bind(&accepted.submission_id)
    .execute(&pool)
    .await;
    assert_sqlstate(result_mutation, "55000");
    let risk_mutation = sqlx::query(
        "UPDATE submission_risk_assessments SET tier = 'high' WHERE submission_id = $1",
    )
    .bind(&accepted.submission_id)
    .execute(&pool)
    .await;
    assert_sqlstate(risk_mutation, "55000");
    let forged_policy = sqlx::query(
        "INSERT INTO submission_risk_assessments (assessment_id, scan_id, submission_id, \
         source_report_sha256, policy_version, tier, reason_codes, created_unix_seconds) \
         SELECT 'risk_99999999999999999999999999999999', scan_id, submission_id, \
         report_sha256, 2, 'high', '[]'::jsonb, created_unix_seconds \
         FROM submission_scan_results WHERE submission_id = $1",
    )
    .bind(&accepted.submission_id)
    .execute(&pool)
    .await;
    assert_sqlstate(forged_policy, "23514");

    sqlx::query(
        "UPDATE developer_keys SET state = 'revoked', revoked_unix_seconds = 200 WHERE key_id = $1",
    )
    .bind(KEY_ID)
    .execute(&pool)
    .await
    .unwrap();
    let revoked_fixture = Fixture::new("1.0.1");
    let rejected = seed_submission(
        &pool,
        &object_root,
        "sub_22222222222222222222222222222222",
        "evt_22222222222222222222222222222222",
        2,
        &revoked_fixture,
        SeedMode::ExpiredLease { attempts: 1 },
    )
    .await;
    assert_eq!(
        worker.run_once().await.unwrap(),
        RunOutcome::Completed {
            submission_id: rejected.submission_id.clone(),
            disposition: ScanDisposition::Rejected,
        }
    );
    assert_submission(&pool, &rejected.submission_id, "rejected", 3).await;
    let finding: Value =
        sqlx::query_scalar("SELECT report FROM submission_scan_results WHERE submission_id = $1")
            .bind(&rejected.submission_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        finding["findings"][0]["code"],
        "package.developer-key-untrusted"
    );
    let reactivate = sqlx::query(
        "UPDATE developer_keys SET state = 'active', revoked_unix_seconds = NULL WHERE key_id = $1",
    )
    .bind(KEY_ID)
    .execute(&pool)
    .await;
    assert_sqlstate(reactivate, "55000");

    let concurrent_fixture = Fixture::with_signing_key("1.0.2", [9_u8; 32]);
    insert_developer_key(
        &pool,
        "key_44444444444444444444444444444444",
        "Concurrent Key",
        &concurrent_fixture.developer_key,
    )
    .await;
    let concurrent = seed_submission(
        &pool,
        &object_root,
        "sub_33333333333333333333333333333333",
        "evt_33333333333333333333333333333333",
        3,
        &concurrent_fixture,
        SeedMode::Pending,
    )
    .await;
    let (left, right) = tokio::join!(worker.run_once(), worker.run_once());
    let outcomes = [left.unwrap(), right.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, RunOutcome::Completed { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, RunOutcome::Idle))
            .count(),
        1
    );
    assert_submission(&pool, &concurrent.submission_id, "ready-for-review", 3).await;
    assert_eq!(count(&pool, "submission_risk_assessments").await, 2);

    let high_risk_fixture = Fixture::with_permissions(
        "1.0.5",
        [9_u8; 32],
        vec![
            PermissionRequest {
                name: Permission::CameraCapture,
                reason: "Capture a user-requested device image.".into(),
            },
            PermissionRequest {
                name: Permission::NetworkClient,
                reason: "Upload the selected image over HTTPS.".into(),
            },
        ],
    );
    let high_risk = seed_submission(
        &pool,
        &object_root,
        "sub_66666666666666666666666666666666",
        "evt_66666666666666666666666666666666",
        6,
        &high_risk_fixture,
        SeedMode::Pending,
    )
    .await;
    assert!(matches!(
        worker.run_once().await.unwrap(),
        RunOutcome::Completed {
            disposition: ScanDisposition::ReadyForReview,
            ..
        }
    ));
    let high_risk_row = sqlx::query(
        "SELECT tier, reason_codes FROM submission_risk_assessments WHERE submission_id = $1",
    )
    .bind(&high_risk.submission_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(high_risk_row.get::<String, _>("tier"), "high");
    assert_eq!(
        high_risk_row.get::<Value, _>("reason_codes"),
        json!([
            "camera-capture",
            "multiple-sensitive-capabilities",
            "network-access"
        ])
    );

    let missing_fixture = Fixture::with_signing_key("1.0.3", [9_u8; 32]);
    let missing = seed_submission(
        &pool,
        &object_root,
        "sub_44444444444444444444444444444444",
        "evt_44444444444444444444444444444444",
        4,
        &missing_fixture,
        SeedMode::ExpiredLease { attempts: 7 },
    )
    .await;
    remove_object(&object_root, &missing_fixture.package).await;
    assert_eq!(
        worker.run_once().await.unwrap(),
        RunOutcome::Deferred {
            submission_id: missing.submission_id.clone(),
            failed_permanently: true,
        }
    );
    assert_submission(&pool, &missing.submission_id, "processing", 2).await;
    let job = sqlx::query(
        "SELECT state, attempts, last_error_code FROM submission_scan_jobs WHERE event_id = $1",
    )
    .bind(&missing.event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(job.get::<String, _>("state"), "failed");
    assert_eq!(job.get::<i16, _>("attempts"), 8);
    assert_eq!(
        job.get::<Option<String>, _>("last_error_code").as_deref(),
        Some("object-unavailable")
    );
    assert_eq!(count(&pool, "submission_scan_results").await, 4);

    let exhausted_fixture = Fixture::with_signing_key("1.0.4", [9_u8; 32]);
    let exhausted = seed_submission(
        &pool,
        &object_root,
        "sub_55555555555555555555555555555555",
        "evt_55555555555555555555555555555555",
        5,
        &exhausted_fixture,
        SeedMode::ExpiredLease { attempts: 8 },
    )
    .await;
    assert!(matches!(worker.run_once().await.unwrap(), RunOutcome::Idle));
    assert_submission(&pool, &exhausted.submission_id, "processing", 2).await;
    let exhausted_job = sqlx::query(
        "SELECT state, last_error_code, completed_unix_seconds FROM submission_scan_jobs \
         WHERE event_id = $1",
    )
    .bind(&exhausted.event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(exhausted_job.get::<String, _>("state"), "failed");
    assert_eq!(
        exhausted_job
            .get::<Option<String>, _>("last_error_code")
            .as_deref(),
        Some("lease-exhausted")
    );
    assert!(
        exhausted_job
            .get::<Option<i64>, _>("completed_unix_seconds")
            .is_some()
    );

    let delete_job = sqlx::query("DELETE FROM submission_scan_jobs WHERE event_id = $1")
        .bind(&missing.event_id)
        .execute(&pool)
        .await;
    assert_sqlstate(delete_job, "55000");
}

#[derive(Clone, Copy)]
enum SeedMode {
    Pending,
    ExpiredLease { attempts: i16 },
}

struct Seeded {
    submission_id: String,
    event_id: String,
}

async fn seed_identity(pool: &PgPool, developer_key: &[u8; 32]) {
    sqlx::query("INSERT INTO teams (team_id, name) VALUES ($1, 'Scan Team')")
        .bind(TEAM_ID)
        .execute(pool)
        .await
        .unwrap();
    insert_developer_key(pool, KEY_ID, "Acceptance Key", developer_key).await;
    sqlx::query(
        "INSERT INTO apps (app_id, owner_team_id, default_locale, resource_version, \
         created_unix_seconds) VALUES ($1, $2, 'en-US', 1, 100)",
    )
    .bind(APP_ID)
    .bind(TEAM_ID)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_developer_key(pool: &PgPool, key_id: &str, name: &str, developer_key: &[u8; 32]) {
    sqlx::query(
        "INSERT INTO developer_keys (key_id, team_id, name, algorithm, public_key, \
         fingerprint_sha256, state, created_unix_seconds) \
         VALUES ($1, $2, $3, 'ed25519', $4, $5, 'active', 100)",
    )
    .bind(key_id)
    .bind(TEAM_ID)
    .bind(name)
    .bind(developer_key.as_slice())
    .bind(sha256_hex(developer_key))
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_submission(
    pool: &PgPool,
    object_root: &Path,
    submission_id: &str,
    event_id: &str,
    revision: i32,
    fixture: &Fixture,
    mode: SeedMode,
) -> Seeded {
    let package_sha256 = sha256_hex(&fixture.package);
    let listing_sha256 = sha256_hex(&fixture.listing_encoded);
    let content_sha256 =
        submission_content_sha256(&package_sha256, &listing_sha256, &fixture.descriptors);
    sqlx::query(
        "INSERT INTO submissions (submission_id, app_id, version, revision, state, \
         package_sha256, package_bytes, listing_sha256, listing_bytes, assets, resource_version, \
         created_unix_seconds, finalized_content_sha256) \
         VALUES ($1, $2, $3, $4, 'processing', $5, $6, $7, $8, $9, 2, 100, $10)",
    )
    .bind(submission_id)
    .bind(APP_ID)
    .bind(&fixture.version)
    .bind(revision)
    .bind(&package_sha256)
    .bind(fixture.package.len() as i64)
    .bind(&listing_sha256)
    .bind(fixture.listing_encoded.len() as i64)
    .bind(serde_json::to_value(&fixture.descriptors).unwrap())
    .bind(&content_sha256)
    .execute(pool)
    .await
    .unwrap();

    let mut parts = vec![
        ("package".to_owned(), fixture.package.as_slice()),
        ("listing".to_owned(), fixture.listing_encoded.as_slice()),
    ];
    for (index, asset) in fixture.assets.iter().enumerate() {
        parts.push((format!("asset-{index}"), asset.as_slice()));
    }
    for (name, encoded) in parts {
        let digest = sha256_hex(encoded);
        sqlx::query(
            "INSERT INTO submission_upload_parts (submission_id, part_name, expected_sha256, \
             expected_bytes, received_bytes) VALUES ($1, $2, $3, $4, $4)",
        )
        .bind(submission_id)
        .bind(&name)
        .bind(&digest)
        .bind(encoded.len() as i64)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO submission_upload_chunks (submission_id, part_name, chunk_offset, \
             chunk_bytes, chunk_sha256, created_unix_seconds) VALUES ($1, $2, 0, $3, $4, 100)",
        )
        .bind(submission_id)
        .bind(&name)
        .bind(encoded.len() as i32)
        .bind(&digest)
        .execute(pool)
        .await
        .unwrap();
        write_object(object_root, encoded).await;
    }
    let published = matches!(mode, SeedMode::ExpiredLease { .. });
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds, \
         published_unix_seconds, attempts) \
         VALUES ($1, 'submission.scan-requested', 'submission', $2, 2, $3, $4, 100, $5, $6)",
    )
    .bind(event_id)
    .bind(submission_id)
    .bind(sha256_hex(event_id.as_bytes()))
    .bind(json!({"submission_id": submission_id, "content_sha256": content_sha256}))
    .bind(published.then_some(100_i64))
    .bind(i32::from(published))
    .execute(pool)
    .await
    .unwrap();
    if let SeedMode::ExpiredLease { attempts } = mode {
        sqlx::query(
            "INSERT INTO submission_scan_jobs (event_id, submission_id, source_resource_version, \
             source_content_sha256, state, lease_token, leased_until_unix_seconds, attempts, \
             created_unix_seconds) VALUES ($1, $2, 2, $3, 'running', \
             'lease_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 1, $4, 100)",
        )
        .bind(event_id)
        .bind(submission_id)
        .bind(&content_sha256)
        .bind(attempts)
        .execute(pool)
        .await
        .unwrap();
    }
    Seeded {
        submission_id: submission_id.to_owned(),
        event_id: event_id.to_owned(),
    }
}

struct Fixture {
    version: String,
    package: Vec<u8>,
    listing_encoded: Vec<u8>,
    descriptors: Vec<ImageAsset>,
    assets: Vec<Vec<u8>>,
    developer_key: [u8; 32],
}

impl Fixture {
    fn new(version: &str) -> Self {
        Self::with_signing_key(version, [7_u8; 32])
    }

    fn with_signing_key(version: &str, signing_key: [u8; 32]) -> Self {
        Self::with_permissions(version, signing_key, Vec::new())
    }

    fn with_permissions(
        version: &str,
        signing_key: [u8; 32],
        permissions: Vec<PermissionRequest>,
    ) -> Self {
        let manifest = AppManifest {
            schema_version: 1,
            id: APP_ID.into(),
            name: "Scan Test".into(),
            version: version.into(),
            sdk_version: "1.0".into(),
            runtime: Runtime::Wamr,
            entrypoint: "app.wasm".into(),
            display: DisplayMode::Standard,
            resources: ResourceLimits {
                memory_mb: 16,
                storage_mb: 16,
            },
            permissions,
            intents: Vec::new(),
        };
        let mut package = CApp::new(vec![
            PackageEntry {
                path: "app.json".into(),
                contents: serde_json::to_vec(&manifest).unwrap(),
            },
            PackageEntry {
                path: "app.wasm".into(),
                contents: b"\0asm\x01\0\0\0".to_vec(),
            },
        ])
        .unwrap();
        package.sign_developer(&signing_key).unwrap();
        let developer_key = package.developer_public_key().unwrap();
        let package = package.encode().unwrap();
        let assets = vec![png(48, 48), png(320, 170)];
        let descriptors = vec![
            descriptor("icon.png", &assets[0], 48, 48),
            descriptor("screen.png", &assets[1], 320, 170),
        ];
        let listing = StoreListing {
            schema_version: 1,
            app_id: APP_ID.into(),
            version: version.into(),
            default_locale: "en-US".into(),
            category: StoreCategory::Utilities,
            age_rating: AgeRating::FourPlus,
            privacy_url: "https://example.com/privacy".into(),
            support_url: "https://example.com/support".into(),
            icon: descriptors[0].clone(),
            screenshots: vec![descriptors[1].clone()],
            localizations: vec![LocalizedListing {
                locale: "en-US".into(),
                name: "Scan Test".into(),
                subtitle: "A scanner fixture".into(),
                description: "A deterministic scanner fixture used by tests.".into(),
                keywords: vec!["scanner".into()],
                release_notes: "Initial release.".into(),
            }],
        };
        Self {
            version: version.into(),
            package,
            listing_encoded: serde_json::to_vec(&listing).unwrap(),
            descriptors,
            assets,
            developer_key,
        }
    }
}

async fn write_object(root: &Path, encoded: &[u8]) {
    let digest = sha256_hex(encoded);
    let directory = root.join("chunks").join(&digest[..2]);
    tokio::fs::create_dir_all(&directory).await.unwrap();
    tokio::fs::write(directory.join(format!("{digest}.chunk")), encoded)
        .await
        .unwrap();
}

async fn remove_object(root: &Path, encoded: &[u8]) {
    let digest = sha256_hex(encoded);
    tokio::fs::remove_file(
        root.join("chunks")
            .join(&digest[..2])
            .join(format!("{digest}.chunk")),
    )
    .await
    .unwrap();
}

async fn assert_submission(pool: &PgPool, submission_id: &str, state: &str, version: i64) {
    let row =
        sqlx::query("SELECT state, resource_version FROM submissions WHERE submission_id = $1")
            .bind(submission_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(row.get::<String, _>("state"), state);
    assert_eq!(row.get::<i64, _>("resource_version"), version);
}

async fn reset_database(pool: &PgPool) {
    sqlx::query(
        "TRUNCATE teams, idempotency_records, audit_events, outbox_events, catalog_sequence \
         RESTART IDENTITY CASCADE",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO catalog_sequence (singleton, last_sequence) VALUES (TRUE, 0)")
        .execute(pool)
        .await
        .unwrap();
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    sqlx::query_scalar(&sql).fetch_one(pool).await.unwrap()
}

fn test_object_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-store-scan-objects")
}

fn assert_sqlstate<T: std::fmt::Debug>(result: Result<T, sqlx::Error>, expected: &str) {
    let error = result.expect_err("database mutation unexpectedly succeeded");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some(expected)
    );
}

fn descriptor(path: &str, encoded: &[u8], width: u16, height: u16) -> ImageAsset {
    ImageAsset {
        path: path.into(),
        sha256: sha256_hex(encoded),
        bytes: encoded.len() as u64,
        width,
        height,
    }
}

fn sha256_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut encoded = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    encoded.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
    encoded.extend_from_slice(&png_chunk(b"IDAT", &[]));
    encoded.extend_from_slice(&png_chunk(b"IEND", &[]));
    encoded
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&(data.len() as u32).to_be_bytes());
    encoded.extend_from_slice(kind);
    encoded.extend_from_slice(data);
    encoded.extend_from_slice(&png_crc32(&encoded[4..]).to_be_bytes());
    encoded
}

fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
