#include "cp0_connectivity_client.h"

#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

int cp0_connectivity_test_parse_state_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_connectivity_state *state);

int main(void)
{
    static const char available[] =
        "{\"protocol_version\":1,\"request_id\":7,\"outcome\":{"
        "\"status\":\"state\",\"state\":{\"available\":true,"
        "\"wifi_available\":true,\"wifi_enabled\":true,"
        "\"airplane_mode\":false}}}";
    static const char no_adapter[] =
        "{\"protocol_version\":1,\"request_id\":8,\"outcome\":{"
        "\"status\":\"state\",\"state\":{\"available\":true,"
        "\"wifi_available\":false,\"wifi_enabled\":false,"
        "\"airplane_mode\":false}}}";
    static const char unavailable[] =
        "{\"protocol_version\":1,\"request_id\":9,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"unavailable\","
        "\"message\":\"connectivity control is unavailable\"}}";
    static const char inconsistent[] =
        "{\"protocol_version\":1,\"request_id\":10,\"outcome\":{"
        "\"status\":\"state\",\"state\":{\"available\":true,"
        "\"wifi_available\":true,\"wifi_enabled\":true,"
        "\"airplane_mode\":true}}}";
    struct cp0_connectivity_state state;

    assert(cp0_connectivity_test_parse_state_response(
               available, strlen(available), 7, &state) ==
           CP0_CONNECTIVITY_OK);
    assert(state.available && state.wifi_available && state.wifi_enabled);
    assert(!state.airplane_mode);
    assert(cp0_connectivity_test_parse_state_response(
               no_adapter, strlen(no_adapter), 8, &state) ==
           CP0_CONNECTIVITY_OK);
    assert(state.available && !state.wifi_available && !state.wifi_enabled);
    assert(cp0_connectivity_test_parse_state_response(
               unavailable, strlen(unavailable), 9, &state) ==
           CP0_CONNECTIVITY_UNAVAILABLE);
    assert(cp0_connectivity_test_parse_state_response(
               inconsistent, strlen(inconsistent), 10, &state) ==
           CP0_CONNECTIVITY_FAILED);
    assert(cp0_connectivity_test_parse_state_response(
               available, strlen(available), 6, &state) ==
           CP0_CONNECTIVITY_FAILED);
    return 0;
}
