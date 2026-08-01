use std::env;
use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, ETAG, IF_MATCH};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use cp0_store_control::register_app_request_sha256;
use cp0_store_control_server::{
    MAX_UPLOAD_CHUNK_BYTES, StoreControlService, connect, migrate, router,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPool;
use sqlx::{Executor, Row};
use tower::ServiceExt;
use uuid::Uuid;

const OWNER_A_TOKEN: &str = "owner-a-token-0000000000000000000001";
const OWNER_B_TOKEN: &str = "owner-b-token-0000000000000000000002";
const NO_2FA_TOKEN: &str = "no-2fa-token-000000000000000000003";
const VIEWER_TOKEN: &str = "viewer-token-0000000000000000000004";
const READ_ONLY_TOKEN: &str = "read-only-token-00000000000000000005";
const EXPIRED_TOKEN: &str = "expired-token-0000000000000000000006";
const REVOKED_TOKEN: &str = "revoked-token-0000000000000000000006";
const REVIEWER_A_TOKEN: &str = "reviewer-a-token-0000000000000000001";
const REVIEWER_B_TOKEN: &str = "reviewer-b-token-0000000000000000002";
const REVIEWER_NO_2FA_TOKEN: &str = "reviewer-no2fa-token-000000000000003";
const REVIEWER_EXPIRED_TOKEN: &str = "reviewer-expired-token-0000000000004";
const REVIEWER_REVOKED_TOKEN: &str = "reviewer-revoked-token-0000000000004";
const RELEASE_MANAGER_TOKEN: &str = "release-manager-token-000000000000001";
const RELEASE_NO_2FA_TOKEN: &str = "release-no2fa-token-00000000000000002";
const STALE_MFA_TOKEN: &str = "stale-mfa-token-000000000000000000001";
const EDITOR_TOKEN: &str = "editor-token-000000000000000000000001";
const EDITOR_NO_2FA_TOKEN: &str = "editor-no2fa-token-000000000000000001";
const EDITOR_EXPIRED_TOKEN: &str = "editor-expired-token-00000000000000001";
const EDITOR_REVOKED_TOKEN: &str = "editor-revoked-token-00000000000000001";
const EDITOR_SUSPENDED_TOKEN: &str = "editor-suspended-token-000000000000001";

const TEAM_A: &str = "team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TEAM_B: &str = "team_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OWNER_A: &str = "member_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OWNER_B: &str = "member_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NO_2FA_MEMBER: &str = "member_cccccccccccccccccccccccccccccccc";
const VIEWER_MEMBER: &str = "member_dddddddddddddddddddddddddddddddd";
const RELEASE_MANAGER: &str = "member_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const RELEASE_NO_2FA: &str = "member_ffffffffffffffffffffffffffffffff";
const REVIEWER_A: &str = "reviewer_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const REVIEWER_B: &str = "reviewer_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const REVIEWER_NO_2FA: &str = "reviewer_cccccccccccccccccccccccccccccccc";
const EDITOR: &str = "operator_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EDITOR_NO_2FA: &str = "operator_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const EDITOR_SUSPENDED: &str = "operator_cccccccccccccccccccccccccccccccc";

struct HttpResult {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires CP0_STORE_TEST_DATABASE_URL"]
async fn postgres_http_transaction_acceptance() {
    let database_url = env::var("CP0_STORE_TEST_DATABASE_URL")
        .expect("CP0_STORE_TEST_DATABASE_URL must be set for the database gate");
    let pool = connect(&database_url, 16)
        .await
        .expect("connect test database");
    migrate(&pool).await.expect("run Store migrations");
    reset_database(&pool).await;
    seed_identities(&pool).await;

    let object_root = test_object_root();
    let service = StoreControlService::with_object_root(pool.clone(), &object_root)
        .await
        .expect("create content object store");
    let application = router(service);
    verify_exact_replay(&application, &pool).await;
    verify_authorization_and_limits(&application, &pool).await;
    verify_concurrent_claim(&application, &pool).await;
    verify_atomic_rollback(&application, &pool).await;
    verify_submission_upload(&application, &pool).await;
    verify_concurrent_submission_revisions(&application, &pool).await;
    verify_review_backend(&application, &pool).await;
    verify_release_backend(&application, &pool).await;
    verify_submission_withdrawal(&application, &pool).await;
    verify_oauth_device_flow(&application, &pool).await;
    verify_team_management(&application, &pool).await;
    verify_editorial_backend(&application, &pool).await;
    verify_database_immutability(&pool).await;
    tokio::fs::remove_dir_all(object_root)
        .await
        .expect("remove repository-local test object store");
}

async fn verify_exact_replay(application: &Router, pool: &PgPool) {
    let body = json!({"app_id": "dev.cardputerzero.notes", "default_locale": "en-US"});
    let first = call(
        application.clone(),
        Method::POST,
        "/v1/apps",
        Some(OWNER_A_TOKEN),
        Some("register-notes-0001"),
        Some(body.clone()),
    )
    .await;
    let replay = call(
        application.clone(),
        Method::POST,
        "/v1/apps",
        Some(OWNER_A_TOKEN),
        Some("register-notes-0001"),
        Some(body),
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED);
    assert_eq!(replay.status, StatusCode::CREATED);
    assert_eq!(first.body, replay.body);
    assert_eq!(first.headers.get(ETAG).unwrap(), "\"1\"");
    assert_eq!(replay.headers.get(ETAG).unwrap(), "\"1\"");

    let get = call(
        application.clone(),
        Method::GET,
        "/v1/apps/dev.cardputerzero.notes",
        Some(OWNER_A_TOKEN),
        None,
        None,
    )
    .await;
    assert_eq!(get.status, StatusCode::OK);
    assert_eq!(get.body, first.body);

    let conflict = call(
        application.clone(),
        Method::POST,
        "/v1/apps",
        Some(OWNER_A_TOKEN),
        Some("register-notes-0001"),
        Some(json!({"app_id": "dev.cardputerzero.other", "default_locale": "en"})),
    )
    .await;
    assert_problem(&conflict, StatusCode::CONFLICT, "idempotency-conflict");

    assert_eq!(count(pool, "apps").await, 1);
    assert_eq!(count(pool, "idempotency_records").await, 1);
    assert_eq!(count(pool, "audit_events").await, 1);
    assert_eq!(count(pool, "outbox_events").await, 1);

    let audit =
        sqlx::query("SELECT request_sha256, idempotency_key_sha256, request_id FROM audit_events")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        audit.get::<String, _>("request_sha256"),
        register_app_request_sha256(TEAM_A, "dev.cardputerzero.notes", "en-US")
    );
    assert_eq!(
        audit.get::<String, _>("idempotency_key_sha256"),
        sha256_hex(b"register-notes-0001")
    );
    assert!(audit.get::<String, _>("request_id").starts_with("req_"));
}

async fn verify_authorization_and_limits(application: &Router, pool: &PgPool) {
    for (token, key, code, status) in [
        (
            EXPIRED_TOKEN,
            "expired-request-001",
            "unauthorized",
            StatusCode::UNAUTHORIZED,
        ),
        (
            REVOKED_TOKEN,
            "revoked-request-001",
            "unauthorized",
            StatusCode::UNAUTHORIZED,
        ),
        (
            NO_2FA_TOKEN,
            "no-two-factor-001",
            "two-factor-required",
            StatusCode::FORBIDDEN,
        ),
        (
            VIEWER_TOKEN,
            "viewer-request-001",
            "forbidden",
            StatusCode::FORBIDDEN,
        ),
        (
            READ_ONLY_TOKEN,
            "read-only-request-1",
            "forbidden",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let result = call(
            application.clone(),
            Method::POST,
            "/v1/apps",
            Some(token),
            Some(key),
            Some(json!({"app_id": "dev.cardputerzero.denied", "default_locale": "en"})),
        )
        .await;
        assert_problem(&result, status, code);
    }

    let unknown_field = call(
        application.clone(),
        Method::POST,
        "/v1/apps",
        Some(OWNER_A_TOKEN),
        Some("unknown-field-0001"),
        Some(json!({"app_id": "dev.cardputerzero.bad", "default_locale": "en", "extra": true})),
    )
    .await;
    assert_problem(&unknown_field, StatusCode::BAD_REQUEST, "invalid-request");

    let wrong_method = call(
        application.clone(),
        Method::PUT,
        "/v1/apps",
        Some(OWNER_A_TOKEN),
        Some("wrong-method-0001"),
        Some(json!({"app_id": "dev.cardputerzero.bad", "default_locale": "en"})),
    )
    .await;
    assert_problem(
        &wrong_method,
        StatusCode::METHOD_NOT_ALLOWED,
        "method-not-allowed",
    );

    let oversized_json = format!(
        "{{\"app_id\":\"dev.cardputerzero.large\",\"default_locale\":\"en\",\"padding\":\"{}\"}}",
        "x".repeat(33_000)
    );
    let oversized = call_bytes(
        application.clone(),
        Method::POST,
        "/v1/apps",
        Some(OWNER_A_TOKEN),
        Some("oversized-body-0001"),
        Some(oversized_json.into_bytes()),
    )
    .await;
    assert_problem(
        &oversized,
        StatusCode::PAYLOAD_TOO_LARGE,
        "payload-too-large",
    );

    assert_eq!(count(pool, "apps").await, 1);
    assert_eq!(count(pool, "idempotency_records").await, 1);
}

async fn verify_concurrent_claim(application: &Router, pool: &PgPool) {
    let request_a = call(
        application.clone(),
        Method::POST,
        "/v1/apps",
        Some(OWNER_A_TOKEN),
        Some("concurrent-team-a-001"),
        Some(json!({"app_id": "dev.cardputerzero.concurrent", "default_locale": "en"})),
    );
    let request_b = call(
        application.clone(),
        Method::POST,
        "/v1/apps",
        Some(OWNER_B_TOKEN),
        Some("concurrent-team-b-001"),
        Some(json!({"app_id": "dev.cardputerzero.concurrent", "default_locale": "en"})),
    );
    let (result_a, result_b) = tokio::join!(request_a, request_b);
    let mut statuses = [result_a.status, result_b.status];
    statuses.sort_by_key(|status| status.as_u16());
    assert_eq!(statuses, [StatusCode::CREATED, StatusCode::CONFLICT]);

    let owner: String = sqlx::query_scalar(
        "SELECT owner_team_id FROM apps WHERE app_id = 'dev.cardputerzero.concurrent'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(matches!(owner.as_str(), TEAM_A | TEAM_B));
    assert_eq!(count(pool, "apps").await, 2);
    assert_eq!(count(pool, "idempotency_records").await, 2);
    assert_eq!(count(pool, "audit_events").await, 2);
    assert_eq!(count(pool, "outbox_events").await, 2);
}

async fn verify_atomic_rollback(application: &Router, pool: &PgPool) {
    pool.execute(
        "CREATE FUNCTION test_reject_audit() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN IF NEW.object_id = 'dev.cardputerzero.rollback' THEN \
         RAISE EXCEPTION 'injected audit failure'; END IF; RETURN NEW; END; $$",
    )
    .await
    .unwrap();
    pool.execute(
        "CREATE TRIGGER test_reject_audit_insert BEFORE INSERT ON audit_events \
         FOR EACH ROW EXECUTE FUNCTION test_reject_audit()",
    )
    .await
    .unwrap();

    let result = call(
        application.clone(),
        Method::POST,
        "/v1/apps",
        Some(OWNER_A_TOKEN),
        Some("atomic-rollback-001"),
        Some(json!({"app_id": "dev.cardputerzero.rollback", "default_locale": "en"})),
    )
    .await;
    assert_problem(
        &result,
        StatusCode::SERVICE_UNAVAILABLE,
        "service-unavailable",
    );

    pool.execute("DROP TRIGGER test_reject_audit_insert ON audit_events")
        .await
        .unwrap();
    pool.execute("DROP FUNCTION test_reject_audit()")
        .await
        .unwrap();

    let app_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM apps WHERE app_id = 'dev.cardputerzero.rollback'")
            .fetch_one(pool)
            .await
            .unwrap();
    let key_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM idempotency_records WHERE key_sha256 = $1")
            .bind(sha256_hex(b"atomic-rollback-001"))
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(app_count, 0);
    assert_eq!(key_count, 0);
    assert_eq!(count(pool, "audit_events").await, 2);
    assert_eq!(count(pool, "outbox_events").await, 2);
}

async fn verify_submission_upload(application: &Router, pool: &PgPool) {
    let package = vec![0x55; MAX_UPLOAD_CHUNK_BYTES + 17];
    let listing = br#"{"schema_version":1,"name":"Notes"}"#.to_vec();
    let icon = b"verified-icon-object".to_vec();
    let screenshot = b"verified-screenshot-object".to_vec();
    let package_sha256 = sha256_hex(&package);
    let listing_sha256 = sha256_hex(&listing);
    let icon_sha256 = sha256_hex(&icon);
    let screenshot_sha256 = sha256_hex(&screenshot);
    let assets = vec![
        json!({
            "path": "icon.png",
            "sha256": icon_sha256,
            "bytes": icon.len(),
            "width": 48,
            "height": 48
        }),
        json!({
            "path": "screens/main.png",
            "sha256": screenshot_sha256,
            "bytes": screenshot.len(),
            "width": 320,
            "height": 170
        }),
    ];
    let request_body = json!({
        "version": "1.0.0",
        "package_sha256": package_sha256,
        "package_bytes": package.len(),
        "listing_sha256": listing_sha256,
        "listing_bytes": listing.len(),
        "assets": assets
    });
    let no_two_factor = call(
        application.clone(),
        Method::POST,
        "/v1/apps/dev.cardputerzero.notes/submissions",
        Some(NO_2FA_TOKEN),
        Some("submission-no2fa-1"),
        Some(request_body.clone()),
    )
    .await;
    assert_problem(&no_two_factor, StatusCode::FORBIDDEN, "two-factor-required");
    let wrong_role = call(
        application.clone(),
        Method::POST,
        "/v1/apps/dev.cardputerzero.notes/submissions",
        Some(VIEWER_TOKEN),
        Some("submission-viewer-1"),
        Some(request_body.clone()),
    )
    .await;
    assert_problem(&wrong_role, StatusCode::FORBIDDEN, "forbidden");
    let counts_before = (
        count(pool, "audit_events").await,
        count(pool, "outbox_events").await,
        count(pool, "idempotency_records").await,
    );
    let created = call(
        application.clone(),
        Method::POST,
        "/v1/apps/dev.cardputerzero.notes/submissions",
        Some(OWNER_A_TOKEN),
        Some("create-submission-001"),
        Some(request_body.clone()),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(etag_version(&created), 1);
    let created_body: Value = serde_json::from_slice(&created.body).unwrap();
    let submission_id = created_body["submission_id"].as_str().unwrap().to_owned();
    assert!(submission_id.starts_with("sub_"));
    assert_eq!(created_body["state"], "uploading");
    assert!(created_body.get("package_bytes").is_none());
    assert!(created_body.get("listing_bytes").is_none());

    let replay = call(
        application.clone(),
        Method::POST,
        "/v1/apps/dev.cardputerzero.notes/submissions",
        Some(OWNER_A_TOKEN),
        Some("create-submission-001"),
        Some(request_body),
    )
    .await;
    assert_eq!(replay.status, StatusCode::CREATED);
    assert_eq!(created.body, replay.body);

    let first_chunk = &package[..MAX_UPLOAD_CHUNK_BYTES];
    let first = upload(
        application.clone(),
        &submission_id,
        "package",
        1,
        0,
        package.len(),
        first_chunk,
        &sha256_hex(first_chunk),
        "upload-package-0001",
    )
    .await;
    assert_eq!(first.status, StatusCode::NO_CONTENT);
    assert_eq!(etag_version(&first), 2);
    let first_replay = upload(
        application.clone(),
        &submission_id,
        "package",
        1,
        0,
        package.len(),
        first_chunk,
        &sha256_hex(first_chunk),
        "upload-package-0001",
    )
    .await;
    assert_eq!(first_replay.status, StatusCode::NO_CONTENT);
    assert_eq!(etag_version(&first_replay), 2);

    let tail = &package[MAX_UPLOAD_CHUNK_BYTES..];
    let stale = upload(
        application.clone(),
        &submission_id,
        "package",
        1,
        MAX_UPLOAD_CHUNK_BYTES,
        package.len(),
        tail,
        &sha256_hex(tail),
        "stale-package-0001",
    )
    .await;
    assert_problem(
        &stale,
        StatusCode::PRECONDITION_FAILED,
        "precondition-failed",
    );
    let non_contiguous = upload(
        application.clone(),
        &submission_id,
        "package",
        2,
        1,
        package.len(),
        tail,
        &sha256_hex(tail),
        "range-package-0001",
    )
    .await;
    assert_problem(
        &non_contiguous,
        StatusCode::CONFLICT,
        "upload-range-conflict",
    );
    let wrong_digest = upload(
        application.clone(),
        &submission_id,
        "package",
        2,
        MAX_UPLOAD_CHUNK_BYTES,
        package.len(),
        tail,
        &"0".repeat(64),
        "digest-package-001",
    )
    .await;
    assert_problem(
        &wrong_digest,
        StatusCode::UNPROCESSABLE_ENTITY,
        "digest-mismatch",
    );

    let second = upload(
        application.clone(),
        &submission_id,
        "package",
        2,
        MAX_UPLOAD_CHUNK_BYTES,
        package.len(),
        tail,
        &sha256_hex(tail),
        "upload-package-0002",
    )
    .await;
    assert_eq!(etag_version(&second), 3);
    let listing_result = upload(
        application.clone(),
        &submission_id,
        "listing",
        3,
        0,
        listing.len(),
        &listing,
        &listing_sha256,
        "upload-listing-001",
    )
    .await;
    assert_eq!(etag_version(&listing_result), 4);
    let icon_result = upload(
        application.clone(),
        &submission_id,
        "asset-0",
        4,
        0,
        icon.len(),
        &icon,
        &icon_sha256,
        "upload-asset-0001",
    )
    .await;
    assert_eq!(etag_version(&icon_result), 5);
    let screenshot_result = upload(
        application.clone(),
        &submission_id,
        "asset-1",
        5,
        0,
        screenshot.len(),
        &screenshot,
        &screenshot_sha256,
        "upload-asset-0002",
    )
    .await;
    assert_eq!(etag_version(&screenshot_result), 6);

    let invalid_finalize = finalize(
        application.clone(),
        &submission_id,
        6,
        &"0".repeat(64),
        "finalize-invalid-01",
    )
    .await;
    assert_problem(
        &invalid_finalize,
        StatusCode::UNPROCESSABLE_ENTITY,
        "digest-mismatch",
    );
    let content_sha256 =
        submission_content_sha256_for_test(&package_sha256, &listing_sha256, &assets);
    let finalized = finalize(
        application.clone(),
        &submission_id,
        6,
        &content_sha256,
        "finalize-submission-1",
    )
    .await;
    assert_eq!(finalized.status, StatusCode::ACCEPTED);
    assert_eq!(etag_version(&finalized), 7);
    let finalized_body: Value = serde_json::from_slice(&finalized.body).unwrap();
    assert_eq!(finalized_body["state"], "processing");
    let finalize_replay = finalize(
        application.clone(),
        &submission_id,
        6,
        &content_sha256,
        "finalize-submission-1",
    )
    .await;
    assert_eq!(finalize_replay.status, StatusCode::ACCEPTED);
    assert_eq!(finalize_replay.body, finalized.body);

    let fetched = call(
        application.clone(),
        Method::GET,
        &format!("/v1/submissions/{submission_id}"),
        Some(OWNER_A_TOKEN),
        None,
        None,
    )
    .await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.body, finalized.body);
    assert_eq!(etag_version(&fetched), 7);
    let cross_team = call(
        application.clone(),
        Method::GET,
        &format!("/v1/submissions/{submission_id}"),
        Some(OWNER_B_TOKEN),
        None,
        None,
    )
    .await;
    assert_problem(&cross_team, StatusCode::NOT_FOUND, "not-found");

    let stored_digest: String = sqlx::query_scalar(
        "SELECT finalized_content_sha256 FROM submissions WHERE submission_id = $1",
    )
    .bind(&submission_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(stored_digest, content_sha256);
    assert_eq!(count(pool, "submission_upload_parts").await, 4);
    assert_eq!(count(pool, "submission_upload_chunks").await, 5);
    assert_eq!(count(pool, "audit_events").await, counts_before.0 + 7);
    assert_eq!(count(pool, "outbox_events").await, counts_before.1 + 7);
    assert_eq!(
        count(pool, "idempotency_records").await,
        counts_before.2 + 7
    );
}

async fn verify_concurrent_submission_revisions(application: &Router, pool: &PgPool) {
    let request = json!({
        "version": "2.0.0",
        "package_sha256": "1".repeat(64),
        "package_bytes": 100,
        "listing_sha256": "2".repeat(64),
        "listing_bytes": 100,
        "assets": [
            {
                "path": "icon.png",
                "sha256": "3".repeat(64),
                "bytes": 100,
                "width": 48,
                "height": 48
            },
            {
                "path": "screens/main.png",
                "sha256": "4".repeat(64),
                "bytes": 100,
                "width": 320,
                "height": 170
            }
        ]
    });
    let first = call(
        application.clone(),
        Method::POST,
        "/v1/apps/dev.cardputerzero.notes/submissions",
        Some(OWNER_A_TOKEN),
        Some("concurrent-revision-1"),
        Some(request.clone()),
    );
    let second = call(
        application.clone(),
        Method::POST,
        "/v1/apps/dev.cardputerzero.notes/submissions",
        Some(OWNER_A_TOKEN),
        Some("concurrent-revision-2"),
        Some(request),
    );
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first.status, StatusCode::CREATED);
    assert_eq!(second.status, StatusCode::CREATED);
    let mut revisions = [
        serde_json::from_slice::<Value>(&first.body).unwrap()["revision"]
            .as_u64()
            .unwrap(),
        serde_json::from_slice::<Value>(&second.body).unwrap()["revision"]
            .as_u64()
            .unwrap(),
    ];
    revisions.sort_unstable();
    assert_eq!(revisions, [1, 2]);
    let revision_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM submissions \
         WHERE app_id = 'dev.cardputerzero.notes' AND version = '2.0.0'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(revision_count, 2);
}

async fn verify_review_backend(application: &Router, pool: &PgPool) {
    const SUBMISSION_A: &str = "sub_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SUBMISSION_B: &str = "sub_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SUBMISSION_C: &str = "sub_cccccccccccccccccccccccccccccccc";
    for (submission_id, version, created) in [
        (SUBMISSION_A, "3.0.0", 100_i64),
        (SUBMISSION_B, "3.0.1", 101_i64),
        (SUBMISSION_C, "3.0.2", 102_i64),
    ] {
        seed_review_submission(pool, submission_id, version, created).await;
    }

    for (token, status, code) in [
        (OWNER_A_TOKEN, StatusCode::UNAUTHORIZED, "unauthorized"),
        (
            REVIEWER_NO_2FA_TOKEN,
            StatusCode::FORBIDDEN,
            "two-factor-required",
        ),
        (
            REVIEWER_EXPIRED_TOKEN,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
        (
            REVIEWER_REVOKED_TOKEN,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
    ] {
        let result = call(
            application.clone(),
            Method::GET,
            "/v1/review/submissions",
            Some(token),
            None,
            None,
        )
        .await;
        assert_problem(&result, status, code);
    }
    let invalid_cursor = call(
        application.clone(),
        Method::GET,
        "/v1/review/submissions?cursor=not-a-cursor",
        Some(REVIEWER_A_TOKEN),
        None,
        None,
    )
    .await;
    assert_problem(&invalid_cursor, StatusCode::BAD_REQUEST, "invalid-request");

    let first_page = call(
        application.clone(),
        Method::GET,
        "/v1/review/submissions?limit=2",
        Some(REVIEWER_A_TOKEN),
        None,
        None,
    )
    .await;
    assert_eq!(first_page.status, StatusCode::OK);
    let first_page: Value = serde_json::from_slice(&first_page.body).unwrap();
    assert_eq!(first_page["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        first_page["items"][0]["submission"]["submission_id"],
        SUBMISSION_A
    );
    assert_eq!(first_page["items"][0]["review_stage"], "primary");
    assert_eq!(first_page["items"][0]["assigned_to_caller"], false);
    assert_eq!(first_page["items"][0]["risk"]["policy_version"], 1);
    assert_eq!(first_page["items"][0]["risk"]["tier"], "standard");
    assert_eq!(first_page["items"][0]["risk"]["reasons"], json!([]));
    assert_eq!(
        first_page["items"][1]["submission"]["submission_id"],
        SUBMISSION_B
    );
    let cursor = first_page["next_cursor"].as_str().unwrap();
    let second_page = call(
        application.clone(),
        Method::GET,
        &format!("/v1/review/submissions?limit=2&cursor={cursor}"),
        Some(REVIEWER_A_TOKEN),
        None,
        None,
    )
    .await;
    assert_eq!(second_page.status, StatusCode::OK);
    let second_page: Value = serde_json::from_slice(&second_page.body).unwrap();
    assert_eq!(second_page["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        second_page["items"][0]["submission"]["submission_id"],
        SUBMISSION_C
    );
    assert!(second_page["next_cursor"].is_null());

    let missing_etag = call(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_C}:begin"),
        Some(REVIEWER_A_TOKEN),
        Some("begin-without-etag1"),
        None,
    )
    .await;
    assert_problem(&missing_etag, StatusCode::BAD_REQUEST, "invalid-request");
    let invalid_action = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_C}:begin-extra"),
        REVIEWER_A_TOKEN,
        "invalid-begin-path1",
        1,
        None,
    )
    .await;
    assert_problem(&invalid_action, StatusCode::BAD_REQUEST, "invalid-request");

    let counts_before = (
        count(pool, "audit_events").await,
        count(pool, "outbox_events").await,
        count(pool, "idempotency_records").await,
    );
    let begun = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_A}:begin"),
        REVIEWER_A_TOKEN,
        "begin-review-a-001",
        1,
        None,
    )
    .await;
    assert_eq!(begun.status, StatusCode::OK);
    assert_eq!(etag_version(&begun), 2);
    assert_eq!(
        serde_json::from_slice::<Value>(&begun.body).unwrap()["state"],
        "in-review"
    );
    let replay = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_A}:begin"),
        REVIEWER_A_TOKEN,
        "begin-review-a-001",
        1,
        None,
    )
    .await;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(replay.body, begun.body);
    let assigned_queue = call(
        application.clone(),
        Method::GET,
        "/v1/review/submissions",
        Some(REVIEWER_A_TOKEN),
        None,
        None,
    )
    .await;
    let assigned_queue: Value = serde_json::from_slice(&assigned_queue.body).unwrap();
    let assigned_item = assigned_queue["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["submission"]["submission_id"] == SUBMISSION_A)
        .unwrap();
    assert_eq!(assigned_item["review_stage"], "primary");
    assert_eq!(assigned_item["assigned_to_caller"], true);

    let concurrent_begin_uri = format!("/v1/review/submissions/{SUBMISSION_B}:begin");
    let claim_a = call_with_etag(
        application.clone(),
        Method::POST,
        &concurrent_begin_uri,
        REVIEWER_A_TOKEN,
        "begin-review-b-a01",
        1,
        None,
    );
    let claim_b = call_with_etag(
        application.clone(),
        Method::POST,
        &concurrent_begin_uri,
        REVIEWER_B_TOKEN,
        "begin-review-b-b01",
        1,
        None,
    );
    let (claim_a, claim_b) = tokio::join!(claim_a, claim_b);
    let mut claim_statuses = [claim_a.status, claim_b.status];
    claim_statuses.sort_by_key(|status| status.as_u16());
    assert_eq!(
        claim_statuses,
        [StatusCode::OK, StatusCode::PRECONDITION_FAILED]
    );
    let assigned_b: String = sqlx::query_scalar(
        "SELECT reviewer_id FROM review_assignments WHERE submission_id = $1 AND state = 'active'",
    )
    .bind(SUBMISSION_B)
    .fetch_one(pool)
    .await
    .unwrap();
    let assigned_b_token = if assigned_b == REVIEWER_A {
        REVIEWER_A_TOKEN
    } else {
        REVIEWER_B_TOKEN
    };

    let foreign_decision = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_A}/decisions"),
        REVIEWER_B_TOKEN,
        "foreign-decision-01",
        2,
        Some(json!({"decision": "approved", "reason_codes": [], "note": ""})),
    )
    .await;
    assert_problem(&foreign_decision, StatusCode::FORBIDDEN, "forbidden");
    let invalid_decision = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_A}/decisions"),
        REVIEWER_A_TOKEN,
        "invalid-decision-01",
        2,
        Some(json!({
            "decision": "needs-changes",
            "reason_codes": ["privacy", "privacy"],
            "note": "Explain data retention."
        })),
    )
    .await;
    assert_problem(
        &invalid_decision,
        StatusCode::BAD_REQUEST,
        "invalid-request",
    );
    let decision_body = json!({
        "decision": "needs-changes",
        "reason_codes": ["privacy-disclosure"],
        "note": "Explain data retention and deletion in the privacy statement."
    });
    let decided = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_A}/decisions"),
        REVIEWER_A_TOKEN,
        "decide-review-a-001",
        2,
        Some(decision_body.clone()),
    )
    .await;
    assert_eq!(decided.status, StatusCode::CREATED);
    assert_eq!(etag_version(&decided), 3);
    assert_eq!(
        serde_json::from_slice::<Value>(&decided.body).unwrap()["state"],
        "needs-changes"
    );
    let decision_replay = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_A}/decisions"),
        REVIEWER_A_TOKEN,
        "decide-review-a-001",
        2,
        Some(decision_body),
    )
    .await;
    assert_eq!(decision_replay.status, StatusCode::CREATED);
    assert_eq!(decision_replay.body, decided.body);

    let approved = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_B}/decisions"),
        assigned_b_token,
        "decide-review-b-001",
        2,
        Some(json!({"decision": "approved", "reason_codes": [], "note": ""})),
    )
    .await;
    assert_eq!(approved.status, StatusCode::CREATED);
    assert_eq!(etag_version(&approved), 3);
    assert_eq!(
        serde_json::from_slice::<Value>(&approved.body).unwrap()["state"],
        "pending-secondary-review"
    );
    let primary_reviewer_token = assigned_b_token;
    let secondary_reviewer_token = if assigned_b == REVIEWER_A {
        REVIEWER_B_TOKEN
    } else {
        REVIEWER_A_TOKEN
    };
    let same_reviewer_secondary = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_B}:begin"),
        primary_reviewer_token,
        "begin-secondary-self1",
        3,
        None,
    )
    .await;
    assert_problem(&same_reviewer_secondary, StatusCode::FORBIDDEN, "forbidden");
    let secondary_queue = call(
        application.clone(),
        Method::GET,
        "/v1/review/submissions",
        Some(secondary_reviewer_token),
        None,
        None,
    )
    .await;
    assert_eq!(secondary_queue.status, StatusCode::OK);
    assert!(
        serde_json::from_slice::<Value>(&secondary_queue.body).unwrap()["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["submission"]["submission_id"] == SUBMISSION_B
                    && item["review_stage"] == "secondary"
                    && item["assigned_to_caller"] == false
            })
    );
    let secondary_begun = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_B}:begin"),
        secondary_reviewer_token,
        "begin-secondary-b001",
        3,
        None,
    )
    .await;
    assert_eq!(secondary_begun.status, StatusCode::OK);
    assert_eq!(etag_version(&secondary_begun), 4);
    pool.execute(
        "CREATE FUNCTION test_reject_secondary_review_audit() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN IF NEW.action = 'submission.review-decided' AND \
         NEW.object_id = 'sub_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' AND \
         NEW.resource_version = 5 THEN \
         RAISE EXCEPTION 'injected secondary review audit failure'; END IF; RETURN NEW; END; $$",
    )
    .await
    .unwrap();
    pool.execute(
        "CREATE TRIGGER test_reject_secondary_review_audit_insert BEFORE INSERT ON audit_events \
         FOR EACH ROW EXECUTE FUNCTION test_reject_secondary_review_audit()",
    )
    .await
    .unwrap();
    let rolled_back_secondary = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_B}/decisions"),
        secondary_reviewer_token,
        "decide-secondary-b01",
        4,
        Some(json!({"decision": "approved", "reason_codes": [], "note": ""})),
    )
    .await;
    assert_problem(
        &rolled_back_secondary,
        StatusCode::SERVICE_UNAVAILABLE,
        "service-unavailable",
    );
    let secondary_rollback = sqlx::query(
        "SELECT submission.state, submission.resource_version, assignment.state AS assignment_state, \
         (SELECT COUNT(*) FROM review_decisions decision \
          WHERE decision.assignment_id = assignment.assignment_id) AS decision_count \
         FROM submissions submission JOIN review_assignments assignment \
           ON assignment.submission_id = submission.submission_id \
          AND assignment.assignment_kind = 'secondary' \
         WHERE submission.submission_id = $1",
    )
    .bind(SUBMISSION_B)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(secondary_rollback.get::<String, _>("state"), "in-review");
    assert_eq!(secondary_rollback.get::<i64, _>("resource_version"), 4);
    assert_eq!(
        secondary_rollback.get::<String, _>("assignment_state"),
        "active"
    );
    assert_eq!(secondary_rollback.get::<i64, _>("decision_count"), 0);
    pool.execute("DROP TRIGGER test_reject_secondary_review_audit_insert ON audit_events")
        .await
        .unwrap();
    pool.execute("DROP FUNCTION test_reject_secondary_review_audit()")
        .await
        .unwrap();
    let double_approved = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_B}/decisions"),
        secondary_reviewer_token,
        "decide-secondary-b01",
        4,
        Some(json!({"decision": "approved", "reason_codes": [], "note": ""})),
    )
    .await;
    assert_eq!(double_approved.status, StatusCode::CREATED);
    assert_eq!(etag_version(&double_approved), 5);
    assert_eq!(
        serde_json::from_slice::<Value>(&double_approved.body).unwrap()["state"],
        "approved"
    );
    let double_approved_replay = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/review/submissions/{SUBMISSION_B}/decisions"),
        secondary_reviewer_token,
        "decide-secondary-b01",
        4,
        Some(json!({"decision": "approved", "reason_codes": [], "note": ""})),
    )
    .await;
    assert_eq!(double_approved_replay.status, StatusCode::CREATED);
    assert_eq!(double_approved_replay.body, double_approved.body);

    let unassigned_message = call(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{SUBMISSION_C}/messages"),
        Some(REVIEWER_A_TOKEN),
        Some("unassigned-message-1"),
        Some(json!({"body": "Unassigned review note"})),
    )
    .await;
    assert_problem(&unassigned_message, StatusCode::FORBIDDEN, "forbidden");
    let cross_team_message = call(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{SUBMISSION_C}/messages"),
        Some(OWNER_B_TOKEN),
        Some("cross-team-message1"),
        Some(json!({"body": "Cross-team message"})),
    )
    .await;
    assert_problem(&cross_team_message, StatusCode::NOT_FOUND, "not-found");
    let developer_message = call(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{SUBMISSION_C}/messages"),
        Some(OWNER_A_TOKEN),
        Some("developer-message-1"),
        Some(json!({"body": "The privacy statement is ready for review."})),
    )
    .await;
    assert_eq!(developer_message.status, StatusCode::CREATED);
    let developer_message_body: Value = serde_json::from_slice(&developer_message.body).unwrap();
    assert_eq!(developer_message_body["actor_id"], OWNER_A);
    let developer_message_replay = call(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{SUBMISSION_C}/messages"),
        Some(OWNER_A_TOKEN),
        Some("developer-message-1"),
        Some(json!({"body": "The privacy statement is ready for review."})),
    )
    .await;
    assert_eq!(developer_message_replay.body, developer_message.body);
    sqlx::query(
        "UPDATE team_members SET two_factor_enabled = FALSE, \
         resource_version = resource_version + 1 WHERE member_id = $1",
    )
    .bind(OWNER_A)
    .execute(pool)
    .await
    .unwrap();
    let replay_after_downgrade = call(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{SUBMISSION_C}/messages"),
        Some(OWNER_A_TOKEN),
        Some("developer-message-1"),
        Some(json!({"body": "The privacy statement is ready for review."})),
    )
    .await;
    assert_problem(
        &replay_after_downgrade,
        StatusCode::FORBIDDEN,
        "two-factor-required",
    );
    sqlx::query(
        "UPDATE team_members SET two_factor_enabled = TRUE, \
         resource_version = resource_version + 1 WHERE member_id = $1",
    )
    .bind(OWNER_A)
    .execute(pool)
    .await
    .unwrap();

    let reviewer_message = call(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{SUBMISSION_A}/messages"),
        Some(REVIEWER_A_TOKEN),
        Some("reviewer-message-001"),
        Some(json!({"body": "Please address the structured privacy finding."})),
    )
    .await;
    assert_eq!(reviewer_message.status, StatusCode::CREATED);
    let reviewer_message_body: Value = serde_json::from_slice(&reviewer_message.body).unwrap();
    assert_eq!(reviewer_message_body["actor_id"], REVIEWER_A);

    sqlx::query("UPDATE reviewer_access_tokens SET revoked = TRUE WHERE token_sha256 = $1")
        .bind(sha256_hex(REVIEWER_B_TOKEN.as_bytes()))
        .execute(pool)
        .await
        .unwrap();
    let revoked_live = call(
        application.clone(),
        Method::GET,
        "/v1/review/submissions",
        Some(REVIEWER_B_TOKEN),
        None,
        None,
    )
    .await;
    assert_problem(&revoked_live, StatusCode::UNAUTHORIZED, "unauthorized");

    assert_eq!(count(pool, "review_assignments").await, 3);
    assert_eq!(count(pool, "review_decisions").await, 3);
    assert_eq!(count(pool, "review_messages").await, 2);
    assert_eq!(count(pool, "audit_events").await, counts_before.0 + 8);
    assert_eq!(count(pool, "outbox_events").await, counts_before.1 + 8);
    assert_eq!(
        count(pool, "idempotency_records").await,
        counts_before.2 + 8
    );
}

