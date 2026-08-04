#ifndef CP0_WAKE_KEY_H
#define CP0_WAKE_KEY_H

#include <stdbool.h>
#include <stdint.h>

enum cp0_wake_key_result {
    CP0_WAKE_KEY_FORWARD,
    CP0_WAKE_KEY_CONSUME,
    CP0_WAKE_KEY_CONSUME_AND_FINISH,
};

struct cp0_wake_key {
    bool armed;
    bool captured;
    uint32_t key;
};

void cp0_wake_key_arm(struct cp0_wake_key *state);
void cp0_wake_key_cancel(struct cp0_wake_key *state);
bool cp0_wake_key_is_armed(const struct cp0_wake_key *state);
enum cp0_wake_key_result cp0_wake_key_handle(struct cp0_wake_key *state,
                                              uint32_t key, bool pressed);

#endif
