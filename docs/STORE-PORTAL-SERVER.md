# Store Portal Server

`cp0-store-portal-server` is the browser-facing authentication BFF for the
Developer Portal. It terminates the Portal session, performs external OpenID
Connect Authorization Code + PKCE exchanges, and reads the Store identity
tables. It is a separate process from `cp0-store-control-server`: browsers do
not receive Store control tokens, upstream tokens, refresh tokens, OIDC
subjects, or signing material.

This slice implements login, callback, session read, MFA step-up, logout, and
the Team invitation lifecycle. Identity-link routes remain disabled until their
transactional acceptance slice is complete.

## HTTP boundary

The implemented routes are:

| Route | Contract |
| --- | --- |
| `GET /portal/auth/login?provider=...` | starts a ten-minute state/nonce/PKCE transaction and redirects to one configured provider |
| `GET /portal/auth/callback` | consumes one transaction, creates or rotates the opaque session, and redirects to the configured Portal URI |
| `GET /portal/v1/session` | returns bounded Account, Team, expiry, MFA freshness, CSRF, and session-version data |
| `POST /portal/v1/session:step-up` | starts an idempotent same-provider MFA challenge |
| `POST /portal/v1/session:logout` | terminally revokes the current session and expires the cookie |
| `GET/POST /portal/v1/teams/{team_id}/invitations` | lists the latest 100 invitations or creates one Owner/MFA-authorized invitation |
| `POST /portal/v1/invitations/{invitation_id}:cancel` | terminally cancels one pending invitation and clears undelivered secret material |
| `POST /portal/v1/invitations:inspect` | returns only Team name, masked email, role, and expiry for a valid pending token |
| `POST /portal/v1/invitations:accept` | binds the verified session email, atomically creates membership, and rotates the session |

The session cookie is `__Host-cp0_portal` with `Secure`, `HttpOnly`,
`SameSite=Strict`, `Path=/`, and no `Domain`. Mutation routes require the exact
configured `Origin`, `Sec-Fetch-Site: same-origin`, the session CSRF value, and
an `Idempotency-Key`. Step-up, invitation creation, and cancellation also
require the relevant strong `ETag` in `If-Match`. Step-up, logout, and
cancellation accept an empty body only; invitation JSON is strictly bounded to
1 KiB with unknown fields rejected.

All responses are `no-store`, use a no-referrer policy and return a bounded,
closed Problem body on failure. Provider tokens, raw external subjects,
database messages, state, nonce, PKCE verifiers, session secrets, and raw
invitation tokens are never returned in JSON.

## Configuration

`CP0_PORTAL_CONFIG` points to a strict JSON file no larger than 64 KiB. Unknown
fields fail startup. Its shape is:

```json
{
  "allowed_origin": "https://developer.cardputerzero.dev",
  "post_login_uri": "https://developer.cardputerzero.dev/portal",
  "providers": [
    {
      "key": "primary",
      "label": "Primary identity",
      "issuer": "https://identity.example.com",
      "authorization_endpoint": "https://identity.example.com/authorize",
      "token_endpoint": "https://identity.example.com/token",
      "client_id": "cardputerzero-portal",
      "client_secret_env": "CP0_OIDC_PRIMARY_SECRET",
      "redirect_uri": "https://developer.cardputerzero.dev/portal/auth/callback",
      "accepted_signing_algorithms": ["RS256"],
      "accepted_mfa_acr": ["urn:example:acr:mfa"],
      "clock_skew_seconds": 60,
      "jwks": {
        "keys": [
          {
            "kty": "RSA",
            "kid": "provider-key-id",
            "use": "sig",
            "alg": "RS256",
            "n": "provider-public-modulus",
            "e": "AQAB"
          }
        ]
      }
    }
  ]
}
```

The example shows the configuration shape; production `jwks` values must be
copied from the reviewed provider key set. One to eight providers are allowed.
The issuer, authorization, token, and redirect URLs must be exact HTTPS URLs
without user information, query, or fragment. Every provider redirect URI must
equal `<allowed_origin>/portal/auth/callback`. Provider keys and issuers must be
unique. Signing algorithms are restricted to RSA PKCS#1, RSA-PSS, and EdDSA;
`none`, HMAC algorithms, symmetric JWKs, and JWK private parameters fail
startup. Static inline JWKS makes key changes an explicit reviewed
configuration rollout and process restart.

Required environment variables:

