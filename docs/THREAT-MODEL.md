# CardputerZero OS threat model

## Scope and claim

This model covers the V0.6 CM0 device, the CardputerZero OS image, trusted
native services, the WASM application platform, Store content and offline
recovery data. It assumes the pinned build inputs and released binaries are
reviewed before distribution.

Application isolation means an untrusted SDK application receives no ambient
host authority and cannot access another application's data or a capability
that was not granted. It is not an absolute guarantee against a kernel, WAMR,
native broker, compiler or hardware compromise. Physical SD-card replacement
is also outside the current verified security boundary because V0.6 has no
demonstrated immutable boot trust anchor.

## Assets and security objectives

- application private state, documents, network credentials and device
  identity remain confidential across application UIDs;
- permission decisions, device policy, Store roots and the installed-app
  registry retain integrity;
- the trusted compositor owns global keys, focus and permission surfaces;
- packages, catalogs, recovery bundles and IPC frames are parsed with fixed
  bounds and fail closed;
- failure or resource exhaustion in one application does not take down the
  compositor, System Shell, appd or another application's state;
- an OS or application downgrade is not accepted through a normal update path;
- recovery is explicit, offline and cannot silently operate on the active root.

Availability after destructive physical damage, confidentiality from an
invasive hardware attacker, RF-law compliance and protection from a malicious
compiler or upstream signing authority are outside this version's scope.

## Actors and trust boundaries

| Actor or boundary | Trusted authority | Attacker-controlled input |
| --- | --- | --- |
| SDK application | none beyond granted broker calls | WASM instructions, host-call arguments, UI and intent payloads |
| App Runtime | validated WASM execution and fixed host ABI | module memory, traps and CPU/memory pressure |
| appd and brokers | package identity, UID mapping, policy and hardware mediation | Unix frames from authenticated peers, network/media/device failures |
| compositor and System Shell | focus, global keys, trusted overlays and user decisions | ordinary Wayland surfaces and bounded app metadata |
| Store | configured Ed25519 root and monotonically increasing catalog sequence | HTTPS responses, catalogs, packages and interrupted downloads |
| recovery operator | explicit root ceremony and selected removable media | backup file contents and block-device selection |
| boot chain | Raspberry Pi firmware, kernel, initramfs and lower root | mutable SD boot/root contents until verified boot exists |
| build/release | pinned source revisions and signing keys | dependency supply chain and build host |

The root account, kernel, initramfs, compositor policy module, System Shell,
appd, App Runtime, capability brokers and production signing keys are in the
trusted computing base. Network peers, Store payloads, recovery bundles,
applications and application-owned data are untrusted.

## Primary threat analysis

| ID | Threat | Existing control | Residual risk / required action |
| --- | --- | --- | --- |
| APP-01 | Application reads host or peer data | WAMR, unique UID, bubblewrap namespaces, no host paths, seccomp, brokered storage | Native runtime/kernel escape remains; retain malicious-app and fuzz campaigns |
| APP-02 | Application impersonates another app | SO_PEERCRED, root-owned registry and stable non-recycled UIDs | A compromised appd or kernel defeats identity |
| UI-01 | Application covers or spoofs a permission prompt | compositor-owned top layer and peer-UID-authenticated private protocol | Visual similarity inside the app surface remains possible; prompts must identify the app |
| IPC-01 | Malformed or oversized frame exhausts a daemon | newline frame limits, strict serde schemas, bounded queues and timeouts | Parser bugs remain; all public decoders are fuzz targets |
| PKG-01 | Package/catalog tampering or rollback | dual Ed25519 signatures, key IDs, catalog sequence and atomic install | Signing-key compromise requires external revocation and release response |
| NET-01 | Broker used for SSRF or local-network discovery | HTTPS only, public-address validation, DNS rebinding checks and response bounds | Public endpoints can still return hostile bounded content |
| DATA-01 | Cross-app persistent access | storage broker derives caller from UID and enforces per-app quota | Offline physical access sees unencrypted `cp0-data` |
| REC-01 | Backup path escape or special-file creation | fixed layout allowlist, no links/devices, owner/mode checks, two-pass hashes and empty target | `CP0 backup v1` is not encrypted or authenticated; operator media is trusted |
| DOS-01 | App consumes all RAM/CPU/processes | one foreground slot, cgroup memory/process limits, WAMR bounds and service restart policy | Sustained kernel-level I/O contention needs hardware soak testing |
| BOOT-01 | SD attacker replaces kernel/initramfs/root | OverlayFS limits runtime writes only | OverlayFS is not an integrity control; verified boot and dm-verity remain release blockers |
| UPDATE-01 | Interrupted or malicious OS update bricks device | no remote OS updater is enabled | Adopt the signed A/B design in ADR 0006 only after boot-chain hardware validation |
| MGMT-01 | Shared development credential exposes SSH/sudo | password is explicit at build time; key-only SSH is supported | Development images with SSH are not production artifacts; production provisioning is unresolved |
| SUPPLY-01 | Dependency/build compromise | pinned BSP/pi-gen revisions, locked Rust dependencies and image gates | Reproducible build comparison, SBOM signing and independent review remain open |

## Release gates

The following conditions block a production security claim even when a
development image passes functional tests:

1. Any artifact named or configured as development may contain an operator-set
   password and enabled SSH. It must not be redistributed as a production
   image. A production provisioning design must use per-device credentials or
   a physical onboarding ceremony and must define how maintenance access is
   revoked.
2. The mutable FAT boot partition and unsigned root hash mean the current image
   cannot resist physical SD modification. The verified-update decision and
   limitations are in ADR 0006.
3. `cp0-data` and `CP0 backup v1` are not encrypted at rest. Device-loss and
   removable-media confidentiality require a separately designed key hierarchy.
4. The 24-hour stability run, destructive recovery flow, peripheral tests and
   OS rollback tests require the hardware conditions listed in the Roadmap.
5. The internal threat model and fuzz harnesses do not substitute for an
   independent assessment. The third-party security review remains open and
   its findings must be tracked to closure before a production security claim.

## Verification mapping

- `make check` covers schema, protocol, package, sandbox, permission, malicious
  application, recovery and static image-policy tests.
- `make fuzz-check` compiles all libFuzzer targets with nightly Rust.
- `make fuzz-smoke` runs bounded AddressSanitizer campaigns for manifest,
  package, Store, appd control and recovery inputs.
- The scheduled fuzz workflow preserves any crash corpus as a CI artifact.
- Image release requires both the compressed-image checksum gate and the
  mounted rootfs/initramfs profile gate.
- Hardware acceptance results are evidence only when their full run directory,
  duration, failure count and restart/memory/SD-write summaries are retained.

This document must be revised whenever a trust boundary, public input format,
production access path, boot chain or signing root changes.
