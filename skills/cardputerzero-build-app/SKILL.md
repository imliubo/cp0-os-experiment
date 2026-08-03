---
name: cardputerzero-build-app
description: Build, modify, debug, simulate, package, sign, pair, deploy, and submit applications for CardputerZero OS with its isolated WebAssembly SDK. Use for CardputerZero app ideas, app.json manifests, 320x170 or 320x150 UI, keyboard and global media input, Rust/C/C++ SDK code, lifecycle checkpoints, cp0ctl workflows, permissions, cross-computer DevKit setup, Developer Mode pairing and deployment, simulator failures, .capp signing, Store listings, OAuth submission, and provisioned-device installation.
---

# Build CardputerZero Apps

Use the supported SDK and `cp0ctl`; never target Raspberry Pi Linux APIs,
desktop frameworks, device nodes, sockets, shell commands, DRM, evdev or WASI.
Applications are isolated WebAssembly modules and receive only declared broker
capabilities.

## Resolve The DevKit

1. Set `SKILL_DIR` to the directory containing this `SKILL.md`. Locate `ROOT`
   in this order: `$CP0_DEVKIT_ROOT`, an extracted DevKit that contains this
   Skill, or the CardputerZero-OS repository root.
2. Run `"$SKILL_DIR/scripts/doctor.sh" "$ROOT" rust`. If it fails, read
   [references/distribution.md](references/distribution.md) and use the pinned
   toolchain image or install only the reported missing component.
3. Prefer `ROOT/bin/cp0ctl` in a released DevKit. In a source checkout, use
   `cargo run --quiet --manifest-path ROOT/Cargo.toml -p cp0ctl --`.
4. Read [references/platform-contract.md](references/platform-contract.md)
   before changing a manifest, input model, display mode or permission set.

Do not download unversioned SDK files, copy private host imports from an
example, or substitute a compiler/toolchain version without reporting it.
When moving an App to another computer, read the migration section of
[references/distribution.md](references/distribution.md); generated Rust
projects currently bind `cp0-sdk` to the creating DevKit's absolute path.

## Choose The Implementation

- Default to Rust `no_std`. It has the complete supported high-level SDK,
  project generator, build, simulator and package workflow.
- The release-ready project workflow is Rust. Treat C11/C++17 and LVGL as an
  advanced SDK integration preview: read the C/C++ section of
  [references/workflows.md](references/workflows.md), report the missing
  project/package automation, and do not claim end-to-end completion.
- Use standard 320x150 display unless the app genuinely needs the trusted
  status-bar area. Use immersive 320x170 sparingly; trusted overlays still win.
- Keep one caller-owned RGB565 frame. Redraw on state changes or at no more
  than 30 FPS. Poll input with a bounded timeout.
- Design for keyboard-only operation, high contrast, stable geometry and the
  physical 320-pixel screen. Never rely on hover, touch or tiny text.
- For media applications, use the targetless `media` SDK and read the media
  section of [references/platform-contract.md](references/platform-contract.md).
  Never treat global media actions as raw focused key events.
- Treat lifecycle checkpoint exports as a forward-compatible simulation
  preview until the target image confirms Runtime support. Do not claim that
  current hardware will invoke them merely because the SDK declarations exist.

## Create Or Modify

For a new Rust app, run:

```sh
cp0ctl new APP_DIR dev.example.app "App Name"
```

Use a reverse-domain app ID owned by the developer. Inspect and edit all three
generated files: `app.json`, `Cargo.toml`, and `src/lib.rs`. Preserve the
generated `cdylib`, release profile, SDK path and exported `main` contract.

For an existing app, inspect those files plus the exact SDK modules it imports.
Follow local patterns from a nearby current example. Use `examples/neon-snake`
for stateful UI and storage, and `examples/media-controls` for global media
actions. Do not add an abstraction unless it removes real application
complexity.

Implement the smallest complete interaction loop:

1. Initialize bounded state and a static frame buffer.
2. Render a valid first frame immediately.
3. Submit through `display::present_rgb565`.
4. Poll focused key events; act only on pressed events.
5. Redraw only when state or time-driven animation changes.
6. Treat capability denial and temporary unavailability as normal states.

## Declare Capabilities

Start with no permissions. Add a manifest permission only after code uses its
matching public SDK module. Give a short user-facing reason. Private storage is
quota-controlled and needs no permission. Never request a broader permission
to work around an implementation failure.

Media-session registration needs no permission and grants no audio access.
Declare `audio.playback` separately when the app actually submits PCM audio.

Read [references/platform-contract.md](references/platform-contract.md) for the
closed permission vocabulary, limits and input codes. Confirm imports and
manifest declarations agree before packaging.

## Verify Before Packaging

Run the bundled verifier with a representative comma-separated key sequence:

```sh
"$SKILL_DIR/scripts/verify-app.sh" APP_DIR left,right,enter deny 1000
```

For a media app, pass global actions as the fifth optional argument:

```sh
"$SKILL_DIR/scripts/verify-app.sh" APP_DIR "" deny 1000 \
  play-pause,previous,next
```

Then inspect both the rendered PPM and JSON profile. Exercise at least the
initial state, main success path, boundary inputs, restart/back behavior and
every permission allow/deny state used by the app. Add deterministic logic
tests for games or stateful tools.

Do not accept a build alone. Completion requires a valid manifest, successful
WASM build, at least one simulator frame, bounded memory, expected input count
and no undeclared capability calls.

## Package And Install

Read [references/workflows.md](references/workflows.md) before signing or device
installation. Package reproducibly with `cp0ctl package`, sign with a developer
key kept outside the project, and verify the signed `.capp` before install.

For a product device, read
[references/developer-mode.md](references/developer-mode.md) before pairing or
remote deployment. The owner must physically enable Developer Mode and open the
ten-minute pairing window. Pair the workstation's developer-signing public key
and Ed25519 SSH public key with `cp0ctl pair`; do not edit trust files or use
`scp`, sudo, a remote shell or an unsigned package.

When Store distribution is requested, read
[references/store-submission.md](references/store-submission.md). Prepare and
locally validate the Listing before starting OAuth Device Flow. Do not run the
internal `store publish` operator workflow or add a Store signature yourself.

Installing changes a real device. Confirm the requested device and that no
stability, recovery or factory acceptance run is active. Product devices must
have completed first-boot provisioning. Developer Mode itself starts the
constrained SSH transport; the independent Owner SSH Shell may remain Off. Use
the owner-selected account and current device IP; never assume `pi`, a fixed
password or a stable address. Never weaken device policy, copy a private key to
the device, or bypass signature verification.

## Diagnose Failures

Read [references/troubleshooting.md](references/troubleshooting.md) when doctor,
build, simulator, package or device install fails. Preserve the first concrete
error, identify whether it belongs to the host toolchain, manifest, WASM ABI,
application logic, permission policy or device state, and retest at the
narrowest layer before continuing.

## Completion Checklist

- `doctor.sh` passes for the selected language.
- Manifest identity, entrypoint, SDK version, display, resources and permissions
  match the implementation.
- Logic tests and `verify-app.sh` pass with representative input.
- The final frame is visually inspected at its real 320-pixel dimensions.
- Package signature verification passes when distribution is requested.
- Developer Mode pairing uses the exact package-signing public key and a
  separate Ed25519 SSH public key when device deployment is requested.
- Store Listing validation passes before submission when Store distribution is
  requested.
- Device install and physical keys are tested only when authorized and safe.
- Source, commands, artifact paths and any remaining hardware checks are
  reported to the user.
