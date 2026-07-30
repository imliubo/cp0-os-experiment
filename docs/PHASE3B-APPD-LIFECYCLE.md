# Phase 3B: appd lifecycle service

## Trust boundary

`cp0-appd` is a root-owned, socket-activated daemon. A lifecycle request names
only an application ID. The daemon derives the installed version, Unix account,
package path, entrypoint, memory limit and unit name from its root-owned registry
and the validated canonical manifest. Clients cannot supply host paths, users,
commands or systemd properties.

Before every start, `appd` verifies:

- the registry is a non-symlink, root-owned regular file inaccessible to group
  and other users;
- registry, application and data parent directories are root-owned and not
  group/world writable;
- the package, manifest, entrypoint and static runtime are root-owned and not
  group/world writable;
- every entrypoint path component is a real directory rather than a symlink;
- `cp0-app-N` resolves to the exact stable UID/GID stored in the registry;
- the private data directory is owned by that UID and has no group/other mode.

At most one application unit may run. A second start is rejected rather than
implicitly terminating the current application. Stop derives the stable unit
name from the registry without reopening package content, so a running app can
still be terminated if its package becomes corrupt.

## Control protocol

systemd owns `/run/cardputerzero-appd/control.sock`. A tmpfiles rule creates the
parent as `root:cp0-control 0750`; the socket is `root:cp0-control 0660`. Only
`cp0-shell` belongs to `cp0-control`. After DAC succeeds, `appd` checks Linux
`SO_PEERCRED` and accepts only UID 0 or the resolved `cp0-shell` UID.

Protocol v1 uses one newline-terminated JSON request per connection. Both
directions are bounded to 8 KiB, reject unknown fields and use request IDs.
Supported commands are `ping`, paged `list` (at most 8 records), `start` and
`stop`. Internal filesystem and command errors are logged but are not exposed to
clients.

Launcher list records additionally expose only canonical manifest name and
standard/immersive display policy. The trusted Shell requests pages of eight,
starts a selected stopped application, then waits for the compositor's
ephemeral surface token before activation. Stopping from Tasks preserves the
installed registry entry.

`cp0ctl app ping|list|start|stop` is the diagnostic client. The System Shell will
use the same contract after permission prompts and application launch UI are
integrated.

## V0.6 validation

The service and socket were hot-deployed without rebooting or flashing.

- The ordinary `pi` account received `EACCES` while opening the control socket.
- `cp0-shell` completed protocol `ping`, paged list, start and stop requests.
- The running Hello unit used UID/GID 20000, about 9.0 MB memory, three tasks,
  `MemoryMax=24M` and `MemorySwapMax=0`.
- The host-side bubblewrap monitor remained outside the application namespace;
  bubblewrap PID 1 and App Runtime PID 2 inside the sandbox both had no `/usr`,
  used a distinct PID/network namespace, and reported `NoNewPrivs=1` plus
  seccomp mode 2.
- After stop, appd, compositor and System Shell remained active and the app unit
  was inactive/collected.

The deployed development artifacts use `cp0-app-20000` and the registry at
`/var/lib/cardputerzero/registry/apps.json` with mode `0600`.
