# Power control

<!-- doc-locale: en -->
> **English** | [简体中文](POWER-CONTROL.zh-CN.md)

## Contract

Restart and Power Off are trusted global actions. The System Shell keeps the
existing confirmation UI, then sends one bounded request to the root-owned
cp0-powerd service. Applications, appd, the SDK, the owner account and the
developer channel have no power-control endpoint.

The version 1 protocol accepts exactly two commands:

- restart;
- power-off.

Requests and responses are newline-delimited, strict JSON frames bounded to
1024 bytes. A successful response binds the request ID and accepted action.

## Privilege boundary

cardputerzero-powerd.socket is mode 0660, owned by root and the
cp0-power-control group. Only cp0-shell is added to that group. The daemon
still authenticates every connection with Linux SO_PEERCRED and accepts only
the exact cp0-shell UID, so group membership alone does not grant authority.

cp0-powerd runs as root with an empty capability bounding set,
NoNewPrivileges=yes, ProtectSystem=strict and AF_UNIX as its only address
family. It has no generic command, unit-name, argument, path or environment
field. The backend maps the closed action enum to one of:

    /usr/bin/systemctl --no-block reboot
    /usr/bin/systemctl --no-block poweroff

The service does not grant sudo, a shell, D-Bus access to the System Shell or
general systemd control. Recovery images mask both the service and socket.

## Verification

Host tests cover strict protocol decoding, frame bounds, fixed command mapping,
backend failures, response/action binding, peer-credential enforcement in the
source boundary, systemd hardening and product/recovery image integration.

V0.6 acceptance requires a newly built product image. Restart must disconnect
SSH, produce a new boot ID and return to Home. Power Off must stop the kernel
and leave the device off until physical power is restored. Both actions must be
checked from the on-device confirmation UI; host-side systemctl is not an
equivalent acceptance path.
