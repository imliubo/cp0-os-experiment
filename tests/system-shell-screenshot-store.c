#define _DARWIN_C_SOURCE
#define _POSIX_C_SOURCE 200809L

#include "cp0_screenshot_store.h"

#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <png.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static size_t png_count(const char *directory)
{
    DIR *stream = opendir(directory);
    struct dirent *entry;
    size_t count = 0;

    assert(stream != NULL);
    while ((entry = readdir(stream)) != NULL) {
        size_t length = strlen(entry->d_name);
        if (strncmp(entry->d_name, "cp0-", 4) == 0 && length > 4 &&
            strcmp(entry->d_name + length - 4, ".png") == 0)
            count++;
    }
    assert(closedir(stream) == 0);
    return count;
}

static void verify_png(const char *directory, const char *name)
{
    char path[512];
    png_structp png;
    png_infop info;
    png_byte signature[8];
    FILE *file;

    assert(snprintf(path, sizeof(path), "%s/%s", directory, name) > 0);
    file = fopen(path, "rb");
    assert(file != NULL);
    assert(fread(signature, 1, sizeof(signature), file) == sizeof(signature));
    assert(png_sig_cmp(signature, 0, sizeof(signature)) == 0);
    png = png_create_read_struct(PNG_LIBPNG_VER_STRING, NULL, NULL, NULL);
    assert(png != NULL);
    info = png_create_info_struct(png);
    assert(info != NULL);
    assert(setjmp(png_jmpbuf(png)) == 0);
    png_init_io(png, file);
    png_set_sig_bytes(png, sizeof(signature));
    png_read_info(png, info);
    assert(png_get_image_width(png, info) == CP0_SCREENSHOT_WIDTH);
    assert(png_get_image_height(png, info) == CP0_SCREENSHOT_HEIGHT);
    assert(png_get_color_type(png, info) == PNG_COLOR_TYPE_RGB);
    png_destroy_read_struct(&png, &info, NULL);
    assert(fclose(file) == 0);
}

int main(int argc, char **argv)
{
    char link_path[512];
    char saved_name[CP0_SCREENSHOT_NAME_MAX];
    static uint32_t pixels[CP0_SCREENSHOT_WIDTH * CP0_SCREENSHOT_HEIGHT];
    const char *directory;

    assert(argc == 2);
    directory = argv[1];
    assert(mkdir(directory, 0700) == 0);
    assert(chmod(directory, 0700) == 0);
    for (size_t y = 0; y < CP0_SCREENSHOT_HEIGHT; y++) {
        for (size_t x = 0; x < CP0_SCREENSHOT_WIDTH; x++) {
            pixels[y * CP0_SCREENSHOT_WIDTH + x] =
                ((uint32_t)(x & 0xffU) << 16) |
                ((uint32_t)(y & 0xffU) << 8) | 0x3fU;
        }
    }

    errno = 0;
    assert(cp0_screenshot_store_save(directory, pixels, 1, saved_name) == -1);
    assert(errno == EINVAL);
    assert(cp0_screenshot_store_save("relative", pixels,
                                     CP0_SCREENSHOT_WIDTH *
                                         CP0_SCREENSHOT_HEIGHT,
                                     saved_name) == -1);

    assert(chmod(directory, 0777) == 0);
    errno = 0;
    assert(cp0_screenshot_store_save(directory, pixels,
                                     CP0_SCREENSHOT_WIDTH *
                                         CP0_SCREENSHOT_HEIGHT,
                                     saved_name) == -1);
    assert(errno == EPERM);
    assert(chmod(directory, 0700) == 0);

    assert(snprintf(link_path, sizeof(link_path), "%s-link", directory) > 0);
    assert(symlink(directory, link_path) == 0);
    assert(cp0_screenshot_store_save(link_path, pixels,
                                     CP0_SCREENSHOT_WIDTH *
                                         CP0_SCREENSHOT_HEIGHT,
                                     saved_name) == -1);
    assert(unlink(link_path) == 0);

    for (size_t index = 0; index < CP0_SCREENSHOT_MAX_FILES + 3U; index++) {
        pixels[0] = (uint32_t)index;
        assert(cp0_screenshot_store_save(directory, pixels,
                                         CP0_SCREENSHOT_WIDTH *
                                             CP0_SCREENSHOT_HEIGHT,
                                         saved_name) == 0);
        assert(strncmp(saved_name, "cp0-", 4) == 0);
        verify_png(directory, saved_name);
    }
    assert(png_count(directory) == CP0_SCREENSHOT_MAX_FILES);

    DIR *stream = opendir(directory);
    struct dirent *entry;
    assert(stream != NULL);
    while ((entry = readdir(stream)) != NULL) {
        char path[512];
        if (entry->d_name[0] == '.')
            continue;
        assert(snprintf(path, sizeof(path), "%s/%s", directory,
                        entry->d_name) > 0);
        assert(unlink(path) == 0);
    }
    assert(closedir(stream) == 0);
    assert(rmdir(directory) == 0);
    return 0;
}
