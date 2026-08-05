#ifndef CARDPUTERZERO_SDK_H
#define CARDPUTERZERO_SDK_H

#include <stddef.h>
#include <stdint.h>

#define CP0_SDK_VERSION_MAJOR 1
#define CP0_SDK_VERSION_MINOR 0
#define CP0_DISPLAY_WIDTH 320U
#define CP0_DISPLAY_HEIGHT 170U
#define CP0_STANDARD_DISPLAY_HEIGHT 150U
#define CP0_MAX_DAMAGE_RECTS 32U
#define CP0_MODIFIER_SHIFT (1U << 0)
#define CP0_MODIFIER_CONTROL (1U << 1)
#define CP0_MODIFIER_ALT (1U << 2)
#define CP0_MODIFIER_SUPER (1U << 3)
#define CP0_MAX_WAIT_MILLISECONDS 1000U
#define CP0_MAX_NOTIFICATION_TITLE_CHARS 32U
#define CP0_MAX_NOTIFICATION_BODY_CHARS 160U
#define CP0_MAX_NETWORK_URL_BYTES 1024U
#define CP0_MAX_NETWORK_BODY_BYTES 2048U
#define CP0_MAX_DOCUMENT_BYTES (16U * 1024U * 1024U)
#define CP0_MAX_DOCUMENT_READ_BYTES 4096U
#define CP0_AUDIO_SAMPLE_RATE_HZ 16000U
#define CP0_AUDIO_CHANNELS 1U
#define CP0_MAX_AUDIO_FRAMES 1024U
#define CP0_CAMERA_WIDTH 320U
#define CP0_CAMERA_HEIGHT 170U
#define CP0_CAMERA_PIXEL_COUNT (CP0_CAMERA_WIDTH * CP0_CAMERA_HEIGHT)
#define CP0_CAMERA_FRAME_BYTES (CP0_CAMERA_PIXEL_COUNT * 2U)
#define CP0_CAMERA_PREVIEW_FPS 30U
#define CP0_CAMERA_PHOTO_WIDTH 1280U
#define CP0_CAMERA_PHOTO_HEIGHT 720U
#define CP0_MAX_LORA_PAYLOAD_BYTES 64U
#define CP0_LORA_METADATA_BYTES 4U
#define CP0_MAX_STORAGE_KEY_BYTES 64U
#define CP0_MAX_STORAGE_VALUE_BYTES (8U * 1024U)
#define CP0_PHOTO_LIST_PAGE_SIZE 8U
#define CP0_PHOTO_VIEW_PAN_MIN (-1000)
#define CP0_PHOTO_VIEW_PAN_MAX 1000
#define CP0_MAX_INTENT_ACTION_BYTES 96U
#define CP0_MAX_INTENT_PAYLOAD_BYTES 1024U
#define CP0_MAX_CHECKPOINT_BYTES (8U * 1024U)

#if !defined(__wasm32__)
#error "CardputerZero applications must target wasm32"
#endif

#if defined(__clang__)
#define CP0_IMPORT(name)                                                       \
    __attribute__((import_module("cardputerzero"), import_name(name)))
#define CP0_EXPORT(name) __attribute__((export_name(name)))
#else
#error "CardputerZero C/C++ SDK 1.0 requires a Clang-compatible wasm compiler"
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef enum cp0_result {
    CP0_OK = 0,
    CP0_ERROR_DENIED = -1,
    CP0_ERROR_UNAVAILABLE = -2,
    CP0_ERROR_INVALID_ARGUMENT = -3,
    CP0_ERROR_RESOURCE_LIMIT = -4,
    CP0_ERROR_INTERNAL = -5,
} cp0_result_t;

typedef struct cp0_rect {
    uint16_t x;
    uint16_t y;
    uint16_t width;
    uint16_t height;
} cp0_rect_t;

typedef struct cp0_key_event {
    uint16_t code;
    uint8_t pressed;
    uint8_t repeated;
    uint8_t modifiers;
    uint8_t reserved[3];
} cp0_key_event_t;

typedef struct cp0_http_response {
    uint16_t status_code;
    uint16_t body_length;
} cp0_http_response_t;

typedef struct cp0_document {
    int32_t handle;
    uint32_t length;
} cp0_document_t;

