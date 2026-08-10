# CardputerZero OS Technical Architecture

<!-- doc-locale: en -->
> **English** | [简体中文](ARCHITECTURE.zh-CN.md)

## 1. Goals and Constraints

The target is fixed to CardputerZero V0.6: Raspberry Pi CM0, 512 MB RAM, SD
card, and a 320x170 LCD. The system has one foreground app and does not support
traditional Linux desktop apps. Third-party development uses the CardputerZero
SDK exclusively.

The security goal is to prevent a malicious app from directly accessing
another app, hardware, or an unauthorized system capability. A shared Linux
kernel cannot provide mathematically absolute isolation, so the design combines
a WASM runtime boundary with a Linux process sandbox.

## 2. Layered Architecture

```text
Third-party application (.capp / WASM)
        |
Cardputer App Runtime (WAMR, one process per running app)
        |                         \
Cardputer SDK host calls          Wayland surface
        |                           |
Capability services         System Shell + Compositor
        |                           |
appd + permissiond + hardware brokers
        |
systemd + Debian arm64 + DRM/KMS + kernel drivers
        |
LCD / keyboard / audio / battery / camera / LoRa / GPIO
```

## 3. Base System

- Debian arm64 minimal and systemd, built using the existing `pi-gen` workflow.
- No X11, full desktop environment, browser, or native compiler in the image.
- A pinned ST7789V DRM/KMS MIPI-DBI driver; apps cannot access framebuffer.
- libinput/evdev delivers keyboard input to the compositor; only the focused
  window receives app events.
- zram without ordinary SD-card swap or zram writeback.
- An eventually read-only system root. A/B OTA and secure boot are outside the
  initial release.

## 4. Graphics and Window Model

The first runnable version uses Weston kiosk shell to validate DRM, Wayland,
and app lifecycle. Product policy lives in a small compositor module,
`cardputerzero-policy.so`, which authenticates the dedicated Shell UID, owns
trusted system layers, and handles global keys. A dedicated wlroots compositor
is considered only if this bounded Weston module cannot satisfy the product.

Window policy is fixed:

- one foreground app surface and one keyboard focus; at most ten logical tasks,
  with background surfaces hidden and unable to cover trusted layers;
- standard mode is 320x150 below an approximately 20 px status bar;
- immersive mode uses 320x170;
- permissions, volume, notifications, and Tasks are trusted System Shell
  overlays;
- app rendering is RGB565 at no more than 30 FPS, with damage regions reducing
  SPI transfers.

Private Shell protocol v7 defines `full`, `status`, `hidden`, and
`notification` coverage. A standard app retains focus below the trusted 21 px
ARGB status bar. Notification mode displays an 88 px trusted banner without
moving keyboard focus. A permission prompt forces opaque `full`. A system page
stays `full` when volume, brightness, or media keys are pressed; only an app in
the foreground temporarily enters `notification`. Repeated shortcut presses
retain the originally saved base mode and restore it exactly on expiry. An
immersive app occupies the whole display only when no trusted overlay is
visible. Compositor always owns global keys and display sleep/wake, which an app
cannot intercept or spoof.

Weston and System Shell have different UIDs and share only the `cp0-wayland`
group required for the compositor socket. Third-party apps never join it.
Private Shell authentication uses the Wayland peer UID, not a forgeable app-id
or RPC field. Protocol v7 also sends the compositor-observed app UID to Shell.
With one task per app, UID prevents cross-app surface replacement; durable task
restore still requires `(task_id, runtime_generation)` and must not activate a
stale generation from UID alone.

## 5. Application Runtime

The device uses WAMR AOT because its resident footprint fits a 512 MB system
better than a full JIT or Component Model runtime. WIT describes the public SDK
types. A machine-readable flat-ABI contract generates the WAMR registry and
private C/Rust imports and maps each operation back to WIT. The first release
does not require native WASM Component Model support on-device.

appd launches every resident task into its own sandbox:

