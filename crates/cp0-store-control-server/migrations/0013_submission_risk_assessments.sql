CREATE FUNCTION valid_risk_reason_codes(value JSONB) RETURNS BOOLEAN
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
    reason TEXT;
    previous TEXT := NULL;
    count SMALLINT := 0;
BEGIN
    IF jsonb_typeof(value) <> 'array' OR jsonb_array_length(value) > 7 THEN
        RETURN FALSE;
    END IF;
    FOR reason IN SELECT jsonb_array_elements_text(value)
    LOOP
        count := count + 1;
        IF reason NOT IN (
            'camera-capture', 'hardware-control', 'microphone-capture',
            'multiple-sensitive-capabilities', 'network-access', 'radio-transmit',
            'user-documents'
        ) OR (previous IS NOT NULL AND reason <= previous) THEN
            RETURN FALSE;
        END IF;
        previous := reason;
    END LOOP;
    RETURN count = jsonb_array_length(value);
EXCEPTION WHEN OTHERS THEN
    RETURN FALSE;
END;
$$;

CREATE TABLE submission_risk_assessments (
    assessment_id TEXT PRIMARY KEY CHECK (assessment_id ~ '^risk_[0-9a-f]{32}$'),
    scan_id TEXT NOT NULL REFERENCES submission_scan_results(scan_id),
    submission_id TEXT NOT NULL REFERENCES submissions(submission_id),
    source_report_sha256 CHAR(64) NOT NULL CHECK (source_report_sha256 ~ '^[0-9a-f]{64}$'),
    policy_version SMALLINT NOT NULL CHECK (policy_version BETWEEN 1 AND 32767),
    tier TEXT NOT NULL CHECK (tier IN ('standard', 'elevated', 'high')),
    reason_codes JSONB NOT NULL CHECK (valid_risk_reason_codes(reason_codes)),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    UNIQUE (scan_id, policy_version)
);

CREATE FUNCTION protect_submission_risk_assessment() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    permissions JSONB;
    stored_report_sha256 TEXT;
    sensitive_count SMALLINT;
    expected_tier TEXT;
    expected_reasons JSONB := '[]'::JSONB;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'Submission risk assessments are append-only' USING ERRCODE = '55000';
    END IF;
    SELECT report->'permissions', report_sha256
      INTO permissions, stored_report_sha256
      FROM submission_scan_results
     WHERE scan_id = NEW.scan_id AND submission_id = NEW.submission_id
       AND outcome = 'ready-for-review';
    IF NOT FOUND OR stored_report_sha256 <> NEW.source_report_sha256 OR
       jsonb_typeof(permissions) <> 'array' OR NEW.policy_version <> 1 THEN
        RAISE EXCEPTION 'Submission risk assessment source is invalid' USING ERRCODE = '23514';
    END IF;

    sensitive_count :=
        (permissions ? 'network.client')::INTEGER +
        (permissions ? 'documents.open')::INTEGER +
        (permissions ? 'audio.capture')::INTEGER +
        (permissions ? 'camera.capture')::INTEGER +
        (permissions ? 'radio.lora')::INTEGER +
        (permissions ? 'hardware.gpio')::INTEGER;
    expected_tier := CASE
        WHEN permissions ? 'radio.lora' OR permissions ? 'hardware.gpio' OR sensitive_count >= 2
            THEN 'high'
        WHEN sensitive_count = 1 THEN 'elevated'
        ELSE 'standard'
    END;
    IF permissions ? 'camera.capture' THEN
        expected_reasons := expected_reasons || jsonb_build_array('camera-capture');
    END IF;
    IF permissions ? 'hardware.gpio' THEN
        expected_reasons := expected_reasons || jsonb_build_array('hardware-control');
    END IF;
    IF permissions ? 'audio.capture' THEN
        expected_reasons := expected_reasons || jsonb_build_array('microphone-capture');
    END IF;
    IF sensitive_count >= 2 THEN
        expected_reasons := expected_reasons || jsonb_build_array('multiple-sensitive-capabilities');
    END IF;
    IF permissions ? 'network.client' THEN
        expected_reasons := expected_reasons || jsonb_build_array('network-access');
    END IF;
    IF permissions ? 'radio.lora' THEN
        expected_reasons := expected_reasons || jsonb_build_array('radio-transmit');
    END IF;
    IF permissions ? 'documents.open' THEN
        expected_reasons := expected_reasons || jsonb_build_array('user-documents');
    END IF;
    IF NEW.tier <> expected_tier OR NEW.reason_codes <> expected_reasons THEN
        RAISE EXCEPTION 'Submission risk assessment does not match policy' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER submission_risk_assessments_policy
    BEFORE INSERT OR UPDATE OR DELETE ON submission_risk_assessments
    FOR EACH ROW EXECUTE FUNCTION protect_submission_risk_assessment();

CREATE INDEX submission_risk_assessments_latest_idx
    ON submission_risk_assessments (scan_id, policy_version DESC);

WITH scan_permissions AS (
    SELECT scan_id, submission_id, report_sha256, created_unix_seconds,
           report->'permissions' AS permissions
      FROM submission_scan_results
     WHERE outcome = 'ready-for-review'
), classified AS (
    SELECT *,
        (permissions ? 'network.client')::INTEGER +
        (permissions ? 'documents.open')::INTEGER +
        (permissions ? 'audio.capture')::INTEGER +
        (permissions ? 'camera.capture')::INTEGER +
        (permissions ? 'radio.lora')::INTEGER +
        (permissions ? 'hardware.gpio')::INTEGER AS sensitive_count
      FROM scan_permissions
     WHERE jsonb_typeof(permissions) = 'array'
)
INSERT INTO submission_risk_assessments (
    assessment_id, scan_id, submission_id, source_report_sha256, policy_version,
    tier, reason_codes, created_unix_seconds
)
SELECT 'risk_' || substr(scan_id, 6, 28) || '0001', scan_id, submission_id,
       report_sha256, 1,
       CASE
           WHEN permissions ? 'radio.lora' OR permissions ? 'hardware.gpio' OR sensitive_count >= 2
               THEN 'high'
           WHEN sensitive_count = 1 THEN 'elevated'
           ELSE 'standard'
       END,
       (CASE WHEN permissions ? 'camera.capture' THEN jsonb_build_array('camera-capture') ELSE '[]'::JSONB END) ||
       (CASE WHEN permissions ? 'hardware.gpio' THEN jsonb_build_array('hardware-control') ELSE '[]'::JSONB END) ||
       (CASE WHEN permissions ? 'audio.capture' THEN jsonb_build_array('microphone-capture') ELSE '[]'::JSONB END) ||
       (CASE WHEN sensitive_count >= 2 THEN jsonb_build_array('multiple-sensitive-capabilities') ELSE '[]'::JSONB END) ||
       (CASE WHEN permissions ? 'network.client' THEN jsonb_build_array('network-access') ELSE '[]'::JSONB END) ||
       (CASE WHEN permissions ? 'radio.lora' THEN jsonb_build_array('radio-transmit') ELSE '[]'::JSONB END) ||
       (CASE WHEN permissions ? 'documents.open' THEN jsonb_build_array('user-documents') ELSE '[]'::JSONB END),
       created_unix_seconds
  FROM classified;
