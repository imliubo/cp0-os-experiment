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
- the private Wayland, appd control and runtime broker sockets.
- cumulative SD-card sectors written from `/sys/block/mmcblk0/stat`, sampled to
  RAM without causing measured SD writes.

Unexpected restarts, missing processes/sockets, failed pings and cgroup memory
above 32 MiB for compositor/Shell or 24 MiB for appd are failures. The final
sample may grow by at most 4 MiB, 2 MiB and 4 MiB respectively from the idle
baseline. Results contain `samples.tsv`, `summary.env`, `status` and, only on
failure, `failures.log`. `block-io.tsv` contains the raw write counter. The
default idle acceptance permits at most 64 MiB of SD writes over the run; a
fourth argument can set a stricter byte limit.

Both tools are copied into the image as explicit diagnostics under
`/usr/libexec/cardputerzero/`; neither is enabled as a boot service.

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

The formal 24-hour acceptance transient service started at approximately
2026-07-31 05:25 CST and was confirmed active. Its RAM-backed result is below
`/run/cardputerzero-stability/acceptance`; the Roadmap remains open until the
full interval finishes and the final report is inspected.
