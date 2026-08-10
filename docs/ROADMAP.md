# CardputerZero OS Roadmap

<!-- doc-locale: en -->
> **English** | [简体中文](ROADMAP.zh-CN.md)

This roadmap requires every phase to produce artifacts that can be validated on physical
hardware. Dates will be estimated after CM0 benchmarking; for now, it freezes dependencies
and completion criteria only.

## Phase 0: Architecture and Development Baseline (Foundation Complete)

- [x] Create an independent Git repository and Rust workspace.
- [x] Freeze hardware constraints, trust boundaries, and memory budgets.
- [x] Define app manifest v1, the permission vocabulary, and its validator.
- [x] Draft the WIT SDK ABI and example-app inventory.
- [x] Add baseline CI, SDK/manifest version policy, and architecture tests.
- [x] Add flat-ABI contract parsing, three-target generation, and compatibility tests against
  published signatures.
- [x] Use the same upstream `wit-parser` as `wasm-tools` for complete WIT syntax and name
  resolution.

Completion criterion: `make check` passes reproducibly, with automated compatibility tests
for the manifest and ABI.

## Phase 1: Board Support Package and Image

- [x] Record the CM0 V0.6 baseline for image, memory, DRM, input, audio, battery, and services.
- [x] Confirm ST7789V uses DRM/KMS MIPI-DBI and validate the 320x170 RGB565 connector.
- [x] Pin the BSP source commit, V0.6 overlay, kernel-module build entry point, and boot args.
- [x] Create a minimal Debian arm64 `pi-gen` stage and service-pruning policy.
- [x] Create a reversible boot-config installer and hardware smoke test.
- [x] Validate the memory cgroup, AppArmor, and every device interface on the existing system.
- [x] Build the first minimal image without Launcher or a desktop and validate its offline
  contents.
- [x] Flash the minimal image, validate 64 MB GPU/CMA, and measure boot time, idle memory, and
  SD writes.
- [x] Add an OverlayFS read-only lower root, fail-closed initramfs boot, and post-boot mount
  self-check. The three-partition product image enables this by default; explicitly removing
  the kernel argument enters writable recovery mode.

Completion criterion: a cold boot reaches the DRM test screen, every I/O device passes the
automated smoke test, Linux sees at least 400 MB RAM, and base-system memory consumption
before Home is below 180 MiB.

## Phase 2: Compositor and System Shell

- [x] Pin and trim Weston; validate DRM/Pixman kiosk and one foreground test client on hardware.
- [x] Isolate the internal LCD/keyboard on a dedicated seat and add a 320x170 headless backend.
- [x] Implement the native Phase 2A Wayland System Shell: Home, status bar, power-dialog
  skeleton, double-buffered renderer, periodic status refresh, and crash recovery to Home.
- [x] Implement the Phase 2B Weston policy core: separate compositor/Shell UIDs, peer-UID
  authentication, trusted layer, and global Home/Back/Tasks/Power action channel.
- [x] Move Launcher, status bar, and permission prompts to compositor-owned trusted overlays
  and implement immersive mode.
- [x] Centralize keyboard focus, global Home/Back/task-switch keys, and screen sleep in the
  compositor.
- [x] Enumerate installed apps from appd and support automatic foreground activation plus
  Tasks resume/stop.
- [x] Implement trusted notification banners, retained app focus, permission-prompt priority,
  and notification pixel regressions.
- [x] Add pixel-level screenshot regressions and complete reliable two-client switching plus
  a 200-cycle stress test.
- [x] Add SIGKILL recovery tests for core services and RAM-backed 24-hour stability/memory
  sampling.
- [x] Add an independent stability-evidence validator that cross-checks raw service/block-I/O
  samples, duration, PIDs, restarts, memory growth, summary fields, and the 64 MiB SD-write
  limit, rejecting missing or forged `PASS` results.
- [x] Complete physical-key acceptance for Launcher open, Tasks stop, and F1/F2/F3/F4 global
  actions.
- [x] Complete the information architecture, local telemetry, detail interaction, and 320x170
  pixel regressions for Home's five trusted system views. See
  `HOME-SYSTEM-APPS-ROADMAP.md`; hardware deployment was deferred until after
  2026-08-02 00:45 CST.
- [x] Implement phone-style Settings, complete app management, hardware diagnostics, and
  ownership of official Fn shortcuts locally. Compositor-owned long-Esc Home and the
  Shell-only backlight broker are implemented and tested; remaining bounded hardware-write
  services and hardware acceptance continue under X4 in `SYSTEM-EXPERIENCE-ROADMAP.md`.