- separate UID, PID/mount/network namespaces, and cgroup; a fixed 60% CPU quota
  and lower CPU weight keep one app from monopolizing the single-core CM0;
- `no_new_privs`, no capabilities, and a syscall allowlist enforced by seccomp;
- read-only package, one quota-controlled private-data directory, and empty
  device view;
- no system D-Bus, Wayland socket path, evdev, DRM, ALSA, or GPIO exposure;
- PID 1 passes only preconnected Wayland FD 3 to trusted App Runtime, which
  brokers SDK calls;
- Runtime queues input only between `wl_keyboard.enter/leave`. It shares the
  V0.6 printable-ASCII mapping with System Shell. Each of 32 `Sym` combinations
  has a unique keycode; ordinary letters combine depressed XKB Shift with a
  physical-Shift fallback to populate `KeyEvent.character`. Apps consume this
  system character field and do not maintain private key maps.

System Shell obtains installed manifest summaries from an authenticated appd
control socket and never infers installation from Wayland surfaces. Launcher
app ID, display name, and standard/immersive policy come from root-controlled
manifests; a compositor token identifies only one temporary mapped surface.
appd starts the task, and Shell activates the token only after identities match
in trusted events. appd protocol v2 tracks up to ten tasks, one foreground task,
FIFO capacity eviction by creation sequence, and an independent MRU switch
order. F3 Tasks presents 160x85 cards; Enter activates and Up closes. Home shows
trusted Shell without destroying the foreground task.

An app has at most one task. While any task exists, non-idempotent package
changes such as developer install, Store upgrade, rollback, and uninstall
require explicitly closing it first. A checkpointed or crashed task with
`is_running() == false` cannot bypass the gate. Same-version Store requests
remain safely replayable. After Intent acceptance is written back to the
sender, the receiver becomes foreground and the sender becomes background
rather than being destroyed.

The merged MT0-MT2 work is a simulation and protocol foundation: multiple
Runtime sessions, task state, F3 cards, checkpoint/journal/thumbnail/resource-
governor models, and SDK lifecycle ABI have automated tests. Compositor-sealed
real thumbnails, authenticated Runtime control, WAMR checkpoint/restore
callbacks, appd boot reconciliation, and a measured CM0 memory-pressure policy
are not in the boot chain. Placeholder cards and model checkpoints must not be
claimed as live hardware recovery. See `docs/MULTITASKING-ARCHITECTURE.md`.

App Runtime moves RGB565 frames between WASM linear memory and a Wayland
surface, converting to Weston's XRGB8888 SHM format. Standard apps submit only
320x150; immersive apps may submit 320x170. The trusted shadow frame plus two
compositor buffers remains below 700 KiB.

## 6. Permissions and Capability Services

App identity comes from the verified package ID and process credentials. It
cannot be supplied in an RPC parameter. Private storage, focused-window input,
and rendering are implicit capabilities; every other capability is declared in
the manifest.

The initial permission vocabulary includes network client, document selection,
audio playback/capture, camera, LoRa, GPIO, clipboard read, and notifications.
On first sensitive use, `permissiond` presents a system prompt for Allow Once,
Always Allow, or Deny.

The current implementation places permissiond coordination inside appd.
System Shell can read and resolve one pending prompt only through a control
socket authenticated with `SO_PEERCRED`. Diagnostic controllers can atomically
reset a persistent decision to Ask Next Time. Third-party apps have no
permission-management endpoint.

System settings use a Shell-only provider separate from app capabilities.
`cp0-displayd` requires both socket DAC and exact `cp0-shell` identity from
`SO_PEERCRED`; it only reads/writes the V0.6 brightness attribute under
`/sys/class/backlight/backlight`. Requests are 5%-100%. Global keys use fixed
10% steps and read back the observed value. Apps, Runtime, and Store have no
corresponding SDK or control path.

