#include "overlay-state.h"

static enum cp0_overlay_state normalize_mode(uint32_t mode)
{
    if (mode <= CP0_OVERLAY_STATE_NOTIFICATION)
        return (enum cp0_overlay_state)mode;
    return CP0_OVERLAY_STATE_FULL;
}

enum cp0_overlay_state cp0_overlay_transient_base(
    bool transient_active, uint32_t current_mode, uint32_t restore_mode)
{
    return normalize_mode(transient_active ? restore_mode : current_mode);
}

enum cp0_overlay_state cp0_overlay_transient_target(uint32_t base_mode)
{
    return normalize_mode(base_mode) == CP0_OVERLAY_STATE_FULL
               ? CP0_OVERLAY_STATE_FULL
               : CP0_OVERLAY_STATE_NOTIFICATION;
}
