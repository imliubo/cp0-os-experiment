ALTER TABLE access_tokens
    ADD COLUMN mfa_authenticated_unix_seconds BIGINT;
ALTER TABLE access_tokens
    ADD CONSTRAINT access_tokens_mfa_authenticated_check CHECK (
        mfa_authenticated_unix_seconds IS NULL OR
        (mfa_authenticated_unix_seconds >= 1 AND
         mfa_authenticated_unix_seconds <= created_unix_seconds)
    );

ALTER TABLE team_members
    ADD CONSTRAINT team_members_email_check CHECK (
        char_length(email) BETWEEN 3 AND 254 AND email = btrim(email) AND email = lower(email)
    );

CREATE OR REPLACE FUNCTION protect_access_token() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Access tokens cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.token_sha256, NEW.member_id, NEW.scopes, NEW.expires_unix_seconds,
        NEW.created_unix_seconds, NEW.mfa_authenticated_unix_seconds) IS DISTINCT FROM
       (OLD.token_sha256, OLD.member_id, OLD.scopes, OLD.expires_unix_seconds,
        OLD.created_unix_seconds, OLD.mfa_authenticated_unix_seconds) OR
       (OLD.revoked AND NOT NEW.revoked) THEN
        RAISE EXCEPTION 'Access tokens can only transition to revoked' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER access_tokens_stable_identity ON access_tokens;
CREATE TRIGGER access_tokens_stable_identity
    BEFORE UPDATE OR DELETE ON access_tokens
    FOR EACH ROW EXECUTE FUNCTION protect_access_token();

CREATE OR REPLACE FUNCTION protect_member_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.member_id <> OLD.member_id OR NEW.team_id <> OLD.team_id THEN
        RAISE EXCEPTION 'Team membership identity cannot be reassigned' USING ERRCODE = '55000';
    END IF;
    IF NEW.resource_version <> OLD.resource_version + 1 THEN
        RAISE EXCEPTION 'Team member resource version must advance by one' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION protect_team_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Teams cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF NEW.team_id <> OLD.team_id OR NEW.resource_version <> OLD.resource_version + 1 THEN
        RAISE EXCEPTION 'Team identity or resource version is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER teams_stable_identity
    BEFORE UPDATE OR DELETE ON teams
    FOR EACH ROW EXECUTE FUNCTION protect_team_identity();
