ALTER TABLE team_members
    DROP CONSTRAINT team_members_membership_state_check,
    DROP CONSTRAINT team_members_removal_state_check,
    ADD CONSTRAINT team_members_membership_state_check CHECK (
        membership_state IN ('active', 'suspended', 'removed')
    ),
    ADD CONSTRAINT team_members_removal_state_check CHECK (
        (membership_state IN ('active', 'suspended') AND removed_unix_seconds IS NULL) OR
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
    IF NEW.membership_state <> OLD.membership_state THEN
        IF NOT (
            (OLD.membership_state = 'active' AND NEW.membership_state IN ('suspended', 'removed')) OR
            (OLD.membership_state = 'suspended' AND NEW.membership_state IN ('active', 'removed'))
        ) THEN
            RAISE EXCEPTION 'Team membership state transition is invalid' USING ERRCODE = '55000';
        END IF;
        IF (NEW.email, NEW.role, NEW.two_factor_enabled) IS DISTINCT FROM
           (OLD.email, OLD.role, OLD.two_factor_enabled) THEN
            RAISE EXCEPTION 'Team membership state changes cannot alter member attributes'
                USING ERRCODE = '55000';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
