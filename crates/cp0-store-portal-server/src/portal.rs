use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, ETAG, LOCATION, ORIGIN, PRAGMA, SET_COOKIE,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Row, Transaction};
use url::Url;
use uuid::Uuid;

use crate::{AuthIntent, OidcError, OidcProvider, PortalSecrets, sha256_hex};

const SESSION_COOKIE: &str = "__Host-cp0_portal";
const SESSION_IDLE_SECONDS: i64 = 1800;
const SESSION_ABSOLUTE_SECONDS: i64 = 28800;
const OIDC_TRANSACTION_SECONDS: i64 = 600;
const MAX_PROVIDERS: usize = 8;
const MAX_REQUEST_BYTES: usize = 1024;

static CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");
static IDEMPOTENCY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");
static IF_MATCH_HEADER: HeaderName = HeaderName::from_static("if-match");
static FETCH_SITE_HEADER: HeaderName = HeaderName::from_static("sec-fetch-site");
static REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");
static REFERRER_POLICY_HEADER: HeaderName = HeaderName::from_static("referrer-policy");
static CONTENT_TYPE_OPTIONS_HEADER: HeaderName = HeaderName::from_static("x-content-type-options");

#[derive(Debug)]
pub enum PortalBuildError {
    InvalidConfiguration,
}

#[derive(Clone)]
pub struct PortalService {
    inner: Arc<PortalServiceInner>,
}

struct PortalServiceInner {
    pool: PgPool,
    secrets: PortalSecrets,
    providers: HashMap<String, Arc<dyn OidcProvider>>,
    allowed_origin: String,
    post_login_uri: String,
}

#[derive(Clone, Debug, Serialize)]
struct TeamSummary {
    team_id: String,
    name: String,
    role: String,
    membership_state: String,
    resource_version: i64,
}

#[derive(Clone, Debug, Serialize)]
struct PortalSessionResponse {
    account_id: String,
    email: String,
    email_verified: bool,
    csrf_token: String,
    mfa_step_up_fresh: bool,
    idle_expires_unix_seconds: i64,
    absolute_expires_unix_seconds: i64,
    teams: Vec<TeamSummary>,
    resource_version: i64,
}

#[derive(Debug, Serialize)]
struct AuthorizationRedirect {
    authorization_uri: String,
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

#[derive(Debug)]
struct SessionRow {
    session_sha256: String,
    account_id: String,
    current_link_id: String,
    provider_key: String,
    issuer: String,
    subject_hmac_sha256: String,
    email: String,
    email_verified: bool,
    state: String,
    resource_version: i64,
    created_unix_seconds: i64,
    last_seen_unix_seconds: i64,
    idle_expires_unix_seconds: i64,
    absolute_expires_unix_seconds: i64,
    mfa_authenticated_unix_seconds: Option<i64>,
    csrf_sha256: String,
}

#[derive(Debug)]
struct OidcTransactionRow {
    transaction_id: String,
    provider_key: String,
    provider_config_sha256: String,
    intent: AuthIntent,
    account_id: Option<String>,
    session_sha256: Option<String>,
    nonce_sha256: String,
    pkce_verifier_ciphertext: Vec<u8>,
    state: String,
    expires_unix_seconds: i64,
}

#[derive(Debug)]
struct CreatedSession {
    secret: String,
}

#[derive(Clone, Copy, Debug)]
enum PortalError {
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    PreconditionRequired,
    PreconditionFailed,
    Unavailable,
    Internal,
}

#[derive(Serialize)]
struct Problem {
    r#type: String,
    title: &'static str,
    status: u16,
    code: &'static str,
    request_id: String,
}

impl PortalService {
    pub fn new(
        pool: PgPool,
        secrets: PortalSecrets,
        providers: Vec<Arc<dyn OidcProvider>>,
        allowed_origin: String,
        post_login_uri: String,
    ) -> Result<Self, PortalBuildError> {
        let origin = exact_https_origin(&allowed_origin)?;
        let post_login =
            Url::parse(&post_login_uri).map_err(|_| PortalBuildError::InvalidConfiguration)?;
        if post_login.origin().ascii_serialization() != origin
            || post_login.username() != ""
            || post_login.password().is_some()
            || post_login.query().is_some()
            || post_login.fragment().is_some()
        {
            return Err(PortalBuildError::InvalidConfiguration);
        }
        if providers.is_empty() || providers.len() > MAX_PROVIDERS {
            return Err(PortalBuildError::InvalidConfiguration);
        }
        let mut by_key = HashMap::with_capacity(providers.len());
        let mut issuers = std::collections::HashSet::new();
        for provider in providers {
            if !valid_provider_key(provider.key())
                || provider.config_sha256().len() != 64
                || !issuers.insert(provider.issuer().to_owned())
                || by_key.insert(provider.key().to_owned(), provider).is_some()
            {
                return Err(PortalBuildError::InvalidConfiguration);
            }
        }
        Ok(Self {
            inner: Arc::new(PortalServiceInner {
                pool,
                secrets,
                providers: by_key,
                allowed_origin: origin,
                post_login_uri,
            }),
        })
    }

