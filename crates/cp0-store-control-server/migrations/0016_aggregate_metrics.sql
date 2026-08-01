CREATE TABLE store_metric_batches (
    batch_id TEXT PRIMARY KEY CHECK (batch_id ~ '^batch_[0-9a-f]{32}$'),
    week_start_unix_seconds BIGINT NOT NULL CHECK (
        week_start_unix_seconds >= 345600 AND
        (week_start_unix_seconds - 345600) % 604800 = 0
    ),
    report_sha256 CHAR(64) NOT NULL CHECK (report_sha256 ~ '^[0-9a-f]{64}$'),
    received_unix_seconds BIGINT NOT NULL CHECK (received_unix_seconds >= 1),
    expires_unix_seconds BIGINT NOT NULL CHECK (
        expires_unix_seconds > received_unix_seconds AND
        expires_unix_seconds = received_unix_seconds + 1296000
    )
);

CREATE TABLE store_metric_aggregates (
    week_start_unix_seconds BIGINT NOT NULL CHECK (
        week_start_unix_seconds >= 345600 AND
        (week_start_unix_seconds - 345600) % 604800 = 0
    ),
    app_id TEXT NOT NULL REFERENCES apps(app_id),
    version TEXT NOT NULL CHECK (
        char_length(version) BETWEEN 5 AND 64 AND
        version ~ '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)([-+][0-9A-Za-z.-]+)*$'
    ),
    batch_count BIGINT NOT NULL CHECK (batch_count >= 1),
    install_count BIGINT NOT NULL CHECK (install_count >= 0),
    launch_count BIGINT NOT NULL CHECK (launch_count >= 0),
    crash_count BIGINT NOT NULL CHECK (
        crash_count >= 0 AND crash_count <= launch_count
    ),
    updated_unix_seconds BIGINT NOT NULL CHECK (updated_unix_seconds >= 1),
    PRIMARY KEY (week_start_unix_seconds, app_id, version)
);

CREATE FUNCTION protect_store_metric_batch() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        RAISE EXCEPTION 'Store metric batch identity is immutable' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'DELETE' AND OLD.expires_unix_seconds >
       EXTRACT(EPOCH FROM clock_timestamp())::BIGINT THEN
        RAISE EXCEPTION 'Live Store metric batch cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_metric_batches_lifecycle
    BEFORE UPDATE OR DELETE ON store_metric_batches
    FOR EACH ROW EXECUTE FUNCTION protect_store_metric_batch();

CREATE FUNCTION metric_identity_has_published_artifact(
    app_id_value TEXT,
    version_value TEXT
) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM store_package_artifacts artifact
        JOIN releases release ON release.release_id = artifact.release_id
        WHERE release.app_id = app_id_value
          AND release.version = version_value
          AND artifact.catalog_app->>'app_id' = app_id_value
          AND artifact.catalog_app->>'version' = version_value
    );
$$;

CREATE FUNCTION protect_store_metric_aggregate() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' OR
       NOT metric_identity_has_published_artifact(NEW.app_id, NEW.version) THEN
        RAISE EXCEPTION 'Store metric aggregate identity is not publishable'
            USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.batch_count <> 1 OR NEW.install_count > 8 OR
           NEW.launch_count > 4096 OR NEW.crash_count > NEW.launch_count THEN
            RAISE EXCEPTION 'Initial Store metric contribution is invalid'
                USING ERRCODE = '55000';
        END IF;
    ELSIF (NEW.week_start_unix_seconds, NEW.app_id, NEW.version) IS DISTINCT FROM
          (OLD.week_start_unix_seconds, OLD.app_id, OLD.version) OR
          NEW.batch_count <> OLD.batch_count + 1 OR
          NEW.install_count NOT BETWEEN OLD.install_count AND OLD.install_count + 8 OR
          NEW.launch_count NOT BETWEEN OLD.launch_count AND OLD.launch_count + 4096 OR
          NEW.crash_count NOT BETWEEN OLD.crash_count AND OLD.crash_count + 4096 OR
          NEW.updated_unix_seconds < OLD.updated_unix_seconds THEN
        RAISE EXCEPTION 'Store metric aggregate transition is invalid'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_metric_aggregates_monotonic
    BEFORE INSERT OR UPDATE OR DELETE ON store_metric_aggregates
    FOR EACH ROW EXECUTE FUNCTION protect_store_metric_aggregate();

CREATE VIEW store_public_metric_aggregates AS
SELECT week_start_unix_seconds, app_id, version, batch_count,
       install_count, launch_count, crash_count
FROM store_metric_aggregates
WHERE batch_count >= 20;