async fn verify_release_backend(application: &Router, pool: &PgPool) {
    const APPROVED_SUBMISSION: &str = "sub_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const UNAPPROVED_SUBMISSION: &str = "sub_cccccccccccccccccccccccccccccccc";
    const CONCURRENT_SUBMISSION: &str = "sub_dddddddddddddddddddddddddddddddd";
    let counts_before = (
        count(pool, "audit_events").await,
        count(pool, "outbox_events").await,
        count(pool, "idempotency_records").await,
    );
    let request = json!({"submission_id": APPROVED_SUBMISSION, "rollout_percent": 25});

    for (token, key, status, code) in [
        (
            REVIEWER_A_TOKEN,
            "release-reviewer-001",
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
        (
            RELEASE_NO_2FA_TOKEN,
            "release-no2fa-0001",
            StatusCode::FORBIDDEN,
            "two-factor-required",
        ),
        (
            VIEWER_TOKEN,
            "release-viewer-0001",
            StatusCode::FORBIDDEN,
            "forbidden",
        ),
    ] {
        let result = call(
            application.clone(),
            Method::POST,
            "/v1/releases",
            Some(token),
            Some(key),
            Some(request.clone()),
        )
        .await;
        assert_problem(&result, status, code);
    }
    let cross_team = call(
        application.clone(),
        Method::POST,
        "/v1/releases",
        Some(OWNER_B_TOKEN),
        Some("release-cross-team1"),
        Some(request.clone()),
    )
    .await;
    assert_problem(&cross_team, StatusCode::NOT_FOUND, "not-found");
    let unapproved = call(
        application.clone(),
        Method::POST,
        "/v1/releases",
        Some(OWNER_A_TOKEN),
        Some("release-unapproved1"),
        Some(json!({
            "submission_id": UNAPPROVED_SUBMISSION,
            "rollout_percent": 25
        })),
    )
    .await;
    assert_problem(&unapproved, StatusCode::CONFLICT, "invalid-transition");
    let invalid_rollout = call(
        application.clone(),
        Method::POST,
        "/v1/releases",
        Some(OWNER_A_TOKEN),
        Some("release-rollout-0001"),
        Some(json!({
            "submission_id": APPROVED_SUBMISSION,
            "rollout_percent": 0
        })),
    )
    .await;
    assert_problem(&invalid_rollout, StatusCode::BAD_REQUEST, "invalid-request");

    let created = call(
        application.clone(),
        Method::POST,
        "/v1/releases",
        Some(RELEASE_MANAGER_TOKEN),
        Some("release-create-0001"),
        Some(request.clone()),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(etag_version(&created), 1);
    let created_body: Value = serde_json::from_slice(&created.body).unwrap();
    let release_id = created_body["release_id"].as_str().unwrap();
    assert!(release_id.starts_with("rel_"));
    assert_eq!(created_body["state"], "ready");
    assert_eq!(created_body["rollout_percent"], 25);
    assert!(created_body["catalog_sequence"].is_null());
    let replay = call(
        application.clone(),
        Method::POST,
        "/v1/releases",
        Some(RELEASE_MANAGER_TOKEN),
        Some("release-create-0001"),
        Some(request),
    )
    .await;
    assert_eq!(replay.status, StatusCode::CREATED);
    assert_eq!(replay.body, created.body);

    let get = call(
        application.clone(),
        Method::GET,
        &format!("/v1/releases/{release_id}"),
        Some(OWNER_A_TOKEN),
        None,
        None,
    )
    .await;
    assert_eq!(get.status, StatusCode::OK);
    assert_eq!(get.body, created.body);
    let hidden = call(
        application.clone(),
        Method::GET,
        &format!("/v1/releases/{release_id}"),
        Some(OWNER_B_TOKEN),
        None,
        None,
    )
    .await;
    assert_problem(&hidden, StatusCode::NOT_FOUND, "not-found");
    let scope_denied = call(
        application.clone(),
        Method::GET,
        &format!("/v1/releases/{release_id}"),
        Some(READ_ONLY_TOKEN),
        None,
        None,
    )
    .await;
    assert_problem(&scope_denied, StatusCode::FORBIDDEN, "forbidden");

    seed_double_approved_submission(pool, CONCURRENT_SUBMISSION, "3.0.3", 103).await;
    let concurrent_body = json!({"submission_id": CONCURRENT_SUBMISSION, "rollout_percent": 100});
    let create_a = call(
        application.clone(),
        Method::POST,
        "/v1/releases",
        Some(OWNER_A_TOKEN),
        Some("release-concurrent-a1"),
        Some(concurrent_body.clone()),
    );
    let create_b = call(
        application.clone(),
        Method::POST,
        "/v1/releases",
        Some(RELEASE_MANAGER_TOKEN),
        Some("release-concurrent-b1"),
        Some(concurrent_body),
    );
    let (create_a, create_b) = tokio::join!(create_a, create_b);
    let mut statuses = [create_a.status, create_b.status];
    statuses.sort_by_key(|status| status.as_u16());
    assert_eq!(statuses, [StatusCode::CREATED, StatusCode::CONFLICT]);
    let concurrent_release = if create_a.status == StatusCode::CREATED {
        serde_json::from_slice::<Value>(&create_a.body).unwrap()
    } else {
        serde_json::from_slice::<Value>(&create_b.body).unwrap()
    };
    let concurrent_release_id = concurrent_release["release_id"].as_str().unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM releases WHERE submission_id = $1",)
            .bind(CONCURRENT_SUBMISSION)
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );

    let now: i64 = sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(pool)
        .await
        .unwrap();
    let missing_schedule_body = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:schedule"),
        RELEASE_MANAGER_TOKEN,
        "release-schedule-empty1",
        1,
        None,
    )
    .await;
    assert_problem(
        &missing_schedule_body,
        StatusCode::BAD_REQUEST,
        "invalid-request",
    );
    let past_schedule = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:schedule"),
        RELEASE_MANAGER_TOKEN,
        "release-schedule-past1",
        1,
        Some(json!({"publish_unix_seconds": now - 1})),
    )
    .await;
    assert_problem(&past_schedule, StatusCode::BAD_REQUEST, "invalid-request");
    let publish_with_body = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:publish"),
        RELEASE_MANAGER_TOKEN,
        "release-publish-body1",
        1,
        Some(json!({})),
    )
    .await;
    assert_problem(
        &publish_with_body,
        StatusCode::BAD_REQUEST,
        "invalid-request",
    );
    let scheduled_at = u64::try_from(now + 3600).unwrap();
    let schedule_body = json!({"publish_unix_seconds": scheduled_at});
    let scheduled = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:schedule"),
        RELEASE_MANAGER_TOKEN,
        "release-schedule-0001",
        1,
        Some(schedule_body.clone()),
    )
    .await;
    assert_eq!(scheduled.status, StatusCode::OK);
    assert_eq!(etag_version(&scheduled), 2);
    let scheduled_body: Value = serde_json::from_slice(&scheduled.body).unwrap();
    assert_eq!(scheduled_body["state"], "scheduled");
    assert_eq!(scheduled_body["scheduled_unix_seconds"], scheduled_at);
    let schedule_replay = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:schedule"),
        RELEASE_MANAGER_TOKEN,
        "release-schedule-0001",
        1,
        Some(schedule_body),
    )
    .await;
    assert_eq!(schedule_replay.body, scheduled.body);

    let stale_publish = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:publish"),
        RELEASE_MANAGER_TOKEN,
        "release-publish-stale1",
        1,
        None,
    )
    .await;
    assert_problem(
        &stale_publish,
        StatusCode::PRECONDITION_FAILED,
        "precondition-failed",
    );
    let published_request = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:publish"),
        RELEASE_MANAGER_TOKEN,
        "release-publish-0001",
        2,
        None,
    )
    .await;
    assert_eq!(published_request.status, StatusCode::ACCEPTED);
    assert_eq!(etag_version(&published_request), 3);
    let publishing_body: Value = serde_json::from_slice(&published_request.body).unwrap();
    assert_eq!(publishing_body["state"], "publishing");
    assert!(publishing_body["catalog_sequence"].is_null());
    assert!(publishing_body["scheduled_unix_seconds"].is_null());
    let queued_topic: String = sqlx::query_scalar(
        "SELECT topic FROM outbox_events WHERE aggregate_id = $1 AND aggregate_version = 3",
    )
    .bind(release_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(queued_topic, "release.publish-requested");
    let early_pause = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:pause"),
        RELEASE_MANAGER_TOKEN,
        "release-pause-early1",
        3,
        None,
    )
    .await;
    assert_problem(&early_pause, StatusCode::CONFLICT, "invalid-transition");

    sqlx::query(
        "UPDATE releases SET state = 'published', catalog_sequence = 1, resource_version = 4 \
         WHERE release_id = $1",
    )
    .bind(release_id)
    .execute(pool)
    .await
    .unwrap();
    let paused = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:pause"),
        OWNER_A_TOKEN,
        "release-pause-0001",
        4,
        None,
    )
    .await;
    assert_eq!(paused.status, StatusCode::OK);
    assert_eq!(etag_version(&paused), 5);
    assert_eq!(
        serde_json::from_slice::<Value>(&paused.body).unwrap()["state"],
        "paused"
    );
    let resumed = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:resume"),
        OWNER_A_TOKEN,
        "release-resume-0001",
        5,
        None,
    )
    .await;
    assert_eq!(resumed.status, StatusCode::OK);
    assert_eq!(etag_version(&resumed), 6);
    assert_eq!(
        serde_json::from_slice::<Value>(&resumed.body).unwrap()["state"],
        "published"
    );
    let removal_body = json!({
        "reason_code": "security-response",
        "note": "Remove this version while the security issue is investigated."
    });
    let removed = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:remove"),
        OWNER_A_TOKEN,
        "release-remove-0001",
        6,
        Some(removal_body.clone()),
    )
    .await;
    assert_eq!(removed.status, StatusCode::OK);
    assert_eq!(etag_version(&removed), 7);
    let removed_body: Value = serde_json::from_slice(&removed.body).unwrap();
    assert_eq!(removed_body["state"], "removed");
    assert_eq!(removed_body["catalog_sequence"], 1);
    let remove_replay = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{release_id}:remove"),
        OWNER_A_TOKEN,
        "release-remove-0001",
        6,
        Some(removal_body),
    )
    .await;
    assert_eq!(remove_replay.body, removed.body);
    let removal_details: Value = sqlx::query_scalar(
        "SELECT details FROM release_operations WHERE release_id = $1 AND action = 'remove'",
    )
    .bind(release_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(removal_details["reason_code"], "security-response");
    assert!(
        removal_details["note"]
            .as_str()
            .unwrap()
            .contains("security")
    );
    let removal_payload: Value = sqlx::query_scalar(
        "SELECT payload FROM outbox_events WHERE aggregate_id = $1 AND aggregate_version = 7",
    )
    .bind(release_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(removal_payload["reason_code"], "security-response");
    assert!(removal_payload.get("note").is_none());

    let first_publish = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{concurrent_release_id}:publish"),
        OWNER_A_TOKEN,
        "release-retry-first1",
        1,
        None,
    )
    .await;
    assert_eq!(first_publish.status, StatusCode::ACCEPTED);
    sqlx::query(
        "UPDATE releases SET state = 'publish-failed', resource_version = 3 \
         WHERE release_id = $1",
    )
    .bind(concurrent_release_id)
    .execute(pool)
    .await
    .unwrap();
    let retried_publish = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/releases/{concurrent_release_id}:publish"),
        OWNER_A_TOKEN,
        "release-retry-second1",
        3,
        None,
    )
    .await;
    assert_eq!(retried_publish.status, StatusCode::ACCEPTED);
    assert_eq!(etag_version(&retried_publish), 4);
    assert_eq!(
        serde_json::from_slice::<Value>(&retried_publish.body).unwrap()["state"],
        "publishing"
    );

    assert_sqlstate(
        sqlx::query(
            "UPDATE releases SET state = 'paused', catalog_sequence = 2, resource_version = 5 \
             WHERE release_id = $1",
        )
        .bind(concurrent_release_id)
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "INSERT INTO releases (release_id, submission_id, app_id, version, state, \
             rollout_percent, resource_version, created_unix_seconds) VALUES \
             ('rel_99999999999999999999999999999999', $1, 'dev.cardputerzero.notes', \
             '3.0.2', 'ready', 100, 1, 1)",
        )
        .bind(UNAPPROVED_SUBMISSION)
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "INSERT INTO release_operations (operation_id, release_id, actor_id, action, \
             before_state, after_state, resource_version, request_sha256, details, \
             created_unix_seconds) VALUES \
             ('releaseop_99999999999999999999999999999999', $1, $2, 'pause', \
             'published', 'paused', 7, $3, '{}', 1)",
        )
        .bind(release_id)
        .bind(OWNER_A)
        .bind("9".repeat(64))
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "INSERT INTO release_operations (operation_id, release_id, actor_id, action, \
             before_state, after_state, resource_version, request_sha256, details, \
             created_unix_seconds) VALUES \
             ('releaseop_88888888888888888888888888888888', $1, $2, 'publish', \
             'publish-failed', 'publishing', 4, $3, '{\"unexpected\":true}', 1)",
        )
        .bind(concurrent_release_id)
        .bind(OWNER_A)
        .bind("8".repeat(64))
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("UPDATE release_operations SET details = '{}' WHERE release_id = $1")
            .bind(release_id)
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("DELETE FROM release_operations WHERE release_id = $1")
            .bind(release_id)
            .execute(pool)
            .await,
        "55000",
    );

    assert_eq!(count(pool, "release_operations").await, 7);
    assert_eq!(count(pool, "audit_events").await, counts_before.0 + 9);
    assert_eq!(count(pool, "outbox_events").await, counts_before.1 + 10);
    assert_eq!(
        count(pool, "idempotency_records").await,
        counts_before.2 + 9
    );
}

