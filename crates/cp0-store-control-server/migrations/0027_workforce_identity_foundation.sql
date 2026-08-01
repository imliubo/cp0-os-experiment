CREATE TABLE workforce_identity_links (
    link_id TEXT PRIMARY KEY CHECK (link_id ~ '^wflink_[0-9a-f]{32}$'),
    provider_key TEXT NOT NULL CHECK (provider_key ~ '^[a-z][a-z0-9-]{0,31}$'),
    issuer TEXT NOT NULL CHECK (
        char_length(issuer) BETWEEN 9 AND 512 AND issuer = btrim(issuer) AND
        issuer ~ '^https://[^[:space:]?#]+$'
    ),
    subject_hmac_sha256 CHAR(64) NOT NULL CHECK (
        subject_hmac_sha256 ~ '^[0-9a-f]{64}$'
    ),
    reviewer_id TEXT REFERENCES reviewers(reviewer_id),
    operator_id TEXT REFERENCES store_operators(operator_id),
    state TEXT NOT NULL CHECK (state IN ('active', 'revoked')),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    linked_unix_seconds BIGINT NOT NULL CHECK (linked_unix_seconds >= 1),
    revoked_unix_seconds BIGINT,
    UNIQUE (issuer, subject_hmac_sha256),
    CHECK ((reviewer_id IS NOT NULL) <> (operator_id IS NOT NULL)),
    CHECK (
        (state = 'active' AND revoked_unix_seconds IS NULL) OR
        (state = 'revoked' AND revoked_unix_seconds >= linked_unix_seconds)
    )
);

CREATE UNIQUE INDEX workforce_identity_links_active_reviewer_idx
    ON workforce_identity_links (reviewer_id)
    WHERE state = 'active' AND reviewer_id IS NOT NULL;
CREATE UNIQUE INDEX workforce_identity_links_active_operator_idx
    ON workforce_identity_links (operator_id)
    WHERE state = 'active' AND operator_id IS NOT NULL;

CREATE TABLE workforce_sessions (
    session_sha256 CHAR(64) PRIMARY KEY CHECK (session_sha256 ~ '^[0-9a-f]{64}$'),
    csrf_sha256 CHAR(64) NOT NULL CHECK (csrf_sha256 ~ '^[0-9a-f]{64}$'),
    link_id TEXT NOT NULL REFERENCES workforce_identity_links(link_id),
    audience TEXT NOT NULL CHECK (audience IN ('review', 'operations')),
    state TEXT NOT NULL CHECK (state IN ('active', 'revoked', 'expired')),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    last_seen_unix_seconds BIGINT NOT NULL,
    idle_expires_unix_seconds BIGINT NOT NULL,
    absolute_expires_unix_seconds BIGINT NOT NULL,
    mfa_authenticated_unix_seconds BIGINT NOT NULL CHECK (
        mfa_authenticated_unix_seconds >= 1
    ),
    ended_unix_seconds BIGINT,
    CHECK (last_seen_unix_seconds BETWEEN created_unix_seconds AND absolute_expires_unix_seconds),
    CHECK (absolute_expires_unix_seconds = created_unix_seconds + 28800),
    CHECK (
        idle_expires_unix_seconds =
            LEAST(last_seen_unix_seconds + 900, absolute_expires_unix_seconds)
    ),
    CHECK (mfa_authenticated_unix_seconds <= created_unix_seconds),
    CHECK (
        (state = 'active' AND ended_unix_seconds IS NULL) OR
        (state IN ('revoked', 'expired') AND ended_unix_seconds >= created_unix_seconds)
    )
);

CREATE INDEX workforce_sessions_active_link_idx
    ON workforce_sessions (link_id, absolute_expires_unix_seconds)
    WHERE state = 'active';

