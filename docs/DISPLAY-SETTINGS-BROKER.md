# Display settings broker v1

`cp0-displayd` is the first privileged settings provider used by the trusted
System Shell. It is not an application capability and has no SDK or Runtime
host call. Applications cannot request, probe or inherit display control.

## Hardware contract

The V0.6 device-tree overlay names the `m5stack,pwm-backlight` node
`backlight`, which maps to these fixed sysfs attributes:

- `/sys/class/backlight/backlight/max_brightness` is read-only;
- `/sys/class/backlight/backlight/brightness` is the only writable attribute.

The broker converts raw levels to an observed percentage with bounded integer
arithmetic. Requests may set 5 through 100 percent. Adjust requests use one
fixed 10-percent step and clamp at the safe bounds, so a global shortcut cannot
turn off the only local display. Every successful write is read back before the
Shell is updated.

The brightness attribute must receive the complete decimal value and trailing
newline in one `write(2)` call. The V0.6 sysfs implementation applies the first
fragment but rejects a second write at the advanced file offset. The broker
therefore encodes the full attribute value before writing it and treats a short
write as a device error.

The fixed path is established by the pinned BSP source. V0.6 inspection has
confirmed the path, its 100-level range and broker-controlled 65/75-percent
write/readback points. A missing path, unexpected value, permission failure or
failed readback makes the control unavailable. The production UI never enables
a simulated fallback.

## Trust boundary

The systemd socket is `0660 root:cp0-display-control`; only `cp0-shell` belongs
to that control group. The service independently resolves the `cp0-shell` UID
and checks every accepted connection with `SO_PEERCRED`. Possession of another
`cp0-control` or application identity is insufficient.

`cp0-displayd` runs as the dedicated `cp0-display` account with an empty
capability set, private devices, Unix-only networking and a strict filesystem.
The service sandbox grants write access only to the brightness attribute, while
tmpfiles narrows the underlying sysfs mode to `0660 root:cp0-display`. Mutating
requests emit a bounded peer UID and observed percentage audit line to the
RAM-backed journal.

## Protocol and Shell behavior

The newline-delimited JSON protocol has a 2 KiB frame ceiling, exact version 1,
strict unknown-field rejection and three commands: `get-state`,
`set-brightness` and `adjust-brightness`. Responses return either an observed
state or a bounded error. Unavailable state is represented explicitly and
cannot carry a stale percentage.

Fn+U/Fn+I remain compositor-owned global actions. The trusted Shell converts
them to one broker adjustment and renders the returned value in its transient
overlay. The Settings brightness row uses the same path. When the socket or
hardware is unavailable, the row and overlay say `UNAVAILABLE`; no local-only
value is presented as hardware state.

## Verification status

Local coverage includes protocol framing and validation, safe-bound clamping,
unavailable hardware, strict C response parsing, Shell event routing, unchanged
320x170 pixel snapshots, systemd confinement and image/deployment integration.
The Linux AArch64 build and full repository check are required before a device
candidate is prepared.

The retained 24-hour stability evidence, physical sysfs identity,
65/75-percent write/readback, non-Shell denial and service restart checks have
passed. Fn+U/Fn+I LCD overlays, Settings navigation and input latency still
require operator observation; the short performance run recorded zero SD
writes.