async fn verify_submission_withdrawal(application: &Router, pool: &PgPool) {
    let created = call(
        application.clone(),
        Method::POST,
        "/v1/apps/dev.cardputerzero.notes/submissions",
        Some(OWNER_A_TOKEN),
        Some("withdraw-create-upload-01"),
        Some(json!({
            "version": "91.0.0",
            "package_sha256": "1".repeat(64),
            "package_bytes": 100,
            "listing_sha256": "2".repeat(64),
            "listing_bytes": 100,
            "assets": [
                {"path": "icon.png", "sha256": "3".repeat(64), "bytes": 100,
                 "width": 48, "height": 48},
                {"path": "screens/main.png", "sha256": "4".repeat(64), "bytes": 100,
                 "width": 320, "height": 170}
            ]
        })),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let created_body: Value = serde_json::from_slice(&created.body).unwrap();
    let uploading_id = created_body["submission_id"].as_str().unwrap();

    let body_rejected = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{uploading_id}:withdraw"),
        OWNER_A_TOKEN,
        "withdraw-body-reject1",
        1,
        Some(json!({})),
    )
    .await;
    assert_problem(&body_rejected, StatusCode::BAD_REQUEST, "invalid-request");
    for (token, key, status, code) in [
        (
            NO_2FA_TOKEN,
            "withdraw-no-twofa-01",
            StatusCode::FORBIDDEN,
            "two-factor-required",
        ),
        (
            VIEWER_TOKEN,
            "withdraw-viewer-0001",
            StatusCode::FORBIDDEN,
            "forbidden",
        ),
        (
            OWNER_B_TOKEN,
            "withdraw-cross-team1",
            StatusCode::NOT_FOUND,
            "not-found",
        ),
    ] {
        let result = call_with_etag(
            application.clone(),
            Method::POST,
            &format!("/v1/submissions/{uploading_id}:withdraw"),
            token,
            key,
            1,
            None,
        )
        .await;
        assert_problem(&result, status, code);
    }
    let stale = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{uploading_id}:withdraw"),
        OWNER_A_TOKEN,
        "withdraw-stale-etag1",
        2,
        None,
    )
    .await;
    assert_problem(
        &stale,
        StatusCode::PRECONDITION_FAILED,
        "precondition-failed",
    );

    let counts_before = (
        count(pool, "audit_events").await,
        count(pool, "outbox_events").await,
        count(pool, "idempotency_records").await,
    );
    let withdrawn = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{uploading_id}:withdraw"),
        OWNER_A_TOKEN,
        "withdraw-uploading-01",
        1,
        None,
    )
    .await;
    assert_eq!(withdrawn.status, StatusCode::OK);
    assert_eq!(etag_version(&withdrawn), 2);
    assert_eq!(
        serde_json::from_slice::<Value>(&withdrawn.body).unwrap()["state"],
        "withdrawn"
    );
    let replay = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{uploading_id}:withdraw"),
        OWNER_A_TOKEN,
        "withdraw-uploading-01",
        1,
        None,
    )
    .await;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(replay.body, withdrawn.body);
    let terminal = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{uploading_id}:withdraw"),
        OWNER_A_TOKEN,
        "withdraw-terminal-001",
        2,
        None,
    )
    .await;
    assert_problem(&terminal, StatusCode::CONFLICT, "invalid-transition");
    assert_eq!(count(pool, "audit_events").await, counts_before.0 + 1);
    assert_eq!(count(pool, "outbox_events").await, counts_before.1 + 1);
    assert_eq!(
        count(pool, "idempotency_records").await,
        counts_before.2 + 1
    );

    const PROCESSING: &str = "sub_55555555555555555555555555555555";
    seed_withdraw_submission(pool, PROCESSING, "processing", 7, "91.0.1").await;
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds) VALUES \
         ('evt_55555555555555555555555555555555', 'submission.scan-requested', 'submission', \
          $1, 7, $2, $3, 1)",
    )
    .bind(PROCESSING)
    .bind("5".repeat(64))
    .bind(json!({
        "submission_id": PROCESSING,
        "content_sha256": "f".repeat(64)
    }))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO submission_scan_jobs (event_id, submission_id, source_resource_version, \
         source_content_sha256, state, created_unix_seconds) VALUES \
         ('evt_55555555555555555555555555555555', $1, 7, $2, 'queued', 1)",
    )
    .bind(PROCESSING)
    .bind("f".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    pool.execute(
        "CREATE FUNCTION test_reject_withdraw_audit() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN IF NEW.action = 'submission.withdrawn' AND \
         NEW.object_id = 'sub_55555555555555555555555555555555' THEN \
         RAISE EXCEPTION 'injected withdraw audit failure'; END IF; RETURN NEW; END; $$",
    )
    .await
    .unwrap();
    pool.execute(
        "CREATE TRIGGER test_reject_withdraw_audit_insert BEFORE INSERT ON audit_events \
         FOR EACH ROW EXECUTE FUNCTION test_reject_withdraw_audit()",
    )
    .await
    .unwrap();
    let rolled_back = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{PROCESSING}:withdraw"),
        OWNER_A_TOKEN,
        "withdraw-processing-1",
        7,
        None,
    )
    .await;
    assert_problem(
        &rolled_back,
        StatusCode::SERVICE_UNAVAILABLE,
        "service-unavailable",
    );
    let rollback_state: String =
        sqlx::query_scalar("SELECT state FROM submissions WHERE submission_id = $1")
            .bind(PROCESSING)
            .fetch_one(pool)
            .await
            .unwrap();
    let rollback_job: String =
        sqlx::query_scalar("SELECT state FROM submission_scan_jobs WHERE submission_id = $1")
            .bind(PROCESSING)
            .fetch_one(pool)
            .await
            .unwrap();
    let rollback_published: Option<i64> = sqlx::query_scalar(
        "SELECT published_unix_seconds FROM outbox_events \
         WHERE event_id = 'evt_55555555555555555555555555555555'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(rollback_state, "processing");
    assert_eq!(rollback_job, "queued");
    assert_eq!(rollback_published, None);
    pool.execute("DROP TRIGGER test_reject_withdraw_audit_insert ON audit_events")
        .await
        .unwrap();
    pool.execute("DROP FUNCTION test_reject_withdraw_audit()")
        .await
        .unwrap();
    let processing_withdrawn = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{PROCESSING}:withdraw"),
        OWNER_A_TOKEN,
        "withdraw-processing-1",
        7,
        None,
    )
    .await;
    assert_eq!(processing_withdrawn.status, StatusCode::OK);
    assert_eq!(etag_version(&processing_withdrawn), 8);
    let cancelled_job = sqlx::query(
        "SELECT state, lease_token, leased_until_unix_seconds, last_error_code, \
         completed_unix_seconds FROM submission_scan_jobs WHERE submission_id = $1",
    )
    .bind(PROCESSING)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(cancelled_job.get::<String, _>("state"), "cancelled");
    assert_eq!(
        cancelled_job
            .get::<Option<String>, _>("last_error_code")
            .as_deref(),
        Some("submission-withdrawn")
    );
    assert!(
        cancelled_job
            .get::<Option<String>, _>("lease_token")
            .is_none()
    );
    assert!(
        cancelled_job
            .get::<Option<i64>, _>("leased_until_unix_seconds")
            .is_none()
    );
    assert!(
        cancelled_job
            .get::<Option<i64>, _>("completed_unix_seconds")
            .is_some()
    );
    let scan_event_published: Option<i64> = sqlx::query_scalar(
        "SELECT published_unix_seconds FROM outbox_events \
         WHERE event_id = 'evt_55555555555555555555555555555555'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(scan_event_published.is_some());

    const RUNNING: &str = "sub_88888888888888888888888888888888";
    seed_withdraw_submission(pool, RUNNING, "processing", 11, "91.0.4").await;
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds, \
         published_unix_seconds, attempts) VALUES \
         ('evt_88888888888888888888888888888888', 'submission.scan-requested', 'submission', \
          $1, 11, $2, $3, 1, 2, 1)",
    )
    .bind(RUNNING)
    .bind("8".repeat(64))
    .bind(json!({
        "submission_id": RUNNING,
        "content_sha256": "e".repeat(64)
    }))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO submission_scan_jobs (event_id, submission_id, source_resource_version, \
         source_content_sha256, state, lease_token, leased_until_unix_seconds, attempts, \
         created_unix_seconds) VALUES \
         ('evt_88888888888888888888888888888888', $1, 11, $2, 'running', \
          'lease_88888888888888888888888888888888', 9999999999, 1, 1)",
    )
    .bind(RUNNING)
    .bind("e".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    let running_withdrawn = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{RUNNING}:withdraw"),
        OWNER_A_TOKEN,
        "withdraw-running-001",
        11,
        None,
    )
    .await;
    assert_eq!(running_withdrawn.status, StatusCode::OK);
    assert_eq!(etag_version(&running_withdrawn), 12);
    let cancelled_running = sqlx::query(
        "SELECT state, lease_token, leased_until_unix_seconds, completed_unix_seconds \
         FROM submission_scan_jobs WHERE submission_id = $1",
    )
    .bind(RUNNING)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(cancelled_running.get::<String, _>("state"), "cancelled");
    assert!(
        cancelled_running
            .get::<Option<String>, _>("lease_token")
            .is_none()
    );
    assert!(
        cancelled_running
            .get::<Option<i64>, _>("leased_until_unix_seconds")
            .is_none()
    );
    assert!(
        cancelled_running
            .get::<Option<i64>, _>("completed_unix_seconds")
            .is_some()
    );

    const IN_REVIEW: &str = "sub_66666666666666666666666666666666";
    seed_withdraw_submission(pool, IN_REVIEW, "ready-for-review", 1, "91.0.2").await;
    sqlx::query(
        "INSERT INTO review_assignments (assignment_id, submission_id, reviewer_id, \
         assignment_kind, state, source_resource_version, created_unix_seconds) VALUES \
         ('assignment_66666666666666666666666666666666', $1, $2, 'primary', 'active', 1, 1)",
    )
    .bind(IN_REVIEW)
    .bind(REVIEWER_A)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE submissions SET state = 'in-review', resource_version = 2 \
         WHERE submission_id = $1",
    )
    .bind(IN_REVIEW)
    .execute(pool)
    .await
    .unwrap();
    let review_withdrawn = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{IN_REVIEW}:withdraw"),
        OWNER_A_TOKEN,
        "withdraw-in-review-1",
        2,
        None,
    )
    .await;
    assert_eq!(review_withdrawn.status, StatusCode::OK);
    assert_eq!(etag_version(&review_withdrawn), 3);
    let assignment = sqlx::query(
        "SELECT state, completed_unix_seconds FROM review_assignments WHERE submission_id = $1",
    )
    .bind(IN_REVIEW)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(assignment.get::<String, _>("state"), "cancelled");
    assert!(
        assignment
            .get::<Option<i64>, _>("completed_unix_seconds")
            .is_some()
    );

    const APPROVED: &str = "sub_77777777777777777777777777777777";
    seed_withdraw_submission(pool, APPROVED, "approved", 3, "91.0.3").await;
    let approved = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/submissions/{APPROVED}:withdraw"),
        OWNER_A_TOKEN,
        "withdraw-approved-001",
        3,
        None,
    )
    .await;
    assert_problem(&approved, StatusCode::CONFLICT, "invalid-transition");
}

