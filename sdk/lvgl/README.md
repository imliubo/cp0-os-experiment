# CardputerZero LVGL 9 adapter

This optional C adapter connects LVGL 9 to the supported CardputerZero SDK ABI.
It does not access DRM, evdev, Wayland or Linux devices. Display flushes use
`cp0_present_rgb565`, keyboard input uses `cp0_poll_key_event`, and scheduling
uses the bounded event wait hostcall.

Applications must compile LVGL and this directory for `wasm32` with the
freestanding C SDK include directory. Allocate two complete RGB565 buffers for
the selected display mode:

```c
#include <cardputerzero_lvgl.h>

static uint8_t first[CP0_DISPLAY_WIDTH * CP0_STANDARD_DISPLAY_HEIGHT * 2U];
static uint8_t second[CP0_DISPLAY_WIDTH * CP0_STANDARD_DISPLAY_HEIGHT * 2U];
static cp0_lvgl_context_t context;

int main(void) {
    if (cp0_lvgl_init(&context, first, second, sizeof(first), 0U) != CP0_OK)
        return 1;
    for (;;)
        if (cp0_lvgl_run_once(250U) < 0)
            return 1;
}
```

The adapter uses `LV_DISPLAY_RENDER_MODE_FULL`, RGB565 color format and one
keypad input device. Printable input comes from the Runtime-produced
`cp0_key_event_t.character`, so LVGL text widgets inherit the same Shift and
`Sym` behavior as first boot. Arrow, Enter, Escape and Backspace evdev codes are
mapped to the corresponding LVGL keys when an event has no character. Only one
LVGL context may exist because an OS application owns one foreground surface.
The standard mode is 320x150 below the trusted 20-pixel status bar; immersive
mode is 320x170.

The build test uses a minimal LVGL 9 declaration fixture to verify the WebAssembly
ABI without vendoring LVGL. A released SDK toolchain will pin and package the
tested upstream LVGL source separately.
