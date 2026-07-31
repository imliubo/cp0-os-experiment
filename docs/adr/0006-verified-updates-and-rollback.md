# ADR 0006: verified OS updates and rollback

- Status: accepted architecture; hardware enablement deferred
- Date: 2026-07-31

## Context

The current three-partition image has one FAT boot partition, one ext4 lower
root and one ext4 `cp0-data` partition. OverlayFS protects the lower root from
ordinary runtime writes, but an SD-card attacker can replace the firmware
configuration, kernel, initramfs or lower root. A dm-verity root hash stored in
that same mutable boot partition would detect random corruption but would not
authenticate the image.

The system also has no transactional OS updater. Application Store rollback is
unrelated to kernel/rootfs rollback and must not be reused as an OS trust root.

## Decision

The eventual production update design is a signed A/B system:

1. Use separate boot/root slot pairs plus the existing shared `cp0-data`
   partition. An update always writes the inactive slot and never mutates the
   running slot.
2. Protect each root slot with dm-verity. Its root hash is carried in signed
   boot metadata, not in an independently mutable command line.
3. Use RAUC bundles signed by a dedicated offline OS-update key. The Store key
   cannot authorize OS images, recovery media or boot metadata.
4. Mark a new slot pending, require the signed boot context to identify that
   exact slot and release sequence plus compositor/appd/data-mount health
   checks, then commit it. A bounded boot-attempt counter reverts to the
   previous slot when the health ceremony does not finish.
5. Keep `cp0-data` outside slot replacement. Schema migrations must be
   forward-compatible with the previous OS slot or stage a separately
   recoverable data migration before the new slot is committed.
6. Keep the independent recovery image and offline `CP0 backup v1` workflow.
   Recovery media does not inherit the online OS update key.

U-Boot with signed FIT images is a candidate implementation for signed boot
metadata and rollback state, not a trust anchor by itself. Raspberry Pi
firmware still loads U-Boot from the mutable FAT partition. U-Boot/FIT signing
therefore provides authenticity only if an earlier immutable hardware or
firmware root verifies that first mutable stage.

No OTP, secure-boot key or irreversible hardware state will be programmed on
the only V0.6 test device. Hardware enablement requires a disposable board or
recoverable vendor procedure, confirmation that the BCM2710-based CM0 boot path
supports the required root of trust, and a proven key-loss recovery process.

## Rejected shortcuts

- OverlayFS alone: prevents persistent writes during normal operation but does
  not detect offline modification.
- dm-verity with an unsigned root hash: useful for corruption detection, not
  authenticity against an SD attacker.
- RAUC with one root slot: authenticates a bundle but cannot guarantee recovery
  from power loss or a bad kernel/userspace combination.
- U-Boot/FIT on mutable FAT without an earlier verified stage: moves rather
  than closes the physical trust gap.
- Reusing the Store signing root: couples application review to full OS control
  and makes revocation blast radius unacceptable.

## Enablement gates

Implementation is deferred until all of these can be tested on disposable
media and, where necessary, a spare board:

- exact V0.6 ROM/firmware secure-boot capabilities are recorded from vendor
  documentation and hardware evidence;
- final partition sizes fit the supported SD-card floor with two complete OS
  slots and bounded RAUC scratch space;
- 100 interrupted-update fault-injection cycles preserve one bootable slot;
- invalid signature, stale version, wrong board ID and corrupted verity tree
  all fail closed;
- automatic rollback survives power loss while retaining `cp0-data`;
- update-key rotation, revocation, backup custody and disaster recovery are
  documented and rehearsed;
- peak updater memory remains compatible with the 512 MB device budget.

Until these gates pass, CardputerZero OS artifacts remain development or
recovery candidates. The project must not claim verified boot or unattended
transactional OS updates.