    async fn begin_login(&self, provider_key: &str) -> Result<String, PortalError> {
        let provider = self.provider(provider_key)?;
        let state = self
            .inner
            .secrets
            .random_token()
            .map_err(|_| PortalError::Internal)?;
        let nonce = self.inner.secrets.nonce_for_state(&state);
        let pkce_verifier = self
            .inner
            .secrets
            .random_token()
            .map_err(|_| PortalError::Internal)?;
        let authorization_uri = provider
            .authorization_uri(AuthIntent::Login, &state, &nonce, &pkce_verifier)
            .map_err(map_oidc_error)?;
        let encrypted = self
            .inner
            .secrets
            .encrypt_pkce(&pkce_verifier)
            .map_err(|_| PortalError::Internal)?;
        let state_sha256 = sha256_hex(state.as_bytes());
        let nonce_sha256 = sha256_hex(nonce.as_bytes());
        let transaction_id = opaque_id("oidctx_");
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        expire_oidc_transactions(&mut transaction, now).await?;
        sqlx::query(
            "INSERT INTO oidc_login_transactions (transaction_id, state_sha256, nonce_sha256, \
             pkce_verifier_ciphertext, provider_key, provider_config_sha256, intent, state, \
             requested_unix_seconds, expires_unix_seconds) VALUES \
             ($1, $2, $3, $4, $5, $6, 'login', 'pending', $7, $7 + $8)",
        )
        .bind(transaction_id)
        .bind(state_sha256)
        .bind(nonce_sha256)
        .bind(encrypted)
        .bind(provider.key())
        .bind(provider.config_sha256())
        .bind(now)
        .bind(OIDC_TRANSACTION_SECONDS)
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(authorization_uri)
    }

