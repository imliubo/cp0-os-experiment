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

## DevKit and AI Skill

The recommended distribution is a versioned host-native DevKit archive or the
full toolchain image described in `docs/APP-DEVKIT-DISTRIBUTION.md`. Verify the
release checksum before extraction, then set the root and run its doctor:

```sh
export CP0_DEVKIT_ROOT=/path/to/cardputerzero-app-devkit-1.0.0-HOST
export PATH="$CP0_DEVKIT_ROOT/bin:$PATH"
"$CP0_DEVKIT_ROOT/skills/cardputerzero-build-app/scripts/doctor.sh" \
  "$CP0_DEVKIT_ROOT" rust
```

The bundled `$cardputerzero-build-app` Skill gives an AI agent the platform
contract, project workflow, permission boundaries, deterministic verifier and
failure routing needed to complete an application without private Runtime or
Linux APIs. Keep the Skill, SDK, simulator and `cp0ctl` from the same DevKit.

## Create and build

With the released DevKit, create and build directly with `cp0ctl`:

```sh
cp0ctl new /tmp/my-clock dev.example.clock "Clock"
cp0ctl build /tmp/my-clock
```

In an OS source checkout, install the pinned Rust wasm target and use the
workspace tool:

```sh
rustup target add --toolchain 1.85.1 wasm32-unknown-unknown
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

The simulator records submitted frames, capability calls, input count, private
storage bytes/keys, linear memory and timing in the JSON profile. Its private
storage fixture enforces the manifest byte quota, 256-key limit and missing-key
semantics. It is a deterministic SDK test harness, not a security substitute
for the device namespace, seccomp and cgroup tests.

`examples/neon-snake` is a complete allocation-free game example using the
display, focused keyboard input, monotonic clock and isolated private storage.
It includes simulator commands and physical-key controls in its README.

## Package, sign and install

Generate a developer key once, then create and sign a reproducible `.capp`:

```sh
cargo run -p cp0ctl -- key generate developer.key developer.pub
cargo run -p cp0ctl -- package /tmp/my-clock /tmp/my-clock.capp
cargo run -p cp0ctl -- sign developer /tmp/my-clock.capp \
  /tmp/my-clock.developer.capp developer.key
cargo run -p cp0ctl -- verify /tmp/my-clock.developer.capp
cargo run -p cp0ctl -- install /tmp/my-clock.developer.capp \
  --device pi@192.168.20.146
```

Device installation succeeds only when the developer key is trusted and the
device has explicitly enabled developer mode. The mode can be inspected with
`sudo cp0ctl device status` and changed locally with `sudo cp0ctl device developer
on|off`, unless parent or organization policy locks it. Store distribution adds
an independent store review signature. Trust and device policy configuration
is root-owned and is never writable by an application.

Store submission uses the developer-signed package, not an unsigned build and
not a package that already has a store signature. Review metadata must bind the
exact submission SHA-256, declared permissions and inspected WASM imports. The
developer first validates the package and Store resources locally:

```sh
cargo run -p cp0ctl -- store validate \
  dev.cardputerzero.example-1.0.0.signed.capp store/listing.json
```

This rejects identity mismatches, invalid developer signatures, pre-existing
Store signatures, unsafe or symbolic-link asset paths, size/digest mismatches
and malformed PNG dimensions. After the App ID is registered in Developer
Portal, submit the same immutable inputs with:

```sh
cargo run -p cp0ctl -- store submit \
  dev.cardputerzero.example-1.0.0.signed.capp store/listing.json
```

The CLI uses OAuth Device Flow, bounded resumable chunks and in-memory tokens.
Its final stdout value is machine-readable JSON with the Submission ID and
Portal URL. The current manual review/publishing operator then runs:

```sh
cargo run -p cp0ctl -- store publish \
  submissions reviews public-store https://store.example.invalid \
  42 1800000000 1800600000 store.key
```

This creates a new static HTTPS tree containing store-signed packages, a signed
catalog and `store.pub`. The output directory must not already exist. Developers
cannot self-approve a package by adding a store signature; device trust keys and
the review signing key are controlled independently. See
`docs/PHASE5B-APPLICATION-STORE.md` for the review schema and device trust
boundary.

On a provisioned test device, operators can exercise the fixed Store control
surface with `sudo cp0ctl store list`, `sudo cp0ctl store search <query>
[offset limit]`, `sudo cp0ctl store refresh` and `sudo cp0ctl store install
<app-id>`. Search is local, bounded to eight results per page and does not send
the query to an origin. The device chooses the catalog URL, package URL,
expected identity, size and hash from root-owned configuration and the verified
catalog; none can be supplied through these commands.

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
