use std::env;
use std::sync::Arc;
use std::sync::Mutex;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::{COOKIE, ETAG, LOCATION, SET_COOKIE};
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cp0_store_portal_server::{
    AuthIntent, InvitationDelivery, InvitationDeliveryFailure, InvitationDeliveryFuture,
    InvitationEmailWorker, InvitationMailer, OidcError, OidcFuture, OidcProvider, PortalSecrets,
    PortalService, VerifiedIdentity, connect, migrate, pkce_challenge, router, sha256_hex,
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
            let invitee = code.contains("invitee");
            Ok(VerifiedIdentity {
                issuer: ISSUER.to_owned(),
                subject: if invitee {
                    "provider-invitee-subject-must-never-be-returned"
                } else {
                    "provider-subject-must-never-be-returned"
                }
                .to_owned(),
                email: if invitee {
                    "invitee@example.com"
                } else {
                    "developer@example.com"
                }
                .to_owned(),
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
    let invitation_key = URL_SAFE_NO_PAD.encode([11_u8; 32]);
    let secrets = PortalSecrets::from_base64(
        &csrf_key,
        &nonce_key,
        &pkce_key,
        &subject_key,
        &invitation_key,
    )
    .unwrap();
    let delivery_secrets = secrets.clone();
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

    let create_body = serde_json::to_vec(&serde_json::json!({
        "email": "Invitee@Example.COM",
        "role": "release-manager"
    }))
    .unwrap();
    let team_etag = format!("\"{}\"", stepped_body["teams"][0]["resource_version"]);
    let create_headers = [
        (COOKIE.as_str(), second_cookie.as_str()),
        ("x-csrf-token", stepped_csrf),
        ("idempotency-key", "portal-invitation-create-0001"),
        ("if-match", team_etag.as_str()),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-type", "application/json"),
    ];
    let created = call_with_body(
        &application,
        Method::POST,
        "/portal/v1/teams/team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/invitations",
        &create_headers,
        &create_body,
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(header(&created, ETAG.as_str()), "\"2\"");
    let created_body: Value = serde_json::from_slice(&created.body).unwrap();
    assert!(created_body["invitation_id"].as_str().is_some());
    assert_eq!(created_body["email"], "invitee@example.com");
    assert!(!String::from_utf8_lossy(&created.body).contains("invitation_token"));
    let create_replay = call_with_body(
        &application,
        Method::POST,
        "/portal/v1/teams/team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/invitations",
        &create_headers,
        &create_body,
    )
    .await;
    assert_eq!(create_replay.status, StatusCode::CREATED);
    assert_eq!(create_replay.body, created.body);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM team_invitations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );

    let mailer = CapturingMailer::default();
    let worker = InvitationEmailWorker::new(
        pool.clone(),
        delivery_secrets,
        "https://developer.cardputerzero.dev",
    )
    .unwrap();
    assert!(
        worker
            .run_once("portal-mailer-test", &mailer)
            .await
            .unwrap()
    );
    assert!(
        !worker
            .run_once("portal-mailer-test", &mailer)
            .await
            .unwrap()
    );
    let acceptance_url = mailer.urls.lock().unwrap()[0].clone();
    let invitation_token = Url::parse(&acceptance_url)
        .unwrap()
        .fragment()
        .unwrap()
        .strip_prefix("token=")
        .unwrap()
        .to_owned();
    assert_eq!(invitation_token.len(), 43);
    let inspect_body =
        serde_json::to_vec(&serde_json::json!({"invitation_token": invitation_token})).unwrap();
    let inspect = call_with_body(
        &application,
        Method::POST,
        "/portal/v1/invitations:inspect",
        &[("content-type", "application/json")],
        &inspect_body,
    )
    .await;
    assert_eq!(inspect.status, StatusCode::OK);
    let preview: Value = serde_json::from_slice(&inspect.body).unwrap();
    assert_eq!(preview["team_name"], "Portal Team");
    assert_eq!(preview["masked_email"], "i***@example.com");
    assert!(!String::from_utf8_lossy(&inspect.body).contains("invitee@example.com"));

    let wrong_accept_headers = [
        (COOKIE.as_str(), second_cookie.as_str()),
        ("x-csrf-token", stepped_csrf),
        ("idempotency-key", "portal-invitation-wrong-account-0001"),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-type", "application/json"),
    ];
    assert_eq!(
        call_with_body(
            &application,
            Method::POST,
            "/portal/v1/invitations:accept",
            &wrong_accept_headers,
            &inspect_body,
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    let invitee_login = call(
        &application,
        Method::GET,
        "/portal/auth/login?provider=primary",
        &[],
    )
    .await;
    let invitee_login_uri = Url::parse(header(&invitee_login, LOCATION.as_str())).unwrap();
    let invitee_state = invitee_login_uri
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let invitee_callback = call(
        &application,
        Method::GET,
        &format!("/portal/auth/callback?code=valid-invitee-login-code-0001&state={invitee_state}"),
        &[],
    )
    .await;
    assert_eq!(invitee_callback.status, StatusCode::SEE_OTHER);
    let invitee_cookie = session_cookie(&invitee_callback);
    let invitee_session = call(
        &application,
        Method::GET,
        "/portal/v1/session",
        &[(COOKIE.as_str(), &invitee_cookie)],
    )
    .await;
    let invitee_session_body: Value = serde_json::from_slice(&invitee_session.body).unwrap();
    let invitee_csrf = invitee_session_body["csrf_token"].as_str().unwrap();
    let accept_headers = [
        (COOKIE.as_str(), invitee_cookie.as_str()),
        ("x-csrf-token", invitee_csrf),
        ("idempotency-key", "portal-invitation-accept-0001"),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-type", "application/json"),
    ];
    let accepted = call_with_body(
        &application,
        Method::POST,
        "/portal/v1/invitations:accept",
        &accept_headers,
        &inspect_body,
    )
    .await;
    assert_eq!(accepted.status, StatusCode::OK);
    assert_eq!(header(&accepted, ETAG.as_str()), "\"3\"");
    let rotated_invitee_cookie = session_cookie(&accepted);
    assert_ne!(invitee_cookie, rotated_invitee_cookie);
    assert_eq!(
        call(
            &application,
            Method::GET,
            "/portal/v1/session",
            &[(COOKIE.as_str(), &invitee_cookie)],
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let rotated_invitee_session = call(
        &application,
        Method::GET,
        "/portal/v1/session",
        &[(COOKIE.as_str(), &rotated_invitee_cookie)],
    )
    .await;
    let rotated_invitee_body: Value =
        serde_json::from_slice(&rotated_invitee_session.body).unwrap();
    assert_eq!(rotated_invitee_body["teams"][0]["role"], "release-manager");
    let rotated_invitee_csrf = rotated_invitee_body["csrf_token"].as_str().unwrap();
    let replay_headers = [
        (COOKIE.as_str(), rotated_invitee_cookie.as_str()),
        ("x-csrf-token", rotated_invitee_csrf),
        ("idempotency-key", "portal-invitation-accept-0001"),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-type", "application/json"),
    ];
    let accept_replay = call_with_body(
        &application,
        Method::POST,
        "/portal/v1/invitations:accept",
        &replay_headers,
        &inspect_body,
    )
    .await;
    assert_eq!(accept_replay.status, StatusCode::OK);
    assert_eq!(accept_replay.body, accepted.body);

    let cancel_create_body = serde_json::to_vec(&serde_json::json!({
        "email": "cancel@example.com",
        "role": "viewer"
    }))
    .unwrap();
    let cancel_create_headers = [
        (COOKIE.as_str(), second_cookie.as_str()),
        ("x-csrf-token", stepped_csrf),
        ("idempotency-key", "portal-invitation-create-0002"),
        ("if-match", "\"3\""),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-type", "application/json"),
    ];
    let cancel_created = call_with_body(
        &application,
        Method::POST,
        "/portal/v1/teams/team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/invitations",
        &cancel_create_headers,
        &cancel_create_body,
    )
    .await;
    assert_eq!(cancel_created.status, StatusCode::CREATED);
    let cancel_created_body: Value = serde_json::from_slice(&cancel_created.body).unwrap();
    let cancel_invitation_id = cancel_created_body["invitation_id"].as_str().unwrap();
    let cancel_headers = [
        (COOKIE.as_str(), second_cookie.as_str()),
        ("x-csrf-token", stepped_csrf),
        ("idempotency-key", "portal-invitation-cancel-0001"),
        ("if-match", "\"4\""),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-length", "0"),
    ];
    let cancelled = call(
        &application,
        Method::POST,
        &format!("/portal/v1/invitations/{cancel_invitation_id}:cancel"),
        &cancel_headers,
    )
    .await;
    assert_eq!(cancelled.status, StatusCode::OK);
    assert_eq!(header(&cancelled, ETAG.as_str()), "\"5\"");
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM portal_invitation_deliveries WHERE invitation_id = $1",
        )
        .bind(cancel_invitation_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
        "cancelled"
    );
    let cancelled_ciphertext: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT token_ciphertext FROM portal_invitation_deliveries WHERE invitation_id = $1",
    )
    .bind(cancel_invitation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(cancelled_ciphertext.is_none());

    let invitations = call(
        &application,
        Method::GET,
        "/portal/v1/teams/team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/invitations",
        &[(COOKIE.as_str(), second_cookie.as_str())],
    )
    .await;
    assert_eq!(invitations.status, StatusCode::OK);
    assert_eq!(header(&invitations, ETAG.as_str()), "\"5\"");
    let invitations_body: Value = serde_json::from_slice(&invitations.body).unwrap();
    assert_eq!(invitations_body["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM outbox_events WHERE payload::text LIKE $1",
        )
        .bind(format!("%{invitation_token}%"))
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM audit_events event \
             WHERE to_jsonb(event)::text LIKE $1",
        )
        .bind(format!("%{invitation_token}%"))
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM idempotency_records record \
             WHERE to_jsonb(record)::text LIKE $1",
        )
        .bind(format!("%{invitation_token}%"))
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM portal_invitation_deliveries \
             WHERE token_ciphertext IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );

    let database_now: i64 =
        sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut expiry_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE teams SET resource_version = 6 \
         WHERE team_id = 'team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
    )
    .execute(&mut *expiry_transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO team_invitations (invitation_id, team_id, email, role, token_sha256, \
         state, invited_by_member_id, team_resource_version, created_unix_seconds, \
         expires_unix_seconds) VALUES \
         ('invite_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', \
          'team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'expired@example.com', 'viewer', $1, \
          'pending', 'member_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 6, $2, $3)",
    )
    .bind("e".repeat(64))
    .bind(database_now - 604800)
    .bind(database_now)
    .execute(&mut *expiry_transaction)
    .await
    .unwrap();
    expiry_transaction.commit().await.unwrap();
    assert!(
        worker
            .run_once("portal-mailer-test", &mailer)
            .await
            .unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM team_invitations \
             WHERE invitation_id = 'invite_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        "expired"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT resource_version FROM teams \
             WHERE team_id = 'team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        7
    );

    let retry_create_body = serde_json::to_vec(&serde_json::json!({
        "email": "retry@example.com",
        "role": "developer"
    }))
    .unwrap();
    let retry_create_headers = [
        (COOKIE.as_str(), second_cookie.as_str()),
        ("x-csrf-token", stepped_csrf),
        ("idempotency-key", "portal-invitation-create-0003"),
        ("if-match", "\"7\""),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-type", "application/json"),
    ];
    let retry_created = call_with_body(
        &application,
        Method::POST,
        "/portal/v1/teams/team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/invitations",
        &retry_create_headers,
        &retry_create_body,
    )
    .await;
    assert_eq!(retry_created.status, StatusCode::CREATED);
    let retry_created_body: Value = serde_json::from_slice(&retry_created.body).unwrap();
    let retry_invitation_id = retry_created_body["invitation_id"].as_str().unwrap();
    let transient_mailer = FailingMailer(InvitationDeliveryFailure::Transient("provider-timeout"));
    assert!(
        worker
            .run_once("portal-mailer-test", &transient_mailer)
            .await
            .unwrap()
    );
    let retry_delivery = sqlx::query(
        "SELECT state, token_ciphertext IS NOT NULL AS has_ciphertext, attempts, last_error_code \
         FROM portal_invitation_deliveries WHERE invitation_id = $1",
    )
    .bind(retry_invitation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retry_delivery.get::<String, _>("state"), "pending");
    assert!(retry_delivery.get::<bool, _>("has_ciphertext"));
    assert_eq!(retry_delivery.get::<i16, _>("attempts"), 1);
    assert_eq!(
        retry_delivery.get::<String, _>("last_error_code"),
        "provider-timeout"
    );

    let failed_create_body = serde_json::to_vec(&serde_json::json!({
        "email": "failed@example.com",
        "role": "viewer"
    }))
    .unwrap();
    let failed_create_headers = [
        (COOKIE.as_str(), second_cookie.as_str()),
        ("x-csrf-token", stepped_csrf),
        ("idempotency-key", "portal-invitation-create-0004"),
        ("if-match", "\"8\""),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-type", "application/json"),
    ];
    let failed_created = call_with_body(
        &application,
        Method::POST,
        "/portal/v1/teams/team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/invitations",
        &failed_create_headers,
        &failed_create_body,
    )
    .await;
    assert_eq!(failed_created.status, StatusCode::CREATED);
    let failed_created_body: Value = serde_json::from_slice(&failed_created.body).unwrap();
    let failed_invitation_id = failed_created_body["invitation_id"].as_str().unwrap();
    let permanent_mailer =
        FailingMailer(InvitationDeliveryFailure::Permanent("recipient-rejected"));
    assert!(
        worker
            .run_once("portal-mailer-test", &permanent_mailer)
            .await
            .unwrap()
    );
    let failed_delivery = sqlx::query(
        "SELECT state, token_ciphertext IS NULL AS secret_cleared, last_error_code \
         FROM portal_invitation_deliveries WHERE invitation_id = $1",
    )
    .bind(failed_invitation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(failed_delivery.get::<String, _>("state"), "failed");
    assert!(failed_delivery.get::<bool, _>("secret_cleared"));
    assert_eq!(
        failed_delivery.get::<String, _>("last_error_code"),
        "recipient-rejected"
    );

    pool.execute(
        "CREATE FUNCTION fail_portal_invitation_audit() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN IF NEW.action = 'team.invitation-created' THEN \
         RAISE EXCEPTION 'injected Portal invitation audit failure'; END IF; RETURN NEW; END; $$",
    )
    .await
    .unwrap();
    pool.execute(
        "CREATE TRIGGER fail_portal_invitation_audit_trigger BEFORE INSERT ON audit_events \
         FOR EACH ROW EXECUTE FUNCTION fail_portal_invitation_audit()",
    )
    .await
    .unwrap();
    let rollback_create_body = serde_json::to_vec(&serde_json::json!({
        "email": "rollback@example.com",
        "role": "viewer"
    }))
    .unwrap();
    let rollback_create_headers = [
        (COOKIE.as_str(), second_cookie.as_str()),
        ("x-csrf-token", stepped_csrf),
        ("idempotency-key", "portal-invitation-create-rollback-0001"),
        ("if-match", "\"9\""),
        ("origin", "https://developer.cardputerzero.dev"),
        ("sec-fetch-site", "same-origin"),
        ("content-type", "application/json"),
    ];
    let rollback_failure = call_with_body(
        &application,
        Method::POST,
        "/portal/v1/teams/team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/invitations",
        &rollback_create_headers,
        &rollback_create_body,
    )
    .await;
    assert_eq!(rollback_failure.status, StatusCode::SERVICE_UNAVAILABLE);
    pool.execute("DROP TRIGGER fail_portal_invitation_audit_trigger ON audit_events")
        .await
        .unwrap();
    pool.execute("DROP FUNCTION fail_portal_invitation_audit()")
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT resource_version FROM teams \
             WHERE team_id = 'team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        9
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM team_invitations WHERE email = 'rollback@example.com'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM idempotency_records WHERE request_sha256 = $1",
        )
        .bind(sha256_hex(
            &serde_json::to_vec(&serde_json::json!({
                "email": "rollback@example.com",
                "expected_version": 9,
                "operation": "invitation-create",
                "role": "viewer",
                "team_id": "team_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }))
            .unwrap()
        ))
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    assert_sqlstate(
        sqlx::query(
            "UPDATE portal_invitation_deliveries SET state = 'leased', \
             attempts = attempts + 1, lease_owner = 'early-takeover', \
             lease_expires_unix_seconds = available_unix_seconds + 60, \
             last_error_code = NULL, resource_version = resource_version + 1 \
             WHERE invitation_id = $1",
        )
        .bind(retry_invitation_id)
        .execute(&pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query(
            "UPDATE portal_invitation_deliveries SET state = 'pending', \
             token_ciphertext = decode(repeat('00', 64), 'hex'), \
             available_unix_seconds = available_unix_seconds + 60, \
             last_error_code = 'forged-retry', resource_version = resource_version + 1 \
             WHERE invitation_id = $1",
        )
        .bind(failed_invitation_id)
        .execute(&pool)
        .await,
        "55000",
    );
    assert_sqlstate(
        sqlx::query("DELETE FROM portal_invitation_deliveries WHERE invitation_id = $1")
            .bind(failed_invitation_id)
            .execute(&pool)
            .await,
        "55000",
    );

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
    call_with_body(application, method, path, headers, &[]).await
}

async fn call_with_body(
    application: &Router,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> HttpResult {
    let mut request = Request::builder().method(method).uri(path);
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
        "TRUNCATE portal_invitation_deliveries, oidc_login_transactions, portal_sessions, \
         team_invitations, external_identity_links, portal_accounts, access_tokens, \
         team_members, teams, idempotency_records, audit_events, outbox_events \
         RESTART IDENTITY CASCADE",
    )
    .await
    .unwrap();
}

#[derive(Default)]
struct CapturingMailer {
    urls: Mutex<Vec<String>>,
}

impl InvitationMailer for CapturingMailer {
    fn deliver<'a>(&'a self, delivery: InvitationDelivery<'a>) -> InvitationDeliveryFuture<'a> {
        Box::pin(async move {
            assert_eq!(delivery.email, "invitee@example.com");
            assert_eq!(delivery.team_name, "Portal Team");
            assert_eq!(delivery.role, "release-manager");
            self.urls
                .lock()
                .unwrap()
                .push(delivery.acceptance_url.to_owned());
            Ok(())
        })
    }
}

struct FailingMailer(InvitationDeliveryFailure);

impl InvitationMailer for FailingMailer {
    fn deliver<'a>(&'a self, _delivery: InvitationDelivery<'a>) -> InvitationDeliveryFuture<'a> {
        Box::pin(async move { Err(self.0) })
    }
}

fn assert_sqlstate<T: std::fmt::Debug>(result: Result<T, sqlx::Error>, expected: &str) {
    let error = result.expect_err("statement must fail closed");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some(expected)
    );
}
