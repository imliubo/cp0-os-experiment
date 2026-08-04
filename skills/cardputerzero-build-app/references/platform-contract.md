# CardputerZero App Platform Contract

## Display and execution

- Runtime: isolated WebAssembly under WAMR; no Linux/WASI compatibility layer.
- Standard surface: 320x150 RGB565 below the trusted 20-pixel status bar.
- Immersive surface: 320x170 RGB565; system overlays remain trusted and visible.
- Frame byte size: `width * height * 2`, little-endian RGB565.
- Maximum frame rate: 30 FPS. Prefer redraw-on-change for static applications.
- Foreground: exactly one app; only it receives keyboard events.
- Default app memory request: 16-24 MiB. CM0 has 512 MiB shared by the OS.

The Rust SDK renderer is `cp0_sdk::ui::Canvas`. Direct framebuffer, DRM,
Wayland and evdev access are forbidden even when a native API appears present
on a development machine.

## Tasks and lifecycle preview

The product remains single-foreground even as the task model evolves: only one
App is visible and focused. F3 belongs to the trusted System Shell. Background
tasks never receive focused keys and may be frozen or destroyed under CM0
memory pressure; exclusive camera, microphone and GPIO output leases are not a
background entitlement.

SDK 1.0 declares optional `cp0_app_checkpoint` and `cp0_app_restore` exports.
Checkpoint schema versions are App-owned nonzero `u32` values and payloads are
limited to 8 KiB. Apps without the exports remain valid and restart cleanly.
The current multitasking implementation is simulation-first: the PC App
simulator does not drive checkpoint/restore and production Runtime/compositor
integration is still gated on device work. Keep state recovery independently
correct and do not promise checkpoint persistence on a target image unless its
release notes explicitly enable it.

## Focused input

`input::poll_key_event` returns Linux evdev-compatible numeric codes through a
bounded SDK event, not an evdev file. Handle `pressed`; use `repeated` only when
the interaction intentionally supports key repeat.

| Key | Code | Simulator name |
| --- | ---: | --- |
| Escape | 1 | `esc` |
| Backspace | 14 | `backspace` |
| Enter | 28 | `enter` |
| Space | 57 | `space` |
| Up | 103 | `up` |
| Left | 105 | `left` |
| Right | 106 | `right` |
| Down | 108 | `down` |
| F1-F4 | 59-62 | `f1`-`f4` |

F1-F4 may be intercepted as global System Shell actions on the device. Do not
make them the only route to an app feature. Letter and number codes follow the
closed simulator map in `simulator/cp0-simulator.mjs`.

## Global media actions

Media Play/Pause, Previous and Next are trusted System Shell actions (`Fn+Q`,
`Fn+W` and `Fn+E` on V0.6), not focused key events. Register only playback
state and a supported-action mask with `media::update_session`, then consume
each routed action once with `media::take_action`. The call contains no App ID,
title, artwork, path or target; appd binds it to the authenticated foreground
Runtime. Registering an inactive session requires an empty mask; paused or
playing requires at least one supported action.

The simulator accepts `--media-actions play-pause,previous,next`. This fixture
tests registration and action handling without granting `audio.playback`.

## Manifest

`app.json` schema version 1 requires a stable reverse-domain ID, display name,
semantic version, SDK `1.0`, WAMR runtime, canonical `bin/*.wasm` entrypoint,
display mode, memory/storage limits, permissions and intents. Validate through
`cp0ctl manifest validate`; do not hand-roll equivalent validation.

Use exactly one permission entry per capability used by the app:

| Capability | Public SDK | Limit or boundary |
| --- | --- | --- |
| `notifications.post` | `system` | 32-char title, 160-char body |
| `network.client` | `network` | HTTPS GET, public destinations, 2048-byte body |
| `documents.open` | `documents` | portal-selected read-only descriptor |
| `audio.playback` | `audio` | 16 kHz mono S16_LE, 1024 frames/call |
| `audio.capture` | `audio` | separate capture grant, same fixed format |
| `camera.capture` | `camera` | one fixed 320x170 RGB565 frame |
| `hardware.gpio` | `gpio` | four logical V0.6 connector lines only |
| `radio.lora` | `radio` | fixed external SX1276 policy; disabled by default |
| `photos.read` | `photos` | read the bounded shared photo library |
| `photos.write` | `photos` | save or explicitly delete shared photos |

Private `storage` is always identity-bound and limited by
`resources.storage_mb`; it has no permission name. Intents must be explicitly
declared and use reverse-domain actions. Apps cannot select another app by ID.

The `media` module is targetless coordination state and has no permission name.
It never substitutes for `audio.playback`.

## Isolation rules

Never add host paths, network addresses, device nodes, shell text, UIDs, socket
paths or secrets to application code as an escape hatch. Capability brokers
bind requests to the calling app identity and permission decision. A denial is
part of the product behavior and must have a usable UI state.

## Device readiness

A product device exposes no fixed human account or password. First-boot Setup
creates the owner and chooses networking. Setup blocks third-party App
activation until its durable commit completes. Developer Mode and Owner SSH
Shell are independent settings: Developer Mode starts only the constrained
signed-App transport, while Owner SSH Shell grants an owner Bash session with
no sudo. Treat the visible IP and owner-selected username as device state, not
constants to embed in source, scripts or Skill output.