    async fn begin_step_up(
        &self,
        session_secret: &str,
        csrf_token: &str,
        idempotency_key: &str,
        expected_version: i64,
    ) -> Result<String, PortalError> {
        let session_sha256 = sha256_hex(session_secret.as_bytes());
        let csrf_sha256 = sha256_hex(csrf_token.as_bytes());
        let state = self.inner.secrets.state_for_action(
            &session_sha256,
            "session-step-up",
            idempotency_key,
        );
        let nonce = self.inner.secrets.nonce_for_state(&state);
        let state_sha256 = sha256_hex(state.as_bytes());
        let nonce_sha256 = sha256_hex(nonce.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        expire_oidc_transactions(&mut transaction, now).await?;
        let session = lock_session(&mut transaction, &session_sha256).await?;
        if !ensure_active_session(&mut transaction, &session, now).await? {
            transaction
                .commit()
                .await
                .map_err(|_| PortalError::Unavailable)?;
            return Err(PortalError::Unauthorized);
        }
        if session.csrf_sha256 != csrf_sha256 {
            return Err(PortalError::Forbidden);
        }
        if session.resource_version != expected_version {
            return Err(PortalError::PreconditionFailed);
        }
        let provider = self.provider(&session.provider_key)?;
        if provider.issuer() != session.issuer {
            return Err(PortalError::Forbidden);
        }
        let existing = load_oidc_transaction(&mut transaction, &state_sha256, true).await?;
        let pkce_verifier = if let Some(existing) = existing {
            if existing.intent != AuthIntent::StepUp
                || existing.account_id.as_deref() != Some(session.account_id.as_str())
                || existing.session_sha256.as_deref() != Some(session.session_sha256.as_str())
                || existing.provider_key != provider.key()
                || existing.provider_config_sha256 != provider.config_sha256()
                || existing.nonce_sha256 != nonce_sha256
                || existing.state != "pending"
                || existing.expires_unix_seconds <= now
            {
                return Err(PortalError::Conflict);
            }
            self.inner
                .secrets
                .decrypt_pkce(&existing.pkce_verifier_ciphertext)
                .map_err(|_| PortalError::Internal)?
        } else {
            let verifier = self
                .inner
                .secrets
                .random_token()
                .map_err(|_| PortalError::Internal)?;
            let encrypted = self
                .inner
                .secrets
                .encrypt_pkce(&verifier)
                .map_err(|_| PortalError::Internal)?;
            sqlx::query(
                "INSERT INTO oidc_login_transactions (transaction_id, state_sha256, \
                 nonce_sha256, pkce_verifier_ciphertext, provider_key, provider_config_sha256, \
                 intent, account_id, session_sha256, state, requested_unix_seconds, \
                 expires_unix_seconds) VALUES ($1, $2, $3, $4, $5, $6, 'step-up', $7, $8, \
                 'pending', $9, $9 + $10)",
            )
            .bind(opaque_id("oidctx_"))
            .bind(&state_sha256)
            .bind(&nonce_sha256)
            .bind(encrypted)
            .bind(provider.key())
            .bind(provider.config_sha256())
            .bind(&session.account_id)
            .bind(&session.session_sha256)
            .bind(now)
            .bind(OIDC_TRANSACTION_SECONDS)
            .execute(&mut *transaction)
            .await
            .map_err(|_| PortalError::Unavailable)?;
            zeroize::Zeroizing::new(verifier)
        };
        let authorization_uri = provider
            .authorization_uri(AuthIntent::StepUp, &state, &nonce, &pkce_verifier)
            .map_err(map_oidc_error)?;
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(authorization_uri)
    }

    async fn complete_callback(
        &self,
        code: &str,
        state: &str,
    ) -> Result<CreatedSession, PortalError> {
        if !valid_secret(state)
            || !(16..=4096).contains(&code.len())
            || code.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(PortalError::InvalidRequest);
        }
        let state_sha256 = sha256_hex(state.as_bytes());
        let now = database_now_pool(&self.inner.pool).await?;
        let oidc = load_oidc_transaction_pool(&self.inner.pool, &state_sha256).await?;
        if oidc.state != "pending" || oidc.expires_unix_seconds <= now {
            return Err(PortalError::Unauthorized);
        }
        let provider = self.provider(&oidc.provider_key)?;
        if oidc.provider_config_sha256 != provider.config_sha256() {
            return Err(PortalError::Unauthorized);
        }
        let nonce = self.inner.secrets.nonce_for_state(state);
        if sha256_hex(nonce.as_bytes()) != oidc.nonce_sha256 {
            return Err(PortalError::Unauthorized);
        }
        let pkce_verifier = self
            .inner
            .secrets
            .decrypt_pkce(&oidc.pkce_verifier_ciphertext)
            .map_err(|_| PortalError::Internal)?;
        let identity = match provider
            .exchange(oidc.intent, code, &nonce, &pkce_verifier, now)
            .await
        {
            Ok(identity) => identity,
            Err(OidcError::InvalidToken | OidcError::InvalidRequest) => {
                let _ = terminally_expire_oidc(&self.inner.pool, &state_sha256).await;
                return Err(PortalError::Unauthorized);
            }
            Err(error) => return Err(map_oidc_error(error)),
        };
        if identity.issuer != provider.issuer() || !identity.email_verified {
            let _ = terminally_expire_oidc(&self.inner.pool, &state_sha256).await;
            return Err(PortalError::Unauthorized);
        }
        let subject_hmac = self
            .inner
            .secrets
            .subject_hmac(&identity.issuer, &identity.subject);
        let session_secret = self
            .inner
            .secrets
            .random_token()
            .map_err(|_| PortalError::Internal)?;
        let new_session_sha256 = sha256_hex(session_secret.as_bytes());
        let csrf_token = self.inner.secrets.csrf_for_session(&session_secret);
        let csrf_sha256 = sha256_hex(csrf_token.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let commit_now = database_now(&mut transaction).await?;
        let locked = load_oidc_transaction(&mut transaction, &state_sha256, true)
            .await?
            .ok_or(PortalError::Unauthorized)?;
        if locked.transaction_id != oidc.transaction_id
            || locked.state != "pending"
            || locked.expires_unix_seconds <= commit_now
            || locked.provider_config_sha256 != provider.config_sha256()
        {
            return Err(PortalError::Unauthorized);
        }
        let (account_id, current_link_id, mfa_time) = match locked.intent {
            AuthIntent::Login => {
                let link = sqlx::query(
                    "SELECT link_id, account_id, state FROM external_identity_links \
                     WHERE issuer = $1 AND subject_hmac_sha256 = $2 FOR UPDATE",
                )
                .bind(&identity.issuer)
                .bind(&subject_hmac)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|_| PortalError::Unavailable)?;
                if let Some(link) = link {
                    if link.get::<String, _>("state") != "active" {
                        return Err(PortalError::Forbidden);
                    }
                    let account_id = link.get::<String, _>("account_id");
                    require_active_account(&mut transaction, &account_id).await?;
                    (
                        account_id,
                        link.get::<String, _>("link_id"),
                        identity.mfa_authenticated_unix_seconds,
                    )
                } else {
                    let email_owner: Option<String> = sqlx::query_scalar(
                        "SELECT account_id FROM portal_accounts WHERE email = $1",
                    )
                    .bind(&identity.email)
                    .fetch_optional(&mut *transaction)
                    .await
                    .map_err(|_| PortalError::Unavailable)?;
                    if email_owner.is_some() {
                        return Err(PortalError::Conflict);
                    }
                    let account_id = opaque_id("account_");
                    let link_id = opaque_id("link_");
                    sqlx::query(
                        "INSERT INTO portal_accounts (account_id, email, email_verified, state, \
                         created_unix_seconds) VALUES ($1, $2, TRUE, 'active', $3)",
                    )
                    .bind(&account_id)
                    .bind(&identity.email)
                    .bind(commit_now)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|_| PortalError::Unavailable)?;
                    sqlx::query(
                        "INSERT INTO external_identity_links (link_id, account_id, provider_key, \
                         issuer, subject_hmac_sha256, state, linked_unix_seconds) VALUES \
                         ($1, $2, $3, $4, $5, 'active', $6)",
                    )
                    .bind(&link_id)
                    .bind(&account_id)
                    .bind(provider.key())
                    .bind(&identity.issuer)
                    .bind(&subject_hmac)
                    .bind(commit_now)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|_| PortalError::Unavailable)?;
                    (account_id, link_id, identity.mfa_authenticated_unix_seconds)
                }
            }
            AuthIntent::StepUp => {
                let old_session_sha256 = locked
                    .session_sha256
                    .as_deref()
                    .ok_or(PortalError::Unauthorized)?;
                let old = lock_session(&mut transaction, old_session_sha256).await?;
                if !ensure_active_session(&mut transaction, &old, commit_now).await? {
                    transaction
                        .commit()
                        .await
                        .map_err(|_| PortalError::Unavailable)?;
                    return Err(PortalError::Unauthorized);
                }
                if locked.account_id.as_deref() != Some(old.account_id.as_str())
                    || old.provider_key != provider.key()
                    || old.issuer != identity.issuer
                    || old.subject_hmac_sha256 != subject_hmac
                {
                    return Err(PortalError::Unauthorized);
                }
                let mfa_time = identity
                    .mfa_authenticated_unix_seconds
                    .filter(|value| *value <= commit_now && *value >= commit_now - 300)
                    .ok_or(PortalError::Unauthorized)?;
                (old.account_id, old.current_link_id, Some(mfa_time))
            }
            AuthIntent::Link => return Err(PortalError::InvalidRequest),
        };
        sqlx::query(
            "UPDATE oidc_login_transactions SET state = 'consumed', consumed_unix_seconds = $1 \
             WHERE transaction_id = $2 AND state = 'pending'",
        )
        .bind(commit_now)
        .bind(&locked.transaction_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        if let Some(old_session_sha256) = locked.session_sha256.as_deref() {
            sqlx::query(
                "UPDATE portal_sessions SET state = 'revoked', ended_unix_seconds = $1, \
                 resource_version = resource_version + 1 \
                 WHERE session_sha256 = $2 AND state = 'active'",
            )
            .bind(commit_now)
            .bind(old_session_sha256)
            .execute(&mut *transaction)
            .await
            .map_err(|_| PortalError::Unavailable)?;
        }
        sqlx::query(
            "INSERT INTO portal_sessions (session_sha256, csrf_sha256, account_id, \
             current_link_id, state, created_unix_seconds, last_seen_unix_seconds, \
             idle_expires_unix_seconds, absolute_expires_unix_seconds, \
             mfa_authenticated_unix_seconds) VALUES \
             ($1, $2, $3, $4, 'active', $5, $5, $5 + $6, $5 + $7, $8)",
        )
        .bind(new_session_sha256)
        .bind(csrf_sha256)
        .bind(account_id)
        .bind(current_link_id)
        .bind(commit_now)
        .bind(SESSION_IDLE_SECONDS)
        .bind(SESSION_ABSOLUTE_SECONDS)
        .bind(mfa_time.filter(|value| *value <= commit_now))
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(CreatedSession {
            secret: session_secret,
        })
    }

