# ADR 0003: CM0 Memory Allocation and Pinned BSP

<!-- doc-locale: en -->
> **English** | [简体中文](0003-cm0-memory-and-bsp.zh-CN.md)

- Status: Accepted
- Date: 2026-07-30

## Decision

CM0 V0.6 uses 64 MB for VideoCore, 448 MB for ARM, and 64 MB for VC4 CMA. The
image build removes only the `cgroup_disable=memory` token embedded in the base
DTB. The system enables the memory controller in unified cgroup v2 and enables
AppArmor.

The BSP is pinned to M5Stack dtoverlays commit
`c3b254819307c177a34100b66fe19e52059ce8c4`, using the upstream V0.5 build
switches and `cardputerzero-v5-overlay`. On V0.6, `powerfail_suo` in that overlay
exclusively owns P12 and performs bounded retries for `imx219` after power is
stable. The separate `camera-py12-high-overlay` would contend for the same line,
and the older `camera-gpio16-high-overlay` must not be used on V0.6.

The image sets `start_x=1` and uses the camera-capable
`start_x.elf`/`fixup_x.dat` pair supplied by `raspi-firmware`. The M5Stack
`m5stack_bootscreen` binary must not enter the image; both the build and finished
image gates reject its pinned SHA-256. ADR 0008 defines the splash path without
changing the overlay or camera-power decisions.

## Rationale

The physical device originally split 512 MB evenly between ARM and VideoCore,
leaving Linux about 227 MiB. CM0's 512 MB-specific default overrides the generic
`gpu_mem=64`, so `gpu_mem_512=64` must also be set. VC4 also requested 256 MB of
CMA and repeatedly failed allocation, producing DRM and camera errors. Full KMS
does not require a 256 MB firmware heap; 64 MB GPU plus 64 MB CMA provides a
conservative margin for current camera and DRM testing.

Production-device testing on 2026-08-06 disproved the assumption that the
supplier's `m5stack_bootscreen` firmware respected this budget. Even with both
`gpu_mem=64` and `gpu_mem_512=64` in `config.txt`, it reported `arm=256M` and
`gpu=256M`, while Linux `MemTotal` remained about 227 MiB. Restoring the standard
`start_x` firmware is therefore required by this ADR. A 256/256 allocation is
not acceptable merely to obtain a pre-kernel splash.

The base `bcm2710-rpi-cm0.dtb` contains `cgroup_disable=memory`; appending an
enable parameter cannot undo a disable operation already processed. An overlay
cannot replace all of `bootargs`, because firmware merges `cmdline.txt` before
applying overlays; replacement would also remove rootfs parameters and prevent
boot. The image build therefore patches the matching firmware's base DTB and
removes only the target token.

Application resource isolation depends on the memory cgroup. AppArmor is the
primary LSM already compiled into this Raspberry Pi kernel and can be enabled
without replacing the kernel. This kernel lacks Landlock, Yama, and BPF LSM, so
the initial isolation design cannot depend on them.

## Consequences

The camera broker must use libcamera/V4L2 and must not depend again on a large
legacy VideoCore heap. Phase 1 physical-device testing must cover continuous
camera preview, DRM refresh, and audio playback to confirm that 64 MB CMA is
sufficient. If it is not, increase CMA only, not `gpu_mem`.
