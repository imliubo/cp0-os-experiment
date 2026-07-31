CREATE TABLE submission_scan_jobs (
    event_id TEXT PRIMARY KEY REFERENCES outbox_events(event_id),
    submission_id TEXT NOT NULL UNIQUE REFERENCES submissions(submission_id),
    source_resource_version BIGINT NOT NULL CHECK (source_resource_version >= 1),
    source_content_sha256 CHAR(64) NOT NULL CHECK (source_content_sha256 ~ '^[0-9a-f]{64}$'),
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'completed', 'failed')),
    lease_token TEXT CHECK (lease_token IS NULL OR lease_token ~ '^lease_[0-9a-f]{32}$'),
    leased_until_unix_seconds BIGINT,
    attempts SMALLINT NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 8),
    last_error_code TEXT CHECK (
        last_error_code IS NULL OR last_error_code ~ '^[a-z][a-z0-9.-]{0,63}$'
    ),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    completed_unix_seconds BIGINT,
    CHECK (
        (state = 'running' AND lease_token IS NOT NULL AND leased_until_unix_seconds IS NOT NULL AND
         completed_unix_seconds IS NULL) OR
        (state = 'queued' AND lease_token IS NULL AND leased_until_unix_seconds IS NULL AND
         completed_unix_seconds IS NULL) OR
        (state IN ('completed', 'failed') AND lease_token IS NULL AND
         leased_until_unix_seconds IS NULL AND completed_unix_seconds IS NOT NULL)
    )
);

CREATE TABLE submission_scan_results (
    scan_id TEXT PRIMARY KEY CHECK (scan_id ~ '^scan_[0-9a-f]{32}$'),
    event_id TEXT NOT NULL UNIQUE REFERENCES submission_scan_jobs(event_id),
    submission_id TEXT NOT NULL UNIQUE REFERENCES submissions(submission_id),
    source_resource_version BIGINT NOT NULL CHECK (source_resource_version >= 1),
    source_content_sha256 CHAR(64) NOT NULL CHECK (source_content_sha256 ~ '^[0-9a-f]{64}$'),
    outcome TEXT NOT NULL CHECK (outcome IN ('ready-for-review', 'needs-changes', 'rejected')),
    scanner_version TEXT NOT NULL CHECK (char_length(scanner_version) BETWEEN 1 AND 64),
    report JSONB NOT NULL CHECK (jsonb_typeof(report) = 'object' AND pg_column_size(report) <= 32768),
    report_sha256 CHAR(64) NOT NULL CHECK (report_sha256 ~ '^[0-9a-f]{64}$'),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1)
);

CREATE INDEX submission_scan_jobs_claim_idx
    ON submission_scan_jobs (created_unix_seconds, event_id)
    WHERE state IN ('queued', 'running');

CREATE FUNCTION protect_submission_scan_job() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Submission scan jobs cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.event_id, NEW.submission_id, NEW.source_resource_version,
        NEW.source_content_sha256, NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.event_id, OLD.submission_id, OLD.source_resource_version,
        OLD.source_content_sha256, OLD.created_unix_seconds) OR
       NEW.attempts < OLD.attempts OR
       OLD.state IN ('completed', 'failed') OR
       NOT (
           (OLD.state = 'queued' AND NEW.state = 'running') OR
           (OLD.state = 'running' AND NEW.state IN ('queued', 'completed', 'failed'))
       ) THEN
        RAISE EXCEPTION 'Submission scan job transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER submission_scan_jobs_state_machine
    BEFORE UPDATE OR DELETE ON submission_scan_jobs
    FOR EACH ROW EXECUTE FUNCTION protect_submission_scan_job();

CREATE TRIGGER submission_scan_results_append_only
    BEFORE UPDATE OR DELETE ON submission_scan_results
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();

CREATE FUNCTION protect_developer_key() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Developer keys cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.key_id, NEW.team_id, NEW.name, NEW.algorithm, NEW.public_key,
        NEW.fingerprint_sha256, NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.key_id, OLD.team_id, OLD.name, OLD.algorithm, OLD.public_key,
        OLD.fingerprint_sha256, OLD.created_unix_seconds) OR
       OLD.state = 'revoked' OR
       NOT (
           (OLD.state = 'active' AND NEW.state = 'active' AND
            NEW.revoked_unix_seconds IS NULL) OR
           (OLD.state = 'active' AND NEW.state = 'revoked' AND
            NEW.revoked_unix_seconds IS NOT NULL)
       ) THEN
        RAISE EXCEPTION 'Developer key transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER developer_keys_stable_identity
    BEFORE UPDATE OR DELETE ON developer_keys
    FOR EACH ROW EXECUTE FUNCTION protect_developer_key();
