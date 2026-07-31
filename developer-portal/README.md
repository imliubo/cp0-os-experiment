# CardputerZero Developer Portal

Local S4 frontend MVP for the Store control plane. The Portal is a separate Web
application and is never installed in the CardputerZero device image.

The current adapter uses bounded in-memory demo data. It demonstrates account,
team, role, 2FA, public developer key, permanent App ID, Listing, submission,
review timeline, schedule, rollout, pause, resume and removal workflows against
the frozen Store state machines. Refreshing the page resets all changes.

The Portal never asks for or stores a developer private key. A production client
must provide the short-lived OAuth token to `StoreApi` in memory. The API client
requires a bare HTTPS origin, omits browser credentials, refuses redirects,
limits JSON responses to 64 KiB, adds idempotency keys to writes, and requires
ETags for existing-resource mutations.

## Development

```sh
npm install
npm run check
npm run dev
```

The UI supports desktop and mobile widths, physical keyboard navigation, visible
focus, reduced motion, and native form controls. `npm run check` runs the state
model/API boundary tests before creating the production bundle.
