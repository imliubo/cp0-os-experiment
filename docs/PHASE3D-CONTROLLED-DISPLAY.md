# Phase 3D: Controlled application display

## Security boundary

Application accounts are not members of `cp0-wayland` and cannot traverse the
compositor runtime directory. `appd` asks systemd PID 1 to connect the exact
root-controlled Wayland socket with `OpenFile=`. The resulting connected stream
is descriptor 3 in the transient service. The pinned bubblewrap 0.11.0 passes
that inherited descriptor to its PID-namespace child while its monitor closes
its own copy; no socket path is mounted into the sandbox. The transient unit
does not inherit any other non-standard descriptor.

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
progress. A monotonic host-side pacer also rejects commits closer than
33,333,334 ns, enforcing the architecture's 30 FPS ceiling even when a WASM
application spins instead of waiting for input.

## Build and tests

`make app-runtime` builds host `wayland-scanner`, cross-compiles a static
AArch64 libffi, generates core and xdg-shell protocols and links the final
static Runtime. `tests/test-runtime-display.sh` verifies RGB565 conversion,
the standard-mode offset and hostile damage bounds independently of hardware.

Hardware acceptance additionally requires the SDK-only Hello application to
render through the appd/systemd/bubblewrap path, compositor discovery and
single-foreground activation. The app account must remain unable to open the
Wayland socket path directly.

## V0.6 validation

The controlled path was hot-deployed to the 512 MB V0.6 device without an
image flash. systemd opened the compositor endpoint and the Runtime inherited
FD 3 as `socket:[34366]` while running with UID/GID 20000, seccomp mode 2 and
the exact application cgroup. The application account remained outside the
`cp0-wayland` group and could not traverse `/run/cardputerzero`.

Weston announced `app token=1` after the SDK-only Hello WASM committed its
first buffer. A trusted 30-second activation client exposed the surface, and a
4K camera check showed the expected white border and red, green and blue
RGB565 bands. Restarting the production Shell restored Home while the hidden
application remained alive. The application unit used 9.8 MiB at peak, zero
swap and three tasks; compositor, Shell and appd stayed active. The application
was then stopped through appd, leaving all three core services active.