- [x] Complete local Store Today/Apps/Search/Updates, physical-keyboard search, recent queries,
  bounded pagination, stale-browse gating, and strict SemVer update calculation. Hardware
  deployment waits for the active stability cycle.
- [ ] Complete 24-hour hardware acceptance for compositor/Shell/appd stability, memory leaks,
  and SD writes. Earlier runs were invalidated by reboots or app installation; the replacement
  no-foreground run was scheduled from about 2026-08-01 00:43 CST to 2026-08-02 00:43 CST.

Completion criterion: two Wayland clients switch reliably; an app cannot read unfocused
input or cover a system permission prompt; the compositor does not leak over 24 hours.

## Phase 3: Application Runtime and Isolation

- [x] Integrate WAMR interpreter/AOT and make App Runtime a controlled Wayland client.
- [x] Implement appd lifecycle, per-app UID, namespaces, seccomp, and cgroups.
- [x] Freeze the initial appd sandbox contract: stable app accounts, systemd cgroups,
  bubblewrap namespaces, read-only packages, and one writable private-data directory.
- [x] Pin and statically build WAMR 2.4.5; exercise each app UID, systemd cgroup, bubblewrap
  namespace, Runtime seccomp, and WASM execution on hardware.
- [x] Implement socket-activated appd lifecycle with a trusted install registry, canonical
  paths, root/Shell peer-UID authentication, one running slot, paginated list, and hardware
  start/stop acceptance.
- [x] Integrate appd, both sockets, static Runtime, stable test UID/registry, and SDK example
  into `pi-gen`; new development images start compositor/System Shell by default.
- [x] Implement manifest permission storage, session/persistent decisions, and trusted Shell
  prompt control.
- [x] Implement the first `notifications.post` typed broker, peer-UID binding, and bounded
  Shell notification queue.
- [x] Connect `notifications.post` to WAMR hostcalls with linear-memory bounds, Unix-only
  seccomp, and a hardware WASM end-to-end call.
- [x] Connect the notification queue to trusted System Shell without stealing keyboard focus
  from standard or immersive apps.
- [x] Implement the `network.client` HTTPS-only broker, SSRF/DNS-rebinding defense, WAMR
  hostcall, and bounded Rust/C/C++ SDK response API.
- [x] Implement the pathless Document Portal, trusted picker, `SCM_RIGHTS` read-only FD
  transfer, and bounded Rust/C/C++ SDK read API.
- [x] Implement the bounded ES8389 PCM broker, separate playback/capture authorization, and
  Rust/C/C++ SDK APIs.
- [x] Extend audiod to a fixed 48 kHz stereo hardware stream while retaining the 16 kHz mono
  API; add SDK 1.1 music streaming, global key sounds, and persistent Key Sounds policy.
- [ ] After stability monitoring, complete hardware acceptance for audio playback, capture,
  and permission denial.
- [x] Implement the camera broker for fixed 320x170 RGB565 frames, sealed read-only FD
  transfer, and Rust/C/C++ SDK APIs.
- [ ] Connect a compatible sensor and complete hardware acceptance for capture, denial, and
  orientation.
- [x] Implement the four V0.6 logical GPIO outputs, excluding critical onboard pins and raw
  gpiochip APIs.
- [ ] After stability monitoring, complete hardware acceptance for GPIO reads/writes,
  permission denial, and tightened sysfs permissions.
- [x] Implement an external SX1276 LoRa broker with fixed SPI/modulation settings, regional
  frequency bounds, send rate limits, `radio.lora`, and Rust/C/C++ SDK APIs. Images keep
  transmission disabled by default.
- [ ] Connect SX1276, confirm legal local frequencies, and accept send/receive, rate limits,
  and permission denial on hardware.
- [x] Implement Intent Broker with manifest exports, unique receiver routing, an eight-item
  bounded queue, foreground switch after response, and one-shot Rust/C/C++ `take` API.
- [x] Implement the app-private storage broker and manifest quota, remove host data-directory
  mounts from Runtime, and expose an atomic bounded key/value Rust/C/C++ SDK API.
- [x] Add hardware acceptance tooling for audio, GPIO, storage quotas, and cross-app isolation
  through real UID/cgroup/permission paths, with stability interlocks protecting the 24-hour
  baseline.
- [ ] After stability monitoring, accept storage persistence, quota denial, and cross-app read
  isolation on hardware.
- [x] Add malicious-app tests for WASI ambient authority, path escape, device access, arbitrary
  IPC, seccomp bypass, and cgroup exhaustion to `make check`.

