CREATE TABLE store_operators (
    operator_id TEXT PRIMARY KEY CHECK (operator_id ~ '^operator_[0-9a-f]{32}$'),
    email TEXT NOT NULL UNIQUE CHECK (
        char_length(email) BETWEEN 3 AND 254 AND email = btrim(email) AND email = lower(email)
    ),
    role TEXT NOT NULL CHECK (role IN ('editor', 'admin')),
    two_factor_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    state TEXT NOT NULL CHECK (state IN ('active', 'suspended')),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1)
);

CREATE TABLE store_operator_access_tokens (
    token_sha256 CHAR(64) PRIMARY KEY CHECK (token_sha256 ~ '^[0-9a-f]{64}$'),
    operator_id TEXT NOT NULL REFERENCES store_operators(operator_id),
    scopes TEXT[] NOT NULL CHECK (scopes = ARRAY['store.editorial']::TEXT[]),
    expires_unix_seconds BIGINT NOT NULL CHECK (expires_unix_seconds >= 1),
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    CHECK (expires_unix_seconds > created_unix_seconds AND
           expires_unix_seconds <= created_unix_seconds + 3600)
);

DROP TRIGGER access_tokens_cross_domain_unique ON access_tokens;
DROP TRIGGER reviewer_access_tokens_cross_domain_unique ON reviewer_access_tokens;

CREATE OR REPLACE FUNCTION enforce_token_domain_uniqueness() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(NEW.token_sha256, 0));
    IF TG_TABLE_NAME = 'access_tokens' AND (
        EXISTS (SELECT 1 FROM reviewer_access_tokens WHERE token_sha256 = NEW.token_sha256) OR
        EXISTS (SELECT 1 FROM store_operator_access_tokens WHERE token_sha256 = NEW.token_sha256)
    ) THEN
        RAISE EXCEPTION 'Access token digest already belongs to another identity domain'
            USING ERRCODE = '23505';
    ELSIF TG_TABLE_NAME = 'reviewer_access_tokens' AND (
        EXISTS (SELECT 1 FROM access_tokens WHERE token_sha256 = NEW.token_sha256) OR
        EXISTS (SELECT 1 FROM store_operator_access_tokens WHERE token_sha256 = NEW.token_sha256)
    ) THEN
        RAISE EXCEPTION 'Reviewer token digest already belongs to another identity domain'
            USING ERRCODE = '23505';
    ELSIF TG_TABLE_NAME = 'store_operator_access_tokens' AND (
        EXISTS (SELECT 1 FROM access_tokens WHERE token_sha256 = NEW.token_sha256) OR
        EXISTS (SELECT 1 FROM reviewer_access_tokens WHERE token_sha256 = NEW.token_sha256)
    ) THEN
        RAISE EXCEPTION 'Operator token digest already belongs to another identity domain'
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
CREATE TRIGGER store_operator_access_tokens_cross_domain_unique
    BEFORE INSERT OR UPDATE ON store_operator_access_tokens
    FOR EACH ROW EXECUTE FUNCTION enforce_token_domain_uniqueness();

CREATE FUNCTION protect_store_operator() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' OR
       (NEW.operator_id, NEW.email, NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.operator_id, OLD.email, OLD.created_unix_seconds) OR
       NEW.resource_version <= OLD.resource_version THEN
        RAISE EXCEPTION 'Store operator identity is immutable' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_operators_stable_identity
    BEFORE UPDATE OR DELETE ON store_operators
    FOR EACH ROW EXECUTE FUNCTION protect_store_operator();

CREATE FUNCTION protect_store_operator_access_token() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' OR
       (NEW.token_sha256, NEW.operator_id, NEW.scopes, NEW.expires_unix_seconds,
        NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.token_sha256, OLD.operator_id, OLD.scopes, OLD.expires_unix_seconds,
        OLD.created_unix_seconds) OR (OLD.revoked AND NOT NEW.revoked) THEN
        RAISE EXCEPTION 'Store operator tokens can only transition to revoked'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_operator_access_tokens_stable_identity
    BEFORE UPDATE OR DELETE ON store_operator_access_tokens
    FOR EACH ROW EXECUTE FUNCTION protect_store_operator_access_token();

CREATE TABLE store_editorial_layouts (
    layout_id TEXT PRIMARY KEY CHECK (layout_id = 'today'),
    headline TEXT NOT NULL CHECK (
        char_length(headline) BETWEEN 1 AND 48 AND headline = btrim(headline) AND
        headline !~ '[[:cntrl:]]'
    ),
    featured_release_id TEXT NOT NULL REFERENCES releases(release_id),
    featured_app_id TEXT NOT NULL REFERENCES apps(app_id),
    collections JSONB NOT NULL CHECK (
        jsonb_typeof(collections) = 'array' AND jsonb_array_length(collections) BETWEEN 1 AND 2 AND
        pg_column_size(collections) <= 4096
    ),
    resource_version BIGINT NOT NULL CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    updated_unix_seconds BIGINT NOT NULL CHECK (updated_unix_seconds >= created_unix_seconds)
);

