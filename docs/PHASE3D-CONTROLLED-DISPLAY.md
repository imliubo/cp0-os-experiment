# Phase 3D: Controlled application display

## Security boundary

Application accounts are not members of `cp0-wayland` and cannot traverse the
compositor runtime directory. `appd` asks systemd PID 1 to connect the exact
root-controlled Wayland socket with `OpenFile=`. The resulting connected stream
is descriptor 3 in the transient service. Bubblewrap preserves only that
descriptor with `--keep-fd 3`; no socket path is mounted into the sandbox.

The Runtime requires the fixed `WAYLAND_SOCKET=3` launch contract and calls
`wl_display_connect_to_fd(3)`. App identity and display mode are copied from the
trusted installed manifest into the cleared bubblewrap environment. A WASM
module cannot create another Wayland connection, access a host path or supply
its own xdg app-id.

## Rendering path

Wayland 1.23.1 and libffi 3.5.2 are statically linked into the Runtime. The
xdg-shell bindings are generated from wayland-protocols 1.44. All repositories
and exact commits are pinned in `app-runtime/wayland.env`.

The public host ABI accepts a complete little-endian RGB565 content frame and
up to 32 damage rectangles. The Runtime validates the full WASM memory ranges,
frame length and every rectangle before updating a trusted XRGB8888 shadow
frame. Two `wl_shm` buffers prevent compositor ownership from racing the next
application update.

Standard applications receive 320x150 dimensions. Their content starts at
physical y=20, so the reserved status area is not addressable from WASM.
Immersive applications receive the complete 320x170 frame. The compositor owns
the final surface placement and keeps inactive applications hidden.

The Runtime event wait pumps the Wayland connection, including buffer release,
configure and close events. A busy double buffer returns the stable SDK
`ResourceLimit` result so applications can retry without blocking compositor
progress.

## Build and tests

`make app-runtime` builds host `wayland-scanner`, cross-compiles a static
AArch64 libffi, generates core and xdg-shell protocols and links the final
static Runtime. `tests/test-runtime-display.sh` verifies RGB565 conversion,
the standard-mode offset and hostile damage bounds independently of hardware.

Hardware acceptance additionally requires the SDK-only Hello application to
render through the appd/systemd/bubblewrap path, compositor discovery and
single-foreground activation. The app account must remain unable to open the
Wayland socket path directly.
