# Phase 6C: independent recovery image profile

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE6C-RECOVERY-IMAGE.zh-CN.md)

## Purpose

The recovery image is a separate maintenance artifact, not an alternate desktop
and not a less secure product mode. It uses the same pinned BSP and audited
binaries as the product image, but deliberately boots a writable lower root to
`tty1`. This gives an operator a predictable local keyboard and SSH environment
when the product compositor or immutable-root path cannot start.

Every built root filesystem contains exactly one root-owned marker:

```text
/etc/cardputerzero/image-profile
```

Its value is `product` or `recovery`. The profile controls the kernel command
line, enabled units, artifact suffix, LCD banner and final mounted-rootfs release
gate. A filename alone is never trusted as the profile authority.

## Build

`product` remains the default. Build a recovery artifact explicitly with a
different container name so an interrupted product build cannot be mistaken for
a resumable recovery build:

```sh
CP0_IMAGE_PROFILE=recovery \
CP0_BUILD_CONTAINER=cardputerzero-pigen-recovery \
CP0_FIRST_USER_PASSWORD='one-time-maintenance-password' \
CP0_SSH_PUBLIC_KEY='ssh-ed25519 ...' \
./image/build-image.sh
```

The result uses the suffix `-cp0-os-recovery.img.xz`. A recovery build rejects
`CP0_STORE_PUBLIC_KEY`; maintenance media must not become a Store trust root.
The build still requires an explicit local login password. Providing an SSH key
also makes remote maintenance use the operator-controlled key instead of shared
credentials.

## Boot contract

The recovery profile has these fail-closed differences from the product image:

- `cp0.overlay_root=volatile` is absent, so the root filesystem is writable;
- `getty@tty1`, the LCD boot summary, NetworkManager and SSH remain available;
- the LCD heading is `CardputerZero OS RECOVERY`;
- compositor and System Shell are masked;
- appd and every capability/Store activation socket are masked;
- `cp0-data` is neither grown nor mounted by the initramfs.

The last point prevents recovery boot from silently binding mutable application,
identity or network state into the maintenance root. Root can inspect or mount a
target only through an explicit, reviewed recovery procedure. The included
three-partition layout is retained so the image exporter and partition parser
remain identical, but its data partition is inert in this profile.

## Release gate

The final pi-gen verifier reads the profile marker from the mounted ext4 root.
For a product image it requires the immutable-root argument, compositor, seatd,
recovery selector and all broker sockets. For a recovery image it requires that
argument to be absent, `tty1` enabled and every application execution entry
point linked to `/dev/null`. Both profiles retain the same package, initramfs,
world-writable-file and three-partition checks. The copied marker in the seeded
data partition must match the lower root.

Repository tests also reject unknown `CP0_IMAGE_PROFILE` values before any
clone, package build or Docker action, verify profile-specific artifact names,
and scan the recovery branches for accidental compositor/appd activation.

The first reproducible recovery candidate completed the mounted-rootfs and
initramfs gate with `PASS built rootfs and initramfs profile: recovery`:

```text
artifact: image_2026-07-31-cardputerzero-os-cp0-os-recovery.img.xz
size:     243974280 bytes
sha256:   2895e90f592c4e9c892873eb328f097e6d45598d15cff95b6f7c4b1c59746d92
```

## Operational limitation

Flashing this image overwrites the selected SD card. It cannot back up the same
card that it replaces. User data must therefore be exported from the running
product recovery console before reflashing, or the original card must be read on
a separate trusted computer. The audited bounded backup/restore format and
factory-reset workflow are defined in `docs/PHASE6D-RECOVERY-DATA.md`; they do
not treat an unrestricted `tar` command as a recovery protocol.

Final acceptance requires building the compressed artifact, flashing a separate
SD card with user assistance, confirming the LCD recovery banner and keyboard,
and proving that compositor/appd sockets remain absent after reboot.
