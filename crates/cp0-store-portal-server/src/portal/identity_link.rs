use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};

use super::{
    AuthorizationRedirect, OidcTransactionRow, PortalError, PortalService, SessionRow,
    database_now, ensure_active_session, expire_oidc_transactions, expired_session_cookie,
    json_response, load_oidc_transaction, lock_session, mutation_headers, opaque_id, request_id,
    serializable, sha256_hex,
};
use crate::{AuthIntent, OidcProvider, VerifiedIdentity};

const IDEMPOTENCY_TTL_SECONDS: i64 = 24 * 60 * 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct IdentityLink {
    link_id: String,
    provider: String,
    linked_unix_seconds: i64,
    current: bool,
    resource_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct IdentityLinkList {
    items: Vec<IdentityLink>,
    resource_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BeginIdentityLinkRequest {
    provider: String,
}

struct RemoveOutcome {
    links: IdentityLinkList,
    expire_session: bool,
}

enum IdempotencyReservation {
    Fresh,
    Replay { status: i16, body: Value },
}

struct LinkEvent<'a> {
    now: i64,
    actor_id: &'a str,
    action: &'a str,
    link_id: &'a str,
    provider: &'a str,
    before_state: Option<&'a str>,
    after_state: &'a str,
    link_resource_version: i64,
    account_resource_version: i64,
    request_id: &'a str,
    request_sha256: &'a str,
    key_sha256: &'a str,
}

pub(super) async fn list_identity_links(
    State(service): State<PortalService>,
    headers: HeaderMap,
) -> Response {
    let response_request_id = request_id();
    let session_secret = match super::session_cookie_value(&headers) {
        Ok(secret) => secret,
        Err(error) => return error.response(response_request_id),
    };
    match service.list_identity_links(&session_secret).await {
        Ok(links) => {
            let version = links.resource_version;
            json_response(StatusCode::OK, &links, Some(version), response_request_id)
        }
        Err(error) => error.response(response_request_id),
    }
}

pub(super) async fn begin_identity_link(
    State(service): State<PortalService>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let response_request_id = request_id();
    let security = match mutation_headers(&service, &headers, true) {
        Ok(security) => security,
        Err(error) => return error.response(response_request_id),
    };
    let request: BeginIdentityLinkRequest = match decode_json(&headers, &body) {
        Ok(request) => request,
        Err(error) => return error.response(response_request_id),
    };
    let expected_version = security.expected_version.expect("required above");
    match service
        .begin_identity_link(
            &security.session_secret,
            &security.csrf_token,
            &security.idempotency_key,
            expected_version,
            &request.provider,
        )
        .await
    {
        Ok(authorization_uri) => json_response(
            StatusCode::OK,
            &AuthorizationRedirect { authorization_uri },
            None,
            response_request_id,
        ),
        Err(error) => error.response(response_request_id),
    }
}

pub(super) async fn remove_identity_link(
    State(service): State<PortalService>,
    Path(link_action): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let response_request_id = request_id();
    let Some(link_id) = link_action.strip_suffix(":remove") else {
        return PortalError::NotFound.response(response_request_id);
    };
    if !valid_id(link_id, "link_") {
        return PortalError::NotFound.response(response_request_id);
    }
    let security = match mutation_headers(&service, &headers, true) {
        Ok(security) => security,
        Err(error) => return error.response(response_request_id),
    };
    if let Err(error) = super::require_empty_body(&headers, &body) {
        return error.response(response_request_id);
    }
    let expected_version = security.expected_version.expect("required above");
    match service
        .remove_identity_link(
            &security.session_secret,
            &security.csrf_token,
            &security.idempotency_key,
            expected_version,
            link_id,
            &response_request_id,
        )
        .await
    {
        Ok(outcome) => {
            let version = outcome.links.resource_version;
            let mut response = json_response(
                StatusCode::OK,
                &outcome.links,
                Some(version),
                response_request_id,
            );
            if outcome.expire_session {
                if let Ok(cookie) = HeaderValue::from_str(&expired_session_cookie()) {
                    response.headers_mut().insert(SET_COOKIE, cookie);
                }
            }
            response
        }
        Err(error) => error.response(response_request_id),
    }
}

impl PortalService {
    async fn list_identity_links(
        &self,
        session_secret: &str,
    ) -> Result<IdentityLinkList, PortalError> {
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let session = active_session(&mut transaction, session_secret, None, now).await?;
        let links = load_identity_links(&mut transaction, &session).await?;
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(links)
    }

    async fn begin_identity_link(
        &self,
        session_secret: &str,
        csrf_token: &str,
        idempotency_key: &str,
        expected_version: i64,
        provider_key: &str,
    ) -> Result<String, PortalError> {
        if !super::valid_provider_key(provider_key) {
            return Err(PortalError::InvalidRequest);
        }
        let provider = self.provider(provider_key)?;
        let session_sha256 = sha256_hex(session_secret.as_bytes());
        let state =
            self.inner
                .secrets
                .state_for_action(&session_sha256, "identity-link", idempotency_key);
        let state_sha256 = sha256_hex(state.as_bytes());
        let nonce = self.inner.secrets.nonce_for_state(&state);
        let nonce_sha256 = sha256_hex(nonce.as_bytes());
        let request_sha256 = request_digest(&json!({
            "operation": "identity-link-begin",
            "provider": provider_key,
            "expected_version": expected_version
        }))?;
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        expire_oidc_transactions(&mut transaction, now).await?;
        let session =
            active_session(&mut transaction, session_secret, Some(csrf_token), now).await?;
        let account_version = lock_active_account(&mut transaction, &session.account_id).await?;
        if account_version != expected_version {
            return Err(PortalError::PreconditionFailed);
        }
        let active_links: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM external_identity_links \
             WHERE account_id = $1 AND state = 'active'",
        )
        .bind(&session.account_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        if active_links >= 8 {
            return Err(PortalError::Conflict);
        }
        let existing = load_oidc_transaction(&mut transaction, &state_sha256, true).await?;
        let pkce_verifier = if let Some(existing) = existing {
            if existing.intent != AuthIntent::Link
                || existing.account_id.as_deref() != Some(session.account_id.as_str())
                || existing.session_sha256.as_deref() != Some(session.session_sha256.as_str())
                || existing.provider_key != provider.key()
                || existing.provider_config_sha256 != provider.config_sha256()
                || existing.nonce_sha256 != nonce_sha256
                || existing.request_sha256.as_deref() != Some(request_sha256.as_str())
                || existing.idempotency_key_sha256.as_deref() != Some(key_sha256.as_str())
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
                 expires_unix_seconds, request_sha256, idempotency_key_sha256) VALUES \
                 ($1, $2, $3, $4, $5, $6, 'link', $7, $8, 'pending', $9, $9 + $10, $11, $12)",
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
            .bind(super::OIDC_TRANSACTION_SECONDS)
            .bind(&request_sha256)
            .bind(&key_sha256)
            .execute(&mut *transaction)
            .await
            .map_err(|_| PortalError::Unavailable)?;
            zeroize::Zeroizing::new(verifier)
        };
        let authorization_uri = provider
            .authorization_uri(AuthIntent::Link, &state, &nonce, &pkce_verifier)
            .map_err(super::map_oidc_error)?;
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(authorization_uri)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn complete_identity_link_callback(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        locked: &OidcTransactionRow,
        provider: &Arc<dyn OidcProvider>,
        identity: &VerifiedIdentity,
        subject_hmac: &str,
        now: i64,
        callback_request_id: &str,
    ) -> Result<(String, String, Option<i64>), PortalError> {
        let old_session_sha256 = locked
            .session_sha256
            .as_deref()
            .ok_or(PortalError::Unauthorized)?;
        let old = lock_session(transaction, old_session_sha256).await?;
        if !ensure_active_session(transaction, &old, now).await? {
            return Err(PortalError::Unauthorized);
        }
        if locked.account_id.as_deref() != Some(old.account_id.as_str()) {
            return Err(PortalError::Unauthorized);
        }
        let identity_owner = sqlx::query(
            "SELECT account_id, state FROM external_identity_links \
             WHERE issuer = $1 AND subject_hmac_sha256 = $2 FOR UPDATE",
        )
        .bind(&identity.issuer)
        .bind(subject_hmac)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        if identity_owner.is_some() {
            return Err(PortalError::Conflict);
        }
        let account_version = lock_active_account(transaction, &old.account_id).await?;
        let active_links: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM external_identity_links \
             WHERE account_id = $1 AND state = 'active'",
        )
        .bind(&old.account_id)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        if active_links >= 8 {
            return Err(PortalError::Conflict);
        }
        let new_account_version = account_version + 1;
        let affected = sqlx::query(
            "UPDATE portal_accounts SET resource_version = resource_version + 1 \
             WHERE account_id = $1 AND state = 'active' AND resource_version = $2",
        )
        .bind(&old.account_id)
        .bind(account_version)
        .execute(&mut **transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?
        .rows_affected();
        if affected != 1 {
            return Err(PortalError::PreconditionFailed);
        }
        let link_id = opaque_id("link_");
        sqlx::query(
            "INSERT INTO external_identity_links (link_id, account_id, provider_key, issuer, \
             subject_hmac_sha256, state, linked_unix_seconds) \
             VALUES ($1, $2, $3, $4, $5, 'active', $6)",
        )
        .bind(&link_id)
        .bind(&old.account_id)
        .bind(provider.key())
        .bind(&identity.issuer)
        .bind(subject_hmac)
        .bind(now)
        .execute(&mut **transaction)
        .await
        .map_err(|_| PortalError::Conflict)?;
        let request_sha256 = locked
            .request_sha256
            .as_deref()
            .ok_or(PortalError::Internal)?;
        let key_sha256 = locked
            .idempotency_key_sha256
            .as_deref()
            .ok_or(PortalError::Internal)?;
        append_link_event(
            transaction,
            LinkEvent {
                now,
                actor_id: &old.account_id,
                action: "account.identity-linked",
                link_id: &link_id,
                provider: provider.key(),
                before_state: None,
                after_state: "active",
                link_resource_version: 1,
                account_resource_version: new_account_version,
                request_id: callback_request_id,
                request_sha256,
                key_sha256,
            },
        )
        .await?;
        Ok((
            old.account_id,
            link_id,
            identity.mfa_authenticated_unix_seconds,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn remove_identity_link(
        &self,
        session_secret: &str,
        csrf_token: &str,
        idempotency_key: &str,
        expected_version: i64,
        link_id: &str,
        mutation_request_id: &str,
    ) -> Result<RemoveOutcome, PortalError> {
        let request_sha256 = request_digest(&json!({
            "operation": "identity-link-remove",
            "link_id": link_id,
            "expected_version": expected_version
        }))?;
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let session =
            active_session(&mut transaction, session_secret, Some(csrf_token), now).await?;
        if !session
            .mfa_authenticated_unix_seconds
            .is_some_and(|value| value >= now - 300 && value <= now)
        {
            return Err(PortalError::Forbidden);
        }
        match reserve_idempotency(
            &mut transaction,
            &session.account_id,
            &key_sha256,
            &request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::OK.as_u16() as i16 =>
            {
                let links: IdentityLinkList =
                    serde_json::from_value(body).map_err(|_| PortalError::Internal)?;
                transaction
                    .commit()
                    .await
                    .map_err(|_| PortalError::Unavailable)?;
                return Ok(RemoveOutcome {
                    links,
                    expire_session: false,
                });
            }
            IdempotencyReservation::Replay { .. } => return Err(PortalError::Internal),
        }
        let account_version = lock_active_account(&mut transaction, &session.account_id).await?;
        if account_version != expected_version {
            return Err(PortalError::PreconditionFailed);
        }
        let row = sqlx::query(
            "SELECT provider_key, state, resource_version FROM external_identity_links \
             WHERE link_id = $1 AND account_id = $2 FOR UPDATE",
        )
        .bind(link_id)
        .bind(&session.account_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?
        .ok_or(PortalError::NotFound)?;
        if row.get::<String, _>("state") != "active" {
            return Err(PortalError::Conflict);
        }
        let active_links: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM external_identity_links \
             WHERE account_id = $1 AND state = 'active'",
        )
        .bind(&session.account_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        if active_links <= 1 {
            return Err(PortalError::Conflict);
        }
        let new_account_version = account_version + 1;
        let account_affected = sqlx::query(
            "UPDATE portal_accounts SET resource_version = resource_version + 1 \
             WHERE account_id = $1 AND state = 'active' AND resource_version = $2",
        )
        .bind(&session.account_id)
        .bind(account_version)
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?
        .rows_affected();
        if account_affected != 1 {
            return Err(PortalError::PreconditionFailed);
        }
        let link_version = row.get::<i64, _>("resource_version");
        let link_affected = sqlx::query(
            "UPDATE external_identity_links SET state = 'revoked', revoked_unix_seconds = $1, \
             resource_version = resource_version + 1 \
             WHERE link_id = $2 AND account_id = $3 AND state = 'active' \
             AND resource_version = $4",
        )
        .bind(now)
        .bind(link_id)
        .bind(&session.account_id)
        .bind(link_version)
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?
        .rows_affected();
        if link_affected != 1 {
            return Err(PortalError::PreconditionFailed);
        }
        let links = load_identity_links(&mut transaction, &session).await?;
        if links.resource_version != new_account_version {
            return Err(PortalError::Internal);
        }
        let response_body = serde_json::to_value(&links).map_err(|_| PortalError::Internal)?;
        complete_idempotency(
            &mut transaction,
            &session.account_id,
            &key_sha256,
            StatusCode::OK,
            &response_body,
        )
        .await?;
        let provider: String = row.get("provider_key");
        append_link_event(
            &mut transaction,
            LinkEvent {
                now,
                actor_id: &session.account_id,
                action: "account.identity-link-removed",
                link_id,
                provider: &provider,
                before_state: Some("active"),
                after_state: "revoked",
                link_resource_version: link_version + 1,
                account_resource_version: new_account_version,
                request_id: mutation_request_id,
                request_sha256: &request_sha256,
                key_sha256: &key_sha256,
            },
        )
        .await?;
        let expire_session = session.current_link_id == link_id;
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(RemoveOutcome {
            links,
            expire_session,
        })
    }
}

async fn active_session(
    transaction: &mut Transaction<'_, Postgres>,
    session_secret: &str,
    csrf_token: Option<&str>,
    now: i64,
) -> Result<SessionRow, PortalError> {
    let session_sha256 = sha256_hex(session_secret.as_bytes());
    let session = lock_session(transaction, &session_sha256).await?;
    if !ensure_active_session(transaction, &session, now).await? {
        return Err(PortalError::Unauthorized);
    }
    if let Some(csrf_token) = csrf_token {
        if session.csrf_sha256 != sha256_hex(csrf_token.as_bytes()) {
            return Err(PortalError::Forbidden);
        }
    }
    Ok(session)
}

async fn lock_active_account(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: &str,
) -> Result<i64, PortalError> {
    let row = sqlx::query(
        "SELECT state, resource_version FROM portal_accounts \
         WHERE account_id = $1 FOR UPDATE",
    )
    .bind(account_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?
    .ok_or(PortalError::Unauthorized)?;
    if row.get::<String, _>("state") != "active" {
        return Err(PortalError::Forbidden);
    }
    Ok(row.get("resource_version"))
}

async fn load_identity_links(
    transaction: &mut Transaction<'_, Postgres>,
    session: &SessionRow,
) -> Result<IdentityLinkList, PortalError> {
    let resource_version = lock_active_account(transaction, &session.account_id).await?;
    let rows = sqlx::query(
        "SELECT link_id, provider_key, linked_unix_seconds, resource_version \
         FROM external_identity_links WHERE account_id = $1 AND state = 'active' \
         ORDER BY linked_unix_seconds, link_id LIMIT 9",
    )
    .bind(&session.account_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    if rows.is_empty() || rows.len() > 8 {
        return Err(PortalError::Internal);
    }
    let items = rows
        .into_iter()
        .map(|row| {
            let link_id: String = row.get("link_id");
            IdentityLink {
                current: link_id == session.current_link_id,
                link_id,
                provider: row.get("provider_key"),
                linked_unix_seconds: row.get("linked_unix_seconds"),
                resource_version: row.get("resource_version"),
            }
        })
        .collect();
    Ok(IdentityLinkList {
        items,
        resource_version,
    })
}

async fn reserve_idempotency(
    transaction: &mut Transaction<'_, Postgres>,
    actor_id: &str,
    key_sha256: &str,
    request_sha256: &str,
    now: i64,
) -> Result<IdempotencyReservation, PortalError> {
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
    .map_err(|_| PortalError::Unavailable)?
    .rows_affected();
    if inserted == 1 {
        return Ok(IdempotencyReservation::Fresh);
    }
    let row = sqlx::query(
        "SELECT request_sha256, response_status, response_body, expires_unix_seconds \
         FROM idempotency_records WHERE actor_id = $1 AND key_sha256 = $2 FOR UPDATE",
    )
    .bind(actor_id)
    .bind(key_sha256)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    if row.get::<i64, _>("expires_unix_seconds") <= now {
        sqlx::query("DELETE FROM idempotency_records WHERE actor_id = $1 AND key_sha256 = $2")
            .bind(actor_id)
            .bind(key_sha256)
            .execute(&mut **transaction)
            .await
            .map_err(|_| PortalError::Unavailable)?;
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
        .map_err(|_| PortalError::Unavailable)?;
        return Ok(IdempotencyReservation::Fresh);
    }
    if row.get::<String, _>("request_sha256") != request_sha256 {
        return Err(PortalError::Conflict);
    }
    match (
        row.get::<Option<i16>, _>("response_status"),
        row.get::<Option<Value>, _>("response_body"),
    ) {
        (Some(status), Some(body)) => Ok(IdempotencyReservation::Replay { status, body }),
        _ => Err(PortalError::Unavailable),
    }
}

async fn complete_idempotency(
    transaction: &mut Transaction<'_, Postgres>,
    actor_id: &str,
    key_sha256: &str,
    status: StatusCode,
    body: &Value,
) -> Result<(), PortalError> {
    let affected = sqlx::query(
        "UPDATE idempotency_records SET response_status = $1, response_body = $2 \
         WHERE actor_id = $3 AND key_sha256 = $4 AND response_status IS NULL",
    )
    .bind(status.as_u16() as i16)
    .bind(body)
    .bind(actor_id)
    .bind(key_sha256)
    .execute(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?
    .rows_affected();
    if affected != 1 {
        return Err(PortalError::Internal);
    }
    Ok(())
}

async fn append_link_event(
    transaction: &mut Transaction<'_, Postgres>,
    event: LinkEvent<'_>,
) -> Result<(), PortalError> {
    sqlx::query(
        "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
         object_id, before_state, after_state, resource_version, request_id, request_sha256, \
         idempotency_key_sha256) VALUES ($1, $2, $3, 'external-identity-link', $4, $5, $6, \
         $7, $8, $9, $10)",
    )
    .bind(event.now)
    .bind(event.actor_id)
    .bind(event.action)
    .bind(event.link_id)
    .bind(event.before_state)
    .bind(event.after_state)
    .bind(event.link_resource_version)
    .bind(event.request_id)
    .bind(event.request_sha256)
    .bind(event.key_sha256)
    .execute(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds) \
         VALUES ($1, $2, 'portal-account', $3, $4, $5, $6, $7)",
    )
    .bind(opaque_id("evt_"))
    .bind(event.action)
    .bind(event.actor_id)
    .bind(event.account_resource_version)
    .bind(event.request_sha256)
    .bind(json!({
        "account_id": event.actor_id,
        "link_id": event.link_id,
        "provider": event.provider,
        "state": event.after_state,
        "account_resource_version": event.account_resource_version
    }))
    .bind(event.now)
    .execute(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    Ok(())
}

fn decode_json<T: for<'de> Deserialize<'de>>(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<T, PortalError> {
    if body.is_empty() || body.len() > super::MAX_REQUEST_BYTES {
        return Err(PortalError::InvalidRequest);
    }
    if super::exact_header(headers, &CONTENT_TYPE)? != "application/json"
        || headers.contains_key("transfer-encoding")
    {
        return Err(PortalError::InvalidRequest);
    }
    if let Some(length) = headers.get(CONTENT_LENGTH) {
        let expected = body.len().to_string();
        if length.to_str().ok() != Some(expected.as_str()) {
            return Err(PortalError::InvalidRequest);
        }
    }
    serde_json::from_slice(body).map_err(|_| PortalError::InvalidRequest)
}

fn request_digest(value: &Value) -> Result<String, PortalError> {
    serde_json::to_vec(value)
        .map(|encoded| sha256_hex(&encoded))
        .map_err(|_| PortalError::Internal)
}

fn valid_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}
