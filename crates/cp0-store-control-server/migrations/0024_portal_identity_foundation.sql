CREATE TABLE portal_accounts (
    account_id TEXT PRIMARY KEY CHECK (account_id ~ '^account_[0-9a-f]{32}$'),
    email TEXT NOT NULL UNIQUE CHECK (
        char_length(email) BETWEEN 3 AND 254 AND
        email = btrim(email) AND email = lower(email)
    ),
    email_verified BOOLEAN NOT NULL CHECK (email_verified),
    state TEXT NOT NULL CHECK (state IN ('active', 'disabled')),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    disabled_unix_seconds BIGINT,
    CHECK (
        (state = 'active' AND disabled_unix_seconds IS NULL) OR
        (state = 'disabled' AND disabled_unix_seconds >= created_unix_seconds)
    )
);

CREATE TABLE external_identity_links (
    link_id TEXT PRIMARY KEY CHECK (link_id ~ '^link_[0-9a-f]{32}$'),
    account_id TEXT NOT NULL REFERENCES portal_accounts(account_id),
    provider_key TEXT NOT NULL CHECK (provider_key ~ '^[a-z][a-z0-9-]{0,31}$'),
    issuer TEXT NOT NULL CHECK (
        char_length(issuer) BETWEEN 9 AND 512 AND issuer = btrim(issuer) AND
        issuer ~ '^https://[^[:space:]?#]+$'
    ),
    subject_hmac_sha256 CHAR(64) NOT NULL CHECK (
        subject_hmac_sha256 ~ '^[0-9a-f]{64}$'
    ),
    state TEXT NOT NULL CHECK (state IN ('active', 'revoked')),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    linked_unix_seconds BIGINT NOT NULL CHECK (linked_unix_seconds >= 1),
    revoked_unix_seconds BIGINT,
    UNIQUE (issuer, subject_hmac_sha256),
    CHECK (
        (state = 'active' AND revoked_unix_seconds IS NULL) OR
        (state = 'revoked' AND revoked_unix_seconds >= linked_unix_seconds)
    )
);

CREATE TABLE portal_sessions (
    session_sha256 CHAR(64) PRIMARY KEY CHECK (session_sha256 ~ '^[0-9a-f]{64}$'),
    csrf_sha256 CHAR(64) NOT NULL CHECK (csrf_sha256 ~ '^[0-9a-f]{64}$'),
    account_id TEXT NOT NULL REFERENCES portal_accounts(account_id),
    current_link_id TEXT NOT NULL REFERENCES external_identity_links(link_id),
    state TEXT NOT NULL CHECK (state IN ('active', 'revoked', 'expired')),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    last_seen_unix_seconds BIGINT NOT NULL,
    idle_expires_unix_seconds BIGINT NOT NULL,
    absolute_expires_unix_seconds BIGINT NOT NULL,
    mfa_authenticated_unix_seconds BIGINT,
    ended_unix_seconds BIGINT,
    CHECK (last_seen_unix_seconds BETWEEN created_unix_seconds AND absolute_expires_unix_seconds),
    CHECK (absolute_expires_unix_seconds = created_unix_seconds + 28800),
    CHECK (
        idle_expires_unix_seconds =
            LEAST(last_seen_unix_seconds + 1800, absolute_expires_unix_seconds)
    ),
    CHECK (
        mfa_authenticated_unix_seconds IS NULL OR
        mfa_authenticated_unix_seconds BETWEEN 1 AND created_unix_seconds
    ),
    CHECK (
        (state = 'active' AND ended_unix_seconds IS NULL) OR
        (state IN ('revoked', 'expired') AND ended_unix_seconds >= created_unix_seconds)
    )
);

