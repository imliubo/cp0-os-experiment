use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};

use super::{
    PortalError, PortalService, SessionRow, TeamSummary, database_now, ensure_active_session,
    json_response, lock_session, mutation_headers, opaque_id, request_id, serializable,
    session_cookie, sha256_hex,
};
use crate::oidc::normalize_verified_email;

const INVITATION_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;
const IDEMPOTENCY_TTL_SECONDS: i64 = 24 * 60 * 60;
const MAX_TEAM_INVITATIONS_PER_HOUR: i64 = 20;
const MAX_EMAIL_INVITATIONS_PER_DAY: i64 = 3;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CreateInvitationRequest {
    email: String,
    role: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InvitationTokenRequest {
    invitation_token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Invitation {
    invitation_id: String,
    team_id: String,
    email: String,
    role: String,
    state: String,
    created_unix_seconds: i64,
    expires_unix_seconds: i64,
    resource_version: i64,
    team_resource_version: i64,
}

#[derive(Debug, Serialize)]
struct InvitationList {
    items: Vec<Invitation>,
}

#[derive(Debug, Serialize)]
struct InvitationPreview {
    team_name: String,
    masked_email: String,
    role: String,
    expires_unix_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AcceptedInvitation {
    invitation: Invitation,
    team: TeamSummary,
}

struct Versioned<T> {
    body: T,
    team_resource_version: i64,
}

struct AcceptanceOutcome {
    accepted: Versioned<AcceptedInvitation>,
    rotated_session_secret: Option<String>,
}

enum IdempotencyReservation {
    Fresh,
    Replay { status: i16, body: Value },
}

struct InvitationActor {
    member_id: String,
}

pub(super) async fn list_invitations(
    State(service): State<PortalService>,
    Path(team_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let response_request_id = request_id();
    if !valid_id(&team_id, "team_") {
        return PortalError::NotFound.response(response_request_id);
    }
    let session_secret = match super::session_cookie_value(&headers) {
        Ok(secret) => secret,
        Err(error) => return error.response(response_request_id),
    };
    match service.list_invitations(&session_secret, &team_id).await {
        Ok(result) => json_response(
            StatusCode::OK,
            &result.body,
            Some(result.team_resource_version),
            response_request_id,
        ),
        Err(error) => error.response(response_request_id),
    }
}

pub(super) async fn create_invitation(
    State(service): State<PortalService>,
    Path(team_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let response_request_id = request_id();
    if !valid_id(&team_id, "team_") {
        return PortalError::NotFound.response(response_request_id);
    }
    let security = match mutation_headers(&service, &headers, true) {
        Ok(security) => security,
        Err(error) => return error.response(response_request_id),
    };
    let request: CreateInvitationRequest = match decode_json(&headers, &body) {
        Ok(request) => request,
        Err(error) => return error.response(response_request_id),
    };
    let expected_version = security.expected_version.expect("required above");
    match service
        .create_invitation(
            &security.session_secret,
            &security.csrf_token,
            &security.idempotency_key,
            expected_version,
            &team_id,
            request,
            &response_request_id,
        )
        .await
    {
        Ok(result) => json_response(
            StatusCode::CREATED,
            &result.body,
            Some(result.team_resource_version),
            response_request_id,
        ),
        Err(error) => error.response(response_request_id),
    }
}

pub(super) async fn cancel_invitation(
    State(service): State<PortalService>,
    Path(invitation_action): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let response_request_id = request_id();
    let Some(invitation_id) = invitation_action.strip_suffix(":cancel") else {
        return PortalError::NotFound.response(response_request_id);
    };
    if !valid_id(invitation_id, "invite_") {
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
        .cancel_invitation(
            &security.session_secret,
            &security.csrf_token,
            &security.idempotency_key,
            expected_version,
            invitation_id,
            &response_request_id,
        )
        .await
    {
        Ok(result) => json_response(
            StatusCode::OK,
            &result.body,
            Some(result.team_resource_version),
            response_request_id,
        ),
        Err(error) => error.response(response_request_id),
    }
}

pub(super) async fn inspect_invitation(
    State(service): State<PortalService>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let response_request_id = request_id();
    let request: InvitationTokenRequest = match decode_json(&headers, &body) {
        Ok(request) => request,
        Err(error) => return error.response(response_request_id),
    };
    if !super::valid_secret(&request.invitation_token) {
        return PortalError::NotFound.response(response_request_id);
    }
    match service.inspect_invitation(&request.invitation_token).await {
        Ok(preview) => json_response(StatusCode::OK, &preview, None, response_request_id),
        Err(error) => error.response(response_request_id),
    }
}

pub(super) async fn accept_invitation(
    State(service): State<PortalService>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let response_request_id = request_id();
    let security = match mutation_headers(&service, &headers, false) {
        Ok(security) => security,
        Err(error) => return error.response(response_request_id),
    };
    let request: InvitationTokenRequest = match decode_json(&headers, &body) {
        Ok(request) => request,
        Err(error) => return error.response(response_request_id),
    };
    if !super::valid_secret(&request.invitation_token) {
        return PortalError::NotFound.response(response_request_id);
    }
    match service
        .accept_invitation(
            &security.session_secret,
            &security.csrf_token,
            &security.idempotency_key,
            &request.invitation_token,
            &response_request_id,
        )
        .await
    {
        Ok(outcome) => {
            let mut response = json_response(
                StatusCode::OK,
                &outcome.accepted.body,
                Some(outcome.accepted.team_resource_version),
                response_request_id,
            );
            if let Some(secret) = outcome.rotated_session_secret {
                if let Ok(cookie) = HeaderValue::from_str(&session_cookie(&secret)) {
                    response.headers_mut().insert(SET_COOKIE, cookie);
                }
            }
            response
        }
        Err(error) => error.response(response_request_id),
    }
}

impl PortalService {
    async fn list_invitations(
        &self,
        session_secret: &str,
        team_id: &str,
    ) -> Result<Versioned<InvitationList>, PortalError> {
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let session = active_session(&mut transaction, session_secret, None, now).await?;
        require_owner(&mut transaction, &session, team_id, now, false).await?;
        let team_resource_version = lock_team(&mut transaction, team_id).await?;
        let rows = sqlx::query(
            "SELECT invitation_id, team_id, email, role, \
             CASE WHEN state = 'pending' AND expires_unix_seconds <= $1 \
                  THEN 'expired' ELSE state END AS state, \
             created_unix_seconds, expires_unix_seconds, resource_version, \
             team_resource_version FROM team_invitations WHERE team_id = $2 \
             ORDER BY created_unix_seconds DESC, invitation_id LIMIT 100",
        )
        .bind(now)
        .bind(team_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        let items = rows.into_iter().map(invitation_from_row).collect();
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(Versioned {
            body: InvitationList { items },
            team_resource_version,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_invitation(
        &self,
        session_secret: &str,
        csrf_token: &str,
        idempotency_key: &str,
        expected_version: i64,
        team_id: &str,
        request: CreateInvitationRequest,
        request_id: &str,
    ) -> Result<Versioned<Invitation>, PortalError> {
        let email =
            normalize_verified_email(&request.email).map_err(|_| PortalError::InvalidRequest)?;
        if !matches!(
            request.role.as_str(),
            "developer" | "release-manager" | "viewer"
        ) {
            return Err(PortalError::InvalidRequest);
        }
        let request_sha256 = request_digest(&json!({
            "operation": "invitation-create",
            "team_id": team_id,
            "email": email,
            "role": request.role,
            "expected_version": expected_version
        }))?;
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let session =
            active_session(&mut transaction, session_secret, Some(csrf_token), now).await?;
        let actor = require_owner(&mut transaction, &session, team_id, now, true).await?;
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
                if status == StatusCode::CREATED.as_u16() as i16 =>
            {
                let invitation: Invitation =
                    serde_json::from_value(body).map_err(|_| PortalError::Internal)?;
                transaction
                    .commit()
                    .await
                    .map_err(|_| PortalError::Unavailable)?;
                return Ok(Versioned {
                    team_resource_version: invitation.team_resource_version,
                    body: invitation,
                });
            }
            IdempotencyReservation::Replay { .. } => return Err(PortalError::Internal),
        }
        let current_version = lock_team(&mut transaction, team_id).await?;
        if current_version != expected_version {
            return Err(PortalError::PreconditionFailed);
        }
        enforce_invitation_rate_limits(&mut transaction, team_id, &email, now).await?;
        let conflict: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM team_members \
             WHERE team_id = $1 AND email = $2) OR \
             EXISTS (SELECT 1 FROM team_invitations \
             WHERE team_id = $1 AND email = $2 AND state = 'pending')",
        )
        .bind(team_id)
        .bind(&email)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        if conflict {
            return Err(PortalError::Conflict);
        }
        let invitation_id = opaque_id("invite_");
        let invitation_token = self
            .inner
            .secrets
            .random_token()
            .map_err(|_| PortalError::Internal)?;
        let token_sha256 = sha256_hex(invitation_token.as_bytes());
        let token_ciphertext = self
            .inner
            .secrets
            .encrypt_invitation_token(&invitation_id, &invitation_token)
            .map_err(|_| PortalError::Internal)?;
        let new_team_version = current_version + 1;
        update_team_version(&mut transaction, team_id, current_version).await?;
        let invitation = Invitation {
            invitation_id,
            team_id: team_id.to_owned(),
            email,
            role: request.role,
            state: "pending".to_owned(),
            created_unix_seconds: now,
            expires_unix_seconds: now + INVITATION_TTL_SECONDS,
            resource_version: 1,
            team_resource_version: new_team_version,
        };
        sqlx::query(
            "INSERT INTO team_invitations (invitation_id, team_id, email, role, token_sha256, \
             state, invited_by_member_id, team_resource_version, created_unix_seconds, \
             expires_unix_seconds) VALUES ($1, $2, $3, $4, $5, 'pending', $6, $7, $8, $9)",
        )
        .bind(&invitation.invitation_id)
        .bind(&invitation.team_id)
        .bind(&invitation.email)
        .bind(&invitation.role)
        .bind(token_sha256)
        .bind(&actor.member_id)
        .bind(new_team_version)
        .bind(now)
        .bind(invitation.expires_unix_seconds)
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Conflict)?;
        sqlx::query(
            "INSERT INTO portal_invitation_deliveries (delivery_id, invitation_id, \
             token_ciphertext, state, available_unix_seconds, created_unix_seconds) \
             VALUES ($1, $2, $3, 'pending', $4, $4)",
        )
        .bind(opaque_id("delivery_"))
        .bind(&invitation.invitation_id)
        .bind(token_ciphertext)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        let response_body = serde_json::to_value(&invitation).map_err(|_| PortalError::Internal)?;
        complete_idempotency(
            &mut transaction,
            &session.account_id,
            &key_sha256,
            StatusCode::CREATED,
            &response_body,
        )
        .await?;
        append_invitation_event(
            &mut transaction,
            InvitationEvent {
                now,
                actor_id: &actor.member_id,
                action: "team.invitation-created",
                invitation: &invitation,
                before_state: None,
                request_id,
                request_sha256: &request_sha256,
                key_sha256: &key_sha256,
            },
        )
        .await?;
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(Versioned {
            team_resource_version: new_team_version,
            body: invitation,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn cancel_invitation(
        &self,
        session_secret: &str,
        csrf_token: &str,
        idempotency_key: &str,
        expected_version: i64,
        invitation_id: &str,
        request_id: &str,
    ) -> Result<Versioned<Invitation>, PortalError> {
        let request_sha256 = request_digest(&json!({
            "operation": "invitation-cancel",
            "invitation_id": invitation_id,
            "expected_version": expected_version
        }))?;
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let session =
            active_session(&mut transaction, session_secret, Some(csrf_token), now).await?;
        let invitation = lock_invitation_by_id(&mut transaction, invitation_id).await?;
        let actor =
            require_owner(&mut transaction, &session, &invitation.team_id, now, true).await?;
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
                let invitation: Invitation =
                    serde_json::from_value(body).map_err(|_| PortalError::Internal)?;
                transaction
                    .commit()
                    .await
                    .map_err(|_| PortalError::Unavailable)?;
                return Ok(Versioned {
                    team_resource_version: invitation.team_resource_version,
                    body: invitation,
                });
            }
            IdempotencyReservation::Replay { .. } => return Err(PortalError::Internal),
        }
        let current_version = lock_team(&mut transaction, &invitation.team_id).await?;
        if current_version != expected_version {
            return Err(PortalError::PreconditionFailed);
        }
        if invitation.state != "pending" || invitation.expires_unix_seconds <= now {
            return Err(PortalError::Conflict);
        }
        let new_team_version = current_version + 1;
        update_team_version(&mut transaction, &invitation.team_id, current_version).await?;
        sqlx::query(
            "UPDATE team_invitations SET state = 'cancelled', decided_unix_seconds = $1, \
             resource_version = resource_version + 1, team_resource_version = $2 \
             WHERE invitation_id = $3 AND state = 'pending'",
        )
        .bind(now)
        .bind(new_team_version)
        .bind(invitation_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        cancel_delivery(&mut transaction, invitation_id).await?;
        let cancelled = Invitation {
            state: "cancelled".to_owned(),
            resource_version: invitation.resource_version + 1,
            team_resource_version: new_team_version,
            ..invitation
        };
        let response_body = serde_json::to_value(&cancelled).map_err(|_| PortalError::Internal)?;
        complete_idempotency(
            &mut transaction,
            &session.account_id,
            &key_sha256,
            StatusCode::OK,
            &response_body,
        )
        .await?;
        append_invitation_event(
            &mut transaction,
            InvitationEvent {
                now,
                actor_id: &actor.member_id,
                action: "team.invitation-cancelled",
                invitation: &cancelled,
                before_state: Some("pending"),
                request_id,
                request_sha256: &request_sha256,
                key_sha256: &key_sha256,
            },
        )
        .await?;
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(Versioned {
            team_resource_version: new_team_version,
            body: cancelled,
        })
    }

    async fn inspect_invitation(
        &self,
        invitation_token: &str,
    ) -> Result<InvitationPreview, PortalError> {
        let token_sha256 = sha256_hex(invitation_token.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let row = sqlx::query(
            "SELECT team.name, invitation.email, invitation.role, \
             invitation.expires_unix_seconds FROM team_invitations invitation \
             JOIN teams team ON team.team_id = invitation.team_id \
             WHERE invitation.token_sha256 = $1 AND invitation.state = 'pending' \
             AND invitation.expires_unix_seconds > $2",
        )
        .bind(token_sha256)
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?
        .ok_or(PortalError::NotFound)?;
        let email: String = row.get("email");
        let preview = InvitationPreview {
            team_name: row.get("name"),
            masked_email: mask_email(&email)?,
            role: row.get("role"),
            expires_unix_seconds: row.get("expires_unix_seconds"),
        };
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(preview)
    }

    async fn accept_invitation(
        &self,
        session_secret: &str,
        csrf_token: &str,
        idempotency_key: &str,
        invitation_token: &str,
        request_id: &str,
    ) -> Result<AcceptanceOutcome, PortalError> {
        let token_sha256 = sha256_hex(invitation_token.as_bytes());
        let request_sha256 = request_digest(&json!({
            "operation": "invitation-accept",
            "invitation_token_sha256": token_sha256
        }))?;
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        let mut transaction = serializable(&self.inner.pool).await?;
        let now = database_now(&mut transaction).await?;
        let session =
            active_session(&mut transaction, session_secret, Some(csrf_token), now).await?;
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
                let accepted: AcceptedInvitation =
                    serde_json::from_value(body).map_err(|_| PortalError::Internal)?;
                transaction
                    .commit()
                    .await
                    .map_err(|_| PortalError::Unavailable)?;
                return Ok(AcceptanceOutcome {
                    accepted: Versioned {
                        team_resource_version: accepted.invitation.team_resource_version,
                        body: accepted,
                    },
                    rotated_session_secret: None,
                });
            }
            IdempotencyReservation::Replay { .. } => return Err(PortalError::Internal),
        }
        let invitation = lock_invitation_by_token(&mut transaction, &token_sha256).await?;
        if invitation.state != "pending"
            || invitation.expires_unix_seconds <= now
            || invitation.email != session.email
            || !session.email_verified
        {
            return Err(PortalError::NotFound);
        }
        let current_version = lock_team(&mut transaction, &invitation.team_id).await?;
        let existing: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM team_members \
             WHERE team_id = $1 AND account_id = $2)",
        )
        .bind(&invitation.team_id)
        .bind(&session.account_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        if existing {
            return Err(PortalError::Conflict);
        }
        let new_team_version = current_version + 1;
        let member_id = opaque_id("member_");
        update_team_version(&mut transaction, &invitation.team_id, current_version).await?;
        sqlx::query(
            "INSERT INTO team_members (member_id, team_id, account_id, email, role, \
             two_factor_enabled) VALUES ($1, $2, $3, $4, $5, FALSE)",
        )
        .bind(&member_id)
        .bind(&invitation.team_id)
        .bind(&session.account_id)
        .bind(&invitation.email)
        .bind(&invitation.role)
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Conflict)?;
        sqlx::query(
            "UPDATE team_invitations SET state = 'accepted', accepted_account_id = $1, \
             accepted_member_id = $2, decided_unix_seconds = $3, \
             resource_version = resource_version + 1, team_resource_version = $4 \
             WHERE invitation_id = $5 AND state = 'pending'",
        )
        .bind(&session.account_id)
        .bind(&member_id)
        .bind(now)
        .bind(new_team_version)
        .bind(&invitation.invitation_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?;
        cancel_delivery(&mut transaction, &invitation.invitation_id).await?;
        let accepted_invitation = Invitation {
            state: "accepted".to_owned(),
            resource_version: invitation.resource_version + 1,
            team_resource_version: new_team_version,
            ..invitation
        };
        let team_name: String = sqlx::query_scalar("SELECT name FROM teams WHERE team_id = $1")
            .bind(&accepted_invitation.team_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| PortalError::Unavailable)?;
        let team = TeamSummary {
            team_id: accepted_invitation.team_id.clone(),
            name: team_name,
            role: accepted_invitation.role.clone(),
            membership_state: "active".to_owned(),
            resource_version: new_team_version,
        };
        let accepted = AcceptedInvitation {
            invitation: accepted_invitation,
            team,
        };
        let rotated_session_secret = self
            .inner
            .secrets
            .random_token()
            .map_err(|_| PortalError::Internal)?;
        rotate_session(
            &mut transaction,
            &self.inner.secrets,
            &session,
            &rotated_session_secret,
            now,
        )
        .await?;
        let response_body = serde_json::to_value(&accepted).map_err(|_| PortalError::Internal)?;
        complete_idempotency(
            &mut transaction,
            &session.account_id,
            &key_sha256,
            StatusCode::OK,
            &response_body,
        )
        .await?;
        append_invitation_event(
            &mut transaction,
            InvitationEvent {
                now,
                actor_id: &member_id,
                action: "team.invitation-accepted",
                invitation: &accepted.invitation,
                before_state: Some("pending"),
                request_id,
                request_sha256: &request_sha256,
                key_sha256: &key_sha256,
            },
        )
        .await?;
        transaction
            .commit()
            .await
            .map_err(|_| PortalError::Unavailable)?;
        Ok(AcceptanceOutcome {
            accepted: Versioned {
                team_resource_version: new_team_version,
                body: accepted,
            },
            rotated_session_secret: Some(rotated_session_secret),
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

async fn require_owner(
    transaction: &mut Transaction<'_, Postgres>,
    session: &SessionRow,
    team_id: &str,
    now: i64,
    fresh_mfa: bool,
) -> Result<InvitationActor, PortalError> {
    let row = sqlx::query(
        "SELECT member_id, role, two_factor_enabled, membership_state \
         FROM team_members WHERE team_id = $1 AND account_id = $2 FOR UPDATE",
    )
    .bind(team_id)
    .bind(&session.account_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?
    .ok_or(PortalError::NotFound)?;
    if row.get::<String, _>("membership_state") != "active"
        || row.get::<String, _>("role") != "owner"
    {
        return Err(PortalError::Forbidden);
    }
    if fresh_mfa
        && (!row.get::<bool, _>("two_factor_enabled")
            || !session
                .mfa_authenticated_unix_seconds
                .is_some_and(|value| value >= now - 300 && value <= now))
    {
        return Err(PortalError::Forbidden);
    }
    Ok(InvitationActor {
        member_id: row.get("member_id"),
    })
}

async fn lock_team(
    transaction: &mut Transaction<'_, Postgres>,
    team_id: &str,
) -> Result<i64, PortalError> {
    sqlx::query_scalar("SELECT resource_version FROM teams WHERE team_id = $1 FOR UPDATE")
        .bind(team_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|_| PortalError::Unavailable)?
        .ok_or(PortalError::NotFound)
}

async fn update_team_version(
    transaction: &mut Transaction<'_, Postgres>,
    team_id: &str,
    current_version: i64,
) -> Result<(), PortalError> {
    let affected = sqlx::query(
        "UPDATE teams SET resource_version = resource_version + 1 \
         WHERE team_id = $1 AND resource_version = $2",
    )
    .bind(team_id)
    .bind(current_version)
    .execute(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?
    .rows_affected();
    if affected != 1 {
        return Err(PortalError::PreconditionFailed);
    }
    Ok(())
}

async fn lock_invitation_by_id(
    transaction: &mut Transaction<'_, Postgres>,
    invitation_id: &str,
) -> Result<Invitation, PortalError> {
    let row = sqlx::query(
        "SELECT invitation_id, team_id, email, role, state, created_unix_seconds, \
         expires_unix_seconds, resource_version, team_resource_version \
         FROM team_invitations WHERE invitation_id = $1 FOR UPDATE",
    )
    .bind(invitation_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?
    .ok_or(PortalError::NotFound)?;
    Ok(invitation_from_row(row))
}

async fn lock_invitation_by_token(
    transaction: &mut Transaction<'_, Postgres>,
    token_sha256: &str,
) -> Result<Invitation, PortalError> {
    let row = sqlx::query(
        "SELECT invitation_id, team_id, email, role, state, created_unix_seconds, \
         expires_unix_seconds, resource_version, team_resource_version \
         FROM team_invitations WHERE token_sha256 = $1 FOR UPDATE",
    )
    .bind(token_sha256)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?
    .ok_or(PortalError::NotFound)?;
    Ok(invitation_from_row(row))
}

fn invitation_from_row(row: sqlx::postgres::PgRow) -> Invitation {
    Invitation {
        invitation_id: row.get("invitation_id"),
        team_id: row.get("team_id"),
        email: row.get("email"),
        role: row.get("role"),
        state: row.get("state"),
        created_unix_seconds: row.get("created_unix_seconds"),
        expires_unix_seconds: row.get("expires_unix_seconds"),
        resource_version: row.get("resource_version"),
        team_resource_version: row.get("team_resource_version"),
    }
}

async fn enforce_invitation_rate_limits(
    transaction: &mut Transaction<'_, Postgres>,
    team_id: &str,
    email: &str,
    now: i64,
) -> Result<(), PortalError> {
    let team_recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM team_invitations \
         WHERE team_id = $1 AND created_unix_seconds > $2",
    )
    .bind(team_id)
    .bind(now - 3600)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    let email_recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM team_invitations \
         WHERE team_id = $1 AND email = $2 AND created_unix_seconds > $3",
    )
    .bind(team_id)
    .bind(email)
    .bind(now - 86400)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    if team_recent >= MAX_TEAM_INVITATIONS_PER_HOUR || email_recent >= MAX_EMAIL_INVITATIONS_PER_DAY
    {
        return Err(PortalError::RateLimited);
    }
    Ok(())
}

async fn cancel_delivery(
    transaction: &mut Transaction<'_, Postgres>,
    invitation_id: &str,
) -> Result<(), PortalError> {
    sqlx::query(
        "UPDATE portal_invitation_deliveries SET state = 'cancelled', \
         token_ciphertext = NULL, lease_owner = NULL, lease_expires_unix_seconds = NULL, \
         delivered_unix_seconds = NULL, last_error_code = NULL, \
         resource_version = resource_version + 1 \
         WHERE invitation_id = $1 AND state IN ('pending', 'leased')",
    )
    .bind(invitation_id)
    .execute(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    Ok(())
}

async fn rotate_session(
    transaction: &mut Transaction<'_, Postgres>,
    secrets: &crate::PortalSecrets,
    old: &SessionRow,
    new_secret: &str,
    now: i64,
) -> Result<(), PortalError> {
    let new_sha256 = sha256_hex(new_secret.as_bytes());
    let csrf = secrets.csrf_for_session(new_secret);
    let csrf_sha256 = sha256_hex(csrf.as_bytes());
    sqlx::query(
        "UPDATE portal_sessions SET state = 'revoked', ended_unix_seconds = $1, \
         resource_version = resource_version + 1 \
         WHERE session_sha256 = $2 AND state = 'active'",
    )
    .bind(now.max(old.created_unix_seconds))
    .bind(&old.session_sha256)
    .execute(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    sqlx::query(
        "INSERT INTO portal_sessions (session_sha256, csrf_sha256, account_id, \
         current_link_id, state, created_unix_seconds, last_seen_unix_seconds, \
         idle_expires_unix_seconds, absolute_expires_unix_seconds, \
         mfa_authenticated_unix_seconds) VALUES \
         ($1, $2, $3, $4, 'active', $5, $5, $5 + 1800, $5 + 28800, $6)",
    )
    .bind(new_sha256)
    .bind(csrf_sha256)
    .bind(&old.account_id)
    .bind(&old.current_link_id)
    .bind(now)
    .bind(
        old.mfa_authenticated_unix_seconds
            .filter(|value| *value <= now),
    )
    .execute(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    Ok(())
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

struct InvitationEvent<'a> {
    now: i64,
    actor_id: &'a str,
    action: &'a str,
    invitation: &'a Invitation,
    before_state: Option<&'a str>,
    request_id: &'a str,
    request_sha256: &'a str,
    key_sha256: &'a str,
}

async fn append_invitation_event(
    transaction: &mut Transaction<'_, Postgres>,
    event: InvitationEvent<'_>,
) -> Result<(), PortalError> {
    sqlx::query(
        "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
         object_id, before_state, after_state, resource_version, request_id, request_sha256, \
         idempotency_key_sha256) VALUES ($1, $2, $3, 'team-invitation', $4, $5, $6, \
         $7, $8, $9, $10)",
    )
    .bind(event.now)
    .bind(event.actor_id)
    .bind(event.action)
    .bind(&event.invitation.invitation_id)
    .bind(event.before_state)
    .bind(&event.invitation.state)
    .bind(event.invitation.resource_version)
    .bind(event.request_id)
    .bind(event.request_sha256)
    .bind(event.key_sha256)
    .execute(&mut **transaction)
    .await
    .map_err(|_| PortalError::Unavailable)?;
    sqlx::query(
        "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
         aggregate_version, request_sha256, payload, created_unix_seconds) \
         VALUES ($1, $2, 'team', $3, $4, $5, $6, $7)",
    )
    .bind(opaque_id("evt_"))
    .bind(event.action)
    .bind(&event.invitation.team_id)
    .bind(event.invitation.team_resource_version)
    .bind(event.request_sha256)
    .bind(json!({
        "invitation_id": event.invitation.invitation_id,
        "team_id": event.invitation.team_id,
        "role": event.invitation.role,
        "state": event.invitation.state,
        "team_resource_version": event.invitation.team_resource_version
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

fn mask_email(email: &str) -> Result<String, PortalError> {
    let (local, domain) = email.split_once('@').ok_or(PortalError::Internal)?;
    let first = local.chars().next().ok_or(PortalError::Internal)?;
    Ok(format!("{first}***@{domain}"))
}

fn valid_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}