CREATE TABLE workforce_oidc_transactions (
    transaction_id TEXT PRIMARY KEY CHECK (transaction_id ~ '^wfoidc_[0-9a-f]{32}$'),
    state_sha256 CHAR(64) NOT NULL UNIQUE CHECK (state_sha256 ~ '^[0-9a-f]{64}$'),
    nonce_sha256 CHAR(64) NOT NULL CHECK (nonce_sha256 ~ '^[0-9a-f]{64}$'),
    pkce_verifier_ciphertext BYTEA NOT NULL CHECK (
        octet_length(pkce_verifier_ciphertext) BETWEEN 32 AND 4096
    ),
    provider_key TEXT NOT NULL CHECK (provider_key ~ '^[a-z][a-z0-9-]{0,31}$'),
    provider_config_sha256 CHAR(64) NOT NULL CHECK (
        provider_config_sha256 ~ '^[0-9a-f]{64}$'
    ),
    audience TEXT NOT NULL CHECK (audience IN ('review', 'operations')),
    intent TEXT NOT NULL CHECK (intent IN ('login', 'step-up')),
    session_sha256 CHAR(64) REFERENCES workforce_sessions(session_sha256),
    state TEXT NOT NULL CHECK (state IN ('pending', 'consumed', 'expired')),
    requested_unix_seconds BIGINT NOT NULL CHECK (requested_unix_seconds >= 1),
    expires_unix_seconds BIGINT NOT NULL,
    consumed_unix_seconds BIGINT,
    CHECK (expires_unix_seconds = requested_unix_seconds + 600),
    CHECK (
        (intent = 'login' AND session_sha256 IS NULL) OR
        (intent = 'step-up' AND session_sha256 IS NOT NULL)
    ),
    CHECK (
        (state = 'pending' AND consumed_unix_seconds IS NULL) OR
        (state IN ('consumed', 'expired') AND
         consumed_unix_seconds >= requested_unix_seconds)
    ),
    CHECK (state <> 'consumed' OR consumed_unix_seconds <= expires_unix_seconds)
);

CREATE INDEX workforce_oidc_transactions_pending_session_idx
    ON workforce_oidc_transactions (session_sha256)
    WHERE state = 'pending' AND session_sha256 IS NOT NULL;

ALTER TABLE reviewer_access_tokens
    ADD COLUMN workforce_session_sha256 CHAR(64) REFERENCES workforce_sessions(session_sha256);
ALTER TABLE store_operator_access_tokens
    ADD COLUMN workforce_session_sha256 CHAR(64) REFERENCES workforce_sessions(session_sha256);

CREATE INDEX reviewer_access_tokens_workforce_session_idx
    ON reviewer_access_tokens (workforce_session_sha256)
    WHERE workforce_session_sha256 IS NOT NULL;
CREATE INDEX store_operator_access_tokens_workforce_session_idx
    ON store_operator_access_tokens (workforce_session_sha256)
    WHERE workforce_session_sha256 IS NOT NULL;

CREATE FUNCTION protect_workforce_identity_link() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Workforce identity links cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'active' OR NEW.resource_version <> 1 OR
           NEW.revoked_unix_seconds IS NOT NULL THEN
            RAISE EXCEPTION 'New workforce links must start active at version one'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF (NEW.link_id, NEW.provider_key, NEW.issuer, NEW.subject_hmac_sha256,
        NEW.reviewer_id, NEW.operator_id, NEW.linked_unix_seconds) IS DISTINCT FROM
       (OLD.link_id, OLD.provider_key, OLD.issuer, OLD.subject_hmac_sha256,
        OLD.reviewer_id, OLD.operator_id, OLD.linked_unix_seconds) OR
       NEW.resource_version <> OLD.resource_version + 1 OR
       OLD.state <> 'active' OR NEW.state <> 'revoked' OR
       NEW.revoked_unix_seconds IS NULL THEN
        RAISE EXCEPTION 'Workforce identity link transition is invalid'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER workforce_identity_links_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON workforce_identity_links
    FOR EACH ROW EXECUTE FUNCTION protect_workforce_identity_link();

