CREATE FUNCTION moderation_reason_codes_are_valid(
    reason_codes_value TEXT[],
    minimum_count INTEGER
) RETURNS BOOLEAN LANGUAGE sql IMMUTABLE AS $$
    SELECT cardinality(reason_codes_value) BETWEEN minimum_count AND 4
       AND reason_codes_value <@ ARRAY[
           'duplicate', 'insufficient-evidence', 'policy-violation',
           'security-review', 'identity-confirmed', 'remediation-accepted'
       ]::TEXT[]
       AND cardinality(reason_codes_value) = (
           SELECT COUNT(DISTINCT reason) FROM unnest(reason_codes_value) reason
       );
$$;

ALTER TABLE store_content_reports
    ADD CONSTRAINT store_content_reports_semantic_version CHECK (
        version ~ '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)([-+][0-9A-Za-z.-]+)*$'
    ),
    ADD CONSTRAINT store_content_reports_decision_vocabulary CHECK (
        moderation_reason_codes_are_valid(decision_reason_codes, 0)
    );
ALTER TABLE store_developer_notices
    ADD CONSTRAINT store_developer_notices_semantic_version CHECK (
        version ~ '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)([-+][0-9A-Za-z.-]+)*$'
    ),
    ADD CONSTRAINT store_developer_notices_reason_vocabulary CHECK (
        moderation_reason_codes_are_valid(reason_codes, 1)
    );
ALTER TABLE store_moderation_appeals
    ADD CONSTRAINT store_moderation_appeals_reason_vocabulary CHECK (
        moderation_reason_codes_are_valid(
            decision_reason_codes,
            CASE WHEN state = 'pending' THEN 0 ELSE 1 END
        )
    );

ALTER TABLE store_content_report_revisions
    ADD CONSTRAINT store_content_report_revisions_state CHECK (state IN (
        'submitted', 'closed-no-action', 'notice-issued', 'security-escalated',
        'closed-after-appeal'
    )),
    ADD CONSTRAINT store_content_report_revisions_disposition CHECK (
        disposition IS NULL OR disposition IN (
            'no-action', 'developer-notice', 'security-escalation'
        )
    ),
    ADD CONSTRAINT store_content_report_revisions_reason_vocabulary CHECK (
        moderation_reason_codes_are_valid(
            decision_reason_codes,
            CASE WHEN state = 'submitted' THEN 0 ELSE 1 END
        )
    );
ALTER TABLE store_developer_notice_revisions
    ADD CONSTRAINT store_developer_notice_revisions_state CHECK (state IN (
        'open', 'appealed', 'resolved-accepted', 'resolved-upheld'
    ));
ALTER TABLE store_moderation_appeal_revisions
    ADD CONSTRAINT store_moderation_appeal_revisions_state CHECK (
        state IN ('pending', 'accepted', 'upheld')
    ),
    ADD CONSTRAINT store_moderation_appeal_revisions_reason_vocabulary CHECK (
        moderation_reason_codes_are_valid(
            decision_reason_codes,
            CASE WHEN state = 'pending' THEN 0 ELSE 1 END
        )
    );

CREATE OR REPLACE FUNCTION protect_store_moderation_revision() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    matching BOOLEAN;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'Store moderation revisions are append-only' USING ERRCODE = '55000';
    END IF;
    IF TG_TABLE_NAME = 'store_content_report_revisions' THEN
        SELECT EXISTS (
            SELECT 1 FROM store_content_reports report
            WHERE report.report_id = NEW.report_id
              AND report.resource_version = NEW.resource_version
              AND report.state = NEW.state
              AND report.disposition IS NOT DISTINCT FROM NEW.disposition
              AND report.decision_reason_codes = NEW.decision_reason_codes
        ) INTO matching;
    ELSIF TG_TABLE_NAME = 'store_developer_notice_revisions' THEN
        SELECT EXISTS (
            SELECT 1 FROM store_developer_notices notice
            WHERE notice.notice_id = NEW.notice_id
              AND notice.resource_version = NEW.resource_version
              AND notice.state = NEW.state
        ) INTO matching;
    ELSE
        SELECT EXISTS (
            SELECT 1 FROM store_moderation_appeals appeal
            WHERE appeal.appeal_id = NEW.appeal_id
              AND appeal.resource_version = NEW.resource_version
              AND appeal.state = NEW.state
              AND appeal.decision_reason_codes = NEW.decision_reason_codes
        ) INTO matching;
    END IF;
    IF NOT matching THEN
        RAISE EXCEPTION 'Store moderation revision does not match current state'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER store_content_report_revisions_append_only ON store_content_report_revisions;
