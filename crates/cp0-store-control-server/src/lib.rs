use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, ETAG, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use cp0_store_control::{AppRecord, is_valid_locale, register_app_request_sha256};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

pub const MAX_REQUEST_BYTES: usize = 32 * 1024;
const IDEMPOTENCY_TTL_SECONDS: i64 = 24 * 60 * 60;
const MAX_TRANSACTION_ATTEMPTS: usize = 4;

#[derive(Clone)]
pub struct StoreControlService {
    pool: PgPool,
}

impl StoreControlService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn create_app(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        request: &CreateAppRequest,
    ) -> Result<AppRecord, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());

        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .create_app_once(&token_sha256, &key_sha256, request_id, request)
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    tokio::time::sleep(Duration::from_millis(5 * (attempt as u64 + 1))).await;
                }
                Err(TxError::Sql(_)) => return Err(ApiError::unavailable()),
                Err(TxError::Api(error)) => return Err(error),
                Ok(app) => return Ok(app),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn create_app_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        request: &CreateAppRequest,
    ) -> Result<AppRecord, TxError> {
        let mut transaction = self.pool.begin().await.map_err(TxError::Sql)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *transaction)
            .await
            .map_err(TxError::Sql)?;

        let identity = authenticate(&mut transaction, token_sha256).await?;
        if !matches!(identity.role.as_str(), "owner" | "developer") {
            return Err(ApiError::forbidden().into());
        }
        if !identity.two_factor_enabled {
            return Err(ApiError::two_factor_required().into());
        }
        if !identity.has_any_scope(&["store.apps.write", "store.control"]) {
            return Err(ApiError::forbidden().into());
        }

        let request_sha256 = register_app_request_sha256(
            &identity.team_id,
            &request.app_id,
            &request.default_locale,
        );
        let now = database_now(&mut transaction).await?;

        if let Some(replay) = reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            transaction.commit().await.map_err(TxError::Sql)?;
            return Ok(replay);
        }

        let inserted = sqlx::query(
            "INSERT INTO apps (app_id, owner_team_id, default_locale, resource_version, created_unix_seconds) \
             VALUES ($1, $2, $3, 1, $4) \
             ON CONFLICT (app_id) DO NOTHING",
        )
        .bind(&request.app_id)
        .bind(&identity.team_id)
        .bind(&request.default_locale)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .rows_affected();
        if inserted != 1 {
            return Err(ApiError::conflict().into());
        }

        let app = AppRecord {
            app_id: request.app_id.clone(),
            owner_team_id: identity.team_id,
            default_locale: request.default_locale.clone(),
            resource_version: 1,
        };
        let response_body = serde_json::to_value(&app).map_err(|_| ApiError::internal())?;

        sqlx::query(
            "UPDATE idempotency_records \
             SET response_status = 201, response_body = $1 \
             WHERE actor_id = $2 AND key_sha256 = $3",
        )
        .bind(&response_body)
        .bind(&identity.member_id)
        .bind(key_sha256)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        sqlx::query(
            "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
             object_id, before_state, after_state, resource_version, request_id, request_sha256, \
             idempotency_key_sha256) \
             VALUES ($1, $2, 'app.registered', 'app', $3, NULL, NULL, 1, $4, $5, $6)",
        )
        .bind(now)
        .bind(&identity.member_id)
        .bind(&app.app_id)
        .bind(request_id)
        .bind(&request_sha256)
        .bind(key_sha256)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        sqlx::query(
            "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
             aggregate_version, request_sha256, payload, created_unix_seconds) \
             VALUES ($1, 'app.registered', 'app', $2, 1, $3, $4, $5)",
        )
        .bind(prefixed_uuid("evt_"))
        .bind(&app.app_id)
        .bind(&request_sha256)
        .bind(json!({"app_id": app.app_id, "owner_team_id": app.owner_team_id}))
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;

        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(app)
    }

    async fn get_app(&self, token: &str, app_id: &str) -> Result<AppRecord, ApiError> {
        let token_sha256 = sha256_hex(token.as_bytes());
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate(&mut transaction, &token_sha256)
            .await
            .map_err(ApiError::from_transaction)?;
        if !identity.has_any_scope(&[
            "store.apps.read",
            "store.apps.write",
            "store.submit",
            "store.control",
        ]) {
            return Err(ApiError::forbidden());
        }

        let row = sqlx::query(
            "SELECT app_id, owner_team_id, default_locale, resource_version \
             FROM apps WHERE app_id = $1 AND owner_team_id = $2",
        )
        .bind(app_id)
        .bind(&identity.team_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?
        .ok_or_else(ApiError::not_found)?;
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;

        Ok(AppRecord {
            app_id: row.get("app_id"),
            owner_team_id: row.get("owner_team_id"),
            default_locale: row.get("default_locale"),
            resource_version: row_version(&row)?,
        })
    }
}

