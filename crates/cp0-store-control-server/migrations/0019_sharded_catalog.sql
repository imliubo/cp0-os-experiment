ALTER TABLE store_catalog_snapshots
    DROP CONSTRAINT store_catalog_snapshots_app_count_check;
ALTER TABLE store_catalog_snapshots
    ALTER COLUMN app_count TYPE INTEGER;
ALTER TABLE store_catalog_snapshots
    ADD CONSTRAINT store_catalog_snapshots_app_count_check
        CHECK (app_count BETWEEN 0 AND 1024),
    ADD COLUMN document_kind TEXT NOT NULL DEFAULT 'legacy'
        CHECK (document_kind IN ('legacy', 'index')),
    ADD COLUMN shard_count SMALLINT NOT NULL DEFAULT 0
        CHECK (shard_count BETWEEN 0 AND 16),
    ADD CONSTRAINT store_catalog_snapshot_document_shape CHECK (
        (document_kind = 'legacy' AND app_count BETWEEN 0 AND 64 AND shard_count = 0) OR
        (document_kind = 'index' AND app_count BETWEEN 65 AND 1024 AND shard_count BETWEEN 2 AND 16)
    );

CREATE TABLE store_catalog_shards (
    catalog_sequence BIGINT NOT NULL REFERENCES store_catalog_snapshots(sequence),
    shard_index SMALLINT NOT NULL CHECK (shard_index BETWEEN 0 AND 15),
    shard_sha256 CHAR(64) NOT NULL CHECK (shard_sha256 ~ '^[0-9a-f]{64}$'),
    shard_bytes INTEGER NOT NULL CHECK (shard_bytes BETWEEN 1 AND 49152),
    relative_path TEXT NOT NULL UNIQUE CHECK (
        relative_path ~ '^generations/[1-9][0-9]{0,18}/shards/[0-9]{4}\.json$'
    ),
    store_key_id CHAR(64) NOT NULL CHECK (store_key_id ~ '^[0-9a-f]{64}$'),
    app_count SMALLINT NOT NULL CHECK (app_count BETWEEN 1 AND 64),
    first_app_id TEXT NOT NULL,
    last_app_id TEXT NOT NULL,
    encoded_shard BYTEA NOT NULL CHECK (
        octet_length(encoded_shard) BETWEEN 1 AND 49152 AND
        octet_length(encoded_shard) = shard_bytes
    ),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    PRIMARY KEY (catalog_sequence, shard_index),
    CHECK (first_app_id <= last_app_id)
);

CREATE FUNCTION enforce_store_catalog_shard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    snapshot store_catalog_snapshots%ROWTYPE;
BEGIN
    SELECT * INTO snapshot FROM store_catalog_snapshots
     WHERE sequence = NEW.catalog_sequence;
    IF snapshot.sequence IS NULL OR snapshot.document_kind <> 'index' OR
       NEW.shard_index >= snapshot.shard_count OR
       NEW.store_key_id <> snapshot.store_key_id OR
       NEW.relative_path <> format(
           'generations/%s/shards/%s.json',
           NEW.catalog_sequence,
           lpad(NEW.shard_index::TEXT, 4, '0')
       ) OR NOT EXISTS (
           SELECT 1 FROM store_publication_jobs job
            WHERE job.event_id = snapshot.source_event_id
              AND job.state = 'running'
              AND job.catalog_sequence = NEW.catalog_sequence
       ) THEN
        RAISE EXCEPTION 'Catalog shard does not match an active indexed publication'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_catalog_shards_authorized_insert
    BEFORE INSERT ON store_catalog_shards
    FOR EACH ROW EXECUTE FUNCTION enforce_store_catalog_shard();
CREATE TRIGGER store_catalog_shards_append_only
    BEFORE UPDATE OR DELETE ON store_catalog_shards
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();

CREATE FUNCTION require_completed_catalog_shards() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    snapshot store_catalog_snapshots%ROWTYPE;
    actual_shards BIGINT;
    actual_apps BIGINT;
    ordered_shards BIGINT;
BEGIN
    IF NEW.state <> 'completed' OR OLD.state = 'completed' THEN
        RETURN NULL;
    END IF;
    SELECT * INTO snapshot FROM store_catalog_snapshots
     WHERE source_event_id = NEW.event_id AND sequence = NEW.catalog_sequence;
    IF snapshot.sequence IS NULL THEN
        RAISE EXCEPTION 'Completed publication is missing its Catalog snapshot'
            USING ERRCODE = '55000';
    END IF;
    SELECT COUNT(*), COALESCE(SUM(app_count), 0),
           COUNT(*) FILTER (WHERE shard_index BETWEEN 0 AND snapshot.shard_count - 1)
      INTO actual_shards, actual_apps, ordered_shards
      FROM store_catalog_shards WHERE catalog_sequence = snapshot.sequence;
    IF (snapshot.document_kind = 'legacy' AND actual_shards <> 0) OR
       (snapshot.document_kind = 'index' AND (
           actual_shards <> snapshot.shard_count OR
           ordered_shards <> snapshot.shard_count OR
           actual_apps <> snapshot.app_count
       )) THEN
        RAISE EXCEPTION 'Completed publication has an incomplete Catalog shard set'
            USING ERRCODE = '55000';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER store_publication_jobs_require_catalog_shards
    AFTER UPDATE ON store_publication_jobs
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_completed_catalog_shards();
