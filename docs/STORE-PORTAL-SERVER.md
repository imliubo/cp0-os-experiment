# Store Portal Server

`cp0-store-portal-server` is the browser-facing authentication BFF for the
Developer Portal. It terminates the Portal session, performs external OpenID
Connect Authorization Code + PKCE exchanges, and reads the Store identity
tables. It is a separate process from `cp0-store-control-server`: browsers do
not receive Store control tokens, upstream tokens, refresh tokens, OIDC
subjects, or signing material.

This slice implements login, callback, session read, MFA step-up, and logout.
Identity-link and invitation routes remain disabled until their own
transactional acceptance slices are complete.

## HTTP boundary

The implemented routes are:

| Route | Contract |
| --- | --- |
| `GET /portal/auth/login?provider=...` | starts a ten-minute state/nonce/PKCE transaction and redirects to one configured provider |
| `GET /portal/auth/callback` | consumes one transaction, creates or rotates the opaque session, and redirects to the configured Portal URI |
| `GET /portal/v1/session` | returns bounded Account, Team, expiry, MFA freshness, CSRF, and session-version data |
| `POST /portal/v1/session:step-up` | starts an idempotent same-provider MFA challenge |
| `POST /portal/v1/session:logout` | terminally revokes the current session and expires the cookie |

The session cookie is `__Host-cp0_portal` with `Secure`, `HttpOnly`,
`SameSite=Strict`, `Path=/`, and no `Domain`. Mutation routes require the exact
configured `Origin`, `Sec-Fetch-Site: same-origin`, the session CSRF value, and
an `Idempotency-Key`; step-up also requires the session `ETag` in `If-Match`.
The two current mutation routes accept an empty body only.

All responses are `no-store`, use a no-referrer policy and return a bounded,
closed Problem body on failure. Provider tokens, raw external subjects,
database messages, state, nonce, PKCE verifiers, and session secrets are never
returned in JSON.

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

The four purpose keys must be distinct. Provider client secrets are loaded only
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

The database stores only SHA-256 session/state/nonce digests, an HMAC of the
issuer and subject, and authenticated encryption of the short-lived PKCE
verifier. Sessions expire after 30 minutes idle or eight hours absolute.
Callback consumption, Account/link creation, session rotation, revocation,
audit, and outbox work use `SERIALIZABLE` transactions and the database clock.

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
MFA rotation, stale-cookie rejection, logout, transaction replay rejection,
and provider-side terminal failure.
