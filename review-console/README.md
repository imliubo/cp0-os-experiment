# CardputerZero Review Console

Internal React/Vite client for the bounded Store review API. The local fixture
supports queue filtering, primary and independent secondary claims, append-only
messages, structured decisions, immutable hashes, scan findings, capabilities,
submitted-screen inspection, and audit history.

The browser client never stores reviewer credentials and sends no cookies. A
production deployment must place this build behind the workforce SSO/BFF trust
boundary documented in `docs/STORE-IDENTITY-TEAMS.md`.

```sh
npm test
npm run build
npm run dev
```