typedef enum cp0_gpio_line {
    CP0_GPIO_GROVE_FUNCTION = 0,
    CP0_GPIO_EXTERNAL_USB_FUNCTION = 1,
    CP0_GPIO_GROVE_5V_POWER = 2,
    CP0_GPIO_EXTERNAL_5V_POWER = 3,
} cp0_gpio_line_t;

typedef struct cp0_lora_metadata {
    int16_t rssi_dbm;
    int8_t snr_quarter_db;
    uint8_t reserved;
} cp0_lora_metadata_t;

typedef struct cp0_intent_message {
    uint32_t action_length;
    uint32_t payload_length;
} cp0_intent_message_t;

typedef enum cp0_media_playback_state {
    CP0_MEDIA_INACTIVE = 0,
    CP0_MEDIA_PAUSED = 1,
    CP0_MEDIA_PLAYING = 2,
} cp0_media_playback_state_t;

typedef enum cp0_media_action {
    CP0_MEDIA_PLAY_PAUSE = 1,
    CP0_MEDIA_PREVIOUS = 2,
    CP0_MEDIA_NEXT = 3,
} cp0_media_action_t;

typedef enum cp0_photo_view_zoom {
    CP0_PHOTO_VIEW_FIT = 0,
    CP0_PHOTO_VIEW_HALF = 1,
    CP0_PHOTO_VIEW_ACTUAL = 2,
} cp0_photo_view_zoom_t;

/* Optional multitasking lifecycle exports. */
CP0_EXPORT("cp0_app_checkpoint")
int32_t cp0_app_checkpoint(uint8_t *output, uint32_t output_capacity,
                           uint32_t *schema_version);
CP0_EXPORT("cp0_app_restore")
cp0_result_t cp0_app_restore(uint32_t schema_version, const uint8_t *payload,
                             uint32_t payload_length);

#define CP0_MEDIA_SUPPORT_PLAY_PAUSE (1U << 0)
#define CP0_MEDIA_SUPPORT_PREVIOUS (1U << 1)
#define CP0_MEDIA_SUPPORT_NEXT (1U << 2)
#define CP0_MEDIA_SUPPORT_ALL                                                  \
    (CP0_MEDIA_SUPPORT_PLAY_PAUSE | CP0_MEDIA_SUPPORT_PREVIOUS |              \
     CP0_MEDIA_SUPPORT_NEXT)

#include "cardputerzero_imports.h"

static inline cp0_result_t cp0_http_get(const uint8_t *url,
                                        uint32_t url_length, uint8_t *body,
                                        uint32_t body_capacity,
                                        cp0_http_response_t *response) {
    int64_t packed;
    uint32_t status_code;
    uint32_t body_length;
    uint32_t index;

    if (url == NULL || body == NULL || response == NULL ||
        url_length <= 8U || url_length > CP0_MAX_NETWORK_URL_BYTES ||
        body_capacity == 0U || body_capacity > CP0_MAX_NETWORK_BODY_BYTES ||
        url[0] != 'h' || url[1] != 't' || url[2] != 't' || url[3] != 'p' ||
        url[4] != 's' || url[5] != ':' || url[6] != '/' || url[7] != '/')
        return CP0_ERROR_INVALID_ARGUMENT;
    for (index = 0; index < url_length; index++) {
        if (url[index] < 0x20U || url[index] == 0x7fU)
            return CP0_ERROR_INVALID_ARGUMENT;
    }
    packed = cp0_http_get_raw(url, url_length, body, body_capacity);
    if (packed < 0)
        return packed >= CP0_ERROR_INTERNAL ? (cp0_result_t)packed
                                           : CP0_ERROR_INTERNAL;
    status_code = (uint32_t)(((uint64_t)packed >> 32) & 0xffffU);
    body_length = (uint32_t)((uint64_t)packed & UINT32_MAX);
    if (status_code < 100U || status_code > 599U ||
        body_length > body_capacity || body_length > UINT16_MAX)
        return CP0_ERROR_INTERNAL;
    response->status_code = (uint16_t)status_code;
    response->body_length = (uint16_t)body_length;
    return CP0_OK;
}

