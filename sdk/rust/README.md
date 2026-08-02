# CardputerZero Rust SDK 1.0

This `no_std` crate is the supported Rust API for CardputerZero applications.
Applications compile for `wasm32-unknown-unknown` and must not declare private
Runtime imports directly.

The `lifecycle` module defines the optional multitasking checkpoint contract
and its 8 KiB bound. Apps opt in by exporting `cp0_app_checkpoint` and
`cp0_app_restore` with the stable core-WASM signatures; Apps without them remain
valid and restart cleanly after capacity eviction.

The SDK 1.0 API exposes display and focused input, a monotonic clock, bounded
event waiting, notifications, documents, restricted HTTPS GET, fixed-format
PCM audio, fixed-frame camera, logical GPIO, LoRa, private storage and intent
capabilities.
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

`storage::put`, `storage::get` and `storage::delete` provide private key/value
storage within the manifest's `storage_mb` quota. Storage isolation is
automatic and does not require a manifest permission.

`intents::send` routes a reverse-domain action and at most 1024 payload bytes
through appd. `intents::take` returns only the next message bound to the current
application and consumes it once. The sender cannot name a target application
or connect to another application's process.

`media::update_session` registers playback state and a bounded set of global
actions for the current foreground Runtime. `media::take_action` consumes one
Play/Pause, Previous or Next action. The API contains no target application or
media metadata and does not grant the separate `audio.playback` permission.

`ui::Canvas` is the allocation-free reference renderer for the 320-pixel-wide
display. It provides clipped RGB565 rectangles, a compact 5x7 font, buttons and
progress bars. Applications own their frame buffer and submit it through
`display::present_rgb565`; the SDK never creates a Linux window or exposes a
framebuffer device.

Build and run an SDK application on the PC with:

```sh
cargo run -p cp0ctl -- build examples/calculator
cargo run -p cp0ctl -- run examples/calculator --keys 1,2,plus,3,equal
```

See `docs/DEVELOPER-GUIDE.md` for the complete package, permission, simulator,
signing and device-install workflow.
