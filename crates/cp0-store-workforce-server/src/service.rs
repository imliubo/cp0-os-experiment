use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, LOCATION, ORIGIN, PRAGMA, SET_COOKIE,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cp0_store_portal_server::{AuthIntent, OidcError, OidcProvider};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Row, Transaction};
use url::Url;
use uuid::Uuid;

use crate::{WorkforceSecrets, sha256_hex};

const SESSION_IDLE_SECONDS: i64 = 900;
const SESSION_ABSOLUTE_SECONDS: i64 = 28_800;
const CONTROL_TOKEN_SECONDS: i64 = 300;
const OIDC_TRANSACTION_SECONDS: i64 = 600;
const MAX_PROVIDERS: usize = 8;
const MAX_REQUEST_BYTES: usize = 1024;

static CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");
static IDEMPOTENCY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");
static FETCH_SITE_HEADER: HeaderName = HeaderName::from_static("sec-fetch-site");
static REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");
static REFERRER_POLICY_HEADER: HeaderName = HeaderName::from_static("referrer-policy");
static CONTENT_TYPE_OPTIONS_HEADER: HeaderName = HeaderName::from_static("x-content-type-options");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkforceAudience {
    Review,
    Operations,
}

impl WorkforceAudience {
    fn as_str(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::Operations => "operations",
        }
    }

    fn cookie_name(self) -> &'static str {
        match self {
            Self::Review => "__Host-cp0_review",
            Self::Operations => "__Host-cp0_operations",
        }
    }
}

#[derive(Debug)]
pub enum WorkforceBuildError {
    InvalidConfiguration,
}

#[derive(Clone)]
pub struct WorkforceService {
    inner: Arc<WorkforceServiceInner>,
}

struct WorkforceServiceInner {
    pool: PgPool,
    secrets: WorkforceSecrets,
    review: AudienceConfig,
    operations: AudienceConfig,
}

struct AudienceConfig {
    providers: HashMap<String, Arc<dyn OidcProvider>>,
    allowed_origin: String,
    post_login_uri: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginQuery {
    provider: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallbackQuery {
    code: String,
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationsTokenRequest {
    scope: String,
}

#[derive(Debug, Serialize)]
struct ReviewSessionResponse {
    principal_id: String,
    role: String,
    audience: &'static str,
    csrf_token: String,
    idle_expires_unix_seconds: i64,
    absolute_expires_unix_seconds: i64,
    mfa_authenticated_unix_seconds: i64,
    resource_version: i64,
}

#[derive(Debug, Serialize)]
struct OperationsSessionResponse {
    principal_id: String,
    role: String,
    audience: &'static str,
    allowed_scopes: Vec<&'static str>,
    csrf_token: String,
    idle_expires_unix_seconds: i64,
    absolute_expires_unix_seconds: i64,
    mfa_authenticated_unix_seconds: i64,
    resource_version: i64,
}

#[derive(Debug, Serialize)]
struct ControlTokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: i64,
    scope: String,
    audience: &'static str,
}

#[derive(Debug)]
struct CreatedSession {
    secret: String,
}

#[derive(Debug)]
struct OidcTransactionRow {
    transaction_id: String,
    provider_key: String,
    provider_config_sha256: String,
    audience: String,
    nonce_sha256: String,
    pkce_verifier_ciphertext: Vec<u8>,
    state: String,
    expires_unix_seconds: i64,
}

#[derive(Debug)]
struct SessionRow {
    session_sha256: String,
    audience: String,
    state: String,
    resource_version: i64,
    created_unix_seconds: i64,
    last_seen_unix_seconds: i64,
    idle_expires_unix_seconds: i64,
    absolute_expires_unix_seconds: i64,
    mfa_authenticated_unix_seconds: i64,
    csrf_sha256: String,
    link_state: String,
    principal_id: String,
    principal_state: String,
    principal_role: String,
    principal_two_factor_enabled: bool,
}

#[derive(Debug)]
struct ExistingIssuance {
    request_sha256: String,
    token_sha256: String,
    principal_id: String,
    scope: String,
    expires_unix_seconds: i64,
}

#[derive(Clone, Copy, Debug)]
enum WorkforceError {
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Unavailable,
    Internal,
}

impl WorkforceService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: PgPool,
        secrets: WorkforceSecrets,
        review_providers: Vec<Arc<dyn OidcProvider>>,
        operations_providers: Vec<Arc<dyn OidcProvider>>,
        review_origin: String,
        review_post_login_uri: String,
        operations_origin: String,
        operations_post_login_uri: String,
    ) -> Result<Self, WorkforceBuildError> {
        let review = audience_config(review_providers, review_origin, review_post_login_uri)?;
        let operations = audience_config(
            operations_providers,
            operations_origin,
            operations_post_login_uri,
        )?;
        if review.allowed_origin == operations.allowed_origin {
            return Err(WorkforceBuildError::InvalidConfiguration);
        }
        Ok(Self {
            inner: Arc::new(WorkforceServiceInner {
                pool,
                secrets,
                review,
                operations,
            }),
        })
    }

    fn config(&self, audience: WorkforceAudience) -> &AudienceConfig {
        match audience {
            WorkforceAudience::Review => &self.inner.review,
            WorkforceAudience::Operations => &self.inner.operations,
        }
    }

    fn provider(
        &self,
        audience: WorkforceAudience,
        key: &str,
    ) -> Result<Arc<dyn OidcProvider>, WorkforceError> {
        self.config(audience)
            .providers
            .get(key)
            .cloned()
            .ok_or(WorkforceError::NotFound)
    }

