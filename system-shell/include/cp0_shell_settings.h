#ifndef CP0_SHELL_SETTINGS_H
#define CP0_SHELL_SETTINGS_H

#include <stdbool.h>

#define CP0_SHELL_THEME_COUNT 3U
#define CP0_SHELL_TIMEOUT_COUNT 4U

struct cp0_shell_settings {
    unsigned int theme;
    unsigned int screen_timeout;
    bool key_sounds;
};

void cp0_shell_settings_defaults(struct cp0_shell_settings *settings);
bool cp0_shell_settings_load(const char *path,
                             struct cp0_shell_settings *settings);
bool cp0_shell_settings_save(const char *path,
                             const struct cp0_shell_settings *settings);

#endif
