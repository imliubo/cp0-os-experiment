# Store Acceptance v1

<!-- doc-locale: en -->
> **English** | [简体中文](README.zh-CN.md)

Store Acceptance v1 is the initial 1.0.0 payload used by the deterministic
Store install, interruption and upgrade acceptance flow. It has no controls or
permissions and is not intended as a user application.

![Store Acceptance v1 green payload](assets/screenshot.png)

The white header and green body make version 1 visually distinguishable after
installation. Run it in the simulator with:

```sh
cargo run -p cp0ctl -- run examples/store-acceptance-v1 \
  --duration 250 --permissions deny --keys '' \
  --output target/store-acceptance-v1.ppm
```

Use `scripts/build-test-store.sh` to produce the signed v1/v2 catalogs and
`scripts/device-store-acceptance.sh` for the complete device sequence. This
payload is not included in the eight-app product image.
