# Phase 2: Compositor bring-up

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

V0.6 hardware validation passed with Weston 14.0.2, the DRM atomic backend,
Pixman shadow framebuffer, kiosk shell and `weston-simple-shm` at
`320x170@30Hz`. Libinput selected `tca8418c` when the custom seat was active.
The non-root compositor and test client used 8.2 MiB together during the
hardware test.

## Development activation

The compositor remains disabled until the real System Shell replaces the SHM
test client. To switch the LCD from the recovery console to the compositor:

```sh
sudo systemctl start cardputerzero-compositor.service
```

To return to the local login console:

```sh
sudo systemctl stop cardputerzero-compositor.service
sudo systemctl start getty@tty1.service
```

Runtime logs are kept in tmpfs at `/run/cardputerzero/weston.log`.
