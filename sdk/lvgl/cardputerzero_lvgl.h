#ifndef CARDPUTERZERO_LVGL_H
#define CARDPUTERZERO_LVGL_H

#include <stdint.h>
#include <lvgl.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct cp0_lvgl_context {
    lv_display_t *display;
    lv_indev_t *keyboard;
    uint16_t height;
    uint32_t frame_bytes;
} cp0_lvgl_context_t;

int32_t cp0_lvgl_init(cp0_lvgl_context_t *context, void *frame_buffer_a,
                      void *frame_buffer_b, uint32_t buffer_bytes,
                      uint8_t immersive);
int32_t cp0_lvgl_run_once(uint32_t maximum_wait_milliseconds);

#ifdef __cplusplus
}
#endif

#endif
