# App Troubleshooting

## Doctor fails

- Missing `sdk/rust` or simulator: `CP0_DEVKIT_ROOT` points at the wrong level
  or the archive is incomplete. Verify `SHA256SUMS`.
- Missing Rust target: install pinned `wasm32-unknown-unknown` or use the
  toolchain image.
- Old Rust/Node: do not edit the app around a host-tool problem; select the
  pinned environment.
- Missing `emcc`: irrelevant for Rust; required for C/C++ and LVGL only.

## Manifest or build fails

- Run `cp0ctl manifest validate app.json` first.
- Match `entrypoint` to Cargo's underscore-normalized `cdylib` artifact.
- Keep SDK version `1.0`, runtime `wamr`, and a canonical relative `bin` path.
- Do not add WASI crates, `std`, filesystem APIs or OS-specific build scripts.
- If Cargo cannot find `cp0-sdk`, regenerate with the released `cp0ctl` or set
  `CP0_DEVKIT_ROOT` before `new`; do not hard-code another developer's path.

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

## Package or install fails

- Rebuild before packaging; verify manifest identity/version and package
  signature locally.
- A developer-signed package needs a matching trusted public key and enabled
  developer mode on the device.
- Store packages need a separate review signature and must advance versions.
- Refuse installation while a stability or destructive acceptance run is
  active. Retrieve its evidence first.
- Use `cp0ctl logs APP_ID` for bounded service-mediated logs; do not grant the
  app shell or SSH access.