CREATE TABLE oidc_login_transactions (
    transaction_id TEXT PRIMARY KEY CHECK (transaction_id ~ '^oidctx_[0-9a-f]{32}$'),
    state_sha256 CHAR(64) NOT NULL UNIQUE CHECK (state_sha256 ~ '^[0-9a-f]{64}$'),
    nonce_sha256 CHAR(64) NOT NULL CHECK (nonce_sha256 ~ '^[0-9a-f]{64}$'),
    pkce_verifier_ciphertext BYTEA NOT NULL CHECK (
        octet_length(pkce_verifier_ciphertext) BETWEEN 32 AND 4096
    ),
    provider_key TEXT NOT NULL CHECK (provider_key ~ '^[a-z][a-z0-9-]{0,31}$'),
    provider_config_sha256 CHAR(64) NOT NULL CHECK (
        provider_config_sha256 ~ '^[0-9a-f]{64}$'
    ),
    intent TEXT NOT NULL CHECK (intent IN ('login', 'step-up', 'link')),
    account_id TEXT REFERENCES portal_accounts(account_id),
    session_sha256 CHAR(64) REFERENCES portal_sessions(session_sha256),
    state TEXT NOT NULL CHECK (state IN ('pending', 'consumed', 'expired')),
    requested_unix_seconds BIGINT NOT NULL CHECK (requested_unix_seconds >= 1),
    expires_unix_seconds BIGINT NOT NULL,
    consumed_unix_seconds BIGINT,
    CHECK (expires_unix_seconds = requested_unix_seconds + 600),
    CHECK (
        (intent = 'login' AND account_id IS NULL AND session_sha256 IS NULL) OR
        (intent IN ('step-up', 'link') AND account_id IS NOT NULL AND session_sha256 IS NOT NULL)
    ),
    CHECK (
        (state = 'pending' AND consumed_unix_seconds IS NULL) OR
        (state IN ('consumed', 'expired') AND
         consumed_unix_seconds >= requested_unix_seconds)
    ),
    CHECK (
        state <> 'consumed' OR consumed_unix_seconds <= expires_unix_seconds
    )
);

ALTER TABLE team_members ADD COLUMN account_id TEXT REFERENCES portal_accounts(account_id);
CREATE UNIQUE INDEX team_members_team_account_idx
    ON team_members (team_id, account_id) WHERE account_id IS NOT NULL;

CREATE TABLE team_invitations (
    invitation_id TEXT PRIMARY KEY CHECK (invitation_id ~ '^invite_[0-9a-f]{32}$'),
    team_id TEXT NOT NULL REFERENCES teams(team_id),
    email TEXT NOT NULL CHECK (
        char_length(email) BETWEEN 3 AND 254 AND
        email = btrim(email) AND email = lower(email)
    ),
    role TEXT NOT NULL CHECK (role IN ('developer', 'release-manager', 'viewer')),
    token_sha256 CHAR(64) NOT NULL UNIQUE CHECK (token_sha256 ~ '^[0-9a-f]{64}$'),
    state TEXT NOT NULL CHECK (state IN ('pending', 'accepted', 'cancelled', 'expired')),
    invited_by_member_id TEXT NOT NULL REFERENCES team_members(member_id),
    accepted_account_id TEXT REFERENCES portal_accounts(account_id),
    accepted_member_id TEXT REFERENCES team_members(member_id),
    resource_version BIGINT NOT NULL DEFAULT 1 CHECK (resource_version >= 1),
    team_resource_version BIGINT NOT NULL CHECK (team_resource_version >= 2),
    created_unix_seconds BIGINT NOT NULL CHECK (created_unix_seconds >= 1),
    expires_unix_seconds BIGINT NOT NULL,
    decided_unix_seconds BIGINT,
    CHECK (expires_unix_seconds = created_unix_seconds + 604800),
    CHECK (
        (state = 'pending' AND accepted_account_id IS NULL AND accepted_member_id IS NULL AND
         decided_unix_seconds IS NULL) OR
        (state = 'accepted' AND accepted_account_id IS NOT NULL AND accepted_member_id IS NOT NULL AND
         decided_unix_seconds BETWEEN created_unix_seconds AND expires_unix_seconds) OR
        (state = 'cancelled' AND accepted_account_id IS NULL AND accepted_member_id IS NULL AND
         decided_unix_seconds BETWEEN created_unix_seconds AND expires_unix_seconds) OR
        (state = 'expired' AND accepted_account_id IS NULL AND accepted_member_id IS NULL AND
         decided_unix_seconds >= expires_unix_seconds)
    )
);

CREATE UNIQUE INDEX team_invitations_pending_email_idx
    ON team_invitations (team_id, email) WHERE state = 'pending';
CREATE INDEX team_invitations_team_created_idx
    ON team_invitations (team_id, created_unix_seconds, invitation_id);
CREATE INDEX portal_sessions_active_account_idx
    ON portal_sessions (account_id, absolute_expires_unix_seconds)
    WHERE state = 'active';