async fn seed_withdraw_submission(
    pool: &PgPool,
    submission_id: &str,
    state: &str,
    resource_version: i64,
    version: &str,
) {
    sqlx::query(
        "INSERT INTO submissions (submission_id, app_id, version, revision, state, package_sha256, \
         package_bytes, listing_sha256, listing_bytes, assets, resource_version, \
         created_unix_seconds, finalized_content_sha256) VALUES \
         ($1, 'dev.cardputerzero.notes', $2, 1, $3, $4, 100, $5, 100, $6, $7, 1, $8)",
    )
    .bind(submission_id)
    .bind(version)
    .bind(state)
    .bind("a".repeat(64))
    .bind("b".repeat(64))
    .bind(json!([
        {"path": "icon.png", "sha256": "c".repeat(64), "bytes": 100,
         "width": 48, "height": 48},
        {"path": "screens/main.png", "sha256": "d".repeat(64), "bytes": 100,
         "width": 320, "height": 170}
    ]))
    .bind(resource_version)
    .bind("f".repeat(64))
    .execute(pool)
    .await
    .unwrap();
}

async fn verify_oauth_device_flow(application: &Router, pool: &PgPool) {
    let invalid = call(
        application.clone(),
        Method::POST,
        "/oauth/device/code",
        None,
        None,
        Some(json!({"client_id": "other", "scope": "store.submit"})),
    )
    .await;
    assert_problem(&invalid, StatusCode::BAD_REQUEST, "invalid-request");
    assert_oauth_headers(&invalid);

    let authorization = create_device_code(application).await;
    let device_code = authorization["device_code"].as_str().unwrap().to_owned();
    let user_code = authorization["user_code"].as_str().unwrap().to_owned();
    assert!(device_code.starts_with("cp0_dc_"));
    assert_eq!(device_code.len(), 71);
    assert_eq!(user_code.len(), 14);
    assert_eq!(
        authorization["verification_uri"],
        "https://developer.cardputerzero.dev/activate"
    );
    assert_eq!(authorization["expires_in"], 600);
    assert_eq!(authorization["interval"], 5);

    let stored_digest: String = sqlx::query_scalar(
        "SELECT device_code_sha256 FROM oauth_device_authorizations WHERE user_code = $1",
    )
    .bind(&user_code)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(stored_digest, sha256_hex(device_code.as_bytes()));
    assert_ne!(stored_digest, device_code);

    let early_poll = exchange_device_code(application, &device_code).await;
    assert_problem(&early_poll, StatusCode::BAD_REQUEST, "slow-down");
    assert_oauth_headers(&early_poll);
    let interval: i16 = sqlx::query_scalar(
        "SELECT poll_interval_seconds FROM oauth_device_authorizations WHERE user_code = $1",
    )
    .bind(&user_code)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(interval, 10);

    for (token, key, status, code) in [
        (
            None,
            "oauth-missing-auth-01",
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
        (
            Some(EXPIRED_TOKEN),
            "oauth-expired-auth-1",
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
        (
            Some(REVOKED_TOKEN),
            "oauth-revoked-auth-1",
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
        (
            Some(NO_2FA_TOKEN),
            "oauth-no2fa-auth-01",
            StatusCode::FORBIDDEN,
            "two-factor-required",
        ),
        (
            Some(VIEWER_TOKEN),
            "oauth-viewer-auth-1",
            StatusCode::FORBIDDEN,
            "forbidden",
        ),
        (
            Some(READ_ONLY_TOKEN),
            "oauth-scope-auth-01",
            StatusCode::FORBIDDEN,
            "forbidden",
        ),
    ] {
        let result = authorize_device_code(application, token, key, &user_code, "approve").await;
        assert_problem(&result, status, code);
        assert_oauth_headers(&result);
    }

    let approved = authorize_device_code(
        application,
        Some(OWNER_A_TOKEN),
        "oauth-approve-main-01",
        &user_code,
        "approve",
    )
    .await;
    assert_eq!(approved.status, StatusCode::NO_CONTENT);
    assert_oauth_headers(&approved);
    let replay = authorize_device_code(
        application,
        Some(OWNER_A_TOKEN),
        "oauth-approve-main-01",
        &user_code,
        "approve",
    )
    .await;
    assert_eq!(replay.status, StatusCode::NO_CONTENT);
    let conflicting_replay = authorize_device_code(
        application,
        Some(OWNER_A_TOKEN),
        "oauth-approve-main-01",
        &user_code,
        "deny",
    )
    .await;
    assert_problem(
        &conflicting_replay,
        StatusCode::CONFLICT,
        "idempotency-conflict",
    );

    let exchange_a = exchange_device_code(application, &device_code);
    let exchange_b = exchange_device_code(application, &device_code);
    let (exchange_a, exchange_b) = tokio::join!(exchange_a, exchange_b);
    let (issued, rejected) = if exchange_a.status == StatusCode::OK {
        (exchange_a, exchange_b)
    } else {
        (exchange_b, exchange_a)
    };
    assert_eq!(issued.status, StatusCode::OK);
    assert_oauth_headers(&issued);
    assert_problem(&rejected, StatusCode::BAD_REQUEST, "access-denied");
    let token_response: Value = serde_json::from_slice(&issued.body).unwrap();
    let access_token = token_response["access_token"].as_str().unwrap().to_owned();
    assert!(access_token.starts_with("cp0_at_"));
    assert_eq!(access_token.len(), 71);
    assert_eq!(token_response["token_type"], "Bearer");
    assert_eq!(token_response["expires_in"], 900);
    assert_eq!(token_response["scope"], "store.submit");

    let token_digest = sha256_hex(access_token.as_bytes());
    let issued_row = sqlx::query(
        "SELECT device_auth.state, device_auth.issued_token_sha256, token.scopes, \
         token.revoked FROM oauth_device_authorizations device_auth \
         JOIN access_tokens token ON token.token_sha256 = device_auth.issued_token_sha256 \
         WHERE device_auth.device_code_sha256 = $1",
    )
    .bind(sha256_hex(device_code.as_bytes()))
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(issued_row.get::<String, _>("state"), "consumed");
    assert_eq!(
        issued_row.get::<String, _>("issued_token_sha256"),
        token_digest
    );
    assert_eq!(
        issued_row.get::<Vec<String>, _>("scopes"),
        vec!["store.submit"]
    );
    assert!(!issued_row.get::<bool, _>("revoked"));

    let submission = call(
        application.clone(),
        Method::POST,
        "/v1/apps/dev.cardputerzero.notes/submissions",
        Some(&access_token),
        Some("oauth-created-submission-01"),
        Some(json!({
            "version": "90.0.0",
            "package_sha256": "a".repeat(64),
            "package_bytes": 100,
            "listing_sha256": "b".repeat(64),
            "listing_bytes": 100,
            "assets": [
                {"path": "icon.png", "sha256": "c".repeat(64), "bytes": 100,
                 "width": 48, "height": 48},
                {"path": "screens/main.png", "sha256": "d".repeat(64), "bytes": 100,
                 "width": 320, "height": 170}
            ]
        })),
    )
    .await;
    assert_eq!(submission.status, StatusCode::CREATED);

    sqlx::query("UPDATE access_tokens SET revoked = TRUE WHERE token_sha256 = $1")
        .bind(&token_digest)
        .execute(pool)
        .await
        .unwrap();
    let revoked = call(
        application.clone(),
        Method::GET,
        "/v1/apps/dev.cardputerzero.notes",
        Some(&access_token),
        None,
        None,
    )
    .await;
    assert_problem(&revoked, StatusCode::UNAUTHORIZED, "unauthorized");

    let pending_code = format!("cp0_dc_{}", "1".repeat(64));
    insert_device_authorization(pool, &pending_code, "1111-2222-3333", -1, 600, -1).await;
    let pending = exchange_device_code(application, &pending_code).await;
    assert_problem(&pending, StatusCode::BAD_REQUEST, "authorization-pending");

    let denied_authorization = create_device_code(application).await;
    let denied_code = denied_authorization["device_code"].as_str().unwrap();
    let denied_user_code = denied_authorization["user_code"].as_str().unwrap();
    let denied = authorize_device_code(
        application,
        Some(OWNER_A_TOKEN),
        "oauth-deny-device-001",
        denied_user_code,
        "deny",
    )
    .await;
    assert_eq!(denied.status, StatusCode::NO_CONTENT);
    let denied_exchange = exchange_device_code(application, denied_code).await;
    assert_problem(&denied_exchange, StatusCode::BAD_REQUEST, "access-denied");

    let role_authorization = create_device_code(application).await;
    sqlx::query(
        "UPDATE team_members SET role = 'developer', \
         resource_version = resource_version + 1 WHERE member_id = $1",
    )
    .bind(VIEWER_MEMBER)
    .execute(pool)
    .await
    .unwrap();
    let role_approved = authorize_device_code(
        application,
        Some(VIEWER_TOKEN),
        "oauth-role-change-001",
        role_authorization["user_code"].as_str().unwrap(),
        "approve",
    )
    .await;
    assert_eq!(role_approved.status, StatusCode::NO_CONTENT);
    sqlx::query(
        "UPDATE team_members SET role = 'viewer', \
         resource_version = resource_version + 1 WHERE member_id = $1",
    )
    .bind(VIEWER_MEMBER)
    .execute(pool)
    .await
    .unwrap();
    let role_changed = exchange_device_code(
        application,
        role_authorization["device_code"].as_str().unwrap(),
    )
    .await;
    assert_problem(&role_changed, StatusCode::BAD_REQUEST, "access-denied");

    let factor_authorization = create_device_code(application).await;
    sqlx::query(
        "UPDATE team_members SET two_factor_enabled = TRUE, \
         resource_version = resource_version + 1 WHERE member_id = $1",
    )
    .bind(NO_2FA_MEMBER)
    .execute(pool)
    .await
    .unwrap();
    let factor_approved = authorize_device_code(
        application,
        Some(NO_2FA_TOKEN),
        "oauth-factor-change-1",
        factor_authorization["user_code"].as_str().unwrap(),
        "approve",
    )
    .await;
    assert_eq!(factor_approved.status, StatusCode::NO_CONTENT);
    sqlx::query(
        "UPDATE team_members SET two_factor_enabled = FALSE, \
         resource_version = resource_version + 1 WHERE member_id = $1",
    )
    .bind(NO_2FA_MEMBER)
    .execute(pool)
    .await
    .unwrap();
    let factor_changed = exchange_device_code(
        application,
        factor_authorization["device_code"].as_str().unwrap(),
    )
    .await;
    assert_problem(&factor_changed, StatusCode::BAD_REQUEST, "access-denied");

    let expired_code = format!("cp0_dc_{}", "2".repeat(64));
    insert_device_authorization(pool, &expired_code, "AAAA-BBBB-CCCC", -10, -1, -10).await;
    let expired = exchange_device_code(application, &expired_code).await;
    assert_problem(&expired, StatusCode::BAD_REQUEST, "expired-token");
    let expired_decision = authorize_device_code(
        application,
        Some(OWNER_A_TOKEN),
        "oauth-expired-code-01",
        "AAAA-BBBB-CCCC",
        "approve",
    )
    .await;
    assert_problem(&expired_decision, StatusCode::BAD_REQUEST, "expired-token");

    for secret in [&device_code, &access_token] {
        let outbox_matches: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM outbox_events WHERE payload::TEXT LIKE '%' || $1 || '%'",
        )
        .bind(secret)
        .fetch_one(pool)
        .await
        .unwrap();
        let audit_matches: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE to_jsonb(audit_events)::TEXT LIKE '%' || $1 || '%'",
        )
        .bind(secret)
        .fetch_one(pool)
        .await
        .unwrap();
        let idempotency_matches: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM idempotency_records \
             WHERE COALESCE(response_body::TEXT, '') LIKE '%' || $1 || '%'",
        )
        .bind(secret)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(
            (outbox_matches, audit_matches, idempotency_matches),
            (0, 0, 0)
        );
    }
}