Root `cp0-powerd` handles power. Socket DAC allows only
`cp0-power-control`, followed by exact `cp0-shell` UID authentication with
`SO_PEERCRED`. Protocol actions are only restart and power-off, mapped to
`/usr/bin/systemctl --no-block reboot|poweroff`. Requests carry no unit,
argument, or path. Shell gains no sudo, generic systemd, or D-Bus access.
Recovery images mask the endpoint. See `docs/POWER-CONTROL.md`.

The notification broker binds identity from `SO_PEERCRED` and current systemd
cgroup. appd returns only canonical app name and bounded content to trusted
Shell, which owns banner layout and the four-second lifetime. Permission prompts
outrank notifications; Home, Tasks, Power, and app exit revoke the banner.

Network capability exposes no raw socket. Runtime and appd allow `AF_UNIX`
only; `cp0-networkd` alone may use `AF_INET/AF_INET6`. The SDK offers
synchronous HTTPS GET with a 1024-byte URL, five-second total timeout, at most
two redirects, and 2048-byte response body. SDK 1.1 also offers fixed byte-range
GET with at most 8 KiB per request and offsets below 256 MiB. networkd disables
environment proxies and rejects loopback, private, link-local, multicast,
reserved, NAT64/Teredo, and other non-public targets on every connection and
redirect resolution. TLS validation cannot be disabled. appd verifies caller
UID/cgroup, manifest, and permission and releases its shared state lock before
network I/O.

Document sharing exposes no path. Dedicated `cp0-documentd` lists at most 16
shared documents read-only, while trusted Shell shows a single foreground
picker. An app request includes neither path nor document ID. The selection
binds an appd snapshot; documentd opens it with `openat(O_NOFOLLOW)` and then
rechecks device/inode. A read-only FD crosses two `SCM_RIGHTS` hops into
Runtime. WASM receives one generation handle, length, and offset reads of at
most 4096 bytes. A document may be 256 MiB for local music streaming, but is
never copied entirely into app memory.

Audio capability exposes no ALSA device or mixer. `cp0-audiod` alone may access
`char-alsa`, and opens only ES8389 `hw:ES8389Audio,0` with a 48 kHz stereo
hardware stream. Compatibility calls accept 16 kHz mono S16_LE, at most 1024
frames, and perform fixed 3x upsampling plus channel duplication. SDK 1.1 adds
48 kHz stereo S16_LE playback, at most 1920 frames. `audio.playback` and
`audio.capture` are authorized independently; protocol, appd, Runtime memory,
and SDK each validate length and frame alignment.

audiod has a dedicated account, empty capability set, and systemd device
allowlist. Its separate Shell-only output role lets only `cp0-shell` read or
adjust DACL/DACR volume and Speaker mute in fixed 10% steps with observed
readback. It also persists Key Sounds and generates a fixed short click only
for Shell or the current foreground Runtime, remaining silent for password and
Wi-Fi key input. Socket DAC permits Shell connection, but `SO_PEERCRED` plus
command classes prevent Shell from submitting arbitrary PCM/capture and prevent
appd from changing system volume or key-sound policy.

Camera capability exposes no V4L2, Media Controller, dma-heap, VideoCore device,
or capture process. Dedicated `cp0-camerad` exclusively accesses allowlisted
device classes and invokes system `rpicam-vid` with a fixed 1280x720, 30 FPS
YUV420 profile. Only the current foreground Runtime may request frames; two
idle seconds release the pipeline. Preview downscales the same frame to
320x170 RGB565_LE in a sealed 108800-byte memfd. Capture encodes the next frame
as quality-90 1280x720 JPEG without restarting the sensor. V0.6 prefers fixed
`/dev/video31` V4L2 JPEG hardware encoding and falls back to a bounded planar-
YUV software encoder. appd stores the original JPEG and 320x170 Gallery
thumbnail; WASM receives only photo ID, never the large image or native FD.

