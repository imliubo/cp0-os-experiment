# App Development Workflows

## Rust

Select `cp0ctl` from an extracted DevKit:

```sh
export CP0_DEVKIT_ROOT=/path/to/cardputerzero-app-devkit
export PATH="$CP0_DEVKIT_ROOT/bin:$PATH"
```

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

Rust apps are `#![no_std]` `cdylib` crates for `wasm32-unknown-unknown`. Keep the
generated panic handler and exported `main`. Use only public modules from
`cp0-sdk`; never declare private imports.

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
cp0ctl install ./my-app.capp --device pi@DEVICE_IP
cp0ctl logs dev.example.my-app 100 --device pi@DEVICE_IP
```

Before install, confirm the device is not running a soak, recovery, update or
factory acceptance test. The device owner must configure the public trust key
and developer mode locally. Installation must fail closed when trust or policy
is missing.
