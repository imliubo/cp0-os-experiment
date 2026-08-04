#include "wake-key.h"

#include <stddef.h>

void cp0_wake_key_arm(struct cp0_wake_key *state)
{
    if (state == NULL)
        return;
    *state = (struct cp0_wake_key){.armed = true};
}

void cp0_wake_key_cancel(struct cp0_wake_key *state)
{
    if (state != NULL)
        *state = (struct cp0_wake_key){0};
}

bool cp0_wake_key_is_armed(const struct cp0_wake_key *state)
{
    return state != NULL && state->armed;
}

enum cp0_wake_key_result cp0_wake_key_handle(struct cp0_wake_key *state,
                                              uint32_t key, bool pressed)
{
    if (!cp0_wake_key_is_armed(state))
        return CP0_WAKE_KEY_FORWARD;
    if (!state->captured && pressed) {
        state->captured = true;
        state->key = key;
        return CP0_WAKE_KEY_CONSUME;
    }
    if (state->captured && !pressed && state->key == key) {
        cp0_wake_key_cancel(state);
        return CP0_WAKE_KEY_CONSUME_AND_FINISH;
    }
    return CP0_WAKE_KEY_CONSUME;
}
