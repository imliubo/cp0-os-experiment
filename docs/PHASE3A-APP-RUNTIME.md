# Phase 3A: App Runtime and Linux sandbox

## Scope

Phase 3A establishes the executable isolation boundary below the SDK. Follow-up
work added the notification broker and the controlled display channel described
in `PHASE3D-CONTROLLED-DISPLAY.md`.

The trusted App Runtime embeds WAMR 2.4.5 at commit
`25bd7eb63e828e4bd242cc9b38d260b4b31c6605`. The target build is a static
aarch64 executable with interpreter and AOT loading enabled. JIT, WASI, native
libc imports, threads, shared memory, SIMD and multi-module loading are disabled.
Third-party modules consequently have no ambient filesystem, socket or process
API.

After reading the already validated module file, the runtime installs an
aarch64 seccomp allowlist before WAMR parses or instantiates untrusted bytes.
The filter permits memory management, clocks, signals and communication over
pre-opened descriptors. It denies `open`, `openat`, `socket`, `connect`, mount,
namespace, process creation and ptrace syscalls with `EPERM`.

## Build

The WAMR source checkout and all build output stay below the repository's
ignored `target/` directory:

```sh
make app-runtime
make example-app
make malicious-apps
```

`scripts/build-app-runtime.sh` refuses WAMR, Wayland, wayland-protocols or
libffi checkouts whose HEAD differs from the pinned commits. It verifies the
resulting ELF is aarch64 and has no dynamic library dependencies.

## Sandbox contract

`cp0-appd plan` combines the static runtime with:

- a stable `cp0-app-N` host account;
- a transient systemd service and cgroup v2;
- `MemoryMax` equal to the manifest budget and `MemorySwapMax=0`;
- an empty bubblewrap root, PID/mount/network/IPC/UTS/cgroup namespaces;
- a read-only package at `/app` and runtime at `/runtime`;
- the application's sole writable directory at `/data`;
- an empty private `/dev`, private `/tmp`, no host `/usr`, `/run` or D-Bus.

The outer unit permits `AF_NETLINK` only because bubblewrap needs
`NETLINK_ROUTE` while constructing its private network namespace. The runtime
seccomp policy denies `socket()`, so this family is unavailable to the running
application. `ProtectKernelTunables=yes` is intentionally omitted for the
namespace compatibility reason recorded in ADR 0005.

## V0.6 validation

Validation on the 512 MB V0.6 device used Debian 13, kernel
`6.18.34+rpt-rpi-v8`, systemd 257 and bubblewrap 0.11.0.

- A minimal WASM module completed through the full systemd, bubblewrap, seccomp
  and WAMR path with status 0.
- The successful unit peaked at 9.3 MB and used no swap.
- The seccomp negative probe confirmed `openat`, IPv4 `socket`, `mount` and
  `ptrace` all returned `EPERM`.
- A module that committed 40 MB of linear memory was terminated with systemd
  result `oom-kill`; the cgroup peaked at exactly 24 MB and used 0 bytes swap.
- `cardputerzero-compositor.service` and
  `cardputerzero-system-shell.service` remained active after every probe.

No image rebuild or flash was required. Development artifacts are installed at
`/usr/libexec/cardputerzero/app-runtime` and the root-owned Hello package path;
the stable test identity is `cp0-app-20000` (UID/GID 20000).
