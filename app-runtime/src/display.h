#ifndef CARDPUTERZERO_DISPLAY_H
#define CARDPUTERZERO_DISPLAY_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

bool cp0_display_initialize(int socket_fd, const char *app_id, bool immersive);
void cp0_display_destroy(void);
uint32_t cp0_display_dimensions(void);
int cp0_display_present_rgb565(const uint8_t *pixels, size_t pixel_bytes,
                               const uint8_t *damage, size_t damage_bytes);
int cp0_display_wait(int timeout_milliseconds);

#endif
