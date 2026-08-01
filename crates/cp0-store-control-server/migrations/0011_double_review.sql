ALTER TABLE submissions DROP CONSTRAINT submissions_state_check;
ALTER TABLE submissions ADD CONSTRAINT submissions_state_check CHECK (state IN (
    'draft', 'uploading', 'processing', 'ready-for-review', 'in-review',
    'pending-secondary-review', 'needs-changes', 'approved', 'rejected', 'withdrawn'
));

DROP INDEX submissions_review_queue_idx;
CREATE INDEX submissions_review_queue_idx ON submissions (created_unix_seconds, submission_id)
    WHERE state IN ('ready-for-review', 'pending-secondary-review');

ALTER TABLE review_decisions ADD COLUMN assignment_id TEXT;
ALTER TABLE review_decisions DISABLE TRIGGER review_decisions_append_only;
INSERT INTO review_assignments (
    assignment_id, submission_id, reviewer_id, assignment_kind, state,
    source_resource_version, created_unix_seconds, completed_unix_seconds
)
SELECT
    'assignment_' || substring(decision.decision_id FROM 10),
    decision.submission_id,
    decision.reviewer_id,
    'primary',
    'completed',
    submission.resource_version,
    decision.created_unix_seconds,
    decision.created_unix_seconds
FROM review_decisions decision
JOIN submissions submission ON submission.submission_id = decision.submission_id
WHERE NOT EXISTS (
    SELECT 1 FROM review_assignments assignment
    WHERE assignment.submission_id = decision.submission_id
      AND assignment.reviewer_id = decision.reviewer_id
      AND assignment.assignment_kind = 'primary'
);
UPDATE review_decisions decision
SET assignment_id = assignment.assignment_id
FROM review_assignments assignment
WHERE assignment.submission_id = decision.submission_id
  AND assignment.reviewer_id = decision.reviewer_id
  AND assignment.assignment_kind = 'primary';
ALTER TABLE review_decisions ALTER COLUMN assignment_id SET NOT NULL;
ALTER TABLE review_decisions ENABLE TRIGGER review_decisions_append_only;
ALTER TABLE review_decisions ADD CONSTRAINT review_decisions_assignment_fk
    FOREIGN KEY (assignment_id) REFERENCES review_assignments(assignment_id);
ALTER TABLE review_decisions ADD CONSTRAINT review_decisions_one_per_assignment
    UNIQUE (assignment_id);

CREATE FUNCTION enforce_review_assignment_stage() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    submission_state TEXT;
BEGIN
    SELECT state INTO submission_state FROM submissions
    WHERE submission_id = NEW.submission_id FOR UPDATE;
    IF NEW.state <> 'active' THEN
        RAISE EXCEPTION 'New review assignments must be active' USING ERRCODE = '55000';
    END IF;
    IF NEW.assignment_kind = 'primary' THEN
        IF submission_state <> 'ready-for-review' OR EXISTS (
            SELECT 1 FROM review_assignments WHERE submission_id = NEW.submission_id
        ) THEN
            RAISE EXCEPTION 'Primary review assignment stage is invalid' USING ERRCODE = '55000';
        END IF;
    ELSIF NEW.assignment_kind = 'secondary' THEN
        IF submission_state <> 'pending-secondary-review' OR EXISTS (
            SELECT 1 FROM review_assignments
            WHERE submission_id = NEW.submission_id AND reviewer_id = NEW.reviewer_id
        ) OR NOT EXISTS (
            SELECT 1
            FROM review_assignments assignment
            JOIN review_decisions decision ON decision.assignment_id = assignment.assignment_id
            WHERE assignment.submission_id = NEW.submission_id
              AND assignment.assignment_kind = 'primary'
              AND assignment.state = 'completed'
              AND decision.decision = 'approved'
        ) THEN
            RAISE EXCEPTION 'Secondary review assignment stage is invalid' USING ERRCODE = '55000';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER review_assignments_stage
    BEFORE INSERT ON review_assignments
    FOR EACH ROW EXECUTE FUNCTION enforce_review_assignment_stage();

CREATE FUNCTION enforce_review_decision_assignment() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM review_assignments assignment
        WHERE assignment.assignment_id = NEW.assignment_id
          AND assignment.submission_id = NEW.submission_id
          AND assignment.reviewer_id = NEW.reviewer_id
          AND assignment.state = 'active'
    ) THEN
        RAISE EXCEPTION 'Review decision must bind the active assignment' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER review_decisions_assignment
    BEFORE INSERT ON review_decisions
    FOR EACH ROW EXECUTE FUNCTION enforce_review_decision_assignment();

