# Store Acceptance v2

Store Acceptance v2 is the 1.1.0 upgrade payload used by the deterministic
Store resume, update and persistence acceptance flow. It has no controls or
permissions and is not intended as a user application.

![Store Acceptance v2 blue payload](assets/screenshot.png)

The white header and blue body make version 2 visually distinguishable from
version 1 after an update. Run it in the simulator with:

```sh
cargo run -p cp0ctl -- run examples/store-acceptance-v2 \
  --duration 250 --permissions deny --keys '' \
  --output target/store-acceptance-v2.ppm
```

Use `scripts/build-test-store.sh` to produce the signed v1/v2 catalogs and
`scripts/device-store-acceptance.sh` for the complete device sequence. This
payload is not included in the eight-app product image.
