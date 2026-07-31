#include "frame_pacing.h"

#include <assert.h>
#include <stddef.h>
#include <stdint.h>

int main(void) {
    struct cp0_frame_pacer pacer = {0};
    const uint64_t first = 9ULL * 1000000000ULL;

    assert(CP0_MAX_FRAME_RATE == 30U);
    assert(CP0_MIN_FRAME_INTERVAL_NS == 33333334ULL);
    assert(cp0_frame_pacer_ready(&pacer, first));
    cp0_frame_pacer_mark_committed(&pacer, first);
    assert(!cp0_frame_pacer_ready(&pacer, first));
    assert(!cp0_frame_pacer_ready(
        &pacer, first + CP0_MIN_FRAME_INTERVAL_NS - 1U));
    assert(cp0_frame_pacer_ready(
        &pacer, first + CP0_MIN_FRAME_INTERVAL_NS));
    assert(!cp0_frame_pacer_ready(&pacer, first - 1U));
    cp0_frame_pacer_reset(&pacer);
    assert(cp0_frame_pacer_ready(&pacer, 0));
    assert(!cp0_frame_pacer_ready(NULL, first));
    return 0;
}
