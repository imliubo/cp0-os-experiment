# Store Workforce Server

`cp0-store-workforce-server` is the browser authentication BFF for Review Console and Store
Operations. It runs independently from the Store control server and Developer Portal. Review and
Operations share implementation but never share origins, cookies, sessions, callback URIs or Store
control tokens.

## HTTP boundary

Review routes are `/review/auth/login`, `/review/auth/callback`, `/review/v1/session`,
`/review/v1/token` and `/review/v1/session:logout`. Operations uses the same route suffixes under
`/operations`. Login creates a ten-minute OIDC state/nonce/PKCE transaction. Callback requires fresh
MFA, resolves only a pre-provisioned workforce link and creates a 15-minute idle/eight-hour absolute
opaque session. The BFF never creates a reviewer or operator.

Session responses include the audience-specific principal, role, deadlines, resource version and
CSRF token. Review can request only `store.review`. Operations requests exactly one authorized
`store.editorial` or `store.moderation` scope. Tokens expire within five minutes and are bound to the
live session, identity link and principal. Logout is idempotent and revokes every bound token in the
same database transaction.

Cookie-authenticated mutations require the browser-generated exact `Origin`,
`Sec-Fetch-Site: same-origin`, the session CSRF value and a 16-128 byte `Idempotency-Key`. Review uses
`__Host-cp0_review`; Operations uses `__Host-cp0_operations`. Both are `Secure`, `HttpOnly`,
`SameSite=Strict`, `Path=/` cookies without a `Domain` attribute. Responses are bounded, `no-store`
and do not expose OIDC subjects, provider tokens, cookies, CSRF values or Store bearer tokens in
audit records.

## Configuration

`CP0_WORKFORCE_CONFIG` points to strict JSON no larger than 128 KiB:

```json
{
  "review": {
    "allowed_origin": "https://review.cardputerzero.dev",
    "post_login_uri": "https://review.cardputerzero.dev/queue",
    "providers": [{
      "key": "primary",
      "label": "Workforce identity",
      "issuer": "https://identity.example.com",
      "authorization_endpoint": "https://identity.example.com/authorize",
      "token_endpoint": "https://identity.example.com/token",
      "client_id": "cardputerzero-review",
      "client_secret_env": "CP0_OIDC_REVIEW_SECRET",
      "redirect_uri": "https://review.cardputerzero.dev/review/auth/callback",
      "accepted_signing_algorithms": ["RS256"],
      "accepted_mfa_acr": ["urn:example:acr:mfa"],
      "clock_skew_seconds": 60,
      "jwks": { "keys": [] }
    }]
  },
  "operations": {
    "allowed_origin": "https://operations.cardputerzero.dev",
    "post_login_uri": "https://operations.cardputerzero.dev/console",
    "providers": [{
      "key": "primary",
      "label": "Workforce identity",
      "issuer": "https://identity.example.com",
      "authorization_endpoint": "https://identity.example.com/authorize",
      "token_endpoint": "https://identity.example.com/token",
      "client_id": "cardputerzero-operations",
      "client_secret_env": "CP0_OIDC_OPERATIONS_SECRET",
      "redirect_uri": "https://operations.cardputerzero.dev/operations/auth/callback",
      "accepted_signing_algorithms": ["RS256"],
      "accepted_mfa_acr": ["urn:example:acr:mfa"],
      "clock_skew_seconds": 60,
      "jwks": { "keys": [] }
    }]
  }
}
```

The inline JWKS values above are placeholders; production startup requires reviewed public keys.
Both origins must be distinct bare HTTPS origins. Every redirect URI must exactly equal the matching
origin plus its fixed callback path. OIDC client secrets are loaded only through valid uppercase
environment-variable names referenced by the configuration.

Required environment variables are `CP0_WORKFORCE_DATABASE_URL`, `CP0_WORKFORCE_CONFIG`,
`CP0_WORKFORCE_CSRF_KEY`, `CP0_WORKFORCE_NONCE_KEY`, `CP0_WORKFORCE_PKCE_KEY`,
`CP0_WORKFORCE_SUBJECT_KEY` and `CP0_WORKFORCE_CONTROL_TOKEN_KEY`. The five keys are distinct 32-byte
unpadded base64url values. The listener defaults to `127.0.0.1:8791`. A non-loopback bind requires
`CP0_WORKFORCE_ALLOW_NON_LOOPBACK=1` and a reviewed external TLS reverse proxy.

## Frontend adapters and verification

Review Console and Store Operations fetch BFF session/token/logout routes with
`credentials: include`. Short Store bearer tokens are kept only in memory, refreshed before expiry
and validated for exact audience and scope. Store control requests remain `credentials: omit`.
Operations holds separate in-memory entries for editorial and moderation scopes.

The ignored PostgreSQL acceptance test uses fake OIDC providers and a fresh Store database:

```sh
CP0_STORE_TEST_DATABASE_URL='postgresql://...' make store-control-db-check
```

It verifies audience/cookie/callback separation, raw-subject absence, exact token replay, scope
authorization, Control API use, idempotency conflict, logout, link/principal revocation and
secret-free audit evidence. A production IdP/JWKS rollout, managed secret storage and live access
revocation exercise remain deployment gates.
