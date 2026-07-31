CREATE TABLE teams (
    team_id TEXT PRIMARY KEY CHECK (team_id ~ '^team_[0-9a-f]{32}$'),
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80 AND name = btrim(name)),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1)
);

CREATE TABLE team_members (
    member_id TEXT PRIMARY KEY CHECK (member_id ~ '^member_[0-9a-f]{32}$'),
    team_id TEXT NOT NULL REFERENCES teams(team_id),
    email TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'developer', 'release-manager', 'viewer')),
    two_factor_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    UNIQUE (team_id, email),
    UNIQUE (team_id, member_id)
);

CREATE TABLE access_tokens (
    token_sha256 CHAR(64) PRIMARY KEY CHECK (token_sha256 ~ '^[0-9a-f]{64}$'),
    member_id TEXT NOT NULL REFERENCES team_members(member_id),
    scopes TEXT[] NOT NULL CHECK (cardinality(scopes) BETWEEN 1 AND 16),
    expires_unix_seconds BIGINT NOT NULL CHECK (expires_unix_seconds >= 1),
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    CHECK (expires_unix_seconds > created_unix_seconds AND
           expires_unix_seconds <= created_unix_seconds + 3600)
);

CREATE TABLE developer_keys (
    key_id TEXT PRIMARY KEY CHECK (key_id ~ '^key_[0-9a-f]{32}$'),
    team_id TEXT NOT NULL REFERENCES teams(team_id),
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    algorithm TEXT NOT NULL CHECK (algorithm = 'ed25519'),
    public_key BYTEA NOT NULL CHECK (octet_length(public_key) = 32),
    fingerprint_sha256 CHAR(64) NOT NULL UNIQUE CHECK (fingerprint_sha256 ~ '^[0-9a-f]{64}$'),
    state TEXT NOT NULL CHECK (state IN ('active', 'revoked')),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    revoked_unix_seconds BIGINT,
    CHECK ((state = 'active' AND revoked_unix_seconds IS NULL) OR
           (state = 'revoked' AND revoked_unix_seconds IS NOT NULL))
);

CREATE TABLE apps (
    app_id TEXT PRIMARY KEY CHECK (
        char_length(app_id) BETWEEN 5 AND 128 AND
        app_id ~ '^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*){2,}$'
    ),
    owner_team_id TEXT NOT NULL REFERENCES teams(team_id),
    default_locale TEXT NOT NULL CHECK (
        char_length(default_locale) BETWEEN 2 AND 16 AND
        default_locale ~ '^[a-z]{2,3}(-[A-Z][a-z]{3})?(-([A-Z]{2}|[0-9]{3}))?$'
    ),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1)
);

CREATE TABLE submissions (
    submission_id TEXT PRIMARY KEY CHECK (submission_id ~ '^sub_[0-9a-f]{32}$'),
    app_id TEXT NOT NULL REFERENCES apps(app_id),
    version TEXT NOT NULL CHECK (char_length(version) BETWEEN 5 AND 64),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    state TEXT NOT NULL CHECK (state IN (
        'draft', 'uploading', 'processing', 'ready-for-review', 'in-review',
        'needs-changes', 'approved', 'rejected', 'withdrawn'
    )),
    package_sha256 CHAR(64) NOT NULL CHECK (package_sha256 ~ '^[0-9a-f]{64}$'),
    package_bytes BIGINT NOT NULL CHECK (package_bytes BETWEEN 1 AND 8392704),
    listing_sha256 CHAR(64) NOT NULL CHECK (listing_sha256 ~ '^[0-9a-f]{64}$'),
    listing_bytes BIGINT NOT NULL CHECK (listing_bytes BETWEEN 1 AND 32768),
    assets JSONB NOT NULL CHECK (
        CASE WHEN jsonb_typeof(assets) = 'array'
            THEN jsonb_array_length(assets) BETWEEN 2 AND 6
            ELSE FALSE
        END
    ),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    UNIQUE (app_id, version, revision)
);

CREATE TABLE review_messages (
    message_id TEXT PRIMARY KEY CHECK (message_id ~ '^msg_[0-9a-f]{32}$'),
    submission_id TEXT NOT NULL REFERENCES submissions(submission_id),
    actor_id TEXT NOT NULL CHECK (char_length(actor_id) BETWEEN 1 AND 128),
    body TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 2000),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1)
);

