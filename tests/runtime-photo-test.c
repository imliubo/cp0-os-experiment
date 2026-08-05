#include "broker_client.h"
#include "photos.h"

#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>

#define FRAME_BYTES (320U * 170U * 2U)

static int fixture_descriptor = -1;
static int32_t fixture_result = CP0_BROKER_OK;

int32_t cp0_broker_photo_load_rgb565(uint64_t photo_id, int *descriptor) {
    if (fixture_result != CP0_BROKER_OK)
        return fixture_result;
    if (photo_id != 42U || descriptor == NULL)
        return CP0_BROKER_INVALID_ARGUMENT;
    *descriptor = dup(fixture_descriptor);
    return *descriptor >= 0 ? CP0_BROKER_OK : CP0_BROKER_UNAVAILABLE;
}

static void write_all(int descriptor, const uint8_t *bytes, size_t length) {
    size_t offset = 0;
    while (offset < length) {
        ssize_t count = write(descriptor, bytes + offset, length - offset);
        assert(count > 0);
        offset += (size_t)count;
    }
}

int main(void) {
    static const char path[] = "target/test-tmp/runtime-photo-frame.rgb565";
    static uint8_t source[FRAME_BYTES];
    static uint8_t output[FRAME_BYTES];
    int writable;

    for (size_t index = 0; index < sizeof(source); index++)
        source[index] = (uint8_t)(index & 0xffU);
    writable = open(path, O_CREAT | O_TRUNC | O_WRONLY, 0600);
    assert(writable >= 0);
    write_all(writable, source, sizeof(source));
    assert(close(writable) == 0);
    fixture_descriptor = open(path, O_RDONLY);
    assert(fixture_descriptor >= 0);
    assert(cp0_photos_load_rgb565(42, output, sizeof(output)) == CP0_BROKER_OK);
    assert(memcmp(source, output, sizeof(source)) == 0);
    assert(cp0_photos_load_rgb565(0, output, sizeof(output)) ==
           CP0_BROKER_INVALID_ARGUMENT);
    assert(cp0_photos_load_rgb565(42, output, sizeof(output) - 1U) ==
           CP0_BROKER_INVALID_ARGUMENT);
    assert(close(fixture_descriptor) == 0);

    writable = open(path, O_TRUNC | O_WRONLY);
    assert(writable >= 0);
    write_all(writable, source, sizeof(source) - 1U);
    assert(close(writable) == 0);
    fixture_descriptor = open(path, O_RDONLY);
    assert(fixture_descriptor >= 0);
    assert(cp0_photos_load_rgb565(42, output, sizeof(output)) ==
           CP0_BROKER_INTERNAL);
    assert(close(fixture_descriptor) == 0);
    assert(unlink(path) == 0);
    return 0;
}