CREATE FUNCTION validate_workforce_session_principal() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    link_state TEXT;
    reviewer_state TEXT;
    reviewer_mfa BOOLEAN;
    operator_state TEXT;
    operator_mfa BOOLEAN;
    linked_reviewer_id TEXT;
    linked_operator_id TEXT;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RETURN NEW;
    END IF;
    SELECT link.state, link.reviewer_id, link.operator_id,
           reviewer.state, reviewer.two_factor_enabled,
           operator.state, operator.two_factor_enabled
      INTO link_state, linked_reviewer_id, linked_operator_id,
           reviewer_state, reviewer_mfa, operator_state, operator_mfa
      FROM workforce_identity_links link
      LEFT JOIN reviewers reviewer ON reviewer.reviewer_id = link.reviewer_id
      LEFT JOIN store_operators operator ON operator.operator_id = link.operator_id
     WHERE link.link_id = NEW.link_id;
    IF link_state IS DISTINCT FROM 'active' OR
       (NEW.audience = 'review' AND (
           linked_reviewer_id IS NULL OR reviewer_state IS DISTINCT FROM 'active' OR
           reviewer_mfa IS DISTINCT FROM TRUE
       )) OR
       (NEW.audience = 'operations' AND (
           linked_operator_id IS NULL OR operator_state IS DISTINCT FROM 'active' OR
           operator_mfa IS DISTINCT FROM TRUE
       )) THEN
        RAISE EXCEPTION 'Workforce session principal is unavailable or mismatched'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER workforce_sessions_validate_principal
    BEFORE INSERT ON workforce_sessions
    FOR EACH ROW EXECUTE FUNCTION validate_workforce_session_principal();

CREATE FUNCTION protect_workforce_session() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Workforce sessions cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'active' OR NEW.resource_version <> 1 OR
           NEW.last_seen_unix_seconds <> NEW.created_unix_seconds OR
           NEW.ended_unix_seconds IS NOT NULL THEN
            RAISE EXCEPTION 'New workforce sessions must start active at version one'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF (NEW.session_sha256, NEW.csrf_sha256, NEW.link_id, NEW.audience,
        NEW.created_unix_seconds, NEW.absolute_expires_unix_seconds,
        NEW.mfa_authenticated_unix_seconds) IS DISTINCT FROM
       (OLD.session_sha256, OLD.csrf_sha256, OLD.link_id, OLD.audience,
        OLD.created_unix_seconds, OLD.absolute_expires_unix_seconds,
        OLD.mfa_authenticated_unix_seconds) OR
       NEW.resource_version <> OLD.resource_version + 1 OR OLD.state <> 'active' THEN
        RAISE EXCEPTION 'Workforce session identity or version is invalid'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.state = 'active' THEN
        IF NEW.ended_unix_seconds IS NOT NULL OR
           NEW.last_seen_unix_seconds <= OLD.last_seen_unix_seconds THEN
            RAISE EXCEPTION 'Workforce session activity must advance monotonically'
                USING ERRCODE = '55000';
        END IF;
    ELSIF NEW.state IN ('revoked', 'expired') THEN
        IF (NEW.last_seen_unix_seconds, NEW.idle_expires_unix_seconds) IS DISTINCT FROM
           (OLD.last_seen_unix_seconds, OLD.idle_expires_unix_seconds) OR
           NEW.ended_unix_seconds IS NULL THEN
            RAISE EXCEPTION 'Workforce session terminal transition is invalid'
                USING ERRCODE = '55000';
        END IF;
    ELSE
        RAISE EXCEPTION 'Workforce session transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER workforce_sessions_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON workforce_sessions
    FOR EACH ROW EXECUTE FUNCTION protect_workforce_session();

CREATE FUNCTION protect_workforce_oidc_transaction() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Workforce OIDC transactions cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'pending' OR NEW.consumed_unix_seconds IS NOT NULL THEN
            RAISE EXCEPTION 'New workforce OIDC transactions must start pending'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF (NEW.transaction_id, NEW.state_sha256, NEW.nonce_sha256,
        NEW.pkce_verifier_ciphertext, NEW.provider_key, NEW.provider_config_sha256,
        NEW.audience, NEW.intent, NEW.session_sha256, NEW.requested_unix_seconds,
        NEW.expires_unix_seconds) IS DISTINCT FROM
       (OLD.transaction_id, OLD.state_sha256, OLD.nonce_sha256,
        OLD.pkce_verifier_ciphertext, OLD.provider_key, OLD.provider_config_sha256,
        OLD.audience, OLD.intent, OLD.session_sha256, OLD.requested_unix_seconds,
        OLD.expires_unix_seconds) OR
       OLD.state <> 'pending' OR NEW.state NOT IN ('consumed', 'expired') OR
       NEW.consumed_unix_seconds IS NULL THEN
        RAISE EXCEPTION 'Workforce OIDC transaction transition is invalid'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER workforce_oidc_transactions_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON workforce_oidc_transactions
    FOR EACH ROW EXECUTE FUNCTION protect_workforce_oidc_transaction();

