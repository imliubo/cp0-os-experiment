#include "hostcalls.h"

#include <errno.h>
#include <stdint.h>
#include <time.h>

static int32_t cp0_wait_event(wasm_exec_env_t execution_environment,
                              int32_t timeout_ms) {
    struct timespec timeout;

    (void)execution_environment;
    if (timeout_ms < 0 || timeout_ms > 1000)
        return -1;
    timeout.tv_sec = timeout_ms / 1000;
    timeout.tv_nsec = (long)(timeout_ms % 1000) * 1000L * 1000L;
    while (nanosleep(&timeout, &timeout) != 0) {
        if (errno != EINTR)
            return -1;
    }
    return 0;
}

static NativeSymbol symbols[] = {
    {"cp0_wait_event", (void *)cp0_wait_event, "(i)i", NULL},
};

NativeSymbol *cp0_host_symbols(uint32_t *count) {
    *count = (uint32_t)(sizeof(symbols) / sizeof(symbols[0]));
    return symbols;
}
