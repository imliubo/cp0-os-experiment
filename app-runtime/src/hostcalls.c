#include "hostcalls.h"
#include "broker_client.h"
#include "camera.h"
#include "display.h"
#include "document.h"

#include <errno.h>
#include <stdint.h>
#include <time.h>

static int64_t cp0_monotonic_milliseconds(
    wasm_exec_env_t execution_environment) {
    struct timespec now;

    (void)execution_environment;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
        return 0;
    return (int64_t)now.tv_sec * 1000 + (int64_t)now.tv_nsec / 1000000;
}

static int32_t cp0_wait_event(wasm_exec_env_t execution_environment,
                              int32_t timeout_ms) {
    (void)execution_environment;
    if (timeout_ms < 0 || timeout_ms > 1000)
        return -1;
    return cp0_display_wait(timeout_ms);
}

static int32_t cp0_get_display_dimensions(
    wasm_exec_env_t execution_environment) {
    (void)execution_environment;
    return (int32_t)cp0_display_dimensions();
}

static int32_t cp0_present_rgb565(wasm_exec_env_t execution_environment,
                                  const uint8_t *pixels,
                                  uint32_t pixel_bytes,
                                  const uint8_t *damage,
                                  uint32_t damage_bytes) {
    (void)execution_environment;
    return cp0_display_present_rgb565(pixels, (size_t)pixel_bytes, damage,
                                      (size_t)damage_bytes);
}

static int32_t cp0_poll_key_event(wasm_exec_env_t execution_environment,
                                  uint8_t *event, uint32_t event_bytes,
                                  int32_t timeout_ms) {
    (void)execution_environment;
    return cp0_display_poll_key_event(event, (size_t)event_bytes, timeout_ms);
}

static int32_t cp0_post_notification(wasm_exec_env_t execution_environment,
                                     const uint8_t *title,
                                     uint32_t title_length,
                                     const uint8_t *body,
                                     uint32_t body_length) {
    (void)execution_environment;
    return cp0_broker_post_notification(title, (size_t)title_length, body,
                                        (size_t)body_length);
}

static int64_t cp0_http_get(wasm_exec_env_t execution_environment,
                            const uint8_t *url, uint32_t url_length,
                            uint8_t *body, uint32_t body_capacity) {
    (void)execution_environment;
    return cp0_broker_http_get(url, (size_t)url_length, body,
                               (size_t)body_capacity);
}

static int64_t cp0_open_document(wasm_exec_env_t execution_environment) {
    (void)execution_environment;
    return cp0_document_open();
}

static int64_t cp0_read_document(wasm_exec_env_t execution_environment,
                                 int32_t handle, int64_t offset,
                                 uint8_t *buffer, uint32_t capacity) {
    (void)execution_environment;
    if (offset < 0)
        return CP0_BROKER_INVALID_ARGUMENT;
    return cp0_document_read(handle, (uint64_t)offset, buffer,
                             (size_t)capacity);
}

static int32_t cp0_close_document(wasm_exec_env_t execution_environment,
                                  int32_t handle) {
    (void)execution_environment;
    return cp0_document_close(handle);
}

static int32_t cp0_audio_play_pcm_s16le(
    wasm_exec_env_t execution_environment, const uint8_t *samples,
    uint32_t sample_bytes) {
    (void)execution_environment;
    return cp0_broker_play_audio(samples, (size_t)sample_bytes);
}

static int32_t cp0_audio_capture_pcm_s16le(
    wasm_exec_env_t execution_environment, uint8_t *samples,
    uint32_t sample_capacity) {
    (void)execution_environment;
    return cp0_broker_capture_audio(samples, (size_t)sample_capacity);
}

static int32_t cp0_capture_camera_rgb565(
    wasm_exec_env_t execution_environment, uint8_t *pixels,
    uint32_t pixel_bytes) {
    (void)execution_environment;
    return cp0_camera_capture_rgb565(pixels, (size_t)pixel_bytes);
}

static int32_t cp0_gpio_read(wasm_exec_env_t execution_environment,
                             uint32_t line) {
    (void)execution_environment;
    return cp0_broker_gpio_read(line);
}