CREATE TABLE store_editorial_revisions (
    layout_id TEXT NOT NULL CHECK (layout_id = 'today'),
    resource_version BIGINT NOT NULL CHECK (resource_version >= 1),
    operator_id TEXT NOT NULL REFERENCES store_operators(operator_id),
    headline TEXT NOT NULL CHECK (
        char_length(headline) BETWEEN 1 AND 48 AND headline = btrim(headline) AND
        headline !~ '[[:cntrl:]]'
    ),
    featured_release_id TEXT NOT NULL REFERENCES releases(release_id),
    featured_app_id TEXT NOT NULL REFERENCES apps(app_id),
    collections JSONB NOT NULL CHECK (
        jsonb_typeof(collections) = 'array' AND jsonb_array_length(collections) BETWEEN 1 AND 2 AND
        pg_column_size(collections) <= 4096
    ),
    request_sha256 CHAR(64) NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    PRIMARY KEY (layout_id, resource_version)
);

CREATE FUNCTION editorial_release_is_publishable(release_id_value TEXT, app_id_value TEXT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM releases release
        JOIN submissions submission ON submission.submission_id = release.submission_id
        WHERE release.release_id = release_id_value
          AND release.app_id = app_id_value
          AND release.state = 'published'
          AND submission.state = 'approved'
          AND submission.app_id = release.app_id
          AND submission.version = release.version
    );
$$;

CREATE FUNCTION editorial_collections_are_valid(
    collections_value JSONB,
    featured_release_id_value TEXT,
    featured_app_id_value TEXT
) RETURNS BOOLEAN LANGUAGE plpgsql STABLE AS $$
DECLARE
    collection JSONB;
    item JSONB;
    title_value TEXT;
    release_id_value TEXT;
    app_id_value TEXT;
    seen_titles TEXT[] := ARRAY[]::TEXT[];
    seen_releases TEXT[] := ARRAY[featured_release_id_value];
    seen_apps TEXT[] := ARRAY[featured_app_id_value];
BEGIN
    IF jsonb_typeof(collections_value) <> 'array' OR
       jsonb_array_length(collections_value) NOT BETWEEN 1 AND 2 THEN
        RETURN FALSE;
    END IF;
    FOR collection IN SELECT value FROM jsonb_array_elements(collections_value) LOOP
        IF jsonb_typeof(collection) <> 'object' OR
           (SELECT COUNT(*) FROM jsonb_object_keys(collection)) <> 2 OR
           NOT (collection ? 'title') OR NOT (collection ? 'items') OR
           jsonb_typeof(collection->'title') <> 'string' OR
           jsonb_typeof(collection->'items') <> 'array' OR
           jsonb_array_length(collection->'items') NOT BETWEEN 1 AND 4 THEN
            RETURN FALSE;
        END IF;
        title_value := collection->>'title';
        IF char_length(title_value) NOT BETWEEN 1 AND 32 OR title_value <> btrim(title_value) OR
           title_value ~ '[[:cntrl:]]' OR title_value = ANY(seen_titles) THEN
            RETURN FALSE;
        END IF;
        seen_titles := array_append(seen_titles, title_value);
        FOR item IN SELECT value FROM jsonb_array_elements(collection->'items') LOOP
            IF jsonb_typeof(item) <> 'object' OR
               (SELECT COUNT(*) FROM jsonb_object_keys(item)) <> 2 OR
               NOT (item ? 'release_id') OR NOT (item ? 'app_id') OR
               jsonb_typeof(item->'release_id') <> 'string' OR
               jsonb_typeof(item->'app_id') <> 'string' THEN
                RETURN FALSE;
            END IF;
            release_id_value := item->>'release_id';
            app_id_value := item->>'app_id';
            IF release_id_value !~ '^rel_[0-9a-f]{32}$' OR
               app_id_value !~ '^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*){2,}$' OR
               release_id_value = ANY(seen_releases) OR app_id_value = ANY(seen_apps) OR
               NOT editorial_release_is_publishable(release_id_value, app_id_value) THEN
                RETURN FALSE;
            END IF;
            seen_releases := array_append(seen_releases, release_id_value);
            seen_apps := array_append(seen_apps, app_id_value);
        END LOOP;
    END LOOP;
    RETURN TRUE;
END;
$$;

