#include "input_ascii.h"

uint8_t cp0_input_ascii_character(uint32_t key, bool shifted) {
    uint8_t character = 0;

    /*
     * The V0.6 Sym layer uses otherwise unreachable evdev codes as stable
     * character identifiers. Keep this table ahead of the US key map because
     * several identifiers intentionally reuse punctuation key codes.
     */
    switch (key) {
    case 26: return (uint8_t)'!';
    case 27: return (uint8_t)'@';
    case 39: return (uint8_t)'#';
    case 40: return (uint8_t)'$';
    case 41: return (uint8_t)'%';
    case 43: return (uint8_t)'^';
    case 51: return (uint8_t)'&';
    case 52: return (uint8_t)'*';
    case 53: return (uint8_t)'(';
    case 94: return (uint8_t)')';
    case 55: return (uint8_t)'~';
    case 69: return (uint8_t)'`';
    case 70: return (uint8_t)'_';
    case 71: return (uint8_t)'-';
    case 72: return (uint8_t)'+';
    case 73: return (uint8_t)'=';
    case 74: return (uint8_t)'[';
    case 75: return (uint8_t)']';
    case 76: return (uint8_t)'{';
    case 77: return (uint8_t)'}';
    case 79: return (uint8_t)';';
    case 80: return (uint8_t)':';
    case 81: return (uint8_t)'\'';
    case 82: return (uint8_t)'"';
    case 83: return (uint8_t)'<';
    case 85: return (uint8_t)'>';
    case 86: return (uint8_t)'\\';
    case 89: return (uint8_t)'|';
    case 90: return (uint8_t)',';
    case 91: return (uint8_t)'.';
    case 92: return (uint8_t)'/';
    case 93: return (uint8_t)'?';
    default: break;
    }

    /* Standard Linux evdev keycodes using the V0.6 US printable layout. */
    switch (key) {
    case 30: character = 'a'; break;
    case 48: character = 'b'; break;
    case 46: character = 'c'; break;
    case 32: character = 'd'; break;
    case 18: character = 'e'; break;
    case 33: character = 'f'; break;
    case 34: character = 'g'; break;
    case 35: character = 'h'; break;
    case 23: character = 'i'; break;
    case 36: character = 'j'; break;
    case 37: character = 'k'; break;
    case 38: character = 'l'; break;
    case 50: character = 'm'; break;
    case 49: character = 'n'; break;
    case 24: character = 'o'; break;
    case 25: character = 'p'; break;
    case 16: character = 'q'; break;
    case 19: character = 'r'; break;
    case 31: character = 's'; break;
    case 20: character = 't'; break;
    case 22: character = 'u'; break;
    case 47: character = 'v'; break;
    case 17: character = 'w'; break;
    case 45: character = 'x'; break;
    case 21: character = 'y'; break;
    case 44: character = 'z'; break;
    default: break;
    }
    if (character != 0U)
        return shifted ? (uint8_t)(character - 'a' + 'A') : character;
    if (key >= 2U && key <= 10U) {
        static const uint8_t shifted_digits[] = "!@#$%^&*(";
        return shifted ? shifted_digits[key - 2U]
                       : (uint8_t)('1' + (key - 2U));
    }
    if (key == 11U)
        return shifted ? (uint8_t)')' : (uint8_t)'0';
    switch (key) {
    case 12: return shifted ? (uint8_t)'_' : (uint8_t)'-';
    case 13: return shifted ? (uint8_t)'+' : (uint8_t)'=';
    case 57: return (uint8_t)' ';
    default: return 0;
    }
}
