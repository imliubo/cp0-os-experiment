# Store Device Rich Details v1

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-DEVICE-RICH-DETAILS-V1.zh-CN.md)

S6D connects the signed Catalog v3 media hierarchy to the 320x170 System Shell
without exposing private cache paths or placing image bytes in the bounded JSON
frame.

## IPC contract

The Store protocol remains newline-delimited JSON with a 64 KiB frame limit.
Two bounded commands extend protocol version 1:

- `details` returns the Catalog-bound app/version, developer, category, age
  rating, privacy/support links, description, release notes and screenshot
  count. The signed details source remains capped at 16 KiB.
- `media` selects either the icon or one screenshot index. A successful response
  binds app ID, version, media kind, index, SHA-256, encoded byte length and
  dimensions, and carries exactly one read-only descriptor with `SCM_RIGHTS`.

Error and non-media responses must not carry a descriptor. A media success must
carry exactly one. The receiver rejects missing, duplicate or truncated control
data, enables `FD_CLOEXEC`, requires a read-only regular file, and checks the
descriptor length against the response before decoding. Cache filesystem paths
never cross the IPC boundary.

`cp0-stored` downloads or reuses the content-addressed object, revalidates its
signed digest, length and PNG structure, then opens the final `0600` inode with
`O_NOFOLLOW | O_CLOEXEC`. The response metadata is produced from the same
signed Catalog/details descriptor.

## Shell rendering

The Store detail view has five single-foreground pages:

1. icon, identity, developer/category/age and install state;
2. word-wrapped, vertically scrollable description;
3. one complete 320:170 screenshot at a time with bounded index navigation;
4. requested permissions and update-time permission additions;
5. word-wrapped, vertically scrollable release notes.

The C client requires every details/media response to match the app ID and
version currently selected by the UI. libpng decodes into fixed XRGB/alpha
pixel buffers only after descriptor and dimension checks. `struct cp0_ui`
remains below 64 KiB; the Shell owns one 48x48 icon buffer and one 320x170
screenshot buffer, about 222 KiB total, under its existing 32 MiB service
limit.

Catalog/list/install behavior remains usable when rich details or media fail.
Legacy Catalog v1/v2 entries therefore retain the existing basic overview and
show an unavailable rich-details state rather than weakening validation.

## Verification

Rust tests cover strict request/response validation, descriptor transfer,
read-only media opening and cached byte identity. C tests cover extra fields,
app/version/index/dimension mismatches, truncated or duplicate descriptors,
`FD_CLOEXEC`, real PNG decoding and fixed pixel output. System Shell tests cover
all five pages, detail scrolling, screenshot navigation, stale Catalog install
blocking and the 64 KiB UI-state budget with pixel snapshots.
