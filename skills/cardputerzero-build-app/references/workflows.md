# App Development Workflows

<!-- doc-locale: en -->
> **English** | [简体中文](workflows.zh-CN.md)

## Rust

Select `cp0ctl` from an extracted DevKit:

```sh
export CP0_DEVKIT_ROOT=/path/to/cardputerzero-app-devkit
export PATH="$CP0_DEVKIT_ROOT/bin:$PATH"
export RUSTUP_TOOLCHAIN=$(awk -F '"' '$1 ~ /^rust_version = / { print $2 }' \
  "$CP0_DEVKIT_ROOT/devkit/toolchain.toml")
```

Keep `RUSTUP_TOOLCHAIN` exported for `new`, `build`, `run`, `package` and host
tests. `doctor.sh` checks availability but cannot update its parent shell, and
the current `cp0ctl` inherits the active Cargo toolchain instead of overriding
it. A valid signature does not prove that the pinned compiler produced the
packaged WASM.

In a source checkout, replace `cp0ctl` below with
`cargo run --quiet -p cp0ctl --`.

```sh
cp0ctl new ./my-app dev.example.my-app "My App"
cp0ctl manifest validate ./my-app/app.json
cp0ctl build ./my-app
cp0ctl run ./my-app --duration 1000 --permissions deny \
  --keys left,right,enter --output ./my-app/frame.ppm \
  --profile ./my-app/profile.json
```

For a media App, use the separate trusted-action fixture:

```sh
cp0ctl run ./my-app --duration 1000 --permissions deny \
  --media-actions play-pause,previous,next \
  --output ./my-app/frame.ppm --profile ./my-app/profile.json
```

Rust apps are `#![no_std]` `cdylib` crates for `wasm32-unknown-unknown`. Keep the
generated panic handler and exported `main`. Use only public modules from
`cp0-sdk`; never declare private imports.

Keep runtime-only imports, the exported `main`, frame storage and panic handler
behind `#[cfg(not(test))]`. Put deterministic state transitions in ordinary
functions so `cargo test` can exercise them with the host test harness.

Optional lifecycle checkpoint exports are an advanced, simulation-first API.
Read `platform-contract.md` before adding them, keep payloads versioned and at
most 8 KiB, and retain a clean-start path. The bundled simulator does not yet
exercise checkpoint/restore.

For a shared-photo App, use `photos::LIST_PAGE_PHOTOS` and one fixed camera-size
pixel buffer. Test both permission modes; a deterministic save and read in one
simulator run can verify the complete brokered API. See `photos.md` for the
contract and failure states.

## C and C++

This is an advanced SDK integration preview, not the release-ready App
workflow. `cp0ctl new`, `build`, `run` and `package` currently accept Rust Cargo
projects only. Do not promise a distributable C/C++ App until equivalent
project generation, final linking, simulator and packaging support lands.

Use C11 or C++17 only with the pinned Emscripten toolchain. Include
`sdk/c/include/cardputerzero.h`, compile freestanding for WebAssembly, disable
exceptions/RTTI for C++, and export `main` plus linear memory. The raw generated
`cardputerzero_imports.h` is internal to the wrapper and is not an app API.

The DevKit validates the public headers and freestanding objects. It does not
yet supply a supported final link recipe. Compiling an object file alone is
insufficient evidence that a C/C++ application can run on CardputerZero OS.

`sdk/lvgl` contains only the CardputerZero LVGL 9 adapter; the DevKit does not
bundle upstream LVGL sources. Prefer the allocation-free Rust Canvas until the
LVGL source, linker and package pipeline is released.

## Package and sign

```sh
cp0ctl package ./my-app ./my-app.unsigned.capp
cp0ctl key generate /secure/developer.key ./developer.pub
cp0ctl sign developer ./my-app.unsigned.capp ./my-app.capp \
  /secure/developer.key
cp0ctl verify ./my-app.capp
```

Generate a developer key only when the user explicitly requests it. Keep the
secret outside source control and application packages. Store distribution
adds an independent review signature; a developer signature is not store
approval.

## Device install and logs

```sh
cp0ctl pair ./developer.pub ~/.ssh/cardputerzero_ed25519.pub workstation \
  --device OWNER@DEVICE_IP
cp0ctl install ./my-app.capp --device OWNER@DEVICE_IP
cp0ctl logs dev.example.my-app 100 --device OWNER@DEVICE_IP
cp0ctl app start dev.example.my-app --device OWNER@DEVICE_IP
cp0ctl app stop dev.example.my-app --device OWNER@DEVICE_IP
cp0ctl app uninstall dev.example.my-app --device OWNER@DEVICE_IP
```

Before install, confirm first-boot Setup is complete and the device is not
running a soak, recovery, update or factory acceptance test. Launching an App
invalidates an active stability run. The owner must physically enable Developer
Mode and open **Pair New Computer** before the first `pair` command. Developer
Mode starts the constrained transport, so Owner SSH Shell can remain Off. Read
`developer-mode.md` for key generation, pairing, revocation and failure
behavior. Installation must fail closed when provisioning, pairing, mode or
policy is missing.
