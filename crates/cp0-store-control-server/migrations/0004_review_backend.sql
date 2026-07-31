CREATE TABLE reviewers (
    reviewer_id TEXT PRIMARY KEY CHECK (reviewer_id ~ '^reviewer_[0-9a-f]{32}$'),
    email TEXT NOT NULL UNIQUE CHECK (
        char_length(email) BETWEEN 3 AND 254 AND email = btrim(email) AND email = lower(email)
    ),
    role TEXT NOT NULL CHECK (role IN ('reviewer', 'senior-reviewer', 'admin')),
    two_factor_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    state TEXT NOT NULL CHECK (state IN ('active', 'suspended')),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1)
);

CREATE TABLE reviewer_access_tokens (
    token_sha256 CHAR(64) PRIMARY KEY CHECK (token_sha256 ~ '^[0-9a-f]{64}$'),
    reviewer_id TEXT NOT NULL REFERENCES reviewers(reviewer_id),
    scopes TEXT[] NOT NULL CHECK (scopes = ARRAY['store.review']::TEXT[]),
    expires_unix_seconds BIGINT NOT NULL CHECK (expires_unix_seconds >= 1),
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    CHECK (expires_unix_seconds > created_unix_seconds AND
           expires_unix_seconds <= created_unix_seconds + 3600)
);

CREATE TABLE review_assignments (
    assignment_id TEXT PRIMARY KEY CHECK (assignment_id ~ '^assignment_[0-9a-f]{32}$'),
    submission_id TEXT NOT NULL REFERENCES submissions(submission_id),
    reviewer_id TEXT NOT NULL REFERENCES reviewers(reviewer_id),
    assignment_kind TEXT NOT NULL CHECK (assignment_kind IN ('primary', 'secondary')),
    state TEXT NOT NULL CHECK (state IN ('active', 'completed', 'cancelled')),
    source_resource_version BIGINT NOT NULL CHECK (source_resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    completed_unix_seconds BIGINT,
    UNIQUE (submission_id, reviewer_id, assignment_kind),
    CHECK ((state = 'active' AND completed_unix_seconds IS NULL) OR
           (state IN ('completed', 'cancelled') AND
            completed_unix_seconds >= created_unix_seconds))
);

CREATE UNIQUE INDEX review_assignments_one_active_kind_idx
    ON review_assignments (submission_id, assignment_kind)
    WHERE state = 'active';

CREATE FUNCTION enforce_token_domain_uniqueness() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(NEW.token_sha256, 0));
    IF TG_TABLE_NAME = 'access_tokens' AND EXISTS (
        SELECT 1 FROM reviewer_access_tokens WHERE token_sha256 = NEW.token_sha256
    ) THEN
        RAISE EXCEPTION 'Access token digest already belongs to reviewer domain'
            USING ERRCODE = '23505';
    ELSIF TG_TABLE_NAME = 'reviewer_access_tokens' AND EXISTS (
        SELECT 1 FROM access_tokens WHERE token_sha256 = NEW.token_sha256
    ) THEN
        RAISE EXCEPTION 'Access token digest already belongs to developer domain'
            USING ERRCODE = '23505';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER access_tokens_cross_domain_unique
    BEFORE INSERT OR UPDATE ON access_tokens
    FOR EACH ROW EXECUTE FUNCTION enforce_token_domain_uniqueness();
CREATE TRIGGER reviewer_access_tokens_cross_domain_unique
    BEFORE INSERT OR UPDATE ON reviewer_access_tokens
    FOR EACH ROW EXECUTE FUNCTION enforce_token_domain_uniqueness();

ALTER TABLE review_messages ADD COLUMN actor_kind TEXT;
UPDATE review_messages
SET actor_kind = CASE
    WHEN actor_id ~ '^member_[0-9a-f]{32}$' THEN 'developer'
    WHEN actor_id ~ '^reviewer_[0-9a-f]{32}$' THEN 'reviewer'
END;
ALTER TABLE review_messages ALTER COLUMN actor_kind SET NOT NULL;
ALTER TABLE review_messages ADD CONSTRAINT review_messages_actor_domain CHECK (
    (actor_kind = 'developer' AND actor_id ~ '^member_[0-9a-f]{32}$') OR
    (actor_kind = 'reviewer' AND actor_id ~ '^reviewer_[0-9a-f]{32}$')
);
ALTER TABLE review_messages ADD CONSTRAINT review_messages_trimmed_body CHECK (body = btrim(body));

ALTER TABLE review_decisions ADD CONSTRAINT review_decisions_reviewer_fk
    FOREIGN KEY (reviewer_id) REFERENCES reviewers(reviewer_id);

CREATE FUNCTION valid_review_reason_codes(codes TEXT[]) RETURNS BOOLEAN
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
    code TEXT;
    seen TEXT[] := ARRAY[]::TEXT[];
BEGIN
    IF cardinality(codes) > 16 THEN
        RETURN FALSE;
    END IF;
    FOREACH code IN ARRAY codes LOOP
        IF code IS NULL OR code !~ '^[a-z][a-z0-9-]{0,63}$' OR code = ANY(seen) THEN
            RETURN FALSE;
        END IF;
        seen := array_append(seen, code);
    END LOOP;
    RETURN TRUE;
END;
$$;

ALTER TABLE review_decisions ADD CONSTRAINT review_decisions_structured_reasons CHECK (
    valid_review_reason_codes(reason_codes) AND note = btrim(note) AND
    (decision = 'approved' OR (cardinality(reason_codes) >= 1 AND char_length(note) >= 1))
);

CREATE FUNCTION protect_reviewer_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Reviewer identities cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.reviewer_id, NEW.email, NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.reviewer_id, OLD.email, OLD.created_unix_seconds) OR
       NEW.resource_version <= OLD.resource_version THEN
        RAISE EXCEPTION 'Reviewer identity is immutable' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER reviewers_stable_identity
    BEFORE UPDATE OR DELETE ON reviewers
    FOR EACH ROW EXECUTE FUNCTION protect_reviewer_identity();

CREATE FUNCTION protect_reviewer_access_token() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Reviewer access tokens cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.token_sha256, NEW.reviewer_id, NEW.scopes, NEW.expires_unix_seconds,
        NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.token_sha256, OLD.reviewer_id, OLD.scopes, OLD.expires_unix_seconds,
        OLD.created_unix_seconds) OR (OLD.revoked AND NOT NEW.revoked) THEN
        RAISE EXCEPTION 'Reviewer access tokens can only transition to revoked'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER reviewer_access_tokens_stable_identity
    BEFORE UPDATE OR DELETE ON reviewer_access_tokens
    FOR EACH ROW EXECUTE FUNCTION protect_reviewer_access_token();

CREATE FUNCTION protect_review_assignment() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Review assignments cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.assignment_id, NEW.submission_id, NEW.reviewer_id, NEW.assignment_kind,
        NEW.source_resource_version, NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.assignment_id, OLD.submission_id, OLD.reviewer_id, OLD.assignment_kind,
        OLD.source_resource_version, OLD.created_unix_seconds) OR
       OLD.state <> 'active' OR NEW.state NOT IN ('completed', 'cancelled') THEN
        RAISE EXCEPTION 'Review assignment transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER review_assignments_state_machine
    BEFORE UPDATE OR DELETE ON review_assignments
    FOR EACH ROW EXECUTE FUNCTION protect_review_assignment();
