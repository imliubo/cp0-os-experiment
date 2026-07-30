# Phase 2B: Trusted compositor policy

## Security contract

The Phase 2B policy must preserve these invariants:

- only a process with the kernel-reported `cp0-shell` UID can bind the private
  System Shell protocol;
- the registered surface must belong to that Wayland client and have the
  expected System Shell app-id;
- a visible trusted surface is placed above normal and fullscreen application
  layers, while a hidden trusted surface is not rendered;
- Home, Back, Tasks and Power are consumed by the compositor and delivered to
  the authenticated Shell independently of application keyboard focus;
- an application cannot join `cp0-wayland`, open DRM/input devices, or run as
  either trusted graphics account.

The app-id check is not authentication. Wayland peer credentials and the
dedicated system account are the security boundary.

## Components

`cardputerzero-policy.so` is a Weston runtime module loaded after kiosk shell.
It creates a `TOP_UI` trusted layer, a hidden layer, global key bindings and
the `cp0_system_shell_v1` global. Surface commits schedule an idle reassertion
so kiosk-shell focus or stacking changes cannot leave the trusted view in an
application layer.

The private protocol supports:

- registering exactly one trusted xdg surface;
- showing or hiding the trusted layer;
- Home, Back, Tasks and Power action events.

The System Shell now requires this protocol at startup. Silent fallback is
intentionally rejected because it would make a permission dialog appear
trusted without compositor enforcement.

## Process boundary

Weston runs as `cp0-compositor` with the video, render and input groups. The
System Shell runs as `cp0-shell` without direct hardware groups. Both use the
`cp0-wayland` group for the `0770` runtime directory and a socket created with
umask `0007`. Weston warns because the generic XDG runtime recommendation is
`0700`; this dedicated system compositor deliberately uses group access so
the differently privileged Shell can connect. Root-owned systemd units and
binaries are the only way to enter either account.

Future application runtimes will use per-app UIDs. They must not receive the
Wayland group; appd will provide a narrowly scoped connection only after the
runtime sandbox is established.

## Current implementation boundary

This increment provides the protocol, policy module, credential checks,
trusted layers, global action delivery, image integration and process-account
split. The existing Home screen remains the only trusted foreground surface.

The next increment will add a test application surface and exercise Shell
hide/show, focus restoration and crash return. Permission prompts will then
use an alpha-capable overlay state, followed by malicious-client and screenshot
regression tests.

## V0.6 hardware validation

The module and updated Shell were built as AArch64 ELF binaries against the
pinned Weston 14.0.2 build with warnings treated as errors. The module exports
`wet_module_init` and has no runtime dependency outside the installed Weston,
Wayland server and libc set.

The Phase 2B files were hot-deployed to the flashed V0.6 image. Weston loaded
the module and logged both `trusted uid=988 policy active` and `trusted System
Shell registered`. The compositor used 7.4 MiB and the Shell used 780 KiB in
their systemd cgroups. Both services remained active after the negative tests.
Camera inspection confirmed the 320x170 Home screen on the physical LCD.

The process boundary was tested in three layers:

- ordinary `pi` could not traverse `/run/cardputerzero`;
- `cp0-shell` could not read the DRM or input device aliases;
- a temporary client using the wrong UID but forced into `cp0-wayland` reached
  the socket, then received `cardputerzero system shell protocol is restricted`
  and was disconnected when it bound the private protocol.

A forced Shell `SIGKILL` changed its PID from 2731 to 2798 with `NRestarts=1`.
The compositor stayed active, the policy accepted the replacement trusted
surface, and camera inspection confirmed that Home returned. Physical Fn-layer
Home/Tasks/Power mapping and two-client focus restoration remain pending.
