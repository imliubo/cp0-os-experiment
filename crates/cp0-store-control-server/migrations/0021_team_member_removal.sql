ALTER TABLE team_members
    ADD COLUMN membership_state TEXT NOT NULL DEFAULT 'active'
        CHECK (membership_state IN ('active', 'removed')),
    ADD COLUMN removed_unix_seconds BIGINT,
    ADD CONSTRAINT team_members_removal_state_check CHECK (
        (membership_state = 'active' AND removed_unix_seconds IS NULL) OR
        (membership_state = 'removed' AND removed_unix_seconds >= 1)
    );

CREATE OR REPLACE FUNCTION protect_member_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Team memberships cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.membership_state <> 'active' OR NEW.removed_unix_seconds IS NOT NULL OR
           NEW.resource_version <> 1 THEN
            RAISE EXCEPTION 'New Team memberships must start active at version one'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF NEW.member_id <> OLD.member_id OR NEW.team_id <> OLD.team_id THEN
        RAISE EXCEPTION 'Team membership identity cannot be reassigned' USING ERRCODE = '55000';
    END IF;
    IF NEW.resource_version <> OLD.resource_version + 1 THEN
        RAISE EXCEPTION 'Team member resource version must advance by one' USING ERRCODE = '55000';
    END IF;
    IF OLD.membership_state = 'removed' THEN
        RAISE EXCEPTION 'Removed Team memberships are immutable' USING ERRCODE = '55000';
    END IF;
    IF NEW.membership_state = 'removed' AND
       (NEW.email, NEW.role, NEW.two_factor_enabled) IS DISTINCT FROM
       (OLD.email, OLD.role, OLD.two_factor_enabled) THEN
        RAISE EXCEPTION 'Team membership removal cannot alter member attributes'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER team_members_stable_identity ON team_members;
CREATE TRIGGER team_members_stable_identity
    BEFORE INSERT OR UPDATE OR DELETE ON team_members
    FOR EACH ROW EXECUTE FUNCTION protect_member_identity();

CREATE OR REPLACE FUNCTION require_team_owner() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    affected_team_id TEXT;
BEGIN
    affected_team_id := CASE WHEN TG_OP = 'DELETE' THEN OLD.team_id ELSE NEW.team_id END;
    IF NOT EXISTS (
        SELECT 1 FROM team_members
        WHERE team_id = affected_team_id AND role = 'owner' AND membership_state = 'active'
    ) THEN
        RAISE EXCEPTION 'A team must retain at least one active Owner' USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;
