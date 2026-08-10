# CardputerZero V0.6 System Apps Device Test Report

<!-- doc-locale: en -->
> **English** | [简体中文](DEVICE-SYSTEM-APPS-TEST-REPORT-20260802.zh-CN.md)

## Test identity

- Date: 2026-08-02, Asia/Shanghai
- Device: CardputerZero V0.6, Raspberry Pi CM0, 512 MiB RAM
- Device address: `192.168.31.121`, wired test connection
- Display: internal LCD, `320x170@30`
- Deployment: volatile RAM-overlay binaries and services; no image flash and no reboot
- Input: Linux `uinput` keyboard using the same Weston/libinput and trusted
  compositor paths as the physical keyboard
- Visual evidence: trusted compositor screenshot API, exact 320x170 PNG output

Status meanings:

- `PASS`: exercised on the V0.6 device and observed through UI plus a backing
  service, hardware readback, or state transition where applicable.
- `BLOCKED`: the implementation path exists, but the required external
  configuration is absent.
- `NOT SUPPORTED`: the V0.6 product capability is intentionally unavailable
  and the UI correctly reports it.
- `NOT EXECUTED`: the control and confirmation UI passed, but committing the
  destructive or persistent operation was outside this test's constraints.

## Outcome

The Home shell and all five System Apps completed the available functional
matrix. Device and Network no longer show errors, Settings no longer reports
available Wi-Fi/display/audio controls as unavailable, and all supported
settings completed a state-change/readback/restore cycle.

One device-only defect was found and fixed during the run. `cp0-appd` used the
`systemctl wait` subcommand, which is unavailable in the device's Debian
systemd version. Application execution worked, but the runtime monitor logged
an error every five seconds and could delay session cleanup. The monitor now
polls `systemctl is-active` every 250 ms. Host tests, the AArch64 release build,
and device launch/stop verification pass with no new wait errors.

## System App matrix

### Home and global navigation

| Function | Result | Device observation |
| --- | --- | --- |
| Apps, Store, Device, Network, Settings tiles | PASS | All five open the correct single foreground view. |
| Directional navigation, Enter, Backspace/ESC | PASS | Selection, open, and Back behavior verified across all views. |
| F1 Home, F2 Back, F3 Tasks, F4 Power | PASS | Trusted compositor actions and overlays verified. |
| Brightness, volume, mute shortcuts | PASS | Overlay plus display/ALSA readback; restored to 75%, 75%, unmuted. |
| Media Play/Pause, Previous, Next | PASS | Correct `UNAVAILABLE` completion when no media session is registered. |
| Help and PrintScreen | PASS | Help opens/closes; trusted screenshot capture produced exact 320x170 PNGs. |
| Sleep and wake | PASS | Display slept and F1 woke it back to Home. |

### Apps

| Function | Result | Device observation |
| --- | --- | --- |
| Installed-app list | PASS | Hello Card listed with lifecycle state. |
| Overview, storage, permissions, actions pages | PASS | Version, ID, install time, package/private bytes, permissions and actions rendered. |
| Launch and stop | PASS | systemd unit changed `inactive -> active -> inactive`; compositor removed the app surface. |
| Uninstall confirmation and cancel | PASS | Default-safe cancel path and retained-private-data warning verified. |
| Commit uninstall | NOT EXECUTED | Would delete the installed package outside the repository. |
| App isolation | PASS | App UID received `EACCES` for appd control and audio, display, and connectivity control sockets. |

### Store

| Function | Result | Device observation |
| --- | --- | --- |
| Today, Apps, Search, Updates tabs | PASS | Each tab reached independently; no duplicate Search/Updates evidence. |
| Refresh handling | PASS | Today and Apps remain stable and explicitly show `NOT CONFIGURED`. |
| Search text input | PASS | Entered `tesx`, Backspace, completed `test`, submitted and cleared. |
| HTTPS catalog browse/search | BLOCKED | `store.conf` has no catalog endpoint by product design. |
| Install, update, update-all and download controls | BLOCKED | No signed catalog item exists to operate on. |
| App metrics upload | BLOCKED | Metrics endpoint is not configured; UI reports `NOT CONFIGURED`. |

### Device

| Page | Result | Device observation |
| --- | --- | --- |
| Overview | PASS | CM0/V0.6 identity, Debian 13, uptime and CPU temperature shown. |
| Resources | PASS | 414 MiB total memory, SD storage and isolated app storage shown. |
| Power | PASS | Capacity, discharging state, voltage, signed current and power shown. |
| Diagnostics | PASS | Display, keyboard and audio ready; camera unavailable; I2C restricted. |

### Network

| Page | Result | Device observation |
| --- | --- | --- |
| Status | PASS | Online through `eth0`, address `192.168.31.121`. |
| Details | PASS | Link up, connectivity ready, IPv4 and read-only management shown. |

### Settings: Connectivity

| Item | Result | Device observation |
| --- | --- | --- |
| Wi-Fi | PASS | OFF/ON transaction audited by connectivityd; final state ON. |
| Airplane mode | PASS | ON disabled Wi-Fi; OFF restored radios transactionally; final state OFF. |
| Network details | PASS | Opens the Network status view. |
| Bluetooth | NOT SUPPORTED | Stable disabled row. |
| Hotspot | NOT SUPPORTED | Stable disabled row. |
| VPN | NOT SUPPORTED | Stable disabled row. |

### Settings: Display