GPIO capability exposes no `/dev/gpiochip*`, BCM pin, sysfs path, direction, or
multiplexing. V0.6 offers only four overlay-defined logical Booleans: Grove
function, external USB function, Grove 5V power, and external 5V power.
`cp0-gpiod` alone writes their four LED-class attributes. The app-platform stage
overrides BSP's global `0666` with `0660 root:cp0-gpio`. LCD, SPI chip select,
audio, infrared, keyboard, headphone detection, and system-power GPIO never
enter the SDK.

LoRa capability is for an external SX1276 series module; V0.6 has no onboard
LoRa. `cp0-radiod` alone accesses SPI0 CS1 (`/dev/spidev0.1`). Apps and Runtime
cannot choose device, frequency, modulation, transmit power, or register.
Production defaults to `enabled=false`; only root configuration selects a
supported region and in-region frequency. The initial profile fixes 125 kHz,
SF7, CR4/5, CRC, 8-byte preamble, private sync word `0x12`, and 14 dBm. Payload
is at most 64 bytes, sends are at least 15 seconds apart, and receive wait is at
most 1000 ms.

Inter-app calls use appd's Intent Broker. A receiver explicitly exports a
bounded reverse-domain action in its root-owned manifest. There is no arbitrary
inter-app socket, target app ID, or path. Payload is at most 1024 bytes and the
global queue holds eight. Zero or multiple receivers rejects the call. appd
writes acceptance back to the sender before backgrounding it and starting the
unique receiver. The receiver uses one-shot `take`, authenticated by
UID/PID/cgroup, for its bound message. A failed response write removes the
queued item, preserving consistency between acceptance and foreground switch.

App-private data is accessible only through the SDK key/value API.
`cp0-storaged` exclusively owns `/var/lib/cardputerzero/data`. appd authenticates
UID/cgroup and root-owned manifest before providing app ID and `storage_mb`
quota. A value is at most 8 KiB and an app has at most 256 keys. A write
calculates post-replacement logical bytes, writes a same-directory temporary
file, calls `fsync`, and atomically renames. Runtime `/data` is an empty sandbox
directory, not a host writable bind. A compromised Runtime cannot bypass the
broker to read another app or consume arbitrary SD space.

## 7. Packages and Application Store

A `.capp` is an immutable signed package containing at least `app.json`, a
WASM/AOT module, resources, and signature. Developer and Store signatures are
separate: the developer establishes origin; Store signs an installable artifact
after review.

Publication binds a review record to the developer-signed submission's complete
SHA-256, manifest permissions, and actual WASM imports, then creates a
deterministic Store-signed package and Ed25519 Catalog. Device `cp0-stored`
accepts only public HTTPS, verifies Catalog sequence, expiry, and signature,
resumes with strict `Content-Range`, and checks size and SHA-256 on completion.

`cp0-stored` runs as independent `cp0-store`, writes only its cache and
`/run/cardputerzero-appd/store`, and accepts only list/refresh/install-by-app-ID
from Shell. Shell cannot specify URL, path, hash, or version. appd accepts
handoff only from the fixed Store UID, independently rechecks file identity,
manifest, both signatures, and strict SemVer upgrade, and atomically installs
to `/var/lib/cardputerzero/apps/<app-id>/<version>`. Store is unconfigured by
default and product trust roots are not embedded. See
`docs/PHASE5B-APPLICATION-STORE.md`.

Automatic updates default off. `cp0-stored` persists a private atomic
preference and six-hour throttle. It selects at most eight strict upgrades with
no new permission from appd's minimal snapshot only when external power, wired
default route, and independent root policy all allow. appd rechecks automatic
policy, signatures, digest, version, and manifest at handoff. See
`docs/STORE-AUTO-UPDATE-V1.md`.

Root-owned `device-policy.json` sets local parent/organization ceilings: lock
Developer/Recovery Mode, disable Store installs, restrict launchable apps, and
globally deny SDK permissions. appd enforces policy at install, launch, and
every capability request; global denial overrides a user's persistent allow.
System Shell switches only two fixed modes when policy allows and cannot submit
paths, app allowlists, or permission text.