async fn create_device_code(application: &Router) -> Value {
    let result = call(
        application.clone(),
        Method::POST,
        "/oauth/device/code",
        None,
        None,
        Some(json!({"client_id": "cp0ctl", "scope": "store.submit"})),
    )
    .await;
    assert_eq!(result.status, StatusCode::OK);
    assert_oauth_headers(&result);
    serde_json::from_slice(&result.body).unwrap()
}

async fn authorize_device_code(
    application: &Router,
    token: Option<&str>,
    idempotency_key: &str,
    user_code: &str,
    decision: &str,
) -> HttpResult {
    call(
        application.clone(),
        Method::POST,
        "/oauth/device/authorize",
        token,
        Some(idempotency_key),
        Some(json!({"user_code": user_code, "decision": decision})),
    )
    .await
}

async fn exchange_device_code(application: &Router, device_code: &str) -> HttpResult {
    call(
        application.clone(),
        Method::POST,
        "/oauth/token",
        None,
        None,
        Some(json!({
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            "device_code": device_code,
            "client_id": "cp0ctl"
        })),
    )
    .await
}

async fn insert_device_authorization(
    pool: &PgPool,
    device_code: &str,
    user_code: &str,
    requested_offset: i64,
    expires_offset: i64,
    next_poll_offset: i64,
) {
    let now: i64 = sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO oauth_device_authorizations (device_code_sha256, user_code, client_id, \
         scopes, state, requested_unix_seconds, expires_unix_seconds, poll_interval_seconds, \
         next_poll_unix_seconds) VALUES ($1, $2, 'cp0ctl', ARRAY['store.submit'], 'pending', \
         $3, $4, 5, $5)",
    )
    .bind(sha256_hex(device_code.as_bytes()))
    .bind(user_code)
    .bind(now + requested_offset)
    .bind(now + expires_offset)
    .bind(now + next_poll_offset)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_standard_risk_assessment(
    pool: &PgPool,
    submission_id: &str,
    created_unix_seconds: i64,
) {
    let suffix = submission_id.strip_prefix("sub_").unwrap();
    let event_id = format!("evt_{suffix}");
    let scan_id = format!("scan_{suffix}");
    let assessment_id = format!("risk_{suffix}");
    let report = json!({
        "scanner_version": "cp0-store-scan/1",
        "disposition": "ready-for-review",
        "developer_key_sha256": "6".repeat(64),
        "imports": [],
        "permissions": [],
        "findings": [],
        "risk": {"policy_version": 1, "tier": "standard", "reasons": []}
    });
    let report_sha256 = sha256_hex(serde_json::to_vec(&report).unwrap().as_slice());
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds, published_unix_seconds) \
         VALUES ($1, 'submission.scan-requested', 'submission', $2, 1, $3, '{}'::jsonb, $4, $4)",
    )
    .bind(&event_id)
    .bind(submission_id)
    .bind(&report_sha256)
    .bind(created_unix_seconds)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO submission_scan_jobs (event_id, submission_id, source_resource_version, \
         source_content_sha256, state, attempts, created_unix_seconds, completed_unix_seconds) \
         VALUES ($1, $2, 1, $3, 'completed', 1, $4, $4)",
    )
    .bind(&event_id)
    .bind(submission_id)
    .bind("5".repeat(64))
    .bind(created_unix_seconds)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO submission_scan_results (scan_id, event_id, submission_id, \
         source_resource_version, source_content_sha256, outcome, scanner_version, report, \
         report_sha256, created_unix_seconds) \
         VALUES ($1, $2, $3, 1, $4, 'ready-for-review', 'cp0-store-scan/1', $5, $6, $7)",
    )
    .bind(&scan_id)
    .bind(&event_id)
    .bind(submission_id)
    .bind("5".repeat(64))
    .bind(report)
    .bind(&report_sha256)
    .bind(created_unix_seconds)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO submission_risk_assessments (assessment_id, scan_id, submission_id, \
         source_report_sha256, policy_version, tier, reason_codes, created_unix_seconds) \
         VALUES ($1, $2, $3, $4, 1, 'standard', '[]'::jsonb, $5)",
    )
    .bind(assessment_id)
    .bind(scan_id)
    .bind(submission_id)
    .bind(report_sha256)
    .bind(created_unix_seconds)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_review_submission(
    pool: &PgPool,
    submission_id: &str,
    version: &str,
    created_unix_seconds: i64,
) {
    let assets = json!([
        {
            "path": "icon.png",
            "sha256": "3".repeat(64),
            "bytes": 100,
            "width": 48,
            "height": 48
        },
        {
            "path": "screens/main.png",
            "sha256": "4".repeat(64),
            "bytes": 100,
            "width": 320,
            "height": 170
        }
    ]);
    sqlx::query(
        "INSERT INTO submissions (submission_id, app_id, version, revision, state, \
         package_sha256, package_bytes, listing_sha256, listing_bytes, assets, resource_version, \
         created_unix_seconds, finalized_content_sha256) VALUES \
         ($1, 'dev.cardputerzero.notes', $2, 1, 'ready-for-review', $3, 100, $4, 100, $5, 1, $6, $7)",
    )
    .bind(submission_id)
    .bind(version)
    .bind("1".repeat(64))
    .bind("2".repeat(64))
    .bind(assets)
    .bind(created_unix_seconds)
    .bind("5".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    seed_standard_risk_assessment(pool, submission_id, created_unix_seconds).await;
}

async fn seed_double_approved_submission(
    pool: &PgPool,
    submission_id: &str,
    version: &str,
    created_unix_seconds: i64,
) {
    const PRIMARY_ASSIGNMENT: &str = "assignment_dddddddddddddddddddddddddddddddd";
    const SECONDARY_ASSIGNMENT: &str = "assignment_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    seed_review_submission(pool, submission_id, version, created_unix_seconds).await;
    sqlx::query(
        "INSERT INTO review_assignments (assignment_id, submission_id, reviewer_id, \
         assignment_kind, state, source_resource_version, created_unix_seconds) \
         VALUES ($1, $2, $3, 'primary', 'active', 1, $4)",
    )
    .bind(PRIMARY_ASSIGNMENT)
    .bind(submission_id)
    .bind(REVIEWER_A)
    .bind(created_unix_seconds)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE submissions SET state = 'in-review', resource_version = 2 \
         WHERE submission_id = $1",
    )
    .bind(submission_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_decisions (decision_id, submission_id, reviewer_id, decision, \
         reason_codes, note, created_unix_seconds, assignment_id) VALUES \
         ('decision_dddddddddddddddddddddddddddddddd', $1, $2, 'approved', '{}', '', $3, $4)",
    )
    .bind(submission_id)
    .bind(REVIEWER_A)
    .bind(created_unix_seconds + 1)
    .bind(PRIMARY_ASSIGNMENT)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE review_assignments SET state = 'completed', completed_unix_seconds = $1 \
         WHERE assignment_id = $2",
    )
    .bind(created_unix_seconds + 1)
    .bind(PRIMARY_ASSIGNMENT)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE submissions SET state = 'pending-secondary-review', resource_version = 3 \
         WHERE submission_id = $1",
    )
    .bind(submission_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_assignments (assignment_id, submission_id, reviewer_id, \
         assignment_kind, state, source_resource_version, created_unix_seconds) \
         VALUES ($1, $2, $3, 'secondary', 'active', 3, $4)",
    )
    .bind(SECONDARY_ASSIGNMENT)
    .bind(submission_id)
    .bind(REVIEWER_B)
    .bind(created_unix_seconds + 2)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE submissions SET state = 'in-review', resource_version = 4 \
         WHERE submission_id = $1",
    )
    .bind(submission_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_decisions (decision_id, submission_id, reviewer_id, decision, \
         reason_codes, note, created_unix_seconds, assignment_id) VALUES \
         ('decision_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', $1, $2, 'approved', '{}', '', $3, $4)",
    )
    .bind(submission_id)
    .bind(REVIEWER_B)
    .bind(created_unix_seconds + 3)
    .bind(SECONDARY_ASSIGNMENT)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE review_assignments SET state = 'completed', completed_unix_seconds = $1 \
         WHERE assignment_id = $2",
    )
    .bind(created_unix_seconds + 3)
    .bind(SECONDARY_ASSIGNMENT)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE submissions SET state = 'approved', resource_version = 5 \
         WHERE submission_id = $1",
    )
    .bind(submission_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn verify_team_management(application: &Router, pool: &PgPool) {
    let team = call(
        application.clone(),
        Method::GET,
        &format!("/v1/teams/{TEAM_A}"),
        Some(OWNER_A_TOKEN),
        None,
        None,
    )
    .await;
    assert_eq!(team.status, StatusCode::OK);
    assert_eq!(etag_version(&team), 1);
    let team_body: Value = serde_json::from_slice(&team.body).unwrap();
    assert_eq!(team_body["team_id"], TEAM_A);
    assert_eq!(team_body["members"].as_array().unwrap().len(), 5);
    assert!(
        team_body["members"]
            .as_array()
            .unwrap()
            .windows(2)
            .all(|members| members[0]["member_id"].as_str() < members[1]["member_id"].as_str())
    );

    let missing_scope = call(
        application.clone(),
        Method::GET,
        &format!("/v1/teams/{TEAM_A}"),
        Some(READ_ONLY_TOKEN),
        None,
        None,
    )
    .await;
    assert_problem(&missing_scope, StatusCode::FORBIDDEN, "forbidden");
    let cross_team_read = call(
        application.clone(),
        Method::GET,
        &format!("/v1/teams/{TEAM_A}"),
        Some(OWNER_B_TOKEN),
        None,
        None,
    )
    .await;
    assert_problem(&cross_team_read, StatusCode::NOT_FOUND, "not-found");

    let invalid_role = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/teams/{TEAM_A}/members/{RELEASE_MANAGER}:set-role"),
        OWNER_A_TOKEN,
        "team-role-invalid-01",
        1,
        Some(json!({"role": "admin"})),
    )
    .await;
    assert_problem(&invalid_role, StatusCode::BAD_REQUEST, "invalid-request");
    for (token, key, status, code) in [
        (
            NO_2FA_TOKEN,
            "team-role-no2fa-001",
            StatusCode::FORBIDDEN,
            "forbidden",
        ),
        (
            VIEWER_TOKEN,
            "team-role-viewer-001",
            StatusCode::FORBIDDEN,
            "forbidden",
        ),
        (
            STALE_MFA_TOKEN,
            "team-role-stale-0001",
            StatusCode::FORBIDDEN,
            "step-up-required",
        ),
        (
            OWNER_B_TOKEN,
            "team-role-cross-0001",
            StatusCode::NOT_FOUND,
            "not-found",
        ),
    ] {
        let response = call_with_etag(
            application.clone(),
            Method::POST,
            &format!("/v1/teams/{TEAM_A}/members/{RELEASE_MANAGER}:set-role"),
            token,
            key,
            1,
            Some(json!({"role": "developer"})),
        )
        .await;
        assert_problem(&response, status, code);
    }
    sqlx::query(
        "UPDATE team_members SET two_factor_enabled = FALSE, \
         resource_version = resource_version + 1 WHERE member_id = $1",
    )
    .bind(OWNER_B)
    .execute(pool)
    .await
    .unwrap();
    let owner_without_factor = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/teams/{TEAM_B}/members/{OWNER_B}:set-role"),
        OWNER_B_TOKEN,
        "team-role-owner2fa1",
        1,
        Some(json!({"role": "developer"})),
    )
    .await;
    assert_problem(
        &owner_without_factor,
        StatusCode::FORBIDDEN,
        "two-factor-required",
    );
    sqlx::query(
        "UPDATE team_members SET two_factor_enabled = TRUE, \
         resource_version = resource_version + 1 WHERE member_id = $1",
    )
    .bind(OWNER_B)
    .execute(pool)
    .await
    .unwrap();
    let stale = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/teams/{TEAM_A}/members/{RELEASE_MANAGER}:set-role"),
        OWNER_A_TOKEN,
        "team-role-etag-00001",
        2,
        Some(json!({"role": "developer"})),
    )
    .await;
    assert_problem(
        &stale,
        StatusCode::PRECONDITION_FAILED,
        "precondition-failed",
    );
    let last_owner = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/teams/{TEAM_B}/members/{OWNER_B}:set-role"),
        OWNER_B_TOKEN,
        "team-role-lastowner1",
        1,
        Some(json!({"role": "developer"})),
    )
    .await;
    assert_problem(&last_owner, StatusCode::CONFLICT, "conflict");

    let counts_before = (
        count(pool, "audit_events").await,
        count(pool, "outbox_events").await,
        count(pool, "idempotency_records").await,
    );
    let changed = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/teams/{TEAM_A}/members/{RELEASE_MANAGER}:set-role"),
        OWNER_A_TOKEN,
        "team-role-change-001",
        1,
        Some(json!({"role": "developer"})),
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(etag_version(&changed), 2);
    let changed_body: Value = serde_json::from_slice(&changed.body).unwrap();
    let changed_member = changed_body["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["member_id"] == RELEASE_MANAGER)
        .unwrap();
    assert_eq!(changed_member["role"], "developer");
    assert_eq!(changed_member["resource_version"], 2);
    let replay = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/teams/{TEAM_A}/members/{RELEASE_MANAGER}:set-role"),
        OWNER_A_TOKEN,
        "team-role-change-001",
        1,
        Some(json!({"role": "developer"})),
    )
    .await;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(replay.body, changed.body);
    assert_eq!(etag_version(&replay), 2);
    assert_eq!(count(pool, "audit_events").await, counts_before.0 + 1);
    assert_eq!(count(pool, "outbox_events").await, counts_before.1 + 1);
    assert_eq!(
        count(pool, "idempotency_records").await,
        counts_before.2 + 1
    );
    let revoked_target = call(
        application.clone(),
        Method::GET,
        &format!("/v1/teams/{TEAM_A}"),
        Some(RELEASE_MANAGER_TOKEN),
        None,
        None,
    )
    .await;
    assert_problem(&revoked_target, StatusCode::UNAUTHORIZED, "unauthorized");
    let viewer_version_before: i64 =
        sqlx::query_scalar("SELECT resource_version FROM team_members WHERE member_id = $1")
            .bind(VIEWER_MEMBER)
            .fetch_one(pool)
            .await
            .unwrap();

    pool.execute(
        "CREATE FUNCTION test_reject_team_role_audit() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN IF NEW.action = 'team.member-role-changed' AND \
         NEW.object_id = 'team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' THEN \
         RAISE EXCEPTION 'injected team role audit failure'; END IF; RETURN NEW; END; $$",
    )
    .await
    .unwrap();
    pool.execute(
        "CREATE TRIGGER test_reject_team_role_audit_insert BEFORE INSERT ON audit_events \
         FOR EACH ROW EXECUTE FUNCTION test_reject_team_role_audit()",
    )
    .await
    .unwrap();
    let rolled_back = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/teams/{TEAM_A}/members/{VIEWER_MEMBER}:set-role"),
        OWNER_A_TOKEN,
        "team-role-rollback01",
        2,
        Some(json!({"role": "developer"})),
    )
    .await;
    assert_problem(
        &rolled_back,
        StatusCode::SERVICE_UNAVAILABLE,
        "service-unavailable",
    );
    let rollback_team_version: i64 =
        sqlx::query_scalar("SELECT resource_version FROM teams WHERE team_id = $1")
            .bind(TEAM_A)
            .fetch_one(pool)
            .await
            .unwrap();
    let rollback_member =
        sqlx::query("SELECT role, resource_version FROM team_members WHERE member_id = $1")
            .bind(VIEWER_MEMBER)
            .fetch_one(pool)
            .await
            .unwrap();
    let rollback_revoked: bool =
        sqlx::query_scalar("SELECT revoked FROM access_tokens WHERE token_sha256 = $1")
            .bind(sha256_hex(VIEWER_TOKEN.as_bytes()))
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(rollback_team_version, 2);
    assert_eq!(rollback_member.get::<String, _>("role"), "viewer");
    assert_eq!(
        rollback_member.get::<i64, _>("resource_version"),
        viewer_version_before
    );
    assert!(!rollback_revoked);
    pool.execute("DROP TRIGGER test_reject_team_role_audit_insert ON audit_events")
        .await
        .unwrap();
    pool.execute("DROP FUNCTION test_reject_team_role_audit()")
        .await
        .unwrap();
    let retried = call_with_etag(
        application.clone(),
        Method::POST,
        &format!("/v1/teams/{TEAM_A}/members/{VIEWER_MEMBER}:set-role"),
        OWNER_A_TOKEN,
        "team-role-rollback01",
        2,
        Some(json!({"role": "developer"})),
    )
    .await;
    assert_eq!(retried.status, StatusCode::OK);
    assert_eq!(etag_version(&retried), 3);
    let revoked_after_commit: bool =
        sqlx::query_scalar("SELECT revoked FROM access_tokens WHERE token_sha256 = $1")
            .bind(sha256_hex(VIEWER_TOKEN.as_bytes()))
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(revoked_after_commit);
}

