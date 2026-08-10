# Notes

<!-- doc-locale: en -->
> **English** | [简体中文](README.zh-CN.md)

Notes is a compact, permission-free text editor. It accepts the printable
CardputerZero ASCII layout, renders punctuation and letter case with the SDK
font, and stores one bounded 192-byte draft in the app's isolated private
storage.

![Notes containing a saved draft](assets/screenshot.png)

## Controls

- Letter, number, Space and symbol keys: insert text.
- `Sym` combinations: insert the printed symbol.
- Enter: start a new line.
- Backspace: delete the previous character.

The draft is saved automatically 600 ms after the last edit. An empty draft
removes the private storage key. No filesystem or shared-document permission
is requested.

## Run in the simulator

```sh
cargo run -p cp0ctl -- run examples/notes \
  --duration 900 --permissions deny --keys h,e,l,l,o \
  --output target/notes.ppm
```

Notes is one of the eight applications included in the product image.
