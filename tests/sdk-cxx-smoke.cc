#include "cardputerzero.h"

extern "C" int sdk_cxx_smoke() {
    return static_cast<int>(cp0_wait_event(1));
}
