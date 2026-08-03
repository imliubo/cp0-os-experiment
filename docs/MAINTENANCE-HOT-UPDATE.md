# One-boot maintenance and first-boot hot updates

## Purpose and boundary

Production images have no shared owner name, password, SSH key or enabled
ordinary SSH listener. Reflashing is still required for boot firmware, DTB,
kernel modules, initramfs, partition layout and persistent lower-root changes.

For first-boot user-space diagnosis, the production image supports a physical,
one-boot maintenance ceremony. An operator writes their own ED25519 public key
and a versioned enable marker to the writable Raspberry Pi boot partition. On
the next boot the root-owned service validates and copies the key into `/run`,
generates an ephemeral host key, validates a dedicated sshd configuration, and
consumes both boot inputs before opening port 22. A reboot closes the path.

This is explicit root maintenance authority. It is intended for development
and field recovery, not ordinary remote administration. It does not protect an
SD card against an attacker with offline write access and it does not replace
verified boot. Password login, forwarding, root passwords, embedded keys and
application access are all disabled in this mode.

## Arm a boot

Power the device off and mount its FAT boot partition on the development host.
Generate a dedicated key if one does not already exist:

```sh
ssh-keygen -t ed25519 -f ~/.ssh/cp0-maintenance
./scripts/enable-maintenance-ssh.sh \
    /path/to/boot-mount ~/.ssh/cp0-maintenance.pub
```

The helper requires `config.txt` and `cmdline.txt`, validates exactly one
ED25519 public key, then atomically writes:

```text
cp0-maintenance.enable
cp0-maintenance.authorized_key
```

Boot the device. Both input files disappear after successful validation. The
remaining `cp0-maintenance.status` records the ephemeral host-key fingerprint,
the IP addresses visible at service start (or `pending`) and the login name
`root`. The image publishes the fixed local discovery name
`cardputerzero-maintenance.local`, independently of the owner-selected device
name. Verify the recorded fingerprint on the first SSH connection:

```sh
ssh -i ~/.ssh/cp0-maintenance root@cardputerzero-maintenance.local
```

The name requires the computer and device to be on the same multicast-capable
LAN. If a router isolates wired and wireless clients, use the IP recorded in
`cp0-maintenance.status` or the router's DHCP lease list instead.

If preparation fails, port 22 remains closed and the input files are retained
for inspection. The service journal uses the unit name
`cardputerzero-maintenance-ssh.service` and never logs private key material.

## Hot-update first boot

Build AArch64 `cp0-provisiond` and System Shell artifacts with the existing
image builders. Verify both are AArch64 executables, then run:

```sh
CP0_SSH_IDENTITY=~/.ssh/cp0-maintenance \
./scripts/device-hot-update-firstboot.sh DEVICE_IP \
    target/aarch64-unknown-linux-gnu/release/cp0-provisiond \
    target/compositor-aarch64/cardputerzero-system-shell
```

The helper uploads only to `/run`. The device-side activator backs up both
installed binaries in `/run/cp0-hot-update-backup`, stops the socket-activated
daemon, replaces both files in the volatile root overlay, restarts the daemon
socket and Shell, and checks that both units remain active. A failed activation
restores both binaries and restarts the previous version.

The `/run` filesystem remains mounted `noexec`. Staged artifacts are validated
as non-empty, root-owned regular files with mode `0700`, then copied into the
executable volatile overlay; the update path does not weaken the runtime mount.

The update lasts only for the current boot because the production root overlay
is RAM-backed. This is deliberate: a tested fix must still enter source
control, pass the full host and mounted-image gates, and be delivered in a new
image. Do not use this path for kernel/BSP changes or as an untracked permanent
installation mechanism.

## Diagnostic sequence

Before replacing files, capture these non-secret facts:

```sh
systemctl status cardputerzero-provisiond.service \
    cardputerzero-system-shell.service --no-pager
journalctl -b -u cardputerzero-provisiond.service \
    -u cardputerzero-system-shell.service --no-pager
cat /var/lib/cardputerzero/provisioning/state.json
```

The provisioning daemon logs request ID, command name, current phase and the
internal error. Password and Wi-Fi secret fields are never included. After a
hot update, restart the wizard from its durable phase rather than deleting or
editing state manually.
