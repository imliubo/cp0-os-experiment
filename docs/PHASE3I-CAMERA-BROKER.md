# Phase 3I: Restricted camera broker

## Scope

The camera APIs do not expose V4L2, Media Controller, dma-heap, VideoCore
devices, native descriptors or capture process arguments to a WASM
application. Their fixed contracts are:

- permission: `camera.capture`;
- foreground preview: 320x170 RGB565 little endian at a 30 FPS target, rotated
  180 degrees for the V0.6 sensor mounting;
- photo capture: 1280x720 JPEG plus a 320x170 RGB565 Gallery thumbnail;
- preview result: exactly 108800 bytes in caller-owned WASM memory;
- photo result: one broker-owned `photo_id`; the JPEG never enters WASM memory.

There is no sensor selection, arbitrary resolution, codec, container or output
path in SDK 1.0. `camera::capture_photo()` also requires `photos.write`, encodes
the next frame from the same live pipeline, and atomically stores the original
and thumbnail without stopping or restarting the sensor.

## Trust Flow

```text
WASM camera SDK call
  -> Runtime validates one exact 108800-byte linear-memory range
  -> appd derives identity from SO_PEERCRED and active systemd cgroup
  -> appd checks the root-owned manifest and camera.capture decision
  -> appd requires the caller to be the System Shell's current foreground runtime
  -> root-only cp0-camerad socket accepts only appd
  -> cp0-camerad keeps one fixed 1280x720 @ 30 FPS /usr/bin/rpicam-vid YUV420 stream
  -> preview frames are downscaled to 320x170 RGB565_LE at a 30 FPS target
  -> photo requests send the next frame to the fixed /dev/video31 JPEG encoder
  -> a bounded planar-YUV software encoder remains available as a fail-safe
  -> sealed memfd is reopened read-only and sent with SCM_RIGHTS
  -> appd verifies type, length, access mode and all write seals
  -> Runtime repeats metadata/seal checks and copies pixels into WASM memory
```

`cp0-camerad` runs as `cp0-camera` with only the `video` supplementary group,
empty capability sets, no network address family and no writable system or
home directory. `DevicePolicy=closed` grants only video4linux, media,
dma-heap and `/dev/vchiq`, which are required by the Raspberry Pi camera
pipeline. The application sandbox has none of those devices. The service and
its camera children run with `Nice=10` and `CPUWeight=10` so camera work
yields to the compositor and keyboard path when the constrained CM0 is busy.

Both service protocols are strict newline-delimited JSON. The private camera
protocol is capped at 2048 bytes and permits one CLOEXEC descriptor only.
Preview data never enters JSON: the broker passes a read-only, immutable
descriptor, and the Runtime performs the final bounded copy. A still photo is
returned to appd as one bounded descriptor containing a fixed thumbnail and at
most 4 MiB of JPEG data. appd validates the envelope and commits it directly to
the system photo library. Missing, writable, unsealed, non-regular or
incorrectly sized descriptors fail closed.

## Image And Hardware Status

The app-platform image installs `rpicam-apps-lite`; the previous minimal image
removed it because no camera service existed. The fixed executable path and
argument vector are owned by the broker, not by an application or manifest.
The continuous YUV420 process uses a 40 FPS internal sensor target so protocol
transfer and downscaling can sustain the public 30 FPS preview contract. Its
process and camera pipeline are reused between preview and photo requests and
released after two seconds without a request, so a frozen/background Camera
task does not retain the sensor indefinitely. Both preview and JPEG quality-90
photo capture use the same fixed 1280x720, 180-degree-rotated frame. Avoiding a
second `rpicam-still` process removes sensor discovery and mode-switch latency.
Process creation runs on a short-lived backend thread and each foreground
request waits for at most 50 ms of frame progress. During a cold start,
camerad preserves both the child process and any partial YUV frame between
retries for up to 20 seconds. After the first complete frame, 500 ms without a
complete frame discards and rebuilds the child. Slow libcamera discovery can
therefore finish without blocking Camera's input loop for the full discovery
deadline or the process creation delay.
The V0.6 hardware JPEG path accepts the stream's planar YUV420 frame directly,
so capture does not allocate or convert a full RGB888 image. If that fixed
kernel encoder is unavailable, the software fallback also reads the planar
YUV buffers directly instead of constructing an RGB intermediate. Exposure
quality remains part of physical acceptance.

The current V0.6 image detects the IMX219 through the Unicam pipeline. Physical
preview throughput, frame layout/orientation, foreground revocation and input
latency still require coordinated device acceptance after each camera broker
change.

## Verification

Automated coverage includes:

- strict request/response framing and exactly-one-FD transfer;
- exact 1280x720 YUV420 input size, RGB565 downscaling and JPEG encoding;
- bounded 1280x720 JPEG envelope and original-plus-thumbnail transaction;
- foreground-runtime camera revocation and idle pipeline release;
- fixed metadata, timeout and capture failure mapping;
- read-only regular-file size and Linux seal verification in appd and Runtime;
- `camera.capture` manifest permission routing and denial behavior;
- Rust, C11, C++17 and WIT SDK surfaces;
- hardened systemd service, image package and installation assertions;
- AArch64 camerad/appd/Runtime and wasm32 Hello Card builds.

The V0.6 device benchmark for the hardware path is a 23.6 ms warm preview
request followed by a 49.7 ms 1280x720 JPEG broker request. This excludes the
incremental sensor/libcamera cold start and the subsequent appd photo-library
transaction; pressing the shutter on an already-live Camera does not pay that
cold-start cost.

Hello Card binds `C` to capture and displays the top 320x150 portion under the
trusted status bar. Green, red, yellow and magenta status marks mean success,
denied, unavailable and internal failure respectively.
