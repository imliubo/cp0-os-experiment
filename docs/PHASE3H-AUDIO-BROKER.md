# Phase 3H: Restricted audio broker

## Scope

The audio API provides bounded synchronous PCM playback and capture operations
without exposing ALSA, mixer controls or device selection to a WASM
application. The device contract is fixed to the CardputerZero V0.6 ES8389:

- ALSA endpoint: `hw:ES8389Audio,0`;
- format: signed 16-bit little-endian PCM;
- legacy/general audio: 16 kHz mono, at most 1024 frames, 2048 bytes or 64 ms;
- SDK 1.1 music playback: 48 kHz stereo, at most 1920 frames, 7680 bytes or
  40 ms per call;
- permissions: `audio.playback` and `audio.capture` are independent.

The two exact formats avoid negotiation, codecs and container parsers in the
trusted path. Longer sounds and recordings are built from repeated bounded
calls. Raw ALSA ioctls, mixer changes, arbitrary sample formats and HDMI audio
are not SDK features. PCM WAV parsing remains in the sandboxed Music App;
future compressed formats belong in a separate bounded system decoder.

The V0.6 ES8389 hardware PCM endpoint runs at 48 kHz with exactly two
interleaved channels. Audiod keeps one playback handle open, sends SDK 1.1 music
frames directly, and preserves the mono SDK contract by repeating each 16 kHz
sample three times and duplicating it to both hardware channels. Capture remains
16 kHz stereo at the hardware boundary and averages left/right samples back to
one mono sample. Applications cannot select or observe the hardware layout.
The capture stream is explicitly started before its first read because the
V0.6 driver returns `EIO` instead of performing ALSA's usual implicit start.

The trusted System Shell separately uses audio protocol v3 output-setting and
key-sound commands.
They are fixed to the ES8389 `DACL`, `DACR` and `Speaker` simple mixer elements;
no card, element or route name is accepted from a request. Volume is bounded to
0 through 100 percent, shortcut adjustments use a fixed 10-percent step, and
every write returns observed volume and mute state. The persisted Key Sounds
setting is owned by audiod so both Shell and foreground App key events follow
one policy. Password and Wi-Fi secret entry stay silent.

The key cue is a fixed 32 ms, 16 kHz mono PCM derivative of UI SFX Soft
`typing` (CC0-1.0). In service mode, `play-key-click` places one token into a
bounded eight-entry queue and returns before touching ALSA. A dedicated
128 KiB-stack worker drains every accepted token in order. A full queue applies
backpressure instead of dropping key feedback. Tokens reached while Key Sounds
is disabled are discarded before hardware access.

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
system view, and one private state directory for the Key Sounds boolean.

The service dynamically resolves the small `libasound.so.2` PCM API, so the
cross-compiled Rust binary does not depend on host ALSA headers or a link-time
sysroot library. It selects the named ES8389 card rather than ALSA card number
zero, preventing an HDMI enumeration change from redirecting application
audio.

## Protocol And Failure Behavior

The Runtime-to-appd and appd-to-audiod protocols are strict newline-delimited
JSON frames capped at 16 KiB and 12 KiB respectively. Binary samples use
canonical base64. Every layer rejects empty, misaligned, oversized or
noncanonical data, capture frame counts outside 1 through 1024, music requests
outside 1 through 1920 stereo frames, mismatched request IDs and mismatched
returned lengths.

Busy devices map to the stable SDK resource-limit result. Missing ALSA support,
device failures and a pending permission prompt map to unavailable. Denied or
undeclared permissions map to denied. Native ALSA error strings and host device
details never cross into the application.

The audiod socket is traversable only by `cp0-audio-control`. `cp0-audiod`
then uses `SO_PEERCRED` to authorize root/appd only for PCM, capture, and
foreground-App key clicks, while `cp0-shell` receives output settings, key-sound
policy, and Shell key clicks. The Shell still cannot submit arbitrary PCM and
appd still cannot change volume or the persistent Key Sounds setting. Socket
membership alone never grants a command.

## Verification

Automated coverage includes:

- protocol round trips, maximum frames and malformed canonical-base64 cases;
- fake-device mono/stereo playback, exact capture and invalid-length dispatch;
- appd-to-audiod request/response correlation;
- separate manifest permission routing in appd;
- Runtime capture decoding and length mismatch rejection;
- cached 48 kHz playback, 16-to-48 kHz conversion, key-click enable/disable,
  persistence across audiod restart, rapid-click queue draining without loss,
  and no hardware access while disabled;
- Rust, C11, C++17 and WIT SDK surfaces;
- hardened systemd service and image-stage assertions;
- AArch64 audiod/appd and static Runtime plus wasm32 Hello Card builds.

The device reports ES8389 playback and capture as `card 0, device 0`; HDMI is a
separate playback-only card. The real-identity allow/deny probe and result
harness are documented in `PHASE3M-DEVICE-CAPABILITY-ACCEPTANCE.md`. SDK 1.1
Music, uninterrupted key-click behavior, and latency/underrun acceptance still
require a product bundle or image and physical V0.6 verification.
