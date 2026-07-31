# Phase 6A: immutable-root prototype and write controls

## Boot model

The image contains an opt-in initramfs profile selected only by the exact kernel
argument `cp0.overlay_root=volatile`. The initramfs moves the ext4 root to a
private lower mount, remounts it read-only, creates a root-owned 64 MiB tmpfs
with 16,384 inodes, and mounts OverlayFS as the runtime root. Any failure after
the argument is selected enters the initramfs failure path rather than silently
booting a writable root.

After switch-root, diagnostics are available only to root below:

```text
/run/cardputerzero-root/lower     ext4, read-only
/run/cardputerzero-root/volatile  tmpfs, nodev, 64 MiB maximum
```

`cardputerzero-overlay-root-status.service` validates all three mount contracts
at boot. `device-smoke.sh` repeats the check during hardware acceptance.

## Deliberately opt-in

The build does not append the kernel argument. A volatile root loses every
change at reboot, including installed applications, permission decisions,
private application data, SSH host keys and Wi-Fi changes. It must therefore be
used only for immutable-boot validation after `rpi-resize.service` has completed.
Enabling it on the first boot would also prevent online root expansion.

Product enablement requires a separate persistent data partition for at least
`/var/lib/cardputerzero`, trust policy, machine identity, SSH host keys and
NetworkManager connection state. That partition needs its own size limit,
mount options, fsck/recovery policy and migration path. Keeping the prototype
off by default avoids presenting volatile application state as a finished
security property.

To validate a future image, add the argument to the single line in
`/boot/firmware/cmdline.txt`, reboot, then run:

```sh
systemctl status cardputerzero-overlay-root-status.service
findmnt / /run/cardputerzero-root/lower /run/cardputerzero-root/volatile
sudo /usr/libexec/cardputerzero/device-smoke.sh
```

Removing the argument returns to the normal writable root on the next boot; no
lower filesystem data is modified while the profile is active.

## SD-card and security controls

Both normal and overlay modes keep journald in `/run`, use zram without
writeback, disable apt timers, and mount `/tmp` and `/var/tmp` as bounded tmpfs.
The system profile enables protected FIFO/regular-file/symlink/hardlink rules,
disables core dumps to storage, restricts kernel pointer and dmesg visibility,
and disables unprivileged BPF.

Compositor, System Shell and appd now have explicit capability bounds, native
syscall architecture, namespace/proc/kernel protections, no writable-executable
memory and zero swap for the Shell/appd workloads. Hardware brokers retain only
their pre-existing fixed device access.

The 24-hour RAM-backed stability monitor records SD sectors written at each
sample. Its default acceptance limit is 64 MiB per run, reported in
`block-io.tsv` and `summary.env`; exceeding the limit is a test failure.
