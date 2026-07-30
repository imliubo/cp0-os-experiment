#include "input_queue.h"

#include <string.h>

_Static_assert(sizeof(struct cp0_key_event) == 8U,
               "key event ABI must remain eight bytes");

void cp0_input_queue_reset(struct cp0_input_queue *queue) {
    memset(queue, 0, sizeof(*queue));
}

bool cp0_input_queue_push(struct cp0_input_queue *queue, uint16_t code,
                          bool pressed, bool repeated, uint8_t modifiers) {
    size_t tail;
    struct cp0_key_event *event;

    if (queue->length == CP0_INPUT_QUEUE_CAPACITY) {
        queue->overflowed = true;
        return false;
    }
    tail = (queue->head + queue->length) % CP0_INPUT_QUEUE_CAPACITY;
    event = &queue->events[tail];
    memset(event, 0, sizeof(*event));
    event->code = code;
    event->pressed = pressed ? 1U : 0U;
    event->repeated = repeated ? 1U : 0U;
    event->modifiers = modifiers;
    queue->length++;
    return true;
}

bool cp0_input_queue_pop(struct cp0_input_queue *queue,
                         struct cp0_key_event *event) {
    if (queue->length == 0U)
        return false;
    *event = queue->events[queue->head];
    queue->head = (queue->head + 1U) % CP0_INPUT_QUEUE_CAPACITY;
    queue->length--;
    return true;
}

bool cp0_input_queue_take_overflow(struct cp0_input_queue *queue) {
    bool overflowed = queue->overflowed;
    queue->overflowed = false;
    return overflowed;
}
