#include "esc-gesture.h"

#include <assert.h>
#include <stdint.h>

int
main(void)
{
    struct cp0_esc_gesture gesture = {0};

    cp0_esc_gesture_press(&gesture, 100);
    assert(cp0_esc_gesture_poll(&gesture, 899, true) ==
           CP0_ESC_GESTURE_NONE);
    assert(cp0_esc_gesture_poll(&gesture, 899, false) ==
           CP0_ESC_GESTURE_BACK);
    assert(cp0_esc_gesture_poll(&gesture, 900, false) ==
           CP0_ESC_GESTURE_NONE);

    cp0_esc_gesture_press(&gesture, 1000);
    assert(cp0_esc_gesture_poll(&gesture, 1799, true) ==
           CP0_ESC_GESTURE_NONE);
    assert(cp0_esc_gesture_poll(&gesture, 1800, true) ==
           CP0_ESC_GESTURE_HOME);
    assert(cp0_esc_gesture_poll(&gesture, 1801, false) ==
           CP0_ESC_GESTURE_NONE);

    cp0_esc_gesture_press(&gesture, 2000);
    cp0_esc_gesture_press(&gesture, 2700);
    assert(cp0_esc_gesture_poll(&gesture, 2800, true) ==
           CP0_ESC_GESTURE_HOME);

    cp0_esc_gesture_press(&gesture, 3000);
    assert(cp0_esc_gesture_poll(&gesture, 2999, true) ==
           CP0_ESC_GESTURE_NONE);
    assert(cp0_esc_gesture_poll(&gesture, 2999, false) ==
           CP0_ESC_GESTURE_BACK);

    cp0_esc_gesture_press(&gesture, UINT64_MAX - 1U);
    cp0_esc_gesture_cancel(&gesture);
    assert(cp0_esc_gesture_poll(&gesture, UINT64_MAX, true) ==
           CP0_ESC_GESTURE_NONE);
    return 0;
}