CREATE TABLE review_decisions (
    decision_id TEXT PRIMARY KEY CHECK (decision_id ~ '^decision_[0-9a-f]{32}$'),
    submission_id TEXT NOT NULL REFERENCES submissions(submission_id),
    reviewer_id TEXT NOT NULL CHECK (char_length(reviewer_id) BETWEEN 1 AND 128),
    decision TEXT NOT NULL CHECK (decision IN ('needs-changes', 'approved', 'rejected')),
    reason_codes TEXT[] NOT NULL CHECK (cardinality(reason_codes) <= 16),
    note TEXT NOT NULL CHECK (char_length(note) <= 2000),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1)
);

CREATE TABLE releases (
    release_id TEXT PRIMARY KEY CHECK (release_id ~ '^rel_[0-9a-f]{32}$'),
    submission_id TEXT NOT NULL REFERENCES submissions(submission_id),
    app_id TEXT NOT NULL REFERENCES apps(app_id),
    version TEXT NOT NULL CHECK (char_length(version) BETWEEN 5 AND 64),
    state TEXT NOT NULL CHECK (state IN ('ready', 'scheduled', 'publishing', 'publish-failed', 'published', 'paused', 'removed')),
    rollout_percent SMALLINT NOT NULL CHECK (rollout_percent BETWEEN 1 AND 100),
    scheduled_unix_seconds BIGINT,
    catalog_sequence BIGINT UNIQUE CHECK (catalog_sequence >= 1),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    UNIQUE (submission_id)
);

CREATE TABLE idempotency_records (
    actor_id TEXT NOT NULL CHECK (char_length(actor_id) BETWEEN 1 AND 128),
    key_sha256 CHAR(64) NOT NULL CHECK (key_sha256 ~ '^[0-9a-f]{64}$'),
    request_sha256 CHAR(64) NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    response_status SMALLINT,
    response_body JSONB,
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    expires_unix_seconds BIGINT NOT NULL CHECK (expires_unix_seconds > created_unix_seconds),
    PRIMARY KEY (actor_id, key_sha256),
    CHECK ((response_status IS NULL) = (response_body IS NULL)),
    CHECK (response_status IS NULL OR response_status BETWEEN 200 AND 599),
    CHECK (response_body IS NULL OR pg_column_size(response_body) <= 32768)
);