    async fn begin_login(
        &self,
        audience: WorkforceAudience,
        provider_key: &str,
    ) -> Result<String, WorkforceError> {
        let provider = self.provider(audience, provider_key)?;
        let state = self
            .inner
            .secrets
            .random_token()
            .map_err(|_| WorkforceError::Internal)?;
        let nonce = self.inner.secrets.nonce_for_state(&state);
        let pkce_verifier = self
            .inner
            .secrets
            .random_token()
            .map_err(|_| WorkforceError::Internal)?;
        let authorization_uri = provider
            .authorization_uri(AuthIntent::StepUp, &state, &nonce, &pkce_verifier)
            .map_err(map_oidc_error)?;
        let encrypted = self
            .inner
            .secrets
            .encrypt_pkce(&pkce_verifier)
            .map_err(|_| WorkforceError::Internal)?;
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        expire_oidc_transactions(&mut transaction, now).await?;
        sqlx::query(
            "INSERT INTO workforce_oidc_transactions (transaction_id, state_sha256, \
             nonce_sha256, pkce_verifier_ciphertext, provider_key, provider_config_sha256, \
             audience, intent, state, requested_unix_seconds, expires_unix_seconds) VALUES \
             ($1, $2, $3, $4, $5, $6, $7, 'login', 'pending', $8, $8 + $9)",
        )
        .bind(opaque_id("wfoidc_"))
        .bind(sha256_hex(state.as_bytes()))
        .bind(sha256_hex(nonce.as_bytes()))
        .bind(encrypted)
        .bind(provider.key())
        .bind(provider.config_sha256())
        .bind(audience.as_str())
        .bind(now)
        .bind(OIDC_TRANSACTION_SECONDS)
        .execute(&mut *transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
        transaction
            .commit()
            .await
            .map_err(|_| WorkforceError::Unavailable)?;
        Ok(authorization_uri)
    }

    async fn complete_callback(
        &self,
        audience: WorkforceAudience,
        code: &str,
        state: &str,
        callback_request_id: &str,
    ) -> Result<CreatedSession, WorkforceError> {
        if !valid_secret(state)
            || !(16..=4096).contains(&code.len())
            || code.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(WorkforceError::InvalidRequest);
        }
        let state_sha256 = sha256_hex(state.as_bytes());
        let now = database_now_pool(&self.inner.pool).await?;
        let oidc = load_oidc_transaction_pool(&self.inner.pool, &state_sha256).await?;
        if oidc.audience != audience.as_str()
            || oidc.state != "pending"
            || oidc.expires_unix_seconds <= now
        {
            return Err(WorkforceError::Unauthorized);
        }
        let provider = self.provider(audience, &oidc.provider_key)?;
        if oidc.provider_config_sha256 != provider.config_sha256() {
            return Err(WorkforceError::Unauthorized);
        }
        let nonce = self.inner.secrets.nonce_for_state(state);
        if sha256_hex(nonce.as_bytes()) != oidc.nonce_sha256 {
            return Err(WorkforceError::Unauthorized);
        }
        let pkce_verifier = self
            .inner
            .secrets
            .decrypt_pkce(&oidc.pkce_verifier_ciphertext)
            .map_err(|_| WorkforceError::Internal)?;
        let identity = match provider
            .exchange(AuthIntent::StepUp, code, &nonce, &pkce_verifier, now)
            .await
        {
            Ok(identity) => identity,
            Err(OidcError::InvalidToken | OidcError::InvalidRequest) => {
                let _ = terminally_expire_oidc(&self.inner.pool, &state_sha256).await;
                return Err(WorkforceError::Unauthorized);
            }
            Err(error) => return Err(map_oidc_error(error)),
        };
        let mfa_time = identity
            .mfa_authenticated_unix_seconds
            .filter(|value| *value >= now - CONTROL_TOKEN_SECONDS && *value <= now)
            .ok_or(WorkforceError::Unauthorized)?;
        if identity.issuer != provider.issuer() || !identity.email_verified {
            let _ = terminally_expire_oidc(&self.inner.pool, &state_sha256).await;
            return Err(WorkforceError::Unauthorized);
        }
        let subject_hmac = self
            .inner
            .secrets
            .subject_hmac(&identity.issuer, &identity.subject);
        let session_secret = self
            .inner
            .secrets
            .random_token()
            .map_err(|_| WorkforceError::Internal)?;
        let session_sha256 = sha256_hex(session_secret.as_bytes());
        let csrf = self.inner.secrets.csrf_for_session(&session_secret);
        let csrf_sha256 = sha256_hex(csrf.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let commit_now = database_now(&mut transaction).await?;
        let locked = load_oidc_transaction(&mut transaction, &state_sha256, true)
            .await?
            .ok_or(WorkforceError::Unauthorized)?;
        if locked.transaction_id != oidc.transaction_id
            || locked.audience != audience.as_str()
            || locked.state != "pending"
            || locked.expires_unix_seconds <= commit_now
            || locked.provider_config_sha256 != provider.config_sha256()
        {
            return Err(WorkforceError::Unauthorized);
        }
        let principal = sqlx::query(
            "SELECT link.link_id, link.provider_key, link.state AS link_state, \
             link.reviewer_id, link.operator_id, reviewer.state AS reviewer_state, \
             reviewer.role AS reviewer_role, reviewer.two_factor_enabled AS reviewer_mfa, \
             operator.state AS operator_state, operator.role AS operator_role, \
             operator.two_factor_enabled AS operator_mfa \
             FROM workforce_identity_links link \
             LEFT JOIN reviewers reviewer ON reviewer.reviewer_id = link.reviewer_id \
             LEFT JOIN store_operators operator ON operator.operator_id = link.operator_id \
             WHERE link.issuer = $1 AND link.subject_hmac_sha256 = $2 FOR UPDATE OF link",
        )
        .bind(&identity.issuer)
        .bind(&subject_hmac)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?
        .ok_or(WorkforceError::Unauthorized)?;
        if principal.get::<String, _>("provider_key") != provider.key()
            || principal.get::<String, _>("link_state") != "active"
        {
            return Err(WorkforceError::Forbidden);
        }
        let principal_id = match audience {
            WorkforceAudience::Review => {
                if principal
                    .get::<Option<String>, _>("reviewer_state")
                    .as_deref()
                    != Some("active")
                    || principal.get::<Option<bool>, _>("reviewer_mfa") != Some(true)
                {
                    return Err(WorkforceError::Forbidden);
                }
                principal
                    .get::<Option<String>, _>("reviewer_id")
                    .ok_or(WorkforceError::Unauthorized)?
            }
            WorkforceAudience::Operations => {
                if principal
                    .get::<Option<String>, _>("operator_state")
                    .as_deref()
                    != Some("active")
                    || principal.get::<Option<bool>, _>("operator_mfa") != Some(true)
                {
                    return Err(WorkforceError::Forbidden);
                }
                principal
                    .get::<Option<String>, _>("operator_id")
                    .ok_or(WorkforceError::Unauthorized)?
            }
        };
        let link_id: String = principal.get("link_id");
        sqlx::query(
            "UPDATE workforce_oidc_transactions SET state = 'consumed', \
             consumed_unix_seconds = $1 WHERE transaction_id = $2 AND state = 'pending'",
        )
        .bind(commit_now)
        .bind(&locked.transaction_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
        sqlx::query(
            "INSERT INTO workforce_sessions (session_sha256, csrf_sha256, link_id, audience, \
             state, created_unix_seconds, last_seen_unix_seconds, idle_expires_unix_seconds, \
             absolute_expires_unix_seconds, mfa_authenticated_unix_seconds) VALUES \
             ($1, $2, $3, $4, 'active', $5, $5, $5 + $6, $5 + $7, $8)",
        )
        .bind(&session_sha256)
        .bind(csrf_sha256)
        .bind(link_id)
        .bind(audience.as_str())
        .bind(commit_now)
        .bind(SESSION_IDLE_SECONDS)
        .bind(SESSION_ABSOLUTE_SECONDS)
        .bind(mfa_time.min(commit_now))
        .execute(&mut *transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
        append_audit(
            &mut transaction,
            commit_now,
            &principal_id,
            "workforce.session-created",
            "workforce-session",
            &session_sha256,
            None,
            Some("active"),
            1,
            callback_request_id,
            &state_sha256,
            &state_sha256,
        )
        .await?;
        transaction
            .commit()
            .await
            .map_err(|_| WorkforceError::Unavailable)?;
        Ok(CreatedSession {
            secret: session_secret,
        })
    }

    async fn session_response(
        &self,
        audience: WorkforceAudience,
        session_secret: &str,
    ) -> Result<(serde_json::Value, i64), WorkforceError> {
        let session_sha256 = sha256_hex(session_secret.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let mut session = lock_session(&mut transaction, &session_sha256).await?;
        if !ensure_active_session(&mut transaction, &session, audience, now).await? {
            transaction
                .commit()
                .await
                .map_err(|_| WorkforceError::Unavailable)?;
            return Err(WorkforceError::Unauthorized);
        }
        touch_session(&mut transaction, &mut session, now).await?;
        let csrf_token = self.inner.secrets.csrf_for_session(session_secret);
        let value = match audience {
            WorkforceAudience::Review => serde_json::to_value(ReviewSessionResponse {
                principal_id: session.principal_id.clone(),
                role: session.principal_role.clone(),
                audience: audience.as_str(),
                csrf_token,
                idle_expires_unix_seconds: session.idle_expires_unix_seconds,
                absolute_expires_unix_seconds: session.absolute_expires_unix_seconds,
                mfa_authenticated_unix_seconds: session.mfa_authenticated_unix_seconds,
                resource_version: session.resource_version,
            }),
            WorkforceAudience::Operations => serde_json::to_value(OperationsSessionResponse {
                principal_id: session.principal_id.clone(),
                role: session.principal_role.clone(),
                audience: audience.as_str(),
                allowed_scopes: allowed_scopes(&session),
                csrf_token,
                idle_expires_unix_seconds: session.idle_expires_unix_seconds,
                absolute_expires_unix_seconds: session.absolute_expires_unix_seconds,
                mfa_authenticated_unix_seconds: session.mfa_authenticated_unix_seconds,
                resource_version: session.resource_version,
            }),
        }
        .map_err(|_| WorkforceError::Internal)?;
        let version = session.resource_version;
        transaction
            .commit()
            .await
            .map_err(|_| WorkforceError::Unavailable)?;
        Ok((value, version))
    }

    async fn issue_token(
        &self,
        audience: WorkforceAudience,
        session_secret: &str,
        csrf_token: &str,
        idempotency_key: &str,
        scope: &str,
        request_id: &str,
    ) -> Result<ControlTokenResponse, WorkforceError> {
        if !valid_scope(audience, scope) {
            return Err(WorkforceError::InvalidRequest);
        }
        let session_sha256 = sha256_hex(session_secret.as_bytes());
        let idempotency_sha256 = sha256_hex(idempotency_key.as_bytes());
        let request_sha256 = token_request_sha256(audience, scope);
        let raw_token = self.inner.secrets.control_token(
            &session_sha256,
            audience.as_str(),
            scope,
            idempotency_key,
        );
        let token_sha256 = sha256_hex(raw_token.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let mut session = lock_session(&mut transaction, &session_sha256).await?;
        if session.csrf_sha256 != sha256_hex(csrf_token.as_bytes()) {
            return Err(WorkforceError::Forbidden);
        }
        if !ensure_active_session(&mut transaction, &session, audience, now).await? {
            transaction
                .commit()
                .await
                .map_err(|_| WorkforceError::Unavailable)?;
            return Err(WorkforceError::Unauthorized);
        }
        if !allowed_scopes(&session).contains(&scope) {
            return Err(WorkforceError::Forbidden);
        }
        touch_session(&mut transaction, &mut session, now).await?;
        if let Some(existing) = load_issuance(
            &mut transaction,
            &session_sha256,
            audience,
            &idempotency_sha256,
        )
        .await?
        {
            if existing.request_sha256 != request_sha256
                || existing.token_sha256 != token_sha256
                || existing.principal_id != session.principal_id
                || existing.scope != scope
            {
                return Err(WorkforceError::Conflict);
            }
            if existing.expires_unix_seconds <= now
                || access_token_revoked(&mut transaction, audience, &token_sha256).await?
            {
                return Err(WorkforceError::Unauthorized);
            }
            transaction
                .commit()
                .await
                .map_err(|_| WorkforceError::Unavailable)?;
            return Ok(ControlTokenResponse {
                access_token: raw_token,
                token_type: "Bearer",
                expires_in: existing.expires_unix_seconds - now,
                scope: scope.to_owned(),
                audience: audience.as_str(),
            });
        }
        let expires = (now + CONTROL_TOKEN_SECONDS)
            .min(session.idle_expires_unix_seconds)
            .min(session.absolute_expires_unix_seconds);
        if expires <= now {
            return Err(WorkforceError::Unauthorized);
        }
        match audience {
            WorkforceAudience::Review => {
                sqlx::query(
                    "INSERT INTO reviewer_access_tokens (token_sha256, reviewer_id, scopes, \
                     expires_unix_seconds, revoked, created_unix_seconds, \
                     workforce_session_sha256) VALUES \
                     ($1, $2, ARRAY[$3], $4, FALSE, $5, $6)",
                )
                .bind(&token_sha256)
                .bind(&session.principal_id)
                .bind(scope)
                .bind(expires)
                .bind(now)
                .bind(&session_sha256)
                .execute(&mut *transaction)
                .await
                .map_err(|_| WorkforceError::Unavailable)?;
            }
            WorkforceAudience::Operations => {
                sqlx::query(
                    "INSERT INTO store_operator_access_tokens (token_sha256, operator_id, scopes, \
                     expires_unix_seconds, revoked, created_unix_seconds, \
                     workforce_session_sha256) VALUES \
                     ($1, $2, ARRAY[$3], $4, FALSE, $5, $6)",
                )
                .bind(&token_sha256)
                .bind(&session.principal_id)
                .bind(scope)
                .bind(expires)
                .bind(now)
                .bind(&session_sha256)
                .execute(&mut *transaction)
                .await
                .map_err(|_| WorkforceError::Unavailable)?;
            }
        }
        sqlx::query(
            "INSERT INTO workforce_control_token_issuances (session_sha256, audience, \
             idempotency_key_sha256, request_sha256, token_sha256, principal_id, scope, \
             created_unix_seconds, expires_unix_seconds) VALUES \
             ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&session_sha256)
        .bind(audience.as_str())
        .bind(&idempotency_sha256)
        .bind(&request_sha256)
        .bind(&token_sha256)
        .bind(&session.principal_id)
        .bind(scope)
        .bind(now)
        .bind(expires)
        .execute(&mut *transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
        append_audit(
            &mut transaction,
            now,
            &session.principal_id,
            "workforce.token-issued",
            "workforce-token",
            &token_sha256,
            None,
            Some("active"),
            1,
            request_id,
            &request_sha256,
            &idempotency_sha256,
        )
        .await?;
        transaction
            .commit()
            .await
            .map_err(|_| WorkforceError::Unavailable)?;
        Ok(ControlTokenResponse {
            access_token: raw_token,
            token_type: "Bearer",
            expires_in: expires - now,
            scope: scope.to_owned(),
            audience: audience.as_str(),
        })
    }

    async fn logout(
        &self,
        audience: WorkforceAudience,
        session_secret: &str,
        csrf_token: &str,
        idempotency_key: &str,
        request_id: &str,
    ) -> Result<(), WorkforceError> {
        let session_sha256 = sha256_hex(session_secret.as_bytes());
        let idempotency_sha256 = sha256_hex(idempotency_key.as_bytes());
        let request_sha256 = logout_request_sha256(audience);
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let session = lock_session(&mut transaction, &session_sha256).await?;
        if session.audience != audience.as_str()
            || session.csrf_sha256 != sha256_hex(csrf_token.as_bytes())
        {
            return Err(WorkforceError::Forbidden);
        }
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT request_sha256 FROM workforce_logout_records \
             WHERE session_sha256 = $1 AND audience = $2 AND idempotency_key_sha256 = $3",
        )
        .bind(&session_sha256)
        .bind(audience.as_str())
        .bind(&idempotency_sha256)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
        if let Some(existing) = existing {
            if existing != request_sha256 {
                return Err(WorkforceError::Conflict);
            }
            transaction
                .commit()
                .await
                .map_err(|_| WorkforceError::Unavailable)?;
            return Ok(());
        }
        if !ensure_active_session(&mut transaction, &session, audience, now).await? {
            transaction
                .commit()
                .await
                .map_err(|_| WorkforceError::Unavailable)?;
            return Err(WorkforceError::Unauthorized);
        }
        sqlx::query(
            "INSERT INTO workforce_logout_records (session_sha256, audience, \
             idempotency_key_sha256, request_sha256, completed_unix_seconds) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&session_sha256)
        .bind(audience.as_str())
        .bind(&idempotency_sha256)
        .bind(&request_sha256)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
        sqlx::query(
            "UPDATE workforce_sessions SET state = 'revoked', ended_unix_seconds = $1, \
             resource_version = resource_version + 1 \
             WHERE session_sha256 = $2 AND state = 'active'",
        )
        .bind(now.max(session.created_unix_seconds))
        .bind(&session_sha256)
        .execute(&mut *transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
        append_audit(
            &mut transaction,
            now,
            &session.principal_id,
            "workforce.session-revoked",
            "workforce-session",
            &session_sha256,
            Some("active"),
            Some("revoked"),
            session.resource_version + 1,
            request_id,
            &request_sha256,
            &idempotency_sha256,
        )
        .await?;
        transaction
            .commit()
            .await
            .map_err(|_| WorkforceError::Unavailable)
    }
}

pub async fn connect(database_url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await
}

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../cp0-store-control-server/migrations")
        .run(pool)
        .await
}

pub fn router(service: WorkforceService) -> Router {
    Router::new()
        .route("/review/auth/login", get(begin_review_login))
        .route("/review/auth/callback", get(complete_review_callback))
        .route("/review/v1/session", get(get_review_session))
        .route(
            "/review/v1/token",
            post(issue_review_token).layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/review/v1/session:logout",
            post(logout_review).layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route("/operations/auth/login", get(begin_operations_login))
        .route(
            "/operations/auth/callback",
            get(complete_operations_callback),
        )
        .route("/operations/v1/session", get(get_operations_session))
        .route(
            "/operations/v1/token",
            post(issue_operations_token)
                .layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/operations/v1/session:logout",
            post(logout_operations).layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .with_state(service)
}

async fn begin_review_login(
    State(service): State<WorkforceService>,
    query: Result<Query<LoginQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    begin_login(service, WorkforceAudience::Review, query).await
}

async fn begin_operations_login(
    State(service): State<WorkforceService>,
    query: Result<Query<LoginQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    begin_login(service, WorkforceAudience::Operations, query).await
}

async fn begin_login(
    service: WorkforceService,
    audience: WorkforceAudience,
    query: Result<Query<LoginQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let request_id = request_id();
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return WorkforceError::InvalidRequest.response(request_id),
    };
    match service.begin_login(audience, &query.provider).await {
        Ok(location) => redirect_response(StatusCode::FOUND, &location, None, request_id),
        Err(error) => error.response(request_id),
    }
}

async fn complete_review_callback(
    State(service): State<WorkforceService>,
    query: Result<Query<CallbackQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    complete_callback(service, WorkforceAudience::Review, query).await
}

async fn complete_operations_callback(
    State(service): State<WorkforceService>,
    query: Result<Query<CallbackQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    complete_callback(service, WorkforceAudience::Operations, query).await
}

async fn complete_callback(
    service: WorkforceService,
    audience: WorkforceAudience,
    query: Result<Query<CallbackQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let request_id = request_id();
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return WorkforceError::InvalidRequest.response(request_id),
    };
    match service
        .complete_callback(audience, &query.code, &query.state, &request_id)
        .await
    {
        Ok(session) => redirect_response(
            StatusCode::SEE_OTHER,
            &service.config(audience).post_login_uri,
            Some(session_cookie(audience, &session.secret)),
            request_id,
        ),
        Err(error) => error.response(request_id),
    }
}

async fn get_review_session(
    State(service): State<WorkforceService>,
    headers: HeaderMap,
) -> Response {
    get_session(service, WorkforceAudience::Review, headers).await
}

async fn get_operations_session(
    State(service): State<WorkforceService>,
    headers: HeaderMap,
) -> Response {
    get_session(service, WorkforceAudience::Operations, headers).await
}

async fn get_session(
    service: WorkforceService,
    audience: WorkforceAudience,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id();
    let secret = match session_cookie_value(&headers, audience) {
        Ok(secret) => secret,
        Err(error) => return error.response(request_id),
    };
    match service.session_response(audience, &secret).await {
        Ok((body, version)) => json_response(StatusCode::OK, &body, Some(version), request_id),
        Err(error) => error.response(request_id),
    }
}

async fn issue_review_token(
    State(service): State<WorkforceService>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request_id = request_id();
    let security = match mutation_headers(&service, WorkforceAudience::Review, &headers) {
        Ok(security) => security,
        Err(error) => return error.response(request_id),
    };
    if let Err(error) = require_empty_body(&headers, &body) {
        return error.response(request_id);
    }
    match service
        .issue_token(
            WorkforceAudience::Review,
            &security.session_secret,
            &security.csrf_token,
            &security.idempotency_key,
            "store.review",
            &request_id,
        )
        .await
    {
        Ok(token) => json_response(StatusCode::OK, &token, None, request_id),
        Err(error) => error.response(request_id),
    }
}

async fn issue_operations_token(
    State(service): State<WorkforceService>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request_id = request_id();
    let security = match mutation_headers(&service, WorkforceAudience::Operations, &headers) {
        Ok(security) => security,
        Err(error) => return error.response(request_id),
    };
    if headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some("application/json")
    {
        return WorkforceError::InvalidRequest.response(request_id);
    }
    let request: OperationsTokenRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => return WorkforceError::InvalidRequest.response(request_id),
    };
    match service
        .issue_token(
            WorkforceAudience::Operations,
            &security.session_secret,
            &security.csrf_token,
            &security.idempotency_key,
            &request.scope,
            &request_id,
        )
        .await
    {
        Ok(token) => json_response(StatusCode::OK, &token, None, request_id),
        Err(error) => error.response(request_id),
    }
}

async fn logout_review(
    State(service): State<WorkforceService>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    logout(service, WorkforceAudience::Review, headers, body).await
}

async fn logout_operations(
    State(service): State<WorkforceService>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    logout(service, WorkforceAudience::Operations, headers, body).await
}

async fn logout(
    service: WorkforceService,
    audience: WorkforceAudience,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request_id = request_id();
    let security = match mutation_headers(&service, audience, &headers) {
        Ok(security) => security,
        Err(error) => return error.response(request_id),
    };
    if let Err(error) = require_empty_body(&headers, &body) {
        return error.response(request_id);
    }
    match service
        .logout(
            audience,
            &security.session_secret,
            &security.csrf_token,
            &security.idempotency_key,
            &request_id,
        )
        .await
    {
        Ok(()) => empty_response(
            StatusCode::NO_CONTENT,
            Some(expired_session_cookie(audience)),
            request_id,
        ),
        Err(error) => error.response(request_id),
    }
}

struct MutationHeaders {
    session_secret: String,
    csrf_token: String,
    idempotency_key: String,
}

fn mutation_headers(
    service: &WorkforceService,
    audience: WorkforceAudience,
    headers: &HeaderMap,
) -> Result<MutationHeaders, WorkforceError> {
    if exact_header(headers, &ORIGIN)? != service.config(audience).allowed_origin
        || exact_header(headers, &FETCH_SITE_HEADER)? != "same-origin"
    {
        return Err(WorkforceError::Forbidden);
    }
    let session_secret = session_cookie_value(headers, audience)?;
    let csrf_token = exact_header(headers, &CSRF_HEADER)?;
    if !valid_secret(&csrf_token) {
        return Err(WorkforceError::InvalidRequest);
    }
    let idempotency_key = exact_header(headers, &IDEMPOTENCY_HEADER)?;
    if !(16..=128).contains(&idempotency_key.len())
        || !idempotency_key.is_ascii()
        || idempotency_key.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(WorkforceError::InvalidRequest);
    }
    Ok(MutationHeaders {
        session_secret,
        csrf_token,
        idempotency_key,
    })
}

fn audience_config(
    providers: Vec<Arc<dyn OidcProvider>>,
    allowed_origin: String,
    post_login_uri: String,
) -> Result<AudienceConfig, WorkforceBuildError> {
    if providers.is_empty() || providers.len() > MAX_PROVIDERS {
        return Err(WorkforceBuildError::InvalidConfiguration);
    }
    let origin = bare_https_origin(&allowed_origin)?;
    let post_login =
        Url::parse(&post_login_uri).map_err(|_| WorkforceBuildError::InvalidConfiguration)?;
    if post_login.origin().ascii_serialization() != origin
        || !post_login.username().is_empty()
        || post_login.password().is_some()
        || post_login.fragment().is_some()
    {
        return Err(WorkforceBuildError::InvalidConfiguration);
    }
    let mut by_key = HashMap::with_capacity(providers.len());
    let mut issuers = HashSet::with_capacity(providers.len());
    for provider in providers {
        if !valid_provider_key(provider.key())
            || provider.config_sha256().len() != 64
            || !provider
                .config_sha256()
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || !issuers.insert(provider.issuer().to_owned())
            || by_key.insert(provider.key().to_owned(), provider).is_some()
        {
            return Err(WorkforceBuildError::InvalidConfiguration);
        }
    }
    Ok(AudienceConfig {
        providers: by_key,
        allowed_origin: origin,
        post_login_uri,
    })
}

fn bare_https_origin(value: &str) -> Result<String, WorkforceBuildError> {
    let url = Url::parse(value).map_err(|_| WorkforceBuildError::InvalidConfiguration)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(WorkforceBuildError::InvalidConfiguration);
    }
    Ok(url.origin().ascii_serialization())
}

fn valid_provider_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && value.len() <= 32
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

async fn serializable(pool: &PgPool) -> Result<Transaction<'_, Postgres>, WorkforceError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
    Ok(transaction)
}

async fn database_now(transaction: &mut Transaction<'_, Postgres>) -> Result<i64, WorkforceError> {
    sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)
}

async fn database_now_pool(pool: &PgPool) -> Result<i64, WorkforceError> {
    sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(pool)
        .await
        .map_err(|_| WorkforceError::Unavailable)
}

async fn expire_oidc_transactions(
    transaction: &mut Transaction<'_, Postgres>,
    now: i64,
) -> Result<(), WorkforceError> {
    sqlx::query(
        "UPDATE workforce_oidc_transactions SET state = 'expired', \
         consumed_unix_seconds = GREATEST(requested_unix_seconds, $1) \
         WHERE state = 'pending' AND expires_unix_seconds <= $1",
    )
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(|_| WorkforceError::Unavailable)?;
    Ok(())
}

async fn terminally_expire_oidc(pool: &PgPool, state_sha256: &str) -> Result<(), WorkforceError> {
    sqlx::query(
        "UPDATE workforce_oidc_transactions SET state = 'expired', \
         consumed_unix_seconds = GREATEST(requested_unix_seconds, \
           EXTRACT(EPOCH FROM clock_timestamp())::BIGINT) \
         WHERE state_sha256 = $1 AND state = 'pending'",
    )
    .bind(state_sha256)
    .execute(pool)
    .await
    .map_err(|_| WorkforceError::Unavailable)?;
    Ok(())
}

async fn load_oidc_transaction_pool(
    pool: &PgPool,
    state_sha256: &str,
) -> Result<OidcTransactionRow, WorkforceError> {
    let row = sqlx::query(
        "SELECT transaction_id, provider_key, provider_config_sha256, audience, nonce_sha256, \
         pkce_verifier_ciphertext, state, expires_unix_seconds \
         FROM workforce_oidc_transactions WHERE state_sha256 = $1",
    )
    .bind(state_sha256)
    .fetch_optional(pool)
    .await
    .map_err(|_| WorkforceError::Unavailable)?
    .ok_or(WorkforceError::Unauthorized)?;
    Ok(oidc_row(row))
}

async fn load_oidc_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    state_sha256: &str,
    lock: bool,
) -> Result<Option<OidcTransactionRow>, WorkforceError> {
    let suffix = if lock { " FOR UPDATE" } else { "" };
    let query = format!(
        "SELECT transaction_id, provider_key, provider_config_sha256, audience, nonce_sha256, \
         pkce_verifier_ciphertext, state, expires_unix_seconds \
         FROM workforce_oidc_transactions WHERE state_sha256 = $1{suffix}"
    );
    let row = sqlx::query(&query)
        .bind(state_sha256)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
    Ok(row.map(oidc_row))
}

fn oidc_row(row: sqlx::postgres::PgRow) -> OidcTransactionRow {
    OidcTransactionRow {
        transaction_id: row.get("transaction_id"),
        provider_key: row.get("provider_key"),
        provider_config_sha256: row.get("provider_config_sha256"),
        audience: row.get("audience"),
        nonce_sha256: row.get("nonce_sha256"),
        pkce_verifier_ciphertext: row.get("pkce_verifier_ciphertext"),
        state: row.get("state"),
        expires_unix_seconds: row.get("expires_unix_seconds"),
    }
}

async fn lock_session(
    transaction: &mut Transaction<'_, Postgres>,
    session_sha256: &str,
) -> Result<SessionRow, WorkforceError> {
    let row = sqlx::query(
        "SELECT session.session_sha256, session.audience, \
         session.state, session.resource_version, session.created_unix_seconds, \
         session.last_seen_unix_seconds, session.idle_expires_unix_seconds, \
         session.absolute_expires_unix_seconds, session.mfa_authenticated_unix_seconds, \
         session.csrf_sha256, link.state AS link_state, \
         COALESCE(link.reviewer_id, link.operator_id) AS principal_id, \
         COALESCE(reviewer.state, operator.state) AS principal_state, \
         COALESCE(reviewer.role, operator.role) AS principal_role, \
         COALESCE(reviewer.two_factor_enabled, operator.two_factor_enabled) \
           AS principal_two_factor_enabled \
         FROM workforce_sessions session \
         JOIN workforce_identity_links link ON link.link_id = session.link_id \
         LEFT JOIN reviewers reviewer ON reviewer.reviewer_id = link.reviewer_id \
         LEFT JOIN store_operators operator ON operator.operator_id = link.operator_id \
         WHERE session.session_sha256 = $1 FOR UPDATE OF session",
    )
    .bind(session_sha256)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| WorkforceError::Unavailable)?
    .ok_or(WorkforceError::Unauthorized)?;
    Ok(SessionRow {
        session_sha256: row.get("session_sha256"),
        audience: row.get("audience"),
        state: row.get("state"),
        resource_version: row.get("resource_version"),
        created_unix_seconds: row.get("created_unix_seconds"),
        last_seen_unix_seconds: row.get("last_seen_unix_seconds"),
        idle_expires_unix_seconds: row.get("idle_expires_unix_seconds"),
        absolute_expires_unix_seconds: row.get("absolute_expires_unix_seconds"),
        mfa_authenticated_unix_seconds: row.get("mfa_authenticated_unix_seconds"),
        csrf_sha256: row.get("csrf_sha256"),
        link_state: row.get("link_state"),
        principal_id: row.get("principal_id"),
        principal_state: row.get("principal_state"),
        principal_role: row.get("principal_role"),
        principal_two_factor_enabled: row.get("principal_two_factor_enabled"),
    })
}