CREATE FUNCTION validate_workforce_oidc_transaction_session() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    session_state TEXT;
    session_audience TEXT;
    session_idle BIGINT;
    session_absolute BIGINT;
BEGIN
    IF NEW.intent = 'step-up' THEN
        SELECT state, audience, idle_expires_unix_seconds, absolute_expires_unix_seconds
          INTO session_state, session_audience, session_idle, session_absolute
          FROM workforce_sessions WHERE session_sha256 = NEW.session_sha256;
        IF session_audience IS DISTINCT FROM NEW.audience OR
           (NEW.state = 'pending' AND (
               session_state IS DISTINCT FROM 'active' OR
               session_idle <= EXTRACT(EPOCH FROM clock_timestamp())::BIGINT OR
               session_absolute <= EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
           )) THEN
            RAISE EXCEPTION 'Workforce OIDC transaction session binding is invalid'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER workforce_oidc_transactions_session_binding
    AFTER INSERT OR UPDATE ON workforce_oidc_transactions
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION validate_workforce_oidc_transaction_session();

CREATE OR REPLACE FUNCTION protect_reviewer_access_token() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Reviewer access tokens cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.token_sha256, NEW.reviewer_id, NEW.scopes, NEW.expires_unix_seconds,
        NEW.created_unix_seconds, NEW.workforce_session_sha256) IS DISTINCT FROM
       (OLD.token_sha256, OLD.reviewer_id, OLD.scopes, OLD.expires_unix_seconds,
        OLD.created_unix_seconds, OLD.workforce_session_sha256) OR
       (OLD.revoked AND NOT NEW.revoked) THEN
        RAISE EXCEPTION 'Reviewer access tokens can only transition to revoked'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION protect_store_operator_access_token() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' OR
       (NEW.token_sha256, NEW.operator_id, NEW.scopes, NEW.expires_unix_seconds,
        NEW.created_unix_seconds, NEW.workforce_session_sha256) IS DISTINCT FROM
       (OLD.token_sha256, OLD.operator_id, OLD.scopes, OLD.expires_unix_seconds,
        OLD.created_unix_seconds, OLD.workforce_session_sha256) OR
       (OLD.revoked AND NOT NEW.revoked) THEN
        RAISE EXCEPTION 'Store operator tokens can only transition to revoked'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION validate_workforce_access_token() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    session_state TEXT;
    session_audience TEXT;
    session_idle BIGINT;
    session_absolute BIGINT;
    session_created BIGINT;
    link_state TEXT;
    linked_reviewer_id TEXT;
    linked_operator_id TEXT;
    token_principal_id TEXT;
BEGIN
    IF NEW.workforce_session_sha256 IS NULL THEN
        RETURN NEW;
    END IF;
    SELECT session.state, session.audience, session.idle_expires_unix_seconds,
           session.absolute_expires_unix_seconds, session.created_unix_seconds,
           link.state, link.reviewer_id, link.operator_id
      INTO session_state, session_audience, session_idle, session_absolute,
           session_created, link_state, linked_reviewer_id, linked_operator_id
      FROM workforce_sessions session
      JOIN workforce_identity_links link ON link.link_id = session.link_id
     WHERE session.session_sha256 = NEW.workforce_session_sha256;
    token_principal_id := COALESCE(
        to_jsonb(NEW)->>'reviewer_id',
        to_jsonb(NEW)->>'operator_id'
    );
    IF session_state IS DISTINCT FROM 'active' OR link_state IS DISTINCT FROM 'active' OR
       NEW.created_unix_seconds < session_created OR
       NEW.expires_unix_seconds > LEAST(
           session_idle, session_absolute, NEW.created_unix_seconds + 300
       ) OR
       (TG_TABLE_NAME = 'reviewer_access_tokens' AND (
           session_audience IS DISTINCT FROM 'review' OR
           token_principal_id IS DISTINCT FROM linked_reviewer_id
       )) OR
       (TG_TABLE_NAME = 'store_operator_access_tokens' AND (
           session_audience IS DISTINCT FROM 'operations' OR
           token_principal_id IS DISTINCT FROM linked_operator_id
       )) THEN
        RAISE EXCEPTION 'Workforce access token is not bound to an active matching session'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER reviewer_access_tokens_validate_workforce_session
    BEFORE INSERT ON reviewer_access_tokens
    FOR EACH ROW EXECUTE FUNCTION validate_workforce_access_token();
