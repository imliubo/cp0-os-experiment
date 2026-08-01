#include "esc-gesture.h"

#include <stddef.h>

void
cp0_esc_gesture_press(struct cp0_esc_gesture *gesture, uint64_t now_msec)
{
    if (gesture == NULL || gesture->active)
        return;
    gesture->active = true;
    gesture->pressed_msec = now_msec;
}

enum cp0_esc_gesture_action
cp0_esc_gesture_poll(struct cp0_esc_gesture *gesture, uint64_t now_msec,
                     bool key_held)
{
    if (gesture == NULL || !gesture->active)
        return CP0_ESC_GESTURE_NONE;
    if (!key_held) {
        gesture->active = false;
        return CP0_ESC_GESTURE_BACK;
    }
    if (now_msec >= gesture->pressed_msec &&
        now_msec - gesture->pressed_msec >= CP0_ESC_LONG_PRESS_MSEC) {
        gesture->active = false;
        return CP0_ESC_GESTURE_HOME;
    }
    return CP0_ESC_GESTURE_NONE;
}

void
cp0_esc_gesture_cancel(struct cp0_esc_gesture *gesture)
{
    if (gesture != NULL)
        gesture->active = false;
}
