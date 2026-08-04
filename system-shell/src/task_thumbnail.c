#include "cp0_task_thumbnail.h"

bool cp0_task_thumbnail_from_xrgb8888(const uint32_t *source,
                                      size_t source_pixels,
                                      uint16_t *destination,
                                      size_t destination_pixels)
{
    if (source == NULL || source_pixels != CP0_TASK_FRAME_PIXELS ||
        destination == NULL ||
        destination_pixels != CP0_TASK_THUMBNAIL_PIXELS)
        return false;

    for (size_t y = 0; y < CP0_TASK_THUMBNAIL_HEIGHT; y++) {
        for (size_t x = 0; x < CP0_TASK_THUMBNAIL_WIDTH; x++) {
            const size_t source_x = x * 2U;
            const size_t source_y = y * 2U;
            const uint32_t *top =
                source + source_y * CP0_TASK_FRAME_WIDTH + source_x;
            const uint32_t *bottom = top + CP0_TASK_FRAME_WIDTH;
            const uint32_t red = ((top[0] >> 16U) & 0xffU) +
                                 ((top[1] >> 16U) & 0xffU) +
                                 ((bottom[0] >> 16U) & 0xffU) +
                                 ((bottom[1] >> 16U) & 0xffU);
            const uint32_t green = ((top[0] >> 8U) & 0xffU) +
                                   ((top[1] >> 8U) & 0xffU) +
                                   ((bottom[0] >> 8U) & 0xffU) +
                                   ((bottom[1] >> 8U) & 0xffU);
            const uint32_t blue = (top[0] & 0xffU) + (top[1] & 0xffU) +
                                  (bottom[0] & 0xffU) +
                                  (bottom[1] & 0xffU);
            const uint32_t average_red = (red + 2U) / 4U;
            const uint32_t average_green = (green + 2U) / 4U;
            const uint32_t average_blue = (blue + 2U) / 4U;

            destination[y * CP0_TASK_THUMBNAIL_WIDTH + x] =
                (uint16_t)(((average_red & 0xf8U) << 8U) |
                           ((average_green & 0xfcU) << 3U) |
                           (average_blue >> 3U));
        }
    }
    return true;
}
