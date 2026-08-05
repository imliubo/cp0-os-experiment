#include "camera.h"
#include "broker_client.h"

#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <sys/stat.h>
#include <unistd.h>

#define CP0_CAMERA_FRAME_BYTES (320U * 170U * 2U)

int32_t cp0_camera_capture_rgb565(uint8_t *pixels, size_t pixel_bytes) {
    struct stat metadata;
    size_t offset = 0;
    int descriptor = -1;
    int flags;
    int seals;
    int32_t result;

    if (pixels == NULL || pixel_bytes != CP0_CAMERA_FRAME_BYTES)
        return CP0_BROKER_INVALID_ARGUMENT;
    result = cp0_broker_capture_camera(&descriptor);
    if (result != CP0_BROKER_OK)
        return result;
    if (descriptor < 0)
        return CP0_BROKER_INTERNAL;
    flags = fcntl(descriptor, F_GETFL);
    seals = fcntl(descriptor, F_GET_SEALS);
    if (fstat(descriptor, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
        metadata.st_size < 0 ||
        (uint64_t)metadata.st_size != CP0_CAMERA_FRAME_BYTES || flags < 0 ||
        (flags & O_ACCMODE) != O_RDONLY || seals < 0 ||
        (seals & (F_SEAL_SEAL | F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_WRITE)) !=
            (F_SEAL_SEAL | F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_WRITE)) {
        close(descriptor);
        return CP0_BROKER_INTERNAL;
    }
    while (offset < pixel_bytes) {
        ssize_t count = pread(descriptor, pixels + offset, pixel_bytes - offset,
                              (off_t)offset);
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

int64_t cp0_camera_capture_photo(void) {
    return cp0_broker_capture_photo();
}
