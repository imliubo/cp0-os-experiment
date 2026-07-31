# Phase 6H: verified-update groundwork

## Scope

This phase implements the parts of ADR 0006 that can be proved without
changing the active CM0 boot chain or writing disposable media. It deliberately
does not enable an OS updater in the product image.

The local implementation contains:

- `cp0-os-update`, a strict parser and policy library for authenticated OS
  release metadata and A/B boot state;
- `schemas/os-release-v1.schema.json`, the release tooling contract;
- `verify-os-release-artifacts.sh`, which binds rootfs, dm-verity hash tree and
  signed FIT bytes to the metadata, then invokes `veritysetup verify`;
- deterministic tests for stale releases, wrong board IDs, incompatible data
  layouts, torn state writes, incomplete health checks, three failed boots and
  100 consecutive interrupted updates.

The fixed board ID is `cardputerzero-cm0-v0.6`. Release sequences are monotonic
and independent from application Store versions. A release carries hashes and
sizes for the rootfs, separate verity hash tree and FIT, plus the verity root
hash, salt, data block count and FIT configuration name. Build metadata in the
SemVer release version is rejected so one visible version cannot describe
different OS bytes.

## Update ceremony

The eventual RAUC integration must perform these steps in order:

1. Verify the RAUC CMS signature using the dedicated offline OS update root.
2. Parse the bounded release metadata and reject the wrong board, a stale
   sequence or an incompatible `cp0-data` layout.
3. Write only the inactive root, verity tree and boot/FIT slot.
4. Re-read every artifact, compare its authenticated size and SHA-256, and run
   `veritysetup verify` over the complete root filesystem.
5. Verify the signed FIT configuration binds the kernel, initramfs, DTB,
   dm-verity root hash and slot identity before making the slot bootable.
6. Stage the inactive slot with three attempts and durably write two complete
   state records.
7. Before every transfer to a pending slot, decrement its remaining attempts,
   write and re-read the new state, then boot. A power loss after selection
   therefore consumes an attempt instead of creating an infinite retry loop.
8. Confirm the slot only after compositor, appd and `cp0-data` mount health all
   succeed. A fourth selection after three unconfirmed attempts clears pending
   state and boots the last confirmed slot.

Each state record contains a generation and a domain-separated SHA-256
checksum. On recovery, the boot chooser selects the highest valid generation;
two different valid records with the same generation fail closed. One torn or
corrupt copy may be ignored. The checksum detects accidental corruption and
torn writes only. It is not a MAC, signature, anti-replay counter or physical
attacker defense.

## Local verification

Run the policy and rollback tests with:

```sh
cargo test -p cp0-os-update
./tests/test-os-update-profile.sh
```

Given artifacts produced by a Linux verity build, run the real offline gate:

```sh
./scripts/verify-os-release-artifacts.sh \
  release.json rootfs.img rootfs.verity slot.itb
```

The verifier requires `jq`, a SHA-256 implementation and `veritysetup`. A pass
means the bytes match the already authenticated metadata and the verity tree is
internally valid. It does not verify RAUC CMS, the FIT signature or the key
chain that authorizes either envelope.

## Deferred hardware boundary

The current product and recovery images retain their existing three-partition
layout, mutable Raspberry Pi firmware/FAT boot path and OverlayFS lower root.
They contain no RAUC service, boot chooser, dm-verity mapping, U-Boot or signed
FIT. Nothing in this phase changes their security claim.

Before integration, disposable media and a spare board must establish the
exact CM0 firmware-to-U-Boot trust path, boot partition/layout constraints,
redundant state storage, FIT algorithm and key injection procedure. Hardware
acceptance must then cover invalid CMS/FIT signatures, stale sequence, wrong
board ID, corrupted root/hash tree, 100 power cuts, automatic fallback,
`cp0-data` retention, key rotation and the 512 MB peak-memory budget. No OTP or
irreversible key state may be written on the only V0.6 device.
