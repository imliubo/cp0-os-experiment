# System Experience Roadmap

<!-- doc-locale: en -->
> **English** | [简体中文](SYSTEM-EXPERIENCE-ROADMAP.zh-CN.md)

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
- USB Media Transfer requires the current Owner password, exposes only the
  isolated `CP0-MEDIA` exchange image, imports verified music atomically, and
  exports photo copies read-only. It does not enable Developer Mode or the
  Owner SSH Shell.
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
- Power-off and restart use the trusted confirmation UI and the implemented
  Shell-only cp0-powerd fixed-action broker.

### Apps, storage and privacy

- Per-app version, install time, package bytes, private-data bytes, display
  mode, lifecycle state and declared permissions.
- F3 Tasks keeps one foreground view while presenting at most ten horizontally
  stacked 160x85 cards. Left/Right selects a task, Up/Down selects its
  OPEN/STOP action, Enter executes it, Space stops the selected task directly,
  and Esc/Home returns to the trusted Home surface.
- Task cards are ordered by most recent activation, while capacity eviction is
  strict creation-order FIFO. A checkpointed or crashed task remains an active
  logical task and blocks package replacement until explicitly closed.
- Launch/resume, stop, permission reset and uninstall with confirmation.
  Uninstall removes executable package versions and permission decisions while
  retaining private data for a later explicit `Clear data` operation.
- Global permission overview, document access history and storage summary are
  planned as separate Settings pages once their bounded query APIs exist.
- Notification policy, default intent handlers and camera/microphone/network
  access history need appd-owned audit APIs. The Shell must never inspect
  application-private data to construct these views.
- Live trusted thumbnails, restore-before-first-frame and background resource
  pressure behavior stay visibly gated until compositor, Runtime and measured
  CM0 integration are complete; local placeholder cards are not device proof.

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
| audio broker | PCM, output volume/mute, key sounds and focus policy | unrestricted ALSA devices or format selection |
| Owner USB Media broker | fixed exchange-image MSC, bounded music import and read-only photo copies | active partitions, block devices, shell, App deployment, caller paths or photo mutation |
| camera broker | capture profile and orientation policy | raw V4L2 devices |
| power broker | implemented restart/shutdown; future supported charge policy | arbitrary systemd or sysfs operations |
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

- [x] Implement and locally validate the Shell-only display settings broker,
  fixed V0.6 backlight path, safe percentage bounds and Fn+U/Fn+I integration.
- [x] Implement and locally validate role-separated ES8389 output settings in
  `cp0-audiod`, fixed DACL/DACR/Speaker controls, bounded volume/mute state and
  Settings/Fn+A/S/D integration without exposing PCM authority to the Shell.
- [x] Validate the V0.6 ES8389 broker on hardware: DACL/DACR volume and Speaker
  mute round-trip through ALSA, root denial for output settings, Shell denial
  for PCM, mono SDK playback converted to stereo hardware frames, and stereo
  capture explicitly started then downmixed to mono SDK samples.
- [x] Validate the pinned V0.6 backlight sysfs path on hardware, including
  broker-controlled 65/75 write/readback, and confirm that the unprivileged
  login user cannot read the protected control. A split sysfs attribute write
  discovered during acceptance is fixed with one complete `write(2)` call.
- [x] After the user-approved no-deploy window, inspect exact backlight, ALSA,
  rfkill/network and camera controls. V0.6 exposes the pinned backlight and
  ES8389 controls, an I2C-1 kernel bus without a raw device node, no camera
  sensor, and a BCM43439 whose firmware is missing from the flashed image.
- [x] Validate physical Fn combinations and LCD overlays. Operator-confirmed
  physical combinations are supplemented by compositor-path injected input and
  116 exact 320x170 trusted captures from the approved RAM-overlay deployment.
  Battery telemetry, I2C-1 diagnostics and the factory gate also pass on V0.6.
  Retained evidence is `target/device-evidence/20260802T033357Z-3730` and
  `target/device-evidence/qa-system-apps-20260802`.
- [ ] Validate uninstall persistence across a flashed-image reboot. The
  confirmation/cancel path passes, but this run did not delete the installed
  package or reboot the device.
- [ ] Measure idle memory, input latency and SD writes before enabling settings
  persistence by default. The final 60-second RAM-overlay run measured 1.306%
  core CPU, zero SD writes and 212.7 MiB minimum available memory. Its 202.1 MiB
  derived used-memory value exceeds the 180 MiB product ceiling, and the Shell
  activation timestamp reflects a late hot restart rather than boot readiness;
  both cold-start gates require the next flashed image because RAM-overlay
  deployments roll back on reboot.
- [x] Implement and locally validate the compositor-owned ESC long-press Home
  gesture; `Fn+K` remains the foreground application's standard Home key.
- [ ] Physically validate ESC short/long press in Home, standard and immersive
  application states, including the 800-millisecond threshold and release.
- [x] Add the trusted screenshot broker with exact-client compositor
  authorization, fixed 320x170 capture, atomic bounded PNG storage and
  completion states. Physical `Fn+J` and SD latency remain device acceptance.
- [x] Add the targetless media-session broker with Runtime-bound identity,
  bounded per-session actions, lifecycle clearing, SDK APIs and explicit Shell
  completion states. Physical `Fn+Q/W/E` remains device acceptance.
- [x] Add SDK 1.1 48 kHz stereo playback, HTTPS Range streaming, persistent
  global key-sound policy, and the production Music App locally.
- [x] Implement the independent Owner USB Media daemon, password-gated Shell
  flow, isolated FAT32 image, WAV import, JPEG/BMP photo export and hashes.
- [ ] Physically validate enumeration/eject, interruption recovery and hash
  round trips on V0.6 with macOS, Linux and Windows hosts.

Local broker work may proceed independently. Deployment remains gated on the
retained stability evidence passing verification and an explicit device step.
