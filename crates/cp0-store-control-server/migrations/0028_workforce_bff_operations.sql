CREATE TABLE workforce_control_token_issuances (
    session_sha256 CHAR(64) NOT NULL REFERENCES workforce_sessions(session_sha256),
    audience TEXT NOT NULL CHECK (audience IN ('review', 'operations')),
    idempotency_key_sha256 CHAR(64) NOT NULL CHECK (
        idempotency_key_sha256 ~ '^[0-9a-f]{64}$'
    ),
    request_sha256 CHAR(64) NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    token_sha256 CHAR(64) NOT NULL UNIQUE CHECK (token_sha256 ~ '^[0-9a-f]{64}$'),
    principal_id TEXT NOT NULL CHECK (
        (audience = 'review' AND principal_id ~ '^reviewer_[0-9a-f]{32}$') OR
        (audience = 'operations' AND principal_id ~ '^operator_[0-9a-f]{32}$')
    ),
    scope TEXT NOT NULL CHECK (
        (audience = 'review' AND scope = 'store.review') OR
        (audience = 'operations' AND scope IN ('store.editorial', 'store.moderation'))
    ),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    expires_unix_seconds BIGINT NOT NULL,
    PRIMARY KEY (session_sha256, audience, idempotency_key_sha256),
    CHECK (
        expires_unix_seconds > created_unix_seconds AND
        expires_unix_seconds <= created_unix_seconds + 300
    )
);

CREATE TABLE workforce_logout_records (
    session_sha256 CHAR(64) NOT NULL REFERENCES workforce_sessions(session_sha256),
    audience TEXT NOT NULL CHECK (audience IN ('review', 'operations')),
    idempotency_key_sha256 CHAR(64) NOT NULL CHECK (
        idempotency_key_sha256 ~ '^[0-9a-f]{64}$'
    ),
    request_sha256 CHAR(64) NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    completed_unix_seconds BIGINT NOT NULL CHECK (completed_unix_seconds >= 1),
    PRIMARY KEY (session_sha256, audience, idempotency_key_sha256)
);

CREATE TRIGGER workforce_control_token_issuances_append_only
    BEFORE UPDATE OR DELETE ON workforce_control_token_issuances
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();
CREATE TRIGGER workforce_logout_records_append_only
    BEFORE UPDATE OR DELETE ON workforce_logout_records
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();

CREATE FUNCTION validate_workforce_control_token_issuance() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    token_count BIGINT;
BEGIN
    IF NEW.audience = 'review' THEN
        SELECT COUNT(*) INTO token_count FROM reviewer_access_tokens
         WHERE token_sha256 = NEW.token_sha256 AND reviewer_id = NEW.principal_id AND
               workforce_session_sha256 = NEW.session_sha256 AND scopes = ARRAY[NEW.scope] AND
               created_unix_seconds = NEW.created_unix_seconds AND
               expires_unix_seconds = NEW.expires_unix_seconds;
    ELSE
        SELECT COUNT(*) INTO token_count FROM store_operator_access_tokens
         WHERE token_sha256 = NEW.token_sha256 AND operator_id = NEW.principal_id AND
               workforce_session_sha256 = NEW.session_sha256 AND scopes = ARRAY[NEW.scope] AND
               created_unix_seconds = NEW.created_unix_seconds AND
               expires_unix_seconds = NEW.expires_unix_seconds;
    END IF;
    IF token_count <> 1 THEN
        RAISE EXCEPTION 'Workforce token issuance does not match its access token'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER workforce_control_token_issuances_token_binding
    AFTER INSERT ON workforce_control_token_issuances
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION validate_workforce_control_token_issuance();
