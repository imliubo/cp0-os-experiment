# Shared Photo Library

<!-- doc-locale: en -->
> **English** | [简体中文](photos.zh-CN.md)

Use this reference when an App captures, saves, lists, displays or deletes a
photo. The library is a brokered SDK capability, not a filesystem or private
storage namespace.

## Public Rust API

Every frame is exactly 320x170 RGB565 little-endian: 54,400 `u16` pixels or
108,800 bytes. Keep one caller-owned frame and use only these high-level calls:

- `photos::count()` returns the active `u64` photo count;
- `photos::list_page(offset, output)` fills a bounded caller-owned `Photo`
  slice from a logical active-photo offset;
- `photos::load_rgb565(photo, pixels)` loads one complete frame;
- `photos::save_rgb565(pixels, suggested_id)` atomically imports one frame and
  returns the broker-assigned nonzero ID;
- `photos::delete(photo)` explicitly removes one selected photo and reports
  whether it existed.

Use `photos::LIST_PAGE_PHOTOS` (eight) for a small fixed navigation cache. Do
not use the SDK's internal keys, chunks, legacy imports or ID hint as an App
storage format. IDs are monotonically allocated by appd, and deletion does not
renumber the remaining library.

## Permissions and isolation

Declare `photos.read` for count, list and load. Declare `photos.write` for save
or delete. Capturing a new camera frame separately requires `camera.capture`.
Private `storage` does not grant shared-photo access and shared-photo
permissions do not expose another App's private data.

No App receives an SD-card path, Gallery index path, device node or mutable
library metadata. Camera photos and trusted System Shell screenshots share the
same library, but an App sees only brokered IDs and frame pixels. Owner Photo
Transfer is a separate owner workflow and not an App or Developer Mode API.

There is no fixed photo-count limit and no automatic eviction. A frame remains
until the owner explicitly deletes it. When storage cannot preserve 64 MiB for
the system while accepting a complete frame, save returns `ResourceLimit` and
existing photos remain unchanged.

## Implementation pattern

1. Render a usable loading, empty or permission state before broker work.
2. Call `count`, clamp the selected logical offset, then fetch one eight-item
   page with `list_page`.
3. Load only the selected frame into one fixed 54,400-pixel buffer.
4. Require explicit confirmation before `delete`, then refresh count and page.
5. Treat `Denied`, `Unavailable`, `ResourceLimit` and malformed/missing frames
   as visible, recoverable states.

After deletion, count/list no longer expose the photo and its frame bytes are
reclaimed, but v2 head/page metadata remains. Simulator `photo_library_bytes`
therefore does not normally return to zero; validate the visible count and
frame removal rather than treating retained index bytes as a leaked photo.

The simulator starts each run with an empty deterministic photo library.
`--permissions allow` permits declared photo calls for that run; `deny` checks
the denial path. A save-then-list test in one App run can exercise import,
pagination and load. Inspect `capability_calls`, `photo_library_keys` and
`photo_library_bytes` in the JSON profile; never infer success from a rendered
frame alone.

Use `examples/camera` for capture/import and `examples/gallery` for paginated
read/delete. They are production built-ins with protected identities, so copy
the interaction pattern into a newly generated project with a developer-owned
App ID.
