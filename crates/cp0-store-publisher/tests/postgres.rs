use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use cp0_manifest::{AppManifest, DisplayMode, ResourceLimits, Runtime};
use cp0_package::{CApp, PackageEntry};
use cp0_store_metadata::{AgeRating, ImageAsset, LocalizedListing, StoreCategory, StoreListing};
use cp0_store_protocol::{decode_signed_catalog, verify_catalog};
use cp0_store_publisher::{RunOutcome, StorePublisher, connect, migrate};
use cp0_store_scan::submission_content_sha256;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const TEAM_ID: &str = "team_11111111111111111111111111111111";
const MEMBER_ID: &str = "member_11111111111111111111111111111111";
const KEY_ID: &str = "key_11111111111111111111111111111111";
const REVIEWER_ID: &str = "reviewer_11111111111111111111111111111111";
const APP_ID: &str = "dev.example.publisher";

#[tokio::test]
#[ignore = "requires CP0_STORE_TEST_DATABASE_URL"]
async fn postgres_store_publisher_acceptance() {
    let Ok(database_url) = std::env::var("CP0_STORE_TEST_DATABASE_URL") else {
        return;
    };
    let pool = connect(&database_url, 8).await.unwrap();
    migrate(&pool).await.unwrap();
    reset_database(&pool).await;
    let root = test_root();
    let objects = root.join("objects");
    let origin = root.join("origin");
    tokio::fs::create_dir_all(objects.join("chunks"))
        .await
        .unwrap();
    tokio::fs::create_dir_all(&origin).await.unwrap();
    let signing_key_path = root.join("store-signing.key");
    let store_secret = [19_u8; 32];
    tokio::fs::write(&signing_key_path, store_secret)
        .await
        .unwrap();
    tokio::fs::set_permissions(&signing_key_path, std::fs::Permissions::from_mode(0o600))
        .await
        .unwrap();

    let first = Fixture::new(APP_ID, "1.0.0", [7_u8; 32]);
    seed_identity(&pool, APP_ID, &first.developer_public_key).await;
    seed_submission_release(
        &pool,
        &objects,
        &first,
        "sub_11111111111111111111111111111111",
        "rel_11111111111111111111111111111111",
        "decision_11111111111111111111111111111111",
        "evt_11111111111111111111111111111111",
        1,
        1,
    )
    .await;
    let publisher = StorePublisher::open(
        pool.clone(),
        &objects,
        &origin,
        &signing_key_path,
        "https://store.example.invalid",
        "publisher-acceptance",
    )
    .await
    .unwrap();
    assert_eq!(
        publisher.run_once().await.unwrap(),
        RunOutcome::Published {
            event_id: "evt_11111111111111111111111111111111".into(),
            release_id: "rel_11111111111111111111111111111111".into(),
            catalog_sequence: 1,
            app_count: 1,
        }
    );
    assert_release(
        &pool,
        "rel_11111111111111111111111111111111",
        "published",
        3,
        Some(1),
    )
    .await;
    verify_snapshot(&pool, &origin, 1, &publisher.store_public_key(), "1.0.0", 1).await;

    let second = Fixture::new(APP_ID, "1.1.0", [7_u8; 32]);
    seed_submission_release(
        &pool,
        &objects,
        &second,
        "sub_22222222222222222222222222222222",
        "rel_22222222222222222222222222222222",
        "decision_22222222222222222222222222222222",
        "evt_22222222222222222222222222222222",
        2,
        2,
    )
    .await;
    let (left, right) = tokio::join!(publisher.run_once(), publisher.run_once());
    let outcomes = [left.unwrap(), right.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, RunOutcome::Published { .. }))
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
    verify_snapshot(&pool, &origin, 2, &publisher.store_public_key(), "1.1.0", 1).await;

    transition_with_rebuild(
        &pool,
        "rel_22222222222222222222222222222222",
        "paused",
        4,
        "evt_33333333333333333333333333333333",
        3,
    )
    .await;
    assert_published_sequence(&publisher, 3, 0).await;
    transition_with_rebuild(
        &pool,
        "rel_22222222222222222222222222222222",
        "published",
        5,
        "evt_44444444444444444444444444444444",
        4,
    )
    .await;
    assert_published_sequence(&publisher, 4, 1).await;
    verify_snapshot(&pool, &origin, 4, &publisher.store_public_key(), "1.1.0", 1).await;

    transition_with_rebuild(
        &pool,
        "rel_22222222222222222222222222222222",
        "paused",
        6,
        "evt_55555555555555555555555555555555",
        5,
    )
    .await;
    transition_with_rebuild(
        &pool,
        "rel_22222222222222222222222222222222",
        "published",
        7,
        "evt_66666666666666666666666666666666",
        6,
    )
    .await;
    assert!(matches!(
        publisher.run_once().await.unwrap(),
        RunOutcome::Superseded {
            catalog_sequence: 5,
            ..
        }
    ));
    assert_eq!(count_where_sequence(&pool, 5).await, 0);
    assert_published_sequence(&publisher, 6, 1).await;

    transition_with_rebuild(
        &pool,
        "rel_22222222222222222222222222222222",
        "removed",
        8,
        "evt_77777777777777777777777777777777",
        7,
    )
    .await;
    assert_published_sequence(&publisher, 7, 0).await;
    verify_snapshot(&pool, &origin, 7, &publisher.store_public_key(), "", 0).await;

    let current = origin.join("current");
    tokio::fs::remove_file(&current).await.unwrap();
    symlink("generations/1", &current).unwrap();
    let recovered = StorePublisher::open(
        pool.clone(),
        &objects,
        &origin,
        &signing_key_path,
        "https://store.example.invalid",
        "publisher-recovered",
    )
    .await
    .unwrap();
    assert_eq!(
        tokio::fs::read_link(&current).await.unwrap(),
        Path::new("generations/7")
    );

    let failed_app = "dev.example.publisher-failed";
    let failed = Fixture::new(failed_app, "1.0.0", [7_u8; 32]);
    seed_app(&pool, failed_app).await;
    seed_submission_release(
        &pool,
        &objects,
        &failed,
        "sub_88888888888888888888888888888888",
        "rel_88888888888888888888888888888888",
        "decision_88888888888888888888888888888888",
        "evt_88888888888888888888888888888888",
        1,
        8,
    )
    .await;
    sqlx::query(
        "UPDATE developer_keys SET state = 'revoked', revoked_unix_seconds = 10 WHERE key_id = $1",
    )
    .bind(KEY_ID)
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        recovered.run_once().await.unwrap(),
        RunOutcome::Deferred {
            failed_permanently: true,
            ..
        }
    ));
    assert_release(
        &pool,
        "rel_88888888888888888888888888888888",
        "publish-failed",
        3,
        None,
    )
    .await;
    assert_eq!(count_where_sequence(&pool, 8).await, 0);
    assert_eq!(last_catalog_sequence(&pool).await, 8);
    assert_eq!(
        tokio::fs::read_link(&current).await.unwrap(),
        Path::new("generations/7")
    );

    verify_database_guards(&pool).await;
}

