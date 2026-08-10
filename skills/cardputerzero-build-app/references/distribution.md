# DevKit and Toolchain Distribution

<!-- doc-locale: en -->
> **English** | [简体中文](distribution.zh-CN.md)

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
- App and Store Listing schemas;
- the complete eight-App product example set with reference screenshots;
- Developer Mode, photo-library and icon documentation, version metadata and
  checksums.

The examples are Hello Card, Calculator, Neon Snake, Camera, Gallery, Media
Controls, Notes and Stopwatch. Camera and Gallery use protected built-in IDs;
they are reference source, not third-party packages to install unchanged.

Set `CP0_DEVKIT_ROOT` to the extracted root and add its `bin` directory to
`PATH`. Select the Rust version in `devkit/toolchain.toml` and run this Skill's
`scripts/doctor.sh` before creating a project:

```sh
shasum -a 256 -c cardputerzero-app-devkit-VERSION-HOST.tar.xz.sha256
tar -xJf cardputerzero-app-devkit-VERSION-HOST.tar.xz
export CP0_DEVKIT_ROOT="$PWD/cardputerzero-app-devkit-VERSION-HOST"
export PATH="$CP0_DEVKIT_ROOT/bin:$PATH"
export RUSTUP_TOOLCHAIN=$(awk -F '"' '$1 ~ /^rust_version = / { print $2 }' \
  "$CP0_DEVKIT_ROOT/devkit/toolchain.toml")
(cd "$CP0_DEVKIT_ROOT" && shasum -a 256 -c SHA256SUMS)
"$CP0_DEVKIT_ROOT/skills/cardputerzero-build-app/scripts/doctor.sh" \
  "$CP0_DEVKIT_ROOT" rust
```

Native archives are host-specific because `bin/cp0ctl` is native. Use the
matching macOS/Linux CPU archive. On Windows or a mismatched CPU, use the OCI
image rather than attempting to run another host's binary.

Keep `RUSTUP_TOOLCHAIN` set for the complete App session. Passing doctor alone
does not select a toolchain for later `cp0ctl` child processes.

A public macOS archive also needs project-approved Developer ID signing and
notarization. If Gatekeeper rejects an internal ad-hoc build, use the verified
OCI image or a source checkout; never disable Gatekeeper globally or strip
quarantine from an unverified archive.

The native archive intentionally does not fetch or silently install compilers.
Install Node 20 or newer and the Rust toolchain pinned in
`devkit/toolchain.toml`, or switch to the full image:

```sh
rustup toolchain install 1.85.1 --profile minimal
rustup target add --toolchain 1.85.1 wasm32-unknown-unknown
```

Install pinned Emscripten only for C/C++ compatibility work; it is not needed
for the release-ready Rust workflow.

Rust is the release-ready App language. C/C++ headers, ABI files and the LVGL
adapter are included for compatibility work, but the current DevKit does not
provide their final link, simulator or package workflow.

## Full toolchain image

The canonical image includes the native DevKit plus Rust 1.85.1,
`wasm32-unknown-unknown`, Node 20 or newer and Emscripten 5.0.4. Load a released
offline image only after checksum verification, or build `devkit/Dockerfile`
from the matching signed source revision.

The Dockerfile copied into a native archive is release metadata and requires
the complete source workspace to build. It cannot bootstrap an OCI image from
the extracted native DevKit alone. Native-archive consumers should obtain a
released OCI archive rather than running that Dockerfile in the extracted
directory.

Launch a project directory with:

```sh
CP0_DEVKIT_IMAGE=cardputerzero/app-devkit:1.0.0 \
  /path/to/devkit/cp0-dev /path/to/project
```

## Move an App to another computer

Install one complete, matching DevKit on the destination; do not copy only
`sdk/rust` or reuse the target device as a compiler. Clone or copy the App
source without `target/`, signing secrets or OAuth tokens.

`cp0ctl new` currently writes the creating DevKit's canonical `sdk/rust` path
into `Cargo.toml`. After moving an existing project, replace only the
`[dependencies].cp0-sdk.path` value with the destination's canonical
`$CP0_DEVKIT_ROOT/sdk/rust` path, then run manifest validation, build and the
bundled verifier again. This path rebinding is a current DevKit limitation; do
not commit one developer's absolute path as though it were portable.

The developer signing key may be moved only through the developer's secure key
management process. A new computer also needs its own Ed25519 SSH key and a new
on-device pairing entry. Reusing the same App signing key preserves developer
identity; pairing the new SSH key does not require copying an old SSH private
key.

## Failure behavior

If a release asset, checksum or pinned tool is unavailable, report the exact
missing artifact. Do not substitute `latest`, curl an arbitrary SDK file,
disable checksum verification, or copy a compiler from the target device.
