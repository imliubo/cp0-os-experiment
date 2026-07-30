# Phase 2C: Single-foreground window switching

## Window policy

The compositor policy now owns three explicit layers:

- the trusted System Shell layer at `TOP_UI`;
- one normal application layer;
- a hidden layer for the Shell or all inactive applications.

When the Shell is visible, every application view is hidden. Activating an
application hides the Shell and moves exactly one mapped application view to
the normal layer. All other application views remain hidden and do not receive
keyboard focus. A global system action moves the Shell back to the trusted
layer before delivering the action event.

Desktop child and popup surfaces are resolved to their root toplevel. Only the
root receives an application token; its children follow the same trusted,
active or hidden layer so dialogs cannot become independent launcher entries.

The policy fails closed. If the Shell disconnects, application views are
hidden until a trusted replacement registers. If the active application
unmaps or exits, the policy clears its token, shows the Shell and emits Home.

## Protocol version 2

`cp0_system_shell_v1` version 2 adds application discovery and activation.
The compositor allocates an opaque non-zero token for each mapped desktop
surface and sends `app_added` and `app_removed` events. The Shell activates a
surface with `activate_app(token)`.

If a surface exits while an activation request is in flight, the compositor
returns `activation_failed(token)`. This lifecycle race does not disconnect
the trusted Shell; the Shell removes the stale row and remains visible.

The token, not app-id, selects the compositor-owned surface. app-id remains
untrusted display data and is restricted to 47 printable identifier
characters before it reaches the Shell. A future appd protocol will associate
the token with a verified manifest identity; version 2 does not grant a normal
application access to the trusted protocol.

The 320x170 Shell renderer keeps at most four visible application rows. Its
state tests cover add, update, selection, removal and open events. Pixel-exact
SHA-256 snapshots cover Home, Apps, Tasks and Power states.

## V0.6 hardware validation

The policy, Shell and private protocol were compiled for AArch64 against the
pinned Weston 14.0.2 ABI with `-Werror`. The binaries were hot-deployed; no
image flash was required.

`weston-simple-shm` ran as a second Wayland client under a UID different from
the Shell. The policy assigned token 1 and the Shell received the same token.
While that fullscreen client continuously committed buffers, camera inspection
confirmed that it could not cover the trusted Home screen.

A trusted test controller activated token 1. Camera inspection then showed the
application fullscreen. Restarting the production Shell hid the still-running
application and restored Home. A stress pass completed 200 application/Shell
layer transitions; compositor, Shell and test application remained active,
and the final physical LCD frame was Home.

A stale-token test used `UINT32_MAX` and received `activation_failed` while the
trusted client connection stayed alive. Stopping the transient application
produced `app_removed`; the production compositor and Shell remained active.

The test controller is source-only under `tests/` and is not installed in the
image. Product validation still requires the real App Runtime connection path,
non-focused keyboard event tests, permission overlays, immersive mode, screen
sleep and the 24-hour compositor run.
