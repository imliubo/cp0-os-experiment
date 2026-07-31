# Phase 4D: small-screen UI and reference applications

## SDK UI surface

The Rust SDK now includes an allocation-free RGB565 `Canvas` designed for the
320x150 standard application surface and the 320x170 immersive surface. It
provides clipped drawing, a compact ASCII font, buttons and progress bars while
keeping frame-buffer ownership in the application.

The optional LVGL 9 C adapter binds a full-render RGB565 display and keypad
input to public CardputerZero hostcalls. It deliberately has no Linux backend.
Two complete buffers make flush semantics deterministic and consume 192,000
bytes in standard mode or 217,600 bytes in immersive mode.

## Reference applications

`examples/calculator` exercises direct focused-key input and the small-screen
renderer without permissions. `examples/camera` exercises the trusted
`camera.capture` permission, denial handling and a fixed 320x170 capture frame.
Both applications are `no_std`, use only `cp0-sdk`, build to WebAssembly and run
under the PC simulator.

The simulator key vocabulary includes digits and calculator operators. CI runs
`12 + 3 =`, verifies the displayed result through frame submission counts, and
captures a deterministic camera fixture after an Enter event. PPM frames and
JSON profiles stay under ignored `target/` output.

## Validation

- Rust SDK unit tests cover canvas size checks and clipped drawing.
- The LVGL adapter compiles as freestanding wasm32 C11 with warnings as errors.
- Calculator and Camera build through `cp0ctl` and run in the simulator.
- Capability profiling verifies that only Camera calls `camera.capture`.
- Visual inspection confirms both reference layouts fit the 320x150 surface.

Phase 4E freezes the flat WAMR ABI, generates Runtime/C/Rust bindings from one
machine-readable contract and validates WIT mapping and signature compatibility.
