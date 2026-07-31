CREATE TABLE store_transparency_leaves (
    tree_index BIGINT PRIMARY KEY CHECK (tree_index >= 0),
    catalog_sequence BIGINT NOT NULL UNIQUE REFERENCES store_catalog_snapshots(sequence),
    leaf_sha256 CHAR(64) NOT NULL UNIQUE CHECK (leaf_sha256 ~ '^[0-9a-f]{64}$'),
    encoded_leaf BYTEA NOT NULL CHECK (octet_length(encoded_leaf) BETWEEN 1 AND 4096),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1)
);

CREATE TABLE store_transparency_checkpoints (
    tree_size BIGINT PRIMARY KEY CHECK (tree_size >= 1),
    catalog_sequence BIGINT NOT NULL UNIQUE REFERENCES store_catalog_snapshots(sequence),
    root_sha256 CHAR(64) NOT NULL CHECK (root_sha256 ~ '^[0-9a-f]{64}$'),
    store_key_id CHAR(64) NOT NULL CHECK (store_key_id ~ '^[0-9a-f]{64}$'),
    encoded_checkpoint BYTEA NOT NULL CHECK (
        octet_length(encoded_checkpoint) BETWEEN 1 AND 2048
    ),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    CHECK (tree_size >= 1)
);

CREATE FUNCTION enforce_transparency_leaf() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    expected_index BIGINT;
BEGIN
    SELECT COALESCE(MAX(tree_index) + 1, 0) INTO expected_index
    FROM store_transparency_leaves;
    IF NEW.tree_index <> expected_index OR NOT EXISTS (
        SELECT 1 FROM store_catalog_snapshots snapshot
        JOIN store_publication_jobs job ON job.event_id = snapshot.source_event_id
        WHERE snapshot.sequence = NEW.catalog_sequence
          AND job.state = 'running'
    ) THEN
        RAISE EXCEPTION 'Transparency leaf is not the next active Catalog publication'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_transparency_leaves_ordered_insert
    BEFORE INSERT ON store_transparency_leaves
    FOR EACH ROW EXECUTE FUNCTION enforce_transparency_leaf();
CREATE TRIGGER store_transparency_leaves_append_only
    BEFORE UPDATE OR DELETE ON store_transparency_leaves
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();

CREATE FUNCTION enforce_transparency_checkpoint() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.tree_size <> (SELECT COUNT(*) FROM store_transparency_leaves) OR
       NOT EXISTS (
           SELECT 1 FROM store_transparency_leaves leaf
           JOIN store_catalog_snapshots snapshot ON snapshot.sequence = leaf.catalog_sequence
           JOIN store_publication_jobs job ON job.event_id = snapshot.source_event_id
           WHERE leaf.tree_index = NEW.tree_size - 1
             AND leaf.catalog_sequence = NEW.catalog_sequence
             AND job.state = 'running'
       ) THEN
        RAISE EXCEPTION 'Transparency checkpoint does not close the current active log'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER store_transparency_checkpoints_ordered_insert
    BEFORE INSERT ON store_transparency_checkpoints
    FOR EACH ROW EXECUTE FUNCTION enforce_transparency_checkpoint();
CREATE TRIGGER store_transparency_checkpoints_append_only
    BEFORE UPDATE OR DELETE ON store_transparency_checkpoints
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();

CREATE OR REPLACE FUNCTION protect_store_publication_job() RETURNS trigger LANGUAGE plpgsql AS $$
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
    IF NEW.state = 'completed' AND (
        NOT EXISTS (
            SELECT 1 FROM store_catalog_snapshots snapshot
            WHERE snapshot.source_event_id = NEW.event_id
              AND snapshot.sequence = NEW.catalog_sequence
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
