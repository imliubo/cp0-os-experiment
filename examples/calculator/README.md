# Calculator

<!-- doc-locale: en -->
> **English** | [简体中文](README.zh-CN.md)

Calculator is a permission-free, built-in arithmetic application for the
CardputerZero keyboard. It performs bounded signed integer arithmetic and
shows division-by-zero errors in the display.

![Calculator showing a completed addition](assets/screenshot.png)

## Controls

- `0`-`9`: enter digits.
- `+`, `-`, `*`, `/`: select an operator. On V0.6, use the printed `Sym`
  combinations for symbols.
- Arrow keys: select any on-screen number or operator; Enter activates the
  selection. This provides a complete fallback when entering symbols is
  inconvenient.
- Space: activate the selected on-screen key.
- `=` or Enter: calculate the result.
- `C`: clear the calculation.
- Backspace: remove the last digit.

## Run in the simulator

From the repository root:

```sh
cargo run -p cp0ctl -- run examples/calculator \
  --duration 250 --permissions deny \
  --keys 1,2,plus,3,equal \
  --output target/calculator.ppm
```

The application requests no permissions and is one of the eight applications
included in the product image.
