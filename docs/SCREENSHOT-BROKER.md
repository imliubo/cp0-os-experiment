# Trusted Screenshot Broker

## Contract

`Fn+J` is owned by the compositor and never reaches an application as a raw
PrintScreen key. The compositor sends the trusted action to System Shell,
which captures the currently visible framebuffer and imports it into the
production photo library.

The V0.6 contract is fixed:

- one 320x170 XRGB8888 or ARGB8888 capture;
- one 108,800-byte RGB565 little-endian Gallery frame;
- the shared photo library is the only persistent destination;
- no Shell-private PNG duplicate and no automatic retention deletion.

No screenshot API is part of the application SDK. Apps cannot request a device
screenshot, name a destination, or access a host path. Gallery reads the result
only through `photos.read`.

## Authorization

Weston's capture global is denied by default. `cardputerzero-policy.so` allows
an attempt only when the exact trusted Shell Wayland client owns the registered
`os.cardputerzero.shell` surface and the selected output is 320x170.

System Shell converts the capture and sends one exact read-only memfd to
appd. The descriptor must carry `F_SEAL_SEAL`, `F_SEAL_SHRINK`, `F_SEAL_GROW`
and `F_SEAL_WRITE`. appd authenticates `SO_PEERCRED` and accepts
`import-screenshot` only from the configured `cp0-shell` UID. Root, Store,
Apps, missing/extra descriptors, writable files and wrong sizes fail closed.

## Persistence

appd holds the shared library transaction lock, atomically publishes one frame
blob, updates the v2 index page and commits `head.v2`. A failed operation
removes its staging data and leaves every previously committed photo visible.
The same lock covers Camera imports and Gallery removals. There is no 32-frame
path and no duplicate screenshot state directory.

The capture starts before the status overlay, so it records the screen visible
at the key press. After the transaction commits, Shell shows
`SCREENSHOT / SAVED` for two seconds. Storage/protocol failures show `FAILED`;
an unsupported capture contract shows `UNAVAILABLE`.

## Verification

Local tests cover XRGB8888-to-RGB565 conversion, strict control responses, FD
transfer and seals, Shell-only authorization, v1 migration, v2 rollback,
single-blob publication, interrupted-tail recovery, startup staging cleanup and
retention beyond 32 frames. Physical `Fn+J`, Gallery display, SD-full behavior
and persistence across power loss remain device acceptance items.
