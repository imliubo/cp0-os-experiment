# CardputerZero Photo Library v1

Camera and Gallery are ordinary sandboxed WASM SDK applications. Neither app
can access `/dev/video*`, an SD-card path, or another application's private
storage. Camera capture remains brokered by `camera.capture`; persistent photos
use a separate shared-library broker backed by storaged.

## Permissions

| Permission | Access |
| --- | --- |
| `camera.capture` | Capture one fixed 320x170 RGB565 frame from camerad. |
| `photos.read` | Read the shared photo index and photo chunks. |
| `photos.write` | Read only the update index, then write or delete library values. |

Camera declares `camera.capture` and `photos.write`. Gallery declares
`photos.read` and `photos.write`. Private `storage` remains identity-bound and
is not part of the photo library.

## Bounds and format

- Maximum photos: 32.
- Frame format: 320x170 RGB565 little-endian, 108,800 bytes.
- Value bound: 8 KiB, inherited from storaged.
- Chunks per frame: 14.
- Shared storaged identity: `dev.cardputerzero.photo-library`.
- Shared quota: 8 MiB.
- Index: versioned binary record containing monotonically increasing photo IDs.

The Rust SDK exposes `photos::list`, `save_rgb565`, `load_rgb565`, and
`delete`. Saving writes every frame chunk before committing the new index. A
failed chunk or index write removes the uncommitted chunks and leaves the old
index visible. Deletion commits the reduced index before reclaiming chunks.