async fn ensure_active_session(
    transaction: &mut Transaction<'_, Postgres>,
    session: &SessionRow,
    audience: WorkforceAudience,
    now: i64,
) -> Result<bool, WorkforceError> {
    if session.audience != audience.as_str() {
        return Ok(false);
    }
    if session.state != "active" {
        return Ok(false);
    }
    let expired =
        now >= session.idle_expires_unix_seconds || now >= session.absolute_expires_unix_seconds;
    let unavailable = session.link_state != "active"
        || session.principal_state != "active"
        || !session.principal_two_factor_enabled;
    if expired || unavailable {
        sqlx::query(
            "UPDATE workforce_sessions SET state = $1, ended_unix_seconds = $2, \
             resource_version = resource_version + 1 WHERE session_sha256 = $3",
        )
        .bind(if expired { "expired" } else { "revoked" })
        .bind(now.max(session.created_unix_seconds))
        .bind(&session.session_sha256)
        .execute(&mut **transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)?;
        return Ok(false);
    }
    Ok(true)
}

async fn touch_session(
    transaction: &mut Transaction<'_, Postgres>,
    session: &mut SessionRow,
    now: i64,
) -> Result<(), WorkforceError> {
    if now <= session.last_seen_unix_seconds {
        return Ok(());
    }
    let idle = (now + SESSION_IDLE_SECONDS).min(session.absolute_expires_unix_seconds);
    sqlx::query(
        "UPDATE workforce_sessions SET last_seen_unix_seconds = $1, \
         idle_expires_unix_seconds = $2, resource_version = resource_version + 1 \
         WHERE session_sha256 = $3",
    )
    .bind(now)
    .bind(idle)
    .bind(&session.session_sha256)
    .execute(&mut **transaction)
    .await
    .map_err(|_| WorkforceError::Unavailable)?;
    session.last_seen_unix_seconds = now;
    session.idle_expires_unix_seconds = idle;
    session.resource_version += 1;
    Ok(())
}