| Variable | Purpose |
| --- | --- |
| `CP0_PORTAL_DATABASE_URL` | PostgreSQL connection using the Store control migrations |
| `CP0_PORTAL_CONFIG` | absolute path to the strict provider JSON |
| `CP0_PORTAL_CSRF_KEY` | 32-byte unpadded base64url CSRF HMAC key |
| `CP0_PORTAL_NONCE_KEY` | 32-byte unpadded base64url nonce/state HMAC key |
| `CP0_PORTAL_PKCE_KEY` | 32-byte unpadded base64url XChaCha20-Poly1305 key |
| `CP0_PORTAL_SUBJECT_KEY` | 32-byte unpadded base64url issuer/subject HMAC key |
| `CP0_PORTAL_INVITATION_KEY` | 32-byte unpadded base64url invitation-delivery encryption key |

The five purpose keys must be distinct. Provider client secrets are loaded only
from the uppercase environment-variable name specified by
`client_secret_env`; omit that field for a public PKCE client.

`CP0_PORTAL_LISTEN_ADDR` defaults to `127.0.0.1:8790`. A non-loopback bind is
rejected unless `CP0_PORTAL_ALLOW_NON_LOOPBACK=1`; that override does not add
TLS and is valid only behind a reviewed TLS reverse proxy. The proxy must
preserve the original `Origin`, `Sec-Fetch-Site`, cookie, and conditional
headers without broad CORS rewriting.

## Provider and database trust

Token exchange is POST-only to the configured token endpoint, does not follow
redirects, has fixed connection/request timeouts, and rejects responses above
64 KiB. ID tokens require a configured public-key algorithm, exact issuer and
audience, signature, nonce, verified email, bounded issue time, and a subject no
larger than 1024 bytes. Step-up additionally requires a configured ACR and an
`auth_time` no older than five minutes.

The database stores only SHA-256 session/state/nonce/invitation digests, an HMAC
of the issuer and subject, and purpose-separated authenticated encryption of
short-lived PKCE verifiers and pending email-delivery tokens. Sessions expire
after 30 minutes idle or eight hours absolute. Callback consumption,
Account/link creation, session rotation, invitation mutation, revocation,
audit, and outbox work use `SERIALIZABLE` transactions and the database clock.

## Invitation delivery

Creation accepts only `developer`, `release-manager`, or `viewer`, normalizes
the verified ASCII email shape, and requires an active Owner membership,
membership 2FA enabled, and a provider MFA proof no older than five minutes.
One Team may contain at most 100 live members plus pending invitations. The BFF
also limits creation to 20 invitations per Team per hour and three for the same
normalized Team/email pair per day. A removed membership identity cannot be
re-invited through v1.

The raw 256-bit token is stored only as a SHA-256 acceptance digest and as an
XChaCha20-Poly1305 envelope in `portal_invitation_deliveries`. The envelope is
bound to the invitation ID and never enters API responses, audit events,
idempotency records, or the general outbox. `InvitationEmailWorker` leases one
job for 60 seconds, commits before decrypting or calling the supplied
`InvitationMailer`, and passes an acceptance URL with the token in the fragment.
It clears ciphertext after delivery, cancellation, permanent failure, corrupt
ciphertext, or expiry. Transient failures use bounded exponential retry and a
maximum of 16 attempts. The same worker atomically expires seven-day invitations,
advances the Team version, and appends audit/outbox evidence.

The repository supplies the durable worker and a strict adapter trait, not a
production email-vendor adapter. Production enablement requires a reviewed
transactional provider implementation that does not log request bodies or URLs,
plus bounce, suppression, retention, and abuse-response policy. Callers should
run `run_once` in a supervised bounded loop and back off when it returns
`false`.

Production should use a dedicated least-privilege database role, an encrypted
database connection, a secrets manager, structured request-ID logging without
headers or query strings, and health supervision outside this process. OIDC
discovery, dynamic registration, provider redirects, and refresh-token storage
are deliberately unsupported.

## Verification

Run the local gates with Rust 1.85 or later:

```sh
cargo +1.85.1 check -p cp0-store-portal-server --all-targets
cargo clippy -p cp0-store-portal-server --all-targets -- -D warnings
cargo test -p cp0-store-portal-server
```

The PostgreSQL acceptance test requires an isolated disposable database whose
schema may be reset:

```sh
CP0_STORE_TEST_DATABASE_URL='postgresql://...' \
  cargo test -p cp0-store-portal-server --test postgres -- --ignored --nocapture
```

That gate covers PKCE confidentiality, subject HMAC storage, login/session
creation, CSRF and origin rejection, idle refresh, idempotent step-up, verified
MFA rotation, stale-cookie rejection, logout, invitation create/list/inspect/
cancel/accept and exact replay, email success/retry/failure, token secrecy,
acceptance session rotation, expiry, SQL bypass rejection, injected transaction
rollback, and provider-side terminal failure.
