# CardputerZero Photo Library v2

Camera and Gallery are production built-in WASM applications. They remain
sandboxed and cannot access `/dev/video*`, an SD-card path, another App's data,
or the photo-library files. appd rejects uninstall requests for
`dev.cardputerzero.camera` and `dev.cardputerzero.gallery`. Developer packages
cannot replace these identities; signed Store upgrades remain allowed.

## User contract

- Camera photos and trusted system screenshots share one Gallery library.
- There is no photo-count retention limit and no automatic eviction.
- A photo remains visible until the Owner explicitly deletes it in Gallery.
- When the SD card cannot accept another complete photo while preserving 64
  MiB for the system, the new save fails with `ResourceLimit`; existing photos
  and indexes are unchanged.
- Gallery caches eight IDs and one RGB565 frame. appd caches only the currently
  decoded Camera original, independent of library size.

Physical media capacity is the only practical library bound. The system photo
identity uses a deliberately unreachable 1 PiB logical quota so storaged's
normal per-App quota cannot become an artificial retention policy.

## Permissions

| Permission | Access |
| --- | --- |
| `camera.capture` | Read a 320x170 preview or request one fixed 1280x720 photo. |
| `photos.read` | Read versioned indexes and selected frame chunks. |
| `photos.write` | Append a frame or delete one selected photo. |

Camera declares `camera.capture` and `photos.write`. Gallery declares
`photos.read` and `photos.write`. Private App storage remains identity-bound.
The current SDK exposes frame import and ID-based removal; page/head mutation
and monotonically increasing photo IDs are broker-owned. The ABI retains the
SDK 1.0 ID hint, but appd does not trust it for allocation. Legacy low-level ABI
symbols remain loadable, while appd rejects direct frame or metadata mutation.

## Format

Every Gallery display frame is a 320x170 RGB565 little-endian thumbnail,
exactly 108,800 bytes. Screenshots and the compatibility
`photos.import-rgb565` call store only this representation. A Camera
`capture-photo` transaction additionally stores a fixed 1280x720 JPEG original
as `p<16-hex-id>.jpg` and a 56-byte `p<16-hex-id>.meta` record containing kind,
dimensions, JPEG size, capture time and SHA-256 digest. The Camera App receives
only the broker-owned photo ID; the JPEG is never copied into WASM memory.

appd authenticates the calling App and owns the complete import transaction.
Its private storaged client writes bounded 8 KiB chunks into mode `0600`
temporary blobs. Every chunk is flushed; only the final chunk atomically
publishes each blob. The thumbnail, optional original and metadata are written
before the index page and authoritative head. A failed transaction removes all
uncommitted components and never lists a partial photo.

Gallery loads a committed frame with one `photos.load-rgb565` hostcall. appd
first verifies that the requested ID is still active in the committed index,
then asks storaged to open the corresponding blob read-only. storaged accepts
this descriptor operation only for the system photo-library identity and only
when the blob is a regular file with the exact RGB565 frame size. The
descriptor crosses both Unix-socket boundaries with `SCM_RIGHTS`; appd and
Runtime independently revalidate its type, size, and access mode before
mapping or copying any pixels. This replaces the legacy fourteen sequential
base64 chunk reads while preserving the same App isolation boundary.

Camera originals are viewed through `photos.load-view-rgb565`. The only inputs
are an active photo ID, Fit/half/actual zoom, and bounded `-1000..1000` pan
coordinates. appd validates the metadata, opens the exact read-only JPEG blob,
decodes and caches the current 1280x720 image, then renders one fixed 320x170
RGB565 viewport into a sealed descriptor. Gallery never receives JPEG bytes,
a storage key, a filesystem path or a full-resolution RGB allocation.

`head.v2` is a fixed 32-byte record containing:

- magic `CP0H`, version 2 and reserved zero bytes;
- active photo count;
- append-only slot count;
- last allocated monotonically increasing photo ID.

`index.v2.<8-hex-page>` contains 256 ordered ID slots. Explicit deletion turns
one slot into a zero tombstone and decrements the page/head active counts; it
does not compact or renumber later photos. Gallery's `list_page(offset, out)`
uses logical active-photo offsets and skips tombstones.

Saving publishes the thumbnail and optional JPEG/metadata, updates its index
page, then commits `head.v2`. Page/head failure restores the old page and
removes every uncommitted component. Deletion commits the tombstone and head
before reclaiming the thumbnail, JPEG and metadata.
Camera imports, Shell screenshot imports and Gallery removals share one appd
transaction lock, so their page/head updates cannot overwrite one another.

The committed head slot count is the recovery boundary. Gallery derives
visible counts from committed page slots instead of trusting a cached count.
Before the next mutation, appd reconciles page counts and clears any page tail
written before a lost head commit. storaged startup securely removes only
validated `.cp0-blob-*` staging files left by an interrupted daemon or power
loss; symlinks and malformed entries fail closed.

## v1 migration

If `head.v2` is absent, a writer reads `index.v1`, writes page zero, and commits
the equivalent v2 head before appending. The legacy index and old fourteen-key
frames remain readable. New frames use one blob. Failed migration removes the
uncommitted page and leaves v1 authoritative.

## Backup

The product data partition maps `/var/lib/cardputerzero` into the
`cardputerzero` root of `CP0 backup v1`, so full recovery backup/restore already
includes the shared library. Daily export to a computer is a separate,
read-only Owner Photo Transfer workflow described in `PHOTO-TRANSFER-V1.md`.