CREATE OR REPLACE FUNCTION protect_submission_state_transition() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.resource_version <> OLD.resource_version + 1 OR NOT (
        (OLD.state = 'draft' AND NEW.state IN ('uploading', 'withdrawn')) OR
        (OLD.state = 'uploading' AND NEW.state IN ('uploading', 'processing', 'withdrawn')) OR
        (OLD.state = 'processing' AND
         NEW.state IN ('ready-for-review', 'needs-changes', 'rejected', 'withdrawn')) OR
        (OLD.state = 'ready-for-review' AND NEW.state = 'in-review' AND EXISTS (
            SELECT 1 FROM review_assignments assignment
            WHERE assignment.submission_id = NEW.submission_id
              AND assignment.assignment_kind = 'primary' AND assignment.state = 'active'
        )) OR
        (OLD.state = 'ready-for-review' AND NEW.state = 'withdrawn') OR
        (OLD.state = 'pending-secondary-review' AND NEW.state = 'in-review' AND EXISTS (
            SELECT 1 FROM review_assignments assignment
            WHERE assignment.submission_id = NEW.submission_id
              AND assignment.assignment_kind = 'secondary' AND assignment.state = 'active'
        )) OR
        (OLD.state = 'pending-secondary-review' AND NEW.state = 'withdrawn') OR
        (OLD.state = 'in-review' AND NEW.state = 'pending-secondary-review' AND EXISTS (
            SELECT 1
            FROM review_assignments assignment
            JOIN review_decisions decision ON decision.assignment_id = assignment.assignment_id
            WHERE assignment.submission_id = NEW.submission_id
              AND assignment.assignment_kind = 'primary'
              AND assignment.state = 'completed' AND decision.decision = 'approved'
        )) OR
        (OLD.state = 'in-review' AND NEW.state = 'approved' AND EXISTS (
            SELECT 1
            FROM review_assignments primary_assignment
            JOIN review_decisions primary_decision
              ON primary_decision.assignment_id = primary_assignment.assignment_id
            JOIN review_assignments secondary_assignment
              ON secondary_assignment.submission_id = primary_assignment.submission_id
             AND secondary_assignment.assignment_kind = 'secondary'
            JOIN review_decisions secondary_decision
              ON secondary_decision.assignment_id = secondary_assignment.assignment_id
            WHERE primary_assignment.submission_id = NEW.submission_id
              AND primary_assignment.assignment_kind = 'primary'
              AND primary_assignment.state = 'completed'
              AND secondary_assignment.state = 'completed'
              AND primary_assignment.reviewer_id <> secondary_assignment.reviewer_id
              AND primary_decision.decision = 'approved'
              AND secondary_decision.decision = 'approved'
        )) OR
        (OLD.state = 'in-review' AND NEW.state IN ('needs-changes', 'rejected') AND EXISTS (
            SELECT 1
            FROM review_assignments assignment
            JOIN review_decisions decision ON decision.assignment_id = assignment.assignment_id
            WHERE assignment.submission_id = NEW.submission_id
              AND assignment.state = 'completed' AND decision.decision = NEW.state
        )) OR
        (OLD.state = 'in-review' AND NEW.state = 'withdrawn')
    ) THEN
        RAISE EXCEPTION 'Submission state transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_release_creation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.state <> 'ready' OR NEW.resource_version <> 1 OR
       NEW.scheduled_unix_seconds IS NOT NULL OR NEW.catalog_sequence IS NOT NULL OR
       NOT EXISTS (
           SELECT 1
           FROM submissions submission
           JOIN review_assignments primary_assignment
             ON primary_assignment.submission_id = submission.submission_id
            AND primary_assignment.assignment_kind = 'primary'
           JOIN review_decisions primary_decision
             ON primary_decision.assignment_id = primary_assignment.assignment_id
            AND primary_decision.decision = 'approved'
           JOIN review_assignments secondary_assignment
             ON secondary_assignment.submission_id = submission.submission_id
            AND secondary_assignment.assignment_kind = 'secondary'
           JOIN review_decisions secondary_decision
             ON secondary_decision.assignment_id = secondary_assignment.assignment_id
            AND secondary_decision.decision = 'approved'
           WHERE submission.submission_id = NEW.submission_id
             AND submission.app_id = NEW.app_id
             AND submission.version = NEW.version
             AND submission.state = 'approved'
             AND primary_assignment.state = 'completed'
             AND secondary_assignment.state = 'completed'
             AND primary_assignment.reviewer_id <> secondary_assignment.reviewer_id
       ) THEN
        RAISE EXCEPTION 'Release must bind an independently double-approved Submission'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;
