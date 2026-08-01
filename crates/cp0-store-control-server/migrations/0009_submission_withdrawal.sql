ALTER TABLE submission_scan_jobs
    DROP CONSTRAINT submission_scan_jobs_state_check;
ALTER TABLE submission_scan_jobs
    ADD CONSTRAINT submission_scan_jobs_state_check
    CHECK (state IN ('queued', 'running', 'completed', 'failed', 'cancelled'));

ALTER TABLE submission_scan_jobs
    DROP CONSTRAINT submission_scan_jobs_check;
ALTER TABLE submission_scan_jobs
    ADD CONSTRAINT submission_scan_jobs_lifecycle_check CHECK (
        (state = 'running' AND lease_token IS NOT NULL AND leased_until_unix_seconds IS NOT NULL AND
         completed_unix_seconds IS NULL) OR
        (state = 'queued' AND lease_token IS NULL AND leased_until_unix_seconds IS NULL AND
         completed_unix_seconds IS NULL) OR
        (state IN ('completed', 'failed', 'cancelled') AND lease_token IS NULL AND
         leased_until_unix_seconds IS NULL AND completed_unix_seconds IS NOT NULL)
    );

CREATE OR REPLACE FUNCTION protect_submission_scan_job() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Submission scan jobs cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.event_id, NEW.submission_id, NEW.source_resource_version,
        NEW.source_content_sha256, NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.event_id, OLD.submission_id, OLD.source_resource_version,
        OLD.source_content_sha256, OLD.created_unix_seconds) OR
       NEW.attempts < OLD.attempts OR
       OLD.state IN ('completed', 'failed', 'cancelled') OR
       NOT (
           (OLD.state = 'queued' AND NEW.state IN ('running', 'cancelled')) OR
           (OLD.state = 'running' AND NEW.state IN ('queued', 'completed', 'failed', 'cancelled'))
       ) THEN
        RAISE EXCEPTION 'Submission scan job transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION protect_submission_state_transition() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.resource_version <> OLD.resource_version + 1 OR NOT (
        (OLD.state = 'draft' AND NEW.state IN ('uploading', 'withdrawn')) OR
        (OLD.state = 'uploading' AND NEW.state IN ('uploading', 'processing', 'withdrawn')) OR
        (OLD.state = 'processing' AND
         NEW.state IN ('ready-for-review', 'needs-changes', 'rejected', 'withdrawn')) OR
        (OLD.state = 'ready-for-review' AND NEW.state IN ('in-review', 'withdrawn')) OR
        (OLD.state = 'in-review' AND
         NEW.state IN ('needs-changes', 'approved', 'rejected', 'withdrawn'))
    ) THEN
        RAISE EXCEPTION 'Submission state transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER submissions_state_machine
    BEFORE UPDATE ON submissions
    FOR EACH ROW EXECUTE FUNCTION protect_submission_state_transition();
