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
int64_t cp0_broker_http_get_range(const uint8_t *url, size_t url_length,
                                  uint64_t offset, uint8_t *body,
                                  size_t body_capacity);
int64_t cp0_broker_decode_http_response(const char *response, uint8_t *body,
                                        size_t body_capacity);
int32_t cp0_broker_open_document(int *descriptor, uint32_t *size_bytes);
int32_t cp0_broker_decode_document_response(const char *response,
                                            int received_descriptor,
                                            int *descriptor,
                                            uint32_t *size_bytes);
int32_t cp0_broker_play_audio(const uint8_t *samples, size_t sample_bytes);
int32_t cp0_broker_play_audio_stereo_48k(const uint8_t *samples,
                                         size_t sample_bytes);
int32_t cp0_broker_play_key_click(void);
int32_t cp0_broker_capture_audio(uint8_t *samples, size_t sample_capacity);
int32_t cp0_broker_decode_audio_capture_response(const char *response,
                                                 uint8_t *samples,
                                                 size_t sample_capacity);
int32_t cp0_broker_capture_camera(int *descriptor);
int64_t cp0_broker_capture_photo(void);
int32_t cp0_broker_decode_camera_response(const char *response,
                                          int received_descriptor,
                                          int *descriptor);
int32_t cp0_broker_gpio_read(uint32_t line);
int32_t cp0_broker_gpio_write(uint32_t line, uint32_t value);
int32_t cp0_broker_decode_gpio_response(const char *response, uint32_t line,
                                        int written, uint32_t expected_value);
int32_t cp0_broker_lora_send(const uint8_t *payload, size_t payload_length);
int32_t cp0_broker_lora_receive(uint8_t *payload, size_t payload_capacity,
                                uint8_t *metadata, size_t metadata_bytes,
                                uint32_t timeout_ms);
int32_t cp0_broker_decode_lora_response(const char *response,
                                        uint8_t *payload,
                                        size_t payload_capacity,
                                        uint8_t *metadata,
                                        size_t metadata_bytes);
int32_t cp0_broker_storage_put(const uint8_t *key, size_t key_length,
                               const uint8_t *value, size_t value_length);
int32_t cp0_broker_storage_get(const uint8_t *key, size_t key_length,
                               uint8_t *value, size_t value_capacity);
int32_t cp0_broker_decode_storage_get_response(const char *response,
                                               uint8_t *value,
                                               size_t value_capacity);
int32_t cp0_broker_storage_delete(const uint8_t *key, size_t key_length);
int32_t cp0_broker_photo_put(const uint8_t *key, size_t key_length,
                             const uint8_t *value, size_t value_length);
int32_t cp0_broker_photo_get(const uint8_t *key, size_t key_length,
                             uint8_t *value, size_t value_capacity);
int32_t cp0_broker_photo_index_get(uint8_t *value, size_t value_capacity);
int32_t cp0_broker_photo_delete(const uint8_t *key, size_t key_length);
int64_t cp0_broker_photo_import_rgb565(const uint8_t *pixels,
                                       size_t pixel_bytes,
                                       uint64_t suggested_id);
int64_t cp0_broker_decode_photo_import_response(const char *response);
int32_t cp0_broker_photo_load_rgb565(uint64_t photo_id, int *descriptor);
int32_t cp0_broker_photo_load_view_rgb565(uint64_t photo_id,
                                          uint32_t zoom_level, int32_t pan_x,
                                          int32_t pan_y, int *descriptor);
int32_t cp0_broker_decode_photo_load_response(const char *response,
                                              int received_descriptor,
                                              uint64_t photo_id,
                                              int *descriptor);
int32_t cp0_broker_photo_remove(uint64_t photo_id);
int32_t cp0_broker_decode_photo_remove_response(const char *response);
int32_t cp0_broker_intent_send(const uint8_t *action, size_t action_length,
                               const uint8_t *payload,
                               size_t payload_length);
int64_t cp0_broker_intent_take(uint8_t *action, size_t action_capacity,
                               uint8_t *payload, size_t payload_capacity);
int64_t cp0_broker_decode_intent_response(const char *response,
                                          uint8_t *action,
                                          size_t action_capacity,
                                          uint8_t *payload,
                                          size_t payload_capacity);
int32_t cp0_broker_media_session_update(uint32_t state,
                                        uint32_t supported_actions);
int32_t cp0_broker_media_take_action(void);
int32_t cp0_broker_decode_media_action_response(const char *response);

#endif
