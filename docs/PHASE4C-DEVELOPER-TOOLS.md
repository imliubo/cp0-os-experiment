# Phase 4C: simulator, deployment and logs

## PC run loop

`cp0ctl run` builds only the SDK application and executes its WASM module with
the bundled Node WebAssembly simulator:

```sh
cp0ctl run ./my-app \
  --duration 1500 \
  --permissions allow \
  --keys left,enter,c \
  --output frame.ppm \
  --profile profile.json
```

The application runs in a worker so an infinite device event loop cannot block
the simulator controller. The controller terminates it at the bounded duration,
writes the last complete RGB565 frame as a portable PPM image and emits a JSON
profile containing WASM size, linear-memory pages, frame count, key count,
notification records, total host calls and per-capability call counts.

The simulated surface is 320x150 for standard applications and 320x170 for
immersive applications. Scripted input names map to the same Linux evdev codes
delivered by the real Runtime. Permission mode is deliberately binary and
deterministic: `deny` rejects every sensitive operation, while `allow` permits
only capabilities that are also declared in the manifest. Network, documents,
audio, camera, GPIO and storage use bounded deterministic fixtures; the
simulator never grants ambient host filesystem or socket access to WASM.

The simulator is a development aid, not a security boundary. Device admission
still depends on `.capp` signatures and appd sandbox enforcement.

## Device install and logs

A local maintenance install is still available to root:

```sh
sudo cp0ctl install my-app-store.capp
sudo cp0ctl logs dev.example.my-app 50
```

For the normal SDK workflow, enable Developer Mode on the device, open the
ten-minute **Pair New Computer** window and register a developer signing key
with an Ed25519 SSH key:

```sh
cp0ctl pair developer.pub ~/.ssh/cardputerzero_ed25519.pub workstation \
  --device OWNER@DEVICE_IP
cp0ctl install my-app.developer.capp --device OWNER@DEVICE_IP
cp0ctl logs dev.example.my-app 50 --device OWNER@DEVICE_IP
```

Remote install verifies the developer signature before transmission and streams
the bounded package body through `ssh -T` and `cp0-dev`. The forced-command key
cannot run a shell, and the transport contains no `scp`, sudo or generic upload
fallback. `cp0-devd` checks Developer Mode and policy on every request, verifies
that the package signing key remains paired, stages it in a root-only runtime
directory and invokes the same appd admission path.

Logs are resolved by appd from the root-owned registry to the stable systemd
unit. The caller cannot supply an arbitrary unit. The result is limited to 100
lines, 256 characters per line and a bounded JSON frame, with control characters
removed. Pairing and revocation are described in `DEVELOPER-ACCESS.md`.

`tests/test-simulator.sh` builds and runs Hello Card with allowed camera, GPIO
and storage input, then checks the PPM header and profile counters. CLI tests
also reject SSH option injection, whitespace and shell metacharacters in device
targets.
