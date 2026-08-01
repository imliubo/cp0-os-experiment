ALTER TABLE oidc_login_transactions
    ADD COLUMN request_sha256 CHAR(64) CHECK (
        request_sha256 IS NULL OR request_sha256 ~ '^[0-9a-f]{64}$'
    ),
    ADD COLUMN idempotency_key_sha256 CHAR(64) CHECK (
        idempotency_key_sha256 IS NULL OR idempotency_key_sha256 ~ '^[0-9a-f]{64}$'
    ),
    ADD CONSTRAINT oidc_login_transactions_link_request_check CHECK (
        (intent = 'link' AND request_sha256 IS NOT NULL AND idempotency_key_sha256 IS NOT NULL) OR
        (intent <> 'link' AND request_sha256 IS NULL AND idempotency_key_sha256 IS NULL)
    );

CREATE OR REPLACE FUNCTION protect_oidc_login_transaction() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'OIDC login transactions cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' AND (NEW.state <> 'pending' OR NEW.consumed_unix_seconds IS NOT NULL) THEN
        RAISE EXCEPTION 'New OIDC login transactions must start pending'
            USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'UPDATE' AND (
        (NEW.transaction_id, NEW.state_sha256, NEW.nonce_sha256,
         NEW.pkce_verifier_ciphertext, NEW.provider_key, NEW.provider_config_sha256,
         NEW.intent, NEW.account_id, NEW.session_sha256, NEW.requested_unix_seconds,
         NEW.expires_unix_seconds, NEW.request_sha256, NEW.idempotency_key_sha256) IS DISTINCT FROM
        (OLD.transaction_id, OLD.state_sha256, OLD.nonce_sha256,
         OLD.pkce_verifier_ciphertext, OLD.provider_key, OLD.provider_config_sha256,
         OLD.intent, OLD.account_id, OLD.session_sha256, OLD.requested_unix_seconds,
         OLD.expires_unix_seconds, OLD.request_sha256, OLD.idempotency_key_sha256) OR
        OLD.state <> 'pending' OR NEW.state NOT IN ('consumed', 'expired') OR
        NEW.consumed_unix_seconds IS NULL
    ) THEN
        RAISE EXCEPTION 'OIDC login transaction transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION revoke_portal_sessions_for_membership_loss() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.membership_state = 'active' AND
       NEW.membership_state IN ('suspended', 'removed') AND
       NEW.account_id IS NOT NULL THEN
        UPDATE portal_sessions SET state = 'revoked',
            ended_unix_seconds = GREATEST(
                created_unix_seconds,
                EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
            ),
            resource_version = resource_version + 1
        WHERE account_id = NEW.account_id AND state = 'active';
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER team_members_revoke_portal_sessions
    AFTER UPDATE ON team_members
    FOR EACH ROW EXECUTE FUNCTION revoke_portal_sessions_for_membership_loss();
