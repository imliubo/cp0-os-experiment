#ifndef CARDPUTERZERO_SDK_H
#define CARDPUTERZERO_SDK_H

#include <stddef.h>
#include <stdint.h>

#define CP0_SDK_VERSION_MAJOR 0
#define CP0_SDK_VERSION_MINOR 1
#define CP0_DISPLAY_WIDTH 320U
#define CP0_DISPLAY_HEIGHT 170U
#define CP0_MAX_WAIT_MILLISECONDS 1000U
#define CP0_MAX_NOTIFICATION_TITLE_CHARS 32U
#define CP0_MAX_NOTIFICATION_BODY_CHARS 160U

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

CP0_IMPORT("cp0_monotonic_milliseconds")
uint64_t cp0_monotonic_milliseconds(void);

CP0_IMPORT("cp0_wait_event")
cp0_result_t cp0_wait_event(int32_t timeout_milliseconds);

CP0_IMPORT("cp0_post_notification")
cp0_result_t cp0_post_notification(const uint8_t *title, uint32_t title_length,
                                   const uint8_t *body, uint32_t body_length);

#ifdef __cplusplus
}
#endif

#endif
