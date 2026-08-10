# Hello Card

<!-- doc-locale: en -->
> **English** | [简体中文](README.zh-CN.md)

Hello Card is the broad SDK capability demonstration included in the product
image. The colored action block at the lower left reports success, denial,
unavailability or an internal error; the lower-right block identifies the
last key delivered by the Runtime.

![Hello Card capability surface](assets/screenshot.png)

## Controls

- `N`: HTTPS network request.
- `D`: trusted document picker and bounded read.
- `P`: play a short generated tone.
- `R`: capture a short microphone sample.
- `C`: capture and display one camera frame.
- `G`: read and toggle the logical Grove GPIO line.
- `L`: receive one bounded LoRa packet.
- `S`: write a value to private app storage.
- `I`: send an application intent back to Hello Card.

Each protected operation is mediated by a declared manifest permission and a
trusted broker. Private storage and app intents do not expose Linux paths or a
general IPC channel.

## Run in the simulator

```sh
cargo run -p cp0ctl -- run examples/hello-card \
  --duration 500 --permissions allow --keys c,g,s \
  --output target/hello-card.ppm
```

Hello Card is one of the eight applications included in the product image.
