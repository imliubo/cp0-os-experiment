# Phase 2G: Core recovery and stability acceptance

## Recovery contract

`device-core-recovery.sh` exercises the three trusted long-running services on
the real device. It refuses to run while an application is active, resolves
only fixed systemd unit names and sends `SIGKILL` only to each unit's main
process. It verifies a new PID, an increased `NRestarts` counter where systemd
performs a failure restart, preserved unrelated service PIDs, all three Unix
sockets and live appd ping/list requests.

The compositor failure case also requires the `BindsTo` System Shell process to
be replaced and reconnect to the new private Wayland socket. A recovery is not
accepted merely because systemd reports `active`.

## 24-hour monitor

`device-stability-monitor.sh` defaults to 86,400 seconds at a 60-second sample
interval. It is root-only and permits output only below
`/run/cardputerzero-stability`, which is RAM-backed on the V0.6 image. Every
run creates a unique directory and never removes prior results.

Each sample records:

- wall-clock epoch and monotonic uptime;
- `ActiveState`, `SubState`, `MainPID`, `NRestarts` and `MemoryCurrent` for
  compositor, System Shell and appd;
- an authenticated appd ping;
- a paginated, schema-checked application list and the number of running
  foreground applications;
- the private Wayland, appd control and runtime broker sockets.
- cumulative SD-card sectors written from `/sys/block/mmcblk0/stat`, sampled to
  RAM without causing measured SD writes.

Unexpected restarts, missing processes/sockets, failed pings and cgroup memory
above 32 MiB for compositor/Shell or 24 MiB for appd are failures. A failed or
malformed application-list query, invalid pagination, or any running application
also fails the idle run. The final
sample may grow by at most 4 MiB, 2 MiB and 4 MiB respectively from the idle
baseline. Results contain `samples.tsv`, `summary.env`, `status` and, only on
failure, `failures.log`. `block-io.tsv` contains the raw write counter and
`foreground.tsv` contains the running-application count at every sample. The
default idle acceptance permits at most 64 MiB of SD writes over the run; a
fourth argument can set a stricter byte limit.

Application transient units also declare a systemd conflict with the named
stability acceptance service. Once that platform version is deployed, starting
an application stops the monitor and its exit trap writes `FAILED`, including
for an application that runs entirely between two 60-second samples. This hard
interlock complements rather than replaces the independently verified
`foreground.tsv` timeline.

Both tools are copied into the image as explicit diagnostics under
`/usr/libexec/cardputerzero/`; neither is enabled as a boot service.

Completed evidence is not accepted solely because the device wrote `PASS`.
After retrieval, `verify-stability-evidence.sh` independently parses the files
without sourcing `summary.env`. It requires an exact field set, one block-I/O
row and exactly three distinct core-service rows per epoch, monotonic wall and
uptime coverage for the requested duration, constant service PIDs/restart
counts, memory limits/growth, summary-to-raw agreement and the SD write bound.
The foreground, block-I/O and service timelines must agree exactly, and every
foreground count must be zero. When the optional stored service is present, it
must appear in every epoch with
a stable PID/restart count and remain below its memory limit.
Unknown/duplicate fields, a non-empty failure log, missing samples, oversized
gaps or a forged summary fail closed. Its mutation tests run in `make check`.

## V0.6 validation

The recovery test passed on 2026-07-31 without a reboot or image flash:

- appd PID `8249 -> 9628`, `NRestarts 0 -> 1`;
- System Shell PID `8351 -> 9651`, `NRestarts 0 -> 1`;
- compositor PID `8334 -> 9679`, `NRestarts 0 -> 1`;
- compositor replacement caused the Shell to rebind as PID `9695` while appd
  remained unchanged;
- all control paths passed and 4K Camera2 showed Home after recovery.

A 15-second, three-second-interval monitor smoke run completed with zero
failures. Compositor memory stayed at 7,487,488 bytes, Shell moved from
1,073,152 to 1,323,008 bytes, and appd moved from 1,081,344 to 1,073,152 bytes.

The first formal 24-hour acceptance run started at approximately 2026-07-31
05:25 CST, but its RAM-backed result was lost when the device rebooted. A later
14:19 run was also invalidated during LCD cold-boot diagnosis. The current run
started at 2026-07-31 20:42:15 CST as transient unit
`cardputerzero-stability-acceptance.service`; at the 21:58 read-only check it and
compositor, System Shell and appd were all `active/running` with zero restarts.
That run was explicitly invalidated at 2026-08-01 00:26 CST by the owner's
requested developer-mode key and Neon Snake package installation. Its eventual
status must not be accepted as idle evidence. It was stopped at 00:43 CST and
its `FAILED` archive was retained under the host `target/device-evidence`
directory with SHA-256
`88ecc5d2414710d3dc60ce63dbbd046e7bc0e010bccc29f609409edaba23c2bf`.

Neon Snake was then stopped and developer mode was disabled. Disabling that
mode restarted appd before the replacement baseline. A replacement run started
at 2026-08-01 00:43:19 CST in
`/run/cardputerzero-stability/acceptance/20260731T164319Z-9619`. Its first sample
recorded compositor PID `909`, System Shell PID `926`, appd PID `9465` and
stored PID `8480`, all active with zero restarts; 4K Camera2 showed Home.
However, a read-only application-list query at 00:49 found Neon Snake still
marked running, so that run is also invalid idle evidence. The replacement
run was stopped without restarting a core service and retained as
`target/device-evidence/invalid-20260731T164319Z-9619.tar.gz`, SHA-256
`9608197a5520cea281054a3ede9d0047362c0cb70135fbe915d643a93120d8fd`.

The foreground-aware monitor was then hot-deployed with SHA-256
`5219e5b33982c598914378c127694fc491f2186b0bca0df952b639dbb3b42797`.
A three-second hardware smoke run passed with three zero-foreground samples,
zero failures, zero SD writes and unchanged core service PIDs. The formal
replacement started at 2026-08-01 01:02:28 CST in
`/run/cardputerzero-stability/acceptance/20260731T170228Z-10620`. Its first
sample recorded zero running applications and compositor PID `909`, System
Shell PID `926`, appd PID `9465` and stored PID `8480`, all active with zero
restarts; 4K Camera2 showed idle Home after startup. The Roadmap remains open
until approximately 2026-08-02 01:02 CST, when the complete uninterrupted
directory must be retrieved and independently verified before any platform
deployment, app launch or reboot.
