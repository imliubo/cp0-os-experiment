# Store Install Preflight v1

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-INSTALL-PREFLIGHT-V1.zh-CN.md)

S7D makes permission consent, device policy, and storage capacity mandatory
before a Store download is accepted. The System Shell is the trusted consent
surface; applications cannot connect to `cp0-stored`, and root remains an
explicit administrative authority.

## Two-step protocol

The Shell first submits the exact verified Catalog sequence and one through
eight sorted application IDs:

```json
{
  "name": "preflight-install",
  "app_ids": ["dev.cardputerzero.example"],
  "catalog_sequence": 42
}
```

The daemon rejects an expired or different Catalog, unknown or active
applications, a disabled Store, an application outside the device allowlist,
and insufficient persistent or handoff capacity. Success returns a single-use
authorization plus the exact signed identity and permissions:

```json
{
  "kind": "install-preflight",
  "authorization_id": 91,
  "catalog_sequence": 42,
  "required_bytes": 50331648,
  "available_bytes": 201326592,
  "apps": [{
    "app_id": "dev.cardputerzero.example",
    "version": "2.0.0",
    "permissions": ["camera.capture", "network.client"],
    "policy_denied_permissions": ["camera.capture"]
  }]
}
```

The Shell compares every returned ID, version, and permission bit with its
current Catalog view. If the install or update adds permissions or policy blocks
any requested permission, it presents a trusted confirmation whose default
selection is Cancel. The final request
contains the authorization ID:

```json
{
  "name": "install",
  "app_id": "dev.cardputerzero.example",
  "authorization_id": 91
}
```

`install-batch` carries the same authorization ID and the exact preflight ID
list. Authorization expires after 60 seconds, is consumed on the first install
attempt, and cannot be replayed. Before publishing queued state, the daemon
again checks policy, capacity, Catalog sequence, complete Catalog application
objects, version, package digest, size, and permissions. A different ID,
reordered batch, later Catalog, expired authorization, or replay is rejected.

## Policy behavior

`/etc/cardputerzero/device-policy.json` remains the root-owned upper bound.
Preflight loads it through the same strict schema and secure-file checks used by
appd. `store_install_allowed=false` and an allowlist miss reject before network
or storage mutation. appd repeats Store/allowlist enforcement at atomic
handoff, so a Store client cannot weaken the final installation boundary.

Globally denied SDK permissions are returned as a sorted subset of the signed
requested permissions. They do not make package bytes unsafe and therefore do
not block installation, but the Shell marks them as policy-blocked. appd still
denies those capabilities before any runtime user permission prompt, including
when a prior per-application decision allowed them.

## Capacity model

The persistent data check reserves 16 MiB for system operation, the complete
new extracted version of every accepted application, and every package byte
that is not already present in a valid digest-named partial file. Completed
package files remain available for verification and retry, so this is a
conservative batch end-state bound rather than only the next network chunk.

The appd inbox is checked separately because it is normally on `/run`. It must
hold the largest package in the serial batch plus 8 MiB headroom. Both checks
use filesystem blocks available to the unprivileged `cp0-store` UID. A missing,
symbolic, public, oversized, or non-regular partial file fails closed instead
of reducing the estimate.

The preflight returns persistent `required_bytes` and `available_bytes` for the
confirmation UI. `insufficient-storage` is distinct from network, verification,
installer, policy, and Catalog failures. Resume retains its original approved
Catalog identity but repeats current policy and both capacity checks before it
returns to queued state.

## User and operator surfaces

The 320x170 confirmation shows application count, distinct newly requested
permission count, distinct policy-blocked permission count, and bounded
required/free storage values. Install and Cancel use fixed dimensions; Cancel
is initially selected. A policy, storage, Catalog, or service preflight error
uses a closed trusted message and never displays daemon-controlled text.

`cp0ctl` requires an explicit operator assertion and performs the same
list/preflight/authorized-install sequence:

```sh
sudo cp0ctl store install dev.cardputerzero.example --approve-permissions
sudo cp0ctl store install-batch --approve-permissions \
  dev.cardputerzero.alpha dev.cardputerzero.beta
```

## Verification

Rust tests cover bounded strict request/response parsing, Catalog sequence and
full-object binding, exact policy-denied permission subsets, Store policy
rejection, insufficient capacity, wrong authorization, successful consumption,
and replay refusal. C tests reject mismatched sequence, IDs, permissions,
policy subsets, shapes, and error vocabulary. UI tests cover default Cancel,
explicit confirmation, error dismissal, and 320x170 confirmation/storage
snapshots. The final AArch64 System Shell links with warnings treated as errors.
