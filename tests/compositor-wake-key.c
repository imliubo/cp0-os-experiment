#include "wake-key.h"

#include <assert.h>

int main(void)
{
    struct cp0_wake_key state = {0};

    assert(cp0_wake_key_handle(&state, 1, true) == CP0_WAKE_KEY_FORWARD);
    cp0_wake_key_arm(&state);
    assert(cp0_wake_key_is_armed(&state));
    assert(cp0_wake_key_handle(&state, 1, false) == CP0_WAKE_KEY_CONSUME);
    assert(cp0_wake_key_handle(&state, 1, true) == CP0_WAKE_KEY_CONSUME);
    assert(cp0_wake_key_handle(&state, 2, true) == CP0_WAKE_KEY_CONSUME);
    assert(cp0_wake_key_handle(&state, 2, false) == CP0_WAKE_KEY_CONSUME);
    assert(cp0_wake_key_handle(&state, 1, false) ==
           CP0_WAKE_KEY_CONSUME_AND_FINISH);
    assert(!cp0_wake_key_is_armed(&state));
    assert(cp0_wake_key_handle(&state, 1, true) == CP0_WAKE_KEY_FORWARD);

    cp0_wake_key_arm(&state);
    assert(cp0_wake_key_handle(&state, 0, true) == CP0_WAKE_KEY_CONSUME);
    cp0_wake_key_cancel(&state);
    assert(!cp0_wake_key_is_armed(&state));
    return 0;
}
