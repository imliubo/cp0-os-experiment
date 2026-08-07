# CardputerZero C/C++ SDK 1.1

Include `include/cardputerzero.h` from a freestanding Clang C11 or C++17
project targeting `wasm32-unknown-unknown`. The header declares only the public
CardputerZero Runtime imports; it does not expose WASI, Linux syscalls or native
linking.

The raw import declarations in `include/cardputerzero_imports.h` are generated
from `sdk/abi/cardputerzero-hostcalls-v1.json`. Applications include only
`cardputerzero.h`; direct use of the generated raw functions is unsupported.
`tests/test-sdk-abi.sh` guarantees that the C declarations, Rust imports and
Runtime registration table remain byte-for-byte synchronized with the contract.

`cp0_key_event_t.character` is the platform-translated printable ASCII byte;
zero means the event has no text. It follows the same held-Shift and V0.6 `Sym`
mapping as first boot and System Shell. Applications use this field for text
entry and keep `code` for navigation or game controls instead of maintaining a
private evdev-to-character table.

Apps that want resumable task eviction may export `cp0_app_checkpoint` and
`cp0_app_restore` with the declarations in `cardputerzero.h`. The Runtime owns
the temporary linear-memory buffers, copies at most 8 KiB, and rejects schema
version zero. The callbacks are optional; an App that omits them restarts
cleanly after capacity eviction.

Strings are UTF-8 byte buffers with explicit lengths. Applications should keep
notification titles at 32 Unicode characters and bodies at 160; the Runtime and
broker enforce byte, encoding and character limits again across the trust
boundary.

`cp0_http_get` accepts an HTTPS URL and a caller-owned response buffer no larger
than 2048 bytes. SDK 1.1 adds `cp0_http_get_range`, with an exact offset and an
8 KiB per-call/256 MiB per-resource bound for streaming. Both return only a
bounded HTTP status/body-length record. The SDK intentionally exposes no POSIX
socket, DNS, TLS override or arbitrary-header API.

`cp0_audio_play` and `cp0_audio_capture` exchange caller-owned signed 16-bit
PCM buffers. Their compatibility format is fixed to 16 kHz mono S16_LE and one
call is limited to 1024 frames. SDK 1.1 also provides
`cp0_audio_play_stereo_48khz` for at most 720 interleaved stereo frames per
call. Playback and capture require separate manifest permissions; the SDK
exposes no ALSA device, mixer, codec or format negotiation API.

`cp0_camera_capture` fills exactly one caller-owned 320x170 RGB565 preview
frame. `cp0_camera_capture_photo` returns the photo ID of a system-managed
1280x720 JPEG plus Gallery thumbnail. Preview requires `camera.capture`; still
capture also requires `photos.write`. Applications cannot select a sensor,
access V4L2 or receive a native descriptor.

`cp0_gpio_read` and `cp0_gpio_write` accept only `cp0_gpio_line_t`. The enum
contains four V0.6 logical connector outputs; it deliberately cannot represent
a BCM GPIO number, gpiochip, path, pin direction or pinmux mode.

`cp0_lora_send` and `cp0_lora_receive` exchange at most 64 bytes with the fixed
external SX1276 configuration. Applications cannot select SPI, frequency,
modulation or power. The image keeps the radio disabled until root supplies a
valid regional configuration.

`cp0_storage_put`, `cp0_storage_get` and `cp0_storage_delete` provide private
key/value storage. Keys are bounded to 64 safe ASCII bytes and values to 8 KiB;
the installed manifest's `storage_mb` field is enforced by the storage broker.
The Runtime does not expose a writable host filesystem.

`cp0_intent_send` and `cp0_intent_take` provide manifest-routed application
handoff. Actions are reverse-domain ASCII names up to 96 bytes, payloads are at
most 1024 bytes, and a taken message is consumed once. No target application,
socket or native IPC handle appears in the SDK.

`cp0_media_session_update` registers the foreground application's playback
state and supported Play/Pause, Previous and Next actions.
`cp0_media_take_action` consumes one action routed by appd. The application
cannot name another application or attach media metadata, and registration
does not replace the `audio.playback` permission required for sound output.
The `cp0_photos_*` calls expose the separately permissioned shared photo
library. Use `cp0_photos_import_rgb565` to atomically add one fixed frame and
`cp0_photos_remove` to remove one selected ID; applications do not update
Gallery indexes directly. `photos.read` and `photos.write` remain independent
permissions. The paginated format and migration contract are described in
`docs/PHOTO-LIBRARY-V2.md`.
`cp0_photos_load_view_rgb565` returns a fixed 320x170 Fit, half-resolution or
1:1 viewport of a Camera JPEG original. Pan coordinates are normalized from
`CP0_PHOTO_VIEW_PAN_MIN` to `CP0_PHOTO_VIEW_PAN_MAX`; original bytes and paths
remain broker-private.
