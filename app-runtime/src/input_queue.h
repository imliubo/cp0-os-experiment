#ifndef CARDPUTERZERO_INPUT_QUEUE_H
#define CARDPUTERZERO_INPUT_QUEUE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define CP0_INPUT_QUEUE_CAPACITY 32U

struct cp0_key_event {
    uint16_t code;
    uint8_t pressed;
    uint8_t repeated;
    uint8_t modifiers;
    uint8_t reserved[3];
};

struct cp0_input_queue {
    struct cp0_key_event events[CP0_INPUT_QUEUE_CAPACITY];
    size_t head;
    size_t length;
    bool overflowed;
};

void cp0_input_queue_reset(struct cp0_input_queue *queue);
bool cp0_input_queue_push(struct cp0_input_queue *queue, uint16_t code,
                          bool pressed, bool repeated, uint8_t modifiers);
bool cp0_input_queue_pop(struct cp0_input_queue *queue,
                         struct cp0_key_event *event);
bool cp0_input_queue_take_overflow(struct cp0_input_queue *queue);

#endif
