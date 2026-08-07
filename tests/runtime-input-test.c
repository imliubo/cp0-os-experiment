#include "input_queue.h"
#include "input_ascii.h"
#include "keyboard_state.h"

#include <assert.h>
#include <stdbool.h>
#include <stddef.h>

int main(void) {
    struct cp0_input_queue queue;
    struct cp0_key_event event;
    struct cp0_keyboard_state keyboard;
    size_t index;

    static const struct {
        uint32_t code;
        uint8_t character;
    } symbol_keys[] = {
        {26, '!'}, {27, '@'}, {39, '#'}, {40, '$'}, {41, '%'},
        {43, '^'}, {51, '&'}, {52, '*'}, {53, '('}, {94, ')'},
        {55, '~'}, {69, '`'}, {70, '_'}, {71, '-'}, {72, '+'},
        {73, '='}, {74, '['}, {75, ']'}, {76, '{'}, {77, '}'},
        {79, ';'}, {80, ':'}, {81, '\''}, {82, '"'}, {83, '<'},
        {85, '>'}, {86, '\\'}, {89, '|'}, {90, ','}, {91, '.'},
        {92, '/'}, {93, '?'},
    };
    static const struct {
        uint32_t code;
        uint8_t lowercase;
    } letter_keys[] = {
        {30, 'a'}, {48, 'b'}, {46, 'c'}, {32, 'd'}, {18, 'e'},
        {33, 'f'}, {34, 'g'}, {35, 'h'}, {23, 'i'}, {36, 'j'},
        {37, 'k'}, {38, 'l'}, {50, 'm'}, {49, 'n'}, {24, 'o'},
        {25, 'p'}, {16, 'q'}, {19, 'r'}, {31, 's'}, {20, 't'},
        {22, 'u'}, {47, 'v'}, {17, 'w'}, {45, 'x'}, {21, 'y'},
        {44, 'z'},
    };

    for (index = 0; index < sizeof(symbol_keys) / sizeof(symbol_keys[0]);
         index++) {
        assert(cp0_input_ascii_character(symbol_keys[index].code, false) ==
               symbol_keys[index].character);
        assert(cp0_input_ascii_character(symbol_keys[index].code, true) ==
               symbol_keys[index].character);
    }
    for (index = 0; index < sizeof(letter_keys) / sizeof(letter_keys[0]);
         index++) {
        assert(cp0_input_ascii_character(letter_keys[index].code, false) ==
               letter_keys[index].lowercase);
        assert(cp0_input_ascii_character(letter_keys[index].code, true) ==
               (uint8_t)(letter_keys[index].lowercase - 'a' + 'A'));
    }
    assert(cp0_input_ascii_character(57, false) == ' ');
    assert(cp0_input_ascii_character(2, false) == '1');
    assert(cp0_input_ascii_character(2, true) == '!');
    assert(cp0_input_ascii_character(13, false) == '=');
    assert(cp0_input_ascii_character(13, true) == '+');
    assert(cp0_input_ascii_character(1, false) == 0U);

    cp0_keyboard_state_reset(&keyboard);
    assert(cp0_keyboard_state_modifiers(&keyboard) == 0U);
    cp0_keyboard_state_set_depressed(&keyboard, 1U);
    assert(cp0_keyboard_state_modifiers(&keyboard) == CP0_MODIFIER_SHIFT);
    assert(cp0_input_ascii_character(
               30, (cp0_keyboard_state_modifiers(&keyboard) &
                    CP0_MODIFIER_SHIFT) != 0U) == 'A');
    cp0_keyboard_state_set_depressed(&keyboard, 2U);
    assert(cp0_keyboard_state_modifiers(&keyboard) == 0U);
    cp0_keyboard_state_set_key(&keyboard, 42U, true);
    assert(cp0_keyboard_state_modifiers(&keyboard) == CP0_MODIFIER_SHIFT);
    cp0_keyboard_state_set_key(&keyboard, 42U, false);
    assert(cp0_keyboard_state_modifiers(&keyboard) == 0U);
    assert(cp0_keyboard_state_is_modifier_key(42U));
    assert(!cp0_keyboard_state_is_modifier_key(30U));

    cp0_input_queue_reset(&queue);
    assert(!cp0_input_queue_pop(&queue, &event));
    assert(!cp0_input_queue_take_overflow(&queue));

    assert(cp0_input_queue_push(&queue, 30, true, false, 1, 'A'));
    assert(cp0_input_queue_push(&queue, 30, false, false, 0, 'a'));
    assert(cp0_input_queue_pop(&queue, &event));
    assert(event.code == 30);
    assert(event.pressed == 1);
    assert(event.repeated == 0);
    assert(event.modifiers == 1);
    assert(event.character == 'A');
    assert(event.reserved[0] == 0U && event.reserved[1] == 0U);
    assert(cp0_input_queue_pop(&queue, &event));
    assert(event.code == 30 && event.pressed == 0);
    assert(event.character == 0U);

    cp0_input_queue_reset(&queue);
    assert(cp0_input_queue_push(&queue, 28, true, false, 0, '\n'));
    assert(cp0_input_queue_pop(&queue, &event));
    assert(event.character == 0U);
    assert(cp0_input_queue_push(&queue, 30, false, true, 0, 'a'));
    assert(cp0_input_queue_pop(&queue, &event));
    assert(event.repeated == 1U && event.character == 'a');

    cp0_input_queue_reset(&queue);
    for (index = 0; index < CP0_INPUT_QUEUE_CAPACITY; index++)
        assert(cp0_input_queue_push(&queue, (uint16_t)index, true, false, 0,
                                    0));
    assert(!cp0_input_queue_push(&queue, 99, true, false, 0, 0));
    assert(cp0_input_queue_take_overflow(&queue));
    assert(!cp0_input_queue_take_overflow(&queue));

    cp0_input_queue_reset(&queue);
    assert(!cp0_input_queue_pop(&queue, &event));
    return 0;
}
