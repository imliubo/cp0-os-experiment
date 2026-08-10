# Remaining Roadmap Audit

<!-- doc-locale: en -->
> **English** | [简体中文](REMAINING-ROADMAP-AUDIT.zh-CN.md)

This document records the evidence audit as of 2026-08-01. It does not equate
"implemented" with "accepted on hardware or by an external owner." The main
Roadmap remains authoritative. Items are grouped by whether they can proceed
without physical-device access.

## 1. Local Work That Can Continue

### System Experience

- Local implementation and pixel regressions are complete for appd app details,
  install time, package/data size, uninstall protocol, Settings groups, app
  management, hardware diagnostics, and official Fn shortcuts.
- Shell-only `cp0-displayd`, fixed backlight sysfs, the 5%-100% range,
  Fn+U/Fn+I, and the unified Settings path are complete. Hardware exposed and
  fixed a false failure caused by splitting a sysfs attribute into two writes;
  broker write/readback at 65/75 passed. Physical Fn overlays and input latency
  still require manual acceptance.
- Hardware on 2026-08-02 confirmed that an ordinary login user cannot read the
  protected brightness attribute. `cp0-displayd`, Shell, appd, and audiod were
  active with zero restarts.
- ES8389 hardware passed DACL/DACR/Speaker volume and mute readback, role denial,
  mono SDK playback converted to stereo hardware, and stereo-to-mono capture
  after explicit capture start. Physical Fn keys and LCD overlay still require
  observation.
- The V0.6 factory gate passed and an independent local validator confirmed it.
  LCD, keyboard, audio, battery, six I2C-1 clients, immutable root, data
  partition, and every broker passed. The only warning was no connected camera.
- Hardware investigation fixed five deployment defects: factory ext4 capacity
  calculation, stale audio-socket permissions, compositor install-group
  prerequisite, invalid app-platform systemctl wait gate, and sockets not being
  recreated.
- Kernel detected SDIO BCM43439, but the flashed image omitted
  `firmware-brcm80211`. The image package inventory and finished-image gate are
  fixed. Wi-Fi and candidate cold boot require acceptance after the next flash.
- The in-repository builder now reproduces the ARM64 compositor. `make check`
  and the AArch64 build remain required at every local milestone.
- A 60-second final RAM-overlay candidate sample recorded 1.306% total core CPU,
  zero SD writes, and 212.7 MiB minimum available memory. Derived used memory of
  202.1 MiB still exceeds the 180 MiB product limit, and warm restart affected
  Shell activation time; this cannot replace cold-boot evidence after flashing.
- LCD overlays, input latency, and persistence remain in the X4 hardware gate.

### Store Productionization

- Local Developer Portal, Review Console, and Store Operations MVPs are
  complete. Store Operations supports bounded Today editing, 320x170 preview,
  published-Release selection, SLA moderation queue, and structured resolution
  through existing ETag/idempotency controls. Real Release discovery enforces
  approved/published/artifact/latest-App projection, bounded keyset pagination,
  and a strict frontend adapter. S8J removed runtime fixtures from both
  consoles; Review Queue/Detail, Today, Release, and moderation use real
  workforce BFF/Control APIs and reread authoritative state after mutation.
  Deployment remains open. S8H froze audience-separated workforce identity
  OpenAPI and accepted external identity links, summary sessions, OIDC
  transactions, at-most-five-minute session-bound tokens, synchronous
  revocation cascade, and HTTP revocation denial through PostgreSQL. S8I added
  independent dual-Origin workforce BFFs, strict production entry, idempotent
  token/logout records, and in-memory Review/Operations token adapters, passing
  fresh PostgreSQL 17 acceptance across BFF and Control API. Production
  IdP/JWKS, key custody, domain deployment, and on-site revocation remain open.
- S8J makes Scan Worker atomically create immutable review-display metadata,
  database-bound to Submission, ready-for-review scan, default locale, and
  creation time. Review Queue/Detail revalidates scan-report SHA-256 and risk
  digest. Old scans without new projections fail closed and require rescanning.
- Submission/Review/Release transactional core is complete. PostgreSQL/HTTP
  vertical slices cover App Registry, Submission create/upload/finalize/read,
  independent dual review, Release control, atomic withdrawal cancellation,
  and developer OAuth Device Flow. Team slices cover read, Owner role change,
  member suspend/resume/terminal removal, and MFA freshness. Identity slices
  cover account linking, invitations, Portal session security/OpenAPI and
  PostgreSQL state; Portal BFF OIDC login, summary session, CSRF, MFA step-up,
  logout, invitation HTTP/encrypted-mail worker, dual-provider linking, and
  Membership session propagation pass PostgreSQL, with the Portal account-
  security page using a real BFF adapter. Production mail/IdP, Review production
  SSO, object storage, dynamic malicious samples, and HSM/key ceremony remain
  open. Isolated Scanner, file-key Publisher/Catalog Builder, and full-prefix
  transparency-log outbox/lease/signing/recovery/atomic-result slices are done.
- Local content-addressed object GC has default dry-run, 24-hour grace, upload
  interlock, strict path checks, and PostgreSQL 17 acceptance. Production
  replication, retention, and multi-region recovery remain external gates.
