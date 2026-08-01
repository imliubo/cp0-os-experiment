# Store automatic application updates v1

S7F adds an explicitly enabled, fail-closed automatic update path for reviewed
applications. It does not add unattended application installation: only an
already installed application can be considered, and the default user
preference is off.

## Policy and preference

`/etc/cardputerzero/device-policy.json` has an independent
`store_auto_update_allowed` upper bound. The field is optional for policy v1 so
deployed policies remain readable, but an omitted field decodes to `false`.
The product policy files set it explicitly. `store_install_allowed` and the
application allowlist still apply.

The user preference is owned by `cp0-stored` and stored at
`/var/lib/cardputerzero/store/auto-update.json`. A missing file means disabled.
The file is a strict, bounded schema, must be a real private file owned by the
service UID, and is replaced with `0600`, `fsync`, atomic rename, and parent
directory `fsync`. It contains only the enabled bit and the last check time.

The Store IPC adds these strict commands:

```json
{"name":"get-auto-update"}
{"name":"set-auto-update","enabled":true}
{"name":"run-auto-update"}
```

Status returns `enabled`, `policy_allowed`, `charging`, `unmetered_network`,
`due`, and `checking`. The System Shell exposes the switch under
`Settings -> Apps & Privacy -> Auto App Updates`; locked, checking, wait-power,
wait-wired, due, and on states fit the 320x170 four-row viewport.

## Scheduling and network gate

The daemon checks at most once every six hours. The last check time is written
before network work begins, so a failing endpoint cannot cause a request or SD
write loop. A backward wall-clock jump permits one new check and then records
the lower time. Enabling the preference may start a due check immediately;
otherwise a bounded daemon scheduler evaluates it once per minute.

Every automatic check requires both:

- an online external supply, or a battery reporting Charging, Full, or Not
  charging;
- a main-table default route whose output interface is Ethernet, has carrier,
  and has no Linux wireless marker.

The route is read directly from a bounded `NETLINK_ROUTE` dump. Wi-Fi is
conservatively treated as ineligible until the OS has a trusted metered-network
source. Conditions are checked again after Catalog refresh and before a queue
is published.

## Candidate and install boundary

`cp0-stored` uses a dedicated paginated appd command that returns only installed
App ID, version, and declared permissions. The Store UID still cannot use the
normal launcher list, settings, lifecycle, log, developer install, or broker
commands.

From the freshly downloaded and verified Catalog, the daemon selects at most
eight applications in canonical App ID order. A candidate must:

- already be installed;
- have a strictly greater SemVer in the Catalog;
- request a permission set that is a subset of its installed permission set;
- pass the current Store switch, automatic-update switch, allowlist, storage,
  Catalog identity, signature, digest, and size checks.

New applications, equal versions, downgrades, and any new permission are never
automatic candidates. The existing digest-named resume, serial queue, failure,
pause, cancel, and handoff recovery path is reused.

The final appd handoff is explicitly marked automatic. appd reloads its active
policy boundary and requires the independent automatic-update permission before
revalidating SemVer, package bytes, manifest identity, SDK compatibility, both
signatures, and the exact digest. A paused automatic job retains this mode, so
resume cannot convert it into a manual-policy installation.

## Verification

Rust tests cover legacy policy fail-closed behavior, private preference restart
persistence, six-hour throttling, power/network gates, strict permission subset
selection, version filtering, automatic appd handoff, and Store UID command
isolation. C tests cover exact response shape and inconsistent status rejection.
UI behavior and a 320x170 wait-power pixel snapshot are part of the normal
System Shell gate. AArch64 compilation covers the Linux Netlink implementation.