async fn verify_editorial_backend(application: &Router, pool: &PgPool) {
    const FEATURED: &str = "rel_e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1";
    const COLLECTION_A: &str = "rel_e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2";
    const COLLECTION_B: &str = "rel_e3e3e3e3e3e3e3e3e3e3e3e3e3e3e3e3";
    const PAUSED: &str = "rel_e4e4e4e4e4e4e4e4e4e4e4e4e4e4e4e4";
    const REMOVED: &str = "rel_e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5";
    const UNAPPROVED: &str = "rel_e6e6e6e6e6e6e6e6e6e6e6e6e6e6e6e6";
    const DUPLICATE_APP: &str = "rel_e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7";

    seed_editorial_release(
        pool,
        FEATURED,
        "sub_e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1",
        "dev.cardputerzero.editorial-featured",
        "1.0.0",
        901,
    )
    .await;
    seed_editorial_release(
        pool,
        COLLECTION_A,
        "sub_e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2",
        "dev.cardputerzero.editorial-a",
        "1.0.0",
        902,
    )
    .await;
    seed_editorial_release(
        pool,
        COLLECTION_B,
        "sub_e3e3e3e3e3e3e3e3e3e3e3e3e3e3e3e3",
        "dev.cardputerzero.editorial-b",
        "1.0.0",
        903,
    )
    .await;
    seed_editorial_release(
        pool,
        PAUSED,
        "sub_e4e4e4e4e4e4e4e4e4e4e4e4e4e4e4e4",
        "dev.cardputerzero.editorial-paused",
        "1.0.0",
        904,
    )
    .await;
    sqlx::query("UPDATE releases SET state = 'paused', resource_version = 4 WHERE release_id = $1")
        .bind(PAUSED)
        .execute(pool)
        .await
        .unwrap();
    seed_editorial_release(
        pool,
        REMOVED,
        "sub_e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5",
        "dev.cardputerzero.editorial-removed",
        "1.0.0",
        905,
    )
    .await;
    sqlx::query(
        "UPDATE releases SET state = 'removed', resource_version = 4 WHERE release_id = $1",
    )
    .bind(REMOVED)
    .execute(pool)
    .await
    .unwrap();
    seed_unapproved_editorial_release(pool, UNAPPROVED).await;
    seed_editorial_release(
        pool,
        DUPLICATE_APP,
        "sub_e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7",
        "dev.cardputerzero.editorial-a",
        "2.0.0",
        907,
    )
    .await;

    let request = json!({
        "headline": "Reviewed for 320 x 170",
        "featured_release_id": FEATURED,
        "collections": [{
            "title": "Small-screen essentials",
            "release_ids": [COLLECTION_A, COLLECTION_B]
        }]
    });
    let missing = call(
        application.clone(),
        Method::GET,
        "/v1/editorial/today",
        Some(EDITOR_TOKEN),
        None,
        None,
    )
    .await;
    assert_problem(&missing, StatusCode::NOT_FOUND, "not-found");
    for (token, status, code) in [
        (OWNER_A_TOKEN, StatusCode::UNAUTHORIZED, "unauthorized"),
        (REVIEWER_A_TOKEN, StatusCode::UNAUTHORIZED, "unauthorized"),
        (
            EDITOR_NO_2FA_TOKEN,
            StatusCode::FORBIDDEN,
            "two-factor-required",
        ),
        (
            EDITOR_EXPIRED_TOKEN,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
        (
            EDITOR_REVOKED_TOKEN,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
        (
            EDITOR_SUSPENDED_TOKEN,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
    ] {
        let denied = call(
            application.clone(),
            Method::GET,
            "/v1/editorial/today",
            Some(token),
            None,
            None,
        )
        .await;
        assert_problem(&denied, status, code);
    }
    let unauthenticated = call(
        application.clone(),
        Method::GET,
        "/v1/editorial/today",
        None,
        None,
        None,
    )
    .await;
    assert_problem(&unauthenticated, StatusCode::UNAUTHORIZED, "unauthorized");
    let create_with_etag = call_with_etag(
        application.clone(),
        Method::POST,
        "/v1/editorial/today",
        EDITOR_TOKEN,
        "editorial-create-etag1",
        1,
        Some(request.clone()),
    )
    .await;
    assert_problem(
        &create_with_etag,
        StatusCode::BAD_REQUEST,
        "invalid-request",
    );

    for (key, featured, expected_status, expected_code) in [
        (
            "editorial-missing-001",
            "rel_ffffffffffffffffffffffffffffffff",
            StatusCode::CONFLICT,
            "invalid-transition",
        ),
        (
            "editorial-paused-001",
            PAUSED,
            StatusCode::CONFLICT,
            "invalid-transition",
        ),
        (
            "editorial-removed-001",
            REMOVED,
            StatusCode::CONFLICT,
            "invalid-transition",
        ),
        (
            "editorial-unapproved1",
            UNAPPROVED,
            StatusCode::CONFLICT,
            "invalid-transition",
        ),
    ] {
        let mut invalid = request.clone();
        invalid["featured_release_id"] = Value::String(featured.into());
        let response = call(
            application.clone(),
            Method::POST,
            "/v1/editorial/today",
            Some(EDITOR_TOKEN),
            Some(key),
            Some(invalid),
        )
        .await;
        assert_problem(&response, expected_status, expected_code);
    }
    let duplicate_release = call(
        application.clone(),
        Method::POST,
        "/v1/editorial/today",
        Some(EDITOR_TOKEN),
        Some("editorial-duplicate1"),
        Some(json!({
            "headline": "Duplicate Release",
            "featured_release_id": FEATURED,
            "collections": [{"title": "Repeated", "release_ids": [FEATURED]}]
        })),
    )
    .await;
    assert_problem(
        &duplicate_release,
        StatusCode::BAD_REQUEST,
        "invalid-request",
    );
    let duplicate_app = call(
        application.clone(),
        Method::POST,
        "/v1/editorial/today",
        Some(EDITOR_TOKEN),
        Some("editorial-dup-app01"),
        Some(json!({
            "headline": "Duplicate App",
            "featured_release_id": FEATURED,
            "collections": [{
                "title": "Repeated App",
                "release_ids": [COLLECTION_A, DUPLICATE_APP]
            }]
        })),
    )
    .await;
    assert_problem(&duplicate_app, StatusCode::BAD_REQUEST, "invalid-request");

    let created = call(
        application.clone(),
        Method::POST,
        "/v1/editorial/today",
        Some(EDITOR_TOKEN),
        Some("editorial-create-001"),
        Some(request.clone()),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(etag_version(&created), 1);
    let created_body: Value = serde_json::from_slice(&created.body).unwrap();
    assert_eq!(created_body["layout_id"], "today");
    assert_eq!(created_body["resource_version"], 1);
    assert_eq!(created_body["featured"]["release_id"], FEATURED);
    assert_eq!(
        created_body["collections"][0]["items"][0]["app_id"],
        "dev.cardputerzero.editorial-a"
    );
    let replay = call(
        application.clone(),
        Method::POST,
        "/v1/editorial/today",
        Some(EDITOR_TOKEN),
        Some("editorial-create-001"),
        Some(request.clone()),
    )
    .await;
    assert_eq!(replay.status, StatusCode::CREATED);
    assert_eq!(replay.body, created.body);
    let replay_conflict = call(
        application.clone(),
        Method::POST,
        "/v1/editorial/today",
        Some(EDITOR_TOKEN),
        Some("editorial-create-001"),
        Some(json!({
            "headline": "Different replay body",
            "featured_release_id": FEATURED,
            "collections": [{"title": "Apps", "release_ids": [COLLECTION_A]}]
        })),
    )
    .await;
    assert_problem(
        &replay_conflict,
        StatusCode::CONFLICT,
        "idempotency-conflict",
    );
    let duplicate_create = call(
        application.clone(),
        Method::POST,
        "/v1/editorial/today",
        Some(EDITOR_TOKEN),
        Some("editorial-create-002"),
        Some(request.clone()),
    )
    .await;
    assert_problem(&duplicate_create, StatusCode::CONFLICT, "conflict");

    let missing_precondition = call(
        application.clone(),
        Method::PUT,
        "/v1/editorial/today",
        Some(EDITOR_TOKEN),
        Some("editorial-put-noetag"),
        Some(request.clone()),
    )
    .await;
    assert_problem(
        &missing_precondition,
        StatusCode::BAD_REQUEST,
        "invalid-request",
    );
    let stale = call_with_etag(
        application.clone(),
        Method::PUT,
        "/v1/editorial/today",
        EDITOR_TOKEN,
        "editorial-put-stale1",
        9,
        Some(request.clone()),
    )
    .await;
    assert_problem(
        &stale,
        StatusCode::PRECONDITION_FAILED,
        "precondition-failed",
    );
    let updated_request = json!({
        "headline": "Fresh picks for CardputerZero",
        "featured_release_id": COLLECTION_A,
        "collections": [{
            "title": "Editor's utilities",
            "release_ids": [FEATURED, COLLECTION_B]
        }]
    });
    let updated = call_with_etag(
        application.clone(),
        Method::PUT,
        "/v1/editorial/today",
        EDITOR_TOKEN,
        "editorial-update-001",
        1,
        Some(updated_request.clone()),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK);
    assert_eq!(etag_version(&updated), 2);
    let update_replay = call_with_etag(
        application.clone(),
        Method::PUT,
        "/v1/editorial/today",
        EDITOR_TOKEN,
        "editorial-update-001",
        1,
        Some(updated_request),
    )
    .await;
    assert_eq!(update_replay.status, StatusCode::OK);
    assert_eq!(update_replay.body, updated.body);
    let get = call(
        application.clone(),
        Method::GET,
        "/v1/editorial/today",
        Some(EDITOR_TOKEN),
        None,
        None,
    )
    .await;
    assert_eq!(get.status, StatusCode::OK);
    assert_eq!(get.body, updated.body);
    assert_eq!(etag_version(&get), 2);

    let revision_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM store_editorial_revisions WHERE layout_id = 'today'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE object_kind = 'editorial' AND object_id = 'today'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let outbox_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events \
         WHERE topic = 'catalog.rebuild-requested' \
           AND payload ? 'editorial_resource_version'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!((revision_count, audit_count, outbox_count), (2, 2, 2));
    let revisions = sqlx::query(
        "SELECT resource_version, headline, featured_release_id \
         FROM store_editorial_revisions WHERE layout_id = 'today' \
         ORDER BY resource_version",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(revisions[0].get::<i64, _>("resource_version"), 1);
    assert_eq!(
        revisions[0].get::<String, _>("headline"),
        "Reviewed for 320 x 170"
    );
    assert_eq!(
        revisions[0].get::<String, _>("featured_release_id"),
        FEATURED
    );
    assert_eq!(revisions[1].get::<i64, _>("resource_version"), 2);
    assert_eq!(
        revisions[1].get::<String, _>("featured_release_id"),
        COLLECTION_A
    );
    let audit_actions = sqlx::query_scalar::<_, String>(
        "SELECT action FROM audit_events \
         WHERE object_kind = 'editorial' AND object_id = 'today' \
         ORDER BY resource_version",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(
        audit_actions,
        vec!["editorial.today-created", "editorial.today-updated"]
    );
    let editorial_versions = sqlx::query_scalar::<_, i64>(
        "SELECT (payload->>'editorial_resource_version')::BIGINT \
         FROM outbox_events WHERE payload ? 'editorial_resource_version' \
         ORDER BY (payload->>'editorial_resource_version')::BIGINT",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(editorial_versions, vec![1, 2]);

    assert_sqlstate(
        sqlx::query(
            "UPDATE store_editorial_revisions SET headline = 'Tampered' \
             WHERE layout_id = 'today' AND resource_version = 1",
        )
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("DELETE FROM store_editorial_layouts WHERE layout_id = 'today'")
            .execute(pool)
            .await,
        "55000",
    );
    assert_editorial_tamper_rejected(pool, false, false, "3".repeat(64)).await;
    assert_editorial_tamper_rejected(pool, true, false, "4".repeat(64)).await;
    assert_editorial_tamper_rejected(pool, true, true, "5".repeat(64)).await;
}

async fn seed_editorial_release(
    pool: &PgPool,
    release_id: &str,
    submission_id: &str,
    app_id: &str,
    version: &str,
    catalog_sequence: i64,
) {
    let now: i64 = sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO apps (app_id, owner_team_id, default_locale, created_unix_seconds) \
         VALUES ($1, $2, 'en', $3) ON CONFLICT (app_id) DO NOTHING",
    )
    .bind(app_id)
    .bind(TEAM_A)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO submissions (submission_id, app_id, version, revision, state, \
         package_sha256, package_bytes, listing_sha256, listing_bytes, assets, \
         resource_version, created_unix_seconds) \
         VALUES ($1, $2, $3, 1, 'approved', $4, 100, $5, 100, '[{},{}]'::jsonb, 1, $6)",
    )
    .bind(submission_id)
    .bind(app_id)
    .bind(version)
    .bind("a".repeat(64))
    .bind("b".repeat(64))
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    let mut release_seed = pool.begin().await.unwrap();
    release_seed
        .execute("ALTER TABLE releases DISABLE TRIGGER releases_approved_creation")
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO releases (release_id, submission_id, app_id, version, state, \
         rollout_percent, resource_version, created_unix_seconds) \
         VALUES ($1, $2, $3, $4, 'ready', 100, 1, $5)",
    )
    .bind(release_id)
    .bind(submission_id)
    .bind(app_id)
    .bind(version)
    .bind(now)
    .execute(&mut *release_seed)
    .await
    .unwrap();
    release_seed
        .execute("ALTER TABLE releases ENABLE TRIGGER releases_approved_creation")
        .await
        .unwrap();
    release_seed.commit().await.unwrap();
    sqlx::query(
        "UPDATE releases SET state = 'publishing', resource_version = 2 WHERE release_id = $1",
    )
    .bind(release_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE releases SET state = 'published', catalog_sequence = $2, resource_version = 3 \
         WHERE release_id = $1",
    )
    .bind(release_id)
    .bind(catalog_sequence)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_unapproved_editorial_release(pool: &PgPool, release_id: &str) {
    let now: i64 = sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO apps (app_id, owner_team_id, default_locale, created_unix_seconds) \
         VALUES ('dev.cardputerzero.editorial-unapproved', $1, 'en', $2)",
    )
    .bind(TEAM_A)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO submissions (submission_id, app_id, version, revision, state, \
         package_sha256, package_bytes, listing_sha256, listing_bytes, assets, \
         resource_version, created_unix_seconds) VALUES \
         ('sub_e6e6e6e6e6e6e6e6e6e6e6e6e6e6e6e6', \
          'dev.cardputerzero.editorial-unapproved', '1.0.0', 1, 'ready-for-review', \
          $1, 100, $2, 100, '[{},{}]'::jsonb, 1, $3)",
    )
    .bind("c".repeat(64))
    .bind("d".repeat(64))
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    let mut release_seed = pool.begin().await.unwrap();
    release_seed
        .execute("ALTER TABLE releases DISABLE TRIGGER releases_approved_creation")
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO releases (release_id, submission_id, app_id, version, state, \
         rollout_percent, catalog_sequence, resource_version, created_unix_seconds) VALUES \
         ($1, 'sub_e6e6e6e6e6e6e6e6e6e6e6e6e6e6e6e6', \
          'dev.cardputerzero.editorial-unapproved', '1.0.0', 'published', 100, 906, 3, $2)",
    )
    .bind(release_id)
    .bind(now)
    .execute(&mut *release_seed)
    .await
    .unwrap();
    release_seed
        .execute("ALTER TABLE releases ENABLE TRIGGER releases_approved_creation")
        .await
        .unwrap();
    release_seed.commit().await.unwrap();
}

async fn assert_editorial_tamper_rejected(
    pool: &PgPool,
    include_revision: bool,
    include_audit: bool,
    request_sha256: String,
) {
    let mut transaction = pool.begin().await.unwrap();
    let now: i64 = sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE store_editorial_layouts SET headline = 'Tampered layout', \
         resource_version = 3, updated_unix_seconds = $1 WHERE layout_id = 'today'",
    )
    .bind(now)
    .execute(&mut *transaction)
    .await
    .unwrap();
    if include_revision {
        sqlx::query(
            "INSERT INTO store_editorial_revisions (layout_id, resource_version, operator_id, \
             headline, featured_release_id, featured_app_id, collections, request_sha256, \
             created_unix_seconds) SELECT layout_id, resource_version, $1, headline, \
             featured_release_id, featured_app_id, collections, $2, updated_unix_seconds \
             FROM store_editorial_layouts WHERE layout_id = 'today'",
        )
        .bind(EDITOR)
        .bind(&request_sha256)
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    if include_audit {
        sqlx::query(
            "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
             object_id, before_state, after_state, resource_version, request_id, request_sha256, \
             idempotency_key_sha256) VALUES \
             ($1, $2, 'editorial.today-updated', 'editorial', 'today', NULL, 'active', 3, \
              'req_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', $3, $4)",
        )
        .bind(now)
        .bind(EDITOR)
        .bind(&request_sha256)
        .bind("6".repeat(64))
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    assert_sqlstate(transaction.commit().await, "55000");
}

async fn verify_database_immutability(pool: &PgPool) {
    assert_sqlstate(
        sqlx::query(
            "UPDATE oauth_device_authorizations SET state = 'approved' WHERE state = 'consumed'",
        )
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("DELETE FROM oauth_device_authorizations WHERE state = 'denied'")
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("DELETE FROM access_tokens WHERE token_sha256 = $1")
            .bind(sha256_hex(OWNER_A_TOKEN.as_bytes()))
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("UPDATE apps SET owner_team_id = $1 WHERE app_id = 'dev.cardputerzero.notes'")
            .bind(TEAM_B)
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("UPDATE audit_events SET action = 'app.changed' WHERE sequence = 1")
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("DELETE FROM audit_events WHERE sequence = 1")
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "UPDATE submission_upload_chunks SET chunk_bytes = chunk_bytes + 1 \
             WHERE part_name = 'package' AND chunk_offset = 0",
        )
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "UPDATE submission_upload_parts SET expected_bytes = expected_bytes + 1 \
             WHERE part_name = 'package'",
        )
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "UPDATE submissions SET finalized_content_sha256 = $1 \
             WHERE finalized_content_sha256 IS NOT NULL",
        )
        .bind("f".repeat(64))
        .execute(pool)
        .await,
        "55000",
    );

    sqlx::query(
        "INSERT INTO submissions (submission_id, app_id, version, revision, state, package_sha256, \
         package_bytes, listing_sha256, listing_bytes, assets, resource_version, created_unix_seconds) \
         VALUES ('sub_11111111111111111111111111111111', 'dev.cardputerzero.notes', '9.9.9', 1, \
         'ready-for-review', $1, 100, $2, 100, '[{},{}]'::jsonb, 1, 1)",
    )
    .bind("1".repeat(64))
    .bind("2".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_assignments (assignment_id, submission_id, reviewer_id, \
         assignment_kind, state, source_resource_version, created_unix_seconds) VALUES \
         ('assignment_11111111111111111111111111111111', \
         'sub_11111111111111111111111111111111', $1, 'primary', 'active', 1, 1)",
    )
    .bind(REVIEWER_A)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE submissions SET state = 'in-review', resource_version = 2 \
         WHERE submission_id = 'sub_11111111111111111111111111111111'",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_messages (message_id, submission_id, actor_id, actor_kind, body, \
         created_unix_seconds) \
         VALUES ('msg_11111111111111111111111111111111', \
         'sub_11111111111111111111111111111111', $1, 'reviewer', 'Review note', 1)",
    )
    .bind(REVIEWER_A)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_decisions (decision_id, submission_id, reviewer_id, decision, \
         reason_codes, note, created_unix_seconds, assignment_id) VALUES \
         ('decision_11111111111111111111111111111111', \
         'sub_11111111111111111111111111111111', $1, 'approved', '{}', '', 1, \
         'assignment_11111111111111111111111111111111')",
    )
    .bind(REVIEWER_A)
    .execute(pool)
    .await
    .unwrap();

    assert_sqlstate(
        sqlx::query(
            "UPDATE submissions SET package_bytes = 101 \
             WHERE submission_id = 'sub_11111111111111111111111111111111'",
        )
        .execute(pool)
        .await,
        "55000",
    );
    sqlx::query(
        "INSERT INTO submissions (submission_id, app_id, version, revision, state, package_sha256, \
         package_bytes, listing_sha256, listing_bytes, assets, resource_version, created_unix_seconds) \
         VALUES ('sub_33333333333333333333333333333333', 'dev.cardputerzero.notes', '9.9.8', 1, \
         'approved', $1, 100, $2, 100, '[{},{}]'::jsonb, 1, 1)",
    )
    .bind("1".repeat(64))
    .bind("2".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    assert_sqlstate(
        sqlx::query(
            "INSERT INTO releases (release_id, submission_id, app_id, version, state, \
             rollout_percent, resource_version, created_unix_seconds) VALUES \
             ('rel_33333333333333333333333333333333', \
              'sub_33333333333333333333333333333333', \
              'dev.cardputerzero.notes', '9.9.8', 'ready', 100, 1, 1)",
        )
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "UPDATE review_messages SET body = 'Changed' \
             WHERE message_id = 'msg_11111111111111111111111111111111'",
        )
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "DELETE FROM review_decisions \
             WHERE decision_id = 'decision_11111111111111111111111111111111'",
        )
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("UPDATE team_members SET team_id = $1 WHERE member_id = $2")
            .bind(TEAM_B)
            .bind(VIEWER_MEMBER)
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("UPDATE access_tokens SET revoked = FALSE WHERE token_sha256 = $1")
            .bind(sha256_hex(REVOKED_TOKEN.as_bytes()))
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "UPDATE access_tokens SET mfa_authenticated_unix_seconds = created_unix_seconds \
             WHERE token_sha256 = $1",
        )
        .bind(sha256_hex(STALE_MFA_TOKEN.as_bytes()))
        .execute(pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("UPDATE teams SET resource_version = resource_version + 2 WHERE team_id = $1")
            .bind(TEAM_A)
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("UPDATE team_members SET role = 'viewer' WHERE member_id = $1")
            .bind(RELEASE_MANAGER)
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("UPDATE reviewer_access_tokens SET revoked = FALSE WHERE token_sha256 = $1")
            .bind(sha256_hex(REVIEWER_REVOKED_TOKEN.as_bytes()))
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("DELETE FROM reviewers WHERE reviewer_id = $1")
            .bind(REVIEWER_A)
            .execute(pool)
            .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "INSERT INTO reviewer_access_tokens (token_sha256, reviewer_id, scopes, \
             expires_unix_seconds, revoked, created_unix_seconds) \
             VALUES ($1, $2, ARRAY['store.review'], 2, FALSE, 1)",
        )
        .bind(sha256_hex(OWNER_A_TOKEN.as_bytes()))
        .bind(REVIEWER_A)
        .execute(pool)
        .await,
        "23505",
    );
    assert_sqlstate(
        sqlx::query(
            "UPDATE review_assignments SET state = 'cancelled' \
             WHERE submission_id = 'sub_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
        )
        .execute(pool)
        .await,
        "55000",
    );
    sqlx::query(
        "INSERT INTO review_assignments (assignment_id, submission_id, reviewer_id, \
         assignment_kind, state, source_resource_version, created_unix_seconds) VALUES \
         ('assignment_22222222222222222222222222222222', $1, $2, \
          'primary', 'active', 1, 1)",
    )
    .bind("sub_cccccccccccccccccccccccccccccccc")
    .bind(REVIEWER_A)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE submissions SET state = 'in-review', resource_version = 2 \
         WHERE submission_id = $1",
    )
    .bind("sub_cccccccccccccccccccccccccccccccc")
    .execute(pool)
    .await
    .unwrap();
    assert_sqlstate(
        sqlx::query(
            "INSERT INTO review_decisions (decision_id, submission_id, reviewer_id, decision, \
             reason_codes, note, created_unix_seconds, assignment_id) VALUES \
             ('decision_22222222222222222222222222222222', \
             $2, $1, 'rejected', \
             ARRAY['privacy', 'privacy'], 'Duplicate reasons', 1, \
             'assignment_22222222222222222222222222222222')",
        )
        .bind(REVIEWER_A)
        .bind("sub_cccccccccccccccccccccccccccccccc")
        .execute(pool)
        .await,
        "23514",
    );
    assert_sqlstate(
        sqlx::query(
            "UPDATE team_members SET role = 'developer', \
             resource_version = resource_version + 1 WHERE member_id = $1",
        )
        .bind(OWNER_B)
        .execute(pool)
        .await,
        "23514",
    );
}

