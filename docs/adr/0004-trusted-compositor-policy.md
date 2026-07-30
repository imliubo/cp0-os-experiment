# ADR 0004: Weston trusted compositor policy

- Status: Accepted
- Date: 2026-07-30

## Decision

Phase 2B keeps the pinned Weston 14 compositor and kiosk shell, and loads a
small CardputerZero policy module after the shell. The module owns the trusted
system layer, the global system-key bindings and a private versioned Wayland
protocol used only by the System Shell.

Weston runs as `cp0-compositor`; the System Shell runs as `cp0-shell`. They
share only the `cp0-wayland` group needed to reach the compositor socket. The
policy accepts its private protocol only when the kernel-provided Wayland peer
UID equals the dedicated `cp0-shell` UID. The `os.cardputerzero.shell` app-id
is checked as an additional consistency rule, but is not treated as identity.

The trusted view is always placed at `WESTON_LAYER_POSITION_TOP_UI` while
visible and at `WESTON_LAYER_POSITION_HIDDEN` otherwise. Home, Back, Tasks
and Power are compositor key bindings. Their actions are sent over the private
protocol, so they do not depend on the currently focused application.

## Rationale

An ordinary xdg-toplevel cannot prevent another client from covering it and
cannot receive keys while another application owns focus. Forking all of
Weston or introducing a second compositor would greatly increase the trusted
code and maintenance cost. A narrow policy module uses existing libweston
layering and peer credentials while keeping the proven DRM/Pixman path.

Separating the process accounts also prevents a compromised System Shell from
using same-UID process access against Weston. Third-party applications and App
Runtime processes must never receive the `cp0-shell` UID or membership in
`cp0-wayland`; appd will pass only explicitly brokered connections or file
descriptors.

## Consequences

The private protocol is part of the OS internals, not the public application
SDK. Its XML is versioned and generated during the image build. A failure to
load the policy fails compositor startup; a failure to authenticate the System
Shell prevents that client from starting. Neither path silently falls back to
an insecure xdg-only System Shell.

This decision establishes the trusted overlay and global-key boundary, but it
does not by itself implement application launch, permission decisions or
24-hour stability. Phase 2B still requires two-client switching, overlay
visibility transitions, screenshot regression and malicious-client tests.
