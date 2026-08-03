# Phase 6G: production access profile

> The first-boot owner model in `FIRST-BOOT-PROVISIONING.md` and ADR 0007 is the
> current production access contract.

## Boundary

The normal `product:development` image intentionally enables unrestricted
operator SSH and accepts an operator-selected password so the only V0.6 board
remains debuggable. That is a development artifact and must never be
redistributed as a production image.

`CP0_ACCESS_PROFILE=production` creates a distinct product artifact whose
network maintenance boundary is closed by default:

- the build rejects `CP0_FIRST_USER_PASSWORD` and `CP0_SSH_PUBLIC_KEY`;
- pi-gen receives a generated 256-bit temporary password only because its stage
  contract requires one; the exported account is locked and uses `nologin`;
- the first owner retains no sudo, device, network or diagnostic group;
- SSH and local/serial getty units are unavailable during Setup;
- the home directory contains no SSH authorization directory;
- root remains locked;
- Recovery Boot is locked off by the root-owned production device policy;
- Developer Mode is Off by default but may be physically enabled by the owner;
- Developer Mode opens only the constrained, signed-App deployment channel;
- the independent Owner SSH Shell option remains Off by default.

The product image contains no conditional maintenance sshd, boot-partition
enable marker, pre-Setup authorized-key path or first-boot hot updater. Writing
files to the FAT boot partition cannot open a network login path. SSH is
generated and gated only after provisioning is complete and either Developer
Mode or the independent Owner SSH Shell setting is On. The owner dispatcher
routes `cp0-dev` only to the constrained daemon; Bash additionally requires the
`cp0-ssh` login group granted by Owner SSH Shell. Root login and SSH forwarding
remain prohibited.

NetworkManager, the compositor, System Shell, appd and the capability brokers
remain available. This access profile changes maintenance authority, not the
application platform or user workflow.

## Build

Build a production-access product image without passing a password or SSH key:

```sh
CP0_ACCESS_PROFILE=production \
CP0_IMAGE_NAME=cardputerzero-os-release-candidate \
make image
make verify-image
```

The artifact suffix is `-cp0-os-production.img.xz`. The profile marker is
stored as `/etc/cardputerzero/access-profile` in both the lower root and the
seeded `cp0-data` filesystem, so filenames are not trusted as policy.

Development and recovery builds retain their explicit password requirement.
A recovery image cannot use the production access profile. Supplying a shared
password, an SSH key, an unknown access profile or the recovery/production
combination fails before repository access, Docker or image mutation begins.

## Owner development and maintenance

Developer Mode is a supported production workflow for a personally owned
device. It requires physical confirmation in the trusted System Shell, a
separate ten-minute **Pair New Computer** window, an owner password for initial
pairing, Ed25519 forced-command SSH authorization and a paired developer signing
key. It grants install/log/start/stop/uninstall operations for signed SDK Apps,
not Bash or root. Individual and bulk revocation are available on-device. See
`DEVELOPER-ACCESS.md`.

The Owner SSH Shell is a separate, explicit setting. Enabling it permits Bash as
the owner account but still does not grant sudo. It is not required for App
development and is not enabled by Developer Mode.

## Recovery ceremony

Persistent and unrestricted maintenance uses a separately built recovery SD
with an operator-selected one-time password or key. Booting that removable
image is the physical authorization ceremony; removing it revokes access. The
recovery image does not automatically mount `cp0-data`, and all product
application entry points remain masked there. There is no boot-marker
maintenance mode in a product image.

This design avoids placing a fleet-wide or per-release login secret in the
product image. It does not protect an SD card from offline replacement and does
not create a hardware root of trust. dm-verity, signed boot metadata and A/B
rollback remain governed by ADR 0006.

## Release gate

The mounted-rootfs verifier rejects a production artifact unless the account,
groups, policy, service masks and persistent profile marker all match this
contract. Repository tests also exercise every invalid parameter combination
before the expensive build path.

Final acceptance still requires booting a production-access artifact on
disposable media and confirming the default closed state, constrained Developer
Mode path, independent Owner SSH Shell behavior, root/sudo denial and all local
login masks. It must not replace the active V0.6 development image before its
RAM-backed stability evidence is retrieved.
