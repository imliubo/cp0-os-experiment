#include "frame_pacing.h"

#include <stddef.h>

bool cp0_frame_pacer_ready(const struct cp0_frame_pacer *pacer,
                           uint64_t now_ns) {
    if (pacer == NULL)
        return false;
    if (!pacer->has_committed)
        return true;
    if (now_ns < pacer->last_commit_ns)
        return false;
    return now_ns - pacer->last_commit_ns >= CP0_MIN_FRAME_INTERVAL_NS;
}

void cp0_frame_pacer_mark_committed(struct cp0_frame_pacer *pacer,
                                    uint64_t now_ns) {
    if (pacer == NULL)
        return;
    pacer->last_commit_ns = now_ns;
    pacer->has_committed = true;
}

void cp0_frame_pacer_reset(struct cp0_frame_pacer *pacer) {
    if (pacer == NULL)
        return;
    pacer->last_commit_ns = 0;
    pacer->has_committed = false;
}