pub async fn connect(database_url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await
}

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!().run(pool).await
}

pub fn router(service: StoreControlService) -> Router {
    Router::new()
        .route("/v1/apps", post(post_app))
        .route("/v1/apps/{app_id}", get(get_app))
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(fallback)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(service)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateAppRequest {
    app_id: String,
    default_locale: String,
}

#[derive(Debug, Serialize)]
struct Problem {
    r#type: String,
    title: &'static str,
    status: u16,
    code: &'static str,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
enum ApiErrorKind {
    InvalidRequest,
    Unauthorized,
    Forbidden,
    TwoFactorRequired,
    NotFound,
    Conflict,
    IdempotencyConflict,
    PayloadTooLarge,
    MethodNotAllowed,
    Internal,
    Unavailable,
}

#[derive(Debug)]
struct ApiError {
    kind: ApiErrorKind,
}

impl ApiError {
    const fn invalid_request() -> Self {
        Self {
            kind: ApiErrorKind::InvalidRequest,
        }
    }

    const fn unauthorized() -> Self {
        Self {
            kind: ApiErrorKind::Unauthorized,
        }
    }

    const fn forbidden() -> Self {
        Self {
            kind: ApiErrorKind::Forbidden,
        }
    }

    const fn two_factor_required() -> Self {
        Self {
            kind: ApiErrorKind::TwoFactorRequired,
        }
    }

    const fn not_found() -> Self {
        Self {
            kind: ApiErrorKind::NotFound,
        }
    }

    const fn conflict() -> Self {
        Self {
            kind: ApiErrorKind::Conflict,
        }
    }

    const fn idempotency_conflict() -> Self {
        Self {
            kind: ApiErrorKind::IdempotencyConflict,
        }
    }

    const fn payload_too_large() -> Self {
        Self {
            kind: ApiErrorKind::PayloadTooLarge,
        }
    }

    const fn method_not_allowed() -> Self {
        Self {
            kind: ApiErrorKind::MethodNotAllowed,
        }
    }

    const fn internal() -> Self {
        Self {
            kind: ApiErrorKind::Internal,
        }
    }

    const fn unavailable() -> Self {
        Self {
            kind: ApiErrorKind::Unavailable,
        }
    }

    fn from_transaction(error: TxError) -> Self {
        match error {
            TxError::Api(error) => error,
            TxError::Sql(_) => Self::unavailable(),
        }
    }

    fn response(self, request_id: String) -> Response {
        let (status, code, title, detail) = match self.kind {
            ApiErrorKind::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                "invalid-request",
                "Invalid request",
                Some("The request does not match the Store API contract."),
            ),
            ApiErrorKind::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authorization required",
                Some("The access token is missing, expired, revoked, or invalid."),
            ),
            ApiErrorKind::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "Operation not permitted",
                Some(
                    "Current team membership, role, or token scope does not allow this operation.",
                ),
            ),
            ApiErrorKind::TwoFactorRequired => (
                StatusCode::FORBIDDEN,
                "two-factor-required",
                "Two-factor authentication required",
                Some("Enable two-factor authentication before changing Store resources."),
            ),
            ApiErrorKind::NotFound => (
                StatusCode::NOT_FOUND,
                "not-found",
                "Resource not found",
                None,
            ),
            ApiErrorKind::Conflict => (
                StatusCode::CONFLICT,
                "conflict",
                "Resource conflict",
                Some("The App ID is already permanently registered."),
            ),
            ApiErrorKind::IdempotencyConflict => (
                StatusCode::CONFLICT,
                "idempotency-conflict",
                "Idempotency conflict",
                Some("This idempotency key was already used for another request."),
            ),
            ApiErrorKind::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload-too-large",
                "Request body too large",
                Some("The JSON request body exceeds 32768 bytes."),
            ),
            ApiErrorKind::MethodNotAllowed => (
                StatusCode::METHOD_NOT_ALLOWED,
                "method-not-allowed",
                "Method not allowed",
                None,
            ),
            ApiErrorKind::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "Internal server error",
                None,
            ),
            ApiErrorKind::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service-unavailable",
                "Service temporarily unavailable",
                Some("Retry this request with the same idempotency key."),
            ),
        };
        let problem = Problem {
            r#type: format!("https://cardputerzero.dev/problems/{code}"),
            title,
            status: status.as_u16(),
            code,
            request_id,
            detail,
        };
        let mut response = (status, Json(problem)).into_response();
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer realm=\"cardputerzero-store\""),
            );
        }
        response
    }
}

