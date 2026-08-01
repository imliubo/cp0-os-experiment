# Store Identity and Teams

S5K freezes the production authentication boundary and implements the first
team-administration slice. The Store control server remains an OAuth resource
server; it does not store passwords, passkey private material, recovery codes,
or upstream OIDC tokens.

## Implemented API slice

- `GET /v1/teams/{team_id}` returns the caller's team and at most 100 members;
- `POST /v1/teams/{team_id}/members/{member_id}:set-role` changes one role;
- `POST /v1/teams/{team_id}/members/{member_id}:remove` irreversibly removes
  one active or suspended membership;
- `POST /v1/teams/{team_id}/members/{member_id}:suspend` disables one active
  membership and `:restore` returns one suspended membership to active;
- remove, suspend, and restore accept an empty body only;
- roles are exactly `owner`, `developer`, `release-manager`, and `viewer`;
- cross-team IDs return `not-found` and never disclose membership data.

Team reads require `store.teams.read`, `store.teams.write`, or internal
`store.control`. A role or membership-state change requires the current `owner` role,
`store.teams.write` or `store.control`, enabled 2FA, and an MFA authentication
time no more than five minutes old. The operation also requires the current
team ETag and an idempotency key.

One PostgreSQL `SERIALIZABLE` transaction locks the Team and target member and
advances both resource versions by exactly one. A role change updates the role;
a removal transitions an active or suspended membership to terminal `removed`
and records its database time without deleting the identity. Suspension and
restoration transition only `active -> suspended -> active`; both revoke every
existing target token, so restoration never revives a token created or retained
while suspended. All mutations append audit/outbox records. Suspended members
remain visible in Team responses but cannot authenticate; removed members are
hidden and cannot authenticate. The database and service both preserve the last
active Owner. Exact replay returns the stored Team body and ETag without another
mutation.

## Production login boundary

The production Portal uses a server-side BFF and an external OpenID Connect
provider:

1. the BFF uses Authorization Code with PKCE, exact redirect URIs, issuer and
   audience allowlists, state, nonce, and bounded clock skew;
2. the provider owns account proofing, passkeys/WebAuthn, MFA enrollment,
   recovery, abuse controls, and credential notifications;
3. the BFF maps the immutable `(issuer, subject)` pair to a Store member and
   creates only a hashed, short-lived opaque control token;
4. `mfa_authenticated_unix_seconds` is populated only after a configured,
   verified MFA assurance claim; token creation time alone is not MFA proof;
5. the browser receives only an opaque `Secure`, `HttpOnly`, `SameSite=Strict`
   Portal session cookie. Control tokens and OIDC refresh tokens remain in the
   BFF; writes also require same-origin CSRF protection;
6. the Portal session has a 30-minute idle and eight-hour absolute lifetime.
   Sensitive operations force a fresh provider challenge when the five-minute
   step-up window has elapsed.

The IdP vendor, accepted ACR values, enterprise federation, account-linking
policy, and recovery escalation must be configured and reviewed before login
is enabled. An email address is display/contact data, never a stable login key;
the external issuer and subject form the identity.

Reviewer identities remain a separate workforce SSO domain. A developer Team
role can never grant `store.review` or become a reviewer through this API.

## Database constraints

- Team identities cannot be reassigned or deleted; member identities cannot be
  reassigned or physically deleted;
- memberships start active at version one, support only active/suspended
  transitions before terminal removal, and become immutable after removal;
- every Team/member update advances its resource version by exactly one;
- member email values are bounded, trimmed, and lowercase;
- MFA authentication time is optional, immutable, positive, and cannot be
  later than token creation;
- access tokens can only transition to revoked and cannot be deleted;
- the deferred last-Owner constraint remains the final database backstop.

Existing tokens migrated from pre-S5K data receive no MFA authentication time,
so they fail closed for sensitive team writes until a real step-up token is
issued.

## Verification

The PostgreSQL acceptance gate covers bounded ordered reads, missing scope,
role and team isolation, disabled and stale MFA, last-Owner protection, stale
ETags, exact replay, immediate target-token revocation, suspension/restoration,
terminal removal, database bypasses, and injected role/removal/suspension audit
failures proving complete rollback. The Portal API tests bind role and lifecycle
changes to ETag/idempotency headers; the local Portal UI exposes suspend/restore,
requires explicit removal confirmation, and disables suspension or removal of
the final active Owner.

The provider-neutral account/linking, invitation, OIDC callback and Portal
session boundary is frozen in `STORE-PORTAL-IDENTITY-V1.md` and
`store-portal-identity-v1.openapi.json`. Its PostgreSQL Account, identity-link,
OIDC transaction, session, invitation and Membership-binding state machines
are implemented with SQL bypass acceptance. The dedicated Portal BFF implements
the external OIDC login, callback, digest-only session, CSRF, idle/absolute
expiry, MFA step-up rotation, and logout slice with PostgreSQL end-to-end
acceptance. Identity-link and invitation HTTP flows, provider MFA
enrollment/recovery, reviewer SSO, and production abuse controls remain
unimplemented. Re-inviting a removed external identity requires a future,
separately versioned membership record design; this endpoint never reactivates
a removed row.
