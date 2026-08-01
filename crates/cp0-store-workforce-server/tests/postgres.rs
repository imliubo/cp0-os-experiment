use std::env;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::{AUTHORIZATION, LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cp0_store_control_server::{StoreControlService, router as control_router};
use cp0_store_portal_server::{AuthIntent, OidcError, OidcFuture, OidcProvider, VerifiedIdentity};
use cp0_store_workforce_server::{WorkforceSecrets, WorkforceService, connect, migrate, router};
use serde_json::Value;
use sqlx::{Executor, Row};
use tower::ServiceExt;
use url::Url;

const REVIEW_ORIGIN: &str = "https://review.cardputerzero.dev";
const OPERATIONS_ORIGIN: &str = "https://operations.cardputerzero.dev";
const REVIEW_ISSUER: &str = "https://review-identity.example.com";
const OPERATIONS_ISSUER: &str = "https://operations-identity.example.com";
const REVIEW_SUBJECT: &str = "raw-review-subject-must-never-be-persisted";
const ADMIN_SUBJECT: &str = "raw-admin-subject-must-never-be-persisted";
const EDITOR_SUBJECT: &str = "raw-editor-subject-must-never-be-persisted";
const REVIEWER_ID: &str = "reviewer_11111111111111111111111111111111";
const ADMIN_ID: &str = "operator_22222222222222222222222222222222";
const EDITOR_ID: &str = "operator_33333333333333333333333333333333";
const REVIEW_LINK_ID: &str = "wflink_11111111111111111111111111111111";
const ADMIN_LINK_ID: &str = "wflink_22222222222222222222222222222222";
const EDITOR_LINK_ID: &str = "wflink_33333333333333333333333333333333";

#[derive(Clone, Copy)]
struct FakeProvider {
    issuer: &'static str,
    config_sha256: &'static str,
    default_subject: &'static str,
    editor_subject: Option<&'static str>,
}

impl OidcProvider for FakeProvider {
    fn key(&self) -> &str {
        "primary"
    }

    fn issuer(&self) -> &str {
        self.issuer
    }

    fn config_sha256(&self) -> &str {
        self.config_sha256
    }

    fn authorization_uri(
        &self,
        intent: AuthIntent,
        state: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<String, OidcError> {
        if intent != AuthIntent::StepUp || pkce_verifier.len() != 43 {
            return Err(OidcError::InvalidRequest);
        }
        Ok(format!(
            "{}/authorize?state={state}&nonce={nonce}",
            self.issuer
        ))
    }

    fn exchange<'a>(
        &'a self,
        intent: AuthIntent,
        code: &'a str,
        nonce: &'a str,
        pkce_verifier: &'a str,
        now: i64,
    ) -> OidcFuture<'a> {
        Box::pin(async move {
            if intent != AuthIntent::StepUp
                || !code.starts_with("valid-")
                || nonce.len() != 43
                || pkce_verifier.len() != 43
            {
                return Err(OidcError::InvalidToken);
            }
            let subject = if code.contains("editor") {
                self.editor_subject.ok_or(OidcError::InvalidToken)?
            } else {
                self.default_subject
            };
            Ok(VerifiedIdentity {
                issuer: self.issuer.to_owned(),
                subject: subject.to_owned(),
                email: format!("{subject}@example.com"),
                email_verified: true,
                mfa_authenticated_unix_seconds: Some(now),
            })
        })
    }
}

const REVIEW_PROVIDER: FakeProvider = FakeProvider {
    issuer: REVIEW_ISSUER,
    config_sha256: "1111111111111111111111111111111111111111111111111111111111111111",
    default_subject: REVIEW_SUBJECT,
    editor_subject: None,
};

const OPERATIONS_PROVIDER: FakeProvider = FakeProvider {
    issuer: OPERATIONS_ISSUER,
    config_sha256: "2222222222222222222222222222222222222222222222222222222222222222",
    default_subject: ADMIN_SUBJECT,
    editor_subject: Some(EDITOR_SUBJECT),
};

