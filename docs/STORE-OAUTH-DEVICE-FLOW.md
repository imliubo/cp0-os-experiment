# Store OAuth Device Flow

S5I implements the developer authorization path used by `cp0ctl store submit`.
It follows the OAuth Device Authorization Grant polling model while keeping the
control plane's existing PostgreSQL identity, RBAC, 2FA, audit, and outbox
boundaries. It is not part of the CardputerZero device image.

## Protocol

1. `cp0ctl` sends `POST /oauth/device/code` with the fixed client `cp0ctl` and
   scope `store.submit`.
2. The service returns a 10-minute device code, a 48-bit user code formatted as
   `XXXX-XXXX-XXXX`, the verification URI, and a 5-second polling interval.
3. An already authenticated developer submits the user code to
   `POST /oauth/device/authorize` with `approve` or `deny` and an idempotency
   key. Approval requires a live bearer identity, role `owner` or `developer`,
   the exact `store.submit` scope, and currently enabled 2FA.
4. `cp0ctl` polls `POST /oauth/token`. Pending authorization returns
   `authorization-pending`; an early poll returns `slow-down` and increases the
   interval by 5 seconds up to 30 seconds.
5. A successful one-time exchange returns a 15-minute bearer token scoped only
   to `store.submit`. Denial, expiry, a consumed code, or a no-longer-eligible
   member cannot issue a token.

All OAuth responses set `Cache-Control: no-store` and `Pragma: no-cache`.
Stable problem codes are `authorization-pending`, `slow-down`, `access-denied`,
and `expired-token`, matching the cases already handled by `cp0ctl`.

## Storage and concurrency

- Device codes and issued access tokens are stored only as SHA-256 digests.
  Their plaintext values exist only in the single successful HTTP response.
- The short user code is stored because the authenticated approval endpoint
  must locate its pending request. It is unique, uppercase, expires after ten
  minutes, and cannot authorize anything without a live developer session.
- Creation is capped at 10,000 active pending records under a transaction-level
  advisory lock. Production ingress must also rate-limit by trusted client and
  network signals before forwarding requests.
- Approval and denial use SERIALIZABLE transactions, row locks, exact
  idempotency replay, and atomic audit/outbox writes.
- Exchange locks the authorization row. The only successful transition is
  `approved -> consumed`, in the same transaction that inserts the access token
  and its audit/outbox records. Concurrent exchanges therefore issue exactly
  one token.
- PostgreSQL triggers reject deletion, identity changes, backwards polling
  clocks, state rollback, token replacement, and token unrevocation.

The audit and outbox payloads contain the device-code digest, member ID,
decision, and scope. They never contain the device code or access token.

## Verification

The PostgreSQL 17 acceptance suite covers malformed clients, pending and early
polling, approval and denial, exact replay, idempotency conflict, missing,
expired, and revoked approver tokens, role/scope/2FA rejection, authorization
expiry, concurrent one-time exchange, use of the issued token against the
Submission API, later token revocation, live member-role and 2FA changes,
plaintext-secret exclusion, and direct SQL state-machine bypass attempts.

```sh
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

Account registration, password/passkey login, 2FA enrollment and challenge
freshness, team/member administration, Portal session management, reviewer SSO,
abuse controls, and production identity recovery remain separate Identity/Teams
work. The current approval route assumes that such a system has already issued
the short-lived developer bearer used to authorize the device.
