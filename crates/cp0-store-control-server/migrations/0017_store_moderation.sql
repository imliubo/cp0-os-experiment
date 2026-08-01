ALTER TABLE store_operator_access_tokens
    DROP CONSTRAINT store_operator_access_tokens_scopes_check;
ALTER TABLE store_operator_access_tokens
    ADD CONSTRAINT store_operator_access_tokens_scopes_check CHECK (
        scopes IN (
            ARRAY['store.editorial']::TEXT[],
            ARRAY['store.moderation']::TEXT[]
        )
    );

CREATE TABLE store_content_reports (
    report_id TEXT PRIMARY KEY CHECK (report_id ~ '^report_[0-9a-f]{32}$'),
    release_id TEXT NOT NULL REFERENCES releases(release_id),
    app_id TEXT NOT NULL REFERENCES apps(app_id),
    version TEXT NOT NULL CHECK (char_length(version) BETWEEN 5 AND 64),
    reason_code TEXT NOT NULL CHECK (reason_code IN (
        'malware', 'privacy', 'fraud', 'harmful-content', 'age-rating', 'other'
    )),
    sla_class TEXT NOT NULL CHECK (sla_class IN ('security', 'standard')),
    state TEXT NOT NULL CHECK (state IN (
        'submitted', 'closed-no-action', 'notice-issued', 'security-escalated',
        'closed-after-appeal'
    )),
    disposition TEXT CHECK (disposition IN (
        'no-action', 'developer-notice', 'security-escalation'
    )),
    decision_reason_codes TEXT[] NOT NULL DEFAULT '{}' CHECK (
        cardinality(decision_reason_codes) BETWEEN 0 AND 4 AND
        decision_reason_codes <@ ARRAY[
            'duplicate', 'insufficient-evidence', 'policy-violation',
            'security-review', 'identity-confirmed', 'remediation-accepted'
        ]::TEXT[]
    ),
    acknowledgement_due_unix_seconds BIGINT NOT NULL,
    resolution_due_unix_seconds BIGINT NOT NULL,
    acknowledged_unix_seconds BIGINT,
    closed_unix_seconds BIGINT,
    resource_version BIGINT NOT NULL CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    updated_unix_seconds BIGINT NOT NULL,
    CHECK (acknowledgement_due_unix_seconds > created_unix_seconds),
    CHECK (resolution_due_unix_seconds > acknowledgement_due_unix_seconds),
    CHECK (updated_unix_seconds >= created_unix_seconds),
    CHECK (
        (sla_class = 'security' AND
         acknowledgement_due_unix_seconds = created_unix_seconds + 14400 AND
         resolution_due_unix_seconds = created_unix_seconds + 259200) OR
        (sla_class = 'standard' AND
         acknowledgement_due_unix_seconds = created_unix_seconds + 259200 AND
         resolution_due_unix_seconds = created_unix_seconds + 1209600)
    ),
    CHECK (
        (state = 'submitted' AND disposition IS NULL AND
         cardinality(decision_reason_codes) = 0 AND
         acknowledged_unix_seconds IS NULL AND closed_unix_seconds IS NULL) OR
        (state = 'closed-no-action' AND disposition = 'no-action' AND
         cardinality(decision_reason_codes) BETWEEN 1 AND 4 AND
         acknowledged_unix_seconds IS NOT NULL AND closed_unix_seconds IS NOT NULL) OR
        (state = 'notice-issued' AND disposition = 'developer-notice' AND
         cardinality(decision_reason_codes) BETWEEN 1 AND 4 AND
         acknowledged_unix_seconds IS NOT NULL AND closed_unix_seconds IS NULL) OR
        (state = 'security-escalated' AND disposition = 'security-escalation' AND
         cardinality(decision_reason_codes) BETWEEN 1 AND 4 AND
         acknowledged_unix_seconds IS NOT NULL AND closed_unix_seconds IS NULL) OR
        (state = 'closed-after-appeal' AND disposition = 'developer-notice' AND
         cardinality(decision_reason_codes) BETWEEN 1 AND 4 AND
         acknowledged_unix_seconds IS NOT NULL AND closed_unix_seconds IS NOT NULL)
    )
);

