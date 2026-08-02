#define _POSIX_C_SOURCE 200809L

#include "cp0_shell_settings.h"

#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

void cp0_shell_settings_defaults(struct cp0_shell_settings *settings)
{
    if (settings == NULL)
        return;
    *settings = (struct cp0_shell_settings){
        .theme = 0,
        .screen_timeout = 1,
        .key_sounds = true,
    };
}

static bool valid(const struct cp0_shell_settings *settings)
{
    return settings != NULL && settings->theme < CP0_SHELL_THEME_COUNT &&
           settings->screen_timeout < CP0_SHELL_TIMEOUT_COUNT;
}

bool cp0_shell_settings_load(const char *path,
                             struct cp0_shell_settings *settings)
{
    FILE *stream;
    unsigned int version;
    unsigned int theme;
    unsigned int timeout;
    unsigned int key_sounds;
    char trailing;

    if (path == NULL || settings == NULL)
        return false;
    stream = fopen(path, "re");
    if (stream == NULL)
        return false;
    bool parsed = fscanf(stream,
                         "version=%u\ntheme=%u\nscreen_timeout=%u\n"
                         "key_sounds=%u\n%c",
                         &version, &theme, &timeout, &key_sounds, &trailing) == 4;
    bool closed = fclose(stream) == 0;
    struct cp0_shell_settings candidate = {
        .theme = theme,
        .screen_timeout = timeout,
        .key_sounds = key_sounds == 1,
    };
    if (!parsed || !closed || version != 1 || key_sounds > 1 ||
        !valid(&candidate))
        return false;
    *settings = candidate;
    return true;
}

bool cp0_shell_settings_save(const char *path,
                             const struct cp0_shell_settings *settings)
{
    char temporary[512];
    int descriptor;
    FILE *stream;

    if (path == NULL || !valid(settings))
        return false;
    int length = snprintf(temporary, sizeof(temporary), "%s.tmp", path);
    if (length <= 0 || (size_t)length >= sizeof(temporary))
        return false;
    descriptor = open(temporary, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC,
                      0600);
    if (descriptor < 0)
        return false;
    stream = fdopen(descriptor, "w");
    if (stream == NULL) {
        close(descriptor);
        return false;
    }
    bool written = fprintf(
                       stream,
                       "version=1\ntheme=%u\nscreen_timeout=%u\nkey_sounds=%u\n",
                       settings->theme, settings->screen_timeout,
                       settings->key_sounds ? 1U : 0U) > 0 &&
                   fflush(stream) == 0 && fsync(descriptor) == 0;
    bool closed = fclose(stream) == 0;
    return written && closed && rename(temporary, path) == 0;
}