CREATE TABLE audit_events (
    sequence BIGSERIAL PRIMARY KEY,
    occurred_unix_seconds BIGINT NOT NULL CHECK (occurred_unix_seconds >= 1),
    actor_id TEXT NOT NULL CHECK (char_length(actor_id) BETWEEN 1 AND 128),
    action TEXT NOT NULL CHECK (action ~ '^[a-z][a-z0-9.-]{0,127}$'),
    object_kind TEXT NOT NULL CHECK (object_kind ~ '^[a-z][a-z0-9-]{0,63}$'),
    object_id TEXT NOT NULL CHECK (char_length(object_id) BETWEEN 1 AND 128),
    before_state TEXT,
    after_state TEXT,
    resource_version BIGINT NOT NULL CHECK (resource_version >= 1),
    request_id TEXT NOT NULL CHECK (char_length(request_id) BETWEEN 1 AND 128),
    request_sha256 CHAR(64) NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    idempotency_key_sha256 CHAR(64) NOT NULL CHECK (idempotency_key_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE outbox_events (
    event_id TEXT PRIMARY KEY CHECK (event_id ~ '^evt_[0-9a-f]{32}$'),
    topic TEXT NOT NULL CHECK (topic ~ '^[a-z][a-z0-9.-]{0,127}$'),
    aggregate_kind TEXT NOT NULL CHECK (aggregate_kind ~ '^[a-z][a-z0-9-]{0,63}$'),
    aggregate_id TEXT NOT NULL CHECK (char_length(aggregate_id) BETWEEN 1 AND 128),
    aggregate_version BIGINT NOT NULL CHECK (aggregate_version >= 1),
    request_sha256 CHAR(64) NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    payload JSONB NOT NULL CHECK (pg_column_size(payload) <= 32768),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    published_unix_seconds BIGINT,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    CHECK (published_unix_seconds IS NULL OR published_unix_seconds >= created_unix_seconds)
);

CREATE TABLE catalog_sequence (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    last_sequence BIGINT NOT NULL CHECK (last_sequence >= 0)
);
INSERT INTO catalog_sequence (singleton, last_sequence) VALUES (TRUE, 0);

CREATE INDEX submissions_review_queue_idx ON submissions (created_unix_seconds, submission_id)
    WHERE state = 'ready-for-review';
CREATE INDEX outbox_pending_idx ON outbox_events (created_unix_seconds, event_id)
    WHERE published_unix_seconds IS NULL;
CREATE INDEX audit_object_idx ON audit_events (object_kind, object_id, sequence);

CREATE FUNCTION reject_append_only_mutation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '% is append-only', TG_TABLE_NAME USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_events_append_only
    BEFORE UPDATE OR DELETE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();
CREATE TRIGGER review_messages_append_only
    BEFORE UPDATE OR DELETE ON review_messages
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();
CREATE TRIGGER review_decisions_append_only
    BEFORE UPDATE OR DELETE ON review_decisions
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();

CREATE FUNCTION protect_member_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.member_id <> OLD.member_id OR NEW.team_id <> OLD.team_id THEN
        RAISE EXCEPTION 'Team membership identity cannot be reassigned' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER team_members_stable_identity
    BEFORE UPDATE ON team_members
    FOR EACH ROW EXECUTE FUNCTION protect_member_identity();

CREATE FUNCTION protect_access_token() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.token_sha256, NEW.member_id, NEW.scopes, NEW.expires_unix_seconds,
        NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.token_sha256, OLD.member_id, OLD.scopes, OLD.expires_unix_seconds,
        OLD.created_unix_seconds) OR (OLD.revoked AND NOT NEW.revoked) THEN
        RAISE EXCEPTION 'Access tokens can only transition to revoked' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER access_tokens_stable_identity
    BEFORE UPDATE ON access_tokens
    FOR EACH ROW EXECUTE FUNCTION protect_access_token();

CREATE FUNCTION require_team_owner() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    affected_team_id TEXT;
BEGIN
    affected_team_id := CASE WHEN TG_OP = 'DELETE' THEN OLD.team_id ELSE NEW.team_id END;
    IF NOT EXISTS (
        SELECT 1 FROM team_members
        WHERE team_id = affected_team_id AND role = 'owner'
    ) THEN
        RAISE EXCEPTION 'A team must retain at least one Owner' USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER team_members_require_owner
    AFTER INSERT OR UPDATE OR DELETE ON team_members
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_team_owner();

CREATE FUNCTION protect_app_ownership() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'App IDs cannot be deleted or recycled' USING ERRCODE = '55000';
    END IF;
    IF NEW.app_id <> OLD.app_id OR NEW.owner_team_id <> OLD.owner_team_id THEN
        RAISE EXCEPTION 'App ID ownership is permanent' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER apps_permanent_ownership
    BEFORE UPDATE OR DELETE ON apps
    FOR EACH ROW EXECUTE FUNCTION protect_app_ownership();

CREATE FUNCTION protect_submission_content() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Submission revisions cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.submission_id, NEW.app_id, NEW.version, NEW.revision, NEW.package_sha256,
        NEW.package_bytes, NEW.listing_sha256, NEW.listing_bytes, NEW.assets,
        NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.submission_id, OLD.app_id, OLD.version, OLD.revision, OLD.package_sha256,
        OLD.package_bytes, OLD.listing_sha256, OLD.listing_bytes, OLD.assets,
        OLD.created_unix_seconds) THEN
        RAISE EXCEPTION 'Submission content is immutable' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER submissions_immutable_content
    BEFORE UPDATE OR DELETE ON submissions
    FOR EACH ROW EXECUTE FUNCTION protect_submission_content();

CREATE FUNCTION protect_release_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Releases cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.release_id, NEW.submission_id, NEW.app_id, NEW.version, NEW.created_unix_seconds)
       IS DISTINCT FROM
       (OLD.release_id, OLD.submission_id, OLD.app_id, OLD.version, OLD.created_unix_seconds) THEN
        RAISE EXCEPTION 'Release identity is immutable' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER releases_immutable_identity
    BEFORE UPDATE OR DELETE ON releases
    FOR EACH ROW EXECUTE FUNCTION protect_release_identity();
