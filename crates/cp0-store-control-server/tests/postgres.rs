use std::env;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, ETAG};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use cp0_store_control::register_app_request_sha256;
use cp0_store_control_server::{StoreControlService, connect, migrate, router};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPool;
use sqlx::{Executor, Row};
use tower::ServiceExt;

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

    let application = router(StoreControlService::new(pool.clone()));
    verify_exact_replay(&application, &pool).await;
    verify_authorization_and_limits(&application, &pool).await;
    verify_concurrent_claim(&application, &pool).await;
    verify_atomic_rollback(&application, &pool).await;
    verify_database_immutability(&pool).await;
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

    sqlx::query(
        "INSERT INTO submissions (submission_id, app_id, version, revision, state, package_sha256, \
         package_bytes, listing_sha256, listing_bytes, assets, resource_version, created_unix_seconds) \
         VALUES ('sub_11111111111111111111111111111111', 'dev.cardputerzero.notes', '1.0.0', 1, \
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
        "TRUNCATE outbox_events, audit_events, idempotency_records, releases, \
         review_decisions, review_messages, submissions, apps, developer_keys, access_tokens, \
         team_members, teams, catalog_sequence RESTART IDENTITY CASCADE",
    )
    .await
    .unwrap();
    pool.execute("INSERT INTO catalog_sequence (singleton, last_sequence) VALUES (TRUE, 0)")
        .await
        .unwrap();
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
            vec!["store.apps.write"],
            now,
            now + 3600,
            false,
        ),
        (
            OWNER_B_TOKEN,
            OWNER_B,
            vec!["store.apps.write"],
            now,
            now + 3600,
            false,
        ),
        (
            NO_2FA_TOKEN,
            NO_2FA_MEMBER,
            vec!["store.apps.write"],
            now,
            now + 3600,
            false,
        ),
        (
            VIEWER_TOKEN,
            VIEWER_MEMBER,
            vec!["store.apps.write"],
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
    let response = application
        .oneshot(builder.body(Body::from(body.unwrap_or_default())).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    HttpResult {
        status: parts.status,
        headers: parts.headers,
        body: to_bytes(body, 64 * 1024).await.unwrap().to_vec(),
    }
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
