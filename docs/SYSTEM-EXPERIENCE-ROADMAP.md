# System Experience Roadmap

## Principles

The Home entries are trusted system surfaces, but the Shell remains
unprivileged. Hardware changes go through a bounded device service; application
management goes through appd. A row is shown as unavailable when its backing
capability does not exist. Local simulation may exercise every state, but must
not imply that unverified hardware writes already work on V0.6.

All screens target 320x170, keep one foreground view, use four visible rows at
most, preserve selection while scrolling, and handle Up/Down, Enter and Back.
Left/Right changes bounded values only when a row is editable.

## Settings information architecture

### Connectivity

- Wi-Fi: radio state, link state, current SSID/IP, scan, saved networks,
  connect/disconnect and forget. Credentials must be submitted only to a
  root-owned network broker and never returned to the Shell.
- Airplane mode: one transaction disables Wi-Fi, Bluetooth when present and the
  optional LoRa radio. Wired Ethernet remains available and is labelled as such.
- Network details: interface, IPv4, gateway, DNS, signal and MAC privacy state.
- Bluetooth, hotspot and VPN appear only after their capability providers exist.
- Wired Ethernet, proxy and private DNS are part of the network detail model;
  cellular-only concepts such as SIM and roaming are intentionally omitted.

### Display and appearance

- Brightness with Fn+U/Fn+I integration and a transient level overlay.
- Dark, light and high-contrast themes.
- Screen timeout: 30 seconds, 1 minute, 5 minutes or never.
- Text density and rotation are future options; neither is exposed until every
  trusted prompt and SDK surface can follow the selected geometry.
- Night light, status-bar visibility and accessibility text scale require a
  compositor-wide appearance contract rather than per-application overrides.

### Sound

- Media volume and mute with Fn+A/Fn+S/Fn+D integration.
- System/key sounds and selected output route.
- Microphone state is informational; application recording remains permission
  mediated and cannot be globally enabled around a denied permission.
- Do Not Disturb and notification sounds require the notification and media
  brokers to share one bounded focus policy.

### Camera

- Preferred capture resolution, rotation and mirror policy.
- The current broker has one exact 320x170 RGB565 contract. Larger resolutions
  remain visibly unavailable until the broker ABI and resource budget change.
- Camera permission access is managed under Privacy, not silently granted here.

### Power and battery

- Charge state, capacity, voltage, signed current and estimated battery power.
- Battery saver and charge limit are capability-gated future controls.
- Power-off and restart remain trusted global actions with confirmation.

### Apps, storage and privacy

- Per-app version, install time, package bytes, private-data bytes, display
  mode, lifecycle state and declared permissions.
- Launch/resume, stop, permission reset and uninstall with confirmation.
  Uninstall removes executable package versions and permission decisions while
  retaining private data for a later explicit `Clear data` operation.
- Global permission overview, document access history and storage summary are
  planned as separate Settings pages once their bounded query APIs exist.
- Notification policy, default intent handlers and camera/microphone/network
  access history need appd-owned audit APIs. The Shell must never inspect
  application-private data to construct these views.

### System

- About: board, OS build, kernel/boot identity, uptime and update channel.
- Hardware diagnostics: display, keyboard, audio, camera, battery and I2C bus.
- Date/time, language, accessibility, OS update and support bundle belong here.
  Automatic time and update controls require dedicated services.
- Keyboard layout, backup, reset options, licences and local diagnostics belong
  here as capability-gated rows. Cloud accounts and cellular settings are not
  product concepts for V0.6 and are deliberately excluded.

### Security, policy and developer options

- Management authority and Store/app/capability restrictions.
- Developer Mode and Recovery Boot retain policy locks and confirmations.
- Screen lock, encryption and credentials are not claimed without a product
  authentication design and verified boot/storage prerequisites.
- Installation sources, trust certificates, security audit history and factory
  reset require root-owned policy endpoints and destructive confirmations.

## Privileged service boundary

