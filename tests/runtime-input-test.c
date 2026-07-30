#include "input_queue.h"

#include <assert.h>
#include <stdbool.h>
#include <stddef.h>

int main(void) {
    struct cp0_input_queue queue;
    struct cp0_key_event event;
    size_t index;

    cp0_input_queue_reset(&queue);
    assert(!cp0_input_queue_pop(&queue, &event));
    assert(!cp0_input_queue_take_overflow(&queue));

    assert(cp0_input_queue_push(&queue, 30, true, false, 1));
    assert(cp0_input_queue_push(&queue, 30, false, false, 0));
    assert(cp0_input_queue_pop(&queue, &event));
    assert(event.code == 30);
    assert(event.pressed == 1);
    assert(event.repeated == 0);
    assert(event.modifiers == 1);
    assert(cp0_input_queue_pop(&queue, &event));
    assert(event.code == 30 && event.pressed == 0);

    cp0_input_queue_reset(&queue);
    for (index = 0; index < CP0_INPUT_QUEUE_CAPACITY; index++)
        assert(cp0_input_queue_push(&queue, (uint16_t)index, true, false, 0));
    assert(!cp0_input_queue_push(&queue, 99, true, false, 0));
    assert(cp0_input_queue_take_overflow(&queue));
    assert(!cp0_input_queue_take_overflow(&queue));

    cp0_input_queue_reset(&queue);
    assert(!cp0_input_queue_pop(&queue, &event));
    return 0;
}