    async fn get_session(
        &self,
        session_secret: &str,
    ) -> Result<PortalSessionResponse, PortalError> {
        let session_sha256 = sha256_hex(session_secret.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let mut session = lock_session(&mut transaction, &session_sha256).await?;
        if !ensure_active_session(&mut transaction, &session, now).await? {
            transaction
                .commit()
                .await
                .map_err(|_| PortalError::Unavailable)?;
            return Err(PortalError::Unauthorized);
        }
        if now > session.last_seen_unix_seconds {
            session.last_seen_unix_seconds = now;
            session.idle_expires_unix_seconds =
                (now + SESSION_IDLE_SECONDS).min(session.absolute_expires_unix_seconds);
            session.resource_version += 1;
            sqlx::query(
                "UPDATE portal_sessions SET last_seen_unix_seconds = $1, \
                 idle_expires_unix_seconds = $2, resource_version = $3 \
                 WHERE session_sha256 = $4",
            )
            .bind(session.last_seen_unix_seconds)
            .bind(session.idle_expires_unix_seconds)
            .bind(session.resource_version)
            .bind(&session.session_sha256)
            .execute(&mut *transaction)
            .await
            .map_err(|_| PortalError::Unavailable)?;
        }
        let teams = sqlx::query(
            "SELECT team.team_id, team.name, member.role, member.membership_state, \
             team.resource_version FROM team_members member \
             JOIN teams team ON team.team_id = member.team_id \
             WHERE member.account_id = $1 AND member.membership_state <> 'removed' \
             ORDER BY team.team_id LIMIT 9",
        )
        .bind(&session.account_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        if teams.len() > 8 {
            return Err(PortalError::Internal);
        }
        let teams = teams
            .into_iter()
            .map(|row| TeamSummary {
                team_id: row.get("team_id"),
                name: row.get("name"),
                role: row.get("role"),
                membership_state: row.get("membership_state"),
                resource_version: row.get("resource_version"),
            })
            .collect();
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(PortalSessionResponse {
            account_id: session.account_id,
            email: session.email,
            email_verified: session.email_verified,
            csrf_token: self.inner.secrets.csrf_for_session(session_secret),
            mfa_step_up_fresh: session
                .mfa_authenticated_unix_seconds
                .is_some_and(|value| value >= now - 300 && value <= now),
            idle_expires_unix_seconds: session.idle_expires_unix_seconds,
            absolute_expires_unix_seconds: session.absolute_expires_unix_seconds,
            teams,
            resource_version: session.resource_version,
        })
    }

    async fn logout(&self, session_secret: &str, csrf_token: &str) -> Result<(), PortalError> {
        let session_sha256 = sha256_hex(session_secret.as_bytes());
        let csrf_sha256 = sha256_hex(csrf_token.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let row = sqlx::query(
            "SELECT state, csrf_sha256, created_unix_seconds FROM portal_sessions \
             WHERE session_sha256 = $1 FOR UPDATE",
        )
        .bind(&session_sha256)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?
        .ok_or(PortalError::Unauthorized)?;
        if row.get::<String, _>("csrf_sha256") != csrf_sha256 {
            return Err(PortalError::Forbidden);
        }
        if row.get::<String, _>("state") == "active" {
            let ended = now.max(row.get::<i64, _>("created_unix_seconds"));
            sqlx::query(
                "UPDATE portal_sessions SET state = 'revoked', ended_unix_seconds = $1, \
                 resource_version = resource_version + 1 WHERE session_sha256 = $2",
            )
            .bind(ended)
            .bind(&session_sha256)
            .execute(&mut *transaction)
            .await
            .map_err(|_| PortalError::Unavailable)?;
        }
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)
    }

    fn provider(&self, key: &str) -> Result<Arc<dyn OidcProvider>, PortalError> {
        self.inner
            .providers
            .get(key)
            .cloned()
            .ok_or(PortalError::NotFound)
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

pub fn router(service: PortalService) -> Router {
    Router::new()
        .route("/portal/auth/login", get(begin_login))
        .route("/portal/auth/callback", get(complete_callback))
        .route("/portal/v1/session", get(get_session))
        .route(
            "/portal/v1/session:logout",
            post(logout).layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route(
            "/portal/v1/session:step-up",
            post(begin_step_up).layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .with_state(service)
}

async fn begin_login(
    State(service): State<PortalService>,
    query: Result<Query<LoginQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let request_id = request_id();
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return PortalError::InvalidRequest.response(request_id),
    };
    match service.begin_login(&query.provider).await {
        Ok(location) => redirect_response(StatusCode::FOUND, &location, None, request_id),
        Err(error) => error.response(request_id),
    }
}

async fn complete_callback(
    State(service): State<PortalService>,
    query: Result<Query<CallbackQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let request_id = request_id();
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return PortalError::InvalidRequest.response(request_id),
    };
    match service.complete_callback(&query.code, &query.state).await {
        Ok(session) => redirect_response(
            StatusCode::SEE_OTHER,
            &service.inner.post_login_uri,
            Some(session_cookie(&session.secret)),
            request_id,
        ),
        Err(error) => error.response(request_id),
    }
}

async fn get_session(State(service): State<PortalService>, headers: HeaderMap) -> Response {
    let request_id = request_id();
    let secret = match session_cookie_value(&headers) {
        Ok(secret) => secret,
        Err(error) => return error.response(request_id),
    };
    match service.get_session(&secret).await {
        Ok(session) => {
            let version = session.resource_version;
            json_response(StatusCode::OK, &session, Some(version), request_id)
        }
        Err(error) => error.response(request_id),
    }
}

async fn logout(State(service): State<PortalService>, headers: HeaderMap, body: Bytes) -> Response {
    let request_id = request_id();
    let security = match mutation_headers(&service, &headers, false) {
        Ok(security) => security,
        Err(error) => return error.response(request_id),
    };
    if let Err(error) = require_empty_body(&headers, &body) {
        return error.response(request_id);
    }
    match service
        .logout(&security.session_secret, &security.csrf_token)
        .await
    {
        Ok(()) => empty_response(
            StatusCode::NO_CONTENT,
            Some(expired_session_cookie()),
            request_id,
        ),
        Err(error) => error.response(request_id),
    }
}

async fn begin_step_up(
    State(service): State<PortalService>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request_id = request_id();
    let security = match mutation_headers(&service, &headers, true) {
        Ok(security) => security,
        Err(error) => return error.response(request_id),
    };
    if let Err(error) = require_empty_body(&headers, &body) {
        return error.response(request_id);
    }
    let expected_version = security.expected_version.expect("required above");
    match service
        .begin_step_up(
            &security.session_secret,
            &security.csrf_token,
            &security.idempotency_key,
            expected_version,
        )
        .await
    {
        Ok(authorization_uri) => json_response(
            StatusCode::OK,
            &AuthorizationRedirect { authorization_uri },
            None,
            request_id,
        ),
        Err(error) => error.response(request_id),
    }
}

struct MutationHeaders {
    session_secret: String,
    csrf_token: String,
    idempotency_key: String,
    expected_version: Option<i64>,
}

fn mutation_headers(
    service: &PortalService,
    headers: &HeaderMap,
    require_if_match: bool,
) -> Result<MutationHeaders, PortalError> {
    if exact_header(headers, &ORIGIN)? != service.inner.allowed_origin
        || exact_header(headers, &FETCH_SITE_HEADER)? != "same-origin"
    {
        return Err(PortalError::Forbidden);
    }
    let session_secret = session_cookie_value(headers)?;
    let csrf_token = exact_header(headers, &CSRF_HEADER)?;
    if !valid_secret(&csrf_token) {
        return Err(PortalError::InvalidRequest);
    }
    let idempotency_key = exact_header(headers, &IDEMPOTENCY_HEADER)?;
    if !(16..=128).contains(&idempotency_key.len())
        || !idempotency_key.is_ascii()
        || idempotency_key.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(PortalError::InvalidRequest);
    }
    let expected_version = if require_if_match {
        if !headers.contains_key(&IF_MATCH_HEADER) {
            return Err(PortalError::PreconditionRequired);
        }
        Some(parse_etag(&exact_header(headers, &IF_MATCH_HEADER)?)?)
    } else {
        None
    };
    Ok(MutationHeaders {
        session_secret,
        csrf_token,
        idempotency_key,
        expected_version,
    })
}

fn require_empty_body(headers: &HeaderMap, body: &[u8]) -> Result<(), PortalError> {
    if !body.is_empty() {
        return Err(PortalError::InvalidRequest);
    }
    if headers.contains_key("transfer-encoding") {
        return Err(PortalError::InvalidRequest);
    }
    if let Some(length) = headers.get(CONTENT_LENGTH) {
        if length.to_str().ok() != Some("0") {
            return Err(PortalError::InvalidRequest);
        }
    }
    Ok(())
}

async fn serializable(pool: &PgPool) -> Result<Transaction<'_, Postgres>, PortalError> {
    let mut transaction = pool.begin().await.map_err(|_| PortalError::Unavailable)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
    Ok(transaction)
}

async fn database_now(transaction: &mut Transaction<'_, Postgres>) -> Result<i64, PortalError> {
    sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| PortalError::Unavailable)
}

async fn database_now_pool(pool: &PgPool) -> Result<i64, PortalError> {
    sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(pool)
        .await
        .map_err(|_| PortalError::Unavailable)
}

async fn expire_oidc_transactions(
    transaction: &mut Transaction<'_, Postgres>,
    now: i64,
) -> Result<(), PortalError> {
    sqlx::query(
        "UPDATE oidc_login_transactions SET state = 'expired', consumed_unix_seconds = $1 \
         WHERE state = 'pending' AND expires_unix_seconds <= $1",
    )
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    Ok(())
}

async fn terminally_expire_oidc(pool: &PgPool, state_sha256: &str) -> Result<(), PortalError> {
    let now = database_now_pool(pool).await?;
    sqlx::query(
        "UPDATE oidc_login_transactions SET state = 'expired', consumed_unix_seconds = $1 \
         WHERE state_sha256 = $2 AND state = 'pending'",
    )
    .bind(now)
    .bind(state_sha256)
    .execute(pool)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    Ok(())
}

async fn load_oidc_transaction_pool(
    pool: &PgPool,
    state_sha256: &str,
) -> Result<OidcTransactionRow, PortalError> {
    let row = sqlx::query(
        "SELECT transaction_id, provider_key, provider_config_sha256, intent, account_id, \
         session_sha256, nonce_sha256, pkce_verifier_ciphertext, state, \
         expires_unix_seconds FROM oidc_login_transactions \
         WHERE state_sha256 = $1",
    )
    .bind(state_sha256)
    .fetch_optional(pool)
    .await
    .map_err(|_| PortalError::Unavailable)?
    .ok_or(PortalError::Unauthorized)?;
    oidc_row(row)
}

async fn load_oidc_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    state_sha256: &str,
    lock: bool,
) -> Result<Option<OidcTransactionRow>, PortalError> {
    let suffix = if lock { " FOR UPDATE" } else { "" };
    let query = format!(
        "SELECT transaction_id, provider_key, provider_config_sha256, intent, account_id, \
         session_sha256, nonce_sha256, pkce_verifier_ciphertext, state, \
         expires_unix_seconds FROM oidc_login_transactions \
         WHERE state_sha256 = $1{suffix}"
    );
    let row = sqlx::query(&query)
        .bind(state_sha256)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
    row.map(oidc_row).transpose()
}

