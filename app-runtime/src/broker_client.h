#ifndef CARDPUTERZERO_BROKER_CLIENT_H
#define CARDPUTERZERO_BROKER_CLIENT_H

#include <stddef.h>
#include <stdint.h>

enum cp0_broker_result {
    CP0_BROKER_OK = 0,
    CP0_BROKER_DENIED = -1,
    CP0_BROKER_UNAVAILABLE = -2,
    CP0_BROKER_INVALID_ARGUMENT = -3,
    CP0_BROKER_RESOURCE_LIMIT = -4,
    CP0_BROKER_INTERNAL = -5,
};

int32_t cp0_broker_post_notification(const uint8_t *title, size_t title_length,
                                     const uint8_t *body, size_t body_length);

#endif
