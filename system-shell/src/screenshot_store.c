#define _DARWIN_C_SOURCE
#define _POSIX_C_SOURCE 200809L

#include "cp0_screenshot_store.h"

#include <errno.h>
#include <fcntl.h>
#include <png.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

static unsigned int screenshot_serial;

static int write_png(FILE *file, const uint32_t *pixels)
{
    png_structp png = NULL;
    png_infop info = NULL;
    uint8_t row[CP0_SCREENSHOT_WIDTH * 3U];
    volatile int result = -1;

    png = png_create_write_struct(PNG_LIBPNG_VER_STRING, NULL, NULL, NULL);
    if (png == NULL)
        return -1;
    info = png_create_info_struct(png);
    if (info == NULL)
        goto out;
    if (setjmp(png_jmpbuf(png)) != 0)
        goto out;

    png_init_io(png, file);
    png_set_compression_level(png, 1);
    png_set_IHDR(png, info, CP0_SCREENSHOT_WIDTH, CP0_SCREENSHOT_HEIGHT, 8,
                 PNG_COLOR_TYPE_RGB, PNG_INTERLACE_NONE,
                 PNG_COMPRESSION_TYPE_DEFAULT, PNG_FILTER_TYPE_DEFAULT);
    png_write_info(png, info);
    for (size_t y = 0; y < CP0_SCREENSHOT_HEIGHT; y++) {
        for (size_t x = 0; x < CP0_SCREENSHOT_WIDTH; x++) {
            uint32_t pixel = pixels[y * CP0_SCREENSHOT_WIDTH + x];
            row[x * 3U] = (uint8_t)(pixel >> 16);
            row[x * 3U + 1U] = (uint8_t)(pixel >> 8);
            row[x * 3U + 2U] = (uint8_t)pixel;
        }
        png_write_row(png, row);
    }
    png_write_end(png, info);
    result = 0;

out:
    png_destroy_write_struct(&png, info == NULL ? NULL : &info);
    return result;
}

static int open_screenshot_directory(const char *path)
{
    struct stat status;
    int fd;

    if (path == NULL || path[0] != '/') {
        errno = EINVAL;
        return -1;
    }
    fd = open(path, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0)
        return -1;
    if (fstat(fd, &status) < 0 || !S_ISDIR(status.st_mode) ||
        status.st_uid != geteuid() || (status.st_mode & 0022) != 0) {
        close(fd);
        errno = EPERM;
        return -1;
    }
    return fd;
}

int cp0_screenshot_store_save(
    const char *directory, const uint32_t *pixels, size_t pixel_count,
    char saved_name[CP0_SCREENSHOT_NAME_MAX])
{
    char final_name[CP0_SCREENSHOT_NAME_MAX];
    char temporary_name[CP0_SCREENSHOT_NAME_MAX + 6U];
    struct tm utc;
    time_t now;
    int directory_fd = -1;
    int file_fd = -1;
    FILE *file = NULL;
    bool temporary_exists = false;
    bool final_exists = false;
    int result = -1;

    if (pixels == NULL ||
        pixel_count != CP0_SCREENSHOT_WIDTH * CP0_SCREENSHOT_HEIGHT ||
        saved_name == NULL) {
        errno = EINVAL;
        return -1;
    }
    saved_name[0] = '\0';
    directory_fd = open_screenshot_directory(directory);
    if (directory_fd < 0)
        return -1;

    now = time(NULL);
    if (now == (time_t)-1 || gmtime_r(&now, &utc) == NULL)
        goto out;
    for (unsigned int attempt = 0; attempt < 1000; attempt++) {
        unsigned int serial = screenshot_serial++ % 1000U;
        snprintf(final_name, sizeof(final_name),
                 "cp0-%04d%02d%02dT%02d%02d%02d-%03u.png",
                 utc.tm_year + 1900, utc.tm_mon + 1, utc.tm_mday,
                 utc.tm_hour, utc.tm_min, utc.tm_sec, serial);
        snprintf(temporary_name, sizeof(temporary_name), ".%s.tmp",
                 final_name);
        if (faccessat(directory_fd, final_name, F_OK, 0) == 0)
            continue;
        if (errno != ENOENT)
            goto out;
        file_fd = openat(directory_fd, temporary_name,
                         O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW,
                         0600);
        if (file_fd >= 0) {
            temporary_exists = true;
            break;
        }
        if (errno != EEXIST)
            goto out;
    }
    if (file_fd < 0) {
        errno = EEXIST;
        goto out;
    }
    file = fdopen(file_fd, "wb");
    if (file == NULL)
        goto out;
    file_fd = -1;
    if (write_png(file, pixels) < 0 || fflush(file) < 0 ||
        fsync(fileno(file)) < 0 || fclose(file) < 0) {
        file = NULL;
        goto out;
    }
    file = NULL;

    if (linkat(directory_fd, temporary_name, directory_fd, final_name, 0) < 0)
        goto out;
    final_exists = true;
    if (unlinkat(directory_fd, temporary_name, 0) < 0)
        goto out;
    temporary_exists = false;
    snprintf(saved_name, CP0_SCREENSHOT_NAME_MAX, "%s", final_name);
    result = 0;

out:
    if (file != NULL)
        fclose(file);
    else if (file_fd >= 0)
        close(file_fd);
    if (temporary_exists)
        unlinkat(directory_fd, temporary_name, 0);
    if (result < 0 && final_exists)
        unlinkat(directory_fd, final_name, 0);
    if (directory_fd >= 0)
        close(directory_fd);
    return result;
}
