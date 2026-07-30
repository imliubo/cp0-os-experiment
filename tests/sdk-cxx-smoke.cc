#include "cardputerzero.h"

static_assert(sizeof(cp0_key_event_t) == 8U, "key event ABI changed");

extern "C" int sdk_cxx_smoke() {
    cp0_key_event_t event{};
    return cp0_poll_key_event(&event, sizeof(event), 1);
}
