use std::future::Future;
use std::pin::Pin;

use sqlx::postgres::PgPool;
use sqlx::{Postgres, Row, Transaction};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{PortalSecrets, sha256_hex};

const LEASE_SECONDS: i64 = 60;
const MAX_ATTEMPTS: i16 = 16;

pub struct InvitationDelivery<'a> {
    pub invitation_id: &'a str,
    pub email: &'a str,
    pub team_name: &'a str,
    pub role: &'a str,
    pub acceptance_url: &'a str,
    pub expires_unix_seconds: i64,
}

#[derive(Clone, Copy, Debug)]
pub enum InvitationDeliveryFailure {
    Transient(&'static str),
    Permanent(&'static str),
}

pub type InvitationDeliveryFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), InvitationDeliveryFailure>> + Send + 'a>>;

pub trait InvitationMailer: Send + Sync {
    fn deliver<'a>(&'a self, delivery: InvitationDelivery<'a>) -> InvitationDeliveryFuture<'a>;
}

#[derive(Debug)]
pub enum InvitationWorkerError {
    InvalidConfiguration,
    Database,
    Decryption,
}

pub struct InvitationEmailWorker {
    pool: PgPool,
    secrets: PortalSecrets,
    acceptance_uri: String,
}

struct LeasedDelivery {
    delivery_id: String,
    invitation_id: String,
    token_ciphertext: Vec<u8>,
    attempts: i16,
    email: String,
    team_name: String,
    role: String,
    expires_unix_seconds: i64,
}

impl InvitationEmailWorker {
    pub fn new(
        pool: PgPool,
        secrets: PortalSecrets,
        allowed_origin: &str,
    ) -> Result<Self, InvitationWorkerError> {
        let origin =
            Url::parse(allowed_origin).map_err(|_| InvitationWorkerError::InvalidConfiguration)?;
        if origin.scheme() != "https"
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
        {
            return Err(InvitationWorkerError::InvalidConfiguration);
        }
        Ok(Self {
            pool,
            secrets,
            acceptance_uri: format!(
                "{}/portal/invitations/accept",
                origin.origin().ascii_serialization()
            ),
        })
    }

    pub async fn run_once(
        &self,
        worker_id: &str,
        mailer: &dyn InvitationMailer,
    ) -> Result<bool, InvitationWorkerError> {
        if !valid_worker_id(worker_id) {
            return Err(InvitationWorkerError::InvalidConfiguration);
        }
        if self.expire_one().await? {
            return Ok(true);
        }
        let Some(leased) = self.lease(worker_id).await? else {
            return Ok(false);
        };
        let token = match self
            .secrets
            .decrypt_invitation_token(&leased.invitation_id, &leased.token_ciphertext)
        {
            Ok(token) => token,
            Err(_) => {
                self.finish(
                    worker_id,
                    &leased,
                    Err(InvitationDeliveryFailure::Permanent("decrypt-failed")),
                )
                .await?;
                return Err(InvitationWorkerError::Decryption);
            }
        };
        let acceptance_url = invitation_url(&self.acceptance_uri, &token)?;
        let result = mailer
            .deliver(InvitationDelivery {
                invitation_id: &leased.invitation_id,
                email: &leased.email,
                team_name: &leased.team_name,
                role: &leased.role,
                acceptance_url: &acceptance_url,
                expires_unix_seconds: leased.expires_unix_seconds,
            })
            .await;
        self.finish(worker_id, &leased, result).await?;
        Ok(true)
    }

