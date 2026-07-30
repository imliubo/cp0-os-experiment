#define _POSIX_C_SOURCE 200809L

#include "document.h"
#include "broker_client.h"

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdbool.h>
#include <stdint.h>
#include <sys/stat.h>
#include <unistd.h>

#define CP0_DOCUMENT_MAX_BYTES (16U * 1024U * 1024U)
#define CP0_DOCUMENT_READ_BYTES 4096U

static int active_descriptor = -1;
static int32_t active_handle;
static uint32_t active_size;

int64_t cp0_document_open(void) {
    struct stat metadata;
    uint32_t size_bytes;
    int descriptor;
    int32_t result = cp0_broker_open_document(&descriptor, &size_bytes);

    if (result != CP0_BROKER_OK)
        return result;
    if (descriptor < 0 || size_bytes > CP0_DOCUMENT_MAX_BYTES ||
        fstat(descriptor, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
        metadata.st_size < 0 || (uint64_t)metadata.st_size != size_bytes ||
        (fcntl(descriptor, F_GETFL) & O_ACCMODE) != O_RDONLY) {
        close(descriptor);
        return CP0_BROKER_INTERNAL;
    }
    if (active_descriptor >= 0)
        close(active_descriptor);
    active_descriptor = descriptor;
    active_size = size_bytes;
    if (active_handle == INT32_MAX)
        active_handle = 1;
    else
        active_handle++;
    return ((int64_t)(uint32_t)active_handle << 32) | (int64_t)active_size;
}

int64_t cp0_document_read(int32_t handle, uint64_t offset, uint8_t *buffer,
                          size_t capacity) {
    size_t bounded_capacity;
    ssize_t count;

    if (handle <= 0 || handle != active_handle || active_descriptor < 0)
        return CP0_BROKER_UNAVAILABLE;
    if (buffer == NULL || capacity == 0U ||
        capacity > CP0_DOCUMENT_READ_BYTES)
        return CP0_BROKER_INVALID_ARGUMENT;
    if (offset >= active_size)
        return 0;
    bounded_capacity = capacity;
    if (bounded_capacity > (size_t)((uint64_t)active_size - offset))
        bounded_capacity = (size_t)((uint64_t)active_size - offset);
    do {
        count = pread(active_descriptor, buffer, bounded_capacity,
                      (off_t)offset);
    } while (count < 0 && errno == EINTR);
    if (count < 0)
        return CP0_BROKER_UNAVAILABLE;
    return count;
}

int32_t cp0_document_close(int32_t handle) {
    if (handle <= 0 || handle != active_handle || active_descriptor < 0)
        return CP0_BROKER_UNAVAILABLE;
    if (close(active_descriptor) != 0) {
        active_descriptor = -1;
        active_size = 0;
        return CP0_BROKER_UNAVAILABLE;
    }
    active_descriptor = -1;
    active_size = 0;
    return CP0_BROKER_OK;
}

void cp0_document_destroy(void) {
    if (active_descriptor >= 0)
        close(active_descriptor);
    active_descriptor = -1;
    active_size = 0;
}