Developer Mode still requires a trusted developer key and valid signature.
Recovery Mode uses a persistent root marker to block compositor and start
`getty@tty1` on next boot; the local keyboard console disables it with
`sudo cp0ctl device recovery off`. See `docs/PHASE5C-DEVICE-POLICY.md`.

On a personal production device, the Owner may physically enable Developer Mode
in trusted System Shell. This starts only a restricted deployment channel, not
Linux administration. A new computer registers an Ed25519 SSH key and 32-byte
developer-signing public key in a separate ten-minute `PAIR NEW COMPUTER`
window; at most eight computers remain paired. Each SSH key has `restrict` and
forced `cp0ctl dev-session`. Root `cp0-devd` rechecks policy, Developer Mode,
signature, and pairing before proxying bounded install, log, and lifecycle
requests to appd. Shell revokes one or all computers, deleting the developer
trust key when its last reference disappears. Full Owner SSH Shell is a
separate marker, off by default, and has no sudo/root when enabled. Developer
Mode never enables it implicitly. See `docs/DEVELOPER-ACCESS.md`.

Music import and photo export use an independent Owner USB Media domain. After
the Owner confirms the current password in trusted Settings, root
`cp0-usb-mediad` exposes only the fixed 512 MiB FAT32
`/var/lib/cardputerzero/usb-media/exchange.img` as one MSC LUN. IPC accepts no
path and rejects symlinks, block devices, and wrong capacity before binding.
rootfs, `cp0-data`, app-private data, and every active partition are forbidden
LUNs. The device mounts the exchange image only while USB is unbound, with
`nodev,nosuid,noexec`. storaged copies read-only photos to `PHOTOS`; validated
48 kHz stereo PCM WAV enters Document Portal atomically. The exchange image is
rebuildable temporary data and absent from recovery backups. This channel
requires neither Developer Mode nor Owner SSH Shell and exposes no shell, app
deployment, or arbitrary file access. See `docs/OWNER-MEDIA-TRANSFER-V1.md`.

The developer channel installs signed `.capp` and proxies bounded app lifecycle
only. It cannot replace appd, System Shell, compositor policy, systemd units, or
the OS image. Multitasking system components therefore cannot be hot-updated
through it; hardware integration requires one controlled same-version system
bundle plus reboot, or a new image, for all three protocol endpoints.

## 8. 512 MB Memory Budget

| Component | Target limit |
| --- | ---: |
| Kernel, systemd, and base services | 100 MB |
| Compositor, Shell, fonts, and graphics buffers | 55 MB |
| appd, permissions, and hardware brokers | 30 MB |
| Foreground App Runtime and app | 96 MB |
| File cache, zram, and burst allowance | 231 MB |

Home idle resident memory targets below 220 MB; total resident app runtime
targets below 360 MB. Ten tasks is a logical limit, not permission for ten
96 MB Runtime processes. Background tasks reduce CPU weight, freeze, or
checkpoint and release their process under a measured policy. cgroups terminate
an app exceeding manifest resources, and System Shell reports the reason.

CM0 memory is fixed at 64 MB VideoCore, 448 MB ARM, and 64 MB VC4 CMA. The
memory cgroup is mandatory for appd manifest limits.

## 9. Trust Boundary

Kernel, compositor, System Shell, App Runtime, appd, and capability services
are trusted computing base. Third-party WASM, app resources, network responses,
and Store content are untrusted inputs. Native third-party executables are
unsupported; Developer Mode also installs only unpublished WASM apps.

## 10. Recovery and Stability

appd and compositor use `Restart=on-failure`; System Shell uses
`Restart=always` and `BindsTo` compositor. Recovery acceptance checks new PID,
expected restart count, Shell reconnection to the new Wayland socket, and appd
control. systemd `active` alone is insufficient.

