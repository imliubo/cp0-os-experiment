#include "cp0_shell_settings.h"

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(void)
{
    char directory[] = "/tmp/cp0-shell-settings-XXXXXX";
    char path[256];
    struct cp0_shell_settings settings;
    struct cp0_shell_settings loaded;

    assert(mkdtemp(directory) != NULL);
    assert(snprintf(path, sizeof(path), "%s/settings.conf", directory) > 0);
    cp0_shell_settings_defaults(&settings);
    assert(settings.theme == 0);
    assert(settings.screen_timeout == 1);
    assert(settings.key_sounds);
    assert(!cp0_shell_settings_load(path, &loaded));

    FILE *stream = fopen(path, "w");
    assert(stream != NULL);
    assert(fputs("version=1\n", stream) >= 0);
    assert(fclose(stream) == 0);
    loaded = (struct cp0_shell_settings){
        .theme = 2,
        .screen_timeout = 3,
        .key_sounds = false,
    };
    assert(!cp0_shell_settings_load(path, &loaded));
    assert(loaded.theme == 2);
    assert(loaded.screen_timeout == 3);
    assert(!loaded.key_sounds);

    settings.theme = 2;
    settings.screen_timeout = 3;
    settings.key_sounds = false;
    assert(cp0_shell_settings_save(path, &settings));
    assert(cp0_shell_settings_load(path, &loaded));
    assert(settings.theme == loaded.theme);
    assert(settings.screen_timeout == loaded.screen_timeout);
    assert(settings.key_sounds == loaded.key_sounds);

    stream = fopen(path, "w");
    assert(stream != NULL);
    assert(fputs("version=1\ntheme=99\nscreen_timeout=0\nkey_sounds=1\n",
                 stream) >= 0);
    assert(fclose(stream) == 0);
    assert(!cp0_shell_settings_load(path, &loaded));
    assert(unlink(path) == 0);
    assert(rmdir(directory) == 0);
    return 0;
}