struct HttpResult {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires CP0_STORE_TEST_DATABASE_URL"]
async fn workforce_bff_acceptance() {
    let database_url = env::var("CP0_STORE_TEST_DATABASE_URL")
        .expect("CP0_STORE_TEST_DATABASE_URL must be set for the database gate");
    let pool = connect(&database_url, 8).await.unwrap();
    migrate(&pool).await.unwrap();
    reset(&pool).await;
    let secrets = secrets();
    seed_workforce(&pool, &secrets).await;
    let workforce = router(
        WorkforceService::new(
            pool.clone(),
            secrets,
            vec![Arc::new(REVIEW_PROVIDER)],
            vec![Arc::new(OPERATIONS_PROVIDER)],
            REVIEW_ORIGIN.to_owned(),
            format!("{REVIEW_ORIGIN}/queue"),
            OPERATIONS_ORIGIN.to_owned(),
            format!("{OPERATIONS_ORIGIN}/console"),
        )
        .unwrap(),
    );
    let control = control_router(StoreControlService::new(pool.clone()));

    let review_state = begin_login(&workforce, "/review/auth/login?provider=primary").await;
    let operations_state = begin_login(&workforce, "/operations/auth/login?provider=primary").await;
    let mismatched = call(
        &workforce,
        Method::GET,
        &format!("/operations/auth/callback?code=valid-review-code-0001&state={review_state}"),
        &[],
        &[],
    )
    .await;
    assert_eq!(mismatched.status, StatusCode::UNAUTHORIZED);

    let review_callback = callback(
        &workforce,
        "/review/auth/callback",
        "valid-review-code-0001",
        &review_state,
        &format!("{REVIEW_ORIGIN}/queue"),
    )
    .await;
    let review_cookie = session_cookie(&review_callback, "__Host-cp0_review");
    let operations_callback = callback(
        &workforce,
        "/operations/auth/callback",
        "valid-admin-code-0001",
        &operations_state,
        &format!("{OPERATIONS_ORIGIN}/console"),
    )
    .await;
    let operations_cookie = session_cookie(&operations_callback, "__Host-cp0_operations");

    let cross_cookie = call(
        &workforce,
        Method::GET,
        "/operations/v1/session",
        &[("cookie", &review_cookie)],
        &[],
    )
    .await;
    assert_eq!(cross_cookie.status, StatusCode::UNAUTHORIZED);
    let review_session = get_session(&workforce, "/review/v1/session", &review_cookie).await;
    assert_eq!(review_session["principal_id"], REVIEWER_ID);
    assert_eq!(review_session["audience"], "review");
    assert!(review_session.get("allowed_scopes").is_none());
    let review_csrf = review_session["csrf_token"].as_str().unwrap().to_owned();
    let operations_session =
        get_session(&workforce, "/operations/v1/session", &operations_cookie).await;
    assert_eq!(operations_session["principal_id"], ADMIN_ID);
    assert_eq!(operations_session["audience"], "operations");
    assert_eq!(
        operations_session["allowed_scopes"],
        serde_json::json!(["store.editorial", "store.moderation"])
    );
    let operations_csrf = operations_session["csrf_token"]
        .as_str()
        .unwrap()
        .to_owned();

    let review_token_result = issue_review_token(
        &workforce,
        &review_cookie,
        &review_csrf,
        "review-token-request-0001",
    )
    .await;
    assert_eq!(review_token_result.status, StatusCode::OK);
    let review_token_body: Value = serde_json::from_slice(&review_token_result.body).unwrap();
    assert_eq!(review_token_body["audience"], "review");
    assert_eq!(review_token_body["scope"], "store.review");
    assert!(review_token_body["expires_in"].as_i64().unwrap() <= 300);
    let review_token = review_token_body["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let replay = issue_review_token(
        &workforce,
        &review_cookie,
        &review_csrf,
        "review-token-request-0001",
    )
    .await;
    assert_eq!(replay.status, StatusCode::OK);
    let replay_body: Value = serde_json::from_slice(&replay.body).unwrap();
    assert_eq!(replay_body["access_token"], review_token);
    let issuance_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workforce_control_token_issuances WHERE audience = 'review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(issuance_count, 1);

    let review_control =
        control_get(&control, "/v1/review/submissions?limit=1", &review_token).await;
    assert_eq!(review_control.status, StatusCode::OK);

    let editorial = issue_operations_token(
        &workforce,
        &operations_cookie,
        &operations_csrf,
        "operations-token-request-0001",
        "store.editorial",
    )
    .await;
    assert_eq!(editorial.status, StatusCode::OK);
    let editorial_body: Value = serde_json::from_slice(&editorial.body).unwrap();
    let editorial_token = editorial_body["access_token"].as_str().unwrap();
    assert_eq!(editorial_body["audience"], "operations");
    assert_eq!(editorial_body["scope"], "store.editorial");
    assert_eq!(
        control_get(&control, "/v1/editorial/releases?limit=1", editorial_token,)
            .await
            .status,
        StatusCode::OK
    );
    let changed_scope = issue_operations_token(
        &workforce,
        &operations_cookie,
        &operations_csrf,
        "operations-token-request-0001",
        "store.moderation",
    )
    .await;
    assert_eq!(changed_scope.status, StatusCode::CONFLICT);

    let editor_state = begin_login(&workforce, "/operations/auth/login?provider=primary").await;
    let editor_callback = callback(
        &workforce,
        "/operations/auth/callback",
        "valid-editor-code-0001",
        &editor_state,
        &format!("{OPERATIONS_ORIGIN}/console"),
    )
    .await;
    let editor_cookie = session_cookie(&editor_callback, "__Host-cp0_operations");
    let editor_session = get_session(&workforce, "/operations/v1/session", &editor_cookie).await;
    assert_eq!(editor_session["principal_id"], EDITOR_ID);
    assert_eq!(
        editor_session["allowed_scopes"],
        serde_json::json!(["store.editorial"])
    );
    let editor_csrf = editor_session["csrf_token"].as_str().unwrap();
    let forbidden = issue_operations_token(
        &workforce,
        &editor_cookie,
        editor_csrf,
        "editor-token-request-0001",
        "store.moderation",
    )
    .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);

