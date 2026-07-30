# Phase 2A: System Shell prototype

## Scope

Phase 2A replaces the diagnostic `weston-simple-shm` startup client with a
trusted native Wayland client. It does not use GTK, Qt, Cairo or a desktop
shell. The deterministic renderer owns two XRGB8888 SHM buffers, about 425 KiB
at 320x170, and provides:

- a four-entry Home screen for apps, device, network and power;
- a 21 px status bar with time, network state and battery capacity;
- Home, Back, Tasks and Power states with keyboard navigation;
- a trusted power-dialog state machine;
- a 30-second timer for idle status refresh.

The renderer is independent from Wayland. `tests/test-system-shell-ui.sh`
compiles it on the host and checks state transitions, boundary guards and
stable layout pixels at the native LCD resolution.

## Input mapping

The Wayland client consumes Linux keyboard keycodes from the compositor. The
prototype maps arrows and Enter for navigation, Escape/Backspace/F2 for Back,
Home/Homepage/F1 or Meta+H for Home, F3 or Meta+Tab for Tasks, and Power/F4 for
the power dialog. The mapping deliberately does not open evdev directly.

The V0.6 hardware test confirmed that Weston/libinput attaches `tca8418c` to
the internal output and gives the System Shell keyboard focus. Final Fn-layer
product mappings remain pending until every physical key combination is
recorded on hardware.

## Process supervision

Weston and the System Shell are separate systemd services. Starting
`cardputerzero-compositor.service` pulls in the Shell; the Shell waits for the
Wayland socket, uses a 32 MiB cgroup limit and restarts after failure. Stopping
the compositor stops the Shell through `BindsTo` and `PartOf`.

On V0.6 hardware the Shell used about 0.9 MiB according to its systemd cgroup
and 2.0 MiB RSS. A forced SIGKILL changed the PID from 1107 to 1121 with
`NRestarts=1`; the new process returned to Home while Weston stayed active.
The timer-enabled build then ran through its first idle refresh with zero
restarts.

## Image candidate

The integrated Phase 2A image is:

```text
deploy/image_2026-07-30-cardputerzero-os-phase2a-cp0-os-dev.img.xz
SHA-256 93793244fa610cfa82203ef325045119ad9c03d1cc64a1c6bd67017bf91179b5
```

It is 223 MiB compressed. Offline validation checked both deploy hashes, both
package manifests, the arm64 Shell executable, compositor/Shell units, the
32 MiB Shell limit, exactly one managed BSP block and the compositor's default
disabled state. A transient repository 502 also exercised the resumed build;
the BSP and DTB patch paths are now safe to run more than once.

## Security boundary

Weston kiosk shell is still a bring-up component. An ordinary xdg-toplevel
cannot prove that its dialog is above an untrusted client, and its key
shortcuts are only available while it owns keyboard focus. Therefore Phase 2A
does not yet satisfy the secure permission-dialog or global Home/Back
requirements.

Before third-party applications are enabled, the compositor side must expose
an authenticated System Shell control protocol or equivalent built-in policy
that reserves the system overlay layer, enforces one foreground app and owns
global shortcuts. Only then can permission prompts be treated as a security
boundary.

## Development activation

The compositor remains disabled by default. After building a new image, start
the integrated Shell with:

```sh
sudo systemctl start cardputerzero-compositor.service
```

Return to the recovery console with:

```sh
sudo systemctl stop cardputerzero-compositor.service
sudo systemctl start getty@tty1.service
```
