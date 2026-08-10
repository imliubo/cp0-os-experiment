# Owner Photo Transfer v1

<!-- doc-locale: en -->
> **English** | [简体中文](PHOTO-TRANSFER-V1.zh-CN.md)

This document records the original photo-only design. The authoritative
implemented transport and security contract is now
`OWNER-MEDIA-TRANSFER-V1.md`: a password-gated, isolated FAT32 USB exchange
image that adds atomic music import while preserving read-only photo export.

Photo transfer is an Owner data workflow, not an App capability and not a
Developer Mode feature. It must not require or enable the full Owner SSH Shell.

## Two backup layers

`CP0 backup v1` remains the complete disaster-recovery path. It includes the
entire persistent `cardputerzero` tree, including photo blobs and indexes, and
is restored only through the recovery workflow.

Owner Photo Transfer is the daily path for copying selected or all photos to a
computer. Its v1 scope is export-only. A remote peer cannot delete photos,
modify indexes, upload frames, restore a backup or install Apps.

## Device flow

The Owner opens **Settings > Apps & Privacy > USB Media Transfer**, confirms
the current password, copies files from `CP0-MEDIA/PHOTOS`, ejects the drive on
the computer, and stops transfer on the device. The host sees export copies,
not the live Photo Library. Developer Mode and Owner SSH stay independent.

## Wire and file contract

- IDs and counts are unsigned 64-bit values; list pages contain at most 64
  entries.
- A snapshot has a random token, creation time, item count and ordered IDs.
- Each item exposes ID, kind (`camera` or `screenshot` when metadata v3 adds
  provenance), dimensions, pixel format, byte length and SHA-256.
- Pull requests specify snapshot token, photo ID and byte offset. Responses are
  bounded chunks, enabling reconnect/resume without buffering a full library.
- The default output is `YYYYMMDD-HHMMSS-<id>.png` plus `manifest.json`.
- A lossless `.cp0photos` archive contains the canonical raw frames, versioned
  metadata and hashes. Restore/import is deliberately outside v1.

Deletion during an active snapshot is serialized by appd. A completed item is
hash-stable; a deleted, not-yet-read item returns a specific stale-snapshot
error and the computer refreshes the manifest. No transfer lease may prevent
the Owner from using Camera or Gallery indefinitely.

## Implementation gates

1. Extend Photo Library metadata with provenance and capture time without
   rewriting frame blobs.
2. Add a root `cp0-photod` read-only broker and bounded protocol; it receives
   frames from storaged and never accepts filesystem paths.
3. Add separate Owner pairing UI, key registry, forced-command dispatcher and
   immediate revocation.
4. Add `cp0ctl photos pair/list/pull/verify`, PNG conversion and resumable
   `.cp0photos` output.
5. Verify authorization separation, snapshot races, disconnect/resume,
   corrupted chunks, SD removal/full behavior and 1/100/10,000-photo memory
   bounds on V0.6.

Exporting a live device partition by USB Mass Storage remains prohibited. The
implemented MSC path exposes only a fixed, rebuildable image which Linux
unmounts before host access. It cannot select or export rootfs, `cp0-data`, App
private data, or any block device.
