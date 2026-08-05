# Gallery

Gallery reads photos through the shared photo-library capability. It never
receives a filesystem path and can delete a photo only after an explicit
confirmation.

The production library has no photo-count eviction policy. Gallery caches only
the current eight-ID page and one 320x170 view. Camera JPEG originals are
decoded and cropped by appd, never copied into the Gallery WASM sandbox.

![Gallery empty-library state](assets/screenshot.png)

## Controls

- F or Z: move to the previous photo, wrapping from the first to the last.
- X or C: move to the next photo, wrapping from the last to the first.
- Up, Down, Left or Right also change photos in the normal browser.
- Enter on a photo: open its full-screen original view.
- Enter in the original view: cycle Fit, half-resolution and 1:1 zoom.
- Direction keys in the original view: move the viewport.
- Backspace in the original view: return to the normal browser.
- Backspace in the normal browser: open the delete confirmation.
- The same previous/next keys in the confirmation choose Cancel or Delete.
- Enter: confirm the selected action.
- Enter when the library is empty: refresh the library.

Plain keys and their Fn combinations are both accepted when they produce the
same key codes. The app requests `photos.read` for viewing and `photos.write`
for deletion.

## Run in the simulator

```sh
cargo run -p cp0ctl -- run examples/gallery \
  --duration 250 --permissions allow \
  --output target/gallery.ppm
```

Gallery is one of the eight applications included in the product image and is
a non-removable production built-in.