fn allowed_scopes(session: &SessionRow) -> Vec<&'static str> {
    if session.audience == "review" {
        vec!["store.review"]
    } else if session.principal_role == "admin" {
        vec!["store.editorial", "store.moderation"]
    } else {
        vec!["store.editorial"]
    }
}

fn valid_scope(audience: WorkforceAudience, scope: &str) -> bool {
    match audience {
        WorkforceAudience::Review => scope == "store.review",
        WorkforceAudience::Operations => {
            matches!(scope, "store.editorial" | "store.moderation")
        }
    }
}

async fn load_issuance(
    transaction: &mut Transaction<'_, Postgres>,
    session_sha256: &str,
    audience: WorkforceAudience,
    idempotency_sha256: &str,
) -> Result<Option<ExistingIssuance>, WorkforceError> {
    let row = sqlx::query(
        "SELECT request_sha256, token_sha256, principal_id, scope, expires_unix_seconds \
         FROM workforce_control_token_issuances WHERE session_sha256 = $1 AND audience = $2 \
         AND idempotency_key_sha256 = $3",
    )
    .bind(session_sha256)
    .bind(audience.as_str())
    .bind(idempotency_sha256)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| WorkforceError::Unavailable)?;
    Ok(row.map(|row| ExistingIssuance {
        request_sha256: row.get("request_sha256"),
        token_sha256: row.get("token_sha256"),
        principal_id: row.get("principal_id"),
        scope: row.get("scope"),
        expires_unix_seconds: row.get("expires_unix_seconds"),
    }))
}

