#include "pixels.h"

#include <assert.h>
#include <stdint.h>
#include <stdlib.h>

int main(void) {
    const size_t source_bytes =
        (size_t)CP0_DISPLAY_WIDTH * CP0_STANDARD_CONTENT_HEIGHT * 2U;
    const size_t destination_pixels =
        (size_t)CP0_DISPLAY_WIDTH * CP0_DISPLAY_HEIGHT;
    uint8_t *source = calloc(source_bytes, 1U);
    uint32_t *destination = calloc(destination_pixels, sizeof(uint32_t));
    struct cp0_damage_rect pixel = {.x = 1, .y = 2, .width = 1, .height = 1};
    struct cp0_damage_rect overflow = {
        .x = 319,
        .y = 0,
        .width = 2,
        .height = 1,
    };
    size_t source_index;
    size_t destination_index;

    assert(source != NULL && destination != NULL);
    assert(cp0_damage_is_valid(NULL, 0, CP0_STANDARD_CONTENT_HEIGHT));
    assert(!cp0_damage_is_valid(&overflow, 1, CP0_STANDARD_CONTENT_HEIGHT));
    assert(!cp0_damage_is_valid(NULL, 1, CP0_STANDARD_CONTENT_HEIGHT));
    assert(!cp0_damage_is_valid(&pixel, CP0_MAX_DAMAGE_RECTS + 1U,
                                CP0_STANDARD_CONTENT_HEIGHT));

    source_index =
        ((size_t)pixel.y * CP0_DISPLAY_WIDTH + pixel.x) * sizeof(uint16_t);
    source[source_index] = 0x00U;
    source[source_index + 1U] = 0xf8U;
    cp0_convert_rgb565(destination, source, CP0_STANDARD_CONTENT_HEIGHT,
                       CP0_STANDARD_CONTENT_OFFSET_Y, &pixel, 1);
    destination_index =
        ((size_t)(pixel.y + CP0_STANDARD_CONTENT_OFFSET_Y) * CP0_DISPLAY_WIDTH +
         pixel.x);
    assert(destination[destination_index] == 0x00ff0000U);
    assert(destination[(size_t)pixel.y * CP0_DISPLAY_WIDTH + pixel.x] == 0U);

    source[0] = 0xe0U;
    source[1] = 0x07U;
    cp0_convert_rgb565(destination, source, CP0_STANDARD_CONTENT_HEIGHT,
                       CP0_STANDARD_CONTENT_OFFSET_Y, NULL, 0);
    assert(destination[(size_t)CP0_STANDARD_CONTENT_OFFSET_Y *
                       CP0_DISPLAY_WIDTH] == 0x0000ff00U);

    free(destination);
    free(source);
    return 0;
}
