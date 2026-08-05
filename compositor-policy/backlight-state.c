#define _POSIX_C_SOURCE 200809L

#include "backlight-state.h"

#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>

static bool
read_unsigned(const char *path, unsigned int *value)
{
    char buffer[64];
    char *end;
    unsigned long parsed;
    FILE *file = fopen(path, "r");

    if (file == NULL)
        return false;
    if (fgets(buffer, sizeof(buffer), file) == NULL || fclose(file) != 0)
        return false;
    errno = 0;
    parsed = strtoul(buffer, &end, 10);
    if (errno != 0 || end == buffer || parsed > UINT_MAX)
        return false;
    while (isspace((unsigned char)*end))
        end++;
    if (*end != '\0')
        return false;
    *value = (unsigned int)parsed;
    return true;
}

static bool
write_unsigned(const char *path, unsigned int value)
{
    FILE *file = fopen(path, "w");

    if (file == NULL)
        return false;
    if (fprintf(file, "%u\n", value) < 0) {
        fclose(file);
        return false;
    }
    return fclose(file) == 0;
}

void
cp0_backlight_state_init(struct cp0_backlight_state *state)
{
    *state = (struct cp0_backlight_state){0};
}

bool
cp0_backlight_sleep(struct cp0_backlight_state *state,
                    const char *brightness_path,
                    const char *saved_state_path)
{
    unsigned int brightness;

    if (state->sleeping)
        return true;
    if (!read_unsigned(brightness_path, &brightness) || brightness == 0)
        return false;
    if (!write_unsigned(saved_state_path, brightness))
        return false;
    if (!write_unsigned(brightness_path, 0))
        return false;
    state->saved_brightness = brightness;
    state->sleeping = true;
    return true;
}

bool
cp0_backlight_wake(struct cp0_backlight_state *state,
                   const char *brightness_path)
{
    if (!state->sleeping)
        return true;
    if (state->saved_brightness == 0 ||
        !write_unsigned(brightness_path, state->saved_brightness))
        return false;
    state->sleeping = false;
    return true;
}