static int32_t cp0_gpio_write(wasm_exec_env_t execution_environment,
                              uint32_t line, uint32_t value) {
    (void)execution_environment;
    return cp0_broker_gpio_write(line, value);
}

static int32_t cp0_lora_send(wasm_exec_env_t execution_environment,
                             const uint8_t *payload,
                             uint32_t payload_length) {
    (void)execution_environment;
    return cp0_broker_lora_send(payload, (size_t)payload_length);
}

static int32_t cp0_lora_receive(wasm_exec_env_t execution_environment,
                                uint8_t *payload, uint32_t payload_capacity,
                                uint8_t *metadata, uint32_t metadata_bytes,
                                uint32_t timeout_ms) {
    (void)execution_environment;
    return cp0_broker_lora_receive(payload, (size_t)payload_capacity, metadata,
                                   (size_t)metadata_bytes, timeout_ms);
}

static int32_t cp0_storage_put(wasm_exec_env_t execution_environment,
                               const uint8_t *key, uint32_t key_length,
                               const uint8_t *value, uint32_t value_length) {
    (void)execution_environment;
    return cp0_broker_storage_put(key, (size_t)key_length, value,
                                  (size_t)value_length);
}

static int32_t cp0_storage_get(wasm_exec_env_t execution_environment,
                               const uint8_t *key, uint32_t key_length,
                               uint8_t *value, uint32_t value_capacity) {
    (void)execution_environment;
    return cp0_broker_storage_get(key, (size_t)key_length, value,
                                  (size_t)value_capacity);
}

static int32_t cp0_storage_delete(wasm_exec_env_t execution_environment,
                                  const uint8_t *key, uint32_t key_length) {
    (void)execution_environment;
    return cp0_broker_storage_delete(key, (size_t)key_length);
}

static int32_t cp0_photos_put(wasm_exec_env_t execution_environment,
                              const uint8_t *key, uint32_t key_length,
                              const uint8_t *value, uint32_t value_length) {
    (void)execution_environment;
    return cp0_broker_photo_put(key, (size_t)key_length, value,
                                (size_t)value_length);
}

static int32_t cp0_photos_get(wasm_exec_env_t execution_environment,
                              const uint8_t *key, uint32_t key_length,
                              uint8_t *value, uint32_t value_capacity) {
    (void)execution_environment;
    return cp0_broker_photo_get(key, (size_t)key_length, value,
                                (size_t)value_capacity);
}

static int32_t cp0_photos_index_get(wasm_exec_env_t execution_environment,
                                    uint8_t *value,
                                    uint32_t value_capacity) {
    (void)execution_environment;
    return cp0_broker_photo_index_get(value, (size_t)value_capacity);
}

static int32_t cp0_photos_delete(wasm_exec_env_t execution_environment,
                                 const uint8_t *key, uint32_t key_length) {
    (void)execution_environment;
    return cp0_broker_photo_delete(key, (size_t)key_length);
}

static int32_t cp0_intent_send(wasm_exec_env_t execution_environment,
                               const uint8_t *action, uint32_t action_length,
                               const uint8_t *payload,
                               uint32_t payload_length) {
    (void)execution_environment;
    return cp0_broker_intent_send(action, (size_t)action_length, payload,
                                  (size_t)payload_length);
}

static int64_t cp0_intent_take(wasm_exec_env_t execution_environment,
                               uint8_t *action, uint32_t action_capacity,
                               uint8_t *payload, uint32_t payload_capacity) {
    (void)execution_environment;
    return cp0_broker_intent_take(action, (size_t)action_capacity, payload,
                                  (size_t)payload_capacity);
}

static int32_t cp0_media_session_update(
    wasm_exec_env_t execution_environment, uint32_t state,
    uint32_t supported_actions) {
    (void)execution_environment;
    return cp0_broker_media_session_update(state, supported_actions);
}

static int32_t cp0_media_take_action(
    wasm_exec_env_t execution_environment) {
    (void)execution_environment;
    return cp0_broker_media_take_action();
}

static NativeSymbol symbols[] = {
#include "hostcall_symbols.inc"
};

NativeSymbol *cp0_host_symbols(uint32_t *count) {
    *count = (uint32_t)(sizeof(symbols) / sizeof(symbols[0]));
    return symbols;
}
