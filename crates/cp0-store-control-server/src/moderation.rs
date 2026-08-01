use super::*;

const REPORT_INTAKE_ACTOR: &str = "report-intake";
const MAX_UNTRIAGED_REPORTS: i64 = 10_000;
const SECURITY_ACK_SECONDS: i64 = 4 * 60 * 60;
const SECURITY_RESOLUTION_SECONDS: i64 = 3 * 24 * 60 * 60;
const STANDARD_ACK_SECONDS: i64 = 3 * 24 * 60 * 60;
const STANDARD_RESOLUTION_SECONDS: i64 = 14 * 24 * 60 * 60;
const APPEAL_WINDOW_SECONDS: i64 = 14 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContentReportRequest {
    release_id: String,
    app_id: String,
    version: String,
    reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContentReportResponse {
    report_id: String,
    release_id: String,
    app_id: String,
    version: String,
    reason_code: String,
    sla_class: String,
    state: String,
    disposition: Option<String>,
    decision_reason_codes: Vec<String>,
    acknowledgement_due_unix_seconds: u64,
    resolution_due_unix_seconds: u64,
    acknowledged_unix_seconds: Option<u64>,
    closed_unix_seconds: Option<u64>,
    resource_version: u64,
    created_unix_seconds: u64,
    updated_unix_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModerationDecisionRequest {
    disposition: String,
    reason_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeveloperNoticeResponse {
    notice_id: String,
    report_id: String,
    release_id: String,
    app_id: String,
    version: String,
    state: String,
    reason_codes: Vec<String>,
    appeal_deadline_unix_seconds: u64,
    appeal_id: Option<String>,
    appeal_state: Option<String>,
    resource_version: u64,
    created_unix_seconds: u64,
    updated_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModerationDecisionResponse {
    report: ContentReportResponse,
    notice: Option<DeveloperNoticeResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AppealRequest {
    ground: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AppealDecisionRequest {
    decision: String,
    reason_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AppealResponse {
    appeal_id: String,
    notice_id: String,
    ground: String,
    state: String,
    decision_reason_codes: Vec<String>,
    resource_version: u64,
    created_unix_seconds: u64,
    updated_unix_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModerationQueueQuery {
    cursor: Option<String>,
    limit: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct ModerationQueueResponse {
    items: Vec<ContentReportResponse>,
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct DeveloperNoticeListResponse {
    items: Vec<DeveloperNoticeResponse>,
    next_cursor: Option<String>,
}

struct ModerationCursor {
    unix_seconds: i64,
    object_id: String,
}

impl StoreControlService {
    async fn submit_content_report(
        &self,
        idempotency_key: &str,
        request_id: &str,
        request: &ContentReportRequest,
    ) -> Result<ContentReportResponse, ApiError> {
        validate_content_report(request)?;
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        let request_sha256 = mutation_request_sha256(
            "moderation.report.submit.v1",
            &[
                &request.release_id,
                &request.app_id,
                &request.version,
                &request.reason_code,
            ],
        );
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .submit_content_report_once(&key_sha256, request_id, &request_sha256, request)
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(ApiError::from_transaction(error)),
                Ok(report) => return Ok(report),
            }
        }
        Err(ApiError::unavailable())
    }

    async fn submit_content_report_once(
        &self,
        key_sha256: &str,
        request_id: &str,
        request_sha256: &str,
        request: &ContentReportRequest,
    ) -> Result<ContentReportResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            REPORT_INTAKE_ACTOR,
            key_sha256,
            request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::ACCEPTED.as_u16() as i16 =>
            {
                let report = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(report);
            }
            IdempotencyReservation::Replay { .. } => return Err(ApiError::internal().into()),
        }

        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('cp0.moderation.open-cap.v1', 0))",
        )
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        let untriaged_reports: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM store_content_reports WHERE state = 'submitted'",
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        if untriaged_reports >= MAX_UNTRIAGED_REPORTS {
            return Err(ApiError::unavailable().into());
        }

        let release_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM releases release \
             JOIN submissions submission ON submission.submission_id = release.submission_id \
             JOIN store_package_artifacts artifact ON artifact.release_id = release.release_id \
             WHERE release.release_id = $1 AND release.app_id = $2 AND release.version = $3 \
               AND release.state = 'published' AND submission.state = 'approved' \
               AND submission.app_id = release.app_id AND submission.version = release.version \
               AND artifact.catalog_app->>'app_id' = release.app_id \
               AND artifact.catalog_app->>'version' = release.version)",
        )
        .bind(&request.release_id)
        .bind(&request.app_id)
        .bind(&request.version)
        .fetch_one(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        if !release_exists {
            return Err(ApiError::not_found().into());
        }

        let (sla_class, ack_seconds, resolution_seconds) = report_sla(&request.reason_code);
        let report = ContentReportResponse {
            report_id: prefixed_uuid("report_"),
            release_id: request.release_id.clone(),
            app_id: request.app_id.clone(),
            version: request.version.clone(),
            reason_code: request.reason_code.clone(),
            sla_class: sla_class.into(),
            state: "submitted".into(),
            disposition: None,
            decision_reason_codes: Vec::new(),
            acknowledgement_due_unix_seconds: unix(now + ack_seconds)?,
            resolution_due_unix_seconds: unix(now + resolution_seconds)?,
            acknowledged_unix_seconds: None,
            closed_unix_seconds: None,
            resource_version: 1,
            created_unix_seconds: unix(now)?,
            updated_unix_seconds: unix(now)?,
        };
        sqlx::query(
            "INSERT INTO store_content_reports (report_id, release_id, app_id, version, \
             reason_code, sla_class, state, disposition, decision_reason_codes, \
             acknowledgement_due_unix_seconds, resolution_due_unix_seconds, \
             acknowledged_unix_seconds, closed_unix_seconds, resource_version, \
             created_unix_seconds, updated_unix_seconds) \
             VALUES ($1, $2, $3, $4, $5, $6, 'submitted', NULL, '{}', $7, $8, NULL, NULL, 1, $9, $9)",
        )
        .bind(&report.report_id)
        .bind(&report.release_id)
        .bind(&report.app_id)
        .bind(&report.version)
        .bind(&report.reason_code)
        .bind(&report.sla_class)
        .bind(i64::try_from(report.acknowledgement_due_unix_seconds).map_err(|_| ApiError::internal())?)
        .bind(i64::try_from(report.resolution_due_unix_seconds).map_err(|_| ApiError::internal())?)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        insert_report_revision(
            &mut transaction,
            &report,
            REPORT_INTAKE_ACTOR,
            request_sha256,
            now,
        )
        .await?;
        let body = serde_json::to_value(&report).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            REPORT_INTAKE_ACTOR,
            key_sha256,
            StatusCode::ACCEPTED,
            &body,
        )
        .await?;
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: REPORT_INTAKE_ACTOR,
                action: "moderation.report-submitted",
                topic: "moderation.report-submitted",
                object_kind: "content-report",
                object_id: &report.report_id,
                before_state: None,
                after_state: Some("submitted"),
                resource_version: 1,
                request_id,
                request_sha256,
                key_sha256,
                payload: json!({
                    "report_id": report.report_id,
                    "release_id": report.release_id,
                    "app_id": report.app_id,
                    "version": report.version,
                    "reason_code": report.reason_code,
                    "sla_class": report.sla_class,
                    "acknowledgement_due_unix_seconds": report.acknowledgement_due_unix_seconds,
                    "resolution_due_unix_seconds": report.resolution_due_unix_seconds
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(report)
    }

    async fn list_moderation_reports(
        &self,
        token: &str,
        cursor: Option<ModerationCursor>,
        limit: usize,
    ) -> Result<ModerationQueueResponse, ApiError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate_store_operator(&mut transaction, &sha256_hex(token.as_bytes()))
            .await
            .map_err(ApiError::from_transaction)?;
        require_moderation_access(&identity)?;
        let (after_due, after_id) = cursor
            .map(|cursor| (cursor.unix_seconds, cursor.object_id))
            .unwrap_or((0, String::new()));
        let rows = sqlx::query(
            "SELECT report_id, release_id, app_id, version, reason_code, sla_class, state, \
             disposition, decision_reason_codes, acknowledgement_due_unix_seconds, \
             resolution_due_unix_seconds, acknowledged_unix_seconds, closed_unix_seconds, \
             resource_version, created_unix_seconds, updated_unix_seconds \
             FROM store_content_reports WHERE state = 'submitted' AND \
             (acknowledgement_due_unix_seconds, report_id) > ($1, $2) \
             ORDER BY acknowledgement_due_unix_seconds, report_id LIMIT $3",
        )
        .bind(after_due)
        .bind(after_id)
        .bind(i64::try_from(limit + 1).map_err(|_| ApiError::invalid_request())?)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        let mut items = rows
            .iter()
            .map(content_report_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = has_more.then(|| items.last()).flatten().map(|item| {
            format!(
                "{}:{}",
                item.acknowledgement_due_unix_seconds, item.report_id
            )
        });
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;
        Ok(ModerationQueueResponse { items, next_cursor })
    }

    async fn decide_content_report(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        report_id: &str,
        expected_version: u64,
        request: &ModerationDecisionRequest,
    ) -> Result<ModerationDecisionResponse, ApiError> {
        validate_moderation_decision(request)?;
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        let expected = expected_version.to_string();
        let encoded_reasons = request.reason_codes.join(",");
        let request_sha256 = mutation_request_sha256(
            "moderation.report.decide.v1",
            &[report_id, &expected, &request.disposition, &encoded_reasons],
        );
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .decide_content_report_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    report_id,
                    expected_version,
                    &request_sha256,
                    request,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(ApiError::from_transaction(error)),
                Ok(response) => return Ok(response),
            }
        }
        Err(ApiError::unavailable())
    }

    #[allow(clippy::too_many_arguments)]
    async fn decide_content_report_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        report_id: &str,
        expected_version: u64,
        request_sha256: &str,
        request: &ModerationDecisionRequest,
    ) -> Result<ModerationDecisionResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate_store_operator(&mut transaction, token_sha256).await?;
        require_moderation_access(&identity)?;
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            &identity.operator_id,
            key_sha256,
            request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::OK.as_u16() as i16 =>
            {
                let response = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(response);
            }
            IdempotencyReservation::Replay { .. } => return Err(ApiError::internal().into()),
        }
        let mut report = load_content_report(&mut transaction, report_id, true).await?;
        if report.resource_version != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        if report.state != "submitted" {
            return Err(ApiError::invalid_transition().into());
        }
        let before = report.state.clone();
        let target = match request.disposition.as_str() {
            "no-action" => "closed-no-action",
            "developer-notice" => "notice-issued",
            "security-escalation" => "security-escalated",
            _ => return Err(ApiError::invalid_request().into()),
        };
        report.state = target.into();
        report.disposition = Some(request.disposition.clone());
        report.decision_reason_codes = request.reason_codes.clone();
        report.acknowledged_unix_seconds = Some(unix(now)?);
        report.closed_unix_seconds = (target == "closed-no-action").then_some(unix(now)?);
        report.resource_version = expected_version
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        report.updated_unix_seconds = unix(now)?;
        let closed_unix_seconds = report
            .closed_unix_seconds
            .map(i64::try_from)
            .transpose()
            .map_err(|_| ApiError::internal())?;
        sqlx::query(
            "UPDATE store_content_reports SET state = $1, disposition = $2, \
             decision_reason_codes = $3, acknowledged_unix_seconds = $4, \
             closed_unix_seconds = $5, resource_version = $6, updated_unix_seconds = $4 \
             WHERE report_id = $7",
        )
        .bind(&report.state)
        .bind(&report.disposition)
        .bind(&report.decision_reason_codes)
        .bind(now)
        .bind(closed_unix_seconds)
        .bind(i64::try_from(report.resource_version).map_err(|_| ApiError::internal())?)
        .bind(report_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        insert_report_revision(
            &mut transaction,
            &report,
            &identity.operator_id,
            request_sha256,
            now,
        )
        .await?;

        let notice = if target == "notice-issued" {
            Some(
                insert_developer_notice(
                    &mut transaction,
                    &report,
                    &identity.operator_id,
                    request_sha256,
                    request_id,
                    key_sha256,
                    now,
                )
                .await?,
            )
        } else {
            None
        };
        let response = ModerationDecisionResponse {
            report: report.clone(),
            notice,
        };
        let body = serde_json::to_value(&response).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.operator_id,
            key_sha256,
            StatusCode::OK,
            &body,
        )
        .await?;
        let action = match target {
            "closed-no-action" => "moderation.report-closed",
            "notice-issued" => "moderation.report-notice-issued",
            _ => "moderation.report-security-escalated",
        };
        append_mutation(
            &mut transaction,
            MutationEvent {
                now,
                actor_id: &identity.operator_id,
                action,
                topic: action,
                object_kind: "content-report",
                object_id: report_id,
                before_state: Some(&before),
                after_state: Some(&report.state),
                resource_version: report.resource_version,
                request_id,
                request_sha256,
                key_sha256,
                payload: json!({
                    "report_id": report.report_id,
                    "release_id": report.release_id,
                    "app_id": report.app_id,
                    "version": report.version,
                    "state": report.state,
                    "disposition": report.disposition,
                    "reason_codes": report.decision_reason_codes,
                    "resolution_due_unix_seconds": report.resolution_due_unix_seconds
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(response)
    }

    async fn list_developer_notices(
        &self,
        token: &str,
        app_id: &str,
        cursor: Option<ModerationCursor>,
        limit: usize,
    ) -> Result<DeveloperNoticeListResponse, ApiError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let identity = authenticate(&mut transaction, &sha256_hex(token.as_bytes()))
            .await
            .map_err(ApiError::from_transaction)?;
        require_developer_write(&identity)?;
        let owns_app: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM apps WHERE app_id = $1 AND owner_team_id = $2)",
        )
        .bind(app_id)
        .bind(&identity.team_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        if !owns_app {
            return Err(ApiError::not_found());
        }
        let (before_created, before_id) = cursor
            .map(|cursor| (cursor.unix_seconds, cursor.object_id))
            .unwrap_or((i64::MAX, "notice_ffffffffffffffffffffffffffffffff".into()));
        let rows = sqlx::query(
            "SELECT notice.notice_id, notice.report_id, notice.release_id, notice.app_id, \
             notice.version, notice.state, notice.reason_codes, \
             notice.appeal_deadline_unix_seconds, notice.resource_version, \
             notice.created_unix_seconds, notice.updated_unix_seconds, \
             appeal.appeal_id, appeal.state AS appeal_state \
             FROM store_developer_notices notice \
             LEFT JOIN store_moderation_appeals appeal ON appeal.notice_id = notice.notice_id \
             WHERE notice.owner_team_id = $1 AND notice.app_id = $2 AND \
             (notice.created_unix_seconds, notice.notice_id) < ($3, $4) \
             ORDER BY notice.created_unix_seconds DESC, notice.notice_id DESC LIMIT $5",
        )
        .bind(&identity.team_id)
        .bind(app_id)
        .bind(before_created)
        .bind(before_id)
        .bind(i64::try_from(limit + 1).map_err(|_| ApiError::invalid_request())?)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| ApiError::unavailable())?;
        let mut items = rows
            .iter()
            .map(developer_notice_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = has_more
            .then(|| items.last())
            .flatten()
            .map(|item| format!("{}:{}", item.created_unix_seconds, item.notice_id));
        transaction
            .commit()
            .await
            .map_err(|_| ApiError::unavailable())?;
        Ok(DeveloperNoticeListResponse { items, next_cursor })
    }

    async fn appeal_notice(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        notice_id: &str,
        expected_version: u64,
        request: &AppealRequest,
    ) -> Result<AppealResponse, ApiError> {
        if !valid_appeal_ground(&request.ground) {
            return Err(ApiError::invalid_request());
        }
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        let expected = expected_version.to_string();
        let request_sha256 = mutation_request_sha256(
            "moderation.notice.appeal.v1",
            &[notice_id, &expected, &request.ground],
        );
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .appeal_notice_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    notice_id,
                    expected_version,
                    &request_sha256,
                    request,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(ApiError::from_transaction(error)),
                Ok(appeal) => return Ok(appeal),
            }
        }
        Err(ApiError::unavailable())
    }

    #[allow(clippy::too_many_arguments)]
    async fn appeal_notice_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        notice_id: &str,
        expected_version: u64,
        request_sha256: &str,
        request: &AppealRequest,
    ) -> Result<AppealResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate(&mut transaction, token_sha256).await?;
        require_developer_write(&identity)?;
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::CREATED.as_u16() as i16 =>
            {
                let appeal = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(appeal);
            }
            IdempotencyReservation::Replay { .. } => return Err(ApiError::internal().into()),
        }
        let notice_row = sqlx::query(
            "SELECT notice_id, report_id, owner_team_id, state, appeal_deadline_unix_seconds, \
             resource_version FROM store_developer_notices WHERE notice_id = $1 FOR UPDATE",
        )
        .bind(notice_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
        if notice_row.get::<String, _>("owner_team_id") != identity.team_id {
            return Err(ApiError::not_found().into());
        }
        if row_version(&notice_row)? != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        if notice_row.get::<String, _>("state") != "open"
            || notice_row.get::<i64, _>("appeal_deadline_unix_seconds") <= now
        {
            return Err(ApiError::invalid_transition().into());
        }
        let appeal = AppealResponse {
            appeal_id: prefixed_uuid("appeal_"),
            notice_id: notice_id.into(),
            ground: request.ground.clone(),
            state: "pending".into(),
            decision_reason_codes: Vec::new(),
            resource_version: 1,
            created_unix_seconds: unix(now)?,
            updated_unix_seconds: unix(now)?,
        };
        sqlx::query(
            "INSERT INTO store_moderation_appeals (appeal_id, notice_id, owner_team_id, \
             ground, state, decision_reason_codes, resource_version, created_unix_seconds, \
             updated_unix_seconds) VALUES ($1, $2, $3, $4, 'pending', '{}', 1, $5, $5)",
        )
        .bind(&appeal.appeal_id)
        .bind(notice_id)
        .bind(&identity.team_id)
        .bind(&appeal.ground)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        insert_appeal_revision(
            &mut transaction,
            &appeal,
            &identity.member_id,
            request_sha256,
            now,
        )
        .await?;
        sqlx::query(
            "UPDATE store_developer_notices SET state = 'appealed', resource_version = 2, \
             updated_unix_seconds = $1 WHERE notice_id = $2",
        )
        .bind(now)
        .bind(notice_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        insert_notice_revision(
            &mut transaction,
            notice_id,
            2,
            "appealed",
            &identity.member_id,
            request_sha256,
            now,
        )
        .await?;
        let body = serde_json::to_value(&appeal).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.member_id,
            key_sha256,
            StatusCode::CREATED,
            &body,
        )
        .await?;
        append_simple_moderation_mutation(
            &mut transaction,
            now,
            &identity.member_id,
            "moderation.notice-appealed",
            "developer-notice",
            notice_id,
            Some("open"),
            "appealed",
            2,
            request_id,
            request_sha256,
            key_sha256,
        )
        .await?;
        append_simple_moderation_mutation(
            &mut transaction,
            now,
            &identity.member_id,
            "moderation.appeal-created",
            "moderation-appeal",
            &appeal.appeal_id,
            None,
            "pending",
            1,
            request_id,
            request_sha256,
            key_sha256,
        )
        .await?;
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(appeal)
    }

    async fn decide_appeal(
        &self,
        token: &str,
        idempotency_key: &str,
        request_id: &str,
        appeal_id: &str,
        expected_version: u64,
        request: &AppealDecisionRequest,
    ) -> Result<AppealResponse, ApiError> {
        validate_appeal_decision(request)?;
        let token_sha256 = sha256_hex(token.as_bytes());
        let key_sha256 = sha256_hex(idempotency_key.as_bytes());
        let expected = expected_version.to_string();
        let reasons = request.reason_codes.join(",");
        let request_sha256 = mutation_request_sha256(
            "moderation.appeal.decide.v1",
            &[appeal_id, &expected, &request.decision, &reasons],
        );
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .decide_appeal_once(
                    &token_sha256,
                    &key_sha256,
                    request_id,
                    appeal_id,
                    expected_version,
                    &request_sha256,
                    request,
                )
                .await
            {
                Err(TxError::Sql(error)) if is_retryable_transaction_error(&error) => {
                    if attempt + 1 == MAX_TRANSACTION_ATTEMPTS {
                        return Err(ApiError::unavailable());
                    }
                    retry_delay(attempt).await;
                }
                Err(error) => return Err(ApiError::from_transaction(error)),
                Ok(appeal) => return Ok(appeal),
            }
        }
        Err(ApiError::unavailable())
    }

    #[allow(clippy::too_many_arguments)]
    async fn decide_appeal_once(
        &self,
        token_sha256: &str,
        key_sha256: &str,
        request_id: &str,
        appeal_id: &str,
        expected_version: u64,
        request_sha256: &str,
        request: &AppealDecisionRequest,
    ) -> Result<AppealResponse, TxError> {
        let mut transaction = begin_serializable(&self.pool).await?;
        let identity = authenticate_store_operator(&mut transaction, token_sha256).await?;
        require_moderation_access(&identity)?;
        let now = database_now(&mut transaction).await?;
        match reserve_idempotency(
            &mut transaction,
            &identity.operator_id,
            key_sha256,
            request_sha256,
            now,
        )
        .await?
        {
            IdempotencyReservation::Fresh => {}
            IdempotencyReservation::Replay { status, body }
                if status == StatusCode::OK.as_u16() as i16 =>
            {
                let appeal = serde_json::from_value(body).map_err(|_| ApiError::internal())?;
                transaction.commit().await.map_err(TxError::Sql)?;
                return Ok(appeal);
            }
            IdempotencyReservation::Replay { .. } => return Err(ApiError::internal().into()),
        }
        let row = sqlx::query(
            "SELECT appeal.appeal_id, appeal.notice_id, appeal.ground, appeal.state, \
             appeal.decision_reason_codes, appeal.resource_version, appeal.created_unix_seconds, \
             appeal.updated_unix_seconds, notice.report_id \
             FROM store_moderation_appeals appeal \
             JOIN store_developer_notices notice ON notice.notice_id = appeal.notice_id \
             WHERE appeal.appeal_id = $1 FOR UPDATE OF appeal, notice",
        )
        .bind(appeal_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
        let mut appeal = appeal_from_row(&row)?;
        if appeal.resource_version != expected_version {
            return Err(ApiError::precondition_failed().into());
        }
        if appeal.state != "pending" {
            return Err(ApiError::invalid_transition().into());
        }
        let notice_id = appeal.notice_id.clone();
        let report_id: String = row.get("report_id");
        appeal.state = request.decision.clone();
        appeal.decision_reason_codes = request.reason_codes.clone();
        appeal.resource_version = 2;
        appeal.updated_unix_seconds = unix(now)?;
        sqlx::query(
            "UPDATE store_moderation_appeals SET state = $1, decision_reason_codes = $2, \
             resource_version = 2, updated_unix_seconds = $3 WHERE appeal_id = $4",
        )
        .bind(&appeal.state)
        .bind(&appeal.decision_reason_codes)
        .bind(now)
        .bind(appeal_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        insert_appeal_revision(
            &mut transaction,
            &appeal,
            &identity.operator_id,
            request_sha256,
            now,
        )
        .await?;
        let notice_state = format!("resolved-{}", appeal.state);
        sqlx::query(
            "UPDATE store_developer_notices SET state = $1, resource_version = 3, \
             updated_unix_seconds = $2 WHERE notice_id = $3 AND state = 'appealed'",
        )
        .bind(&notice_state)
        .bind(now)
        .bind(&notice_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        insert_notice_revision(
            &mut transaction,
            &notice_id,
            3,
            &notice_state,
            &identity.operator_id,
            request_sha256,
            now,
        )
        .await?;
        let mut report = load_content_report(&mut transaction, &report_id, true).await?;
        if report.state != "notice-issued" || report.resource_version != 2 {
            return Err(ApiError::invalid_transition().into());
        }
        report.state = "closed-after-appeal".into();
        report.closed_unix_seconds = Some(unix(now)?);
        report.resource_version = 3;
        report.updated_unix_seconds = unix(now)?;
        sqlx::query(
            "UPDATE store_content_reports SET state = 'closed-after-appeal', \
             closed_unix_seconds = $1, resource_version = 3, updated_unix_seconds = $1 \
             WHERE report_id = $2",
        )
        .bind(now)
        .bind(&report_id)
        .execute(&mut *transaction)
        .await
        .map_err(TxError::Sql)?;
        insert_report_revision(
            &mut transaction,
            &report,
            &identity.operator_id,
            request_sha256,
            now,
        )
        .await?;
        let body = serde_json::to_value(&appeal).map_err(|_| ApiError::internal())?;
        complete_idempotency(
            &mut transaction,
            &identity.operator_id,
            key_sha256,
            StatusCode::OK,
            &body,
        )
        .await?;
        for (action, kind, object_id, before, after, version) in [
            (
                "moderation.appeal-decided",
                "moderation-appeal",
                appeal_id,
                "pending",
                appeal.state.as_str(),
                2,
            ),
            (
                "moderation.notice-resolved",
                "developer-notice",
                notice_id.as_str(),
                "appealed",
                notice_state.as_str(),
                3,
            ),
            (
                "moderation.report-closed-after-appeal",
                "content-report",
                report_id.as_str(),
                "notice-issued",
                "closed-after-appeal",
                3,
            ),
        ] {
            append_simple_moderation_mutation(
                &mut transaction,
                now,
                &identity.operator_id,
                action,
                kind,
                object_id,
                Some(before),
                after,
                version,
                request_id,
                request_sha256,
                key_sha256,
            )
            .await?;
        }
        transaction.commit().await.map_err(TxError::Sql)?;
        Ok(appeal)
    }
}

pub(super) async fn post_content_report(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
    payload: Result<Json<ContentReportRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    let idempotency_key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return json_rejection(rejection).response(request_id),
    };
    match service
        .submit_content_report(&idempotency_key, &request_id, &request)
        .await
    {
        Ok(report) => {
            let version = report.resource_version;
            resource_response(StatusCode::ACCEPTED, report, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

pub(super) async fn list_moderation_reports(
    State(service): State<StoreControlService>,
    headers: HeaderMap,
    query: Result<Query<ModerationQueueQuery>, QueryRejection>,
) -> Response {
    let request_id = request_id();
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return ApiError::invalid_request().response(request_id),
    };
    let limit = usize::from(query.limit.unwrap_or(25));
    if !(1..=50).contains(&limit) {
        return ApiError::invalid_request().response(request_id);
    }
    let cursor = match query.cursor.as_deref().map(parse_cursor).transpose() {
        Ok(cursor) => cursor,
        Err(error) => return error.response(request_id),
    };
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service.list_moderation_reports(&token, cursor, limit).await {
        Ok(queue) => json_response(StatusCode::OK, queue, request_id),
        Err(error) => error.response(request_id),
    }
}

pub(super) async fn decide_content_report(
    State(service): State<StoreControlService>,
    Path(report_action): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<ModerationDecisionRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    let Some(report_id) = report_action.strip_suffix(":decide") else {
        return ApiError::invalid_request().response(request_id);
    };
    if !valid_prefixed_id(report_id, "report_") {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let expected = match expected_version(&headers) {
        Ok(version) => version,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return json_rejection(rejection).response(request_id),
    };
    match service
        .decide_content_report(&token, &key, &request_id, report_id, expected, &request)
        .await
    {
        Ok(response) => {
            let version = response.report.resource_version;
            resource_response(StatusCode::OK, response, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

pub(super) async fn list_developer_notices(
    State(service): State<StoreControlService>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
    query: Result<Query<ModerationQueueQuery>, QueryRejection>,
) -> Response {
    let request_id = request_id();
    if !cp0_manifest::is_valid_app_id(&app_id) {
        return ApiError::invalid_request().response(request_id);
    }
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return ApiError::invalid_request().response(request_id),
    };
    let limit = usize::from(query.limit.unwrap_or(25));
    if !(1..=50).contains(&limit) {
        return ApiError::invalid_request().response(request_id);
    }
    let cursor = match query.cursor.as_deref().map(parse_cursor).transpose() {
        Ok(cursor) => cursor,
        Err(error) => return error.response(request_id),
    };
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    match service
        .list_developer_notices(&token, &app_id, cursor, limit)
        .await
    {
        Ok(notices) => json_response(StatusCode::OK, notices, request_id),
        Err(error) => error.response(request_id),
    }
}

pub(super) async fn appeal_notice(
    State(service): State<StoreControlService>,
    Path(notice_action): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<AppealRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    let Some(notice_id) = notice_action.strip_suffix(":appeal") else {
        return ApiError::invalid_request().response(request_id);
    };
    if !valid_prefixed_id(notice_id, "notice_") {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let expected = match expected_version(&headers) {
        Ok(version) => version,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return json_rejection(rejection).response(request_id),
    };
    match service
        .appeal_notice(&token, &key, &request_id, notice_id, expected, &request)
        .await
    {
        Ok(appeal) => {
            let version = appeal.resource_version;
            resource_response(StatusCode::CREATED, appeal, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

pub(super) async fn decide_appeal(
    State(service): State<StoreControlService>,
    Path(appeal_action): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<AppealDecisionRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id();
    let Some(appeal_id) = appeal_action.strip_suffix(":decide") else {
        return ApiError::invalid_request().response(request_id);
    };
    if !valid_prefixed_id(appeal_id, "appeal_") {
        return ApiError::invalid_request().response(request_id);
    }
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(error) => return error.response(request_id),
    };
    let key = match idempotency_key(&headers) {
        Ok(key) => key,
        Err(error) => return error.response(request_id),
    };
    let expected = match expected_version(&headers) {
        Ok(version) => version,
        Err(error) => return error.response(request_id),
    };
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => return json_rejection(rejection).response(request_id),
    };
    match service
        .decide_appeal(&token, &key, &request_id, appeal_id, expected, &request)
        .await
    {
        Ok(appeal) => {
            let version = appeal.resource_version;
            resource_response(StatusCode::OK, appeal, version, request_id)
        }
        Err(error) => error.response(request_id),
    }
}

fn validate_content_report(request: &ContentReportRequest) -> Result<(), ApiError> {
    if !is_valid_release_id(&request.release_id)
        || !cp0_manifest::is_valid_app_id(&request.app_id)
        || semver::Version::parse(&request.version).is_err()
        || !matches!(
            request.reason_code.as_str(),
            "malware" | "privacy" | "fraud" | "harmful-content" | "age-rating" | "other"
        )
    {
        return Err(ApiError::invalid_request());
    }
    Ok(())
}

fn validate_moderation_decision(request: &ModerationDecisionRequest) -> Result<(), ApiError> {
    if !matches!(
        request.disposition.as_str(),
        "no-action" | "developer-notice" | "security-escalation"
    ) || !valid_decision_reasons(&request.reason_codes)
    {
        return Err(ApiError::invalid_request());
    }
    Ok(())
}

fn validate_appeal_decision(request: &AppealDecisionRequest) -> Result<(), ApiError> {
    if !matches!(request.decision.as_str(), "accepted" | "upheld")
        || !valid_decision_reasons(&request.reason_codes)
    {
        return Err(ApiError::invalid_request());
    }
    Ok(())
}

fn valid_decision_reasons(reasons: &[String]) -> bool {
    (1..=4).contains(&reasons.len())
        && reasons.iter().collect::<BTreeSet<_>>().len() == reasons.len()
        && reasons.iter().all(|reason| {
            matches!(
                reason.as_str(),
                "duplicate"
                    | "insufficient-evidence"
                    | "policy-violation"
                    | "security-review"
                    | "identity-confirmed"
                    | "remediation-accepted"
            )
        })
}

fn valid_appeal_ground(ground: &str) -> bool {
    matches!(
        ground,
        "identity-mismatch" | "policy-misapplied" | "remediated" | "other"
    )
}

fn report_sla(reason: &str) -> (&'static str, i64, i64) {
    if matches!(reason, "malware" | "privacy") {
        (
            "security",
            SECURITY_ACK_SECONDS,
            SECURITY_RESOLUTION_SECONDS,
        )
    } else {
        (
            "standard",
            STANDARD_ACK_SECONDS,
            STANDARD_RESOLUTION_SECONDS,
        )
    }
}

fn require_moderation_access(identity: &StoreOperatorIdentity) -> Result<(), ApiError> {
    if identity.role != "admin" || !identity.has_scope("store.moderation") {
        return Err(ApiError::forbidden());
    }
    if !identity.two_factor_enabled {
        return Err(ApiError::two_factor_required());
    }
    Ok(())
}

fn valid_prefixed_id(value: &str, prefix: &str) -> bool {
    value.len() == prefix.len() + 32
        && value.starts_with(prefix)
        && value[prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_cursor(value: &str) -> Result<ModerationCursor, ApiError> {
    let (seconds, object_id) = value
        .split_once(':')
        .ok_or_else(ApiError::invalid_request)?;
    let unix_seconds = seconds
        .parse::<i64>()
        .map_err(|_| ApiError::invalid_request())?;
    if unix_seconds < 1 || object_id.len() > 64 || object_id.contains(':') {
        return Err(ApiError::invalid_request());
    }
    Ok(ModerationCursor {
        unix_seconds,
        object_id: object_id.into(),
    })
}

fn unix(value: i64) -> Result<u64, ApiError> {
    u64::try_from(value).map_err(|_| ApiError::internal())
}

fn optional_unix(value: Option<i64>) -> Result<Option<u64>, ApiError> {
    value.map(unix).transpose()
}

fn content_report_from_row(row: &sqlx::postgres::PgRow) -> Result<ContentReportResponse, ApiError> {
    Ok(ContentReportResponse {
        report_id: row.get("report_id"),
        release_id: row.get("release_id"),
        app_id: row.get("app_id"),
        version: row.get("version"),
        reason_code: row.get("reason_code"),
        sla_class: row.get("sla_class"),
        state: row.get("state"),
        disposition: row.get("disposition"),
        decision_reason_codes: row.get("decision_reason_codes"),
        acknowledgement_due_unix_seconds: unix(row.get("acknowledgement_due_unix_seconds"))?,
        resolution_due_unix_seconds: unix(row.get("resolution_due_unix_seconds"))?,
        acknowledged_unix_seconds: optional_unix(row.get("acknowledged_unix_seconds"))?,
        closed_unix_seconds: optional_unix(row.get("closed_unix_seconds"))?,
        resource_version: row_version(row)?,
        created_unix_seconds: unix(row.get("created_unix_seconds"))?,
        updated_unix_seconds: unix(row.get("updated_unix_seconds"))?,
    })
}

async fn load_content_report(
    transaction: &mut Transaction<'_, Postgres>,
    report_id: &str,
    lock: bool,
) -> Result<ContentReportResponse, TxError> {
    let lock_clause = if lock { " FOR UPDATE" } else { "" };
    let query = format!(
        "SELECT report_id, release_id, app_id, version, reason_code, sla_class, state, \
         disposition, decision_reason_codes, acknowledgement_due_unix_seconds, \
         resolution_due_unix_seconds, acknowledged_unix_seconds, closed_unix_seconds, \
         resource_version, created_unix_seconds, updated_unix_seconds \
         FROM store_content_reports WHERE report_id = $1{lock_clause}"
    );
    let row = sqlx::query(&query)
        .bind(report_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(TxError::Sql)?
        .ok_or_else(ApiError::not_found)?;
    content_report_from_row(&row).map_err(Into::into)
}

fn developer_notice_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<DeveloperNoticeResponse, ApiError> {
    Ok(DeveloperNoticeResponse {
        notice_id: row.get("notice_id"),
        report_id: row.get("report_id"),
        release_id: row.get("release_id"),
        app_id: row.get("app_id"),
        version: row.get("version"),
        state: row.get("state"),
        reason_codes: row.get("reason_codes"),
        appeal_deadline_unix_seconds: unix(row.get("appeal_deadline_unix_seconds"))?,
        appeal_id: row.get("appeal_id"),
        appeal_state: row.get("appeal_state"),
        resource_version: row_version(row)?,
        created_unix_seconds: unix(row.get("created_unix_seconds"))?,
        updated_unix_seconds: unix(row.get("updated_unix_seconds"))?,
    })
}

fn appeal_from_row(row: &sqlx::postgres::PgRow) -> Result<AppealResponse, ApiError> {
    Ok(AppealResponse {
        appeal_id: row.get("appeal_id"),
        notice_id: row.get("notice_id"),
        ground: row.get("ground"),
        state: row.get("state"),
        decision_reason_codes: row.get("decision_reason_codes"),
        resource_version: row_version(row)?,
        created_unix_seconds: unix(row.get("created_unix_seconds"))?,
        updated_unix_seconds: unix(row.get("updated_unix_seconds"))?,
    })
}

async fn insert_report_revision(
    transaction: &mut Transaction<'_, Postgres>,
    report: &ContentReportResponse,
    actor_id: &str,
    request_sha256: &str,
    now: i64,
) -> Result<(), TxError> {
    sqlx::query(
        "INSERT INTO store_content_report_revisions (report_id, resource_version, actor_id, \
         state, disposition, decision_reason_codes, request_sha256, created_unix_seconds) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(&report.report_id)
    .bind(i64::try_from(report.resource_version).map_err(|_| ApiError::internal())?)
    .bind(actor_id)
    .bind(&report.state)
    .bind(&report.disposition)
    .bind(&report.decision_reason_codes)
    .bind(request_sha256)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    Ok(())
}

async fn insert_notice_revision(
    transaction: &mut Transaction<'_, Postgres>,
    notice_id: &str,
    resource_version: u64,
    state: &str,
    actor_id: &str,
    request_sha256: &str,
    now: i64,
) -> Result<(), TxError> {
    sqlx::query(
        "INSERT INTO store_developer_notice_revisions (notice_id, resource_version, actor_id, \
         state, request_sha256, created_unix_seconds) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(notice_id)
    .bind(i64::try_from(resource_version).map_err(|_| ApiError::internal())?)
    .bind(actor_id)
    .bind(state)
    .bind(request_sha256)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    Ok(())
}

async fn insert_appeal_revision(
    transaction: &mut Transaction<'_, Postgres>,
    appeal: &AppealResponse,
    actor_id: &str,
    request_sha256: &str,
    now: i64,
) -> Result<(), TxError> {
    sqlx::query(
        "INSERT INTO store_moderation_appeal_revisions (appeal_id, resource_version, actor_id, \
         state, decision_reason_codes, request_sha256, created_unix_seconds) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&appeal.appeal_id)
    .bind(i64::try_from(appeal.resource_version).map_err(|_| ApiError::internal())?)
    .bind(actor_id)
    .bind(&appeal.state)
    .bind(&appeal.decision_reason_codes)
    .bind(request_sha256)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_developer_notice(
    transaction: &mut Transaction<'_, Postgres>,
    report: &ContentReportResponse,
    actor_id: &str,
    request_sha256: &str,
    request_id: &str,
    key_sha256: &str,
    now: i64,
) -> Result<DeveloperNoticeResponse, TxError> {
    let owner_team_id: String =
        sqlx::query_scalar("SELECT owner_team_id FROM apps WHERE app_id = $1 FOR SHARE")
            .bind(&report.app_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(TxError::Sql)?;
    let notice = DeveloperNoticeResponse {
        notice_id: prefixed_uuid("notice_"),
        report_id: report.report_id.clone(),
        release_id: report.release_id.clone(),
        app_id: report.app_id.clone(),
        version: report.version.clone(),
        state: "open".into(),
        reason_codes: report.decision_reason_codes.clone(),
        appeal_deadline_unix_seconds: unix(now + APPEAL_WINDOW_SECONDS)?,
        appeal_id: None,
        appeal_state: None,
        resource_version: 1,
        created_unix_seconds: unix(now)?,
        updated_unix_seconds: unix(now)?,
    };
    sqlx::query(
        "INSERT INTO store_developer_notices (notice_id, report_id, owner_team_id, \
         release_id, app_id, version, state, reason_codes, appeal_deadline_unix_seconds, \
         resource_version, created_unix_seconds, updated_unix_seconds) \
         VALUES ($1, $2, $3, $4, $5, $6, 'open', $7, $8, 1, $9, $9)",
    )
    .bind(&notice.notice_id)
    .bind(&notice.report_id)
    .bind(owner_team_id)
    .bind(&notice.release_id)
    .bind(&notice.app_id)
    .bind(&notice.version)
    .bind(&notice.reason_codes)
    .bind(i64::try_from(notice.appeal_deadline_unix_seconds).map_err(|_| ApiError::internal())?)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(TxError::Sql)?;
    insert_notice_revision(
        transaction,
        &notice.notice_id,
        1,
        "open",
        actor_id,
        request_sha256,
        now,
    )
    .await?;
    append_simple_moderation_mutation(
        transaction,
        now,
        actor_id,
        "moderation.notice-issued",
        "developer-notice",
        &notice.notice_id,
        None,
        "open",
        1,
        request_id,
        request_sha256,
        key_sha256,
    )
    .await?;
    Ok(notice)
}

#[allow(clippy::too_many_arguments)]
async fn append_simple_moderation_mutation(
    transaction: &mut Transaction<'_, Postgres>,
    now: i64,
    actor_id: &str,
    action: &str,
    object_kind: &str,
    object_id: &str,
    before_state: Option<&str>,
    after_state: &str,
    resource_version: u64,
    request_id: &str,
    request_sha256: &str,
    key_sha256: &str,
) -> Result<(), TxError> {
    append_mutation(
        transaction,
        MutationEvent {
            now,
            actor_id,
            action,
            topic: action,
            object_kind,
            object_id,
            before_state,
            after_state: Some(after_state),
            resource_version,
            request_id,
            request_sha256,
            key_sha256,
            payload: json!({
                "object_id": object_id,
                "state": after_state,
                "resource_version": resource_version
            }),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sla_and_closed_vocabularies_are_fixed() {
        assert_eq!(report_sla("malware"), ("security", 14_400, 259_200));
        assert_eq!(report_sla("other"), ("standard", 259_200, 1_209_600));
        assert!(valid_decision_reasons(&["policy-violation".into()]));
        assert!(!valid_decision_reasons(&[
            "policy-violation".into(),
            "policy-violation".into()
        ]));
        assert!(!valid_decision_reasons(&["free-form".into()]));
    }

    #[test]
    fn report_contract_rejects_extra_identity_fields() {
        assert!(
            serde_json::from_value::<ContentReportRequest>(json!({
                "release_id": "rel_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "app_id": "dev.cardputerzero.example",
                "version": "1.0.0",
                "reason_code": "privacy",
                "device_id": "forbidden"
            }))
            .is_err()
        );
    }
}