CREATE TRIGGER store_operator_access_tokens_validate_workforce_session
    BEFORE INSERT ON store_operator_access_tokens
    FOR EACH ROW EXECUTE FUNCTION validate_workforce_access_token();

CREATE FUNCTION revoke_workforce_tokens_for_session() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.state = 'active' AND NEW.state IN ('revoked', 'expired') THEN
        UPDATE reviewer_access_tokens SET revoked = TRUE
         WHERE workforce_session_sha256 = NEW.session_sha256 AND NOT revoked;
        UPDATE store_operator_access_tokens SET revoked = TRUE
         WHERE workforce_session_sha256 = NEW.session_sha256 AND NOT revoked;
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER workforce_sessions_revoke_access_tokens
    AFTER UPDATE ON workforce_sessions
    FOR EACH ROW EXECUTE FUNCTION revoke_workforce_tokens_for_session();

CREATE FUNCTION expire_workforce_oidc_transactions_for_session() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.state = 'active' AND NEW.state IN ('revoked', 'expired') THEN
        UPDATE workforce_oidc_transactions SET state = 'expired',
            consumed_unix_seconds = GREATEST(
                requested_unix_seconds,
                EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
            )
         WHERE session_sha256 = NEW.session_sha256 AND state = 'pending';
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER workforce_sessions_expire_oidc_transactions
    AFTER UPDATE ON workforce_sessions
    FOR EACH ROW EXECUTE FUNCTION expire_workforce_oidc_transactions_for_session();

CREATE FUNCTION revoke_workforce_sessions_for_link() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.state = 'active' AND NEW.state = 'revoked' THEN
        UPDATE workforce_sessions SET state = 'revoked',
            ended_unix_seconds = GREATEST(
                created_unix_seconds,
                EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
            ),
            resource_version = resource_version + 1
         WHERE link_id = NEW.link_id AND state = 'active';
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER workforce_identity_links_revoke_sessions
    AFTER UPDATE ON workforce_identity_links
    FOR EACH ROW EXECUTE FUNCTION revoke_workforce_sessions_for_link();

CREATE FUNCTION revoke_workforce_sessions_for_principal() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    principal_id TEXT;
BEGIN
    IF OLD.state = 'active' AND NEW.state = 'suspended' THEN
        principal_id := COALESCE(
            to_jsonb(NEW)->>'reviewer_id',
            to_jsonb(NEW)->>'operator_id'
        );
        UPDATE workforce_sessions session SET state = 'revoked',
            ended_unix_seconds = GREATEST(
                session.created_unix_seconds,
                EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
            ),
            resource_version = session.resource_version + 1
          FROM workforce_identity_links link
         WHERE session.link_id = link.link_id AND session.state = 'active' AND
               ((TG_TABLE_NAME = 'reviewers' AND link.reviewer_id = principal_id) OR
                (TG_TABLE_NAME = 'store_operators' AND link.operator_id = principal_id));
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER reviewers_revoke_workforce_sessions
    AFTER UPDATE ON reviewers
    FOR EACH ROW EXECUTE FUNCTION revoke_workforce_sessions_for_principal();
CREATE TRIGGER store_operators_revoke_workforce_sessions
    AFTER UPDATE ON store_operators
    FOR EACH ROW EXECUTE FUNCTION revoke_workforce_sessions_for_principal();
