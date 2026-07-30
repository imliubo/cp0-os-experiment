#define _POSIX_C_SOURCE 200809L

#include "broker_client.h"
#include "document.h"

#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int fixture_descriptor = -1;

int32_t cp0_broker_open_document(int *descriptor, uint32_t *size_bytes) {
    int duplicated = dup(fixture_descriptor);
    if (duplicated < 0)
        return CP0_BROKER_UNAVAILABLE;
    *descriptor = duplicated;
    *size_bytes = 7U;
    return CP0_BROKER_OK;
}

int main(void) {
    char path[] = "/tmp/cp0-runtime-document.XXXXXX";
    uint8_t buffer[8] = {0};
    int writable = mkstemp(path);
    assert(writable >= 0);
    assert(write(writable, "content", 7) == 7);
    assert(close(writable) == 0);
    fixture_descriptor = open(path, O_RDONLY | O_CLOEXEC);
    assert(fixture_descriptor >= 0);
    assert(unlink(path) == 0);

    int64_t packed = cp0_document_open();
    assert(packed > 0);
    int32_t handle = (int32_t)((uint64_t)packed >> 32);
    assert((uint32_t)packed == 7U);
    assert(cp0_document_read(handle, 0, buffer, 7) == 7);
    assert(memcmp(buffer, "content", 7) == 0);
    assert(cp0_document_read(handle, 4, buffer, sizeof(buffer)) == 3);
    assert(memcmp(buffer, "ent", 3) == 0);
    assert(cp0_document_read(handle, 7, buffer, 1) == 0);
    assert(cp0_document_read(handle, 0, buffer, 0) ==
           CP0_BROKER_INVALID_ARGUMENT);
    assert(cp0_document_read(handle + 1, 0, buffer, 1) ==
           CP0_BROKER_UNAVAILABLE);
    assert(cp0_document_close(handle) == CP0_BROKER_OK);
    assert(cp0_document_read(handle, 0, buffer, 1) ==
           CP0_BROKER_UNAVAILABLE);
    cp0_document_destroy();
    close(fixture_descriptor);
    return 0;
}
