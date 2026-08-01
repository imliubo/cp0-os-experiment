# CardputerZero Developer Portal

Local S4 frontend MVP for the Store control plane. The Portal is a separate Web
application and is never installed in the CardputerZero device image.

Store content workflows still use bounded in-memory demo data. Account security
uses the real Portal BFF when `VITE_PORTAL_BFF_ORIGIN` is configured: session,
MFA step-up, identity list/link/remove, logout, and invitation methods are
implemented by the cookie/CSRF `PortalApi`. Refreshing the page resets demo Store
content but reloads identity state from the BFF.

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

For a same-site production build:

```sh
VITE_PORTAL_BFF_ORIGIN=https://developer.cardputerzero.dev \
VITE_PORTAL_PROVIDERS=primary,secondary npm run build
```

The BFF origin must be a bare HTTPS origin. `PortalApi` sends browser credentials
only to `/portal/v1/*`, keeps CSRF in memory, rejects redirects and oversized
responses, and never accepts an OIDC or Store bearer token from application code.