Completion criterion: a test app can use only granted capabilities; denial has no bypass;
OOM or crash does not affect System Shell or another app's data.

## Phase 4: SDK and Developer Experience

- [x] Create the first `no_std` Rust SDK for clocks, event wait, notification capability, and
  stable errors; remove private FFI from Hello.
- [x] Create freestanding C11/C++17 SDK headers with wasm32 compile tests.
- [x] Publish the complete Rust and C/C++ SDK, generate WAMR/C/Rust bindings from one ABI
  contract, and provide an LVGL 9 adaptation for 320x170.
- [x] Implement SDK-only project generation, Cargo metadata parsing, and canonical artifacts
  for `cp0ctl new/build`.
- [x] Implement `cp0ctl run/package/sign/install/logs` with forced-command SSH install/log
  paths from PC to device and no dependency on scp, sudo, or a full Shell.
- [x] Build a PC WASM simulator, permission simulation, evdev key mapping, and JSON performance
  analysis.
- [x] Port Calculator and Camera without a traditional Linux-app compatibility layer.
- [x] Freeze SDK 1.1, adding HTTPS Range and 48 kHz stereo PCM without changing 1.0 or legacy
  0.1 imports; synchronize DevKit, simulator, permission review, and developer docs.

Completion criterion: a new developer can write, debug, sign, and install an app on hardware
using only the SDK on a PC.

## Phase 5: Application Packages and Store

- [x] Freeze reproducible `.capp` v1 and implement developer signatures plus an independent
  Store review signature.
- [x] Add a root-owned trust directory, revocation for both identities, and explicit
  Developer Mode verification policy.
- [x] Implement atomic `.capp` install, upgrade history, power-loss-recoverable retry, and
  rollback in either direction.
- [x] Implement the on-device 320x170 Store list/details, install progress, installed-version
  reconciliation, and offline state.
- [x] Implement dedicated `cp0-stored`, public-HTTPS restrictions, signed Catalog,
  anti-rollback, and resumable downloads.
- [x] Add WASM static scanning, permission/import review binding, and deterministic publication.
- [x] Expand deterministic Store protocol/downloader mutation tests and malicious Catalog and
  Range-response fixtures.
- [x] Add parent/organization policy and user switches for Developer Mode and Recovery Mode.
- [x] Add default-off, independently policy-controlled weekly Store aggregates without device
  identity, covering install/launch/crash counts, exact retry deduplication, and a 20-batch
  public threshold.
- [x] Implement a non-production content-governance slice with anonymous, no-free-text reports,
  bounded SLA queues, Team-isolated developer notification, one appeal, and append-only
  PostgreSQL audit. Automatic removal, formal SLA, policy approval, and external on-call
  remain production gates in Store Roadmap S8.
- [x] Build a developer-signed two-version test Store, exact review metadata, controlled public
  HTTPS acceptance origin, and a stability-interlocked hardware acceptance tool for refresh,
  HTTP Range resume, install, upgrade, offline cache, and expiration rejection.
- [ ] Configure the test Store endpoint and complete hardware acceptance for refresh, resume,
  install, upgrade, and offline Catalog.

Completion criterion: only trusted signatures install Store apps, power loss creates no
half-installed state, and an older version can be restored.

## Phase 6: Productionization and Further Security

- [x] Add fixed CPU quota/weight to app cgroups, enforce 30 FPS in Runtime, and create a
  RAM-only performance harness for boot, idle CPU/memory, short SD-write, and battery telemetry.
- [ ] After stability monitoring, run performance acceptance and measure whole-device power
  under defined workloads with a calibrated external USB meter.
- [x] Keep journald, temporary directories, and stability reports in RAM; add a 64 MiB/24 h
  SD-write acceptance limit.
- [x] Add kernel sysctl and compositor/Shell/appd systemd product-hardening baselines.
- [x] Implement a separate `cp0-data` partition, idempotent initramfs expansion, persistent-path
  allowlist, and immutable lower root by default; loopback expansion/reentry and final-image
  release gates pass.
- [ ] Flash a three-partition candidate and accept first expansion, reboot persistence,
  power-loss recovery, and 24-hour SD writes before closing immutable-root productionization.
- [x] Create a default-offline, redacted local support bundle, explicit-consent raw-log mode,
  and RAM-only diagnostics excluding app data, network/device identity, and keys.
- [x] Add independent offline validators for factory, performance, capability/persistence, and
  six-step Store hardware evidence. Recalculate critical metrics from raw TSV/JSON and reject
  forged `PASS`, cross-boot, or out-of-order evidence.
