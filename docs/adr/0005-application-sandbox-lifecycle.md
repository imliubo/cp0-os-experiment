# ADR 0005: Application sandbox lifecycle

- Status: accepted for Phase 3 implementation
- Date: 2026-07-30

## Context

CardputerZero applications are untrusted WASM modules. The WASM runtime limits
the module's instruction and memory access, but a runtime defect must not expose
the host filesystem, devices, IPC endpoints or another application's data.
Applications also need a stable identity for storage, compositor policy and
permission decisions.

The target has 512 MB RAM and only runs one foreground application. The design
must therefore use existing kernel and systemd primitives without introducing a
container daemon.

## Decision

The root-owned `appd` is the only component allowed to turn an installed package
into a running application. Its launch sequence is:

1. Load the installed manifest and verify its schema, package signature and
   immutable package location.
2. Resolve the package ID through a root-owned registry to a stable system
   account named `cp0-app-N`. The caller cannot choose this mapping.
3. Create or verify the application's private data directory, owned only by its
   assigned account, and enforce its storage quota.
4. Start a transient systemd unit with a memory cgroup and process hardening.
5. Enter a bubblewrap PID, mount, IPC, UTS, cgroup and network namespace. The
   package is mounted read-only at `/app`; only the private `/data` is writable.
6. Execute the static, root-owned App Runtime. The runtime loads the validated
   WASM/AOT entrypoint and receives only pre-opened Wayland and broker channels.

No host `/usr`, `/home`, `/run`, system D-Bus, device nodes or arbitrary Unix
sockets are visible. Bubblewrap is a namespace constructor, not an application
compatibility environment. Third-party native executables remain unsupported.

The initial implementation emits a structured `SandboxPlan`. Execution is added
only after tests prove that every path, account, package and descriptor in the
plan was derived from trusted state. Commands are always passed as argument
arrays; shell parsing is not part of the launch path.

Seccomp is applied after bubblewrap has constructed namespaces, so the App
Runtime cannot retain the mount and namespace syscalls needed during setup. The
runtime allowlist, inherited descriptor protocol, account registry and quota
backend are required before `appd` can launch production applications.

`ProtectKernelTunables=yes` cannot be applied to the outer transient unit: its
systemd `/proc/sys` remount prevents an unprivileged user namespace from mounting
the private `/proc` required by bubblewrap. The application still cannot change
host tunables because it is in a non-initial user namespace and the runtime
seccomp policy rejects `open`, `openat` and all mount syscalls.

## Consequences

- A compromised WASM runtime is still confined by a unique host UID, namespaces,
  cgroup limits, an empty capability set and seccomp.
- Applications cannot communicate by knowing paths or socket names. Intents and
  hardware access must go through capability brokers.
- A stable account registry must survive upgrades and must never recycle an UID
  while data or permission records for that application remain.
- Direct execution of a generated plan is a diagnostic feature only. Production
  launch always revalidates the installed package and registry under `appd`.