async fn access_token_revoked(
    transaction: &mut Transaction<'_, Postgres>,
    audience: WorkforceAudience,
    token_sha256: &str,
) -> Result<bool, WorkforceError> {
    let query = match audience {
        WorkforceAudience::Review => {
            "SELECT revoked FROM reviewer_access_tokens WHERE token_sha256 = $1"
        }
        WorkforceAudience::Operations => {
            "SELECT revoked FROM store_operator_access_tokens WHERE token_sha256 = $1"
        }
    };
    sqlx::query_scalar(query)
        .bind(token_sha256)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| WorkforceError::Unavailable)
}

#[allow(clippy::too_many_arguments)]
async fn append_audit(
    transaction: &mut Transaction<'_, Postgres>,
    now: i64,
    actor_id: &str,
    action: &str,
    object_kind: &str,
    object_id: &str,
    before_state: Option<&str>,
    after_state: Option<&str>,
    resource_version: i64,
    request_id: &str,
    request_sha256: &str,
    idempotency_key_sha256: &str,
) -> Result<(), WorkforceError> {
    sqlx::query(
        "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
         object_id, before_state, after_state, resource_version, request_id, request_sha256, \
         idempotency_key_sha256) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(now)
    .bind(actor_id)
    .bind(action)
    .bind(object_kind)
    .bind(object_id)
    .bind(before_state)
    .bind(after_state)
    .bind(resource_version)
    .bind(request_id)
    .bind(request_sha256)
    .bind(idempotency_key_sha256)
    .execute(&mut **transaction)
    .await
    .map_err(|_| WorkforceError::Unavailable)?;
    Ok(())
}

