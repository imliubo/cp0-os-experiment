#include "cardputerzero_lvgl.h"

#include <cardputerzero.h>
#include <stddef.h>

static cp0_lvgl_context_t *active_context;
static uint32_t last_key;
static lv_indev_state_t last_key_state = LV_INDEV_STATE_RELEASED;

static uint32_t cp0_lvgl_tick(void) {
    return (uint32_t)cp0_monotonic_milliseconds();
}

static void cp0_lvgl_flush(lv_display_t *display, const lv_area_t *area,
                           uint8_t *pixels) {
    cp0_rect_t damage;
    cp0_result_t result;
    int32_t x1;
    int32_t y1;
    int32_t x2;
    int32_t y2;

    if (active_context == NULL || display != active_context->display ||
        area == NULL || pixels == NULL) {
        lv_display_flush_ready(display);
        return;
    }
    x1 = area->x1 < 0 ? 0 : area->x1;
    y1 = area->y1 < 0 ? 0 : area->y1;
    x2 = area->x2 >= (int32_t)CP0_DISPLAY_WIDTH
             ? (int32_t)CP0_DISPLAY_WIDTH - 1
             : area->x2;
    y2 = area->y2 >= (int32_t)active_context->height
             ? (int32_t)active_context->height - 1
             : area->y2;
    if (x1 > x2 || y1 > y2) {
        lv_display_flush_ready(display);
        return;
    }
    damage.x = (uint16_t)x1;
    damage.y = (uint16_t)y1;
    damage.width = (uint16_t)(x2 - x1 + 1);
    damage.height = (uint16_t)(y2 - y1 + 1);
    result = cp0_present_rgb565(pixels, active_context->frame_bytes, &damage,
                                sizeof(damage));
    (void)result;
    lv_display_flush_ready(display);
}

static uint32_t cp0_lvgl_key(uint16_t code) {
    switch (code) {
    case 103U:
        return LV_KEY_UP;
    case 108U:
        return LV_KEY_DOWN;
    case 105U:
        return LV_KEY_LEFT;
    case 106U:
        return LV_KEY_RIGHT;
    case 28U:
        return LV_KEY_ENTER;
    case 1U:
        return LV_KEY_ESC;
    case 14U:
        return LV_KEY_BACKSPACE;
    default:
        return code;
    }
}

static void cp0_lvgl_read_key(lv_indev_t *input, lv_indev_data_t *data) {
    cp0_key_event_t event;
    int32_t result;

    (void)input;
    result = cp0_poll_key_event(&event, sizeof(event), 0);
    if (result == 1) {
        last_key = cp0_lvgl_key(event.code);
        last_key_state = event.pressed ? LV_INDEV_STATE_PRESSED
                                       : LV_INDEV_STATE_RELEASED;
    }
    data->key = last_key;
    data->state = last_key_state;
    data->continue_reading = result == 1;
}

int32_t cp0_lvgl_init(cp0_lvgl_context_t *context, void *frame_buffer_a,
                      void *frame_buffer_b, uint32_t buffer_bytes,
                      uint8_t immersive) {
    uint16_t height = immersive != 0U ? CP0_DISPLAY_HEIGHT
                                      : CP0_STANDARD_DISPLAY_HEIGHT;
    uint32_t expected = CP0_DISPLAY_WIDTH * (uint32_t)height * sizeof(uint16_t);

    if (context == NULL || frame_buffer_a == NULL || frame_buffer_b == NULL ||
        buffer_bytes != expected || active_context != NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    context->display = NULL;
    context->keyboard = NULL;
    context->height = 0U;
    context->frame_bytes = 0U;
    lv_init();
    context->display = lv_display_create(CP0_DISPLAY_WIDTH, height);
    if (context->display == NULL)
        return CP0_ERROR_RESOURCE_LIMIT;
    context->keyboard = lv_indev_create();
    if (context->keyboard == NULL) {
        lv_display_delete(context->display);
        context->display = NULL;
        return CP0_ERROR_RESOURCE_LIMIT;
    }
    context->height = height;
    context->frame_bytes = expected;
    active_context = context;

    lv_tick_set_cb(cp0_lvgl_tick);
    lv_display_set_color_format(context->display, LV_COLOR_FORMAT_RGB565);
    lv_display_set_buffers(context->display, frame_buffer_a, frame_buffer_b,
                           buffer_bytes, LV_DISPLAY_RENDER_MODE_FULL);
    lv_display_set_flush_cb(context->display, cp0_lvgl_flush);
    lv_indev_set_type(context->keyboard, LV_INDEV_TYPE_KEYPAD);
    lv_indev_set_read_cb(context->keyboard, cp0_lvgl_read_key);
    return CP0_OK;
}

int32_t cp0_lvgl_run_once(uint32_t maximum_wait_milliseconds) {
    uint32_t wait;

    if (active_context == NULL ||
        maximum_wait_milliseconds > CP0_MAX_WAIT_MILLISECONDS)
        return CP0_ERROR_INVALID_ARGUMENT;
    wait = lv_timer_handler();
    if (wait > maximum_wait_milliseconds)
        wait = maximum_wait_milliseconds;
    return cp0_wait_event((int32_t)wait);
}
