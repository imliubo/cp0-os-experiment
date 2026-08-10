# Device Capability Probe

<!-- doc-locale: en -->
> **English** | [简体中文](README.zh-CN.md)

Device Capability Probe is an automated factory and acceptance application. It
tests bounded audio playback, microphone capture, the logical Grove GPIO line
and private-storage quota behavior. It is not intended as a user application.

![Capability Probe result bands](assets/screenshot.png)

The four horizontal bands represent playback, capture, GPIO and storage. Green
means success, blue is the expected first-run storage-quota result, yellow
means unavailable or resource-limited, red means denied and magenta means a
failure. A machine-readable summary is also posted as a notification and saved
to private storage.

## Run in the simulator

```sh
cargo run -p cp0ctl -- run examples/device-capability-probe \
  --duration 5000 --permissions allow --keys '' \
  --output target/device-capability-probe.ppm
```

Use `scripts/device-capability-acceptance.sh` for the complete on-device flow.
This probe is not included in the eight-app product image.
