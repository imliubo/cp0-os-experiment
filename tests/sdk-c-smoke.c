#include "cardputerzero.h"

int sdk_c_smoke(void) {
    static const uint8_t title[] = "C SDK";
    static const uint8_t body[] = "ready";
    cp0_result_t result = cp0_post_notification(
        title, (uint32_t)(sizeof(title) - 1U), body,
        (uint32_t)(sizeof(body) - 1U));
    return result == CP0_ERROR_UNAVAILABLE ? (int)cp0_monotonic_milliseconds()
                                           : (int)result;
}