async fn assert_published_sequence(publisher: &StorePublisher, sequence: u64, app_count: usize) {
    assert!(matches!(
        publisher.run_once().await.unwrap(),
        RunOutcome::Published {
            catalog_sequence,
            app_count: count,
            ..
        } if catalog_sequence == sequence && count == app_count
    ));
}

async fn verify_snapshot(
    pool: &PgPool,
    origin: &Path,
    sequence: i64,
    public_key: &[u8; 32],
    expected_version: &str,
    expected_apps: usize,
) {
    let encoded: Vec<u8> = sqlx::query_scalar(
        "SELECT encoded_catalog FROM store_catalog_snapshots WHERE sequence = $1",
    )
    .bind(sequence)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        tokio::fs::read(origin.join(format!("generations/{sequence}/catalog.json")))
            .await
            .unwrap(),
        encoded
    );
    let signed = decode_signed_catalog(&encoded).unwrap();
    verify_catalog(&signed, public_key).unwrap();
    assert_eq!(signed.catalog.sequence, sequence as u64);
    assert_eq!(signed.catalog.apps.len(), expected_apps);
    if expected_apps == 1 {
        let app = &signed.catalog.apps[0];
        assert_eq!(app.version, expected_version);
        let relative = app
            .package_url
            .strip_prefix("https://store.example.invalid/")
            .unwrap();
        let package = tokio::fs::read(origin.join(relative)).await.unwrap();
        assert_eq!(sha256_hex(&package), app.package_sha256);
        let package = CApp::decode(&package).unwrap();
        package.verify_developer_signature().unwrap();
        package.verify_store_signature(public_key).unwrap();
    }
}

async fn transition_with_rebuild(
    pool: &PgPool,
    release_id: &str,
    state: &str,
    resource_version: i64,
    event_id: &str,
    created: i64,
) {
    sqlx::query("UPDATE releases SET state = $1, resource_version = $2 WHERE release_id = $3")
        .bind(state)
        .bind(resource_version)
        .bind(release_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds) \
         VALUES ($1, 'catalog.rebuild-requested', 'release', $2, $3, $4, $5, $6)",
    )
    .bind(event_id)
    .bind(release_id)
    .bind(resource_version)
    .bind(sha256_hex(event_id.as_bytes()))
    .bind(json!({"release_id": release_id, "state": state}))
    .bind(created)
    .execute(pool)
    .await
    .unwrap();
}

