DROP INDEX submissions_review_queue_idx;
CREATE INDEX submissions_review_queue_idx ON submissions (created_unix_seconds, submission_id)
    WHERE state IN ('ready-for-review', 'pending-secondary-review', 'in-review');
