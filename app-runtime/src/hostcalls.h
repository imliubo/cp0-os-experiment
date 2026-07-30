#ifndef CARDPUTERZERO_HOSTCALLS_H
#define CARDPUTERZERO_HOSTCALLS_H

#include <stdint.h>

#include "wasm_export.h"

NativeSymbol *cp0_host_symbols(uint32_t *count);

#endif
