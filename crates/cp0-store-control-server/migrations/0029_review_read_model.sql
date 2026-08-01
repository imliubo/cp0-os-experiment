CREATE TABLE submission_review_metadata (
    scan_id TEXT PRIMARY KEY REFERENCES submission_scan_results(scan_id),
    submission_id TEXT NOT NULL UNIQUE REFERENCES submissions(submission_id),
    name TEXT NOT NULL CHECK (
        char_length(name) BETWEEN 1 AND 32 AND name = btrim(name) AND name !~ '[[:cntrl:]]'
    ),
    category TEXT NOT NULL CHECK (category IN (
        'developer-tools', 'education', 'entertainment', 'games',
        'hardware', 'media', 'productivity', 'utilities'
    )),
    default_locale TEXT NOT NULL CHECK (
        char_length(default_locale) BETWEEN 2 AND 16 AND
        default_locale ~ '^[a-z]{2,3}(-[A-Z][a-z]{3})?(-([A-Z]{2}|[0-9]{3}))?$'
    ),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1)
);

CREATE TRIGGER submission_review_metadata_append_only
    BEFORE UPDATE OR DELETE ON submission_review_metadata
    FOR EACH ROW EXECUTE FUNCTION reject_append_only_mutation();

CREATE FUNCTION validate_submission_review_metadata() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    matching_scan_count BIGINT;
BEGIN
    SELECT COUNT(*) INTO matching_scan_count
      FROM submission_scan_results scan
      JOIN submissions submission ON submission.submission_id = scan.submission_id
      JOIN apps app ON app.app_id = submission.app_id
     WHERE scan.scan_id = NEW.scan_id AND scan.submission_id = NEW.submission_id AND
           scan.outcome = 'ready-for-review' AND app.default_locale = NEW.default_locale AND
           scan.created_unix_seconds = NEW.created_unix_seconds;
    IF matching_scan_count <> 1 THEN
        RAISE EXCEPTION 'Review metadata is not bound to its reviewable scan'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER submission_review_metadata_scan_binding
    AFTER INSERT ON submission_review_metadata
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION validate_submission_review_metadata();
