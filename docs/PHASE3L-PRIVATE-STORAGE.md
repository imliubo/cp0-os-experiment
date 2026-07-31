# Phase 3L: Quota-enforced private storage

Private application data is exposed only through the CardputerZero SDK. The
Runtime no longer receives a writable bind mount of the host application data
directory. Its `/data` directory is an empty namespace-local directory, and the
seccomp policy continues to reject `open`, `openat` and filesystem mutation
syscalls.

## API and quota

The SDK provides `put`, `get` and `delete` operations over validated keys:

- keys contain 1 through 64 ASCII alphanumeric, `.`, `_` or `-` bytes and may
  not begin with `.`;
- values contain 1 through 8192 bytes;
- each application may store at most 256 keys;
- a missing key is distinct from a service error;
- the hard byte quota is the installed manifest's `resources.storage_mb`.

Storage is a baseline application facility, not a user-granted permission.
appd still authenticates every call against the installed application UID,
active systemd cgroup and root-owned manifest before adding the application ID
and quota to the privileged service request.

## Isolation path

```text
WASM storage SDK call
  -> Runtime validates linear-memory ranges, key and value bounds
  -> appd authenticates UID, PID, active cgroup and installed manifest
  -> root-only cp0-storaged socket
  -> cp0-storaged derives one fixed application directory
  -> quota check, owner/mode/type checks and atomic filesystem operation
```

`cp0-storaged` is the only account that can access
`/var/lib/cardputerzero/data`. The directory and every application subdirectory
use mode `0700`; values use mode `0600`. The service has no device or network
access and systemd grants it one writable path. Application Unix accounts do
not own or mount these directories.

## Atomicity and accounting

A put inspects every direct entry, rejects symbolic links and malformed or
oversized files, subtracts the replaced value, and checks the projected total
against the manifest quota. It writes a uniquely named same-directory file,
calls `fsync`, atomically renames it over the destination and synchronizes the
directory. A temporary file left by power loss counts against the quota, so an
interrupted operation cannot bypass accounting.

Logical value bytes are the documented quota unit. Filesystem metadata and SD
allocation overhead are deliberately not exposed to applications.

## Verification

Tests cover canonical protocol framing, maximum values, invalid keys, atomic
replacement, deletion, exact quota exhaustion, Runtime JSON decoding, Rust and
C/C++ SDK validation, removal of the writable host bind, systemd ownership and
socket restrictions, and AArch64 cross-compilation. The real-identity quota,
process-restart, reboot and cross-app probe is documented in
`PHASE3M-DEVICE-CAPABILITY-ACCEPTANCE.md`. Physical execution and power-loss
acceptance remain pending until the current 24-hour core stability run
completes.
