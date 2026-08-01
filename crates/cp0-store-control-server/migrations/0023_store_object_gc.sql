CREATE INDEX submission_upload_chunks_digest_idx
    ON submission_upload_chunks (chunk_sha256);