struct Fixture {
    app_id: String,
    version: String,
    package: Vec<u8>,
    listing: Vec<u8>,
    assets: Vec<Vec<u8>>,
    descriptors: Vec<ImageAsset>,
    developer_public_key: [u8; 32],
}

impl Fixture {
    fn new(app_id: &str, version: &str, developer_secret: [u8; 32]) -> Self {
        let manifest = AppManifest {
            schema_version: 1,
            id: app_id.into(),
            name: "Publisher Test".into(),
            version: version.into(),
            sdk_version: "1.0".into(),
            runtime: Runtime::Wamr,
            entrypoint: "app.wasm".into(),
            display: DisplayMode::Standard,
            resources: ResourceLimits {
                memory_mb: 16,
                storage_mb: 16,
            },
            permissions: Vec::new(),
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
        package.sign_developer(&developer_secret).unwrap();
        let developer_public_key = package.developer_public_key().unwrap();
        let package = package.encode().unwrap();
        let assets = vec![b"icon-fixture".to_vec(), b"screen-fixture".to_vec()];
        let descriptors = vec![
            descriptor("icon.png", &assets[0], 48, 48),
            descriptor("screen.png", &assets[1], 320, 170),
        ];
        let listing = StoreListing {
            schema_version: 1,
            app_id: app_id.into(),
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
                name: "Publisher Test".into(),
                subtitle: "A deterministic Publisher fixture".into(),
                description: "A deterministic fixture for Store publication acceptance.".into(),
                keywords: vec!["publisher".into()],
                release_notes: "Publication acceptance coverage.".into(),
            }],
        };
        Self {
            app_id: app_id.into(),
            version: version.into(),
            package,
            listing: serde_json::to_vec(&listing).unwrap(),
            assets,
            descriptors,
            developer_public_key,
        }
    }
}

async fn seed_identity(pool: &PgPool, app_id: &str, developer_key: &[u8; 32]) {
    sqlx::query("INSERT INTO teams (team_id, name) VALUES ($1, 'Publisher Team')")
        .bind(TEAM_ID)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO team_members (member_id, team_id, email, role, two_factor_enabled) \
         VALUES ($1, $2, 'publisher@example.com', 'owner', TRUE)",
    )
    .bind(MEMBER_ID)
    .bind(TEAM_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO developer_keys (key_id, team_id, name, algorithm, public_key, \
         fingerprint_sha256, state, created_unix_seconds) \
         VALUES ($1, $2, 'Publisher Key', 'ed25519', $3, $4, 'active', 1)",
    )
    .bind(KEY_ID)
    .bind(TEAM_ID)
    .bind(developer_key.to_vec())
    .bind(sha256_hex(developer_key))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO reviewers (reviewer_id, email, role, two_factor_enabled, state, \
         created_unix_seconds) VALUES ($1, 'reviewer@example.com', 'reviewer', TRUE, 'active', 1)",
    )
    .bind(REVIEWER_ID)
    .execute(pool)
    .await
    .unwrap();
    seed_app(pool, app_id).await;
}