CREATE FUNCTION protect_store_editorial_layout() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Store editorial layout cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF NOT editorial_release_is_publishable(NEW.featured_release_id, NEW.featured_app_id) OR
       NOT editorial_collections_are_valid(
           NEW.collections, NEW.featured_release_id, NEW.featured_app_id
       ) THEN
        RAISE EXCEPTION 'Editorial layout must reference distinct approved published Releases'
            USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.resource_version <> 1 OR NEW.created_unix_seconds <> NEW.updated_unix_seconds THEN
            RAISE EXCEPTION 'Initial editorial layout version is invalid' USING ERRCODE = '55000';
        END IF;
    ELSIF NEW.layout_id <> OLD.layout_id OR
          NEW.created_unix_seconds <> OLD.created_unix_seconds OR
          NEW.resource_version <> OLD.resource_version + 1 OR
          NEW.updated_unix_seconds < OLD.updated_unix_seconds THEN
        RAISE EXCEPTION 'Editorial layout transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_editorial_layout_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON store_editorial_layouts
    FOR EACH ROW EXECUTE FUNCTION protect_store_editorial_layout();

CREATE FUNCTION protect_store_editorial_revision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'Store editorial revisions are append-only' USING ERRCODE = '55000';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM store_editorial_layouts layout
        WHERE layout.layout_id = NEW.layout_id
          AND layout.resource_version = NEW.resource_version
          AND layout.headline = NEW.headline
          AND layout.featured_release_id = NEW.featured_release_id
          AND layout.featured_app_id = NEW.featured_app_id
          AND layout.collections = NEW.collections
          AND layout.updated_unix_seconds = NEW.created_unix_seconds
    ) THEN
        RAISE EXCEPTION 'Editorial revision does not match the committed layout'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_editorial_revisions_append_only
    BEFORE INSERT OR UPDATE OR DELETE ON store_editorial_revisions
    FOR EACH ROW EXECUTE FUNCTION protect_store_editorial_revision();

CREATE FUNCTION require_store_editorial_mutation_records() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    revision store_editorial_revisions%ROWTYPE;
BEGIN
    SELECT * INTO revision
      FROM store_editorial_revisions stored
     WHERE stored.layout_id = NEW.layout_id
       AND stored.resource_version = NEW.resource_version;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Editorial layout requires an immutable matching revision'
            USING ERRCODE = '55000';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM audit_events audit
        WHERE audit.actor_id = revision.operator_id
          AND audit.action = CASE WHEN NEW.resource_version = 1
                                  THEN 'editorial.today-created'
                                  ELSE 'editorial.today-updated' END
          AND audit.object_kind = 'editorial'
          AND audit.object_id = NEW.layout_id
          AND audit.before_state IS NULL
          AND audit.after_state = 'active'
          AND audit.resource_version = NEW.resource_version
          AND audit.request_sha256 = revision.request_sha256
          AND audit.occurred_unix_seconds = revision.created_unix_seconds
    ) THEN
        RAISE EXCEPTION 'Editorial layout requires a matching audit event'
            USING ERRCODE = '55000';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM outbox_events event
        JOIN releases release ON release.release_id = revision.featured_release_id
        WHERE event.topic = 'catalog.rebuild-requested'
          AND event.aggregate_kind = 'release'
          AND event.aggregate_id = revision.featured_release_id
          AND event.aggregate_version = release.resource_version
          AND event.request_sha256 = revision.request_sha256
          AND event.created_unix_seconds = revision.created_unix_seconds
          AND event.payload = jsonb_build_object(
              'release_id', revision.featured_release_id,
              'app_id', revision.featured_app_id,
              'state', 'published',
              'editorial_resource_version', revision.resource_version
          )
    ) THEN
        RAISE EXCEPTION 'Editorial layout requires a matching Catalog rebuild event'
            USING ERRCODE = '55000';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER store_editorial_layout_requires_mutation_records
    AFTER INSERT OR UPDATE ON store_editorial_layouts
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_store_editorial_mutation_records();

ALTER TABLE store_publication_jobs
    ADD COLUMN editorial_resource_version BIGINT CHECK (editorial_resource_version >= 1);
ALTER TABLE store_catalog_snapshots
    ADD COLUMN editorial_resource_version BIGINT CHECK (editorial_resource_version >= 1);

DROP TRIGGER store_publication_jobs_authorized_insert ON store_publication_jobs;
CREATE OR REPLACE FUNCTION enforce_store_publication_job_insert() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM outbox_events event
        WHERE event.event_id = NEW.event_id
          AND event.aggregate_kind = 'release'
          AND event.aggregate_id = NEW.release_id
          AND event.aggregate_version = NEW.source_resource_version
          AND event.payload->>'release_id' = NEW.release_id
          AND event.payload->>'state' = NEW.source_state
          AND (
              (NEW.job_kind = 'publish-release' AND event.topic = 'release.publish-requested') OR
              (NEW.job_kind = 'rebuild-catalog' AND event.topic = 'catalog.rebuild-requested')
          )
          AND (
              (NEW.editorial_resource_version IS NULL AND
               NOT (event.payload ? 'editorial_resource_version')) OR
              (NEW.editorial_resource_version IS NOT NULL AND
               event.payload->>'editorial_resource_version' =
                   NEW.editorial_resource_version::TEXT)
          )
    ) THEN
        RAISE EXCEPTION 'Store publication job does not match its transaction outbox event'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_publication_jobs_authorized_insert
    BEFORE INSERT ON store_publication_jobs
    FOR EACH ROW EXECUTE FUNCTION enforce_store_publication_job_insert();

