#include "cp0_task_thumbnail.h"

#include <assert.h>
#include <stdint.h>
#include <stdlib.h>

static uint16_t rgb565(uint8_t red, uint8_t green, uint8_t blue)
{
    return (uint16_t)(((uint16_t)(red & 0xf8U) << 8U) |
                      ((uint16_t)(green & 0xfcU) << 3U) | (blue >> 3U));
}

int main(void)
{
    uint32_t *source = calloc(CP0_TASK_FRAME_PIXELS, sizeof(*source));
    uint16_t *thumbnail = calloc(CP0_TASK_THUMBNAIL_PIXELS,
                                 sizeof(*thumbnail));
    assert(source != NULL);
    assert(thumbnail != NULL);

    assert(!cp0_task_thumbnail_from_xrgb8888(
        NULL, CP0_TASK_FRAME_PIXELS, thumbnail, CP0_TASK_THUMBNAIL_PIXELS));
    assert(!cp0_task_thumbnail_from_xrgb8888(
        source, CP0_TASK_FRAME_PIXELS - 1U, thumbnail,
        CP0_TASK_THUMBNAIL_PIXELS));

    source[0] = 0x00ff0000U;
    source[1] = 0x0000ff00U;
    source[CP0_TASK_FRAME_WIDTH] = 0x000000ffU;
    source[CP0_TASK_FRAME_WIDTH + 1U] = 0x00ffffffU;
    assert(cp0_task_thumbnail_from_xrgb8888(
        source, CP0_TASK_FRAME_PIXELS, thumbnail,
        CP0_TASK_THUMBNAIL_PIXELS));
    assert(thumbnail[0] == rgb565(128, 128, 128));

    const size_t last_source = CP0_TASK_FRAME_PIXELS - 1U;
    source[last_source] = 0x00f81080U;
    source[last_source - 1U] = 0x00f81080U;
    source[last_source - CP0_TASK_FRAME_WIDTH] = 0x00f81080U;
    source[last_source - CP0_TASK_FRAME_WIDTH - 1U] = 0x00f81080U;
    assert(cp0_task_thumbnail_from_xrgb8888(
        source, CP0_TASK_FRAME_PIXELS, thumbnail,
        CP0_TASK_THUMBNAIL_PIXELS));
    assert(thumbnail[CP0_TASK_THUMBNAIL_PIXELS - 1U] ==
           rgb565(248, 16, 128));

    free(thumbnail);
    free(source);
    return 0;
}
