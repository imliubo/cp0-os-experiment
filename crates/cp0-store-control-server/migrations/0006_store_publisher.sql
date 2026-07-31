CREATE TABLE store_publication_jobs (
    event_id TEXT PRIMARY KEY REFERENCES outbox_events(event_id),
    release_id TEXT NOT NULL REFERENCES releases(release_id),
    job_kind TEXT NOT NULL CHECK (job_kind IN ('publish-release', 'rebuild-catalog')),
    source_resource_version BIGINT NOT NULL CHECK (source_resource_version >= 1),
    source_state TEXT NOT NULL CHECK (source_state IN ('publishing', 'published', 'paused', 'removed')),
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'completed', 'failed', 'superseded')),
    lease_token TEXT CHECK (lease_token IS NULL OR lease_token ~ '^lease_[0-9a-f]{32}$'),
    leased_until_unix_seconds BIGINT,
    attempts SMALLINT NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 8),
    catalog_sequence BIGINT UNIQUE CHECK (catalog_sequence >= 1),
    published_unix_seconds BIGINT CHECK (published_unix_seconds >= 1),
    expires_unix_seconds BIGINT,
    last_error_code TEXT CHECK (
        last_error_code IS NULL OR last_error_code ~ '^[a-z][a-z0-9.-]{0,63}$'
    ),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    completed_unix_seconds BIGINT,
    CHECK (
        (job_kind = 'publish-release' AND source_state = 'publishing') OR
        (job_kind = 'rebuild-catalog' AND source_state IN ('published', 'paused', 'removed'))
    ),
    CHECK (
        (catalog_sequence IS NULL AND published_unix_seconds IS NULL AND expires_unix_seconds IS NULL AND attempts = 0) OR
        (catalog_sequence IS NOT NULL AND published_unix_seconds IS NOT NULL AND
         expires_unix_seconds > published_unix_seconds AND attempts >= 1)
    ),
    CHECK (
        (state = 'running' AND lease_token IS NOT NULL AND leased_until_unix_seconds IS NOT NULL AND
         completed_unix_seconds IS NULL) OR
        (state = 'queued' AND lease_token IS NULL AND leased_until_unix_seconds IS NULL AND
         completed_unix_seconds IS NULL) OR
        (state IN ('completed', 'failed', 'superseded') AND lease_token IS NULL AND
         leased_until_unix_seconds IS NULL AND completed_unix_seconds IS NOT NULL)
    )
);

CREATE UNIQUE INDEX store_publication_jobs_one_running_idx
    ON store_publication_jobs ((TRUE)) WHERE state = 'running';
CREATE INDEX store_publication_jobs_claim_idx
    ON store_publication_jobs (created_unix_seconds, event_id)
    WHERE state IN ('queued', 'running');

CREATE FUNCTION enforce_store_publication_job_insert() RETURNS trigger LANGUAGE plpgsql AS $$
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

CREATE TABLE store_package_artifacts (
    release_id TEXT PRIMARY KEY REFERENCES releases(release_id),
    submission_id TEXT NOT NULL UNIQUE REFERENCES submissions(submission_id),
    catalog_sequence BIGINT NOT NULL UNIQUE CHECK (catalog_sequence >= 1),
    package_sha256 CHAR(64) NOT NULL CHECK (package_sha256 ~ '^[0-9a-f]{64}$'),
    package_bytes BIGINT NOT NULL CHECK (package_bytes BETWEEN 1 AND 33558528),
    relative_path TEXT NOT NULL UNIQUE CHECK (
        relative_path ~ '^generations/[1-9][0-9]{0,18}/packages/rel_[0-9a-f]{32}\.capp$'
    ),
    store_key_id CHAR(64) NOT NULL CHECK (store_key_id ~ '^[0-9a-f]{64}$'),
    catalog_app JSONB NOT NULL CHECK (
        jsonb_typeof(catalog_app) = 'object' AND pg_column_size(catalog_app) <= 8192
    ),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1)
);