CREATE INDEX external_identity_links_active_account_idx
    ON external_identity_links (account_id, linked_unix_seconds, link_id)
    WHERE state = 'active';

CREATE FUNCTION protect_portal_account() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Portal accounts cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'active' OR NEW.resource_version <> 1 OR
           NEW.disabled_unix_seconds IS NOT NULL THEN
            RAISE EXCEPTION 'New Portal accounts must start active at version one'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF NEW.account_id <> OLD.account_id OR
       NEW.created_unix_seconds <> OLD.created_unix_seconds OR
       NEW.email_verified <> OLD.email_verified OR
       NEW.resource_version <> OLD.resource_version + 1 THEN
        RAISE EXCEPTION 'Portal account identity or version is invalid' USING ERRCODE = '55000';
    END IF;
    IF OLD.state = 'disabled' OR NOT (
        (OLD.state = 'active' AND NEW.state = 'active' AND NEW.disabled_unix_seconds IS NULL) OR
        (OLD.state = 'active' AND NEW.state = 'disabled' AND
         NEW.email = OLD.email AND NEW.disabled_unix_seconds IS NOT NULL)
    ) THEN
        RAISE EXCEPTION 'Portal account transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER portal_accounts_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON portal_accounts
    FOR EACH ROW EXECUTE FUNCTION protect_portal_account();

CREATE FUNCTION protect_external_identity_link() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'External identity links cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'active' OR NEW.resource_version <> 1 OR
           NEW.revoked_unix_seconds IS NOT NULL THEN
            RAISE EXCEPTION 'New external identity links must start active at version one'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF (NEW.link_id, NEW.account_id, NEW.provider_key, NEW.issuer,
        NEW.subject_hmac_sha256, NEW.linked_unix_seconds) IS DISTINCT FROM
       (OLD.link_id, OLD.account_id, OLD.provider_key, OLD.issuer,
        OLD.subject_hmac_sha256, OLD.linked_unix_seconds) OR
       NEW.resource_version <> OLD.resource_version + 1 OR
       OLD.state <> 'active' OR NEW.state <> 'revoked' OR
       NEW.revoked_unix_seconds IS NULL THEN
        RAISE EXCEPTION 'External identity link transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER external_identity_links_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON external_identity_links
    FOR EACH ROW EXECUTE FUNCTION protect_external_identity_link();

CREATE FUNCTION require_active_external_identity_link() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    affected_account_id TEXT;
    active_count BIGINT;
BEGIN
    affected_account_id := CASE WHEN TG_OP = 'DELETE' THEN OLD.account_id ELSE NEW.account_id END;
    SELECT COUNT(*) INTO active_count FROM external_identity_links
        WHERE account_id = affected_account_id AND state = 'active';
    IF active_count < 1 OR active_count > 8 THEN
        RAISE EXCEPTION 'A Portal account must retain one to eight active identity links'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER external_identity_links_active_limit
    AFTER INSERT OR UPDATE OR DELETE ON external_identity_links
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_active_external_identity_link();

CREATE CONSTRAINT TRIGGER portal_accounts_active_link_required
    AFTER INSERT OR UPDATE ON portal_accounts
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_active_external_identity_link();

CREATE FUNCTION protect_portal_session() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Portal sessions cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'active' OR NEW.resource_version <> 1 OR
           NEW.ended_unix_seconds IS NOT NULL THEN
            RAISE EXCEPTION 'New Portal sessions must start active at version one'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF (NEW.session_sha256, NEW.csrf_sha256, NEW.account_id, NEW.current_link_id,
        NEW.created_unix_seconds, NEW.absolute_expires_unix_seconds,
        NEW.mfa_authenticated_unix_seconds) IS DISTINCT FROM
       (OLD.session_sha256, OLD.csrf_sha256, OLD.account_id, OLD.current_link_id,
        OLD.created_unix_seconds, OLD.absolute_expires_unix_seconds,
        OLD.mfa_authenticated_unix_seconds) OR
       NEW.resource_version <> OLD.resource_version + 1 OR OLD.state <> 'active' THEN
        RAISE EXCEPTION 'Portal session identity or version is invalid' USING ERRCODE = '55000';
    END IF;
    IF NEW.state = 'active' THEN
        IF NEW.ended_unix_seconds IS NOT NULL OR
           NEW.last_seen_unix_seconds <= OLD.last_seen_unix_seconds THEN
            RAISE EXCEPTION 'Portal session activity must advance monotonically'
                USING ERRCODE = '55000';
        END IF;
    ELSIF NEW.state IN ('revoked', 'expired') THEN
        IF (NEW.last_seen_unix_seconds, NEW.idle_expires_unix_seconds) IS DISTINCT FROM
           (OLD.last_seen_unix_seconds, OLD.idle_expires_unix_seconds) OR
           NEW.ended_unix_seconds IS NULL THEN
            RAISE EXCEPTION 'Portal session terminal transition is invalid'
                USING ERRCODE = '55000';
        END IF;
    ELSE
        RAISE EXCEPTION 'Portal session transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER portal_sessions_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON portal_sessions
    FOR EACH ROW EXECUTE FUNCTION protect_portal_session();

