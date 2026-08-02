# Phase 6D: bounded backup, restore and factory reset

## Scope

Phase 6D provides an offline operator workflow for the complete `cp0-data`
filesystem. It is not an application API, a cloud backup service or an archive
extractor for arbitrary paths. Applications still see only their existing
quota-enforced storage and capability brokers.

The low-level `/usr/bin/cp0-recovery` tool never mounts, formats or deletes a
filesystem. It can create or verify one `CP0 backup v1` file and can restore a
verified file only into an owner-only directory that is empty except for an
empty ext4 `lost+found`. The root-only `device-recovery-data` wrapper owns block
device validation and the explicitly destructive restore/reset operations.

## Format and validation

`CP0 backup v1` is a deterministic, bounded binary stream. It stores a versioned
header, fixed-size entry headers, UTF-8 relative paths, type, mode, UID/GID,
length, one SHA-256 per regular file and a SHA-256 over the complete payload.
The parser rejects:

- absolute paths, `.`/`..`, paths deeper than 32 components and paths outside
  the fixed `cp0-data-layout-v2` top-level allowlist, including the local Owner
  identity database and persistent home;
- duplicate or unsorted paths, unknown types/flags and inconsistent lengths;
- symbolic links, hard links, devices, sockets, setuid/setgid/sticky bits and
  world-writable entries;
- more than 65,536 entries, a file over 4 GiB or a payload over 64 GiB;
- missing layout/profile markers, corruption and any trailing bytes.

Backup reads each file twice from a read-only mount and requires its device,
inode, metadata, length and digest to remain unchanged. Restore verifies the
entire input once before opening the empty target, then validates it again while
streaming the contents. Power loss during restore leaves an incomplete data
filesystem and requires repeating the restore; it never reports partial data as
successful.

## Data sensitivity

A full backup contains the local Owner identity and password hash, installed applications, permission policy, private app
storage, documents, Store trust state, Wi-Fi credentials, SSH host keys, machine
identity and random seed. Version 1 is deliberately offline and provides
corruption detection, not encryption or proof of who created the backup. Store
and transport it only on operator-controlled encrypted media. A hostile party
that can replace both a backup and its hashes can replace persistent policy;
the explicit root recovery ceremony is the trust decision.

## Maintenance profiles

The device wrapper accepts either:

- the independent `recovery` image, whose compositor and application entry
  points are masked; or
- a `product` lower-root maintenance boot with
  `cp0.overlay_root=volatile` removed, so `cp0-data` is not mounted by the
  initramfs.

The normal Settings "Recovery Boot" still uses OverlayFS and does not satisfy
this requirement. The target must be an unmounted real partition 3, ext4,
labelled `cp0-data`, and cannot be the active root filesystem.

## Operations

Verification is non-destructive and does not require root:

```sh
/usr/libexec/cardputerzero/device-recovery-data verify \
    /media/operator/cardputerzero.cp0backup
```

Backup additionally requires a separately mounted output filesystem. The
wrapper refuses to write the backup into the OS root, boot filesystem or source
data partition:

```sh
sudo /usr/libexec/cardputerzero/device-recovery-data backup \
    /dev/mmcblk0p3 /media/operator/cardputerzero.cp0backup
```

Restore first verifies a `profile=product` backup, then requires the exact
confirmation token before it reformats partition 3:

```sh
sudo /usr/libexec/cardputerzero/device-recovery-data restore \
    /dev/mmcblk0p3 /media/operator/cardputerzero.cp0backup \
    RESTORE-CP0-DATA
```

Factory reset is available only from a product lower-root maintenance boot. A
product image generates its factory bundle after installing the default app,
device policy and optional Store trust root. The seed intentionally contains no
machine ID, random seed, network profiles or SSH host keys. The enabled
`regenerate_ssh_host_keys.service` and normal first-boot paths create fresh
device identity after reset.

```sh
sudo /usr/libexec/cardputerzero/device-recovery-data factory-reset \
    /dev/mmcblk0p3 RESET-CP0-DATA
```

The independent recovery artifact does not carry a partial product factory
seed: it cannot know the Store root or policy embedded in a separately built
product image. It can verify, back up and restore an attached product data
partition, while factory reset remains bound to the matching product lower
root.

## Acceptance status

Native unit tests cover deterministic round trips, metadata and private data,
corruption/trailing-byte rejection, unsafe source entries and non-empty target
refusal. Static release tests bind verification-before-format, exact confirmation
tokens, block-device invariants, profile gates, image installation and the
absence of a factory seed from recovery images. The AArch64 binary passes ELF
and RELRO checks. The final mounted-rootfs gate also executes the installed
binary against the product factory seed and requires `profile=product`.

The first Product candidate completed the mounted-rootfs/initramfs gate and an
independent ARM64 restore inspection:

```text
artifact:       image_2026-07-31-cardputerzero-os-phase6d-product-cp0-os-dev.img.xz
size:           244888132 bytes
sha256:         d72ce50b465788c710d4e8917b6986ecc86850eec059f9d82aad9b0606b10113
factory bundle: 10852 bytes, 29 entries, 11 files, 8255 data bytes
factory sha256: 8bb2e73e162aa3dab897fbf184b8cd028696962fcfc35f56a1dca165df683352
```

The restored seed retained the default application, device policy and Store
layout while `machine-id`, `random-seed`, network state, NetworkManager
connections and SSH state were empty.

Final acceptance still requires a disposable SD card: create a backup, inspect
and verify it on a second filesystem, restore it after reformatting, reboot into
the product profile, then exercise factory reset and confirm fresh device keys.
Those operations are intentionally not run against the active stability-test
device.
