# Keyboard Diagnostics

This focused SDK application records the keyboard events delivered through the
trusted Runtime input boundary. It does not open evdev and does not modify the
keyboard configuration.

![Keyboard Diagnostics review step](assets/screenshot.png)

The guided sequence covers:

- lowercase without Shift;
- uppercase while physical Shift is held;
- modifier release without a stuck Shift state;
- all 32 `Sym` combinations from the V0.6 keyboard reference CSV.

For each step, press the requested key or chord. The review screen shows the
received Linux key code, modifier mask, decoded V0.6 ASCII name/code and whether
it matches the expected event. The character shown is produced by Runtime and
read directly from the SDK event. Press Enter to confirm and continue, or
Backspace to discard the capture and retry the same step.

## Run in the simulator

```sh
cargo run -p cp0ctl -- run examples/keyboard-diagnostics \
  --duration 600 --permissions deny --keys a \
  --output target/keyboard-diagnostics.ppm
```

Keyboard Diagnostics is an engineering image option, not one of the eight
built-in product applications.

The application atomically updates `keyboard-test.log` in its private storage
after every capture, confirmation and retry. On a development device, a root
operator can collect the log without weakening the application's sandbox:

```sh
sudo cat /var/lib/cardputerzero/data/dev.cardputerzero.keyboard-diagnostics/keyboard-test.log
```

The compact CSV begins with `CP0K,1,<test-count>`. It contains every
press/release event seen by the Runtime, including physical Shift transitions.
Record types are `S` (step), `E` (event), `C` (capture), `K`
(confirmation), `R` (retry), `D` (done) and `X` (error). The fixed event columns
are sequence, step, Linux key code, pressed, repeated and modifier mask.

Analyze a collected log with:

```sh
./scripts/analyze-keyboard-diagnostics.sh keyboard-test.log
```

The analyzer separates key-code translation, modifier-state and Runtime ASCII
mapping failures. If every Runtime event matches, the remaining fault is in the
consuming text widget or renderer rather than the keyboard event chain.