CREATE FUNCTION validate_portal_session_identity() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    link_account_id TEXT;
    link_state TEXT;
    account_state TEXT;
BEGIN
    SELECT account_id, state INTO link_account_id, link_state
        FROM external_identity_links WHERE link_id = NEW.current_link_id;
    SELECT state INTO account_state FROM portal_accounts WHERE account_id = NEW.account_id;
    IF link_account_id IS DISTINCT FROM NEW.account_id OR
       (NEW.state = 'active' AND
        (account_state IS DISTINCT FROM 'active' OR link_state IS DISTINCT FROM 'active')) THEN
        RAISE EXCEPTION 'Portal session identity binding is invalid' USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER portal_sessions_identity_binding
    AFTER INSERT OR UPDATE ON portal_sessions
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION validate_portal_session_identity();

CREATE FUNCTION revoke_sessions_for_disabled_account() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.state = 'active' AND NEW.state = 'disabled' THEN
        UPDATE portal_sessions SET state = 'revoked',
            ended_unix_seconds = GREATEST(
                created_unix_seconds,
                EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
            ),
            resource_version = resource_version + 1
        WHERE account_id = NEW.account_id AND state = 'active';
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER portal_accounts_revoke_sessions
    AFTER UPDATE ON portal_accounts
    FOR EACH ROW EXECUTE FUNCTION revoke_sessions_for_disabled_account();

CREATE FUNCTION revoke_sessions_for_revoked_link() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.state = 'active' AND NEW.state = 'revoked' THEN
        UPDATE portal_sessions SET state = 'revoked',
            ended_unix_seconds = GREATEST(
                created_unix_seconds,
                EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
            ),
            resource_version = resource_version + 1
        WHERE current_link_id = NEW.link_id AND state = 'active';
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER external_identity_links_revoke_sessions
    AFTER UPDATE ON external_identity_links
    FOR EACH ROW EXECUTE FUNCTION revoke_sessions_for_revoked_link();

CREATE FUNCTION expire_oidc_transactions_for_ended_session() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.state = 'active' AND NEW.state IN ('revoked', 'expired') THEN
        UPDATE oidc_login_transactions SET state = 'expired',
            consumed_unix_seconds = GREATEST(
                requested_unix_seconds,
                EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
            )
        WHERE session_sha256 = NEW.session_sha256 AND state = 'pending';
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER portal_sessions_expire_oidc_transactions
    AFTER UPDATE ON portal_sessions
    FOR EACH ROW EXECUTE FUNCTION expire_oidc_transactions_for_ended_session();

CREATE FUNCTION protect_oidc_login_transaction() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'OIDC login transactions cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' AND (NEW.state <> 'pending' OR NEW.consumed_unix_seconds IS NOT NULL) THEN
        RAISE EXCEPTION 'New OIDC login transactions must start pending'
            USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'UPDATE' AND (
        (NEW.transaction_id, NEW.state_sha256, NEW.nonce_sha256,
         NEW.pkce_verifier_ciphertext, NEW.provider_key, NEW.provider_config_sha256,
         NEW.intent, NEW.account_id, NEW.session_sha256, NEW.requested_unix_seconds,
         NEW.expires_unix_seconds) IS DISTINCT FROM
        (OLD.transaction_id, OLD.state_sha256, OLD.nonce_sha256,
         OLD.pkce_verifier_ciphertext, OLD.provider_key, OLD.provider_config_sha256,
         OLD.intent, OLD.account_id, OLD.session_sha256, OLD.requested_unix_seconds,
         OLD.expires_unix_seconds) OR
        OLD.state <> 'pending' OR NEW.state NOT IN ('consumed', 'expired') OR
        NEW.consumed_unix_seconds IS NULL
    ) THEN
        RAISE EXCEPTION 'OIDC login transaction transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER oidc_login_transactions_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON oidc_login_transactions
    FOR EACH ROW EXECUTE FUNCTION protect_oidc_login_transaction();

