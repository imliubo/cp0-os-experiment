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
int64_t cp0_broker_http_get(const uint8_t *url, size_t url_length,
                            uint8_t *body, size_t body_capacity);
int64_t cp0_broker_decode_http_response(const char *response, uint8_t *body,
                                        size_t body_capacity);
int32_t cp0_broker_open_document(int *descriptor, uint32_t *size_bytes);
int32_t cp0_broker_decode_document_response(const char *response,
                                            int received_descriptor,
                                            int *descriptor,
                                            uint32_t *size_bytes);
int32_t cp0_broker_play_audio(const uint8_t *samples, size_t sample_bytes);
int32_t cp0_broker_capture_audio(uint8_t *samples, size_t sample_capacity);
int32_t cp0_broker_decode_audio_capture_response(const char *response,
                                                 uint8_t *samples,
                                                 size_t sample_capacity);

#endif