fn oidc_row(row: sqlx::postgres::PgRow) -> Result<OidcTransactionRow, PortalError> {
    let intent = match row.get::<String, _>("intent").as_str() {
        "login" => AuthIntent::Login,
        "step-up" => AuthIntent::StepUp,
        "link" => AuthIntent::Link,
        _ => return Err(PortalError::Internal),
    };
    Ok(OidcTransactionRow {
        transaction_id: row.get("transaction_id"),
        provider_key: row.get("provider_key"),
        provider_config_sha256: row.get("provider_config_sha256"),
        intent,
        account_id: row.get("account_id"),
        session_sha256: row.get("session_sha256"),
        nonce_sha256: row.get("nonce_sha256"),
        pkce_verifier_ciphertext: row.get("pkce_verifier_ciphertext"),
        state: row.get("state"),
        expires_unix_seconds: row.get("expires_unix_seconds"),
    })
}

async fn lock_session(
    transaction: &mut Transaction<'_, Postgres>,
    session_sha256: &str,
) -> Result<SessionRow, PortalError> {
    let row = sqlx::query(
        "SELECT session.session_sha256, session.account_id, session.current_link_id, \
         session.state, session.resource_version, session.created_unix_seconds, \
         session.last_seen_unix_seconds, session.idle_expires_unix_seconds, \
         session.absolute_expires_unix_seconds, session.mfa_authenticated_unix_seconds, \
         session.csrf_sha256, account.email, account.email_verified, \
         link.provider_key, link.issuer, link.subject_hmac_sha256 \
         FROM portal_sessions session \
         JOIN portal_accounts account ON account.account_id = session.account_id \
         JOIN external_identity_links link ON link.link_id = session.current_link_id \
         WHERE session.session_sha256 = $1 FOR UPDATE OF session",
    )
    .bind(session_sha256)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?
    .ok_or(PortalError::Unauthorized)?;
    let state = row.get::<String, _>("state");
    let created = row.get::<i64, _>("created_unix_seconds");
    let idle = row.get::<i64, _>("idle_expires_unix_seconds");
    let absolute = row.get::<i64, _>("absolute_expires_unix_seconds");
    Ok(SessionRow {
        session_sha256: row.get("session_sha256"),
        account_id: row.get("account_id"),
        current_link_id: row.get("current_link_id"),
        provider_key: row.get("provider_key"),
        issuer: row.get("issuer"),
        subject_hmac_sha256: row.get("subject_hmac_sha256"),
        email: row.get("email"),
        email_verified: row.get("email_verified"),
        state,
        resource_version: row.get("resource_version"),
        created_unix_seconds: created,
        last_seen_unix_seconds: row.get("last_seen_unix_seconds"),
        idle_expires_unix_seconds: idle,
        absolute_expires_unix_seconds: absolute,
        mfa_authenticated_unix_seconds: row.get("mfa_authenticated_unix_seconds"),
        csrf_sha256: row.get("csrf_sha256"),
    })
}

