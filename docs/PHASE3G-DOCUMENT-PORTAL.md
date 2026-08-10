# Phase 3G: Restricted Document Portal

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE3G-DOCUMENT-PORTAL.zh-CN.md)

## Scope

The first Document Portal implementation lets a WASM application request one
user-selected, read-only document without exposing a host path, directory API or
WASI filesystem. The application must declare `documents.open` and receives only
an opaque Runtime handle plus the bounded file length.

The initial contract is deliberately small:

- documents live under `/var/lib/cardputerzero/documents`;
- the trusted System Shell displays at most 16 direct regular files;
- one document can be active in a Runtime at a time;
- a document is at most 256 MiB;
- each SDK read is at most 4096 bytes and uses an explicit 64-bit offset;
- applications cannot choose or submit a path or document ID.

## Trust Flow

```text
WASM cp0_document_open
  -> Runtime sends open-document with no path or identity
  -> appd verifies peer UID, systemd cgroup, manifest and permission
  -> cp0-documentd returns a bounded opaque-ID/name snapshot
  -> trusted System Shell renders the single foreground file picker
  -> Shell resolves only an ID present in that snapshot
  -> cp0-documentd opens the direct child with openat(O_NOFOLLOW)
  -> cp0-documentd verifies the opened device/inode, type and size
  -> descriptor crosses documentd -> appd -> Runtime with SCM_RIGHTS
  -> Runtime verifies O_RDONLY, regular-file type and exact bounded size
  -> WASM reads through a validated pointer/length host call
```

`cp0-documentd` runs as the dedicated `cp0-document` account. Its systemd unit
has an empty capability set, an empty device view, a strict read-only system view
and only `AF_UNIX`. Its socket is `0600 root:root`, so only appd can call it.
Conversely, appd has no DAC capability and the document root is `0750` and owned
by `cp0-document`; appd receives a descriptor but is not granted directory
traversal.

## Race And Escape Resistance

The document ID is the fixed-width lowercase hexadecimal device/inode identity,
not a filename. The service enumerates only direct UTF-8 names and rejects
slashes, control characters, directories, symbolic links, oversized files and
duplicate hard-link identities. Opening uses a non-following directory FD plus
`openat(O_RDONLY|O_CLOEXEC|O_NOFOLLOW)`, followed by `fstat`; the opened
device/inode must still match the selected ID. A rename, replacement or symlink
swap therefore fails closed.

The Runtime keeps the received FD private. WASM sees a generation handle and can
only call `open`, bounded `read` and `close`; it cannot invoke `read(2)`, duplicate
the descriptor or discover the host path. A second successful open closes the
previous descriptor, and stale handles are rejected.

## Verification

Automated coverage includes:

- strict 4 KiB protocols and exact one-FD `SCM_RIGHTS` transfer;
- symbolic-link, forged-ID and post-selection replacement rejection;
- read-only descriptor and device/inode checks;
- trusted prompt snapshot selection and cancellation;
- Shell JSON parsing, keyboard state, scrolling and pixel snapshot regression;
- Runtime stale-handle, EOF, offset and 4096-byte read bounds;
- Rust, C11, C++17 and WIT SDK surfaces;
- hardened service and image-stage assertions.

The implementation is locally verified and included in future image builds. It
is intentionally not hot-deployed while the Phase 2 24-hour stability run is in
progress. Final physical selection and document-content acceptance belongs to
the next integrated-image hardware pass.