    async fn expire_one(&self) -> Result<bool, InvitationWorkerError> {
        let mut transaction = serializable(&self.pool).await?;
        let now = database_now(&mut transaction).await?;
        let row = sqlx::query(
            "SELECT invitation.invitation_id, invitation.team_id, invitation.role, \
             invitation.resource_version, invitation.expires_unix_seconds, \
             team.resource_version AS team_resource_version \
             FROM team_invitations invitation \
             JOIN teams team ON team.team_id = invitation.team_id \
             WHERE invitation.state = 'pending' AND invitation.expires_unix_seconds <= $1 \
             ORDER BY invitation.expires_unix_seconds, invitation.invitation_id \
             FOR UPDATE OF team, invitation SKIP LOCKED LIMIT 1",
        )
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| InvitationWorkerError::Database)?;
        let Some(row) = row else {
            transaction
                .commit()
                .await
                .map_err(|_| InvitationWorkerError::Database)?;
            return Ok(false);
        };
        let invitation_id: String = row.get("invitation_id");
        let team_id: String = row.get("team_id");
        let role: String = row.get("role");
        let invitation_version: i64 = row.get("resource_version");
        let team_version: i64 = row.get("team_resource_version");
        let expires: i64 = row.get("expires_unix_seconds");
        let new_team_version = team_version + 1;
        sqlx::query(
            "UPDATE teams SET resource_version = resource_version + 1 \
             WHERE team_id = $1 AND resource_version = $2",
        )
        .bind(&team_id)
        .bind(team_version)
        .execute(&mut *transaction)
        .await
        .map_err(|_| InvitationWorkerError::Database)?;
        sqlx::query(
            "UPDATE team_invitations SET state = 'expired', decided_unix_seconds = $1, \
             resource_version = resource_version + 1, team_resource_version = $2 \
             WHERE invitation_id = $3 AND state = 'pending'",
        )
        .bind(now)
        .bind(new_team_version)
        .bind(&invitation_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| InvitationWorkerError::Database)?;
        sqlx::query(
            "UPDATE portal_invitation_deliveries SET state = 'cancelled', \
             token_ciphertext = NULL, lease_owner = NULL, lease_expires_unix_seconds = NULL, \
             delivered_unix_seconds = NULL, last_error_code = NULL, \
             resource_version = resource_version + 1 \
             WHERE invitation_id = $1 AND state IN ('pending', 'leased')",
        )
        .bind(&invitation_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| InvitationWorkerError::Database)?;
        let request_sha256 =
            sha256_hex(format!("invitation-expire\0{invitation_id}\0{expires}").as_bytes());
        let key_sha256 = sha256_hex(format!("system-expire\0{invitation_id}").as_bytes());
        let request_id = prefixed_uuid("req_");
        sqlx::query(
            "INSERT INTO audit_events (occurred_unix_seconds, actor_id, action, object_kind, \
             object_id, before_state, after_state, resource_version, request_id, request_sha256, \
             idempotency_key_sha256) VALUES ($1, 'portal-invitation-expirer', \
             'team.invitation-expired', 'team-invitation', $2, 'pending', 'expired', \
             $3, $4, $5, $6)",
        )
        .bind(now)
        .bind(&invitation_id)
        .bind(invitation_version + 1)
        .bind(request_id)
        .bind(&request_sha256)
        .bind(key_sha256)
        .execute(&mut *transaction)
        .await
        .map_err(|_| InvitationWorkerError::Database)?;
        sqlx::query(
            "INSERT INTO outbox_events (event_id, topic, aggregate_kind, aggregate_id, \
             aggregate_version, request_sha256, payload, created_unix_seconds) \
             VALUES ($1, 'team.invitation-expired', 'team', $2, $3, $4, \
             jsonb_build_object('invitation_id', $5::TEXT, 'team_id', $2::TEXT, \
             'role', $6::TEXT, 'state', 'expired', 'team_resource_version', $3::BIGINT), $7)",
        )
        .bind(prefixed_uuid("evt_"))
        .bind(&team_id)
        .bind(new_team_version)
        .bind(request_sha256)
        .bind(&invitation_id)
        .bind(role)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(|_| InvitationWorkerError::Database)?;
        transaction
            .commit()
            .await
            .map_err(|_| InvitationWorkerError::Database)?;
        Ok(true)
    }