The 24-hour acceptance writes per-minute service state, restart counts, cgroup
memory, foreground-app count, and socket/ping health to a unique `/run` result
directory, avoiding SD writes. Every sample requires no running app and applies
32/32/24 MiB core-service limits plus final growth limits. The monitor is not a
resident product service.

Phase 6 performance evidence is also `/run`-only and, with no foreground app,
records systemd monotonic boot time, idle memory, core CPU/memory, short SD
writes, and BQ27220 telemetry. Battery-gauge values under USB power do not
measure whole-device consumption and cannot replace a calibrated external meter.

Diagnostics are non-resident and never auto-upload. Default support bundles
include de-identified hardware presence, service properties, resources, and
mount state. Root must explicitly request raw journal, marked sensitive.
Production acceptance read-only checks immutable root, `cp0-data`, fixed
services, and socket permissions and writes results only to `/run`. See
`docs/PHASE6B-DIAGNOSTICS-FACTORY.md`.

The independent recovery image has root-owned `image-profile=recovery`, removes
OverlayFS args, and masks compositor, System Shell, appd, and every capability
socket. It retains tty1, LCD boot summary, network, and SSH. Recovery does not
automatically expand, mount, or bind `cp0-data`, so damaged state is not
implicitly attached to a writable maintenance root. See
`docs/PHASE6C-RECOVERY-IMAGE.md`.

Persistent migration uses versioned `CP0 backup v1`, not generic archive
extraction. It accepts only a `cp0-data-layout-v2` allowlist, records mode/owner,
and hashes every file and the complete payload. Links, special files, path
escape, dangerous permissions, and non-empty targets are rejected. Device
wrappers mount partitions only in independent recovery or product lower-root
maintenance boot. Restore rebuilds `cp0-data` only after full verification and
a fixed confirmation phrase. Product images contain their trusted factory
seed; recovery media does not copy an incomplete product trust root. See
`docs/PHASE6D-RECOVERY-DATA.md`.

Production images contain no fixed human account. pi-gen's temporary
`cp0-build` user, home, groups, and UID residue are removed before export.
Trusted System Shell exclusively owns first-boot 320x170 Setup and calls root
`cp0-provisiond` over `SOCK_SEQPACKET` authenticated by exact Shell UID through
`SO_PEERCRED`. Owner is fixed UID 1000 without sudo; identity database and home
live under `cp0-data-layout-v2`. SSH starts only after Setup and when either
Owner SSH Shell or Developer Mode root marker exists; dispatcher uses the
`cp0-ssh` login group to restrict sessions to Bash or `cp0-dev`. Before Setup,
Home, Tasks, ordinary apps, screenshots, and key clicks are unavailable.
Explicit Offline is a valid persistent network decision. See
`docs/FIRST-BOOT-PROVISIONING.md` and ADR 0007.

System security claims remain bounded by `docs/THREAT-MODEL.md`: app isolation
does not resist kernel compromise, trusted-native-service compromise, or
physical SD attacks. OverlayFS is runtime write protection, not boot integrity.
Development-image shared SSH/explicit password is not a production identity.
The `production` access profile rejects build-time passwords and SSH keys and
locks getty and Recovery Boot. A personal Owner may enable restricted Developer
Mode and separately enable the default-off, no-sudo Owner SSH Shell. Root
maintenance requires an explicitly inserted recovery SD and is revoked when
removed.

Future OS updates use a separate signing root, A/B boot/root, dm-verity, and
health confirmation before commit. Phase 6H implements release metadata outside
the boot chain, verity artifact checking, and a three-failure rollback model.
The decremented dual-copy state must persist before boot; the new slot is
confirmed only after compositor, appd, and `cp0-data` are healthy. Checksums
detect torn writes only. Verified boot exists only if an earlier immutable
stage authenticates U-Boot/FIT. See `docs/PHASE6G-PRODUCTION-ACCESS.md`,
`docs/PHASE6H-VERIFIED-UPDATE-GROUNDWORK.md`, and ADR 0006.
