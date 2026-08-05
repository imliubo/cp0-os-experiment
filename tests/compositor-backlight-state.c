#include "backlight-state.h"

#include <assert.h>
#include <stdio.h>
#include <string.h>

static void
write_value(const char *path, const char *value)
{
    FILE *file = fopen(path, "w");

    assert(file != NULL);
    assert(fputs(value, file) >= 0);
    assert(fclose(file) == 0);
}

static unsigned int
read_value(const char *path)
{
    unsigned int value = 0;
    FILE *file = fopen(path, "r");

    assert(file != NULL);
    assert(fscanf(file, "%u", &value) == 1);
    assert(fclose(file) == 0);
    return value;
}

int
main(int argc, char *argv[])
{
    struct cp0_backlight_state state;
    char brightness[1024];
    char saved[1024];

    assert(argc == 2);
    assert(snprintf(brightness, sizeof(brightness), "%s/brightness", argv[1]) > 0);
    assert(snprintf(saved, sizeof(saved), "%s/saved", argv[1]) > 0);

    cp0_backlight_state_init(&state);
    write_value(brightness, "127\n");
    assert(cp0_backlight_sleep(&state, brightness, saved));
    assert(state.sleeping);
    assert(state.saved_brightness == 127);
    assert(read_value(brightness) == 0);
    assert(read_value(saved) == 127);
    assert(cp0_backlight_sleep(&state, brightness, saved));

    assert(cp0_backlight_wake(&state, brightness));
    assert(!state.sleeping);
    assert(read_value(brightness) == 127);
    assert(cp0_backlight_wake(&state, brightness));

    write_value(brightness, "0\n");
    assert(!cp0_backlight_sleep(&state, brightness, saved));
    write_value(brightness, "invalid\n");
    assert(!cp0_backlight_sleep(&state, brightness, saved));
    assert(!state.sleeping);
    return 0;
}
