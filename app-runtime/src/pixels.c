#include "pixels.h"

static uint32_t rgb565_to_xrgb8888(const uint8_t *source) {
    uint16_t pixel = (uint16_t)source[0] | (uint16_t)((uint16_t)source[1] << 8U);
    uint32_t red = (uint32_t)((pixel >> 11U) & 0x1fU);
    uint32_t green = (uint32_t)((pixel >> 5U) & 0x3fU);
    uint32_t blue = (uint32_t)(pixel & 0x1fU);

    red = (red << 3U) | (red >> 2U);
    green = (green << 2U) | (green >> 4U);
    blue = (blue << 3U) | (blue >> 2U);
    return (red << 16U) | (green << 8U) | blue;
}

bool cp0_damage_is_valid(const struct cp0_damage_rect *rectangles,
                         size_t rectangle_count, uint16_t content_height) {
    size_t index;

    if (content_height != CP0_STANDARD_CONTENT_HEIGHT &&
        content_height != CP0_DISPLAY_HEIGHT)
        return false;
    if (rectangle_count > CP0_MAX_DAMAGE_RECTS)
        return false;
    if (rectangle_count > 0U && rectangles == NULL)
        return false;

    for (index = 0; index < rectangle_count; index++) {
        const struct cp0_damage_rect *rectangle = &rectangles[index];
        uint32_t right = (uint32_t)rectangle->x + rectangle->width;
        uint32_t bottom = (uint32_t)rectangle->y + rectangle->height;

        if (rectangle->width == 0U || rectangle->height == 0U ||
            right > CP0_DISPLAY_WIDTH || bottom > content_height)
            return false;
    }
    return true;
}

static void convert_rectangle(uint32_t *destination, const uint8_t *source,
                              uint16_t destination_offset_y,
                              const struct cp0_damage_rect *rectangle) {
    uint32_t y;

    for (y = rectangle->y; y < (uint32_t)rectangle->y + rectangle->height;
         y++) {
        uint32_t x;
        for (x = rectangle->x;
             x < (uint32_t)rectangle->x + rectangle->width; x++) {
            size_t source_index =
                ((size_t)y * CP0_DISPLAY_WIDTH + x) * sizeof(uint16_t);
            size_t destination_index =
                ((size_t)(y + destination_offset_y) * CP0_DISPLAY_WIDTH + x);
            destination[destination_index] =
                rgb565_to_xrgb8888(&source[source_index]);
        }
    }
}

void cp0_convert_rgb565(uint32_t *destination, const uint8_t *source,
                        uint16_t content_height, uint16_t destination_offset_y,
                        const struct cp0_damage_rect *rectangles,
                        size_t rectangle_count) {
    struct cp0_damage_rect full = {
        .x = 0,
        .y = 0,
        .width = CP0_DISPLAY_WIDTH,
        .height = content_height,
    };
    size_t index;

    if (rectangle_count == 0U) {
        convert_rectangle(destination, source, destination_offset_y, &full);
        return;
    }
    for (index = 0; index < rectangle_count; index++)
        convert_rectangle(destination, source, destination_offset_y,
                          &rectangles[index]);
}
