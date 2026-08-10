# CardputerZero Store Operations

<!-- doc-locale: en -->
> **English** | [简体中文](README.zh-CN.md)

Internal React/Vite client for the Store operator control plane. The runtime
uses the Operations workforce BFF for session/login/logout and audience-scoped
control-token exchange. It reads and writes the real bounded Today layout,
loads published Release candidates with keyset pagination, renders the 320x170
device preview, and reads and decides the real SLA-ordered moderation queue.
Successful mutations are followed by a fresh server read.

The strict API adapter uses `/v1/editorial/releases`, `/v1/editorial/today`,
and `/v1/moderation/*`. Published Release discovery validates the exact `rel_`
identity, current Catalog sequence order, unique Apps, canonical cursor, and
bounded response before the picker can consume it. The adapter requires a
short-lived in-memory operator bearer token. Cookies are sent only to the BFF;
Store Control requests use `credentials: omit`. The client rejects redirects,
cross-audience or scope mismatches, malformed objects, and responses larger
than 64 KiB, and binds every mutation to a random idempotency key and strong
ETag. Editors receive only `store.editorial`; admins may also receive
`store.moderation`.

Set `VITE_OPERATIONS_WORKFORCE_ORIGIN` to the Operations BFF HTTPS origin and
`VITE_OPERATIONS_CONTROL_ORIGIN` to the Store Control HTTPS origin. Both must
be bare HTTPS origins.

```sh
npm test
npm run build
npm run dev
```

Production deployment remains blocked on real IdP/JWKS integration, managed
keys, production domains, and a live revocation exercise. Production moderation
enforcement also remains disabled until policy ownership, two-person approval,
reversible Catalog suppression, notification delivery, security on-call, and
retention are approved and exercised.