enum TxError {
    Api(ApiError),
    Sql(sqlx::Error),
}

impl From<ApiError> for TxError {
    fn from(value: ApiError) -> Self {
        Self::Api(value)
    }
}

#[derive(Debug)]
struct Identity {
    member_id: String,
    team_id: String,
    role: String,
    two_factor_enabled: bool,
    scopes: Vec<String>,
}

impl Identity {
    fn has_any_scope(&self, expected: &[&str]) -> bool {
        self.scopes
            .iter()
            .any(|scope| expected.contains(&scope.as_str()))
    }
}

async fn authenticate(
    transaction: &mut Transaction<'_, Postgres>,
    token_sha256: &str,
) -> Result<Identity, TxError> {
    let row = sqlx::query(
        "SELECT member.member_id, member.team_id, member.role, member.two_factor_enabled, token.scopes \
         FROM access_tokens token \
         JOIN team_members member ON member.member_id = token.member_id \
         WHERE token.token_sha256 = $1 AND NOT token.revoked \
           AND token.expires_unix_seconds > EXTRACT(EPOCH FROM clock_timestamp())::BIGINT",
    )
    .bind(token_sha256)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(TxError::Sql)?
    .ok_or_else(ApiError::unauthorized)?;

    Ok(Identity {
        member_id: row.get("member_id"),
        team_id: row.get("team_id"),
        role: row.get("role"),
        two_factor_enabled: row.get("two_factor_enabled"),
        scopes: row.get("scopes"),
    })
}

async fn database_now(transaction: &mut Transaction<'_, Postgres>) -> Result<i64, TxError> {
    sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(&mut **transaction)
        .await
        .map_err(TxError::Sql)
}

