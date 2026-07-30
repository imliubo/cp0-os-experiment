# Phase 3E: Focused application input

## Input boundary

Applications never receive evdev or libinput descriptors. The trusted Runtime
binds `wl_seat` and `wl_keyboard` on its pre-connected Wayland channel. Weston
gives keyboard focus only to the single surface activated by the compositor
policy, so a hidden application receives no key events.

The Runtime starts accepting events only after a `wl_keyboard.enter` for its
own surface. `wl_keyboard.leave` clears the entire event queue, held modifier
state and focus flag before control returns to WASM. Home, Back, Tasks and Power
remain compositor-owned bindings and do not depend on application polling.

## SDK ABI

Rust exposes `input::poll_key_event(timeout)` and C/C++ exposes
`cp0_poll_key_event`. A key event is a fixed eight-byte little-endian record:

- Linux input key code (`u16`);
- pressed and repeated flags;
- stable Shift, Control, Alt and Super bits;
- three reserved zero bytes.

The Runtime accepts a poll timeout from 0 through 1000 ms. It returns one event,
a clean timeout, or a stable SDK error. Its bounded 32-event queue never grows
application-controlled memory; one overflow is reported as `ResourceLimit` and
then cleared. Repeat metadata is reserved in ABI 0.1 but synthesized key repeat
is deferred until the SDK includes a keymap-independent repeat policy.

## Verification

Host tests cover FIFO ordering, exact wire size, reset and overflow behavior.
Rust and freestanding C/C++ tests compile the public polling API and assert the
record size. The AArch64 Runtime was hot-deployed and remained active after
binding the V0.6 `wl_keyboard` under its seccomp policy. Hello Card now polls
only the SDK input API and changes a visible indicator on key press.

The device has no remote key injection interface. Physical verification of the
indicator, key code mapping and compositor interception remains a manual V0.6
acceptance item; it does not require another image flash.
