#ifndef CARDPUTERZERO_KEYBOARD_STATE_H
#define CARDPUTERZERO_KEYBOARD_STATE_H

#include <stdbool.h>
#include <stdint.h>

enum cp0_modifier {
    CP0_MODIFIER_SHIFT = 1U << 0,
    CP0_MODIFIER_CONTROL = 1U << 1,
    CP0_MODIFIER_ALT = 1U << 2,
    CP0_MODIFIER_SUPER = 1U << 3,
};

struct cp0_keyboard_state {
    uint32_t depressed_modifiers;
    uint8_t held_modifiers;
};

void cp0_keyboard_state_reset(struct cp0_keyboard_state *state);
void cp0_keyboard_state_set_key(struct cp0_keyboard_state *state,
                                uint32_t key, bool pressed);
void cp0_keyboard_state_set_depressed(struct cp0_keyboard_state *state,
                                      uint32_t depressed);
uint8_t cp0_keyboard_state_modifiers(const struct cp0_keyboard_state *state);
bool cp0_keyboard_state_is_modifier_key(uint32_t key);

#endif
