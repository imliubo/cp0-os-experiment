# CardputerZero Photo Library v1 (legacy)

Photo Library v1 used one `index.v1` record with at most 32 photo IDs and
stored every 320x170 RGB565 frame as fourteen 8 KiB storage values. It is kept
only as an on-device migration source.

Photo Library v2 reads an existing v1 index, writes the equivalent v2 page and
head, then appends the new photo. The v2 head is the visibility boundary and
the legacy index and frame values are not removed during migration. See
`PHOTO-LIBRARY-V2.md` for the active contract.
