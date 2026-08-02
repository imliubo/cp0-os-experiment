#ifndef CP0_DISPLAY_CLIENT_H
#define CP0_DISPLAY_CLIENT_H

#include <stdbool.h>

#define CP0_DISPLAY_BRIGHTNESS_MIN_PERCENT 5U
#define CP0_DISPLAY_BRIGHTNESS_MAX_PERCENT 100U

enum cp0_display_direction {
    CP0_DISPLAY_DECREASE,
    CP0_DISPLAY_INCREASE,
};

enum cp0_display_result {
    CP0_DISPLAY_FAILED = -1,
    CP0_DISPLAY_OK = 0,
    CP0_DISPLAY_UNAVAILABLE = 1,
};

struct cp0_display_state {
    bool available;
    unsigned int brightness_percent;
};

int cp0_display_get_state(struct cp0_display_state *state);
int cp0_display_set_brightness(unsigned int percent,
                               struct cp0_display_state *state);
int cp0_display_adjust_brightness(enum cp0_display_direction direction,
                                  struct cp0_display_state *state);

#endif
