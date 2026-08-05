#ifndef CARDPUTERZERO_PHOTOS_H
#define CARDPUTERZERO_PHOTOS_H

#include <stddef.h>
#include <stdint.h>

int32_t cp0_photos_load_rgb565(uint64_t photo_id, uint8_t *pixels,
                               size_t pixel_bytes);

#endif
