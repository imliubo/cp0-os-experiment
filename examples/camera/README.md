# Camera

Camera displays a 320x170 preview from the trusted 30 FPS camera pipeline.
Enter or Space encodes the next frame as a 1280x720 JPEG and saves the original
plus a 320x170 thumbnail to the shared photo library without restarting preview.

![Camera preview after saving a photo](assets/screenshot.png)

## Controls

- Enter or Space: capture and save a 1280x720 photo.
- `Esc`: leave the application through the System Shell.

The first launch can show permission prompts for `camera.capture` and
`photos.write`. Denying either permission keeps the app isolated and produces
an explicit unavailable or denied state.

## Run in the simulator

```sh
cargo run -p cp0ctl -- run examples/camera \
  --duration 700 --permissions allow --keys enter \
  --output target/camera.ppm
```

Camera is one of the eight applications included in the product image.