- [x] Add a V0.6 read-only production acceptance tool for fixed hardware, immutable root, data
  partition, core services, socket permissions, and appd control paths; run it with the flashed
  three-partition candidate.
- [x] Add a separate `recovery` image profile with writable maintenance root, tty1/LCD/SSH,
  no compositor or app entry points, a separate artifact name, and final-image gates for both
  profiles.
- [x] Implement bounded `CP0 backup v1`, two-pass integrity verification, restore to an empty
  target only, partition/maintenance-mode gates, and factory reset bound to the product trust
  root.
- [ ] Flash recovery media and complete backup/restore/factory reset, hardware boot, and
  production-station physical-I/O acceptance.
- [x] Complete the threat model, production blockers, and conditional architecture assessment
  for dm-verity, RAUC A/B, U-Boot/FIT, and hardware root of trust. Current development images
  do not claim verified boot.
- [x] Add libFuzzer/ASan entry points, bounded local smoke, and periodic CI for manifest,
  `.capp`, Store, appd control frames, and recovery backups.
- [x] Add a separate production-access profile that rejects build-time shared passwords and
  SSH keys and locks getty/Recovery Mode. A personal Owner may physically enable restricted
  Developer Mode; full Owner SSH Shell remains separate and off by default; root maintenance
  still requires a recovery SD.
- [ ] Boot a production-access candidate on rewritable media and verify no default listener,
  Developer Mode forced-command access, independent Owner SSH Shell switch, tty/root/sudo
  denial, and locked recovery entry.
- [x] Phase 6I-A: freeze first-boot state, bounded `cp0-provisiond`, 320x170 Setup pages, and
  pixel tests; see `FIRST-BOOT-PROVISIONING.md` and ADR 0007.
- [x] Phase 6I-B: remove pi-gen's temporary human account from final product rootfs; add the
  extrausers/PAM Owner identity and persistent home under `cp0-data`, plus a zero-fixed-
  credential image gate.
- [x] Phase 6I-C: implement a root provisioning daemon accepting only the exact Shell UID,
  with atomic state, yescrypt passwords, power-loss recovery, and `REPAIR_REQUIRED` checks.
- [x] Phase 6I-D: implement explicit Ethernet, Wi-Fi, and offline decisions; NetworkManager
  scan/connect; and owner-only SSH disabled by default and controlled by a persistent marker.
- [x] Phase 6I-E: implement graphical Setup in trusted System Shell; block Home, Tasks, and
  ordinary apps until completion, and do not re-enter Setup after temporary network loss.
- [x] Phase 6I-E2: add Wi-Fi scan/connect, Owner password change after current-password
  verification, and an independent, default-off Owner SSH Shell switch to trusted Settings;
  apps retain no NetworkManager/credential access and production retains no root/sudo.
- [ ] Phase 6I-F: complete protocol fuzzing, secret-leak checks, power-loss injection at every
  atomic write, full-page pixel regressions, PAM/NSS, and product/development/recovery image
  classification gates. `make check`, representative Setup pixels, Linux/arm64 sequential
  packages and peer credentials, complete Shell linking, and Debian 13 NSS/PAM password auth
  pass; the full write-fault matrix, final product-rootfs mount check, and every page/long-text
  pixel case remain for the candidate image.
- [ ] Phase 6I-G: on a fresh SD without HDMI/SSH, report V0.6 first boot, every keyboard path,
  three network choices, SSH denial/allow, staged power loss, ten cold boots, and factory reset.
- [x] Phase 6J-A: implement bounded root `cp0-devd`, separate Owner and trusted-Shell UIDs,
  per-request Developer Mode/policy checks, paired signing keys, and forced-command SSH keys.
- [x] Phase 6J-B: move PC-side `cp0ctl pair/install/logs/app` to streaming
  `ssh -T cp0-dev`; remove scp, sudo, generic upload, and remote-Shell dependencies.
- [x] Phase 6J-C: implement Developer Mode, a ten-minute `PAIR NEW COMPUTER` window, a list of
  up to eight paired computers, and individual/all revocation in the 320x170 Security UI;
  Owner SSH Shell remains independent and off by default.
- [x] Phase 6J-D: include devd, SSH gate/dispatcher/path unit, persistent pairing state, and
  trust directory in product images; explicitly mask them in recovery and add Rust/C, pixel,
  and image gates.
- [ ] Phase 6J-E: on a V0.6 production candidate, accept password-based first pairing, key
  reuse, window timeout, individual/all revoke, Developer Mode off, independent Owner Shell,
  normal-reboot persistence, and real OpenSSH `SSH_ORIGINAL_COMMAND` behavior.
