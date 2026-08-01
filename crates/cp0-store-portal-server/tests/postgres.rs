use std::env;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::{COOKIE, ETAG, LOCATION, SET_COOKIE};
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cp0_store_portal_server::{
    AuthIntent, OidcError, OidcFuture, OidcProvider, PortalSecrets, PortalService,
    VerifiedIdentity, connect, migrate, pkce_challenge, router, sha256_hex,
};
use serde_json::Value;
use sqlx::{Executor, Row};
use tower::ServiceExt;
use url::Url;

const PROVIDER_KEY: &str = "primary";
const ISSUER: &str = "https://identity.example.com";
const CONFIG_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct FakeProvider;

impl OidcProvider for FakeProvider {
    fn key(&self) -> &str {
        PROVIDER_KEY
    }

    fn issuer(&self) -> &str {
        ISSUER
    }

    fn config_sha256(&self) -> &str {
        CONFIG_SHA256
    }

    fn authorization_uri(
        &self,
        intent: AuthIntent,
        state: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<String, OidcError> {
        let intent = match intent {
            AuthIntent::Login => "login",
            AuthIntent::StepUp => "step-up",
            AuthIntent::Link => "link",
        };
        Ok(format!(
            "{ISSUER}/authorize?state={state}&nonce={nonce}&code_challenge={}&intent={intent}",
            pkce_challenge(pkce_verifier)
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
            if nonce.len() != 43 || pkce_verifier.len() != 43 || !code.starts_with("valid-") {
                return Err(OidcError::InvalidToken);
            }
            Ok(VerifiedIdentity {
                issuer: ISSUER.to_owned(),
                subject: "provider-subject-must-never-be-returned".to_owned(),
                email: "developer@example.com".to_owned(),
                email_verified: true,
                mfa_authenticated_unix_seconds: (intent == AuthIntent::StepUp).then_some(now),
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires CP0_STORE_TEST_DATABASE_URL"]
async fn portal_oidc_session_acceptance() {
    let database_url = env::var("CP0_STORE_TEST_DATABASE_URL")
        .expect("CP0_STORE_TEST_DATABASE_URL must be set for the database gate");
    let pool = connect(&database_url, 8).await.unwrap();
    migrate(&pool).await.unwrap();
    reset(&pool).await;
    let csrf_key = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let nonce_key = URL_SAFE_NO_PAD.encode([8_u8; 32]);
    let pkce_key = URL_SAFE_NO_PAD.encode([9_u8; 32]);
    let subject_key = URL_SAFE_NO_PAD.encode([10_u8; 32]);
    let secrets =
        PortalSecrets::from_base64(&csrf_key, &nonce_key, &pkce_key, &subject_key).unwrap();
    let application = router(
        PortalService::new(
            pool.clone(),
            secrets,
            vec![Arc::new(FakeProvider)],
            "https://developer.cardputerzero.dev".to_owned(),
            "https://developer.cardputerzero.dev/portal".to_owned(),
        )
        .unwrap(),
    );

    let login = call(
        &application,
        Method::GET,
        "/portal/auth/login?provider=primary",
        &[],
    )
    .await;
    assert_eq!(login.status, StatusCode::FOUND);
    assert_security_headers(&login);
    let login_uri = Url::parse(header(&login, LOCATION.as_str())).unwrap();
    let state = login_uri
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let nonce = login_uri
        .query_pairs()
        .find(|(key, _)| key == "nonce")
        .unwrap()
        .1
        .into_owned();
    let persisted = sqlx::query(
        "SELECT state_sha256, nonce_sha256, pkce_verifier_ciphertext \
         FROM oidc_login_transactions WHERE intent = 'login'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted.get::<String, _>("state_sha256"),
        sha256_hex(state.as_bytes())
    );
    assert_eq!(
        persisted.get::<String, _>("nonce_sha256"),
        sha256_hex(nonce.as_bytes())
    );
    let encrypted: Vec<u8> = persisted.get("pkce_verifier_ciphertext");
    assert!(
        !encrypted
            .windows(state.len())
            .any(|window| window == state.as_bytes())
    );
    assert!(
        !encrypted
            .windows(nonce.len())
            .any(|window| window == nonce.as_bytes())
    );

    let callback = call(
        &application,
        Method::GET,
        &format!("/portal/auth/callback?code=valid-login-code-0001&state={state}"),
        &[],
    )
    .await;
    assert_eq!(callback.status, StatusCode::SEE_OTHER);
    assert_eq!(
        header(&callback, LOCATION.as_str()),
        "https://developer.cardputerzero.dev/portal"
    );
    let first_cookie = session_cookie(&callback);
    assert!(header(&callback, SET_COOKIE.as_str()).contains("Secure; HttpOnly; SameSite=Strict"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM portal_accounts")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    let raw_subject_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM external_identity_links WHERE subject_hmac_sha256 = $1",
    )
    .bind("provider-subject-must-never-be-returned")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(raw_subject_count, 0);

    seed_membership(&pool).await;
    let session = call(
        &application,
        Method::GET,
        "/portal/v1/session",
        &[(COOKIE.as_str(), &first_cookie)],
    )
    .await;
    assert_eq!(session.status, StatusCode::OK);
    let session_body: Value = serde_json::from_slice(&session.body).unwrap();
    let csrf = session_body["csrf_token"].as_str().unwrap().to_owned();
    assert_eq!(session_body["email"], "developer@example.com");
    assert_eq!(session_body["teams"][0]["role"], "owner");
    assert_eq!(session_body["mfa_step_up_fresh"], false);
    assert!(session_body.get("access_token").is_none());
    assert!(session_body.get("refresh_token").is_none());
    assert!(!String::from_utf8_lossy(&session.body).contains("provider-subject"));
    let etag = header(&session, ETAG.as_str()).to_owned();

    let rejected_origin = call(
        &application,
        Method::POST,
        "/portal/v1/session:step-up",
        &[
            (COOKIE.as_str(), &first_cookie),
            ("x-csrf-token", &csrf),
            ("idempotency-key", "portal-step-up-0001"),
            ("if-match", &etag),
            ("origin", "https://attacker.example"),
            ("sec-fetch-site", "same-origin"),
            ("content-length", "0"),
        ],
    )
    .await;
    assert_eq!(rejected_origin.status, StatusCode::FORBIDDEN);

    let step_headers = [
        (COOKIE.as_str(), first_cookie.as_str()),
        ("x-csrf-token", csrf.as_str()),
        ("idempotency-key", "portal-step-up-0001"),
        ("if-match", etag.as_str()),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-length", "0"),
    ];
    let step_up = call(
        &application,
        Method::POST,
        "/portal/v1/session:step-up",
        &step_headers,
    )
    .await;
    assert_eq!(step_up.status, StatusCode::OK);
    let step_body: Value = serde_json::from_slice(&step_up.body).unwrap();
    let authorization_uri = step_body["authorization_uri"].as_str().unwrap();
    let step_uri = Url::parse(authorization_uri).unwrap();
    let step_state = step_uri
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let replay = call(
        &application,
        Method::POST,
        "/portal/v1/session:step-up",
        &step_headers,
    )
    .await;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(replay.body, step_up.body);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM oidc_login_transactions WHERE intent = 'step-up'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );

    let step_callback = call(
        &application,
        Method::GET,
        &format!("/portal/auth/callback?code=valid-step-up-code-0001&state={step_state}"),
        &[],
    )
    .await;
    assert_eq!(step_callback.status, StatusCode::SEE_OTHER);
    let second_cookie = session_cookie(&step_callback);
    assert_ne!(first_cookie, second_cookie);
    assert_eq!(
        call(
            &application,
            Method::GET,
            "/portal/v1/session",
            &[(COOKIE.as_str(), &first_cookie)],
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let stepped_session = call(
        &application,
        Method::GET,
        "/portal/v1/session",
        &[(COOKIE.as_str(), &second_cookie)],
    )
    .await;
    assert_eq!(stepped_session.status, StatusCode::OK);
    let stepped_body: Value = serde_json::from_slice(&stepped_session.body).unwrap();
    assert_eq!(stepped_body["mfa_step_up_fresh"], true);
    let stepped_csrf = stepped_body["csrf_token"].as_str().unwrap();

    let logout_headers = [
        (COOKIE.as_str(), second_cookie.as_str()),
        ("x-csrf-token", stepped_csrf),
        ("idempotency-key", "portal-logout-0001"),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-length", "0"),
    ];
    let logout = call(
        &application,
        Method::POST,
        "/portal/v1/session:logout",
        &logout_headers,
    )
    .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
    assert!(header(&logout, SET_COOKIE.as_str()).contains("Max-Age=0"));
    let logout_replay = call(
        &application,
        Method::POST,
        "/portal/v1/session:logout",
        &logout_headers,
    )
    .await;
    assert_eq!(logout_replay.status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(
            &application,
            Method::GET,
            "/portal/v1/session",
            &[(COOKIE.as_str(), &second_cookie)],
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &application,
            Method::GET,
            &format!("/portal/auth/callback?code=valid-step-up-code-0001&state={step_state}"),
            &[],
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
}

struct HttpResult {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

async fn call(
    application: &Router,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
) -> HttpResult {
    let mut request = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = application
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap()
        .to_vec();
    HttpResult {
        status,
        headers,
        body,
    }
}

fn header<'a>(response: &'a HttpResult, name: &str) -> &'a str {
    response.headers.get(name).unwrap().to_str().unwrap()
}

fn session_cookie(response: &HttpResult) -> String {
    header(response, SET_COOKIE.as_str())
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

fn assert_security_headers(response: &HttpResult) {
    assert_eq!(header(response, "cache-control"), "no-store");
    assert_eq!(header(response, "pragma"), "no-cache");
    assert_eq!(header(response, "referrer-policy"), "no-referrer");
    assert_eq!(header(response, "x-content-type-options"), "nosniff");
}

async fn seed_membership(pool: &sqlx::PgPool) {
    let account_id: String = sqlx::query_scalar("SELECT account_id FROM portal_accounts")
        .fetch_one(pool)
        .await
        .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    transaction
        .execute(
            "INSERT INTO teams (team_id, name) VALUES \
             ('team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'Portal Team')",
        )
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO team_members (member_id, team_id, account_id, email, role, \
         two_factor_enabled) VALUES ('member_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
         'team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', $1, 'developer@example.com', 'owner', TRUE)",
    )
    .bind(account_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn reset(pool: &sqlx::PgPool) {
    pool.execute(
        "TRUNCATE oidc_login_transactions, portal_sessions, team_invitations, \
         external_identity_links, portal_accounts, access_tokens, team_members, teams \
         RESTART IDENTITY CASCADE",
    )
    .await
    .unwrap();
}