async fn ensure_active_session(
    transaction: &mut Transaction<'_, Postgres>,
    session: &SessionRow,
    now: i64,
) -> Result<bool, PortalError> {
    if session.state != "active" {
        return Ok(false);
    }
    if now >= session.idle_expires_unix_seconds || now >= session.absolute_expires_unix_seconds {
        sqlx::query(
            "UPDATE portal_sessions SET state = 'expired', ended_unix_seconds = $1, \
             resource_version = resource_version + 1 WHERE session_sha256 = $2",
        )
        .bind(now.max(session.created_unix_seconds))
        .bind(&session.session_sha256)
        .execute(&mut **transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        return Ok(false);
    }
    Ok(true)
}

async fn require_active_account(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: &str,
) -> Result<(), PortalError> {
    let state: String =
        sqlx::query_scalar("SELECT state FROM portal_accounts WHERE account_id = $1 FOR UPDATE")
            .bind(account_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(|_| PortalError::Unavailable)?;
    if state != "active" {
        return Err(PortalError::Forbidden);
    }
    Ok(())
}

fn session_cookie_value(headers: &HeaderMap) -> Result<String, PortalError> {
    let mut found = None;
    let mut cookie_headers = headers.get_all(COOKIE).iter();
    let Some(header) = cookie_headers.next() else {
        return Err(PortalError::Unauthorized);
    };
    if cookie_headers.next().is_some() {
        return Err(PortalError::InvalidRequest);
    }
    let encoded = header.to_str().map_err(|_| PortalError::InvalidRequest)?;
    if encoded.len() > 4096 {
        return Err(PortalError::InvalidRequest);
    }
    for part in encoded.split(';') {
        let Some((name, value)) = part.trim().split_once('=') else {
            return Err(PortalError::InvalidRequest);
        };
        if name == SESSION_COOKIE {
            if found.is_some() || !valid_secret(value) {
                return Err(PortalError::InvalidRequest);
            }
            found = Some(value.to_owned());
        }
    }
    found.ok_or(PortalError::Unauthorized)
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

fn exact_header(headers: &HeaderMap, name: &HeaderName) -> Result<String, PortalError> {
    let mut values = headers.get_all(name).iter();
    let value = values.next().ok_or(PortalError::InvalidRequest)?;
    if values.next().is_some() {
        return Err(PortalError::InvalidRequest);
    }
    value
        .to_str()
        .map(str::to_owned)
        .map_err(|_| PortalError::InvalidRequest)
}

fn parse_etag(value: &str) -> Result<i64, PortalError> {
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or(PortalError::InvalidRequest)?;
    let version = value
        .parse::<i64>()
        .map_err(|_| PortalError::InvalidRequest)?;
    if version < 1 || version.to_string() != value {
        return Err(PortalError::InvalidRequest);
    }
    Ok(version)
}

fn valid_provider_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && value.len() <= 32
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn exact_https_origin(value: &str) -> Result<String, PortalBuildError> {
    let parsed = Url::parse(value).map_err(|_| PortalBuildError::InvalidConfiguration)?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(PortalBuildError::InvalidConfiguration);
    }
    Ok(parsed.origin().ascii_serialization())
}

fn opaque_id(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().simple())
}

fn request_id() -> String {
    opaque_id("req_")
}

fn map_oidc_error(error: OidcError) -> PortalError {
    match error {
        OidcError::InvalidConfiguration => PortalError::Internal,
        OidcError::InvalidRequest | OidcError::InvalidToken => PortalError::Unauthorized,
        OidcError::ProviderUnavailable => PortalError::Unavailable,
    }
}

impl PortalError {
    fn response(self, request_id: String) -> Response {
        let (status, code, title) = match self {
            Self::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                "invalid-request",
                "Invalid request",
            ),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authentication required",
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden", "Request forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not-found", "Resource not found"),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "conflict",
                "Request conflicts with current state",
            ),
            Self::PreconditionRequired => (
                StatusCode::PRECONDITION_REQUIRED,
                "precondition-required",
                "Precondition required",
            ),
            Self::PreconditionFailed => (
                StatusCode::PRECONDITION_FAILED,
                "precondition-failed",
                "Precondition failed",
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                "Service unavailable",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "Internal error",
            ),
        };
        json_response(
            status,
            &Problem {
                r#type: format!("https://developer.cardputerzero.dev/problems/{code}"),
                title,
                status: status.as_u16(),
                code,
                request_id: request_id.clone(),
            },
            None,
            request_id,
        )
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
        Err(_) => return PortalError::Internal.response(request_id),
    };
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if status.is_client_error() || status.is_server_error() {
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
    }
    if let Some(version) = version {
        if let Ok(value) = HeaderValue::from_str(&format!("\"{version}\"")) {
            response.headers_mut().insert(ETAG, value);
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
        return PortalError::Internal.response(request_id);
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

fn session_cookie(secret: &str) -> String {
    format!(
        "{SESSION_COOKIE}={secret}; Path=/; Max-Age={SESSION_ABSOLUTE_SECONDS}; Secure; HttpOnly; SameSite=Strict"
    )
}

fn expired_session_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_parser_is_strict_and_secret_is_canonical() {
        let secret = URL_SAFE_NO_PAD.encode([9_u8; 32]);
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("other=1; {SESSION_COOKIE}={secret}")).unwrap(),
        );
        assert_eq!(session_cookie_value(&headers).unwrap(), secret);
        headers.append(
            COOKIE,
            HeaderValue::from_str(&format!("{SESSION_COOKIE}={secret}")).unwrap(),
        );
        assert!(session_cookie_value(&headers).is_err());
    }

    #[test]
    fn etags_and_origins_are_canonical() {
        assert_eq!(parse_etag("\"12\"").unwrap(), 12);
        assert!(parse_etag("12").is_err());
        assert!(parse_etag("\"01\"").is_err());
        assert_eq!(
            exact_https_origin("https://developer.cardputerzero.dev").unwrap(),
            "https://developer.cardputerzero.dev"
        );
        assert!(exact_https_origin("https://developer.cardputerzero.dev/path").is_err());
    }
}
