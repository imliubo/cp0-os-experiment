# Owner Photo Transfer v1

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

1. The Owner opens **Gallery > Transfer** or **Settings > Photo Transfer** and
   confirms the device password.
2. The device opens a volatile ten-minute pairing/transfer window and displays
   a short code plus the device name and address.
3. `cp0ctl photos pair` creates a dedicated Ed25519 photo-transfer key. This key
   is stored separately from developer deployment and Owner Shell keys.
4. `cp0ctl photos list` obtains a paginated, immutable snapshot manifest.
5. `cp0ctl photos pull <destination>` resumes missing items, verifies SHA-256,
   converts RGB565 frames to PNG on the computer and writes the manifest last.
6. Closing the UI or reaching the timeout blocks new sessions. The Owner can
   revoke one computer or all photo-transfer computers from Settings.

The transport may use the existing OpenSSH listener, but every authorized key
must carry `restrict,command="/usr/bin/cp0ctl photo-session"`. The forced
command accepts only the bounded photo protocol and remains available
independently of Developer Mode. Port forwarding, PTY, agent forwarding, file
upload, arbitrary paths and shell commands stay disabled.

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

USB PTP/MTP can be added later as another front end to `cp0-photod`. USB Mass
Storage is excluded because exporting the live data filesystem would allow the
computer and device to mutate one filesystem concurrently.
