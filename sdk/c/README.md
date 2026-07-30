# CardputerZero C/C++ SDK 0.1

Include `include/cardputerzero.h` from a freestanding Clang C11 or C++17
project targeting `wasm32-unknown-unknown`. The header declares only the public
CardputerZero Runtime imports; it does not expose WASI, Linux syscalls or native
linking.

Strings are UTF-8 byte buffers with explicit lengths. Applications should keep
notification titles at 32 Unicode characters and bodies at 160; the Runtime and
broker enforce byte, encoding and character limits again across the trust
boundary.

`cp0_http_get` is the only network API. It accepts an HTTPS URL and a
caller-owned response buffer no larger than 2048 bytes, then returns a bounded
HTTP status/body-length record. The SDK intentionally exposes no POSIX socket,
DNS, TLS override or arbitrary-header API.

`cp0_audio_play` and `cp0_audio_capture` exchange caller-owned signed 16-bit
PCM buffers. The format is fixed to 16 kHz mono S16_LE and one call is limited
to 1024 frames. Playback and capture require separate manifest permissions; the
SDK exposes no ALSA device, mixer or format negotiation API.

`cp0_camera_capture` fills exactly one caller-owned 320x170 RGB565 frame. It
requires `camera.capture`; applications cannot select a sensor, access V4L2 or
receive a native descriptor.
