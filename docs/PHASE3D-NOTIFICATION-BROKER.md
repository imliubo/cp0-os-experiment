# Phase 3D: notification capability broker

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE3D-NOTIFICATION-BROKER.zh-CN.md)

## Request path

The notification broker is the first typed SDK capability service. `appd` owns
two socket-activated listeners:

- `/run/cardputerzero-appd/control.sock` is mode `0660`, belongs to the control
  group and accepts only root or the trusted Shell UID after `SO_PEERCRED`;
- `/run/cardputerzero-broker/runtime.sock` is mode `0666` so distinct app UIDs
  can connect, but every request is authenticated by `SO_PEERCRED` and mapped
  back to one installed, currently running application in the root-owned
  registry. The peer PID must also be a member of that application's exact
  systemd cgroup; a host process using the same UID is rejected.

The broker socket is the only host IPC endpoint mounted into the application's
otherwise empty root at `/run/cardputerzero/broker.sock`. The request cannot
name an application, permission, file, device or host command. It contains only
a bounded notification title and body.

## Authorization and resource bounds

`notifications.post` must exist in the canonical installed manifest. The shared
permission coordinator then returns allow, deny, undeclared or one pending
prompt ID. A newly granted application retries the typed request; prompt
resolution never replays untrusted payload automatically.

Notification titles are limited to 32 characters and bodies to 160 characters.
Control characters and frames over 4 KiB are rejected. The in-memory FIFO holds
at most eight notifications and returns `resource-exhausted` instead of growing
without bound.

The trusted Shell retrieves notifications through the authenticated control
socket. `cp0ctl` exposes diagnostic commands for bring-up:

```sh
cp0ctl broker notify <title> <body>
cp0ctl permission pending
cp0ctl permission resolve <prompt-id> once|always|deny
cp0ctl notification take
```

`cp0ctl broker notify` does not bypass identity checks. It is useful only when
run as the registered UID of the foreground test application.

System Shell protocol v4 presents each dequeued item as a compositor-enforced
trusted banner for four seconds. The application remains the keyboard focus
while the Shell occupies the top 88 pixels. Permission prompts take priority
and switch to the full trusted surface; Home, Tasks, Power and application
withdrawal clear a visible banner. The application cannot control this policy
or draw into the trusted layer.

## Deployment invariants

The appd service has no capabilities, only `AF_UNIX`, and a 24 MB cgroup limit.
`ProtectSystem=strict` remains enabled; only the permission registry directory
is writable so `allow-always` and `deny` can be committed atomically. The broker
socket directory is root-owned and non-writable by applications. Launch-time
host validation rejects a missing, symbolic-link, non-socket or non-root-owned
broker endpoint before entering bubblewrap.

## V0.6 validation

Phase 3D was hot-deployed to the V0.6 device without rebooting or flashing.

- The final aarch64 `cp0-appd` SHA-256 is
  `e2ad7cb396a19ff2163f45930fdc1f030db6056cfbf25ac37c110ab2b50eb0b1`;
  `cp0ctl` is
  `0dc07fb09643ef0902b421ed06e0d006ee7c6a4d8d1019db21071cbae3a71b66`.
- The control socket was `root:cp0-control 0660`; the broker socket was
  `root:root 0666` inside a `root:root 0711` directory.
- Bubblewrap mounted the broker endpoint and the Hello app remained active in
  `/system.slice/cardputerzero-app-20000.service`.
- A request from `pi` was rejected because its UID was not registered. A host
  process using UID 20000 was separately rejected because its PID was outside
  the exact application cgroup.
- A cgroup-bound request returned prompt 1 with canonical app name, permission
  and manifest reason. `allow-always` persisted
  `/var/lib/cardputerzero/registry/permissions.json` as `root:root 0600`.
- Retrying returned notification ID 1 and the Shell channel retrieved the exact
  trusted app identity, title and body. The test app then stopped cleanly while
  appd, compositor and System Shell remained active.
