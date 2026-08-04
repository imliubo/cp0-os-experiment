# Storage Isolation Probe

Storage Isolation Probe verifies that it cannot read the marker written by the
Device Capability Probe under a different application identity. It is an
acceptance payload, not a user application.

![Storage Isolation Probe successful result](assets/screenshot.png)

The screen is green when no foreign marker is visible, red when data leaked
across app identities and magenta when storage returned an error. The result is
also emitted as a notification and stored in this app's private namespace.

## Run in the simulator

```sh
cargo run -p cp0ctl -- run examples/storage-isolation-probe \
  --duration 1000 --permissions allow --keys '' \
  --output target/storage-isolation-probe.ppm
```

Use `scripts/device-capability-acceptance.sh` to run it after the capability
probe on a device. It is not included in the eight-app product image.
