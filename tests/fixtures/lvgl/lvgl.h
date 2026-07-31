#ifndef TEST_LVGL_H
#define TEST_LVGL_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define LV_COLOR_FORMAT_RGB565 1
#define LV_DISPLAY_RENDER_MODE_FULL 1
#define LV_INDEV_TYPE_KEYPAD 1
#define LV_KEY_UP 17U
#define LV_KEY_DOWN 18U
#define LV_KEY_LEFT 19U
#define LV_KEY_RIGHT 20U
#define LV_KEY_ENTER 10U
#define LV_KEY_ESC 27U
#define LV_KEY_BACKSPACE 8U

typedef struct lv_display_t lv_display_t;
typedef struct lv_indev_t lv_indev_t;
typedef enum lv_indev_state_t {
    LV_INDEV_STATE_RELEASED,
    LV_INDEV_STATE_PRESSED,
} lv_indev_state_t;
typedef struct lv_area_t {
    int32_t x1;
    int32_t y1;
    int32_t x2;
    int32_t y2;
} lv_area_t;
typedef struct lv_indev_data_t {
    uint32_t key;
    lv_indev_state_t state;
    bool continue_reading;
} lv_indev_data_t;

void lv_init(void);
lv_display_t *lv_display_create(int32_t width, int32_t height);
void lv_display_delete(lv_display_t *display);
void lv_display_set_color_format(lv_display_t *display, int32_t format);
void lv_display_set_buffers(lv_display_t *display, void *first, void *second,
                            uint32_t bytes, int32_t mode);
void lv_display_set_flush_cb(
    lv_display_t *display,
    void (*callback)(lv_display_t *, const lv_area_t *, uint8_t *));
void lv_display_flush_ready(lv_display_t *display);
lv_indev_t *lv_indev_create(void);
void lv_indev_set_type(lv_indev_t *input, int32_t type);
void lv_indev_set_read_cb(lv_indev_t *input,
                          void (*callback)(lv_indev_t *, lv_indev_data_t *));
void lv_tick_set_cb(uint32_t (*callback)(void));
uint32_t lv_timer_handler(void);

#endif
