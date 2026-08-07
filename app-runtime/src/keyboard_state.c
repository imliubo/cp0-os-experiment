#include "keyboard_state.h"

#include <string.h>

/* XKB core modifier indices are fixed; Shift is bit zero on the pinned keymap. */
#define CP0_XKB_CORE_SHIFT_MASK (1U << 0)

#define CP0_KEY_LEFT_CONTROL 29U
#define CP0_KEY_LEFT_SHIFT 42U
#define CP0_KEY_LEFT_ALT 56U
#define CP0_KEY_RIGHT_SHIFT 54U
#define CP0_KEY_RIGHT_CONTROL 97U
#define CP0_KEY_RIGHT_ALT 100U
#define CP0_KEY_LEFT_SUPER 125U
#define CP0_KEY_RIGHT_SUPER 126U

enum cp0_held_modifier {
    CP0_HELD_LEFT_SHIFT = 1U << 0,
    CP0_HELD_RIGHT_SHIFT = 1U << 1,
    CP0_HELD_LEFT_CONTROL = 1U << 2,
    CP0_HELD_RIGHT_CONTROL = 1U << 3,
    CP0_HELD_LEFT_ALT = 1U << 4,
    CP0_HELD_RIGHT_ALT = 1U << 5,
    CP0_HELD_LEFT_SUPER = 1U << 6,
    CP0_HELD_RIGHT_SUPER = 1U << 7,
};

void cp0_keyboard_state_reset(struct cp0_keyboard_state *state) {
    memset(state, 0, sizeof(*state));
}

void cp0_keyboard_state_set_key(struct cp0_keyboard_state *state,
                                uint32_t key, bool pressed) {
    uint8_t bit = 0;

    switch (key) {
    case CP0_KEY_LEFT_SHIFT: bit = CP0_HELD_LEFT_SHIFT; break;
    case CP0_KEY_RIGHT_SHIFT: bit = CP0_HELD_RIGHT_SHIFT; break;
    case CP0_KEY_LEFT_CONTROL: bit = CP0_HELD_LEFT_CONTROL; break;
    case CP0_KEY_RIGHT_CONTROL: bit = CP0_HELD_RIGHT_CONTROL; break;
    case CP0_KEY_LEFT_ALT: bit = CP0_HELD_LEFT_ALT; break;
    case CP0_KEY_RIGHT_ALT: bit = CP0_HELD_RIGHT_ALT; break;
    case CP0_KEY_LEFT_SUPER: bit = CP0_HELD_LEFT_SUPER; break;
    case CP0_KEY_RIGHT_SUPER: bit = CP0_HELD_RIGHT_SUPER; break;
    default: return;
    }
    if (pressed)
        state->held_modifiers |= bit;
    else
        state->held_modifiers &= (uint8_t)~bit;
}

void cp0_keyboard_state_set_depressed(struct cp0_keyboard_state *state,
                                      uint32_t depressed) {
    state->depressed_modifiers = depressed;
}

uint8_t cp0_keyboard_state_modifiers(const struct cp0_keyboard_state *state) {
    uint8_t modifiers = 0;

    if ((state->depressed_modifiers & CP0_XKB_CORE_SHIFT_MASK) != 0U ||
        (state->held_modifiers &
         (CP0_HELD_LEFT_SHIFT | CP0_HELD_RIGHT_SHIFT)) != 0U)
        modifiers |= CP0_MODIFIER_SHIFT;
    if ((state->held_modifiers &
         (CP0_HELD_LEFT_CONTROL | CP0_HELD_RIGHT_CONTROL)) != 0U)
        modifiers |= CP0_MODIFIER_CONTROL;
    if ((state->held_modifiers &
         (CP0_HELD_LEFT_ALT | CP0_HELD_RIGHT_ALT)) != 0U)
        modifiers |= CP0_MODIFIER_ALT;
    if ((state->held_modifiers &
         (CP0_HELD_LEFT_SUPER | CP0_HELD_RIGHT_SUPER)) != 0U)
        modifiers |= CP0_MODIFIER_SUPER;
    return modifiers;
}

bool cp0_keyboard_state_is_modifier_key(uint32_t key) {
    return key == CP0_KEY_LEFT_SHIFT || key == CP0_KEY_RIGHT_SHIFT ||
           key == CP0_KEY_LEFT_CONTROL || key == CP0_KEY_RIGHT_CONTROL ||
           key == CP0_KEY_LEFT_ALT || key == CP0_KEY_RIGHT_ALT ||
           key == CP0_KEY_LEFT_SUPER || key == CP0_KEY_RIGHT_SUPER;
}