CREATE TRIGGER store_content_report_revisions_append_only
    BEFORE INSERT OR UPDATE OR DELETE ON store_content_report_revisions
    FOR EACH ROW EXECUTE FUNCTION protect_store_moderation_revision();
DROP TRIGGER store_developer_notice_revisions_append_only ON store_developer_notice_revisions;
CREATE TRIGGER store_developer_notice_revisions_append_only
    BEFORE INSERT OR UPDATE OR DELETE ON store_developer_notice_revisions
    FOR EACH ROW EXECUTE FUNCTION protect_store_moderation_revision();
DROP TRIGGER store_moderation_appeal_revisions_append_only ON store_moderation_appeal_revisions;
CREATE TRIGGER store_moderation_appeal_revisions_append_only
    BEFORE INSERT OR UPDATE OR DELETE ON store_moderation_appeal_revisions
    FOR EACH ROW EXECUTE FUNCTION protect_store_moderation_revision();

CREATE OR REPLACE FUNCTION protect_store_content_report() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'submitted' OR NEW.resource_version <> 1 OR NOT EXISTS (
            SELECT 1 FROM releases release
            JOIN submissions submission ON submission.submission_id = release.submission_id
            JOIN store_package_artifacts artifact ON artifact.release_id = release.release_id
            WHERE release.release_id = NEW.release_id
              AND release.app_id = NEW.app_id
              AND release.version = NEW.version
              AND release.state = 'published'
              AND submission.state = 'approved'
              AND submission.app_id = release.app_id
              AND submission.version = release.version
              AND artifact.catalog_app->>'app_id' = release.app_id
              AND artifact.catalog_app->>'version' = release.version
        ) THEN
            RAISE EXCEPTION 'Invalid initial Store content report'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' OR
       (NEW.report_id, NEW.release_id, NEW.app_id, NEW.version, NEW.reason_code,
        NEW.sla_class, NEW.acknowledgement_due_unix_seconds,
        NEW.resolution_due_unix_seconds, NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.report_id, OLD.release_id, OLD.app_id, OLD.version, OLD.reason_code,
        OLD.sla_class, OLD.acknowledgement_due_unix_seconds,
        OLD.resolution_due_unix_seconds, OLD.created_unix_seconds) OR
       NEW.resource_version <> OLD.resource_version + 1 OR
       NOT ((OLD.state = 'submitted' AND NEW.state IN
             ('closed-no-action', 'notice-issued', 'security-escalated')) OR
            (OLD.state = 'notice-issued' AND NEW.state = 'closed-after-appeal')) THEN
        RAISE EXCEPTION 'Invalid Store content report transition' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER store_content_reports_state_machine ON store_content_reports;
CREATE TRIGGER store_content_reports_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON store_content_reports
    FOR EACH ROW EXECUTE FUNCTION protect_store_content_report();

CREATE OR REPLACE FUNCTION protect_store_developer_notice() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'open' OR NEW.resource_version <> 1 OR NOT EXISTS (
            SELECT 1 FROM store_content_reports report
            JOIN apps app ON app.app_id = report.app_id
            WHERE report.report_id = NEW.report_id
              AND report.state = 'notice-issued'
              AND report.release_id = NEW.release_id
              AND report.app_id = NEW.app_id
              AND report.version = NEW.version
              AND app.owner_team_id = NEW.owner_team_id
        ) THEN
            RAISE EXCEPTION 'Invalid initial Store developer notice'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' OR
       (NEW.notice_id, NEW.report_id, NEW.owner_team_id, NEW.release_id, NEW.app_id,
        NEW.version, NEW.reason_codes, NEW.appeal_deadline_unix_seconds,
        NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.notice_id, OLD.report_id, OLD.owner_team_id, OLD.release_id, OLD.app_id,
        OLD.version, OLD.reason_codes, OLD.appeal_deadline_unix_seconds,
        OLD.created_unix_seconds) OR
       NEW.resource_version <> OLD.resource_version + 1 OR
       NOT ((OLD.state = 'open' AND NEW.state = 'appealed') OR
            (OLD.state = 'appealed' AND NEW.state IN
             ('resolved-accepted', 'resolved-upheld'))) THEN
        RAISE EXCEPTION 'Invalid Store developer notice transition' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER store_developer_notices_state_machine ON store_developer_notices;
CREATE TRIGGER store_developer_notices_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON store_developer_notices
    FOR EACH ROW EXECUTE FUNCTION protect_store_developer_notice();

