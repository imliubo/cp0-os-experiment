# Phase 3H: Restricted audio broker

## Scope

The first audio API provides short, synchronous PCM playback and capture
operations without exposing ALSA, mixer controls or device selection to a WASM
application. The device contract is fixed to the CardputerZero V0.6 ES8389:

- ALSA endpoint: `hw:ES8389Audio,0`;
- format: signed 16-bit little-endian PCM;
- rate and channels: 16 kHz mono;
- maximum call: 1024 frames, 2048 bytes or 64 ms;
- permissions: `audio.playback` and `audio.capture` are independent.

The narrow format avoids resampler, codec and container parsers in the trusted
path. Longer sounds and recordings are built from repeated bounded calls. Raw
ALSA ioctls, mixer changes, arbitrary sample formats and HDMI audio are not SDK
features.

## Trust Flow

```text
WASM audio SDK call
  -> Runtime validates one linear-memory pointer/length pair
  -> Runtime sends bounded base64 PCM over its only Unix broker socket
  -> appd derives identity from SO_PEERCRED
  -> appd verifies active systemd cgroup and root-owned manifest
  -> permission engine evaluates playback or capture independently
  -> appd forwards to root-only cp0-audiod socket
  -> cp0-audiod validates the protocol again and accesses ES8389 through ALSA
```

`cp0-audiod` runs as `cp0-audio` with only the standard `audio` supplementary
group. Its capability sets are empty. systemd uses `DevicePolicy=closed` and
grants only `char-alsa rw`; all other device nodes remain denied. The service
has only `AF_UNIX`, a 16 MiB memory limit, no swap, eight tasks, a read-only
system view and no writable home.

The service dynamically resolves the small `libasound.so.2` PCM API, so the
cross-compiled Rust binary does not depend on host ALSA headers or a link-time
sysroot library. It selects the named ES8389 card rather than ALSA card number
zero, preventing an HDMI enumeration change from redirecting application
audio.

## Protocol And Failure Behavior

The Runtime-to-appd and appd-to-audiod protocols are strict newline-delimited
JSON frames capped at 4096 bytes. Binary samples use canonical base64. Every
layer rejects empty, odd-length, oversized or noncanonical data, capture frame
counts outside 1 through 1024, mismatched request IDs and mismatched returned
lengths.

Busy devices map to the stable SDK resource-limit result. Missing ALSA support,
device failures and a pending permission prompt map to unavailable. Denied or
undeclared permissions map to denied. Native ALSA error strings and host device
details never cross into the application.

## Verification

Automated coverage includes:

- protocol round trips, maximum frames and malformed canonical-base64 cases;
- fake-device playback, exact capture and invalid capture-length dispatch;
- appd-to-audiod request/response correlation;
- separate manifest permission routing in appd;
- Runtime capture decoding and length mismatch rejection;
- Rust, C11, C++17 and WIT SDK surfaces;
- hardened systemd service and image-stage assertions;
- AArch64 audiod/appd and static Runtime plus wasm32 Hello Card builds.

The device reports ES8389 playback and capture as `card 0, device 0`; HDMI is a
separate playback-only card. The real-identity allow/deny probe and result
harness are documented in `PHASE3M-DEVICE-CAPABILITY-ACCEPTANCE.md`. Execution
and audible/microphone observation remain deferred until the active Phase 2
24-hour stability monitor finishes, so the baseline process set is unchanged.
