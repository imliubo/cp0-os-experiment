#include "overlay-state.h"

#include <assert.h>
#include <stdint.h>

int main(void)
{
    assert(cp0_overlay_transient_target(CP0_OVERLAY_STATE_FULL) ==
           CP0_OVERLAY_STATE_FULL);
    assert(cp0_overlay_transient_target(CP0_OVERLAY_STATE_STATUS) ==
           CP0_OVERLAY_STATE_NOTIFICATION);
    assert(cp0_overlay_transient_target(CP0_OVERLAY_STATE_HIDDEN) ==
           CP0_OVERLAY_STATE_NOTIFICATION);
    assert(cp0_overlay_transient_target(CP0_OVERLAY_STATE_NOTIFICATION) ==
           CP0_OVERLAY_STATE_NOTIFICATION);

    assert(cp0_overlay_transient_base(false, CP0_OVERLAY_STATE_FULL,
                                      CP0_OVERLAY_STATE_HIDDEN) ==
           CP0_OVERLAY_STATE_FULL);
    assert(cp0_overlay_transient_base(false, CP0_OVERLAY_STATE_STATUS,
                                      CP0_OVERLAY_STATE_FULL) ==
           CP0_OVERLAY_STATE_STATUS);
    assert(cp0_overlay_transient_base(true,
                                      CP0_OVERLAY_STATE_NOTIFICATION,
                                      CP0_OVERLAY_STATE_STATUS) ==
           CP0_OVERLAY_STATE_STATUS);
    assert(cp0_overlay_transient_base(true,
                                      CP0_OVERLAY_STATE_NOTIFICATION,
                                      CP0_OVERLAY_STATE_HIDDEN) ==
           CP0_OVERLAY_STATE_HIDDEN);

    assert(cp0_overlay_transient_target(UINT32_MAX) ==
           CP0_OVERLAY_STATE_FULL);
    assert(cp0_overlay_transient_base(false, UINT32_MAX,
                                      CP0_OVERLAY_STATE_STATUS) ==
           CP0_OVERLAY_STATE_FULL);
    return 0;
}