CREATE FUNCTION validate_oidc_login_transaction_identity() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    bound_account_id TEXT;
    bound_session_state TEXT;
BEGIN
    IF NEW.intent IN ('step-up', 'link') THEN
        SELECT account_id, state INTO bound_account_id, bound_session_state
            FROM portal_sessions WHERE session_sha256 = NEW.session_sha256;
        IF bound_account_id IS DISTINCT FROM NEW.account_id OR
           (NEW.state = 'pending' AND bound_session_state IS DISTINCT FROM 'active') THEN
            RAISE EXCEPTION 'OIDC transaction identity binding is invalid'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER oidc_login_transactions_identity_binding
    AFTER INSERT OR UPDATE ON oidc_login_transactions
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION validate_oidc_login_transaction_identity();

CREATE FUNCTION protect_team_invitation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Team invitations cannot be deleted' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.state <> 'pending' OR NEW.resource_version <> 1 OR
           NEW.accepted_account_id IS NOT NULL OR NEW.accepted_member_id IS NOT NULL OR
           NEW.decided_unix_seconds IS NOT NULL THEN
            RAISE EXCEPTION 'New Team invitations must start pending at version one'
                USING ERRCODE = '55000';
        END IF;
        RETURN NEW;
    END IF;
    IF (NEW.invitation_id, NEW.team_id, NEW.email, NEW.role, NEW.token_sha256,
        NEW.invited_by_member_id, NEW.created_unix_seconds, NEW.expires_unix_seconds) IS DISTINCT FROM
       (OLD.invitation_id, OLD.team_id, OLD.email, OLD.role, OLD.token_sha256,
        OLD.invited_by_member_id, OLD.created_unix_seconds, OLD.expires_unix_seconds) OR
       NEW.resource_version <> OLD.resource_version + 1 OR
       NEW.team_resource_version <= OLD.team_resource_version OR
       OLD.state <> 'pending' OR NEW.state NOT IN ('accepted', 'cancelled', 'expired') OR
       NEW.decided_unix_seconds IS NULL THEN
        RAISE EXCEPTION 'Team invitation transition is invalid' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER team_invitations_state_machine
    BEFORE INSERT OR UPDATE OR DELETE ON team_invitations
    FOR EACH ROW EXECUTE FUNCTION protect_team_invitation();

CREATE FUNCTION validate_team_invitation_relations() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    inviter_team_id TEXT;
    inviter_role TEXT;
    inviter_state TEXT;
    current_team_version BIGINT;
    accepted_team_id TEXT;
    accepted_account_id TEXT;
    accepted_email TEXT;
    accepted_role TEXT;
    accepted_state TEXT;
    accepted_portal_email TEXT;
    accepted_email_verified BOOLEAN;
    accepted_portal_state TEXT;
BEGIN
    SELECT resource_version INTO current_team_version
        FROM teams WHERE team_id = NEW.team_id;
    IF current_team_version IS DISTINCT FROM NEW.team_resource_version THEN
        RAISE EXCEPTION 'Team invitation aggregate binding is invalid'
            USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'INSERT' THEN
        SELECT team_id, role, membership_state
            INTO inviter_team_id, inviter_role, inviter_state
            FROM team_members WHERE member_id = NEW.invited_by_member_id;
        IF inviter_team_id IS DISTINCT FROM NEW.team_id OR
           inviter_role IS DISTINCT FROM 'owner' OR inviter_state IS DISTINCT FROM 'active' THEN
            RAISE EXCEPTION 'Team invitation owner binding is invalid'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    IF NEW.state = 'accepted' THEN
        SELECT team_id, account_id, email, role, membership_state
            INTO accepted_team_id, accepted_account_id, accepted_email, accepted_role,
                 accepted_state
            FROM team_members WHERE member_id = NEW.accepted_member_id;
        SELECT email, email_verified, state
            INTO accepted_portal_email, accepted_email_verified, accepted_portal_state
            FROM portal_accounts WHERE account_id = NEW.accepted_account_id;
        IF accepted_team_id IS DISTINCT FROM NEW.team_id OR
           accepted_account_id IS DISTINCT FROM NEW.accepted_account_id OR
           accepted_email IS DISTINCT FROM NEW.email OR accepted_role IS DISTINCT FROM NEW.role OR
           accepted_state IS DISTINCT FROM 'active' OR
           accepted_portal_email IS DISTINCT FROM NEW.email OR
           accepted_email_verified IS DISTINCT FROM TRUE OR
           accepted_portal_state IS DISTINCT FROM 'active' THEN
            RAISE EXCEPTION 'Accepted invitation membership binding is invalid'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER team_invitations_relation_binding
    AFTER INSERT OR UPDATE ON team_invitations
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION validate_team_invitation_relations();

