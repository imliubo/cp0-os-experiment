# Phase 2D: Trusted overlays and display policy

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE2D-TRUSTED-OVERLAYS.zh-CN.md)

## Compositor contract

Version 3 of `cp0_system_shell_v1` makes the display policy explicit. Only the
kernel-authenticated `cp0-shell` client can select one of these modes:

- `full`: the trusted Shell is opaque, applications are hidden and keyboard
  focus belongs to the Shell;
- `status`: a standard application remains focused below a trusted 21-pixel
  status strip;
- `hidden`: an immersive application owns all 320x170 pixels while the trusted
  Shell surface is not rendered.

The App Runtime supplies `cardputerzero:standard` or
`cardputerzero:immersive` as a trusted launch-contract title. This title only
selects rendering policy. Application identity still comes from the
root-controlled appd manifest and per-app Unix account.

The compositor keeps application and trusted views in separate Weston layers.
It rejects status or hidden mode when no application is active, restores full
mode when the active surface disappears, and always processes Home, Back,
Tasks and Power before application keyboard delivery. Applications therefore
cannot paint over a permission decision or retain focus beneath one.

## System Shell

The Shell uses double-buffered ARGB8888 SHM buffers. In status mode pixels
below row 20 are transparent and the Wayland input region is limited to the
status strip. In notification mode only the status strip and the exact trusted
banner or system-action rectangle are opaque; the rest of the App remains
visible and retained Shell pages cannot become the overlay backdrop. In hidden
mode the input region is empty. Full mode covers the display and receives
keyboard focus.

Permission prompts are read from appd over the authenticated control socket.
The Shell uses a bounded 8 KiB frame, a 128-token JSON parser and 250 ms socket
timeouts. It displays only canonical manifest data and offers ONCE, ALWAYS and
DENY. A prompt resolved by another trusted controller is removed by the next
one-second poll. The diagnostic command

```sh
cp0ctl permission reset <app-id> <capability>
```

atomically removes a persistent decision and restores first-use prompting.

The Power dialog now sends a compositor sleep request. Weston powers down its
outputs and owns input wakeup; its wake signal restores the trusted Home view
before returning focus.

## Verification

Host tests cover protocol/profile invariants, ARGB UI rendering, permission
dialog pixels, JSON escaping and Unicode decoding, malformed input, token
limits and integer overflow. The compositor, Shell, Runtime, appd and cp0ctl
were also cross-compiled for AArch64 before deployment.

The complete stage was hot-deployed to the 512 MB V0.6 device without flashing
or rebooting. Hardware verification established that:

- standard Hello content remained below the trusted status strip;
- an unresolved `notifications.post` request displayed its canonical app name,
  permission, reason and three decisions on the physical LCD;
- an external one-time decision removed the prompt, allowed the waiting WASM
  host call and produced the expected notification;
- stopping the application restored Home while compositor and Shell remained
  active;
- physical F1, F2, F3 and F4 invoked Home, Back, Tasks and Power through the
  compositor-owned global bindings.

Image flashing is not required for this stage. The same sources and service
configuration are included by the next full `pi-gen` image build.