- Device Today/Apps/Search/Updates, 1024-entry bounded Apps/Search pagination,
  Catalog v3 rich details, download/update queues, interruption recovery, and
  default-off automatic updates are implemented. Catalog v4 projects only
  approved Releases into Today editorial. Default-off weekly aggregate metrics
  without device identity have device and PostgreSQL vertical slices.
- Non-production content-governance S8C accepts only anonymous reports for an
  exact published version and fixed reason, storing no free text, contact,
  device/account/network identity. Bounded SLA queues, Team-isolated notice,
  one appeal, exact replay, audit/outbox, and append-only revision are accepted.
  Automatic removal, dual approval, external security on-call, production SLA/
  retention, and final policy remain external gates.
- Backend can publish from approved Releases and `cp0ctl` OAuth is connected;
  Portal account, session, and Team management are not yet a complete
  self-service loop.
- Existing Store security foundations remain reusable; see `STORE-ROADMAP.md`.
- The final S6E device UI gap is closed: Store Apps no longer uses legacy
  `list`, which covers only 64 entries. It uses strict eight-entry
  `browse(all)` pages. Apps and Search share one cache; `cp0_ui` is 60,624 bytes.

### Product Safety and Release Engineering

- A/B, verity, and signed boot have offline policy/models; real boot-chain
  integration still requires spare media.
- Recovery, production-access, and three-partition build/static gates can
  continue to be strengthened.
- Catalog key rotation has device overlap/switchover/old-key revocation tests
  and an operations runbook. Production HSM ceremony evidence v1 freezes
  dual-person separation, absence of private-key fields, HSM/trust-update
  digests, and a strict validator. HSM selection, real quorum, signing trust-root
  update, offline-device cohorts, and independent audit need real infrastructure.
  Production runbooks and remaining disaster-recovery documents can continue
  locally.

## 2. Do Not Touch the Only Device Until the 24-Hour Run Ends

- Retrieve and independently validate continuous service, memory-growth, and
  SD-write evidence for compositor, Shell, appd, and stored.
- Deploy latest Home, hard stability interlock, Phase 6F limits, and Store work.
- Reboot normally and verify Home, boot ordering, and service continuity.
- Run factory, performance, capability-full, and persistence-only evidence.
- [x] Complete hardware acceptance for audio playback/capture/permission denial.
- Complete GPIO read/write/denial/sysfs-tightening hardware acceptance.
- Complete private-storage persistence, quota, and cross-app isolation acceptance.
- Complete six Store steps: refresh, resume, install, upgrade, offline, and
  expiration rejection. Each also binds App Metrics default-off/no pending data;
  install and upgrade verify appd's blocking lifecycle observer exits after an
  explicit stop.

The formal run is
`/run/cardputerzero-stability/acceptance/20260731T170228Z-10620`, started at
2026-08-01 01:02:28 CST. Do not deploy, reboot, or launch an app before complete
evidence retrieval.

## 3. Requires User-Supplied or Confirmed Peripherals

- Camera broker: connect a V0.6-compatible sensor and verify orientation,
  capture, and permission denial.
- LoRa broker: requires SX1276 plus product-owner confirmation of deployment
  region and legal frequencies.
- Power: requires a calibrated external USB meter and defined brightness,
  network, idle, and app-load conditions.

Simulator and onboard battery-gauge results cannot replace this evidence.

## 4. Requires Reflashing or Spare Media

- Three-partition product candidate: first expansion, reboot persistence,
  power-loss recovery, and 24-hour SD writes.
- Recovery media: boot, backup, restore, factory reset, and workstation I/O.
- Production-access candidate: verify SSH, tty, sudo, Developer Mode, and
  in-product recovery entry are all disabled.
- A/B, dm-verity, signed FIT, fault injection, and automatic rollback must use
  spare hardware or a rewritable test SD.
- Never write irreversible OTP state on the only V0.6 device.

Before a flash, generate an unambiguous image path, profile, SHA-256, and
acceptance procedure, then pause for user assistance.

## 5. External Organization Gates

- No third-party security review has been commissioned.
- Content/privacy policy, review appeal, and developer terms need product/legal
  approval.
- Store HSM/key ceremony, production domain, CDN, backup regions, and on-call
  operations require real infrastructure decisions.

## 6. Phase Summary

| Phase | Implementation state | Missing completion evidence |
| --- | --- | --- |
| Phase 2 | Core window/trusted UI/input and extended system experience implemented locally | 24-hour evidence and final hardware deployment |
| Phase 3 | Runtime, sandbox, and capability brokers implemented | Matching audio/GPIO/storage/camera/LoRa hardware gates |
| Phase 4 | SDK 1.0, CLI, simulator, and DevKit implemented | Later Store submission CLI belongs to Store product phase |
| Phase 5 | Dual signing, atomic install, on-device Store, 1024-entry Apps/Search, OAuth, Portal OIDC/session/invite/linking, real Portal BFF adapter, Team roles/member suspension/removal, isolated scan, independent dual review, Review Console, Catalog v4/Today, auto-update, anonymous weekly aggregates, non-production governance, and workforce identity/BFF/frontend vertical slices implemented | Production mail/IdP/JWKS/key custody, on-site workforce revocation, production HSM, formal governance/enforcement, operations drills, and six-step hardware evidence |
| Phase 6 | Profiles, validators, and security tools implemented | Performance/power, flashed media, A/B hardware, and third-party review |
