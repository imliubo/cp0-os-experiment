ALTER TABLE store_catalog_snapshots
    DROP CONSTRAINT store_catalog_snapshot_document_shape;
ALTER TABLE store_catalog_snapshots
    ADD CONSTRAINT store_catalog_snapshot_document_shape CHECK (
        (document_kind = 'legacy' AND app_count BETWEEN 0 AND 64 AND shard_count = 0) OR
        (document_kind = 'index' AND app_count BETWEEN 1 AND 1024 AND shard_count BETWEEN 1 AND 16)
    );
