# CardputerZero Rust SDK 0.1

This `no_std` crate is the supported Rust API for CardputerZero applications.
Applications compile for `wasm32-unknown-unknown` and must not declare private
Runtime imports directly.

The initial API exposes display and focused input, a monotonic clock, bounded
event waiting, notifications, documents, restricted HTTPS GET, fixed-format
PCM audio and fixed-frame camera capabilities.
`network::http_get` accepts a caller-owned buffer of at most 2048 bytes and
returns only the HTTP status and body length. `Error::Unavailable` means a
capability may be waiting for a System Shell permission decision or a transient
service may be unavailable and can be retried later.

`audio::play_pcm_s16le` and `audio::capture_pcm_s16le` accept at most 1024
signed 16-bit mono frames at 16 kHz. The application manifest must declare
playback and capture separately.

`camera::capture_rgb565` always fills one caller-owned 320x170 RGB565 frame.
The application manifest must declare `camera.capture`; the SDK exposes no
sensor selection, camera device, capture process or file path.

`gpio::read` and `gpio::write` accept only the four `gpio::Line` variants
defined for the V0.6 connector functions. They expose booleans rather than
Linux gpiochip numbers, device paths, pin direction or pinmux configuration.
