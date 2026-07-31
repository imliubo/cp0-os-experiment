#ifndef CARDPUTERZERO_FRAME_PACING_H
#define CARDPUTERZERO_FRAME_PACING_H

#include <stdbool.h>
#include <stdint.h>

#define CP0_MAX_FRAME_RATE 30U
#define CP0_MIN_FRAME_INTERVAL_NS 33333334ULL

struct cp0_frame_pacer {
    uint64_t last_commit_ns;
    bool has_committed;
};

bool cp0_frame_pacer_ready(const struct cp0_frame_pacer *pacer,
                           uint64_t now_ns);
void cp0_frame_pacer_mark_committed(struct cp0_frame_pacer *pacer,
                                    uint64_t now_ns);
void cp0_frame_pacer_reset(struct cp0_frame_pacer *pacer);

#endif