CREATE TABLE store_catalog_snapshots (
    sequence BIGINT PRIMARY KEY CHECK (sequence >= 1),
    source_event_id TEXT NOT NULL UNIQUE REFERENCES store_publication_jobs(event_id),
    source_release_id TEXT NOT NULL REFERENCES releases(release_id),
    catalog_sha256 CHAR(64) NOT NULL CHECK (catalog_sha256 ~ '^[0-9a-f]{64}$'),
    catalog_bytes INTEGER NOT NULL CHECK (catalog_bytes BETWEEN 1 AND 49152),
    relative_path TEXT NOT NULL UNIQUE CHECK (
        relative_path ~ '^generations/[1-9][0-9]{0,18}/catalog\.json$'
    ),
    store_key_id CHAR(64) NOT NULL CHECK (store_key_id ~ '^[0-9a-f]{64}$'),
    app_count SMALLINT NOT NULL CHECK (app_count BETWEEN 0 AND 64),
    published_unix_seconds BIGINT NOT NULL CHECK (published_unix_seconds >= 1),
    expires_unix_seconds BIGINT NOT NULL CHECK (expires_unix_seconds > published_unix_seconds),
    encoded_catalog BYTEA NOT NULL CHECK (
        octet_length(encoded_catalog) BETWEEN 1 AND 49152 AND
        octet_length(encoded_catalog) = catalog_bytes
    ),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1)
);

CREATE FUNCTION protect_catalog_sequence_counter() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' OR NEW.singleton IS DISTINCT FROM OLD.singleton OR
       NEW.last_sequence <> OLD.last_sequence + 1 THEN
        RAISE EXCEPTION 'Catalog sequence must advance exactly once' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER catalog_sequence_monotonic_counter
    BEFORE UPDATE OR DELETE ON catalog_sequence
    FOR EACH ROW EXECUTE FUNCTION protect_catalog_sequence_counter();

CREATE FUNCTION protect_store_publication_job() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Store publication jobs cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.event_id, NEW.release_id, NEW.job_kind, NEW.source_resource_version,
        NEW.source_state, NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.event_id, OLD.release_id, OLD.job_kind, OLD.source_resource_version,
        OLD.source_state, OLD.created_unix_seconds) OR
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
    IF NEW.state = 'completed' AND NOT EXISTS (
        SELECT 1 FROM store_catalog_snapshots snapshot
        WHERE snapshot.source_event_id = NEW.event_id AND snapshot.sequence = NEW.catalog_sequence
    ) THEN
        RAISE EXCEPTION 'Completed publication job requires its Catalog snapshot' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_publication_jobs_state_machine
    BEFORE UPDATE OR DELETE ON store_publication_jobs
    FOR EACH ROW EXECUTE FUNCTION protect_store_publication_job();

CREATE FUNCTION enforce_store_package_artifact() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM store_publication_jobs job
        JOIN releases release ON release.release_id = job.release_id
        WHERE job.release_id = NEW.release_id
          AND job.job_kind = 'publish-release'
          AND job.state = 'running'
          AND job.catalog_sequence = NEW.catalog_sequence
          AND release.submission_id = NEW.submission_id
          AND release.state = 'publishing'
          AND release.resource_version = job.source_resource_version
    ) OR NEW.relative_path <> format(
        'generations/%s/packages/%s.capp', NEW.catalog_sequence, NEW.release_id
    ) OR NEW.catalog_app->>'app_id' <> (
        SELECT app_id FROM releases WHERE release_id = NEW.release_id
    ) OR NEW.catalog_app->>'version' <> (
        SELECT version FROM releases WHERE release_id = NEW.release_id
    ) OR NEW.catalog_app->>'package_sha256' <> NEW.package_sha256 OR
       jsonb_typeof(NEW.catalog_app->'package_bytes') <> 'number' OR
       NEW.catalog_app->>'package_bytes' !~ '^[1-9][0-9]{0,9}$' OR
       (NEW.catalog_app->>'package_bytes')::BIGINT <> NEW.package_bytes THEN
        RAISE EXCEPTION 'Package artifact does not match an active publication lease'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_package_artifacts_authorized_insert
    BEFORE INSERT ON store_package_artifacts
    FOR EACH ROW EXECUTE FUNCTION enforce_store_package_artifact();
CREATE TRIGGER store_package_artifacts_append_only
    BEFORE UPDATE OR DELETE ON store_package_artifacts
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();

CREATE FUNCTION enforce_store_catalog_snapshot() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM store_publication_jobs job
        WHERE job.event_id = NEW.source_event_id
          AND job.release_id = NEW.source_release_id
          AND job.state = 'running'
          AND job.catalog_sequence = NEW.sequence
          AND job.published_unix_seconds = NEW.published_unix_seconds
          AND job.expires_unix_seconds = NEW.expires_unix_seconds
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
CREATE TRIGGER store_catalog_snapshots_append_only
    BEFORE UPDATE OR DELETE ON store_catalog_snapshots
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();