| Item | Result | Device observation |
| --- | --- | --- |
| Brightness | PASS | 55%, 65%, 75% written and audited; sysfs final readback is 75. |
| Theme | PASS | Light, High Contrast and Dark rendered; final state Dark. |
| Screen timeout | PASS | 5 min, Never, 30 sec and 1 min applied to compositor; final state 1 min. |
| Preference persistence | PASS | Shell restart retained Dark, 1 min and Key Sounds ON. |

### Settings: Sound

| Item | Result | Device observation |
| --- | --- | --- |
| Media volume | PASS | 65%/75% broker cycle; DACL and DACR final ALSA readback is 75%. |
| Mute | PASS | Speaker OFF/ON cycle; final ALSA Speaker state is ON. |
| Key Sounds | PASS | OFF/ON persisted; bounded 240-frame click path exercised. |
| Output | PASS | ES8389 output reports `READY`. |

### Settings: Camera

| Item | Result | Device observation |
| --- | --- | --- |
| Resolution | NOT SUPPORTED | `NO CAMERA`; no fabricated profile. |
| Rotation | NOT SUPPORTED | `NO CAMERA`; control has no side effect. |
| Mirror | NOT SUPPORTED | `NO CAMERA`; control has no side effect. |
| Camera access | NOT SUPPORTED | Hardware diagnostics and Settings agree. |

### Settings: Power

| Item | Result | Device observation |
| --- | --- | --- |
| Battery status | PASS | Opens Device power telemetry. |
| Battery saver | NOT SUPPORTED | Stable disabled row. |
| Charge limit | NOT SUPPORTED | Stable disabled row. |
| Restart | PASS / NOT EXECUTED | Restart-selected confirmation verified and canceled; no reboot. |
| Power off | PASS / NOT EXECUTED | Power-off-selected confirmation verified and canceled. |

### Settings: Apps and Privacy

| Item | Result | Device observation |
| --- | --- | --- |
| Installed Apps | PASS | Opens Apps. |
| Permissions | PASS | Opens Apps permission management; zero policy-denied permissions. |
| Storage | PASS | Opens Device resources. |
| Document access | NOT SUPPORTED | Audit provider is not implemented. |
| Auto App Updates | PASS | OFF/ON cycle logged; final state ON and `WAIT POWER` while discharging. |
| App Metrics | BLOCKED | Endpoint not configured; no identity or logs were sent. |

### Settings: System

| Item | Result | Device observation |
| --- | --- | --- |
| About | PASS | Opens Device overview. |
| Hardware diagnostics | PASS | Opens Device diagnostics. |
| Date and time | PASS | Read-only automatic state rendered. |
| Language | PASS | Read-only English state rendered. |
| Accessibility | PASS | Read-only default state rendered. |
| OS Update | NOT SUPPORTED | No update service/endpoint is claimed. |

### Settings: Security

| Item | Result | Device observation |
| --- | --- | --- |
| Authority | PASS | Personal management authority rendered. |
| Developer Mode | PASS / NOT EXECUTED | Enable warning and default Cancel verified; mode remains OFF. |
| Recovery Boot | PASS / NOT EXECUTED | Next-boot warning and default Cancel verified; mode remains OFF. |
| Screen Lock | NOT SUPPORTED | No product authentication design is claimed. |
| Encryption | NOT SUPPORTED | No verified-boot/storage claim is made. |

Developer and Recovery enable commits were not run because restoring either
mode deletes a marker below `/var/lib/cardputerzero`, contrary to the explicit
instruction not to delete files outside the repository. Neither marker exists
after testing.

## Regression results

- `cargo fmt --all -- --check`: PASS
- `cargo test -p cp0-appd -p cp0-audio-protocol -p cp0-audiod`: PASS,
  97 unit tests
- `tests/test-system-shell-ui.sh`: PASS, including 320x170 snapshot hashes
- `tests/test-compositor-profile.sh`: PASS
- `tests/test-appd-profile.sh`: PASS
- `tests/test-device-deployment.sh`: PASS
- `git diff --check`: PASS
- AArch64 `cp0-appd` build: PASS
- Device `cp0-appd` SHA-256:
  `123b76bf2523bb01d091cecc6825d9992ea18f753c6e622a0e85bc8a8c8d339a`

## Final device state

- compositor, System Shell, appd, stored, audiod, connectivityd and displayd:
  all `active`
- compositor memory: 9,289,728 bytes
- System Shell memory: 1,744,896 bytes
- appd memory: 970,752 bytes
- foreground application unit: `inactive`
- Wi-Fi: ON; airplane mode: OFF; wired network: connected
- brightness: 75%; theme: Dark; screen timeout: 1 minute
- media volume: 75%; mute: OFF; Key Sounds: ON
- Auto App Updates: ON, waiting for external power
- Developer Mode and Recovery Boot: OFF
- final visible view: Home, Apps selected

## Evidence

- Extracted PNG set and checksum manifest:
  `target/device-evidence/qa-system-apps-20260802/`
- Transport archive:
  `target/device-evidence/qa-system-apps-20260802-final.tar.gz`
- Earlier factory gate:
  `target/device-evidence/cp0-factory-evidence-20260802T033357Z-3730.tar`

The evidence directory contains a named image for every primary page, each
Settings row or state family, destructive confirmation dialogs, global action
overlays, Store search, app lifecycle, and the final restored state.

## Remaining image-flash checks

No new image was flashed in this run. The following cannot be closed by a
RAM-overlay deployment:

1. Cold-boot service readiness and memory budget from the next candidate image.
2. Cold-boot BCM43439 firmware/package behavior.
3. Persistence across a physical reboot rather than a Shell/service restart.
