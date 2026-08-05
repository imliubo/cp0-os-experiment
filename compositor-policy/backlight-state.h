#ifndef CP0_BACKLIGHT_STATE_H
#define CP0_BACKLIGHT_STATE_H

#include <stdbool.h>

struct cp0_backlight_state {
    unsigned int saved_brightness;
    bool sleeping;
};

void cp0_backlight_state_init(struct cp0_backlight_state *state);
bool cp0_backlight_sleep(struct cp0_backlight_state *state,
                         const char *brightness_path,
                         const char *saved_state_path);
bool cp0_backlight_wake(struct cp0_backlight_state *state,
                        const char *brightness_path);

#endif
