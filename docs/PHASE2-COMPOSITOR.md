# Phase 2: Compositor bring-up

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE2-COMPOSITOR.zh-CN.md)

## Baseline

CardputerZero OS pins Weston 14.0.2 at commit
`015b3b4d4c05da44a22349ea6e651d1a8f678c59`. The image builds Weston from
source with only these production-facing parts:

- DRM and 320x170 headless test backends;
- Pixman software renderer and shadow framebuffer;
- kiosk shell and the SHM smoke-test client;
- libseat/seatd session control and libinput keyboard handling.

EGL, XWayland, desktop/IVI shells, RDP, VNC, PipeWire, GStreamer, VA-API,
remoting, demo clients and upstream tests are disabled. The staged Weston
runtime is about 4 MB before stripping, compared with roughly 487 MB of
additional installed size for Debian's general-purpose Weston package.

## Hardware routing

The BSP udev rules assign the internal LCD and keyboard to
`seat-cardputer-zero` and create stable aliases:

```text
/dev/dri/cardputer-zero-internal
/dev/input/cardputer-zero-internal
```

The compositor service selects both the stable DRM alias and the custom seat.
This excludes HDMI, IR and unrelated external input devices from the trusted
system-shell input path.

The aliases appear about ten seconds into CM0 coldplug. The compositor unit
therefore requires the corresponding systemd device units instead of using
one-shot `ConditionPathExists` checks. The keyboard udev rule carries the
`systemd` tag so its stable alias also receives a device unit. This prevents an
early multi-user target transaction from permanently skipping the compositor
before the LCD and keyboard aliases exist.

The product does not statically enable either `getty@tty1` or the compositor.
At manager startup, `cardputerzero-display-generator` selects exactly one
session: the compositor for a normal product boot, or the recovery console for
a recovery image or a product with the persistent recovery marker. This avoids
placing mutually conflicting display sessions in the same systemd transaction.
The compositor's `OnFailure=getty@tty1.service` remains an independent fallback
for failures after normal-session selection.

## Splash handoff

The product keeps the BSP RGB565 splash visible while the cold-boot display
stabilizer waits. It then starts Weston exactly once. Kiosk shell autolaunches
`cardputerzero-boot-splash` as `cp0-compositor`; compositor policy accepts its
reserved app-id only from that UID and places it above normal apps but below
the trusted System Shell. The compositor service makes a bounded wait for the
splash client's first frame callback before it becomes active, so the normal
System Shell startup cannot race the Wayland splash. Its first complete Setup
or Home surface then covers the splash without an intermediate clear. A broken
splash client does not block recovery or Home indefinitely.

The splash surface is excluded from app discovery, Tasks and screenshots of
application state. It remains available during the boot session so an early
System Shell restart exposes the product image rather than a black compositor
background. This handoff complements the initramfs and framebuffer renderers;
it does not use the retired VideoCore firmware or change the 64/448 MB memory
budget.

V0.6 hardware validation passed with Weston 14.0.2, the DRM atomic backend,
Pixman shadow framebuffer, kiosk shell and `weston-simple-shm` at
`320x170@30Hz`. Libinput selected `tca8418c` when the custom seat was active.
The final image's systemd cgroup reported 9.7 MiB for the non-root compositor
and test client, with zero restarts. Stopping the service restored `tty1`, and
a subsequent reboot kept the compositor disabled and the recovery console
active.

## Development activation

The compositor remains disabled until the real System Shell replaces the SHM
test client. Phase 2A now supplies that client, but default activation remains
disabled until its compositor-side trusted overlay is complete. To switch the
LCD from the recovery console to the compositor:

```sh
sudo systemctl start cardputerzero-compositor.service
```

To return to the local login console:

```sh
sudo systemctl stop cardputerzero-compositor.service
sudo systemctl start getty@tty1.service
```

Runtime logs are kept in tmpfs at `/run/cardputerzero/weston.log`.

Weston's DRM backend also requires an `AF_NETLINK` udev monitor to discover and
track the dedicated input device. The compositor unit therefore permits exactly
`AF_UNIX AF_NETLINK`; restricting it to `AF_UNIX` makes backend creation fail
after DRM opens successfully. This failure was reproduced and fixed on V0.6 on
2026-07-31, after which Weston enabled `UNNAMED-1` at 320x170@30 and registered
the `tca8418c` keyboard.