    async fn lease(
        &self,
        worker_id: &str,
    ) -> Result<Option<LeasedDelivery>, InvitationWorkerError> {
        let mut transaction = serializable(&self.pool).await?;
        let now = database_now(&mut transaction).await?;
        let row = sqlx::query(
            "SELECT delivery.delivery_id, delivery.invitation_id, \
             delivery.token_ciphertext, delivery.attempts, invitation.email, \
             invitation.role, invitation.expires_unix_seconds, team.name \
             FROM portal_invitation_deliveries delivery \
             JOIN team_invitations invitation \
               ON invitation.invitation_id = delivery.invitation_id \
             JOIN teams team ON team.team_id = invitation.team_id \
             WHERE ((delivery.state = 'pending' AND delivery.available_unix_seconds <= $1) OR \
                    (delivery.state = 'leased' AND delivery.lease_expires_unix_seconds <= $1)) \
               AND delivery.attempts < $2 AND invitation.state = 'pending' \
               AND invitation.expires_unix_seconds > $1 \
             ORDER BY delivery.available_unix_seconds, delivery.delivery_id \
             FOR UPDATE OF delivery SKIP LOCKED LIMIT 1",
        )
        .bind(now)
        .bind(MAX_ATTEMPTS)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| InvitationWorkerError::Database)?;
        let Some(row) = row else {
            transaction
                .commit()
                .await
                .map_err(|_| InvitationWorkerError::Database)?;
            return Ok(None);
        };
        let delivery_id: String = row.get("delivery_id");
        let attempts: i16 = row.get::<i16, _>("attempts") + 1;
        let affected = sqlx::query(
            "UPDATE portal_invitation_deliveries SET state = 'leased', attempts = $1, \
             lease_owner = $2, lease_expires_unix_seconds = $3, last_error_code = NULL, \
             resource_version = resource_version + 1 WHERE delivery_id = $4 \
             AND state IN ('pending', 'leased')",
        )
        .bind(attempts)
        .bind(worker_id)
        .bind(now + LEASE_SECONDS)
        .bind(&delivery_id)
        .execute(&mut *transaction)
        .await
        .map_err(|_| InvitationWorkerError::Database)?
        .rows_affected();
        if affected != 1 {
            return Err(InvitationWorkerError::Database);
        }
        let leased = LeasedDelivery {
            delivery_id,
            invitation_id: row.get("invitation_id"),
            token_ciphertext: row.get("token_ciphertext"),
            attempts,
            email: row.get("email"),
            team_name: row.get("name"),
            role: row.get("role"),
            expires_unix_seconds: row.get("expires_unix_seconds"),
        };
        transaction
            .commit()
            .await
            .map_err(|_| InvitationWorkerError::Database)?;
        Ok(Some(leased))
    }

    async fn finish(
        &self,
        worker_id: &str,
        leased: &LeasedDelivery,
        result: Result<(), InvitationDeliveryFailure>,
    ) -> Result<(), InvitationWorkerError> {
        let mut transaction = serializable(&self.pool).await?;
        let now = database_now(&mut transaction).await?;
        let (state, retry_at, error_code) = match result {
            Ok(()) => ("delivered", None, None),
            Err(InvitationDeliveryFailure::Transient(code)) if leased.attempts < MAX_ATTEMPTS => {
                let shift = u32::try_from(leased.attempts.saturating_sub(1).min(6))
                    .map_err(|_| InvitationWorkerError::Database)?;
                let delay = (30_i64 * (1_i64 << shift)).min(3600);
                ("pending", Some(now + delay), Some(safe_error_code(code)))
            }
            Err(InvitationDeliveryFailure::Transient(code))
            | Err(InvitationDeliveryFailure::Permanent(code)) => {
                ("failed", None, Some(safe_error_code(code)))
            }
        };
        let affected = if state == "pending" {
            sqlx::query(
                "UPDATE portal_invitation_deliveries SET state = 'pending', \
                 available_unix_seconds = $1, lease_owner = NULL, \
                 lease_expires_unix_seconds = NULL, last_error_code = $2, \
                 resource_version = resource_version + 1 \
                 WHERE delivery_id = $3 AND state = 'leased' AND lease_owner = $4 \
                 AND attempts = $5",
            )
            .bind(retry_at.ok_or(InvitationWorkerError::Database)?)
            .bind(error_code)
            .bind(&leased.delivery_id)
            .bind(worker_id)
            .bind(leased.attempts)
            .execute(&mut *transaction)
            .await
        } else {
            sqlx::query(
                "UPDATE portal_invitation_deliveries SET state = $1, token_ciphertext = NULL, \
                 lease_owner = NULL, lease_expires_unix_seconds = NULL, \
                 delivered_unix_seconds = CASE WHEN $1 = 'delivered' THEN $2 ELSE NULL END, \
                 last_error_code = $3, resource_version = resource_version + 1 \
                 WHERE delivery_id = $4 AND state = 'leased' AND lease_owner = $5 \
                 AND attempts = $6",
            )
            .bind(state)
            .bind(now)
            .bind(error_code)
            .bind(&leased.delivery_id)
            .bind(worker_id)
            .bind(leased.attempts)
            .execute(&mut *transaction)
            .await
        }
        .map_err(|_| InvitationWorkerError::Database)?
        .rows_affected();
        if affected > 1 {
            return Err(InvitationWorkerError::Database);
        }
        transaction
            .commit()
            .await
            .map_err(|_| InvitationWorkerError::Database)
    }
}

fn invitation_url(base: &str, token: &str) -> Result<Zeroizing<String>, InvitationWorkerError> {
    let url = Zeroizing::new(format!("{base}#token={token}"));
    Url::parse(&url).map_err(|_| InvitationWorkerError::InvalidConfiguration)?;
    Ok(url)
}

fn valid_worker_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_error_code(value: &str) -> &str {
    if (1..=64).contains(&value.len())
        && matches!(value.as_bytes().first(), Some(b'a'..=b'z'))
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        value
    } else {
        "delivery-failed"
    }
}

fn prefixed_uuid(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().simple())
}

async fn serializable(pool: &PgPool) -> Result<Transaction<'_, Postgres>, InvitationWorkerError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| InvitationWorkerError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .map_err(|_| InvitationWorkerError::Database)?;
    Ok(transaction)
}

async fn database_now(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<i64, InvitationWorkerError> {
    sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT")
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| InvitationWorkerError::Database)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_ids_and_error_codes_are_bounded() {
        assert!(valid_worker_id("portal-mailer-1"));
        assert!(!valid_worker_id("worker/one"));
        assert_eq!(safe_error_code("provider-timeout"), "provider-timeout");
        assert_eq!(safe_error_code("UPSTREAM SECRET"), "delivery-failed");
    }

    #[test]
    fn invitation_token_is_fragment_only() {
        let url = invitation_url(
            "https://developer.cardputerzero.dev/portal/invitations/accept",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
        let parsed = Url::parse(&url).unwrap();
        assert!(parsed.query().is_none());
        assert_eq!(
            parsed.fragment(),
            Some("token=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }
}
