# CardputerZero Review Console

Internal React/Vite client for the bounded Store review API. The runtime uses
the Review workforce BFF for session/login/logout and short-lived control-token
exchange, then reads the real Review queue and Submission detail from Store
Control. Queue pagination, primary and independent secondary claims,
append-only messages, structured decisions, immutable hashes, scan findings,
permissions, imports, submitted asset metadata, and bounded audit history all
come from authoritative API responses. Successful mutations are followed by a
fresh server read rather than optimistic local state.

Cookies are sent only to the audience-specific workforce BFF. Control bearer
tokens are held in memory, never persisted, and Store Control requests use
`credentials: omit`. The strict adapters reject redirects, cross-audience
sessions, scope mismatches, malformed objects, and responses larger than 64
KiB.

Set `VITE_REVIEW_WORKFORCE_ORIGIN` to the Review BFF HTTPS origin and
`VITE_REVIEW_CONTROL_ORIGIN` to the Store Control HTTPS origin. Both must be
bare HTTPS origins. Production deployment remains subject to the IdP/JWKS,
managed-key, domain, and revocation-exercise gates documented in
`docs/STORE-WORKFORCE-SERVER.md`.

```sh
npm test
npm run build
npm run dev
```