- [x] Phase 6K-A: implement bounded root cp0-powerd restart/power-off, dual Shell-UID auth,
  fixed systemctl arguments, System Shell client, and product/recovery image gates without
  granting generic systemd control to Shell or apps.
- [ ] Phase 6K-B: on a new V0.6 product image, use confirmation UI to verify normal restart,
  a new boot ID, return to Home, and that full power-off requires physical power to start again.
- [x] Phase 6L-A: freeze one foreground/ten tasks, FIFO capacity eviction, MRU switching,
  five-state lifecycle, and checkpoint/resource boundaries; implement appd protocol v2,
  multi-session state, simulated F3 cards, SDK lifecycle ABI, and randomized model tests.
- [x] Phase 6L-B: merge the multitasking candidate into current main while retaining first
  boot, Developer Access, Store, and cp0-powerd; close UI 64 KiB overflow, non-resident task
  package mutation, and stale surface-token regressions; publish `MULTITASKING-MERGE-REPORT.md`.
- [ ] Phase 6L-C: wire atomic TaskJournal, appd restart reconciliation, Runtime authenticated
  control socket, and compositor `(task_id, runtime_generation)` binding. Preserve trusted
  Shell on failure; never infer generations from app-id or UID.
- [ ] Phase 6L-D: implement compositor-sealed RGB565 thumbnails at 2 Hz with read-only Shell
  delivery; test 0/1/3/10 tasks, stale generations, forged identity, and memory limits.
- [ ] Phase 6L-E: integrate WAMR checkpoint/restore bounded to 8 KiB, 250 ms, and fuel; private
  broker namespaces; eleventh-app FIFO eviction; and upgrade compatibility. Apps without
  callbacks must restart cleanly.
- [ ] Phase 6L-F: measure RSS, CPU, SPI, SD writes, and switch latency for 1/3/10 apps on CM0;
  set background/freeze/checkpoint thresholds and verify foreground capability-lease revocation.
- [ ] Phase 6L-G: after authorized deployment of one matching appd/Shell/compositor/Runtime
  bundle and normal reboot, verify F3, Intent, developer install/stop/uninstall, eleventh app,
  appd restart, persistence, and power-loss recovery before new-image release acceptance.
- [x] Phase 6M-A: implement Photo Library v2 paginated index, in-place v1 migration, per-frame
  atomic blobs, appd atomic import/delete, tail recovery, storaged startup cleanup, reserved
  SD space, and an eight-image Gallery page cache; remove 32-image eviction and duplicate
  Shell PNG copies.
- [x] Phase 6M-B: protect Camera/Gallery from uninstall in appd lifecycle and expose
  `removable` to Shell while retaining Store-signed upgrades.
- [x] Phase 6M-C1: implement USB Media Transfer independently of Developer Mode and Owner
  Shell: Owner-password confirmation, fixed 512 MiB FAT32 exchange image, one-LUN ConfigFS,
  Camera JPEG and Screenshot BMP+manifest export, WAV validation, and atomic Document Portal
  import. The protocol accepts no paths and never exposes rootfs, `cp0-data`, app-private
  directories, or any active partition.
- [ ] Phase 6M-C2: obtain a legal production USB VID/PID and complete V0.6 macOS/Linux/Windows
  enumeration, eject, unexpected removal, power loss, full disk, FAT recovery, and photo/WAV
  hash acceptance; see `OWNER-MEDIA-TRANSFER-V1.md`.
- [x] Phase 6N-A: implement global key sounds, persistent Key Sounds, SDK 1.1 HTTPS Range/
  48 kHz stereo streaming, and a production built-in Music app. The first bounded version
  supports local and public-HTTPS 48 kHz stereo 16-bit PCM WAV.
- [ ] Phase 6N-B: on a V0.6 production candidate, verify key-click latency/silence, uninterrupted
  music during clicks, local/public playback, pause and foreground/background media actions,
  pops/underruns, CPU/RSS/SPI, and input latency.
- [x] Implement OS release metadata outside the current boot chain, rootfs/hash-tree/FIT digest
  gates, offline dm-verity verification, a three-boot rollback state machine, dual-copy
  torn-write detection, and a 100-cycle power-loss model. RAUC CMS, signed FIT, and hardware
  root of trust remain separately gated.
- [ ] Implement and verify A/B, verity, signed boot metadata, fault injection, and automatic
  rollback on spare hardware or rewritable SD; never write irreversible OTP state on the only
  V0.6 device.
- [ ] Commission an independent security review and close every finding or record explicit
  acceptance by the product owner.