async fn reset_database(pool: &PgPool) {
    pool.execute("DROP TRIGGER IF EXISTS test_reject_audit_insert ON audit_events")
        .await
        .unwrap();
    pool.execute("DROP FUNCTION IF EXISTS test_reject_audit()")
        .await
        .unwrap();
    pool.execute("DROP TRIGGER IF EXISTS test_reject_withdraw_audit_insert ON audit_events")
        .await
        .unwrap();
    pool.execute("DROP FUNCTION IF EXISTS test_reject_withdraw_audit()")
        .await
        .unwrap();
    pool.execute("DROP TRIGGER IF EXISTS test_reject_team_role_audit_insert ON audit_events")
        .await
        .unwrap();
    pool.execute("DROP FUNCTION IF EXISTS test_reject_team_role_audit()")
        .await
        .unwrap();
    pool.execute(
        "DROP TRIGGER IF EXISTS test_reject_secondary_review_audit_insert ON audit_events",
    )
    .await
    .unwrap();
    pool.execute("DROP FUNCTION IF EXISTS test_reject_secondary_review_audit()")
        .await
        .unwrap();
    pool.execute(
        "TRUNCATE store_catalog_snapshots, store_publication_jobs, \
         store_editorial_revisions, store_editorial_layouts, \
         store_operator_access_tokens, store_operators, \
         outbox_events, audit_events, idempotency_records, oauth_device_authorizations, \
         release_operations, \
         submission_upload_chunks, submission_upload_parts, releases, \
         review_decisions, review_messages, review_assignments, submissions, apps, developer_keys, \
         reviewer_access_tokens, reviewers, access_tokens, team_members, teams, catalog_sequence \
         RESTART IDENTITY CASCADE",
    )
    .await
    .unwrap();
    pool.execute("INSERT INTO catalog_sequence (singleton, last_sequence) VALUES (TRUE, 0)")
        .await
        .unwrap();
}

