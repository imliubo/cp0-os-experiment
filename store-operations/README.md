# CardputerZero Store Operations

Internal React/Vite client for the Store operator control plane. The local
fixture supports a complete bounded Today layout, a 320x170 device preview,
published-Release selection, SLA-ordered moderation triage, and structured
decisions.

The strict API adapter uses `/v1/editorial/releases`, `/v1/editorial/today`,
and `/v1/moderation/*`. Published Release discovery validates the exact `rel_`
identity, current Catalog sequence order, unique Apps, canonical cursor, and
bounded response before the picker can consume it. The adapter requires a
short-lived operator bearer token, sends no cookies, rejects redirects and
oversized responses, and binds every mutation to a random idempotency key and
strong ETag.

```sh
npm test
npm run build
npm run dev
```

The UI remains a local fixture until production identity is connected. A
production deployment must place this build behind the separate workforce
SSO/BFF, obtain tokens outside browser persistence, and use the bounded
published-Release adapter before replacing fixture data. Production moderation
enforcement remains disabled until policy ownership, two-person approval,
reversible Catalog suppression, notification delivery, security on-call, and
retention are approved and exercised.
