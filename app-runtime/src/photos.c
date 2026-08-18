#define _POSIX_C_SOURCE 200809L

#include "photos.h"
#include "broker_client.h"

#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <sys/stat.h>
#include <unistd.h>

#define CP0_PHOTO_FRAME_BYTES (320U * 170U * 2U)

static int32_t load_photo_descriptor(int32_t broker_result, int descriptor,
                                     uint8_t *pixels, size_t pixel_bytes) {
    struct stat metadata;
    size_t offset = 0;
    int flags;
    if (broker_result != CP0_BROKER_OK)
        return broker_result;
    if (descriptor < 0)
        return CP0_BROKER_INTERNAL;
    flags = fcntl(descriptor, F_GETFL);
    if (fstat(descriptor, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
        metadata.st_size < 0 ||
        (uint64_t)metadata.st_size != CP0_PHOTO_FRAME_BYTES || flags < 0 ||
        (flags & O_ACCMODE) != O_RDONLY) {
        close(descriptor);
        return CP0_BROKER_INTERNAL;
    }
    while (offset < pixel_bytes) {
        ssize_t count = pread(descriptor, pixels + offset,
                              pixel_bytes - offset, (off_t)offset);
        if (count < 0 && errno == EINTR)
            continue;
        if (count <= 0) {
            close(descriptor);
            return CP0_BROKER_UNAVAILABLE;
        }
        offset += (size_t)count;
    }
    if (close(descriptor) != 0)
        return CP0_BROKER_UNAVAILABLE;
    return CP0_BROKER_OK;
}

int32_t cp0_photos_load_rgb565(uint64_t photo_id, uint8_t *pixels,
                               size_t pixel_bytes) {
    int descriptor = -1;
    int32_t result;

    if (photo_id == 0U || pixels == NULL ||
        pixel_bytes != CP0_PHOTO_FRAME_BYTES)
        return CP0_BROKER_INVALID_ARGUMENT;
    result = cp0_broker_photo_load_rgb565(photo_id, &descriptor);
    return load_photo_descriptor(result, descriptor, pixels, pixel_bytes);
}

int32_t cp0_photos_load_view_rgb565(uint64_t photo_id, uint32_t zoom_level,
                                    int32_t pan_x, int32_t pan_y,
                                    uint8_t *pixels, size_t pixel_bytes) {
    int descriptor = -1;
    int32_t result;

    if (photo_id == 0U || zoom_level > 2U || pan_x < -1000 || pan_x > 1000 ||
        pan_y < -1000 || pan_y > 1000 || pixels == NULL ||
        pixel_bytes != CP0_PHOTO_FRAME_BYTES)
        return CP0_BROKER_INVALID_ARGUMENT;
    result = cp0_broker_photo_load_view_rgb565(photo_id, zoom_level, pan_x,
                                               pan_y, &descriptor);
    return load_photo_descriptor(result, descriptor, pixels, pixel_bytes);
}
