# Phase 6F: resource and performance acceptance

Phase 6F turns the CM0 performance budget into enforced runtime limits and a
repeatable device report. It does not relax any application isolation boundary
and does not add a background telemetry service.

## Enforced limits

Every application transient unit now has a fixed `CPUQuota=60%` and
`CPUWeight=50` in addition to its manifest memory cgroup, zero swap and task
limit. On the single-core CM0 this prevents a spinning or compromised Runtime
from consuming the entire processor while leaving enough foreground capacity
for normal SDK UI work. CPU quota is a platform security ceiling, not an
application-controlled manifest field.

The trusted Runtime enforces at most 30 display commits per second using
`CLOCK_MONOTONIC`. A submission less than 33,333,334 ns after the preceding
commit receives the existing SDK `ResourceLimit` result. The two-buffer bound,
RGB565 validation and exact damage rectangles remain unchanged. The compositor
also has a 32 MiB memory limit, zero swap and a 32-task limit, matching the
existing 24-hour monitor contract.

## Device gate

Run the performance gate only after the 24-hour stability result has completed
and been retrieved:

```sh
sudo /usr/libexec/cardputerzero/device-performance-acceptance
```

The default run samples an idle Home screen for 60 seconds at five-second
intervals. It refuses to run while the stability unit or any application is
active. Each invocation writes a new root-only directory below
`/run/cardputerzero-performance` containing `checks.tsv`, `samples.tsv`,
`services.tsv`, `summary.env` and `status`; it never deletes earlier evidence.

The V0.6 release thresholds are:

- systemd boot completion and System Shell activation no later than 35 seconds;
- at most 180 MiB used and at least 200 MiB available during idle sampling;
- compositor, Shell and appd within 32/32/24 MiB respectively;
- aggregate idle CPU for those three services at or below 10 percent;
- no more than 1 MiB of SD writes during the short idle sample.

The 1 MiB check catches immediate persistent-write regressions; the authoritative
write-amplification gate remains the 64 MiB limit over the separate 24-hour
stability run.

The script records BQ27220 voltage, signed battery current and an estimated
battery-side power value when available. That value is informational only:
while USB powers the board, battery current is not total device power. A product
power claim still requires an inline, calibrated USB power meter under defined
brightness, network and workload conditions.

## Current baseline

Read-only sampling on the V0.6 device before implementation measured 27.939
seconds to systemd completion, 27.790 seconds to Shell activation, about 164 MiB
idle memory in use, and 7.4/1.7/1.8 MiB for compositor/Shell/appd. These values
define headroom rather than being copied as exact pass criteria. Formal evidence
remains pending until the active 24-hour run completes and the updated platform
can be deployed without invalidating it.
