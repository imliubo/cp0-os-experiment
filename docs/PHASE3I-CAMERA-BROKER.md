# Phase 3I: Restricted camera broker

## Scope

The first camera API captures one fixed frame without exposing V4L2, Media
Controller, dma-heap, VideoCore devices, native descriptors or capture process
arguments to a WASM application. Its complete contract is:

- permission: `camera.capture`;
- dimensions: 320x170;
- format: RGB565 little endian;
- result: exactly 108800 bytes in caller-owned WASM memory;
- operation: synchronous, one frame, four-second service deadline.

There is no sensor selection, preview stream, arbitrary resolution, codec,
container or output path in SDK 0.1. A future streaming API must use a separate
permission and bounded shared-buffer protocol rather than expanding this call.

## Trust Flow

```text
WASM camera SDK call
  -> Runtime validates one exact 108800-byte linear-memory range
  -> appd derives identity from SO_PEERCRED and active systemd cgroup
  -> appd checks the root-owned manifest and camera.capture decision
  -> root-only cp0-camerad socket accepts only appd
  -> cp0-camerad invokes fixed /usr/bin/rpicam-still arguments
  -> exact RGB888 output is converted to RGB565_LE
  -> sealed memfd is reopened read-only and sent with SCM_RIGHTS
  -> appd verifies type, length, access mode and all write seals
  -> Runtime repeats metadata/seal checks and copies pixels into WASM memory
```

`cp0-camerad` runs as `cp0-camera` with only the `video` supplementary group,
empty capability sets, no network address family and no writable system or
home directory. `DevicePolicy=closed` grants only video4linux, media,
dma-heap and `/dev/vchiq`, which are required by the Raspberry Pi camera
pipeline. The application sandbox has none of those devices.

Both service protocols are strict newline-delimited JSON. The private camera
protocol is capped at 2048 bytes and permits one CLOEXEC descriptor only.
Captured data never enters JSON or appd heap memory: the broker passes a
read-only, immutable descriptor, and the Runtime performs the final bounded
copy. Missing, writable, unsealed, non-regular or incorrectly sized
descriptors fail closed.

## Image And Hardware Status

The app-platform image installs `rpicam-apps-lite`; the previous minimal image
removed it because no camera service existed. The fixed executable path and
argument vector are owned by the broker, not by an application or manifest.

Read-only inspection of the current V0.6 device found the BCM2835 codec and ISP
nodes but no connected Unicam sensor node. Therefore local protocol, sandbox,
cross-build and image tests can complete now, while physical capture, frame
orientation and permission-denial acceptance remain pending until a compatible
sensor is attached. This does not require interrupting the active 24-hour core
stability run.

## Verification

Automated coverage includes:

- strict request/response framing and exactly-one-FD transfer;
- exact RGB888 input size and RGB565 little-endian conversion;
- fixed metadata, timeout and capture failure mapping;
- read-only regular-file size and Linux seal verification in appd and Runtime;
- `camera.capture` manifest permission routing and denial behavior;
- Rust, C11, C++17 and WIT SDK surfaces;
- hardened systemd service, image package and installation assertions;
- AArch64 camerad/appd/Runtime and wasm32 Hello Card builds.

Hello Card binds `C` to capture and displays the top 320x150 portion under the
trusted status bar. Green, red, yellow and magenta status marks mean success,
denied, unavailable and internal failure respectively.
