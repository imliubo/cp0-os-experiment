# Phase 3J: Restricted logical GPIO broker

## Scope

The V0.6 GPIO API exposes four logical connector outputs and no raw Linux GPIO
interface:

| SDK line | Fixed kernel attribute | Purpose |
|---|---|---|
| `grove-function` | `grove_fun/brightness` | Grove connector function output |
| `external-usb-function` | `ext_usb_gpio_fun/brightness` | external USB function output |
| `grove-5v-power` | `grove_5v_out/brightness` | Grove 5 V rail control |
| `external-5v-power` | `ext_5v_out/brightness` | external 5 V rail control |

All operations are boolean read or write requests under the existing
`hardware.gpio` permission. Applications cannot name a path, gpiochip, line
number, direction, edge, pull, drive strength or pinmux mode. Adding a future
input line requires a new reviewed logical enum member rather than accepting a
number from a manifest or RPC request.

## Hardware Evidence

Read-only V0.6 inspection found three gpiochips and the live upstream overlay
at pinned BSP commit `c3b254819307c177a34100b66fe19e52059ce8c4`. The overlay
and kernel ownership data reserve, among others:

- GPIO7/8/9/10/11 and GPIO22 for SPI chip selects/data;
- GPIO12/13 for infrared, GPIO17 and GPIO18-21 for audio;
- GPIO24/25 for speaker/display and GPIO27 for the keyboard interrupt;
- expander lines for keyboard reset/LEDs, display power and power-fail control.

Those lines are excluded even when a momentary pinctrl snapshot reports one as
unclaimed. The API is based on named board functions, not incidental driver
state. The four selected outputs already have stable LED-class attributes in
the V0.6 overlay and therefore do not require userspace pinmux or gpiochip
ownership.

## Trust Flow

```text
WASM gpio SDK call with fixed enum + bool
  -> Runtime validates enum/value and sends bounded Unix JSON
  -> appd binds SO_PEERCRED to the active app UID and systemd cgroup
  -> appd verifies root-owned manifest and hardware.gpio decision
  -> root-only cp0-gpiod socket accepts only appd
  -> cp0-gpiod maps the enum to one compiled-in sysfs attribute
  -> kernel LED/GPIO driver performs the boolean operation
```

`cp0-gpiod` runs under the dedicated `cp0-gpio` account with no capabilities,
private devices, only `AF_UNIX`, an 8 MiB memory limit and four explicit
`ReadWritePaths`. The service does not receive gpiochip device nodes.

The upstream BSP installs development-oriented `0666` modes for these
attributes. CardputerZero OS overrides all four with
`0660 root:cp0-gpio`, so neither the login user nor an application Runtime can
bypass the broker through sysfs. The application sandbox does not mount host
sysfs in any case; the tightened modes provide defense in depth for other
native processes.

## Verification

Automated coverage includes:

- strict 2 KiB protocol frames and rejection of unknown lines/fields;
- fixed enum-to-path mapping and mock backend read/write behavior;
- appd request, line, value and request-ID correlation;
- `hardware.gpio` manifest/permission routing;
- Runtime boolean decoding and mismatched-line rejection;
- Rust, C11, C++17 and WIT SDK surfaces;
- service hardening, sysfs modes, image and hot-deployment assertions;
- AArch64 gpiod/appd/Runtime and wasm32 Hello Card builds.

Hello Card binds `G` to read and invert only `grove-function`. The power lines
are never changed by the example. Physical read/write and denial acceptance is
automated through the real-identity probe in
`PHASE3M-DEVICE-CAPABILITY-ACCEPTANCE.md`, but execution remains deferred until
the active 24-hour compositor/Shell/appd stability run ends.
