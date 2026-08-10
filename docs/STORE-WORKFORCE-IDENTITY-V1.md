# Store Workforce Identity v1

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-WORKFORCE-IDENTITY-V1.zh-CN.md)

This document freezes the provider-neutral identity boundary shared by Review Console and Store
Operations. It defines the BFF contract and the Store control-plane persistence invariants; it does
not select or emulate a production identity provider.

## Trust boundary

Review and Operations audiences never share a browser session, cookie, callback path or control
token. Review uses `__Host-cp0_review` and the `review` audience. Operations uses
`__Host-cp0_operations` and the `operations` audience. Both cookies are `Secure`, `HttpOnly`,
`SameSite=Strict`, have `Path=/`, and have no `Domain` attribute.

The BFF is the only OIDC client. It never forwards an ID token, provider access token or refresh
token to either browser application or the Store control API. The browser receives only a random
session cookie and, when explicitly requested, a short-lived Store control token for its exact
audience and scope.

## Login and session

1. The BFF resolves a configured provider key and creates a one-time OIDC transaction. It stores
   SHA-256 digests of `state` and `nonce`, encrypted PKCE verifier bytes, the provider configuration
   digest, audience and intent. The transaction expires after ten minutes and is terminal after
   consumption or expiry.
2. The callback verifies the exact issuer, client audience, signature, `state`, `nonce`, PKCE,
   provider configuration digest and MFA evidence. Redirect targets are fixed same-origin paths,
   never request parameters.
3. The normalized issuer and subject are keyed through HMAC-SHA-256 before persistence. Raw OIDC
   subjects, ID tokens and provider tokens are never stored in Store tables or logs.
4. An active identity link must resolve to one active, 2FA-enabled reviewer or operator in the
   matching audience. A principal has at most one active workforce link.
5. A session has a 15-minute idle lifetime and an eight-hour absolute lifetime. It stores only
   SHA-256 digests of its cookie and CSRF secret. Activity can move the idle deadline forward but
   can never extend the absolute deadline or restore a terminal session.

MFA evidence must come from a provider configuration whose accepted `acr`, `amr` and maximum
`auth_time` age are deployment policy. Merely having a local `two_factor_enabled` flag is not proof
that the current OIDC transaction performed MFA.

## Control tokens

The token endpoints require the audience-specific session cookie, `X-CSRF-Token` and an
`Idempotency-Key`. Responses use `Cache-Control: no-store`. Review issues only `store.review`.
Operations issues exactly one of `store.editorial` or `store.moderation`, subject to the current
operator role.

A control token is opaque and purpose-key-derived from its session, audience, scope and idempotency
key, so an exact replay reconstructs the same bearer without storing it. Only its SHA-256 digest is
stored. It is bound to one session and valid for no more than five minutes. Its expiry cannot exceed
the session idle deadline or absolute deadline. Every Store
authentication query rechecks all of the following rather than trusting issuance alone:

- token digest, expiry and revocation state;
- active session plus current idle and absolute deadlines;
- active identity link;
- exact Review or Operations audience;
- exact reviewer or operator bound to both link and token;
- token creation at or after session creation and the five-minute maximum.

Legacy unbound test/administrative tokens remain schema-compatible during migration. Production
browser traffic must use session-bound tokens; removing the nullable compatibility path requires a
separate credential migration and deployment gate.

## Immediate revocation

Logout terminally revokes the session and immediately revokes every bound control token. Revoking
an identity link terminally revokes every active session for that link, which in turn revokes its
tokens. Suspending a reviewer or operator performs the same session-to-token cascade. Authentication
also checks the live principal, session and link rows, so a failed or delayed worker is never the
security boundary.

Identity links, sessions and OIDC transactions are append-preserving state machines: terminal rows
cannot be restored or deleted. Access tokens can transition only from unrevoked to revoked. Database
triggers enforce each transition in the same transaction as its parent revocation.

## Browser and deployment controls

- Review and Operations are separate origins with fixed callback allowlists and no permissive CORS.
- Every cookie-authenticated mutation requires the matching CSRF secret and rejects cross-origin
  requests before parsing a body.
- Authentication and callback responses are bounded, use `no-store`, and never reflect provider
  errors, claims, tokens or secrets.
- Subject-HMAC, PKCE encryption, CSRF, nonce and control-token keys are distinct versioned secrets
  outside PostgreSQL. Rotation needs an overlap procedure and independent audit evidence.
- Login, callback, token issuance, logout, link revocation and principal suspension emit bounded
  audit events without cookie, CSRF, OIDC or bearer secrets.
- Provider metadata, signing keys, issuer allowlists, MFA policy, clock skew and emergency disable
  controls are production configuration, not browser input.

## Delivered evidence and remaining gates

Migrations `0027_workforce_identity_foundation.sql` and
`0028_workforce_bff_operations.sql` implement separate links, sessions, OIDC transactions,
session-bound tokens, idempotent issuance/logout records and synchronous revocation.
`cp0-store-workforce-server` implements the production-shaped BFF routes and strict dual-origin
configuration. PostgreSQL acceptance covers valid Review and Operations authentication, audience
mismatch, token lifetime/replay, Control API use, scope authorization, all three revocation paths,
terminal immutability, secret-free audit evidence and HTTP rejection after revocation. Review Console
and Store Operations include strict in-memory session/token adapters. The OpenAPI contract is
`schemas/store-workforce-identity-v1.openapi.json`; deployment details are in
`STORE-WORKFORCE-SERVER.md`.

Production IdP/JWKS configuration, managed secret storage, workforce account provisioning and a live
access-revocation exercise remain deployment gates. No fake provider or local acceptance database is
production SSO evidence.