DROP TRIGGER store_catalog_snapshots_authorized_insert ON store_catalog_snapshots;
CREATE OR REPLACE FUNCTION enforce_store_catalog_snapshot() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM store_publication_jobs job
        WHERE job.event_id = NEW.source_event_id
          AND job.release_id = NEW.source_release_id
          AND job.state = 'running'
          AND job.catalog_sequence = NEW.sequence
          AND job.published_unix_seconds = NEW.published_unix_seconds
          AND job.expires_unix_seconds = NEW.expires_unix_seconds
          AND job.editorial_resource_version IS NOT DISTINCT FROM
              NEW.editorial_resource_version
    ) OR NEW.relative_path <> format('generations/%s/catalog.json', NEW.sequence) THEN
        RAISE EXCEPTION 'Catalog snapshot does not match an active publication lease'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_catalog_snapshots_authorized_insert
    BEFORE INSERT ON store_catalog_snapshots
    FOR EACH ROW EXECUTE FUNCTION enforce_store_catalog_snapshot();

DROP TRIGGER store_publication_jobs_state_machine ON store_publication_jobs;
CREATE OR REPLACE FUNCTION protect_store_publication_job() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    expected_developer_name TEXT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Store publication jobs cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.event_id, NEW.release_id, NEW.job_kind, NEW.source_resource_version,
        NEW.source_state, NEW.created_unix_seconds, NEW.editorial_resource_version) IS DISTINCT FROM
       (OLD.event_id, OLD.release_id, OLD.job_kind, OLD.source_resource_version,
        OLD.source_state, OLD.created_unix_seconds, OLD.editorial_resource_version) OR
       (OLD.developer_name IS NOT NULL AND NEW.developer_name IS DISTINCT FROM OLD.developer_name) OR
       (OLD.catalog_sequence IS NOT NULL AND
        (NEW.catalog_sequence, NEW.published_unix_seconds, NEW.expires_unix_seconds) IS DISTINCT FROM
        (OLD.catalog_sequence, OLD.published_unix_seconds, OLD.expires_unix_seconds)) OR
       NEW.attempts < OLD.attempts OR OLD.state IN ('completed', 'failed', 'superseded') OR
       NOT (
           (OLD.state = 'queued' AND NEW.state = 'running' AND NEW.attempts = OLD.attempts + 1) OR
           (OLD.state = 'running' AND NEW.state IN ('queued', 'completed', 'failed', 'superseded') AND
            NEW.attempts = OLD.attempts)
       ) THEN
        RAISE EXCEPTION 'Store publication job transition is invalid' USING ERRCODE = '55000';
    END IF;
    IF OLD.attempts = 0 AND NEW.state = 'running' THEN
        SELECT team.name INTO expected_developer_name
          FROM releases release
          JOIN apps app ON app.app_id = release.app_id
          JOIN teams team ON team.team_id = app.owner_team_id
         WHERE release.release_id = NEW.release_id;
        IF expected_developer_name IS NULL OR
           NEW.developer_name IS DISTINCT FROM expected_developer_name THEN
            RAISE EXCEPTION 'Publication developer snapshot is invalid' USING ERRCODE = '55000';
        END IF;
    END IF;
    IF NEW.state = 'completed' AND (
        NOT EXISTS (
            SELECT 1 FROM store_catalog_snapshots snapshot
            WHERE snapshot.source_event_id = NEW.event_id
              AND snapshot.sequence = NEW.catalog_sequence
              AND snapshot.editorial_resource_version IS NOT DISTINCT FROM
                  NEW.editorial_resource_version
        ) OR NOT EXISTS (
            SELECT 1 FROM store_transparency_leaves leaf
            JOIN store_transparency_checkpoints checkpoint
              ON checkpoint.tree_size = leaf.tree_index + 1
            WHERE leaf.catalog_sequence = NEW.catalog_sequence
              AND checkpoint.catalog_sequence = NEW.catalog_sequence
        )
    ) THEN
        RAISE EXCEPTION 'Completed publication job requires Catalog and transparency records'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_publication_jobs_state_machine
    BEFORE UPDATE OR DELETE ON store_publication_jobs
    FOR EACH ROW EXECUTE FUNCTION protect_store_publication_job();
