# Gallery

Gallery reads photos through the shared photo-library capability. It never
receives a filesystem path and can delete a photo only after an explicit
confirmation.

The production library has no photo-count eviction policy. Gallery caches only
the current eight-ID page and loads another page when navigation crosses its
boundary, so memory use does not grow with the library.

![Gallery empty-library state](assets/screenshot.png)

## Controls

- Left or Right: move between photos.
- Enter on a photo: open the delete confirmation.
- Left or Right in the confirmation: choose Cancel or Delete.
- Enter: confirm the selected action.
- Enter when the library is empty: refresh the library.

The app requests `photos.read` for viewing and `photos.write` for deletion.

## Run in the simulator

```sh
cargo run -p cp0ctl -- run examples/gallery \
  --duration 250 --permissions allow \
  --output target/gallery.ppm
```

Gallery is one of the eight applications included in the product image and is
a non-removable production built-in.
