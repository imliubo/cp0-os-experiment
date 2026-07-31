CREATE TABLE release_operations (
    operation_id TEXT PRIMARY KEY CHECK (operation_id ~ '^releaseop_[0-9a-f]{32}$'),
    release_id TEXT NOT NULL REFERENCES releases(release_id),
    actor_id TEXT NOT NULL REFERENCES team_members(member_id),
    action TEXT NOT NULL CHECK (action IN ('schedule', 'publish', 'pause', 'resume', 'remove')),
    before_state TEXT NOT NULL CHECK (before_state IN (
        'ready', 'scheduled', 'publishing', 'publish-failed', 'published', 'paused', 'removed'
    )),
    after_state TEXT NOT NULL CHECK (after_state IN (
        'ready', 'scheduled', 'publishing', 'publish-failed', 'published', 'paused', 'removed'
    )),
    resource_version BIGINT NOT NULL CHECK (resource_version >= 2),
    request_sha256 CHAR(64) NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    details JSONB NOT NULL CHECK (jsonb_typeof(details) = 'object' AND pg_column_size(details) <= 8192),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    UNIQUE (release_id, resource_version)
);

CREATE FUNCTION enforce_release_creation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.state <> 'ready' OR NEW.resource_version <> 1 OR
       NEW.scheduled_unix_seconds IS NOT NULL OR NEW.catalog_sequence IS NOT NULL OR
       NOT EXISTS (
           SELECT 1 FROM submissions submission
           WHERE submission.submission_id = NEW.submission_id
             AND submission.app_id = NEW.app_id
             AND submission.version = NEW.version
             AND submission.state = 'approved'
       ) THEN
        RAISE EXCEPTION 'Release must bind an approved immutable Submission'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER releases_approved_creation
    BEFORE INSERT ON releases
    FOR EACH ROW EXECUTE FUNCTION enforce_release_creation();

CREATE FUNCTION enforce_release_operation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT (
           (NEW.action = 'schedule' AND NEW.before_state = 'ready' AND
            NEW.after_state = 'scheduled') OR
           (NEW.action = 'publish' AND NEW.before_state IN ('ready', 'scheduled', 'publish-failed') AND
            NEW.after_state = 'publishing') OR
           (NEW.action = 'pause' AND NEW.before_state = 'published' AND
            NEW.after_state = 'paused') OR
           (NEW.action = 'resume' AND NEW.before_state = 'paused' AND
            NEW.after_state = 'published') OR
           (NEW.action = 'remove' AND
            NEW.before_state IN ('ready', 'scheduled', 'publish-failed', 'published', 'paused') AND
            NEW.after_state = 'removed')
       ) OR NOT EXISTS (
           SELECT 1 FROM releases release
           JOIN apps app ON app.app_id = release.app_id
           JOIN team_members member ON member.member_id = NEW.actor_id
           WHERE release.release_id = NEW.release_id
             AND release.state = NEW.after_state
             AND release.resource_version = NEW.resource_version
             AND member.team_id = app.owner_team_id
       ) THEN
        RAISE EXCEPTION 'Release operation does not match the committed transition'
            USING ERRCODE = '55000';
    END IF;

    IF NEW.action = 'schedule' THEN
        IF NOT (NEW.details ? 'publish_unix_seconds') OR
           (NEW.details - 'publish_unix_seconds') <> '{}'::JSONB OR
           jsonb_typeof(NEW.details->'publish_unix_seconds') <> 'number' OR
           NEW.details->>'publish_unix_seconds' !~ '^[1-9][0-9]{0,18}$' OR
           (NEW.details->>'publish_unix_seconds')::NUMERIC > 9223372036854775807 THEN
            RAISE EXCEPTION 'Release schedule details are invalid' USING ERRCODE = '55000';
        END IF;
    ELSIF NEW.action = 'remove' THEN
        IF NOT (NEW.details ? 'reason_code') OR NOT (NEW.details ? 'note') OR
           (NEW.details - 'reason_code' - 'note') <> '{}'::JSONB OR
           jsonb_typeof(NEW.details->'reason_code') <> 'string' OR
           NEW.details->>'reason_code' !~ '^[a-z][a-z0-9-]{0,63}$' OR
           jsonb_typeof(NEW.details->'note') <> 'string' OR
           char_length(NEW.details->>'note') NOT BETWEEN 1 AND 2000 OR
           (NEW.details->>'note') <> btrim(NEW.details->>'note') THEN
            RAISE EXCEPTION 'Release removal details are invalid' USING ERRCODE = '55000';
        END IF;
    ELSIF NEW.details <> '{}'::JSONB THEN
        RAISE EXCEPTION 'Release operation details must be empty' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER release_operations_match_transition
    BEFORE INSERT ON release_operations
    FOR EACH ROW EXECUTE FUNCTION enforce_release_operation();

CREATE TRIGGER release_operations_append_only
    BEFORE UPDATE OR DELETE ON release_operations
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();

CREATE FUNCTION protect_release_state_machine() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.rollout_percent <> OLD.rollout_percent OR
       NEW.resource_version <> OLD.resource_version + 1 OR
       NOT (
           (OLD.state = 'ready' AND NEW.state IN ('scheduled', 'publishing', 'removed')) OR
           (OLD.state = 'scheduled' AND NEW.state IN ('publishing', 'removed')) OR
           (OLD.state = 'publishing' AND NEW.state IN ('published', 'publish-failed')) OR
           (OLD.state = 'publish-failed' AND NEW.state IN ('publishing', 'removed')) OR
           (OLD.state = 'published' AND NEW.state IN ('paused', 'removed')) OR
           (OLD.state = 'paused' AND NEW.state IN ('published', 'removed'))
       ) THEN
        RAISE EXCEPTION 'Release transition is invalid' USING ERRCODE = '55000';
    END IF;

    IF (NEW.state = 'scheduled') <> (NEW.scheduled_unix_seconds IS NOT NULL) OR
       (NEW.state IN ('ready', 'scheduled', 'publishing', 'publish-failed') AND
        NEW.catalog_sequence IS NOT NULL) OR
       (NEW.state IN ('published', 'paused') AND NEW.catalog_sequence IS NULL) OR
       (NEW.state = 'removed' AND NEW.scheduled_unix_seconds IS NOT NULL) THEN
        RAISE EXCEPTION 'Release state metadata is inconsistent' USING ERRCODE = '55000';
    END IF;

    IF NEW.state IN ('paused', 'published') AND
       NEW.catalog_sequence IS DISTINCT FROM OLD.catalog_sequence AND
       OLD.state <> 'publishing' THEN
        RAISE EXCEPTION 'Published Catalog sequence is immutable' USING ERRCODE = '55000';
    END IF;
    IF NEW.state = 'removed' AND NEW.catalog_sequence IS DISTINCT FROM OLD.catalog_sequence THEN
        RAISE EXCEPTION 'Removed Release must retain its Catalog sequence' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER releases_state_machine
    BEFORE UPDATE ON releases
    FOR EACH ROW EXECUTE FUNCTION protect_release_state_machine();
