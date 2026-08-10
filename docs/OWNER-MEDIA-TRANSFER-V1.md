# Owner USB Media Transfer v1

<!-- doc-locale: en -->
> **English** | [简体中文](OWNER-MEDIA-TRANSFER-V1.zh-CN.md)

Owner USB Media Transfer is the product workflow for copying Camera and
Screenshot photos to a computer and importing local music. It uses USB Mass
Storage only for a dedicated exchange image. It never exports a mounted device
partition, the root filesystem, `cp0-data`, an App package, or App-private
storage.

This is an Owner workflow. It is independent of Developer Mode and the Owner
SSH Shell and does not make either one available.

## Owner flow

1. Connect the CardputerZero USB data port to a computer.
2. Open **Settings > Apps & Privacy > USB Media Transfer**.
3. Enter the current Owner password and press Enter.
4. The device creates or validates the exchange image, exports photo copies,
   unmounts the image locally, and then presents `CP0-MEDIA` to the computer.
5. Copy files from `PHOTOS/` for backup. Put supported WAV files in
   `MUSIC/IMPORT/` for import.
6. Eject `CP0-MEDIA` on the computer, then press Enter on the device. The device
   disconnects the USB LUN before checking the FAT32 filesystem and importing
   music.

The trusted Shell does not allow Home, Back, or Tasks to leave the preparing,
connected, or importing states. Unexpected cable removal is recoverable, but
the Owner should still stop transfer on the device so the FAT filesystem is
checked and pending music is imported.

## Non-negotiable storage boundary

The only permitted LUN backing object is:

```text
/var/lib/cardputerzero/usb-media/exchange.img
```

The v1 image is exactly 512 MiB and contains FAT32. The control protocol has
only `get-status`, `start`, and `stop`; none accepts a path, device name, LUN,
mount option, capacity, or ConfigFS attribute.

Before every bind, `cp0-usb-mediad` proves that the backing object:

- has the fixed `exchange.img` name under the fixed exchange directory;
- resolves to that exact canonical path;
- is a regular file, not a symbolic link or block device;
- has exactly the fixed capacity;
- is owned inside the root-only exchange directory.

The daemon creates one ConfigFS `mass_storage.0` function and writes only the
validated canonical image path to `lun.0/file`. `/dev/mmcblk*`, rootfs,
`cp0-data`, `/var/lib/cardputerzero/data`, and caller-selected paths can never
reach the protocol or LUN setup code.

The exchange filesystem is never mounted by Linux while it is connected to the
computer. Device-side mounts use `nodev,nosuid,noexec,umask=0077`; the sequence
is always device mount -> stage/import -> device unmount -> USB bind, or USB
unbind -> filesystem check -> device mount. This prevents concurrent
filesystem mutation by Linux and the USB host.

`exchange.img` is transient and rebuildable. `CP0 backup v1` explicitly
excludes `cardputerzero/usb-media`; it backs up the authoritative photo library
and imported Document Portal files instead. Product images do not pre-seed the
mutable 512 MiB image.

## Exchange layout

```text
CP0-MEDIA/
  README.TXT
  manifest.json
  PHOTOS/
    IMG_<photo-id>.JPG
    SCREEN_<photo-id>.BMP
  MUSIC/
    IMPORT/
    IMPORT-RESULTS.JSON
```

Camera entries are copies of the canonical 1280x720 JPEG originals. Screenshot
RGB565 frames are losslessly encoded as standard 16-bit bitfield BMP files so
macOS, Linux, and Windows can open them without CardputerZero software.
`manifest.json` records source, dimensions, byte length, capture time where
available, and SHA-256. Deleting or editing an item in `PHOTOS/` changes only
the exchange copy; it never deletes or changes the device photo.

Music v1 accepts regular `.wav` files whose names satisfy the Document Portal
contract and whose contents are 48 kHz, stereo, 16-bit PCM. Each file is
bounded by the Document Portal file limit. Symlinks, directories, malformed
RIFF chunks, unsupported formats, unstable files, and excess entries are
rejected.

Accepted music is copied to a root-created temporary file, rechecked for size,
inode and modification stability, `fsync`ed, assigned to the Document Portal
account, and published without overwrite. A name collision becomes
`name (n).wav`. Only after publication is the exchange source removed.
`MUSIC/IMPORT-RESULTS.JSON` reports imported and rejected names and hashes.

## Service and authentication boundary

The System Shell first calls `cp0-provisiond` to verify the current Owner
password using the existing yescrypt hash. The password is zeroized in the
protocol and C client buffers. `cp0-usb-mediad` does not read shadow data.

The media socket is writable only by `cp0-shell` through
`cp0-usb-media-control`, and the daemon independently verifies the peer UID.
Apps, App Runtime, Store, Developer Mode sessions, and the Owner SSH account
cannot call it.

`cp0-usb-mediad` runs as a sandboxed root service because loop mounts,
ConfigFS, LUN binding, and final document ownership require privilege. Its
systemd unit grants only `CAP_SYS_ADMIN` and `CAP_CHOWN`, loop-device access,
the fixed exchange/document directories, and the fixed USB gadget ConfigFS
tree. Photo contents are read through the storaged protocol; the daemon is not
granted direct access to App-private storage or the full `cp0-data` tree.

Boot explicitly loads `dwc2`, `loop`, and `libcomposite` before the service can
start, and the service requires the ConfigFS and persistent data mounts. The
trusted Shell also receives the media-control socket group explicitly in its
systemd unit instead of relying only on account-database supplementary-group
initialization.

## Failure behavior

- A new image is allocated with real storage reservation, formatted, staged,
  checked, and unmounted before USB binding. Insufficient SD space fails before
  exposure; photos are never silently omitted.
- Start checks ConfigFS, an available UDC, and usable loop control before it
  allocates or rewrites the 512 MiB exchange image. The Owner UI reports the
  failing hardware or filesystem stage, while the journal retains the original
  operating-system error.
- Existing exchange filesystems are checked before recovery import. Normal
  stop unbinds the UDC first and checks FAT32 both before and after import.
- Service stop and shutdown perform an emergency unbind, unmount, and FAT32
  check. A later Start recovers pending imports before rebuilding the exchange.
- A bad path, wrong size, symlink, special file, mount-state conflict, failed
  filesystem check, or missing USB device controller fails closed.
- The authoritative photo library and already imported music remain untouched
  if the host corrupts or deletes the exchange filesystem.

## Release gates

1. Obtain and configure a legitimate production USB VID/PID. The current
   Linux Foundation development placeholder `1d6b:0104` is not releasable.
2. On V0.6, verify ConfigFS, `dwc2` peripheral mode, loop-device policy, service
   sandboxing, and clean shutdown with the final kernel/systemd versions.
3. Verify enumeration, read/write, eject, reconnect, malformed FAT, cable pull,
   power loss, full SD, and full exchange behavior on current macOS, Linux, and
   Windows hosts.
4. Round-trip hashes for Camera JPEG, Screenshot BMP, and imported WAV, then
   prove that host deletion never changes originals and recovery backup never
   contains `exchange.img`.
5. Measure preparation/import latency, peak memory, and SD writes on the 512 MB
   CM0 before marking the feature production-ready.