CREATE FUNCTION require_team_portal_capacity() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    affected_team_id TEXT;
    live_member_count BIGINT;
    pending_invitation_count BIGINT;
BEGIN
    affected_team_id := CASE WHEN TG_OP = 'DELETE' THEN OLD.team_id ELSE NEW.team_id END;
    SELECT COUNT(*) INTO live_member_count FROM team_members
        WHERE team_id = affected_team_id AND membership_state <> 'removed';
    SELECT COUNT(*) INTO pending_invitation_count FROM team_invitations
        WHERE team_id = affected_team_id AND state = 'pending';
    IF live_member_count + pending_invitation_count > 100 THEN
        RAISE EXCEPTION 'Team membership and pending invitation capacity exceeded'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER team_invitations_portal_capacity
    AFTER INSERT OR UPDATE OR DELETE ON team_invitations
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_team_portal_capacity();

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
    IF NEW.member_id <> OLD.member_id OR NEW.team_id <> OLD.team_id OR
       (OLD.account_id IS NOT NULL AND NEW.account_id IS DISTINCT FROM OLD.account_id) THEN
        RAISE EXCEPTION 'Team membership identity cannot be reassigned' USING ERRCODE = '55000';
    END IF;
    IF NEW.resource_version <> OLD.resource_version + 1 THEN
        RAISE EXCEPTION 'Team member resource version must advance by one' USING ERRCODE = '55000';
    END IF;
    IF OLD.membership_state = 'removed' THEN
        RAISE EXCEPTION 'Removed Team memberships are immutable' USING ERRCODE = '55000';
    END IF;
    IF OLD.account_id IS NULL AND NEW.account_id IS NOT NULL AND
       (NEW.email, NEW.role, NEW.two_factor_enabled, NEW.membership_state,
        NEW.removed_unix_seconds) IS DISTINCT FROM
       (OLD.email, OLD.role, OLD.two_factor_enabled, OLD.membership_state,
        OLD.removed_unix_seconds) THEN
        RAISE EXCEPTION 'Linking a Portal account cannot alter membership attributes'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.membership_state <> OLD.membership_state THEN
        IF NOT (
            (OLD.membership_state = 'active' AND NEW.membership_state IN ('suspended', 'removed')) OR
            (OLD.membership_state = 'suspended' AND NEW.membership_state IN ('active', 'removed'))
        ) THEN
            RAISE EXCEPTION 'Team membership state transition is invalid' USING ERRCODE = '55000';
        END IF;
        IF (NEW.email, NEW.role, NEW.two_factor_enabled, NEW.account_id) IS DISTINCT FROM
           (OLD.email, OLD.role, OLD.two_factor_enabled, OLD.account_id) THEN
            RAISE EXCEPTION 'Team membership state changes cannot alter member attributes'
                USING ERRCODE = '55000';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION require_portal_membership_limit() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    affected_account_id TEXT;
BEGIN
    affected_account_id := CASE WHEN TG_OP = 'DELETE' THEN OLD.account_id ELSE NEW.account_id END;
    IF affected_account_id IS NOT NULL AND (
        SELECT COUNT(*) FROM team_members
        WHERE account_id = affected_account_id AND membership_state <> 'removed'
    ) > 8 THEN
        RAISE EXCEPTION 'A Portal account cannot hold more than eight live Team memberships'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER team_members_portal_membership_limit
    AFTER INSERT OR UPDATE OR DELETE ON team_members
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_portal_membership_limit();

CREATE CONSTRAINT TRIGGER team_members_portal_capacity
    AFTER INSERT OR UPDATE OR DELETE ON team_members
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION require_team_portal_capacity();