    let logout_headers = mutation_headers(
        REVIEW_ORIGIN,
        &review_cookie,
        &review_csrf,
        "review-logout-request-0001",
    );
    let logout = call(
        &workforce,
        Method::POST,
        "/review/v1/session:logout",
        &logout_headers,
        &[],
    )
    .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
    assert!(header(&logout, SET_COOKIE.as_str()).contains("Max-Age=0"));
    let logout_replay = call(
        &workforce,
        Method::POST,
        "/review/v1/session:logout",
        &logout_headers,
        &[],
    )
    .await;
    assert_eq!(logout_replay.status, StatusCode::NO_CONTENT);
    assert_eq!(
        control_get(&control, "/v1/review/submissions?limit=1", &review_token)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    revoke_link(&pool, ADMIN_LINK_ID).await;
    assert_eq!(
        call(
            &workforce,
            Method::GET,
            "/operations/v1/session",
            &[("cookie", &operations_cookie)],
            &[],
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        control_get(&control, "/v1/editorial/releases?limit=1", editorial_token,)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    sqlx::query(
        "UPDATE store_operators SET state = 'suspended', resource_version = resource_version + 1 \
         WHERE operator_id = $1",
    )
    .bind(EDITOR_ID)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        call(
            &workforce,
            Method::GET,
            "/operations/v1/session",
            &[("cookie", &editor_cookie)],
            &[],
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );

    assert_sensitive_values_absent(
        &pool,
        &[
            REVIEW_SUBJECT,
            ADMIN_SUBJECT,
            EDITOR_SUBJECT,
            "valid-review-code-0001",
            "valid-admin-code-0001",
            &cookie_secret(&review_cookie),
            &review_csrf,
            &review_token,
        ],
    )
    .await;
}

fn secrets() -> WorkforceSecrets {
    WorkforceSecrets::from_base64(
        &URL_SAFE_NO_PAD.encode([1_u8; 32]),
        &URL_SAFE_NO_PAD.encode([2_u8; 32]),
        &URL_SAFE_NO_PAD.encode([3_u8; 32]),
        &URL_SAFE_NO_PAD.encode([4_u8; 32]),
        &URL_SAFE_NO_PAD.encode([5_u8; 32]),
    )
    .unwrap()
}

async fn seed_workforce(pool: &sqlx::PgPool, secrets: &WorkforceSecrets) {
    let now: i64 = sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO reviewers (reviewer_id, email, role, two_factor_enabled, state, \
         created_unix_seconds) VALUES ($1, 'reviewer@example.com', 'reviewer', TRUE, 'active', $2)",
    )
    .bind(REVIEWER_ID)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO store_operators (operator_id, email, role, two_factor_enabled, state, \
         created_unix_seconds) VALUES \
         ($1, 'admin@example.com', 'admin', TRUE, 'active', $3), \
         ($2, 'editor@example.com', 'editor', TRUE, 'active', $3)",
    )
    .bind(ADMIN_ID)
    .bind(EDITOR_ID)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    for (link_id, issuer, subject, reviewer_id, operator_id) in [
        (
            REVIEW_LINK_ID,
            REVIEW_ISSUER,
            REVIEW_SUBJECT,
            Some(REVIEWER_ID),
            None,
        ),
        (
            ADMIN_LINK_ID,
            OPERATIONS_ISSUER,
            ADMIN_SUBJECT,
            None,
            Some(ADMIN_ID),
        ),
        (
            EDITOR_LINK_ID,
            OPERATIONS_ISSUER,
            EDITOR_SUBJECT,
            None,
            Some(EDITOR_ID),
        ),
    ] {
        sqlx::query(
            "INSERT INTO workforce_identity_links (link_id, provider_key, issuer, \
             subject_hmac_sha256, reviewer_id, operator_id, state, linked_unix_seconds) \
             VALUES ($1, 'primary', $2, $3, $4, $5, 'active', $6)",
        )
        .bind(link_id)
        .bind(issuer)
        .bind(secrets.subject_hmac(issuer, subject))
        .bind(reviewer_id)
        .bind(operator_id)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn begin_login(application: &Router, uri: &str) -> String {
    let result = call(application, Method::GET, uri, &[], &[]).await;
    assert_eq!(result.status, StatusCode::FOUND);
    let location = Url::parse(header(&result, LOCATION.as_str())).unwrap();
    location
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned()
}

async fn callback(
    application: &Router,
    callback_uri: &str,
    code: &str,
    state: &str,
    expected_location: &str,
) -> HttpResult {
    let result = call(
        application,
        Method::GET,
        &format!("{callback_uri}?code={code}&state={state}"),
        &[],
        &[],
    )
    .await;
    assert_eq!(result.status, StatusCode::SEE_OTHER);
    assert_eq!(header(&result, LOCATION.as_str()), expected_location);
    result
}

async fn get_session(application: &Router, uri: &str, cookie: &str) -> Value {
    let result = call(application, Method::GET, uri, &[("cookie", cookie)], &[]).await;
    assert_eq!(result.status, StatusCode::OK);
    serde_json::from_slice(&result.body).unwrap()
}

async fn issue_review_token(
    application: &Router,
    cookie: &str,
    csrf: &str,
    idempotency_key: &str,
) -> HttpResult {
    let headers = mutation_headers(REVIEW_ORIGIN, cookie, csrf, idempotency_key);
    call(application, Method::POST, "/review/v1/token", &headers, &[]).await
}

async fn issue_operations_token(
    application: &Router,
    cookie: &str,
    csrf: &str,
    idempotency_key: &str,
    scope: &str,
) -> HttpResult {
    let mut headers = mutation_headers(OPERATIONS_ORIGIN, cookie, csrf, idempotency_key);
    headers.push(("content-type", "application/json"));
    let body = serde_json::to_vec(&serde_json::json!({"scope": scope})).unwrap();
    call(
        application,
        Method::POST,
        "/operations/v1/token",
        &headers,
        &body,
    )
    .await
}

fn mutation_headers<'a>(
    origin: &'a str,
    cookie: &'a str,
    csrf: &'a str,
    idempotency_key: &'a str,
) -> Vec<(&'a str, &'a str)> {
    vec![
        ("origin", origin),
        ("sec-fetch-site", "same-origin"),
        ("cookie", cookie),
        ("x-csrf-token", csrf),
        ("idempotency-key", idempotency_key),
    ]
}

async fn control_get(application: &Router, uri: &str, token: &str) -> HttpResult {
    let authorization = format!("Bearer {token}");
    call(
        application,
        Method::GET,
        uri,
        &[(AUTHORIZATION.as_str(), &authorization)],
        &[],
    )
    .await
}

async fn call(
    application: &Router,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> HttpResult {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = application
        .clone()
        .oneshot(request.body(Body::from(body.to_vec())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 128 * 1024)
        .await
        .unwrap()
        .to_vec();
    HttpResult {
        status,
        headers,
        body,
    }
}

fn header<'a>(result: &'a HttpResult, name: &str) -> &'a str {
    result.headers.get(name).unwrap().to_str().unwrap()
}

fn session_cookie(result: &HttpResult, expected_name: &str) -> String {
    let set_cookie = header(result, SET_COOKIE.as_str());
    assert!(set_cookie.contains("Secure; HttpOnly; SameSite=Strict"));
    let cookie = set_cookie.split(';').next().unwrap();
    assert!(cookie.starts_with(&format!("{expected_name}=")));
    cookie.to_owned()
}

fn cookie_secret(cookie: &str) -> String {
    cookie.split_once('=').unwrap().1.to_owned()
}

async fn revoke_link(pool: &sqlx::PgPool, link_id: &str) {
    sqlx::query(
        "UPDATE workforce_identity_links SET state = 'revoked', \
         revoked_unix_seconds = EXTRACT(EPOCH FROM clock_timestamp())::BIGINT, \
         resource_version = resource_version + 1 WHERE link_id = $1",
    )
    .bind(link_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn assert_sensitive_values_absent(pool: &sqlx::PgPool, values: &[&str]) {
    let audit_rows: Vec<String> = sqlx::query_scalar(
        "SELECT to_jsonb(audit_events)::TEXT FROM audit_events WHERE action LIKE 'workforce.%'",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let persisted_links: Vec<String> =
        sqlx::query("SELECT issuer, subject_hmac_sha256 FROM workforce_identity_links")
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| {
                format!(
                    "{} {}",
                    row.get::<String, _>("issuer"),
                    row.get::<String, _>("subject_hmac_sha256")
                )
            })
            .collect();
    let persisted = audit_rows
        .into_iter()
        .chain(persisted_links)
        .collect::<Vec<_>>()
        .join("\n");
    for value in values {
        assert!(!persisted.contains(value), "sensitive value was persisted");
    }
}

async fn reset(pool: &sqlx::PgPool) {
    pool.execute(
        "TRUNCATE workforce_control_token_issuances, workforce_logout_records, \
         workforce_oidc_transactions, workforce_sessions, workforce_identity_links, \
         reviewer_access_tokens, reviewers, store_operator_access_tokens, store_operators, \
         idempotency_records, audit_events, outbox_events RESTART IDENTITY CASCADE",
    )
    .await
    .unwrap();
}
