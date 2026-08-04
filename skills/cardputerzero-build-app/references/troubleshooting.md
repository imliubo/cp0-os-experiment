# App Troubleshooting

## Doctor fails

- Missing `sdk/rust` or simulator: `CP0_DEVKIT_ROOT` points at the wrong level
  or the archive is incomplete. Verify `SHA256SUMS`.
- Missing Rust target: install pinned `wasm32-unknown-unknown` or use the
  toolchain image.
- Old Rust/Node: do not edit the app around a host-tool problem; select the
  pinned environment.
- Rust target appears installed but doctor still fails: install
  `wasm32-unknown-unknown` for the exact Rust version in
  `devkit/toolchain.toml`, not only for the current default toolchain.
- Missing `emcc`: irrelevant for Rust; required for C/C++ and LVGL only.

## Manifest or build fails

- Run `cp0ctl manifest validate app.json` first.
- Match `entrypoint` to Cargo's underscore-normalized `cdylib` artifact.
- Keep SDK version `1.0`, runtime `wamr`, and a canonical relative `bin` path.
- Do not add WASI crates, `std`, filesystem APIs or OS-specific build scripts.
- If Cargo cannot find `cp0-sdk`, regenerate with the released `cp0ctl` or set
  `CP0_DEVKIT_ROOT` before `new`. For a project moved from another computer,
  rebind only `cp0-sdk.path` to this DevKit's canonical `sdk/rust` directory;
  do not retain another developer's absolute path.

## Simulator fails

- "did not present a valid frame": render and submit a full 320x150 or 320x170
  RGB565 frame before blocking for input.
- Unknown key name: use the closed names in `platform-contract.md` and
  `simulator/cp0-simulator.mjs`.
- Permission denied: declare the exact capability and test both `allow` and
  `deny`; do not bypass the broker.
- Invalid frame: manifest display mode, frame allocation and Canvas height do
  not agree.
- App returns nonzero: preserve the first SDK error and test that branch rather
  than converting every error to success.
- Unknown `cp0_media_*` import: the SDK and simulator came from different
  DevKit versions. Do not remove the media call; restore a matched DevKit.
- Media action is not consumed: register a non-inactive session with the exact
  supported mask, use `--media-actions` rather than raw `--keys`, and inspect
  `media_session_updates` plus `media_actions_taken` in the profile.
- Photo count is unexpectedly zero: simulator photo state lasts for one run;
  save and list in the same App execution or test Gallery's empty state.
- Photo call is denied: `photos.read`, `photos.write` and `camera.capture` are
  separate declarations. Add only the capability used and test allow/deny.
- Photo load is damaged or incomplete: require an exact 320x170 RGB565 buffer,
  preserve the broker error, and never reconstruct index/chunk keys yourself.
- Photo save returns `ResourceLimit`: keep the previous library unchanged and
  show a recoverable storage-full state; do not retry in a tight loop.
- First key appears missing only after screen sleep: this is the compositor's
  wake-key contract. Require a fresh deliberate key for the App action.

## Package or install fails

- Rebuild before packaging; verify manifest identity/version and package
  signature locally.
- A developer-signed package needs a matching trusted public key and enabled
  developer mode on the device.
- `DeveloperModeOff`: physically enable **Settings > Security > Developer
  Mode**. Owner SSH Shell is unrelated and may remain Off.
- `PairingClosed`: select **Pair New Computer** and retry within ten minutes.
- First pairing asks for the owner password; later operations use the paired
  forced-command Ed25519 SSH key. Do not fall back to `scp`, sudo or Bash.
- Unknown SSH host key before the password prompt: verify its fingerprint
  through a trusted device/operator channel. Current product UI does not expose
  it; stop on an untrusted network rather than accepting it blindly.
- A signature/key mismatch means the package was signed by a different key
  than the paired `developer.pub`; re-sign with the paired key or deliberately
  create a new pairing. Never edit root trust files.
- A new workstation needs a separate pairing entry. The device accepts at most
  eight paired computers; revoke obsolete entries on-device when full.
- Store packages need a separate review signature and must advance versions.
- Refuse installation while a stability or destructive acceptance run is
  active. Retrieve its evidence first.
- Use `cp0ctl logs APP_ID` for bounded service-mediated logs; do not grant the
  app shell or SSH access.
- SSH unavailable: confirm first-boot Setup completed and Developer Mode is On.
  Developer Mode starts the constrained transport even when Owner SSH Shell is
  Off. Do not try fixed usernames, passwords or maintenance backdoors.
- macOS blocks `cp0ctl`: verify artifact provenance and notarization. Do not
  disable Gatekeeper or remove quarantine from an unverified archive; use the
  OCI image or matching source checkout until a notarized release exists.

## Store submission fails

- Run `cp0ctl store validate` again after every package, Listing or PNG change.
- Identity mismatch: `app_id` and `version` must exactly match the signed
  package; do not edit a signed `.capp`.
- OAuth pending or expired: follow the displayed verification URI, use the
  registered App owner's eligible account with 2FA, and restart only after the
  stable error is reported.
- Never place a private key, OAuth token or Store signature in `store/`.
