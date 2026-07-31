# Home System Apps Roadmap

## Product boundary

The five Home entries are trusted System Shell views. They are part of the OS
and are not SDK applications. Only one view is visible at a time, all input is
keyboard driven, and every view fits the fixed 320x170 framebuffer below the
21-pixel trusted status bar.

The first release exposes only data and operations with an existing trusted
control path. Wi-Fi provisioning, saved-network management, display brightness
and idle timeout need new privileged brokers and are intentionally not
represented as working controls in this release.

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
- Data is read-only. Connection setup and credential storage remain deferred
  until a dedicated Network Manager broker and permission model exist.

### Settings

- Developer Mode and Recovery Boot remain bounded appd device-policy controls.
  Enabling either mode requires confirmation; disabling is immediate.
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

- [ ] After the no-deploy window, package the candidate without changing the
  current device first.
- [ ] Deploy only with explicit approval, then validate LCD pixels, physical
  keyboard navigation, live telemetry and service restart behavior.
- [ ] Confirm memory/CPU/SD-write budgets and close any hardware-only defects.

No H4 action is part of the local development window ending around
2026-08-02 00:45 CST.