CREATE INDEX store_content_reports_sla_queue
    ON store_content_reports (acknowledgement_due_unix_seconds, report_id)
    WHERE state = 'submitted';

CREATE TABLE store_content_report_revisions (
    report_id TEXT NOT NULL REFERENCES store_content_reports(report_id),
    resource_version BIGINT NOT NULL CHECK (resource_version >= 1),
    actor_id TEXT NOT NULL,
    state TEXT NOT NULL,
    disposition TEXT,
    decision_reason_codes TEXT[] NOT NULL,
    request_sha256 CHAR(64) NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    PRIMARY KEY (report_id, resource_version)
);

CREATE TABLE store_developer_notices (
    notice_id TEXT PRIMARY KEY CHECK (notice_id ~ '^notice_[0-9a-f]{32}$'),
    report_id TEXT NOT NULL UNIQUE REFERENCES store_content_reports(report_id),
    owner_team_id TEXT NOT NULL REFERENCES teams(team_id),
    release_id TEXT NOT NULL REFERENCES releases(release_id),
    app_id TEXT NOT NULL REFERENCES apps(app_id),
    version TEXT NOT NULL CHECK (char_length(version) BETWEEN 5 AND 64),
    state TEXT NOT NULL CHECK (state IN (
        'open', 'appealed', 'resolved-accepted', 'resolved-upheld'
    )),
    reason_codes TEXT[] NOT NULL CHECK (cardinality(reason_codes) BETWEEN 1 AND 4),
    appeal_deadline_unix_seconds BIGINT NOT NULL,
    resource_version BIGINT NOT NULL CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    updated_unix_seconds BIGINT NOT NULL,
    CHECK (appeal_deadline_unix_seconds = created_unix_seconds + 1209600),
    CHECK (updated_unix_seconds >= created_unix_seconds)
);

CREATE INDEX store_developer_notices_team_app
    ON store_developer_notices (owner_team_id, app_id, created_unix_seconds DESC, notice_id);

CREATE TABLE store_developer_notice_revisions (
    notice_id TEXT NOT NULL REFERENCES store_developer_notices(notice_id),
    resource_version BIGINT NOT NULL CHECK (resource_version >= 1),
    actor_id TEXT NOT NULL,
    state TEXT NOT NULL,
    request_sha256 CHAR(64) NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    PRIMARY KEY (notice_id, resource_version)
);

CREATE TABLE store_moderation_appeals (
    appeal_id TEXT PRIMARY KEY CHECK (appeal_id ~ '^appeal_[0-9a-f]{32}$'),
    notice_id TEXT NOT NULL UNIQUE REFERENCES store_developer_notices(notice_id),
    owner_team_id TEXT NOT NULL REFERENCES teams(team_id),
    ground TEXT NOT NULL CHECK (ground IN (
        'identity-mismatch', 'policy-misapplied', 'remediated', 'other'
    )),
    state TEXT NOT NULL CHECK (state IN ('pending', 'accepted', 'upheld')),
    decision_reason_codes TEXT[] NOT NULL DEFAULT '{}' CHECK (
        cardinality(decision_reason_codes) BETWEEN 0 AND 4
    ),
    resource_version BIGINT NOT NULL CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    updated_unix_seconds BIGINT NOT NULL,
    CHECK (updated_unix_seconds >= created_unix_seconds),
    CHECK ((state = 'pending' AND cardinality(decision_reason_codes) = 0) OR
           (state IN ('accepted', 'upheld') AND
            cardinality(decision_reason_codes) BETWEEN 1 AND 4))
);

