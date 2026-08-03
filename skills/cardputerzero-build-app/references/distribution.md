# DevKit and Toolchain Distribution

## Preferred order

1. Use a released full-toolchain OCI image for the most reproducible setup.
2. Use the host-native DevKit archive plus the exact tools in
   `devkit/toolchain.toml` when containers are unavailable.
3. Use a source checkout only for OS/SDK development.

Every released archive must have an adjacent SHA-256 file. Verify it before
extracting, then verify the archive's internal `SHA256SUMS`. Do not mix the
Skill, SDK, simulator or `cp0ctl` across DevKit versions.

Do not publicly redistribute an internally built archive unless the release
also contains the project-owner-approved license and required third-party
notices. A license declaration on one SDK crate does not cover every bundled
tool and file.

## Native DevKit

The native archive is named
`cardputerzero-app-devkit-VERSION-HOST.tar.xz`. It contains:

- `bin/cp0ctl` for the named host;
- `sdk/{rust,c,lvgl,wit,abi}`;
- `simulator/cp0-simulator.mjs`;
- this Skill under `skills/`;
- verified Neon Snake and Media Controls examples, version metadata and
  checksums.

Set `CP0_DEVKIT_ROOT` to the extracted root and add its `bin` directory to
`PATH`. Run this Skill's `scripts/doctor.sh` before creating a project.

The native archive intentionally does not fetch or silently install compilers.
Install the Rust, Node and optional Emscripten versions pinned in
`devkit/toolchain.toml`, or switch to the full image.

Rust is the release-ready App language. C/C++ headers, ABI files and the LVGL
adapter are included for compatibility work, but the current DevKit does not
provide their final link, simulator or package workflow.

## Full toolchain image

The canonical image includes the native DevKit plus Rust 1.85.1,
`wasm32-unknown-unknown`, Node 20 or newer and Emscripten 5.0.4. Load a released
offline image only after checksum verification, or build `devkit/Dockerfile`
from the matching signed source revision.

Launch a project directory with:

```sh
CP0_DEVKIT_IMAGE=cardputerzero/app-devkit:1.0.0 \
  /path/to/devkit/cp0-dev /path/to/project
```

## Failure behavior

If a release asset, checksum or pinned tool is unavailable, report the exact
missing artifact. Do not substitute `latest`, curl an arbitrary SDK file,
disable checksum verification, or copy a compiler from the target device.
