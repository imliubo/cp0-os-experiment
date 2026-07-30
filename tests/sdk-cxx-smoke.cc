#include "cardputerzero.h"

static_assert(sizeof(cp0_key_event_t) == 8U, "key event ABI changed");

extern "C" int sdk_cxx_smoke() {
    cp0_key_event_t event{};
    cp0_http_response_t response{};
    uint8_t body[32]{};
    static const uint8_t url[] = "https://example.com";
    (void)cp0_http_get(url, sizeof(url) - 1U, body, sizeof(body), &response);
    return cp0_poll_key_event(&event, sizeof(event), 1);
}
