# Phase 4A: Rust SDK foundation

## Public application API

`sdk/rust` is the first supported CardputerZero application SDK. It is a
dependency-free `no_std` crate for `wasm32-unknown-unknown`; application code
does not declare WAMR imports or depend on Linux APIs.

SDK 0.1 provides:

- `system::monotonic_milliseconds()`;
- `system::wait_event()` with a maximum one-second wait;
- `system::post_notification()` with the manifest capability broker;
- stable `Denied`, `Unavailable`, `InvalidArgument`, `ResourceLimit` and
  `Internal` errors.

The SDK validates character counts and control characters before crossing the
WASM boundary. Runtime-private integer status codes and import symbols remain
encapsulated. A pending permission prompt maps to `Unavailable`, allowing the
single foreground event loop to retry without blocking.

Hello Card now depends only on `cp0-sdk`. Its source has no raw
`extern "C"`, socket, path or permission protocol knowledge.

## Runtime ABI

The Runtime adds `cp0_monotonic_milliseconds: () -> i64`, implemented with
`CLOCK_MONOTONIC`. It complements the existing bounded wait and typed
notification calls. The WIT file remains the source-level contract; Phase 4
code generation will replace the small hand-maintained private binding module
while preserving the public Rust API.

## Validation

- Workspace unit tests cover SDK error mapping and argument limits on the host.
- The SDK and Hello build successfully for `wasm32-unknown-unknown`.
- Final aarch64 Runtime SHA-256:
  `8cb76b9e34309a5a85adb0999d132d8a2eaf50975ea66854d75dc407cd9aeccd`.
- SDK-based Hello WASM SHA-256:
  `d1830261bec651deb3cabc35f05e8bf524a97fd136c61b9cefc68da87d91eff6`.
- On V0.6, Hello started through appd, posted notification ID 4 through the SDK,
  and stopped cleanly. Appd, compositor and System Shell remained active.

## Project workflow

The first host development commands are:

```sh
cp0ctl new <directory> <app-id> <display-name>
cp0ctl build <directory>
```

`new` refuses to overwrite an existing path, validates the generated manifest
before writing and creates a `no_std` cdylib with no private Runtime imports.
Until SDK 0.1 is published to a developer registry, its Cargo dependency points
to the canonical SDK path in the current checkout.

`build` validates `app.json`, reads Cargo's structured metadata to find the
actual cdylib target and target directory, builds `wasm32-unknown-unknown`, and
stages a package-shaped tree below
`target/cardputerzero/<app-id>/<version>`. Tests execute the complete generated
project build rather than checking templates only.

## Image integration

`image/build-image.sh` builds the pinned aarch64 appd, Runtime and Hello
artifacts before invoking pi-gen. The `02-app-platform` stage installs only
those release artifacts, not a compiler toolchain. It creates the reserved
UID/GID 20000, private data directory, root-owned package, and then asks
`cp0-appd register-installed` to create the canonical registry rather than
hand-writing trusted state.

Both socket units are enabled and the compositor now starts by default. The
stage has offline profile tests; a complete image build and flash are deferred
until the remaining boot/read-only-rootfs work is ready for one consolidated
hardware validation cycle.
