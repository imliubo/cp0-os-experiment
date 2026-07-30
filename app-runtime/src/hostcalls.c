#include "hostcalls.h"
#include "broker_client.h"
#include "display.h"

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

static int32_t cp0_post_notification(wasm_exec_env_t execution_environment,
                                     const uint8_t *title,
                                     uint32_t title_length,
                                     const uint8_t *body,
                                     uint32_t body_length) {
    (void)execution_environment;
    return cp0_broker_post_notification(title, (size_t)title_length, body,
                                        (size_t)body_length);
}

static NativeSymbol symbols[] = {
    {"cp0_monotonic_milliseconds", (void *)cp0_monotonic_milliseconds, "()I",
     NULL},
    {"cp0_wait_event", (void *)cp0_wait_event, "(i)i", NULL},
    {"cp0_display_dimensions", (void *)cp0_get_display_dimensions, "()i",
     NULL},
    {"cp0_present_rgb565", (void *)cp0_present_rgb565, "(*~*~)i", NULL},
    {"cp0_post_notification", (void *)cp0_post_notification, "(*~*~)i", NULL},
};

NativeSymbol *cp0_host_symbols(uint32_t *count) {
    *count = (uint32_t)(sizeof(symbols) / sizeof(symbols[0]));
    return symbols;
}
