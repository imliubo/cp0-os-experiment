ALTER TABLE submissions
    ADD COLUMN finalized_content_sha256 CHAR(64)
    CHECK (finalized_content_sha256 IS NULL OR
           finalized_content_sha256 ~ '^[0-9a-f]{64}$');

CREATE TABLE submission_upload_parts (
    submission_id TEXT NOT NULL REFERENCES submissions(submission_id),
    part_name TEXT NOT NULL CHECK (part_name ~ '^(package|listing|asset-[0-5])$'),
    expected_sha256 CHAR(64) NOT NULL CHECK (expected_sha256 ~ '^[0-9a-f]{64}$'),
    expected_bytes BIGINT NOT NULL CHECK (expected_bytes BETWEEN 1 AND 8392704),
    received_bytes BIGINT NOT NULL DEFAULT 0 CHECK (
        received_bytes >= 0 AND received_bytes <= expected_bytes
    ),
    PRIMARY KEY (submission_id, part_name)
);

CREATE TABLE submission_upload_chunks (
    submission_id TEXT NOT NULL,
    part_name TEXT NOT NULL,
    chunk_offset BIGINT NOT NULL CHECK (chunk_offset >= 0),
    chunk_bytes INTEGER NOT NULL CHECK (chunk_bytes BETWEEN 1 AND 262144),
    chunk_sha256 CHAR(64) NOT NULL CHECK (chunk_sha256 ~ '^[0-9a-f]{64}$'),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    PRIMARY KEY (submission_id, part_name, chunk_offset),
    FOREIGN KEY (submission_id, part_name)
        REFERENCES submission_upload_parts(submission_id, part_name)
);

CREATE INDEX submission_upload_chunks_order_idx
    ON submission_upload_chunks (submission_id, part_name, chunk_offset);

CREATE FUNCTION protect_upload_part() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Submission upload parts cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF (NEW.submission_id, NEW.part_name, NEW.expected_sha256, NEW.expected_bytes)
       IS DISTINCT FROM
       (OLD.submission_id, OLD.part_name, OLD.expected_sha256, OLD.expected_bytes)
       OR NEW.received_bytes < OLD.received_bytes THEN
        RAISE EXCEPTION 'Submission upload part identity is immutable' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER submission_upload_parts_immutable
    BEFORE UPDATE OR DELETE ON submission_upload_parts
    FOR EACH ROW EXECUTE FUNCTION protect_upload_part();

CREATE TRIGGER submission_upload_chunks_append_only
    BEFORE UPDATE OR DELETE ON submission_upload_chunks
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();

CREATE FUNCTION protect_submission_finalization() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.finalized_content_sha256 IS NOT NULL AND
       NEW.finalized_content_sha256 IS DISTINCT FROM OLD.finalized_content_sha256 THEN
        RAISE EXCEPTION 'Submission content digest is immutable' USING ERRCODE = '55000';
    END IF;
    IF OLD.finalized_content_sha256 IS NULL AND NEW.finalized_content_sha256 IS NOT NULL AND
       NOT (OLD.state = 'uploading' AND NEW.state = 'processing') THEN
        RAISE EXCEPTION 'Submission digest can only be set during finalization' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER submissions_finalize_once
    BEFORE UPDATE ON submissions
    FOR EACH ROW EXECUTE FUNCTION protect_submission_finalization();