CREATE OR REPLACE FUNCTION protect_store_moderation_appeal() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'pending' OR NEW.resource_version <> 1 OR NOT EXISTS (
            SELECT 1 FROM store_developer_notices notice
            WHERE notice.notice_id = NEW.notice_id
              AND notice.owner_team_id = NEW.owner_team_id
              AND notice.state = 'open'
              AND NEW.created_unix_seconds <= notice.appeal_deadline_unix_seconds
        ) THEN
            RAISE EXCEPTION 'Invalid initial Store moderation appeal'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' OR
       (NEW.appeal_id, NEW.notice_id, NEW.owner_team_id, NEW.ground,
        NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.appeal_id, OLD.notice_id, OLD.owner_team_id, OLD.ground,
        OLD.created_unix_seconds) OR
       NEW.resource_version <> OLD.resource_version + 1 OR
       NOT (OLD.state = 'pending' AND NEW.state IN ('accepted', 'upheld')) THEN
        RAISE EXCEPTION 'Invalid Store moderation appeal transition' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER store_moderation_appeals_state_machine ON store_moderation_appeals;
CREATE TRIGGER store_moderation_appeals_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON store_moderation_appeals
    FOR EACH ROW EXECUTE FUNCTION protect_store_moderation_appeal();

CREATE OR REPLACE FUNCTION require_store_moderation_revision() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    revision_actor_id TEXT;
    revision_request_sha256 TEXT;
    revision_created_unix_seconds BIGINT;
    matching BOOLEAN;
    object_kind_value TEXT;
    object_id_value TEXT;
BEGIN
    IF TG_TABLE_NAME = 'store_content_reports' THEN
        SELECT revision.actor_id, revision.request_sha256, revision.created_unix_seconds
          INTO revision_actor_id, revision_request_sha256, revision_created_unix_seconds
          FROM store_content_report_revisions revision
         WHERE revision.report_id = NEW.report_id
           AND revision.resource_version = NEW.resource_version
           AND revision.state = NEW.state
           AND revision.disposition IS NOT DISTINCT FROM NEW.disposition
           AND revision.decision_reason_codes = NEW.decision_reason_codes;
        object_kind_value := 'content-report';
        object_id_value := NEW.report_id;
    ELSIF TG_TABLE_NAME = 'store_developer_notices' THEN
        SELECT revision.actor_id, revision.request_sha256, revision.created_unix_seconds
          INTO revision_actor_id, revision_request_sha256, revision_created_unix_seconds
          FROM store_developer_notice_revisions revision
         WHERE revision.notice_id = NEW.notice_id
           AND revision.resource_version = NEW.resource_version
           AND revision.state = NEW.state;
        object_kind_value := 'developer-notice';
        object_id_value := NEW.notice_id;
    ELSE
        SELECT revision.actor_id, revision.request_sha256, revision.created_unix_seconds
          INTO revision_actor_id, revision_request_sha256, revision_created_unix_seconds
          FROM store_moderation_appeal_revisions revision
         WHERE revision.appeal_id = NEW.appeal_id
           AND revision.resource_version = NEW.resource_version
           AND revision.state = NEW.state
           AND revision.decision_reason_codes = NEW.decision_reason_codes;
        object_kind_value := 'moderation-appeal';
        object_id_value := NEW.appeal_id;
    END IF;
    IF revision_actor_id IS NULL THEN
        RAISE EXCEPTION 'Store moderation mutation requires an immutable revision'
            USING ERRCODE = '55000';
    END IF;
    SELECT EXISTS (
        SELECT 1 FROM audit_events audit
        WHERE audit.actor_id = revision_actor_id
          AND audit.object_kind = object_kind_value
          AND audit.object_id = object_id_value
          AND audit.after_state = NEW.state
          AND audit.resource_version = NEW.resource_version
          AND audit.request_sha256 = revision_request_sha256
          AND audit.occurred_unix_seconds = revision_created_unix_seconds
    ) AND EXISTS (
        SELECT 1 FROM outbox_events event
        WHERE event.aggregate_kind = object_kind_value
          AND event.aggregate_id = object_id_value
          AND event.aggregate_version = NEW.resource_version
          AND event.request_sha256 = revision_request_sha256
          AND event.created_unix_seconds = revision_created_unix_seconds
    ) INTO matching;
    IF NOT matching THEN
        RAISE EXCEPTION 'Store moderation mutation requires matching audit and outbox records'
            USING ERRCODE = '55000';
    END IF;
    RETURN NULL;
END;
$$;
