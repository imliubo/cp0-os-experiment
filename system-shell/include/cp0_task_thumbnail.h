#ifndef CP0_TASK_THUMBNAIL_H
#define CP0_TASK_THUMBNAIL_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define CP0_TASK_FRAME_WIDTH 320U
#define CP0_TASK_FRAME_HEIGHT 170U
#define CP0_TASK_THUMBNAIL_WIDTH 160U
#define CP0_TASK_THUMBNAIL_HEIGHT 85U
#define CP0_TASK_FRAME_PIXELS (CP0_TASK_FRAME_WIDTH * CP0_TASK_FRAME_HEIGHT)
#define CP0_TASK_THUMBNAIL_PIXELS \
    (CP0_TASK_THUMBNAIL_WIDTH * CP0_TASK_THUMBNAIL_HEIGHT)

bool cp0_task_thumbnail_from_xrgb8888(const uint32_t *source,
                                      size_t source_pixels,
                                      uint16_t *destination,
                                      size_t destination_pixels);

#endif