fn token_request_sha256(audience: WorkforceAudience, scope: &str) -> String {
    sha256_hex(
        format!(
            "CardputerZero workforce control token v1\0{}\0{scope}",
            audience.as_str()
        )
        .as_bytes(),
    )
}

fn logout_request_sha256(audience: WorkforceAudience) -> String {
    sha256_hex(
        format!(
            "CardputerZero workforce session logout v1\0{}",
            audience.as_str()
        )
        .as_bytes(),
    )
}

fn require_empty_body(headers: &HeaderMap, body: &[u8]) -> Result<(), WorkforceError> {
    if !body.is_empty() || headers.contains_key("transfer-encoding") {
        return Err(WorkforceError::InvalidRequest);
    }
    if let Some(length) = headers.get(CONTENT_LENGTH) {
        if length.to_str().ok() != Some("0") {
            return Err(WorkforceError::InvalidRequest);
        }
    }
    Ok(())
}

fn session_cookie_value(
    headers: &HeaderMap,
    audience: WorkforceAudience,
) -> Result<String, WorkforceError> {
    let mut found = None;
    let mut cookie_headers = headers.get_all(COOKIE).iter();
    let Some(header) = cookie_headers.next() else {
        return Err(WorkforceError::Unauthorized);
    };
    if cookie_headers.next().is_some() {
        return Err(WorkforceError::InvalidRequest);
    }
    let encoded = header
        .to_str()
        .map_err(|_| WorkforceError::InvalidRequest)?;
    if encoded.len() > 4096 {
        return Err(WorkforceError::InvalidRequest);
    }
    for part in encoded.split(';') {
        let Some((name, value)) = part.trim().split_once('=') else {
            return Err(WorkforceError::InvalidRequest);
        };
        if name == audience.cookie_name() {
            if found.is_some() || !valid_secret(value) {
                return Err(WorkforceError::InvalidRequest);
            }
            found = Some(value.to_owned());
        }
    }
    found.ok_or(WorkforceError::Unauthorized)
}