static inline cp0_result_t cp0_document_open(cp0_document_t *document) {
    int64_t packed;
    int32_t handle;
    uint32_t length;

    if (document == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    packed = cp0_document_open_raw();
    if (packed < 0)
        return packed >= CP0_ERROR_INTERNAL ? (cp0_result_t)packed
                                           : CP0_ERROR_INTERNAL;
    handle = (int32_t)((uint64_t)packed >> 32);
    length = (uint32_t)packed;
    if (handle <= 0 || length > CP0_MAX_DOCUMENT_BYTES)
        return CP0_ERROR_INTERNAL;
    document->handle = handle;
    document->length = length;
    return CP0_OK;
}

static inline cp0_result_t cp0_document_read(const cp0_document_t *document,
                                             uint64_t offset,
                                             uint8_t *buffer,
                                             uint32_t capacity,
                                             uint32_t *bytes_read) {
    int64_t count;

    if (document == NULL || document->handle <= 0 || buffer == NULL ||
        capacity == 0U || capacity > CP0_MAX_DOCUMENT_READ_BYTES ||
        bytes_read == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    count = cp0_document_read_raw(document->handle, offset, buffer, capacity);
    if (count < 0)
        return count >= CP0_ERROR_INTERNAL ? (cp0_result_t)count
                                          : CP0_ERROR_INTERNAL;
    if ((uint64_t)count > capacity)
        return CP0_ERROR_INTERNAL;
    *bytes_read = (uint32_t)count;
    return CP0_OK;
}

static inline cp0_result_t cp0_document_close(cp0_document_t *document) {
    cp0_result_t result;

    if (document == NULL || document->handle <= 0)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_document_close_raw(document->handle);
    if (result == CP0_OK) {
        document->handle = 0;
        document->length = 0;
    }
    return result;
}

static inline cp0_result_t cp0_audio_play(const int16_t *samples,
                                          uint32_t frame_count) {
    if (samples == NULL || frame_count == 0U ||
        frame_count > CP0_MAX_AUDIO_FRAMES)
        return CP0_ERROR_INVALID_ARGUMENT;
    return cp0_audio_play_pcm_s16le_raw((const uint8_t *)samples,
                                        frame_count * sizeof(int16_t));
}

static inline cp0_result_t cp0_audio_capture(int16_t *samples,
                                             uint32_t frame_capacity,
                                             uint32_t *frames_captured) {
    int32_t result;
    uint32_t sample_capacity;

    if (samples == NULL || frame_capacity == 0U ||
        frame_capacity > CP0_MAX_AUDIO_FRAMES || frames_captured == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    sample_capacity = frame_capacity * sizeof(int16_t);
    result = cp0_audio_capture_pcm_s16le_raw((uint8_t *)samples,
                                             sample_capacity);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if ((uint32_t)result != sample_capacity || result % (int32_t)sizeof(int16_t) != 0)
        return CP0_ERROR_INTERNAL;
    *frames_captured = (uint32_t)result / sizeof(int16_t);
    return CP0_OK;
}

static inline cp0_result_t cp0_camera_capture(uint16_t *pixels,
                                              uint32_t pixel_count) {
    if (pixels == NULL || pixel_count != CP0_CAMERA_PIXEL_COUNT)
        return CP0_ERROR_INVALID_ARGUMENT;
    return cp0_camera_capture_rgb565_raw((uint8_t *)pixels,
                                         pixel_count * sizeof(uint16_t));
}

static inline cp0_result_t cp0_camera_capture_photo(uint64_t *photo_id) {
    int64_t result;

    if (photo_id == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_camera_capture_photo_raw();
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if (result == 0)
        return CP0_ERROR_INTERNAL;
    *photo_id = (uint64_t)result;
    return CP0_OK;
}

static inline cp0_result_t cp0_gpio_read(cp0_gpio_line_t line,
                                         uint8_t *value) {
    int32_t result;

    if ((uint32_t)line > (uint32_t)CP0_GPIO_EXTERNAL_5V_POWER || value == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_gpio_read_raw((uint32_t)line);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if (result > 1)
        return CP0_ERROR_INTERNAL;
    *value = (uint8_t)result;
    return CP0_OK;
}

static inline cp0_result_t cp0_gpio_write(cp0_gpio_line_t line,
                                          uint8_t value) {
    if ((uint32_t)line > (uint32_t)CP0_GPIO_EXTERNAL_5V_POWER || value > 1U)
        return CP0_ERROR_INVALID_ARGUMENT;
    return cp0_gpio_write_raw((uint32_t)line, value);
}

static inline cp0_result_t cp0_lora_send(const uint8_t *payload,
                                         uint32_t payload_length) {
    if (payload == NULL || payload_length == 0U ||
        payload_length > CP0_MAX_LORA_PAYLOAD_BYTES)
        return CP0_ERROR_INVALID_ARGUMENT;
    return cp0_lora_send_raw(payload, payload_length);
}

static inline cp0_result_t cp0_lora_receive(
    uint8_t *payload, uint32_t payload_capacity, cp0_lora_metadata_t *metadata,
    uint32_t timeout_milliseconds, uint32_t *payload_length) {
    int32_t result;

    if (payload == NULL || payload_capacity == 0U ||
        payload_capacity > CP0_MAX_LORA_PAYLOAD_BYTES || metadata == NULL ||
        timeout_milliseconds == 0U ||
        timeout_milliseconds > CP0_MAX_WAIT_MILLISECONDS ||
        payload_length == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_lora_receive_raw(payload, payload_capacity,
                                  (uint8_t *)metadata,
                                  CP0_LORA_METADATA_BYTES,
                                  timeout_milliseconds);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if ((uint32_t)result > payload_capacity)
        return CP0_ERROR_INTERNAL;
    *payload_length = (uint32_t)result;
    return CP0_OK;
}

static inline uint8_t cp0_storage_key_is_valid(const uint8_t *key,
                                               uint32_t key_length) {
    uint32_t index;

    if (key == NULL || key_length == 0U ||
        key_length > CP0_MAX_STORAGE_KEY_BYTES || key[0] == '.')
        return 0U;
    for (index = 0; index < key_length; index++) {
        uint8_t byte = key[index];
        if (!((byte >= 'A' && byte <= 'Z') ||
              (byte >= 'a' && byte <= 'z') ||
              (byte >= '0' && byte <= '9') || byte == '.' || byte == '_' ||
              byte == '-'))
            return 0U;
    }
    return 1U;
}

static inline cp0_result_t cp0_storage_put(const uint8_t *key,
                                           uint32_t key_length,
                                           const uint8_t *value,
                                           uint32_t value_length) {
    if (!cp0_storage_key_is_valid(key, key_length) || value == NULL ||
        value_length == 0U || value_length > CP0_MAX_STORAGE_VALUE_BYTES)
        return CP0_ERROR_INVALID_ARGUMENT;
    return cp0_storage_put_raw(key, key_length, value, value_length);
}

static inline cp0_result_t cp0_storage_get(const uint8_t *key,
                                           uint32_t key_length,
                                           uint8_t *value,
                                           uint32_t value_capacity,
                                           uint32_t *value_length) {
    int32_t result;

    if (!cp0_storage_key_is_valid(key, key_length) || value == NULL ||
        value_capacity == 0U || value_capacity > CP0_MAX_STORAGE_VALUE_BYTES ||
        value_length == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_storage_get_raw(key, key_length, value, value_capacity);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if ((uint32_t)result > value_capacity)
        return CP0_ERROR_INTERNAL;
    *value_length = (uint32_t)result;
    return CP0_OK;
}

static inline cp0_result_t cp0_storage_delete(const uint8_t *key,
                                              uint32_t key_length,
                                              uint8_t *existed) {
    int32_t result;

    if (!cp0_storage_key_is_valid(key, key_length) || existed == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_storage_delete_raw(key, key_length);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if (result > 1)
        return CP0_ERROR_INTERNAL;
    *existed = (uint8_t)result;
    return CP0_OK;
}

static inline cp0_result_t cp0_photos_put(const uint8_t *key,
                                          uint32_t key_length,
                                          const uint8_t *value,
                                          uint32_t value_length) {
    if (!cp0_storage_key_is_valid(key, key_length) || value == NULL ||
        value_length == 0U || value_length > CP0_MAX_STORAGE_VALUE_BYTES)
        return CP0_ERROR_INVALID_ARGUMENT;
    return cp0_photos_put_raw(key, key_length, value, value_length);
}

static inline cp0_result_t cp0_photos_get(const uint8_t *key,
                                          uint32_t key_length,
                                          uint8_t *value,
                                          uint32_t value_capacity,
                                          uint32_t *value_length) {
    int32_t result;

    if (!cp0_storage_key_is_valid(key, key_length) || value == NULL ||
        value_capacity == 0U || value_capacity > CP0_MAX_STORAGE_VALUE_BYTES ||
        value_length == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_photos_get_raw(key, key_length, value, value_capacity);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if ((uint32_t)result > value_capacity)
        return CP0_ERROR_INTERNAL;
    *value_length = (uint32_t)result;
    return CP0_OK;
}

static inline cp0_result_t cp0_photos_index_get_for_update(
    uint8_t *value, uint32_t value_capacity, uint32_t *value_length) {
    int32_t result;

    if (value == NULL || value_capacity == 0U ||
        value_capacity > CP0_MAX_STORAGE_VALUE_BYTES || value_length == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_photos_index_get_raw(value, value_capacity);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if ((uint32_t)result > value_capacity)
        return CP0_ERROR_INTERNAL;
    *value_length = (uint32_t)result;
    return CP0_OK;
}

static inline cp0_result_t cp0_photos_delete(const uint8_t *key,
                                             uint32_t key_length,
                                             uint8_t *existed) {
    int32_t result;

    if (!cp0_storage_key_is_valid(key, key_length) || existed == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_photos_delete_raw(key, key_length);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if (result > 1)
        return CP0_ERROR_INTERNAL;
    *existed = (uint8_t)result;
    return CP0_OK;
}

static inline cp0_result_t cp0_photos_import_rgb565(
    const uint16_t *pixels, uint32_t pixel_count, uint64_t suggested_id,
    uint64_t *photo_id) {
    int64_t result;

    if (pixels == NULL || pixel_count != CP0_CAMERA_PIXEL_COUNT ||
        suggested_id > INT64_MAX || photo_id == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_photos_import_rgb565_raw(
        (const uint8_t *)pixels, CP0_CAMERA_FRAME_BYTES, suggested_id);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if (result == 0)
        return CP0_ERROR_INTERNAL;
    *photo_id = (uint64_t)result;
    return CP0_OK;
}

static inline cp0_result_t cp0_photos_load_rgb565(uint64_t photo_id,
                                                  uint16_t *pixels,
                                                  uint32_t pixel_count) {
    if (photo_id == 0U || photo_id > INT64_MAX || pixels == NULL ||
        pixel_count != CP0_CAMERA_PIXEL_COUNT)
        return CP0_ERROR_INVALID_ARGUMENT;
    return cp0_photos_load_rgb565_raw(
        photo_id, (uint8_t *)pixels, CP0_CAMERA_FRAME_BYTES);
}

static inline cp0_result_t cp0_photos_load_view_rgb565(
    uint64_t photo_id, cp0_photo_view_zoom_t zoom, int32_t pan_x,
    int32_t pan_y, uint16_t *pixels, uint32_t pixel_count) {
    if (photo_id == 0U || photo_id > INT64_MAX ||
        (uint32_t)zoom > (uint32_t)CP0_PHOTO_VIEW_ACTUAL ||
        pan_x < CP0_PHOTO_VIEW_PAN_MIN || pan_x > CP0_PHOTO_VIEW_PAN_MAX ||
        pan_y < CP0_PHOTO_VIEW_PAN_MIN || pan_y > CP0_PHOTO_VIEW_PAN_MAX ||
        pixels == NULL || pixel_count != CP0_CAMERA_PIXEL_COUNT)
        return CP0_ERROR_INVALID_ARGUMENT;
    return cp0_photos_load_view_rgb565_raw(
        photo_id, (uint32_t)zoom, pan_x, pan_y, (uint8_t *)pixels,
        CP0_CAMERA_FRAME_BYTES);
}

static inline cp0_result_t cp0_photos_remove(uint64_t photo_id,
                                             uint8_t *existed) {
    int32_t result;

    if (photo_id == 0U || photo_id > INT64_MAX || existed == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_photos_remove_raw(photo_id);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if (result > 1)
        return CP0_ERROR_INTERNAL;
    *existed = (uint8_t)result;
    return CP0_OK;
}

static inline uint8_t cp0_intent_action_is_valid(const uint8_t *action,
                                                 uint32_t action_length) {
    uint32_t part_length = 0U;
    uint32_t parts = 1U;

    if (action == NULL || action_length == 0U ||
        action_length > CP0_MAX_INTENT_ACTION_BYTES)
        return 0U;
    for (uint32_t index = 0; index < action_length; index++) {
        uint8_t byte = action[index];
        if (byte == '.') {
            if (part_length == 0U || action[index - 1U] == '-')
                return 0U;
            part_length = 0U;
            parts++;
            continue;
        }
        if (part_length == 0U && (byte < 'a' || byte > 'z'))
            return 0U;
        if (!((byte >= 'a' && byte <= 'z') ||
              (byte >= '0' && byte <= '9') || byte == '-'))
            return 0U;
        part_length++;
        if (part_length > 32U)
            return 0U;
    }
    return parts >= 3U && part_length > 0U && action[action_length - 1U] != '-';
}

static inline cp0_result_t cp0_intent_send(const uint8_t *action,
                                           uint32_t action_length,
                                           const uint8_t *payload,
                                           uint32_t payload_length) {
    if (!cp0_intent_action_is_valid(action, action_length) ||
        payload_length > CP0_MAX_INTENT_PAYLOAD_BYTES ||
        (payload == NULL && payload_length != 0U))
        return CP0_ERROR_INVALID_ARGUMENT;
    return cp0_intent_send_raw(action, action_length, payload, payload_length);
}

static inline cp0_result_t cp0_intent_take(
    uint8_t *action, uint32_t action_capacity, uint8_t *payload,
    uint32_t payload_capacity, cp0_intent_message_t *message) {
    int64_t result;
    uint32_t action_length;
    uint32_t payload_length;

    if (action == NULL || action_capacity == 0U ||
        action_capacity > CP0_MAX_INTENT_ACTION_BYTES || payload == NULL ||
        payload_capacity == 0U ||
        payload_capacity > CP0_MAX_INTENT_PAYLOAD_BYTES || message == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_intent_take_raw(action, action_capacity, payload,
                                 payload_capacity);
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    action_length = (uint32_t)((uint64_t)result >> 32);
    payload_length = (uint32_t)result;
    if ((result != 0 && action_length == 0U) ||
        action_length > action_capacity || payload_length > payload_capacity)
        return CP0_ERROR_INTERNAL;
    message->action_length = action_length;
    message->payload_length = payload_length;
    return CP0_OK;
}

static inline cp0_result_t cp0_media_session_update(
    cp0_media_playback_state_t state, uint32_t supported_actions) {
    if ((supported_actions & ~CP0_MEDIA_SUPPORT_ALL) != 0U ||
        (state == CP0_MEDIA_INACTIVE && supported_actions != 0U) ||
        ((state == CP0_MEDIA_PAUSED || state == CP0_MEDIA_PLAYING) &&
         supported_actions == 0U) ||
        state < CP0_MEDIA_INACTIVE || state > CP0_MEDIA_PLAYING)
        return CP0_ERROR_INVALID_ARGUMENT;
    return cp0_media_session_update_raw((uint32_t)state, supported_actions);
}

static inline cp0_result_t cp0_media_take_action(cp0_media_action_t *action,
                                                 uint8_t *available) {
    int32_t result;

    if (action == NULL || available == NULL)
        return CP0_ERROR_INVALID_ARGUMENT;
    result = cp0_media_take_action_raw();
    if (result < 0)
        return result >= CP0_ERROR_INTERNAL ? (cp0_result_t)result
                                           : CP0_ERROR_INTERNAL;
    if (result > CP0_MEDIA_NEXT)
        return CP0_ERROR_INTERNAL;
    *available = result != 0;
    if (result != 0)
        *action = (cp0_media_action_t)result;
    return CP0_OK;
}

#ifdef __cplusplus
}
#endif

#endif
