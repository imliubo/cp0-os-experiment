# Phase 3N: Malicious application regression set

The negative test set makes the application isolation contract independently
repeatable instead of relying only on a successful example application.

## Samples

`memory-hog.wasm` commits 40 MiB of linear memory and verifies the manifest
memory cgroup terminates it without swap or damage to the Shell. This sample was
already accepted on V0.6 hardware.

`ambient-authority.wasm` deliberately bypasses the SDK and imports WASI
`path_open` and `sock_open`. The build test inspects the binary WebAssembly
import table and fixes those imports as the expected malicious behavior. The
production Runtime has WAMR builtin libc and WASI disabled, so the module cannot
instantiate those imports; a third-party module has no filesystem or socket
syscall ABI.

`path-escape-app.json` uses `../etc/passwd.wasm` as its entry point. The
manifest parser rejects it before registry or launch planning. Lifecycle tests
also reject symbolic links and identity changes inside a root-owned installed
package.

## Compromised Runtime boundary

WASM isolation is not the only boundary. The generated bubblewrap plan is
tested to expose exactly three read-only host sources: the trusted Runtime, the
selected immutable package and the single appd broker socket. `/dev` is a new
private device tree. There is no host `/usr`, D-Bus, Wayland path, DRM, input,
GPIO, ALSA or another application's data mount. The connected Wayland stream is
opened by PID 1 and passed as the only `OpenFile` descriptor.

The static aarch64 seccomp probe now verifies denial of:

- `/etc` and `/proc/self/root` path access;
- DRM, input, gpiochip and ALSA device opens;
- IPv4, IPv6 and netlink sockets plus `socketpair`;
- mount, ptrace, clone, exec and process signalling.

One AF_UNIX socket remains available so the trusted Runtime can reach the only
mounted broker endpoint. An application cannot invoke `socket` itself because
WASI and native libc imports are absent; even a compromised Runtime sees no
other host socket path in its mount namespace.

## Verification

`tests/test-malicious-apps.sh` rebuilds both WASM samples, inspects imports and
checks rejection of the malicious manifest. `make app-runtime` cross-compiles
the expanded static seccomp probe. Re-running that aarch64 probe and the
existing memory-cgroup sample on V0.6 is deferred until the active 24-hour
stability monitor completes.