fn valid_secret(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        && URL_SAFE_NO_PAD
            .decode(value)
            .is_ok_and(|decoded| decoded.len() == 32)
}

fn exact_header(headers: &HeaderMap, name: &HeaderName) -> Result<String, WorkforceError> {
    let mut values = headers.get_all(name).iter();
    let value = values.next().ok_or(WorkforceError::InvalidRequest)?;
    if values.next().is_some() {
        return Err(WorkforceError::InvalidRequest);
    }
    value
        .to_str()
        .map(str::to_owned)
        .map_err(|_| WorkforceError::InvalidRequest)
}

fn opaque_id(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().simple())
}

fn request_id() -> String {
    opaque_id("req_")
}

fn map_oidc_error(error: OidcError) -> WorkforceError {
    match error {
        OidcError::InvalidConfiguration => WorkforceError::Internal,
        OidcError::InvalidRequest | OidcError::InvalidToken => WorkforceError::Unauthorized,
        OidcError::ProviderUnavailable => WorkforceError::Unavailable,
    }
}

impl WorkforceError {
    fn response(self, request_id: String) -> Response {
        let (status, code, title) = match self {
            Self::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                "invalid-request",
                "Invalid request",
            ),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", "Unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden", "Forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not-found", "Not found"),
            Self::Conflict => (StatusCode::CONFLICT, "conflict", "Conflict"),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "temporarily-unavailable",
                "Temporarily unavailable",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "Internal error",
            ),
        };
        let problem = json!({
            "type": format!("https://cardputerzero.dev/problems/{code}"),
            "title": title,
            "status": status.as_u16(),
            "code": code,
            "request_id": request_id
        });
        json_response(status, &problem, None, request_id)
    }
}

