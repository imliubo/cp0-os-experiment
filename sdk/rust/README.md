# CardputerZero Rust SDK 0.1

This `no_std` crate is the supported Rust API for CardputerZero applications.
Applications compile for `wasm32-unknown-unknown` and must not declare private
Runtime imports directly.

The initial API exposes display and focused input, a monotonic clock, bounded
event waiting, notifications, documents, restricted HTTPS GET and fixed-format
PCM audio capabilities.
`network::http_get` accepts a caller-owned buffer of at most 2048 bytes and
returns only the HTTP status and body length. `Error::Unavailable` means a
capability may be waiting for a System Shell permission decision or a transient
service may be unavailable and can be retried later.

`audio::play_pcm_s16le` and `audio::capture_pcm_s16le` accept at most 1024
signed 16-bit mono frames at 16 kHz. The application manifest must declare
playback and capture separately.