async fn reserve_idempotency(
    transaction: &mut Transaction<'_, Postgres>,
    actor_id: &str,
    key_sha256: &str,
    request_sha256: &str,
    now: i64,
) -> Result<Option<AppRecord>, TxError> {
    let inserted = sqlx::query(
        "INSERT INTO idempotency_records (actor_id, key_sha256, request_sha256, \
         created_unix_seconds, expires_unix_seconds) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (actor_id, key_sha256) DO NOTHING",
    )
    .bind(actor_id)
    .bind(key_sha256)
    .bind(request_sha256)
    .bind(now)
    .bind(now + IDEMPOTENCY_TTL_SECONDS)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?
    .rows_affected();
    if inserted == 1 {
        return Ok(None);
    }

    let row = sqlx::query(
        "SELECT request_sha256, response_status, response_body, expires_unix_seconds \
         FROM idempotency_records WHERE actor_id = $1 AND key_sha256 = $2 FOR UPDATE",
    )
    .bind(actor_id)
    .bind(key_sha256)
    .fetch_one(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;

    let expires: i64 = row.get("expires_unix_seconds");
    if expires <= now {
        sqlx::query("DELETE FROM idempotency_records WHERE actor_id = $1 AND key_sha256 = $2")
            .bind(actor_id)
            .bind(key_sha256)
            .execute(&mut **transaction)
            .await
            .map_err(TxError::Sql)?;
        sqlx::query(
            "INSERT INTO idempotency_records (actor_id, key_sha256, request_sha256, \
             created_unix_seconds, expires_unix_seconds) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(actor_id)
        .bind(key_sha256)
        .bind(request_sha256)
        .bind(now)
        .bind(now + IDEMPOTENCY_TTL_SECONDS)
        .execute(&mut **transaction)
        .await
        .map_err(TxError::Sql)?;
        return Ok(None);
    }

    let existing_request: String = row.get("request_sha256");
    if existing_request != request_sha256 {
        return Err(ApiError::idempotency_conflict().into());
    }
    let status: Option<i16> = row.get("response_status");
    let body: Option<Value> = row.get("response_body");
    if status != Some(StatusCode::CREATED.as_u16() as i16) {
        return Err(ApiError::unavailable().into());
    }
    serde_json::from_value(body.ok_or_else(ApiError::internal)?)
        .map(Some)
        .map_err(|_| ApiError::internal().into())
}

async fn post_app(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
    payload: Result<Json<CreateAppRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            let error = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                ApiError::payload_too_large()
            } else {
                ApiError::invalid_request()
            };
            return error.response(request_id);
        }
    };
    if !cp0_manifest::is_valid_app_id(&request.app_id) || !is_valid_locale(&request.default_locale)
    {
        return ApiError::invalid_request().response(request_id);
    }

    match service
        .create_app(&token, &idempotency_key, &request_id, &request)
        .await
    {
        Ok(app) => resource_response(StatusCode::CREATED, app, request_id),
        Err(error) => error.response(request_id),
    }
}

async fn get_app(
    State(service): State<StoreControlService>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let request_id = request_id();
    if !cp0_manifest::is_valid_app_id(&app_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service.get_app(&token, &app_id).await {
        Ok(app) => resource_response(StatusCode::OK, app, request_id),
        Err(error) => error.response(request_id),
    }
}

async fn fallback() -> Response {
    ApiError::not_found().response(request_id())
}

async fn method_not_allowed() -> Response {
    ApiError::method_not_allowed().response(request_id())
}

fn resource_response(status: StatusCode, app: AppRecord, request_id: String) -> Response {
    let etag = format!("\"{}\"", app.resource_version);
    let mut response = (status, Json(app)).into_response();
    if let Ok(value) = HeaderValue::from_str(&etag) {
        response.headers_mut().insert(ETAG, value);
    }
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

fn bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(ApiError::unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .filter(|token| {
            (32..=512).contains(&token.len()) && token.bytes().all(|byte| byte.is_ascii_graphic())
        })
        .ok_or_else(ApiError::unauthorized)?;
    Ok(token.to_owned())
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|key| {
            (16..=128).contains(&key.len())
                && key.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-')
                })
        })
        .ok_or_else(ApiError::invalid_request)?;
    Ok(key.to_owned())
}

fn row_version(row: &sqlx::postgres::PgRow) -> Result<u64, ApiError> {
    let value: i64 = row.get("resource_version");
    u64::try_from(value).map_err(|_| ApiError::internal())
}

fn request_id() -> String {
    prefixed_uuid("req_")
}

fn prefixed_uuid(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().simple())
}

fn sha256_hex(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_retryable_transaction_error(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| matches!(code.as_ref(), "40001" | "40P01"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_validators_reject_ambiguous_credentials() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer token-with-safe-bytes-1234567890"),
        );
        headers.insert(
            "idempotency-key",
            HeaderValue::from_static("request-key-0001"),
        );
        assert!(bearer_token(&headers).is_ok());
        assert!(idempotency_key(&headers).is_ok());

        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("bearer token-with-safe-bytes-1234567890"),
        );
        assert!(bearer_token(&headers).is_err());
        headers.insert("idempotency-key", HeaderValue::from_static("short"));
        assert!(idempotency_key(&headers).is_err());
    }

    #[test]
    fn retry_classifier_is_closed_by_default() {
        let error = sqlx::Error::RowNotFound;
        assert!(!is_retryable_transaction_error(&error));
    }
}
