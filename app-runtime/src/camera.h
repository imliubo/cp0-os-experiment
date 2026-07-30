#ifndef CARDPUTERZERO_CAMERA_H
#define CARDPUTERZERO_CAMERA_H

#include <stddef.h>
#include <stdint.h>

int32_t cp0_camera_capture_rgb565(uint8_t *pixels, size_t pixel_bytes);

#endif
