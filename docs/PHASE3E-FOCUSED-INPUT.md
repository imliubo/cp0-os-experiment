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

Text translation is owned by the platform rather than each application. The
System Shell and App Runtime compile the same V0.6 evdev-to-printable-ASCII
table. The 32 `Sym` combinations use the V0.6 keymap's distinct evdev
identifiers and therefore map directly to their printed characters without
synthetic Shift state. Runtime combines depressed XKB Shift with raw left/right
Shift only for ordinary letters and digits, and deliberately ignores latched
and locked state. This matches first-boot Owner Name and Wi-Fi password input.

## SDK ABI

Rust exposes `input::poll_key_event(timeout)` and C/C++ exposes
`cp0_poll_key_event`. A key event is a fixed eight-byte little-endian record:

- Linux input key code (`u16`);
- pressed and repeated flags;
- stable Shift, Control, Alt and Super bits;
- a system-produced printable ASCII byte, or zero when the event has no text;
- two reserved zero bytes.

Rust exposes the text byte as `KeyEvent::character: Option<u8>`; C/C++ exposes
`cp0_key_event_t.character`, where zero means absent. Applications use that
field for text entry and retain `code` for navigation, shortcuts and games.
Release and non-printable events always carry no character. The record remains
exactly eight bytes, so previously built SDK 0.1, 1.0 and 1.1 applications keep
their flat ABI and see the former reserved byte only as ignored data.
The Rust reference `Canvas` covers every printable ASCII glyph and preserves
uppercase and lowercase instead of normalizing display text.

The Runtime accepts a poll timeout from 0 through 1000 ms. It returns one event,
a clean timeout, or a stable SDK error. Its bounded 32-event queue never grows
application-controlled memory; one overflow is reported as `ResourceLimit` and
then cleared. Repeat metadata is reserved in ABI 0.1 but synthesized key repeat
is deferred until the SDK includes a keymap-independent repeat policy.

## Verification

Host tests cover the complete printable mapping, depressed Shift, raw Shift
fallback, FIFO ordering, exact wire size, character offset, reset and overflow
behavior. Notes, Music, Calculator and Keyboard Diagnostics consume the SDK
character field rather than maintaining application-local keymaps.
Rust and freestanding C/C++ tests compile the public polling API and assert the
record size. The AArch64 Runtime builds as a fully static executable with no
`DT_NEEDED` entries. The earlier focused-input boundary was hot-deployed and
remained active after binding the V0.6 `wl_keyboard` under its seccomp policy;
the new system-character behavior still requires physical acceptance.

The device has no remote key injection interface. Physical verification must
cover lowercase, held-Shift uppercase and all 32 `Sym` combinations in Notes,
plus compositor interception. It remains a manual V0.6 acceptance item.
