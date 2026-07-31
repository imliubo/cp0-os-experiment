# Phase 5B: reviewed application store

## Trust chain

The store never turns a network response directly into an installed
application. The complete production chain is:

```text
developer-signed .capp
        |
exact review metadata (submission SHA-256, permissions, WASM imports)
        |
cp0ctl store publish: validate, store-sign, build signed catalog
        |
HTTPS catalog and package download through cp0-stored
        |
cp0-stored verifies catalog key, package size and SHA-256
        |
appd independently verifies source identity and both package signatures
        |
atomic version install and registry activation
```

The developer signature identifies the source submission. The store signature
states that the exact immutable submission passed review. A package is not
approved merely because its application ID, version or developer key matches a
previous review.

Store catalog keys are raw Ed25519 public keys installed below
`/etc/cardputerzero/trust/store/<key-id>.pub`. The directory is root-owned and
read with no-follow semantics. Store signing secrets exist only on the release
host and never enter an image or device.

## Review and deterministic publishing

Each submission has one review file named
`<app-id>-<version>.review.json`, validated against
`schemas/store-review-v1.schema.json`. An approved record binds:

- application ID and semantic version;
- SHA-256 of the complete developer-signed submission;
- reviewer identity and review time;
- the exact sorted manifest permission set;
- the exact sorted CardputerZero WASM import set;
- a bounded user-facing summary.

`cp0ctl store publish` decodes the canonical `.capp`, verifies its developer
signature, rejects an existing store signature, validates SDK compatibility,
validates the WASM module with `wasmparser`, and rejects non-function or
unsupported imports. Every capability import must have its corresponding
manifest permission. Publishing fails if review metadata differs from any of
these inspected values.

The publisher store-signs each approved package, sorts applications by ID, and
writes a signed `catalog.json`, immutable package paths and `store.pub` into a
new output directory. Given identical inputs, key and timestamps, the output
bytes are identical. The command refuses to merge into an existing directory.

```sh
cargo run -p cp0ctl -- store publish \
  submissions reviews public-store https://store.example.invalid \
  42 1800000000 1800600000 store.key
```

The output directory is a static HTTPS origin. Deployment of that origin and
reviewer workflow authorization are outside the device trust boundary.

## Device service boundary

`cp0-stored` runs as the dedicated `cp0-store` user with a 40 MiB cgroup limit,
no capabilities or device access, and only `AF_UNIX`, `AF_INET` and `AF_INET6`.
It owns only its cache and the narrow appd staging inbox:

| Path | Owner/mode | Purpose |
|---|---|---|
| `/etc/cardputerzero/store.conf` | `root:root 0644` | HTTPS catalog URL |
| `/etc/cardputerzero/trust/store` | `root:root`, non-writable | catalog trust keys |
| `/var/lib/cardputerzero/store` | `cp0-store:cp0-store 0700` | catalog and partial downloads |
| `/run/cardputerzero-appd/store` | `cp0-store:cp0-store 0700` | one-file appd handoff |
| `/run/cardputerzero-store/control.sock` | `root:cp0-control 0660` | bounded Shell protocol |

The Shell may list, refresh and request installation. It cannot provide a URL,
package path, hash, version or signature. `cp0-stored` selects all of those from
the verified catalog. The Shell never downloads an application.

After a verified download, `cp0-stored` creates a private handoff file and asks
appd to perform `StoreInstall`. appd accepts that command only from the fixed
`cp0-store` UID. It independently checks regular-file type, owner, mode, byte
count, SHA-256, catalog identity, manifest identity, developer signature and
store signature. Store installation accepts only a strict semantic-version
upgrade; root-mediated rollback remains a separate operation.

## Network and rollback protection

Both catalog and package URLs must use HTTPS without credentials or fragments.
The same public-address resolver as `networkd` rejects loopback, private,
link-local, multicast, reserved and transition addresses on every resolution.
Environment proxies are disabled, redirects are bounded, response sizes are
bounded, and TLS verification cannot be disabled.

Catalogs have a non-zero monotonic sequence and a bounded validity interval.
The device rejects expired or future-dated catalogs, lower sequences, and
different content reusing the current sequence. A successfully verified
catalog is replaced atomically. An expired cached catalog may be displayed as
stale but cannot authorize installation.

Packages download into SHA-256-named `.part` files. Resume requires a matching
HTTP range response and content range. The final byte count and SHA-256 are
checked through the already-open descriptor before handoff, so pathname
replacement cannot change the verified content.

## Shell behavior

The 320x170 System Shell has a dedicated Store entry. The list retains at most
32 of the protocol's bounded 64 catalog applications and displays up to four
rows at once. Enter opens a detail view containing version, review summary,
state and all approved permissions; a second Enter requests installation.
Right requests a catalog refresh, and Escape returns from detail to list and
then Home.

The Shell reconciles catalog entries with appd's installed-version list:

- exact installed version: `INSTALLED`;
- another installed version: `UPDATE`;
- no installed version: `GET`;
- queued, download and install progress: retained from `cp0-stored` and never
  overwritten by reconciliation.

Version ordering is deliberately absent from the Shell. appd is the security
authority for strict upgrade semantics.

The same bounded control path is available for hardware acceptance without
giving an operator a URL, package path, hash or signature override:

```sh
sudo cp0ctl store list
sudo cp0ctl store search notes
sudo cp0ctl store search notes 8 8
sudo cp0ctl store refresh
sudo cp0ctl store install dev.cardputerzero.example
```

`cp0ctl` rejects a response whose kind does not match the request, an install
response for a different application ID, search results for a different query
or page, an uncorrelated request ID or any malformed protocol field. Search is
performed locally by `cp0-stored` against the verified Catalog; the query never
leaves the device. These commands do not change the product Store configuration
or trust roots.

## Offline and unconfigured operation

Product images ship with an empty `catalog_url` and no production trust key.
The Store screen reports `NOT CONFIGURED` until an operator provisions both.
This prevents a development endpoint from becoming an implicit product trust
root.

When the network is unavailable, a previously verified unexpired cached
catalog remains browsable. Partial package bytes remain private and can resume
after connectivity returns; offline installation is not claimed. A stale
catalog is visibly marked and cannot install. With no verified cache, the Shell
reports the Store as unavailable. Store failure never prevents local installed
applications from launching.

## Verification

Automated coverage includes signed catalog bounds, expiry and sequence
rollback, same-sequence equivocation, HTTPS/public-address enforcement,
resumable downloads, malicious Content-Range variants, deterministic catalog
and protocol frame mutation, package hash failure, peer UID authorization,
strict upgrade enforcement, review/import/permission binding, malformed Shell
responses, Store navigation and 320x170 screenshot regression.

The AArch64 Store controls were hot-deployed on 2026-07-31. With the product
configuration intentionally empty, `sudo cp0ctl store list` reached
`cp0-stored` and returned the expected structured `Unconfigured` error without
starting a download or modifying application state. This verifies the device
control path, not the online Store acceptance below.

Before a product endpoint is enabled, complete a real-device run covering
refresh, interrupted download/resume, install, launch, update, expired catalog,
offline cached catalog and power loss during appd handoff.