The local UI models are not hardware authority. Before any V0.6 setting becomes
writable, a root-owned service must expose a narrow request protocol, validate
the caller as the trusted Shell, apply policy, return the observed state and
write a bounded audit record. The planned providers are:

| Provider | Owns | Must not expose |
| --- | --- | --- |
| network broker | Wi-Fi scan/connect/forget, airplane transaction, DNS/proxy | credentials, raw NetworkManager or netlink control |
| display broker | backlight level, timeout and compositor appearance state | raw sysfs/DRM handles |
| audio broker | output volume, mute, route and focus policy | unrestricted ALSA devices |
| camera broker | capture profile and orientation policy | raw V4L2 devices |
| power broker | sleep, restart, shutdown and supported charge policy | arbitrary systemd or sysfs operations |
| system broker | time, update, backup/reset and support bundle jobs | shell commands or filesystem paths |

Every mutating request is capability queried first. Unsupported controls remain
visible as `UNAVAILABLE`; local simulation is never enabled by the production
Shell entry point.

## Official keyboard mapping

The system follows `https://cardputer.cc/#/documents/cp0-keys`:

| Combination | Linux key/action | Ownership |
| --- | --- | --- |
| Fn+1..0, Fn+O, Fn+P | F1..F12 | F1 Home, F2 Back, F3 Tasks, F4 Power; F5..F12 delivered to the foreground app |
| Fn+Q/W/E | Play/Pause, Previous, Next | compositor global media action |
| Fn+U/I | Brightness down/up | compositor global display action |
| Fn+A/S/D | Mute, Volume down/up | compositor global sound action |
| Fn+F/Z/X/C | Up/Left/Down/Right | standard navigation; letters also navigate trusted non-text views |
| Fn+H | Help | trusted help overlay or foreground app help event |
| Fn+J | PrintScreen | trusted screenshot request with bounded destination |
| Fn+K/L/M | Home/PageUp/PageDown | foreground standard keys; Home remains distinct from OS Home/F1 |
| Fn+B/N | Insert/End | foreground standard keys |
| Fn+Backspace | Delete | foreground standard key |

ESC short press remains Back where the Shell is focused. The documented long
press Home behavior belongs in the keyboard driver/compositor and must not be
implemented as an unreliable userspace timer in each application.

## Delivery milestones

### X1: local architecture and models

- [x] Freeze the complete settings taxonomy and keyboard ownership boundary.
- [x] Extend appd metadata and uninstall protocol with migration tests.
- [x] Extend bounded device telemetry for power and hardware diagnostics.

### X2: local trusted UI

- [x] Implement grouped Settings navigation and detail/value/disabled states.
- [x] Implement multi-page application details and uninstall confirmation.
- [x] Implement expanded Device overview, power and bus diagnostics.
- [x] Implement brightness, volume, media, help and screenshot global actions.

### X3: local acceptance

- [x] Add deterministic snapshots for every category and destructive dialog.
- [x] Test bounded protocol payloads, list scrolling, unavailable capabilities
  and the ownership of every official key mapping.
- [x] Pass `make check` and the Linux aarch64 compositor build.

### X4: deferred V0.6 acceptance

- [ ] After the user-approved no-deploy window, inspect exact backlight, ALSA,
  rfkill/network and camera controls before enabling hardware writes.
- [ ] Deploy only with explicit approval and validate physical Fn combinations,
  LCD overlays, battery telemetry, I2C diagnostics and uninstall persistence.
- [ ] Measure idle memory, input latency and SD writes before enabling settings
  persistence by default.
- [ ] Implement and validate the keyboard driver/compositor ESC long-press Home
  gesture; `Fn+K` must remain the foreground application's standard Home key.
- [x] Add the trusted screenshot broker with exact-client compositor
  authorization, fixed 320x170 capture, atomic bounded PNG storage and
  completion states. Physical `Fn+J` and SD latency remain device acceptance.
- [ ] Add the media-session broker before replacing media `REQUESTED` overlays
  with successful completion states.

Local broker work may proceed independently. Deployment remains gated on the
retained stability evidence passing verification and an explicit device step.
