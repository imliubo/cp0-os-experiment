# CardputerZero application developer guide

## Application model

CardputerZero applications are WebAssembly modules built against the platform
SDK. They do not receive Linux paths, sockets, device nodes or shell access.
Each installed application has a stable UID, a private storage quota and an
explicit permission set. Only one application is foreground at a time, and
only that application receives keyboard input or owns the application surface.

The supported Rust target is `wasm32-unknown-unknown`. C and C++ applications
use a Clang-compatible freestanding wasm32 toolchain and
`sdk/c/include/cardputerzero.h`. Traditional Raspberry Pi desktop or Linux
applications are intentionally not compatible.

## Create and build

Install the Rust wasm target, then create a project with the repository tool:

```sh
rustup target add wasm32-unknown-unknown
cargo run -p cp0ctl -- new /tmp/my-clock dev.example.clock "Clock"
cargo run -p cp0ctl -- build /tmp/my-clock
```

The generated `app.json` declares the application identity, SDK requirement,
display mode, memory and private-storage limits, permissions and intents.
`cp0ctl build` validates this manifest and stages a deterministic package tree
under `target/cardputerzero/<app-id>/<version>`.

Use standard display mode for a 320x150 application surface below the trusted
status bar. Immersive mode uses 320x170, but System Shell permission prompts,
notifications and global actions remain trusted compositor overlays.

## Event loop and UI

Keep one caller-owned RGB565 frame and redraw only after state changes. Poll
focused keyboard events with a bounded timeout so lifecycle and broker work can
continue. `cp0_sdk::ui::Canvas` is the minimal allocation-free renderer. The
optional LVGL 9 adapter in `sdk/lvgl` offers a larger widget toolkit through the
same public SDK ABI.

Neither UI path grants direct framebuffer, DRM, Wayland or evdev access. A
frame submission and every input event pass through App Runtime.

## Permissions

Declare only capabilities the application uses, with a short reason visible in
the trusted permission prompt. Examples include `network.client`,
`audio.playback`, `audio.capture`, `camera.capture`, `hardware.gpio`,
`radio.lora`, `documents.open`, `notifications.post` and intent declarations.
Private key/value storage is automatically available within the manifest's
`resources.storage_mb` quota and has no separate permission name. A denied
capability returns `Error::Denied`; a pending decision or temporarily
unavailable service returns `Error::Unavailable` and may be retried after
returning to the event loop.

The PC simulator never grants ambient host access. `--permissions allow` uses
deterministic capability fixtures, while `--permissions deny` verifies the
application's denial path.

## Simulate and profile

```sh
cargo run -p cp0ctl -- run examples/calculator \
  --keys 1,2,plus,3,equal --output /tmp/calculator.ppm \
  --profile /tmp/calculator.json

cargo run -p cp0ctl -- run examples/camera \
  --permissions allow --keys enter --output /tmp/camera.ppm
```

The simulator records submitted frames, capability calls, input count, linear
memory and timing in the JSON profile. It is a deterministic SDK test harness,
not a security substitute for the device namespace, seccomp and cgroup tests.

## Package, sign and install

Generate a developer key once, then create and sign a reproducible `.capp`:

```sh
cargo run -p cp0ctl -- key generate developer.key developer.pub
cargo run -p cp0ctl -- package /tmp/my-clock /tmp/my-clock.capp
cargo run -p cp0ctl -- sign /tmp/my-clock.capp developer.key
cargo run -p cp0ctl -- verify /tmp/my-clock.capp
cargo run -p cp0ctl -- install /tmp/my-clock.capp \
  --device pi@192.168.20.146
```

Device installation succeeds only when the developer key is trusted or the
device has explicitly enabled developer mode. Store distribution adds an
independent store review signature. Trust configuration is root-owned and is
never writable by an application.

Application logs are bounded and root-mediated:

```sh
cargo run -p cp0ctl -- logs dev.example.clock --device pi@192.168.20.146
```

## Compatibility

The current manifest SDK requirement is `1.0`, backed by WIT package
`cardputerzero:sdk@1.0.0`. The device rejects unknown majors and accepts an
application minor no newer than its own within the same major. It also accepts
exactly legacy SDK `0.1`; arbitrary `0.x` versions are not compatible. Public
WIT describes the typed source contract;
`sdk/abi/cardputerzero-hostcalls-v1.json` is the canonical flat WAMR import
contract and generated SDK bindings must match it exactly.
