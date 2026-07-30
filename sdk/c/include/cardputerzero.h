#ifndef CARDPUTERZERO_SDK_H
#define CARDPUTERZERO_SDK_H

#include <stddef.h>
#include <stdint.h>

#define CP0_SDK_VERSION_MAJOR 0
#define CP0_SDK_VERSION_MINOR 1
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

#if !defined(__wasm32__)
#error "CardputerZero applications must target wasm32"
#endif

#if defined(__clang__)
#define CP0_IMPORT(name)                                                       \
    __attribute__((import_module("cardputerzero"), import_name(name)))
#else
#error "CardputerZero C/C++ SDK 0.1 requires a Clang-compatible wasm compiler"
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

CP0_IMPORT("cp0_monotonic_milliseconds")
uint64_t cp0_monotonic_milliseconds(void);

CP0_IMPORT("cp0_wait_event")
cp0_result_t cp0_wait_event(int32_t timeout_milliseconds);

CP0_IMPORT("cp0_display_dimensions")
uint32_t cp0_display_dimensions(void);

CP0_IMPORT("cp0_present_rgb565")
cp0_result_t cp0_present_rgb565(const uint8_t *pixels, uint32_t pixel_bytes,
                                const cp0_rect_t *damage,
                                uint32_t damage_bytes);

CP0_IMPORT("cp0_poll_key_event")
int32_t cp0_poll_key_event(cp0_key_event_t *event, uint32_t event_bytes,
                           int32_t timeout_milliseconds);

CP0_IMPORT("cp0_post_notification")
cp0_result_t cp0_post_notification(const uint8_t *title, uint32_t title_length,
                                   const uint8_t *body, uint32_t body_length);

CP0_IMPORT("cp0_http_get")
int64_t cp0_http_get_raw(const uint8_t *url, uint32_t url_length,
                         uint8_t *body, uint32_t body_capacity);

CP0_IMPORT("cp0_document_open")
int64_t cp0_document_open_raw(void);

CP0_IMPORT("cp0_document_read")
int64_t cp0_document_read_raw(int32_t handle, uint64_t offset,
                              uint8_t *buffer, uint32_t capacity);

CP0_IMPORT("cp0_document_close")
cp0_result_t cp0_document_close_raw(int32_t handle);

CP0_IMPORT("cp0_audio_play_pcm_s16le")
cp0_result_t cp0_audio_play_pcm_s16le_raw(const uint8_t *samples,
                                          uint32_t sample_bytes);

CP0_IMPORT("cp0_audio_capture_pcm_s16le")
int32_t cp0_audio_capture_pcm_s16le_raw(uint8_t *samples,
                                       uint32_t sample_capacity);

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

#ifdef __cplusplus
}
#endif

#endif
