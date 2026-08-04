#ifndef CP0_SCREENSHOT_STORE_H
#define CP0_SCREENSHOT_STORE_H

#include <stddef.h>
#include <stdint.h>

#define CP0_SCREENSHOT_DIRECTORY "/var/lib/cardputerzero/screenshots"
#define CP0_SCREENSHOT_WIDTH 320U
#define CP0_SCREENSHOT_HEIGHT 170U
#define CP0_SCREENSHOT_NAME_MAX 48U

int cp0_screenshot_store_save(
    const char *directory, const uint32_t *pixels, size_t pixel_count,
    char saved_name[CP0_SCREENSHOT_NAME_MAX]);

#endif
