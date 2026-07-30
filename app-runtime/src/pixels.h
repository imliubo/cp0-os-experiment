#ifndef CARDPUTERZERO_PIXELS_H
#define CARDPUTERZERO_PIXELS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define CP0_DISPLAY_WIDTH 320U
#define CP0_DISPLAY_HEIGHT 170U
#define CP0_STANDARD_CONTENT_HEIGHT 150U
#define CP0_STANDARD_CONTENT_OFFSET_Y 20U
#define CP0_MAX_DAMAGE_RECTS 32U

struct cp0_damage_rect {
    uint16_t x;
    uint16_t y;
    uint16_t width;
    uint16_t height;
};

bool cp0_damage_is_valid(const struct cp0_damage_rect *rectangles,
                         size_t rectangle_count, uint16_t content_height);
void cp0_convert_rgb565(uint32_t *destination, const uint8_t *source,
                        uint16_t content_height, uint16_t destination_offset_y,
                        const struct cp0_damage_rect *rectangles,
                        size_t rectangle_count);

#endif