async fn seed_app(pool: &PgPool, app_id: &str) {
    sqlx::query(
        "INSERT INTO apps (app_id, owner_team_id, default_locale, created_unix_seconds) \
         VALUES ($1, $2, 'en-US', 1)",
    )
    .bind(app_id)
    .bind(TEAM_ID)
    .execute(pool)
    .await
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn seed_submission_release(
    pool: &PgPool,
    object_root: &Path,
    fixture: &Fixture,
    submission_id: &str,
    release_id: &str,
    decision_id: &str,
    event_id: &str,
    revision: i32,
    created: i64,
) {
    let package_sha256 = sha256_hex(&fixture.package);
    let listing_sha256 = sha256_hex(&fixture.listing);
    let content_sha256 =
        submission_content_sha256(&package_sha256, &listing_sha256, &fixture.descriptors);
    sqlx::query(
        "INSERT INTO submissions (submission_id, app_id, version, revision, state, \
         package_sha256, package_bytes, listing_sha256, listing_bytes, assets, resource_version, \
         created_unix_seconds, finalized_content_sha256) VALUES \
         ($1, $2, $3, $4, 'approved', $5, $6, $7, $8, $9, 2, $10, $11)",
    )
    .bind(submission_id)
    .bind(&fixture.app_id)
    .bind(&fixture.version)
    .bind(revision)
    .bind(&package_sha256)
    .bind(fixture.package.len() as i64)
    .bind(&listing_sha256)
    .bind(fixture.listing.len() as i64)
    .bind(serde_json::to_value(&fixture.descriptors).unwrap())
    .bind(created)
    .bind(&content_sha256)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_decisions (decision_id, submission_id, reviewer_id, decision, \
         reason_codes, note, created_unix_seconds) VALUES ($1, $2, $3, 'approved', '{}', '', $4)",
    )
    .bind(decision_id)
    .bind(submission_id)
    .bind(REVIEWER_ID)
    .bind(created)
    .execute(pool)
    .await
    .unwrap();
    let parts = [
        ("package", fixture.package.as_slice()),
        ("listing", fixture.listing.as_slice()),
        ("asset-0", fixture.assets[0].as_slice()),
        ("asset-1", fixture.assets[1].as_slice()),
    ];
    for (name, encoded) in parts {
        let digest = sha256_hex(encoded);
        write_object(object_root, &digest, encoded).await;
        sqlx::query(
            "INSERT INTO submission_upload_parts (submission_id, part_name, expected_sha256, \
             expected_bytes, received_bytes) VALUES ($1, $2, $3, $4, $4)",
        )
        .bind(submission_id)
        .bind(name)
        .bind(&digest)
        .bind(encoded.len() as i64)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO submission_upload_chunks (submission_id, part_name, chunk_offset, \
             chunk_bytes, chunk_sha256, created_unix_seconds) VALUES ($1, $2, 0, $3, $4, $5)",
        )
        .bind(submission_id)
        .bind(name)
        .bind(encoded.len() as i32)
        .bind(&digest)
        .bind(created)
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO releases (release_id, submission_id, app_id, version, state, \
         rollout_percent, resource_version, created_unix_seconds) \
         VALUES ($1, $2, $3, $4, 'ready', 100, 1, $5)",
    )
    .bind(release_id)
    .bind(submission_id)
    .bind(&fixture.app_id)
    .bind(&fixture.version)
    .bind(created)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE releases SET state = 'publishing', resource_version = 2 WHERE release_id = $1",
    )
    .bind(release_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds) \
         VALUES ($1, 'release.publish-requested', 'release', $2, 2, $3, $4, $5)",
    )
    .bind(event_id)
    .bind(release_id)
    .bind(sha256_hex(event_id.as_bytes()))
    .bind(json!({
        "release_id": release_id,
        "app_id": fixture.app_id,
        "version": fixture.version,
        "state": "publishing"
    }))
    .bind(created)
    .execute(pool)
    .await
    .unwrap();
}

async fn write_object(root: &Path, digest: &str, encoded: &[u8]) {
    let directory = root.join("chunks").join(&digest[..2]);
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let path = directory.join(format!("{digest}.chunk"));
    if tokio::fs::symlink_metadata(&path).await.is_err() {
        tokio::fs::write(path, encoded).await.unwrap();
    }
}

async fn assert_release(
    pool: &PgPool,
    release_id: &str,
    state: &str,
    resource_version: i64,
    catalog_sequence: Option<i64>,
) {
    let row = sqlx::query(
        "SELECT state, resource_version, catalog_sequence FROM releases WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("state"), state);
    assert_eq!(row.get::<i64, _>("resource_version"), resource_version);
    assert_eq!(
        row.get::<Option<i64>, _>("catalog_sequence"),
        catalog_sequence
    );
}

async fn verify_database_guards(pool: &PgPool) {
    assert_sqlstate(
        sqlx::query("UPDATE catalog_sequence SET last_sequence = last_sequence + 2")
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("UPDATE store_catalog_snapshots SET app_count = 2 WHERE sequence = 1")
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("DELETE FROM store_package_artifacts WHERE catalog_sequence = 1")
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("DELETE FROM store_publication_jobs WHERE catalog_sequence = 1")
            .execute(pool)
            .await,
        "55000",
    );
}

async fn reset_database(pool: &PgPool) {
    sqlx::query(
        "TRUNCATE teams, reviewers, audit_events, outbox_events, idempotency_records, catalog_sequence \
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

async fn count_where_sequence(pool: &PgPool, sequence: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM store_catalog_snapshots WHERE sequence = $1")
        .bind(sequence)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn last_catalog_sequence(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT last_sequence FROM catalog_sequence WHERE singleton = TRUE")
        .fetch_one(pool)
        .await
        .unwrap()
}

fn test_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-store-publisher")
        .join(Uuid::new_v4().simple().to_string())
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

fn sha256_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