fn json_response<T: Serialize>(
    status: StatusCode,
    body: &T,
    version: Option<i64>,
    request_id: String,
) -> Response {
    let mut response = match serde_json::to_vec(body) {
        Ok(encoded) => Response::new(Body::from(encoded)),
        Err(_) => return WorkforceError::Internal.response(request_id),
    };
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static(if status.is_success() {
            "application/json"
        } else {
            "application/problem+json"
        }),
    );
    if let Some(version) = version {
        if let Ok(value) = HeaderValue::from_str(&format!("\"{version}\"")) {
            response.headers_mut().insert("etag", value);
        }
    }
    secure_headers(&mut response, &request_id);
    response
}

fn redirect_response(
    status: StatusCode,
    location: &str,
    cookie: Option<String>,
    request_id: String,
) -> Response {
    let mut response = status.into_response();
    let Ok(location) = HeaderValue::from_str(location) else {
        return WorkforceError::Internal.response(request_id);
    };
    response.headers_mut().insert(LOCATION, location);
    if let Some(cookie) = cookie {
        if let Ok(cookie) = HeaderValue::from_str(&cookie) {
            response.headers_mut().insert(SET_COOKIE, cookie);
        }
    }
    secure_headers(&mut response, &request_id);
    response
}

fn empty_response(status: StatusCode, cookie: Option<String>, request_id: String) -> Response {
    let mut response = status.into_response();
    if let Some(cookie) = cookie {
        if let Ok(cookie) = HeaderValue::from_str(&cookie) {
            response.headers_mut().insert(SET_COOKIE, cookie);
        }
    }
    secure_headers(&mut response, &request_id);
    response
}

fn secure_headers(response: &mut Response, request_id: &str) {
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(
        REFERRER_POLICY_HEADER.clone(),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        CONTENT_TYPE_OPTIONS_HEADER.clone(),
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(value) = HeaderValue::from_str(request_id) {
        headers.insert(REQUEST_ID_HEADER.clone(), value);
    }
}

fn session_cookie(audience: WorkforceAudience, secret: &str) -> String {
    format!(
        "{}={secret}; Path=/; Max-Age={SESSION_ABSOLUTE_SECONDS}; Secure; HttpOnly; SameSite=Strict",
        audience.cookie_name()
    )
}

fn expired_session_cookie(audience: WorkforceAudience) -> String {
    format!(
        "{}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict",
        audience.cookie_name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audience_cookies_and_scopes_are_separate() {
        assert_ne!(
            WorkforceAudience::Review.cookie_name(),
            WorkforceAudience::Operations.cookie_name()
        );
        assert!(valid_scope(WorkforceAudience::Review, "store.review"));
        assert!(!valid_scope(WorkforceAudience::Review, "store.editorial"));
        assert!(valid_scope(
            WorkforceAudience::Operations,
            "store.moderation"
        ));
    }

    #[test]
    fn cookie_parser_rejects_cross_audience_and_duplicates() {
        let review = URL_SAFE_NO_PAD.encode([7_u8; 32]);
        let operations = URL_SAFE_NO_PAD.encode([8_u8; 32]);
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!(
                "__Host-cp0_review={review}; __Host-cp0_operations={operations}"
            ))
            .unwrap(),
        );
        assert_eq!(
            session_cookie_value(&headers, WorkforceAudience::Review).unwrap(),
            review
        );
        assert_eq!(
            session_cookie_value(&headers, WorkforceAudience::Operations).unwrap(),
            operations
        );
        headers.append(COOKIE, HeaderValue::from_static("other=1"));
        assert!(session_cookie_value(&headers, WorkforceAudience::Review).is_err());
    }
}
