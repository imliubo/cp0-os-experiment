CREATE TABLE portal_invitation_deliveries (
    delivery_id TEXT PRIMARY KEY CHECK (delivery_id ~ '^delivery_[0-9a-f]{32}$'),
    invitation_id TEXT NOT NULL UNIQUE REFERENCES team_invitations(invitation_id),
    token_ciphertext BYTEA CHECK (
        token_ciphertext IS NULL OR octet_length(token_ciphertext) BETWEEN 32 AND 4096
    ),
    state TEXT NOT NULL CHECK (
        state IN ('pending', 'leased', 'delivered', 'cancelled', 'failed')
    ),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    attempts SMALLINT NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 16),
    available_unix_seconds BIGINT NOT NULL CHECK (available_unix_seconds >= 1),
    lease_owner TEXT CHECK (
        lease_owner IS NULL OR
        (char_length(lease_owner) BETWEEN 1 AND 64 AND
         lease_owner ~ '^[A-Za-z0-9._-]+$')
    ),
    lease_expires_unix_seconds BIGINT,
    delivered_unix_seconds BIGINT,
    last_error_code TEXT CHECK (
        last_error_code IS NULL OR last_error_code ~ '^[a-z][a-z0-9-]{0,63}$'
    ),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    CHECK (available_unix_seconds >= created_unix_seconds),
    CHECK (
        (state = 'pending' AND token_ciphertext IS NOT NULL AND lease_owner IS NULL AND
         lease_expires_unix_seconds IS NULL AND delivered_unix_seconds IS NULL) OR
        (state = 'leased' AND token_ciphertext IS NOT NULL AND lease_owner IS NOT NULL AND
         lease_expires_unix_seconds > available_unix_seconds AND
         delivered_unix_seconds IS NULL) OR
        (state = 'delivered' AND token_ciphertext IS NULL AND lease_owner IS NULL AND
         lease_expires_unix_seconds IS NULL AND
         delivered_unix_seconds >= created_unix_seconds AND last_error_code IS NULL) OR
        (state IN ('cancelled', 'failed') AND token_ciphertext IS NULL AND
         lease_owner IS NULL AND lease_expires_unix_seconds IS NULL AND
         delivered_unix_seconds IS NULL)
    )
);

CREATE INDEX portal_invitation_deliveries_ready_idx
    ON portal_invitation_deliveries (available_unix_seconds, delivery_id)
    WHERE state IN ('pending', 'leased');

CREATE FUNCTION protect_portal_invitation_delivery() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Portal invitation deliveries cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'pending' OR NEW.resource_version <> 1 OR NEW.attempts <> 0 OR
           NEW.token_ciphertext IS NULL OR NEW.lease_owner IS NOT NULL OR
           NEW.lease_expires_unix_seconds IS NOT NULL OR
           NEW.delivered_unix_seconds IS NOT NULL OR NEW.last_error_code IS NOT NULL THEN
            RAISE EXCEPTION 'New invitation deliveries must start pending at version one'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF (NEW.delivery_id, NEW.invitation_id, NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.delivery_id, OLD.invitation_id, OLD.created_unix_seconds) OR
       NEW.resource_version <> OLD.resource_version + 1 OR
       OLD.state IN ('delivered', 'cancelled', 'failed') THEN
        RAISE EXCEPTION 'Invitation delivery identity or version is invalid'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.state = 'leased' THEN
        IF OLD.state NOT IN ('pending', 'leased') OR
           NEW.token_ciphertext IS DISTINCT FROM OLD.token_ciphertext OR
           NEW.attempts <> OLD.attempts + 1 OR NEW.lease_owner IS NULL OR
           NEW.lease_expires_unix_seconds IS NULL OR NEW.last_error_code IS NOT NULL OR
           NEW.available_unix_seconds > EXTRACT(EPOCH FROM clock_timestamp())::BIGINT OR
           (OLD.state = 'leased' AND
            OLD.lease_expires_unix_seconds > EXTRACT(EPOCH FROM clock_timestamp())::BIGINT) THEN
            RAISE EXCEPTION 'Invitation delivery lease transition is invalid'
                USING ERRCODE = '55000';
        END IF;
    ELSIF NEW.state = 'pending' THEN
        IF OLD.state <> 'leased' OR NEW.token_ciphertext IS DISTINCT FROM OLD.token_ciphertext OR
           NEW.attempts <> OLD.attempts OR NEW.available_unix_seconds <= OLD.available_unix_seconds OR
           NEW.lease_owner IS NOT NULL OR NEW.lease_expires_unix_seconds IS NOT NULL OR
           NEW.last_error_code IS NULL THEN
            RAISE EXCEPTION 'Invitation delivery retry transition is invalid'
                USING ERRCODE = '55000';
        END IF;
    ELSIF NEW.state = 'delivered' THEN
        IF OLD.state <> 'leased' OR NEW.attempts <> OLD.attempts OR
           NEW.token_ciphertext IS NOT NULL OR NEW.delivered_unix_seconds IS NULL THEN
            RAISE EXCEPTION 'Invitation delivery completion transition is invalid'
                USING ERRCODE = '55000';
        END IF;
    ELSIF NEW.state = 'cancelled' THEN
        IF OLD.state NOT IN ('pending', 'leased') OR NEW.attempts <> OLD.attempts OR
           NEW.token_ciphertext IS NOT NULL THEN
            RAISE EXCEPTION 'Invitation delivery cancellation transition is invalid'
                USING ERRCODE = '55000';
        END IF;
    ELSIF NEW.state = 'failed' THEN
        IF OLD.state <> 'leased' OR NEW.attempts <> OLD.attempts OR
           NEW.token_ciphertext IS NOT NULL OR NEW.last_error_code IS NULL THEN
            RAISE EXCEPTION 'Invitation delivery failure transition is invalid'
                USING ERRCODE = '55000';
        END IF;
    ELSE
        RAISE EXCEPTION 'Invitation delivery state transition is invalid'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER portal_invitation_deliveries_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON portal_invitation_deliveries
    FOR EACH ROW EXECUTE FUNCTION protect_portal_invitation_delivery();

CREATE FUNCTION validate_portal_invitation_delivery_binding() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    invitation_state TEXT;
BEGIN
    SELECT state INTO invitation_state
        FROM team_invitations WHERE invitation_id = NEW.invitation_id;
    IF NEW.state IN ('pending', 'leased') AND invitation_state IS DISTINCT FROM 'pending' THEN
        RAISE EXCEPTION 'Live delivery must belong to a pending invitation'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER portal_invitation_deliveries_invitation_binding
    AFTER INSERT OR UPDATE ON portal_invitation_deliveries
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION validate_portal_invitation_delivery_binding();
