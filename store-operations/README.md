# CardputerZero Store Operations

Internal React/Vite client for the Store operator control plane. The local
fixture supports a complete bounded Today layout, a 320x170 device preview,
published-Release selection, SLA-ordered moderation triage, and structured
decisions.

The strict API adapter uses the existing `/v1/editorial/today` and
`/v1/moderation/*` contracts. It requires a short-lived operator bearer token,
sends no cookies, rejects redirects and oversized responses, and binds every
mutation to a random idempotency key and strong ETag.

```sh
npm test
npm run build
npm run dev
```

The fixture is not production identity or Release discovery. A production
deployment must place this build behind the separate workforce SSO/BFF, obtain
tokens outside browser persistence, and add a bounded published-Release query
before replacing fixture data. Production moderation enforcement remains
disabled until policy ownership, two-person approval, reversible Catalog
suppression, notification delivery, security on-call, and retention are
approved and exercised.
