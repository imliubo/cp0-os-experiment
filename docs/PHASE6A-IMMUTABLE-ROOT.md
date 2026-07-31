# Phase 6A: immutable root and persistent data

## Image layout

Product images use an MBR partition table with three aligned partitions:

| Partition | Initial size | Filesystem | Purpose |
|---|---:|---|---|
| bootfs | 512 MiB | FAT32 | Firmware, kernel, DTB and `initramfs8` |
| rootfs | calculated | ext4 | Immutable OS lower filesystem |
| cp0-data | 256 MiB | ext4 | Mutable identity, policy and application state |

`cp0-data` is present and valid in the image before it is flashed. On each boot,
the initramfs verifies that it is partition 3, is the last partition, and belongs
to `/dev/mmcblk0`. It expands the partition to the end of the SD card and runs
offline `e2fsck` followed by `resize2fs`. This is idempotent. If partition-table
growth is interrupted after the MBR update, the next boot observes the larger
partition and completes filesystem growth. If growth cannot be attempted, the
original 256 MiB filesystem remains usable and growth is retried next boot.

The upstream `resize` kernel argument and `rpi-resize.service` are disabled so
they cannot expand partition 2 across partition 3.

## Boot sequence

The image contains the exact kernel argument `cp0.overlay_root=volatile` by
default. The initramfs performs these operations before PID 1 starts:

1. Validate and grow `cp0-data` while it is unmounted.
2. Move the mounted ext4 root to a private lower mount and remount it read-only.
3. Create a root-owned 64 MiB tmpfs upper with 16,384 inodes.
4. Mount OverlayFS as `/`.
5. Mount `cp0-data` read-write with `nodev,nosuid,noexec,noatime`.
6. Generate a per-device machine ID on `cp0-data` if one does not exist.
7. Bind only the approved persistent directories and files into the new root.
8. Move all diagnostic mounts with the initramfs `/run` into the final root.

Any missing label, duplicate label, invalid layout marker, mount failure or bind
failure enters the initramfs panic path. It never silently boots the product
profile with a writable root or without its persistent security state.

After switch-root, root-only diagnostics are available at:

```text
/run/cardputerzero-root/lower     ext4, read-only
/run/cardputerzero-root/volatile  tmpfs, nodev, 64 MiB maximum
/run/cardputerzero-data           ext4, rw,nodev,nosuid,noexec
```

Removing `cp0.overlay_root=volatile` from `cmdline.txt` is the explicit
lower-root service recovery path. It boots the lower root read-write and does
not mount or bind `cp0-data`. This is distinct from the Settings "Recovery
Boot", which keeps the immutable/persistent layout and only selects the tty1
console instead of the compositor.

## Persistent allowlist

The exported image seeds a versioned `cp0-data-layout-v1` layout. Only these
paths survive reboot:

| Persistent source | Runtime path |
|---|---|
| `cardputerzero/` | `/var/lib/cardputerzero` |
| `etc-cardputerzero/` | `/etc/cardputerzero` |
| `ssh/` | `/etc/ssh` |
| `network-connections/` | `/etc/NetworkManager/system-connections` |
| `network-state/` | `/var/lib/NetworkManager` |
| `machine-id` | `/etc/machine-id` (read-only bind) |
| `random-seed` | `/var/lib/systemd/random-seed` |

This covers installed applications, the registry and permission decisions,
private app data, trust/revocation policy, LoRa policy, Wi-Fi credentials,
NetworkManager state, SSH host keys, machine identity and the random seed.
Everything else written to `/etc` or `/var` is discarded on reboot.

The data filesystem root is mode `0700`. Applications do not receive its mount
path. Their only writable persistent interface remains the quota-enforced
storage broker; appd and brokers retain their existing systemd path allowlists.

## Verification

`tests/test-built-rootfs-profile.sh` is injected into pi-gen's finalise stage,
after final initramfs generation and immediately before the image is unmounted
and compressed. It verifies the actual mounted boot, root and data filesystems,
enabled units, persistent seed, default kernel arguments, proxy removal and
exact initramfs entries.

The integrated development candidate is:

```text
deploy/image_2026-07-31-cardputerzero-os-d19d1ca-cp0-os-dev.img.xz
SHA-256 e965d4dc6b9d42bb03a37e70ea700c7e128b5a10c15ddb54f8a91cb20e448c05
```

It is 244,050,632 bytes compressed and 2,097,152,000 bytes uncompressed.
Independent read-only inspection confirmed an MBR containing 512 MiB
`bootfs`, 1,283,457,024-byte `rootfs` and 256 MiB `cp0-data` partitions.
The root filesystem uses about 770 MiB, bootfs uses about 49 MiB and the seeded
data filesystem uses less than 1 MiB. The compressed stream, package profile,
filesystem labels, default command line, persistent layout and required
initramfs files all passed their release checks.

The partition grow script is also tested against a privileged loopback MBR:

- partition 3 and its ext4 filesystem grew from 64 MiB to 344 MiB;
- the resulting block device and filesystem sizes were identical;
- a second invocation was a no-op;
- passing partition 2 was rejected before filesystem access.

Final acceptance still requires flashing the integrated image, verifying the
first boot on V0.6, reboot persistence, interrupted-write recovery and the full
24-hour SD write budget. Until those hardware checks pass, the corresponding
Roadmap acceptance item remains open.

## Write and service controls

journald, `/tmp`, `/var/tmp` and stability reports remain in RAM. zram has no
writeback, apt timers are disabled, and the 24-hour monitor rejects more than
64 MiB of SD writes by default. Kernel sysctls restrict unsafe link behavior,
core dumps, dmesg, kernel pointers and unprivileged BPF.

Compositor, System Shell and appd use explicit capability bounds, native syscall
architecture, namespace/proc/kernel protections, no writable-executable memory
and zero swap. Hardware brokers retain only their fixed device access.
