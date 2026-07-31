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

const TEAM_A: &str = "team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TEAM_B: &str = "team_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OWNER_A: &str = "member_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OWNER_B: &str = "member_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NO_2FA_MEMBER: &str = "member_cccccccccccccccccccccccccccccccc";
const VIEWER_MEMBER: &str = "member_dddddddddddddddddddddddddddddddd";

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

async fn verify_database_immutability(pool: &PgPool) {
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
         'in-review', $1, 100, $2, 100, '[{},{}]'::jsonb, 1, 1)",
    )
    .bind("1".repeat(64))
    .bind("2".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_messages (message_id, submission_id, actor_id, body, created_unix_seconds) \
         VALUES ('msg_11111111111111111111111111111111', \
         'sub_11111111111111111111111111111111', 'reviewer-1', 'Review note', 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_decisions (decision_id, submission_id, reviewer_id, decision, \
         reason_codes, note, created_unix_seconds) VALUES \
         ('decision_11111111111111111111111111111111', \
         'sub_11111111111111111111111111111111', 'reviewer-1', 'approved', '{}', '', 1)",
    )
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
        sqlx::query("UPDATE team_members SET role = 'developer' WHERE member_id = $1")
            .bind(OWNER_B)
            .execute(pool)
            .await,
        "23514",
    );
}

async fn reset_database(pool: &PgPool) {
    pool.execute(
        "TRUNCATE outbox_events, audit_events, idempotency_records, \
         submission_upload_chunks, submission_upload_parts, releases, \
         review_decisions, review_messages, submissions, apps, developer_keys, access_tokens, \
         team_members, teams, catalog_sequence RESTART IDENTITY CASCADE",
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
    for (token, member_id, scopes, created, expires, revoked) in [
        (
            OWNER_A_TOKEN,
            OWNER_A,
            vec!["store.apps.write", "store.submit"],
            now,
            now + 3600,
            false,
        ),
        (
            OWNER_B_TOKEN,
            OWNER_B,
            vec!["store.apps.write", "store.submit"],
            now,
            now + 3600,
            false,
        ),
        (
            NO_2FA_TOKEN,
            NO_2FA_MEMBER,
            vec!["store.apps.write", "store.submit"],
            now,
            now + 3600,
            false,
        ),
        (
            VIEWER_TOKEN,
            VIEWER_MEMBER,
            vec!["store.apps.write", "store.submit"],
            now,
            now + 3600,
            false,
        ),
        (
            READ_ONLY_TOKEN,
            OWNER_A,
            vec!["store.apps.read"],
            now,
            now + 3600,
            false,
        ),
        (
            EXPIRED_TOKEN,
            OWNER_A,
            vec!["store.apps.write"],
            now - 3601,
            now - 1,
            false,
        ),
        (
            REVOKED_TOKEN,
            OWNER_A,
            vec!["store.apps.write"],
            now,
            now + 3600,
            true,
        ),
    ] {
        let scopes = scopes.into_iter().map(str::to_owned).collect::<Vec<_>>();
        sqlx::query(
            "INSERT INTO access_tokens (token_sha256, member_id, scopes, expires_unix_seconds, \
             revoked, created_unix_seconds) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(sha256_hex(token.as_bytes()))
        .bind(member_id)
        .bind(scopes)
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
