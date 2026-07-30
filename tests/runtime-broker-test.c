#include "broker_client.h"

#include <assert.h>
#include <stdint.h>
#include <string.h>

int main(void) {
    static const char success[] =
        "{\"protocol_version\":1,\"request_id\":2,\"outcome\":{"
        "\"status\":\"http-response\",\"status_code\":206,"
        "\"body_base64\":\"AAH+/3g=\"}}\n";
    static const char blocked[] =
        "{\"protocol_version\":1,\"request_id\":2,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"blocked-address\","
        "\"message\":\"blocked\"}}\n";
    uint8_t body[8] = {0};
    int64_t packed =
        cp0_broker_decode_http_response(success, body, sizeof(body));

    assert(packed >= 0);
    assert((uint16_t)((uint64_t)packed >> 32) == 206U);
    assert((uint32_t)packed == 5U);
    assert(memcmp(body, "\x00\x01\xfe\xffx", 5U) == 0);
    assert(cp0_broker_decode_http_response(success, body, 4U) ==
           CP0_BROKER_RESOURCE_LIMIT);
    assert(cp0_broker_decode_http_response(blocked, body, sizeof(body)) ==
           CP0_BROKER_DENIED);
    return 0;
}
