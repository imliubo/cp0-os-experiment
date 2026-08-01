#ifndef CP0_ESC_GESTURE_H
#define CP0_ESC_GESTURE_H

#include <stdbool.h>
#include <stdint.h>

#define CP0_ESC_LONG_PRESS_MSEC 800U

enum cp0_esc_gesture_action {
    CP0_ESC_GESTURE_NONE,
    CP0_ESC_GESTURE_BACK,
    CP0_ESC_GESTURE_HOME,
};

struct cp0_esc_gesture {
    bool active;
    uint64_t pressed_msec;
};

void cp0_esc_gesture_press(struct cp0_esc_gesture *gesture,
                           uint64_t now_msec);
enum cp0_esc_gesture_action
cp0_esc_gesture_poll(struct cp0_esc_gesture *gesture, uint64_t now_msec,
                     bool key_held);
void cp0_esc_gesture_cancel(struct cp0_esc_gesture *gesture);

#endif
