DROP TRIGGER store_publication_jobs_state_machine ON store_publication_jobs;

ALTER TABLE store_publication_jobs ADD COLUMN developer_name TEXT;

UPDATE store_publication_jobs job
   SET developer_name = team.name
  FROM releases release
  JOIN apps app ON app.app_id = release.app_id
  JOIN teams team ON team.team_id = app.owner_team_id
 WHERE release.release_id = job.release_id AND job.attempts >= 1;

ALTER TABLE store_publication_jobs
    ADD CONSTRAINT store_publication_jobs_developer_snapshot_check CHECK (
        (attempts = 0 AND developer_name IS NULL) OR
        (attempts >= 1 AND developer_name IS NOT NULL AND
         char_length(developer_name) BETWEEN 1 AND 80 AND
         developer_name = btrim(developer_name) AND
         developer_name !~ '[[:cntrl:]]')
    );

CREATE OR REPLACE FUNCTION protect_store_publication_job() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    expected_developer_name TEXT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Store publication jobs cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.event_id, NEW.release_id, NEW.job_kind, NEW.source_resource_version,
        NEW.source_state, NEW.created_unix_seconds) IS DISTINCT FROM
       (OLD.event_id, OLD.release_id, OLD.job_kind, OLD.source_resource_version,
        OLD.source_state, OLD.created_unix_seconds) OR
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
