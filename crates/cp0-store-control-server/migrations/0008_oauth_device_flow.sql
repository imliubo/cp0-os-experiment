CREATE TABLE oauth_device_authorizations (
    device_code_sha256 CHAR(64) PRIMARY KEY CHECK (device_code_sha256 ~ '^[0-9a-f]{64}$'),
    user_code CHAR(14) NOT NULL UNIQUE CHECK (user_code ~ '^[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}$'),
    client_id TEXT NOT NULL CHECK (client_id = 'cp0ctl'),
    scopes TEXT[] NOT NULL CHECK (scopes = ARRAY['store.submit']::TEXT[]),
    state TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'denied', 'consumed')),
    member_id TEXT REFERENCES team_members(member_id),
    requested_unix_seconds BIGINT NOT NULL CHECK (requested_unix_seconds >= 1),
    expires_unix_seconds BIGINT NOT NULL CHECK (
        expires_unix_seconds > requested_unix_seconds AND
        expires_unix_seconds <= requested_unix_seconds + 900
    ),
    poll_interval_seconds SMALLINT NOT NULL CHECK (poll_interval_seconds BETWEEN 5 AND 30),
    next_poll_unix_seconds BIGINT NOT NULL CHECK (next_poll_unix_seconds >= requested_unix_seconds),
    last_poll_unix_seconds BIGINT,
    decided_unix_seconds BIGINT,
    consumed_unix_seconds BIGINT,
    issued_token_sha256 CHAR(64) UNIQUE REFERENCES access_tokens(token_sha256),
    CHECK (last_poll_unix_seconds IS NULL OR
           last_poll_unix_seconds >= requested_unix_seconds),
    CHECK (
        (state = 'pending' AND member_id IS NULL AND decided_unix_seconds IS NULL AND
         consumed_unix_seconds IS NULL AND issued_token_sha256 IS NULL) OR
        (state IN ('approved', 'denied') AND member_id IS NOT NULL AND
         decided_unix_seconds >= requested_unix_seconds AND consumed_unix_seconds IS NULL AND
         issued_token_sha256 IS NULL) OR
        (state = 'consumed' AND member_id IS NOT NULL AND
         decided_unix_seconds >= requested_unix_seconds AND
         consumed_unix_seconds >= decided_unix_seconds AND issued_token_sha256 IS NOT NULL)
    )
);

CREATE INDEX oauth_device_authorizations_active_user_code_idx
    ON oauth_device_authorizations (user_code)
    WHERE state = 'pending';

CREATE FUNCTION protect_oauth_device_authorization() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'OAuth device authorizations cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.device_code_sha256, NEW.user_code, NEW.client_id, NEW.scopes,
        NEW.requested_unix_seconds, NEW.expires_unix_seconds) IS DISTINCT FROM
       (OLD.device_code_sha256, OLD.user_code, OLD.client_id, OLD.scopes,
        OLD.requested_unix_seconds, OLD.expires_unix_seconds) OR NOT (
        (OLD.state = 'pending' AND NEW.state = 'pending' AND
         (NEW.member_id, NEW.decided_unix_seconds, NEW.consumed_unix_seconds,
          NEW.issued_token_sha256) IS NOT DISTINCT FROM
         (OLD.member_id, OLD.decided_unix_seconds, OLD.consumed_unix_seconds,
          OLD.issued_token_sha256) AND
         NEW.poll_interval_seconds >= OLD.poll_interval_seconds AND
         NEW.next_poll_unix_seconds >= OLD.next_poll_unix_seconds AND
         NEW.last_poll_unix_seconds IS NOT NULL AND
         (OLD.last_poll_unix_seconds IS NULL OR
          NEW.last_poll_unix_seconds >= OLD.last_poll_unix_seconds)) OR
        (OLD.state = 'pending' AND NEW.state IN ('approved', 'denied') AND
         (NEW.poll_interval_seconds, NEW.next_poll_unix_seconds,
          NEW.last_poll_unix_seconds, NEW.consumed_unix_seconds,
          NEW.issued_token_sha256) IS NOT DISTINCT FROM
         (OLD.poll_interval_seconds, OLD.next_poll_unix_seconds,
          OLD.last_poll_unix_seconds, OLD.consumed_unix_seconds,
          OLD.issued_token_sha256) AND NEW.member_id IS NOT NULL AND
         NEW.decided_unix_seconds IS NOT NULL) OR
        (OLD.state = 'approved' AND NEW.state = 'consumed' AND
         (NEW.member_id, NEW.decided_unix_seconds, NEW.poll_interval_seconds,
          NEW.next_poll_unix_seconds, NEW.last_poll_unix_seconds) IS NOT DISTINCT FROM
         (OLD.member_id, OLD.decided_unix_seconds, OLD.poll_interval_seconds,
          OLD.next_poll_unix_seconds, OLD.last_poll_unix_seconds) AND
         NEW.consumed_unix_seconds IS NOT NULL AND NEW.issued_token_sha256 IS NOT NULL)
       ) THEN
        RAISE EXCEPTION 'OAuth device authorization transition is invalid'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER oauth_device_authorizations_state_machine
    BEFORE UPDATE OR DELETE ON oauth_device_authorizations
    FOR EACH ROW EXECUTE FUNCTION protect_oauth_device_authorization();

CREATE OR REPLACE FUNCTION protect_access_token() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Access tokens cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.token_sha256, NEW.member_id, NEW.scopes, NEW.expires_unix_seconds,
        NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.token_sha256, OLD.member_id, OLD.scopes, OLD.expires_unix_seconds,
        OLD.created_unix_seconds) OR (OLD.revoked AND NOT NEW.revoked) THEN
        RAISE EXCEPTION 'Access tokens can only transition to revoked' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER access_tokens_stable_identity ON access_tokens;
CREATE TRIGGER access_tokens_stable_identity
    BEFORE UPDATE OR DELETE ON access_tokens
    FOR EACH ROW EXECUTE FUNCTION protect_access_token();
