# Home System Apps Roadmap

<!-- doc-locale: en -->
> **English** | [简体中文](HOME-SYSTEM-APPS-ROADMAP.zh-CN.md)

## Product boundary

The five Home entries are trusted System Shell views. They are part of the OS
and are not SDK applications. Only one view is visible at a time, all input is
keyboard driven, and every view fits the fixed 320x170 framebuffer below the
21-pixel trusted status bar.

The first release exposes only data and operations with an existing trusted
control path. Display brightness uses the Shell-only `cp0-displayd` path and
its V0.6 sysfs control has passed physical readback. Wi-Fi provisioning,
saved-network management and idle timeout still need new privileged providers
and are intentionally not represented as working controls. The flashed image
also lacks the BCM43439 firmware; the image package fix must be flashed before
the future Wi-Fi broker can be accepted.

## View specification

### Apps

- Show every app returned by appd, including name, lifecycle state and list
  truncation.
- Up/Down selects, Enter launches or resumes, Right opens details, and Back
  returns Home.
- Details show version, application ID, standard/immersive display mode and
  lifecycle state. Enter launches a stopped app or stops a running app.
- Preserve selection across appd catalog refreshes and expose explicit empty,
  starting and failed states.

### Store

- Show the reviewed catalog with available, update, queued, downloading,
  installing, installed and failed states.
- Right refreshes the catalog. Enter opens details and then requests a bounded
  install/update through cp0-stored.
- Details show version, summary and declared SDK permissions. The list exposes
  stale, unavailable, unconfigured, loading, empty and truncated states.

### Device

- Overview shows Cardputer Zero V0.6 / CM0 identity, OS build, uptime and CPU
  temperature.
- Resources show available/total memory, free/total persistent storage and
  battery capacity.
- Left/Right switches the two fixed pages. Missing telemetry is shown as
  `UNKNOWN`; it never becomes a fabricated value.

### Network

- Status shows online/offline/link-only state, selected non-loopback interface
  and IPv4 address.
- Details show interface, link state and address in a stable two-page layout.
- The Network view remains read-only. Trusted Wi-Fi radio, Airplane Mode, scan
  and connection controls live under Settings and use the dedicated
  connectivity/provisioning brokers; applications receive neither
  NetworkManager access nor credentials.

### Settings

- Developer Mode and Recovery Boot remain bounded appd device-policy controls.
  Enabling either mode requires confirmation; disabling is immediate.
- Connectivity exposes the Wi-Fi radio, Airplane Mode and a bounded network
  picker with masked WPA credential input.
- Security exposes current-password-authenticated owner password replacement
  and an independent Owner SSH Shell toggle that never grants root or sudo.
- A third Policy row opens a read-only detail page showing management authority,
  Store installation, application launch and capability restrictions.
- Locked or unavailable settings are visibly non-actionable. Back closes a
  detail/confirmation before returning Home.

## Delivery roadmap

### H1: specification and telemetry

- [x] Freeze the five-view information architecture and trusted-operation
  boundary.
- [x] Add bounded host telemetry collection for device, storage and network
  status with parser/fixture tests.

### H2: interaction and rendering

- [x] Add Apps details and lifecycle actions without changing appd authority.
- [x] Complete Device and Network two-page views.
- [x] Add Settings policy details while retaining confirmation and lock rules.
- [x] Keep Store behavior compatible and make refresh/install affordances
  visible in its existing list/detail states.

### H3: local acceptance

- [x] Cover all view transitions, boundary navigation, empty/error/locked states
  and telemetry formatting in native unit tests.
- [x] Add deterministic 320x170 snapshots for Home plus every primary/detail
  view and validate framebuffer guard bytes.
- [x] Run `make check` and retain a local screenshot artifact set.

### H4: deferred device acceptance

- [x] After the no-deploy window, package the candidate without changing the
  current device first.
- [x] Deploy only with explicit approval, then validate LCD pixels, physical
  keyboard navigation, live telemetry and service restart behavior. Physical
  keys were operator-confirmed and the final trusted-capture run retained 116
  exact 320x170 LCD frames; battery/I2C telemetry and service restart also pass.
- [ ] Confirm memory/CPU/SD-write budgets and close any hardware-only defects.

No H4 action is part of the local development window ending around
2026-08-02 00:45 CST.