CREATE TABLE store_moderation_appeal_revisions (
    appeal_id TEXT NOT NULL REFERENCES store_moderation_appeals(appeal_id),
    resource_version BIGINT NOT NULL CHECK (resource_version >= 1),
    actor_id TEXT NOT NULL,
    state TEXT NOT NULL,
    decision_reason_codes TEXT[] NOT NULL,
    request_sha256 CHAR(64) NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    PRIMARY KEY (appeal_id, resource_version)
);

CREATE FUNCTION protect_store_moderation_revision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Store moderation revisions are append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER store_content_report_revisions_append_only
    BEFORE UPDATE OR DELETE ON store_content_report_revisions
    FOR EACH ROW EXECUTE FUNCTION protect_store_moderation_revision();
CREATE TRIGGER store_developer_notice_revisions_append_only
    BEFORE UPDATE OR DELETE ON store_developer_notice_revisions
    FOR EACH ROW EXECUTE FUNCTION protect_store_moderation_revision();
CREATE TRIGGER store_moderation_appeal_revisions_append_only
    BEFORE UPDATE OR DELETE ON store_moderation_appeal_revisions
    FOR EACH ROW EXECUTE FUNCTION protect_store_moderation_revision();

CREATE FUNCTION protect_store_content_report() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
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

CREATE TRIGGER store_content_reports_state_machine
    BEFORE UPDATE OR DELETE ON store_content_reports
    FOR EACH ROW EXECUTE FUNCTION protect_store_content_report();

CREATE FUNCTION protect_store_developer_notice() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
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

CREATE TRIGGER store_developer_notices_state_machine
    BEFORE UPDATE OR DELETE ON store_developer_notices
    FOR EACH ROW EXECUTE FUNCTION protect_store_developer_notice();

CREATE FUNCTION protect_store_moderation_appeal() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
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

CREATE TRIGGER store_moderation_appeals_state_machine
    BEFORE UPDATE OR DELETE ON store_moderation_appeals
    FOR EACH ROW EXECUTE FUNCTION protect_store_moderation_appeal();

CREATE FUNCTION require_store_moderation_revision() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    matching BOOLEAN;
BEGIN
    IF TG_TABLE_NAME = 'store_content_reports' THEN
        SELECT EXISTS (
            SELECT 1 FROM store_content_report_revisions revision
            WHERE revision.report_id = NEW.report_id
              AND revision.resource_version = NEW.resource_version
              AND revision.state = NEW.state
              AND revision.disposition IS NOT DISTINCT FROM NEW.disposition
              AND revision.decision_reason_codes = NEW.decision_reason_codes
        ) INTO matching;
    ELSIF TG_TABLE_NAME = 'store_developer_notices' THEN
        SELECT EXISTS (
            SELECT 1 FROM store_developer_notice_revisions revision
            WHERE revision.notice_id = NEW.notice_id
              AND revision.resource_version = NEW.resource_version
              AND revision.state = NEW.state
        ) INTO matching;
    ELSE
        SELECT EXISTS (
            SELECT 1 FROM store_moderation_appeal_revisions revision
            WHERE revision.appeal_id = NEW.appeal_id
              AND revision.resource_version = NEW.resource_version
              AND revision.state = NEW.state
              AND revision.decision_reason_codes = NEW.decision_reason_codes
        ) INTO matching;
    END IF;
    IF NOT matching THEN
        RAISE EXCEPTION 'Store moderation mutation requires an immutable revision'
            USING ERRCODE = '55000';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER store_content_reports_require_revision
    AFTER INSERT OR UPDATE ON store_content_reports
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_store_moderation_revision();
CREATE CONSTRAINT TRIGGER store_developer_notices_require_revision
    AFTER INSERT OR UPDATE ON store_developer_notices
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_store_moderation_revision();
CREATE CONSTRAINT TRIGGER store_moderation_appeals_require_revision
    AFTER INSERT OR UPDATE ON store_moderation_appeals
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_store_moderation_revision();
