# CardputerZero V0.6 first-boot device test report

<!-- doc-locale: en -->
> **English** | [简体中文](FIRST-BOOT-DEVICE-TEST-REPORT-20260803.zh-CN.md)

> Historical diagnostic report. The one-boot maintenance SSH and hot-update
> mechanism used for this run was removed after Setup completed successfully.
> Current product images expose SSH only after completion and explicit consent.

## Test identity

- Date: 2026-08-03, Asia/Shanghai
- Device: CardputerZero V0.6, Raspberry Pi CM0, 512 MiB RAM
- Device address: `192.168.20.66`, wired test connection
- Display verification: operator observation; no local camera was available
- Base media: earlier production candidate with a one-boot volatile
  `cp0-provisiond` hot update
- Candidate source: `d12383c` (`Support hot updates from noexec runtime`)

Status meanings:

- `PASS`: exercised on the V0.6 device with service or durable-state readback.
- `FIXED / IMAGE PENDING`: the device exposed the defect, the source fix and
  host regression pass, but the revised path still needs a fresh-media run.
- `NOT EXECUTED`: requires a physical reboot or newly burned image.

## Outcome

The Device Name failure was reproduced and traced to the Raspberry Pi OS
layout `/etc/default/locale -> ../locale.conf`. The provisioning daemon rejected
the destination because it was a symbolic link. The corrected implementation
atomically replaces the link itself and never follows or modifies its former
target. With the corrected AArch64 daemon hot-updated, the device completed the
region, owner, password, wired-network and SSH-consent steps and reached Review.

Final commit exposed a second production-layout defect. `ssh.service` is gated
on both the completion marker and SSH consent, but the old transaction started
SSH before writing the completion marker. systemd therefore skipped the start,
then maintenance SSH was stopped. Source now writes `COMMITTING`, installs the
completion marker, activates SSH, and only then writes `COMPLETE`. Recovery
tests cover interruption on both sides of the marker transaction. This revised
ordering is included in the new image but cannot be claimed as a device pass
until that image is burned and completed once.

## Device test matrix

| Function | Result | Device observation |
| --- | --- | --- |
| Maintenance SSH | PASS | Root maintenance access accepted the image-specific ED25519 key before commit. |
| Device Name / region | PASS | Hot-updated daemon advanced `unprovisioned -> owner`. |
| Hostname, locale and time zone | PASS | Durable hostname, locale, timezone and localtime content matched the request. |
| Locale symlink handling | PASS | Link was replaced without writing through to its original target. |
| Wi-Fi scan | PASS | 13 physical access points returned with Open, WPA2 and WPA3 classification. |
| Owner creation | PASS | Owner identity and persistent home were created. |
| Password | PASS | A password longer than ten characters was accepted and stored as a yescrypt hash. |
| Ethernet selection | PASS | Setup reported wired address `192.168.20.66`. |
| SSH consent | PASS | Explicit SSH On choice persisted. |
| Review state | PASS | Durable provisioning phase reached `review`. |
| Final completion markers | PASS | Completion and SSH-enabled markers persisted on the current SD card. |
| Immediate owner SSH after commit | FIXED / IMAGE PENDING | Port 22 returned `Connection refused`; old ordering started sshd before the completion marker existed. |
| Owner SSH after cold boot | NOT EXECUTED | Requires one physical restart of the current card or a full run on the new image. |
| LCD wizard completion to Home | NOT EXECUTED | No camera was available for independent visual capture. |

The temporary owner on this debug SD card is `cp0test`. It is runtime test data
and is not embedded in the product image. No fixed human account or password is
present in the candidate image.

## Defects fixed

1. Safe conventional locale symlinks are now replaced atomically rather than
   rejected. A regression test proves the old target remains unchanged.
2. Final commit now publishes the completion marker before starting gated SSH,
   while retaining `COMMITTING` crash recovery.
3. One-boot hot updates staged below `/run` no longer depend on the executable
   bit being usable on a `noexec` mount. Validation requires a regular,
   non-symlink, non-empty, root-owned mode `0700` file before copying it into the
   executable volatile overlay.

## Regression and image results

- `cargo test -p cp0-provisiond`: PASS, 13 tests
- `tests/test-maintenance-access.sh`: PASS
- `tests/test-appd-profile.sh`: PASS
- full `make check`: PASS
- `git diff --check`: PASS
- product rootfs and initramfs profile gate: PASS
- compressed-image checksum verification: PASS

Candidate image:

```text
deploy/image_2026-08-03-firstboot-stable-d12383c-cp0-os-production.img.xz
size: 253220648 bytes
sha256: 40aae6933d22bc64a3697a568d11e0c6edbe65b6ee792f74851064140934733d
```

## Remaining fresh-media acceptance

1. Burn the candidate image and complete all Setup pages from Welcome without a
   hot update.
2. Confirm held Shift produces uppercase input on the physical keyboard.
3. Exercise Ethernet, Wi-Fi and offline setup paths independently.
4. Complete with SSH On, verify owner authentication, and prove root and
   application identities are rejected.
5. Cold boot with SSH Off and prove no listener or reusable access path exists.
6. Interrupt power during each durable phase and both sides of final commit,
   then confirm deterministic resume without a second owner.
7. Confirm completion transitions to Home and ten subsequent cold boots do not
   reopen Setup.
