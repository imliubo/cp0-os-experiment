# CardputerZero App DevKit distribution

## Release contract

Every App SDK release has one version across the manifest contract, Rust/C/C++
SDKs, simulator, `cp0ctl`, Skill and DevKit. `devkit/toolchain.toml` pins the
compiler environment used for acceptance. A release must not mix files from
different commits or SDK versions.

Public distribution is blocked until the project owner selects and records a
license for `cp0ctl` and the repository content included in the DevKit, then
ships the required license and third-party notices. The Rust `cp0-sdk` crate
declares Apache-2.0, but that declaration alone does not license the other
bundled files or executable. Internal acceptance archives may be built before
this decision; do not publish them as public release assets.

Rust is the only release-ready end-to-end App workflow in DevKit 1.0. The
C/C++ headers, ABI contract and LVGL adapter are shipped for advanced
integration, but `cp0ctl` does not yet generate, finally link, simulate or
package those projects. Upstream LVGL sources are not bundled. Release notes
must preserve this distinction.

Run `make devkit` to produce a host-native archive under `target/app-devkit`.
The archive contains:

- a relocatable `bin/cp0ctl` for its named host target;
- Rust, C/C++, WIT, ABI and LVGL SDK sources;
- the deterministic PC simulator with focused-key and global-media fixtures;
- the `cardputerzero-build-app` Skill, Store Listing schema, Neon Snake and
  Media Controls examples;
- Developer Mode and developer workflow documentation;
- a machine-readable `devkit.json`, per-file `SHA256SUMS` and archive checksum.

Publish a separate native archive for each supported host. Native archives do
not silently download compilers. A developer must use the versions in
`devkit/toolchain.toml`, or use the full toolchain image.

## Use on another computer

Obtain the archive and adjacent checksum for the destination computer's exact
OS and CPU. Then verify both the archive and its extracted contents:

```sh
shasum -a 256 -c cardputerzero-app-devkit-1.0.0-HOST.tar.xz.sha256
tar -xJf cardputerzero-app-devkit-1.0.0-HOST.tar.xz
export CP0_DEVKIT_ROOT="$PWD/cardputerzero-app-devkit-1.0.0-HOST"
export PATH="$CP0_DEVKIT_ROOT/bin:$PATH"
export RUSTUP_TOOLCHAIN=1.85.1
(cd "$CP0_DEVKIT_ROOT" && shasum -a 256 -c SHA256SUMS)
"$CP0_DEVKIT_ROOT/skills/cardputerzero-build-app/scripts/doctor.sh" \
  "$CP0_DEVKIT_ROOT" rust
```

Install the exact Rust toolchain and `wasm32-unknown-unknown` target named in
`devkit/toolchain.toml`; Rust App development also needs Node 20 or newer. The
native archive contains `cp0ctl`, SDK sources, simulator, Skill, schemas,
examples and developer documentation, but intentionally contains no compiler.
Use the full OCI image when installing matching host tools is undesirable or no
native archive exists for the workstation.

Public macOS native archives require project-approved Developer ID signing and
notarization in addition to the checksum. Do not instruct users to disable
Gatekeeper or remove quarantine from an unverified internal build.

```sh
rustup toolchain install 1.85.1 --profile minimal
rustup target add --toolchain 1.85.1 wasm32-unknown-unknown
```

An App repository may be copied without `target/`, private signing keys or
OAuth tokens. `cp0ctl new` currently writes the source DevKit's canonical
`sdk/rust` path into the generated `Cargo.toml`; rebind that single dependency
path to the destination DevKit before building. A newly paired workstation
uses its own Ed25519 SSH key. Transfer a developer signing private key only
through the developer's secure key-management process when App signing identity
must remain unchanged.

## Full toolchain image

`devkit/Dockerfile` builds the canonical environment from the pinned Emscripten
SDK multi-platform digest and Rust toolchain. It includes Rust
`wasm32-unknown-unknown`, Node, Emscripten, `cp0ctl`, every SDK and the
simulator. Build and export it with:

```sh
docker build -f devkit/Dockerfile -t cardputerzero/app-devkit:1.0.0 .
docker save cardputerzero/app-devkit:1.0.0 | xz -T0 > cardputerzero-app-devkit-1.0.0.oci.tar.xz
shasum -a 256 cardputerzero-app-devkit-1.0.0.oci.tar.xz > cardputerzero-app-devkit-1.0.0.oci.tar.xz.sha256
```

Release the OCI archive and checksum for offline use. Developers load it with
`docker load` and launch it through `devkit/cp0-dev`. Do not instruct AI agents
to fetch an unversioned SDK, compiler installer or container tag.

`devkit/Dockerfile` requires the complete source workspace. The copy carried in
a native DevKit is release metadata and cannot build the full image from the
extracted archive alone; native consumers must obtain a released OCI archive.

## Release acceptance

Before publishing, verify all of the following on every host artifact:

1. The archive checksum and internal `SHA256SUMS` pass.
2. `bin/cp0ctl new` creates a project whose SDK path resolves inside the
   extracted DevKit.
3. The generated project builds without the source repository.
4. Neon Snake and Media Controls build; the simulator produces 320x150 frames,
   profiles and consumes all scripted global media actions.
5. The Skill passes its structural validator and its `doctor.sh` reports the
   expected toolchain.
6. `make check` passes at the release commit.
7. The release contains the owner-approved license and third-party notices.
8. macOS archives are Developer ID signed and notarized before public release.

Signing and publishing release artifacts belongs to the release pipeline. The
Skill may verify published checksums, but must not bypass a missing checksum or
substitute files from another version.