fn test_object_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-store-control")
        .join(Uuid::new_v4().simple().to_string())
}

async fn seed_identities(pool: &PgPool) {
    sqlx::query("INSERT INTO teams (team_id, name) VALUES ($1, 'Team A'), ($2, 'Team B')")
        .bind(TEAM_A)
        .bind(TEAM_B)
        .execute(pool)
        .await
        .unwrap();
    for (member_id, team_id, email, role, two_factor) in [
        (OWNER_A, TEAM_A, "owner-a@example.com", "owner", true),
        (OWNER_B, TEAM_B, "owner-b@example.com", "owner", true),
        (
            NO_2FA_MEMBER,
            TEAM_A,
            "no-2fa@example.com",
            "developer",
            false,
        ),
        (VIEWER_MEMBER, TEAM_A, "viewer@example.com", "viewer", true),
        (
            RELEASE_MANAGER,
            TEAM_A,
            "release@example.com",
            "release-manager",
            true,
        ),
        (
            RELEASE_NO_2FA,
            TEAM_A,
            "release-no2fa@example.com",
            "release-manager",
            false,
        ),
    ] {
        sqlx::query(
            "INSERT INTO team_members (member_id, team_id, email, role, two_factor_enabled) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(member_id)
        .bind(team_id)
        .bind(email)
        .bind(role)
        .bind(two_factor)
        .execute(pool)
        .await
        .unwrap();
    }

    let now: i64 = sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(pool)
        .await
        .unwrap();
    for (token, member_id, scopes, created, expires, revoked, mfa_authenticated) in [
        (
            OWNER_A_TOKEN,
            OWNER_A,
            vec![
                "store.apps.write",
                "store.submit",
                "store.release",
                "store.teams.read",
                "store.teams.write",
            ],
            now,
            now + 3600,
            false,
            Some(now),
        ),
        (
            OWNER_B_TOKEN,
            OWNER_B,
            vec![
                "store.apps.write",
                "store.submit",
                "store.release",
                "store.teams.read",
                "store.teams.write",
            ],
            now,
            now + 3600,
            false,
            Some(now),
        ),
        (
            NO_2FA_TOKEN,
            NO_2FA_MEMBER,
            vec!["store.apps.write", "store.submit", "store.teams.write"],
            now,
            now + 3600,
            false,
            None,
        ),
        (
            VIEWER_TOKEN,
            VIEWER_MEMBER,
            vec![
                "store.apps.write",
                "store.submit",
                "store.teams.read",
                "store.teams.write",
            ],
            now,
            now + 3600,
            false,
            Some(now),
        ),
        (
            READ_ONLY_TOKEN,
            OWNER_A,
            vec!["store.apps.read"],
            now,
            now + 3600,
            false,
            Some(now),
        ),
        (
            EXPIRED_TOKEN,
            OWNER_A,
            vec!["store.apps.write"],
            now - 3601,
            now - 1,
            false,
            None,
        ),
        (
            REVOKED_TOKEN,
            OWNER_A,
            vec!["store.apps.write"],
            now,
            now + 3600,
            true,
            Some(now),
        ),
        (
            RELEASE_MANAGER_TOKEN,
            RELEASE_MANAGER,
            vec!["store.release"],
            now,
            now + 3600,
            false,
            Some(now),
        ),
        (
            RELEASE_NO_2FA_TOKEN,
            RELEASE_NO_2FA,
            vec!["store.release"],
            now,
            now + 3600,
            false,
            None,
        ),
        (
            STALE_MFA_TOKEN,
            OWNER_A,
            vec!["store.teams.read", "store.teams.write"],
            now,
            now + 3600,
            false,
            Some(now - 301),
        ),
    ] {
        let scopes = scopes.into_iter().map(str::to_owned).collect::<Vec<_>>();
        sqlx::query(
            "INSERT INTO access_tokens (token_sha256, member_id, scopes, expires_unix_seconds, \
             revoked, created_unix_seconds, mfa_authenticated_unix_seconds) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(sha256_hex(token.as_bytes()))
        .bind(member_id)
        .bind(scopes)
        .bind(expires)
        .bind(revoked)
        .bind(created)
        .bind(mfa_authenticated)
        .execute(pool)
        .await
        .unwrap();
    }

    for (reviewer_id, email, role, two_factor) in [
        (REVIEWER_A, "reviewer-a@cardputerzero.dev", "reviewer", true),
        (
            REVIEWER_B,
            "reviewer-b@cardputerzero.dev",
            "senior-reviewer",
            true,
        ),
        (
            REVIEWER_NO_2FA,
            "reviewer-no2fa@cardputerzero.dev",
            "reviewer",
            false,
        ),
    ] {
        sqlx::query(
            "INSERT INTO reviewers (reviewer_id, email, role, two_factor_enabled, state, \
             created_unix_seconds) VALUES ($1, $2, $3, $4, 'active', $5)",
        )
        .bind(reviewer_id)
        .bind(email)
        .bind(role)
        .bind(two_factor)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
    }
    for (token, reviewer_id, created, expires, revoked) in [
        (REVIEWER_A_TOKEN, REVIEWER_A, now, now + 3600, false),
        (REVIEWER_B_TOKEN, REVIEWER_B, now, now + 3600, false),
        (
            REVIEWER_NO_2FA_TOKEN,
            REVIEWER_NO_2FA,
            now,
            now + 3600,
            false,
        ),
        (
            REVIEWER_EXPIRED_TOKEN,
            REVIEWER_A,
            now - 3601,
            now - 1,
            false,
        ),
        (REVIEWER_REVOKED_TOKEN, REVIEWER_A, now, now + 3600, true),
    ] {
        sqlx::query(
            "INSERT INTO reviewer_access_tokens (token_sha256, reviewer_id, scopes, \
             expires_unix_seconds, revoked, created_unix_seconds) \
             VALUES ($1, $2, ARRAY['store.review'], $3, $4, $5)",
        )
        .bind(sha256_hex(token.as_bytes()))
        .bind(reviewer_id)
        .bind(expires)
        .bind(revoked)
        .bind(created)
        .execute(pool)
        .await
        .unwrap();
    }

    for (operator_id, email, role, two_factor, state) in [
        (EDITOR, "editor@cardputerzero.dev", "editor", true, "active"),
        (
            EDITOR_NO_2FA,
            "editor-no2fa@cardputerzero.dev",
            "editor",
            false,
            "active",
        ),
        (
            EDITOR_SUSPENDED,
            "editor-suspended@cardputerzero.dev",
            "admin",
            true,
            "suspended",
        ),
    ] {
        sqlx::query(
            "INSERT INTO store_operators (operator_id, email, role, two_factor_enabled, state, \
             created_unix_seconds) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(operator_id)
        .bind(email)
        .bind(role)
        .bind(two_factor)
        .bind(state)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
    }
    for (token, operator_id, created, expires, revoked) in [
        (EDITOR_TOKEN, EDITOR, now, now + 3600, false),
        (EDITOR_NO_2FA_TOKEN, EDITOR_NO_2FA, now, now + 3600, false),
        (
            EDITOR_SUSPENDED_TOKEN,
            EDITOR_SUSPENDED,
            now,
            now + 3600,
            false,
        ),
        (EDITOR_EXPIRED_TOKEN, EDITOR, now - 3601, now - 1, false),
        (EDITOR_REVOKED_TOKEN, EDITOR, now, now + 3600, true),
    ] {
        sqlx::query(
            "INSERT INTO store_operator_access_tokens (token_sha256, operator_id, scopes, \
             expires_unix_seconds, revoked, created_unix_seconds) \
             VALUES ($1, $2, ARRAY['store.editorial'], $3, $4, $5)",
        )
        .bind(sha256_hex(token.as_bytes()))
        .bind(operator_id)
        .bind(expires)
        .bind(revoked)
        .bind(created)
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn call(
    application: Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    idempotency_key: Option<&str>,
    body: Option<Value>,
) -> HttpResult {
    let bytes = body.map(|value| serde_json::to_vec(&value).unwrap());
    call_bytes(application, method, uri, token, idempotency_key, bytes).await
}

#[allow(clippy::too_many_arguments)]
async fn call_with_etag(
    application: Router,
    method: Method,
    uri: &str,
    token: &str,
    idempotency_key: &str,
    expected_version: u64,
    body: Option<Value>,
) -> HttpResult {
    let bytes = body.map(|value| serde_json::to_vec(&value).unwrap());
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header("idempotency-key", idempotency_key)
        .header(IF_MATCH, format!("\"{expected_version}\""));
    if bytes.is_some() {
        builder = builder.header(CONTENT_TYPE, "application/json");
    }
    let response = application
        .oneshot(builder.body(Body::from(bytes.unwrap_or_default())).unwrap())
        .await
        .unwrap();
    collect_response(response).await
}

#[allow(clippy::too_many_arguments)]
async fn upload(
    application: Router,
    submission_id: &str,
    part_name: &str,
    expected_version: u64,
    offset: usize,
    total: usize,
    body: &[u8],
    chunk_sha256: &str,
    idempotency_key: &str,
) -> HttpResult {
    let end = offset + body.len() - 1;
    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/v1/submissions/{submission_id}/parts/{part_name}"))
        .header(AUTHORIZATION, format!("Bearer {OWNER_A_TOKEN}"))
        .header("idempotency-key", idempotency_key)
        .header(IF_MATCH, format!("\"{expected_version}\""))
        .header("content-sha256", chunk_sha256)
        .header("content-range", format!("bytes {offset}-{end}/{total}"))
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(body.to_vec()))
        .unwrap();
    collect_response(application.oneshot(request).await.unwrap()).await
}

async fn finalize(
    application: Router,
    submission_id: &str,
    expected_version: u64,
    content_sha256: &str,
    idempotency_key: &str,
) -> HttpResult {
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/submissions/{submission_id}:finalize"))
        .header(AUTHORIZATION, format!("Bearer {OWNER_A_TOKEN}"))
        .header("idempotency-key", idempotency_key)
        .header(IF_MATCH, format!("\"{expected_version}\""))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({"content_sha256": content_sha256})).unwrap(),
        ))
        .unwrap();
    collect_response(application.oneshot(request).await.unwrap()).await
}

async fn call_bytes(
    application: Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    idempotency_key: Option<&str>,
    body: Option<Vec<u8>>,
) -> HttpResult {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(key) = idempotency_key {
        builder = builder.header("idempotency-key", key);
    }
    if body.is_some() {
        builder = builder.header(CONTENT_TYPE, "application/json");
    }
    let http_response = application
        .oneshot(builder.body(Body::from(body.unwrap_or_default())).unwrap())
        .await
        .unwrap();
    collect_response(http_response).await
}

async fn collect_response(response: axum::response::Response) -> HttpResult {
    let (parts, body) = response.into_parts();
    HttpResult {
        status: parts.status,
        headers: parts.headers,
        body: to_bytes(body, 64 * 1024).await.unwrap().to_vec(),
    }
}

fn etag_version(result: &HttpResult) -> u64 {
    result
        .headers
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix('"'))
        .and_then(|value| value.strip_suffix('"'))
        .and_then(|value| value.parse().ok())
        .expect("response must contain a canonical ETag")
}

fn assert_problem(result: &HttpResult, status: StatusCode, code: &str) {
    assert_eq!(result.status, status);
    assert_eq!(
        result.headers.get(CONTENT_TYPE).unwrap(),
        "application/problem+json"
    );
    let value: Value = serde_json::from_slice(&result.body).unwrap();
    assert_eq!(value["status"], status.as_u16());
    assert_eq!(value["code"], code);
    assert!(value["request_id"].as_str().unwrap().starts_with("req_"));
    assert!(result.body.len() <= 1024);
}

fn assert_oauth_headers(result: &HttpResult) {
    assert_eq!(result.headers.get("cache-control").unwrap(), "no-store");
    assert_eq!(result.headers.get("pragma").unwrap(), "no-cache");
}

fn assert_sqlstate<T: std::fmt::Debug>(result: Result<T, sqlx::Error>, expected: &str) {
    let error = result.expect_err("database mutation should be rejected");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some(expected)
    );
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    let query = format!("SELECT count(*) FROM {table}");
    sqlx::query_scalar(&query).fetch_one(pool).await.unwrap()
}

fn sha256_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn submission_content_sha256_for_test(
    package_sha256: &str,
    listing_sha256: &str,
    assets: &[Value],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"CardputerZero Store submission content v1\0");
    hash_field(&mut hasher, package_sha256.as_bytes());
    hash_field(&mut hasher, listing_sha256.as_bytes());
    for asset in assets {
        hash_field(&mut hasher, asset["path"].as_str().unwrap().as_bytes());
        hash_field(&mut hasher, asset["sha256"].as_str().unwrap().as_bytes());
        hasher.update(asset["bytes"].as_u64().unwrap().to_be_bytes());
        hasher.update(
            u16::try_from(asset["width"].as_u64().unwrap())
                .unwrap()
                .to_be_bytes(),
        );
        hasher.update(
            u16::try_from(asset["height"].as_u64().unwrap())
                .unwrap()
                .to_be_bytes(),
        );
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}
