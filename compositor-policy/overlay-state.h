#ifndef CP0_OVERLAY_STATE_H
#define CP0_OVERLAY_STATE_H

#include <stdbool.h>
#include <stdint.h>

enum cp0_overlay_state {
    CP0_OVERLAY_STATE_FULL = 0,
    CP0_OVERLAY_STATE_STATUS = 1,
    CP0_OVERLAY_STATE_HIDDEN = 2,
    CP0_OVERLAY_STATE_NOTIFICATION = 3,
};

enum cp0_overlay_state cp0_overlay_transient_base(
    bool transient_active, uint32_t current_mode, uint32_t restore_mode);
enum cp0_overlay_state cp0_overlay_transient_target(uint32_t base_mode);

#endif
