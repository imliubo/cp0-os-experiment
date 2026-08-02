#include "cp0_display_client.h"

#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

int cp0_display_test_parse_state_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_display_state *state);

int main(void)
{
    static const char available[] =
        "{\"protocol_version\":1,\"request_id\":7,\"outcome\":{"
        "\"status\":\"state\",\"state\":{\"available\":true,"
        "\"brightness_percent\":75}}}";
    static const char unavailable[] =
        "{\"protocol_version\":1,\"request_id\":8,\"outcome\":{"
        "\"status\":\"state\",\"state\":{\"available\":false,"
        "\"brightness_percent\":null}}}";
    static const char device_error[] =
        "{\"protocol_version\":1,\"request_id\":9,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"unavailable\","
        "\"message\":\"display backlight is unavailable\"}}";
    static const char inconsistent[] =
        "{\"protocol_version\":1,\"request_id\":10,\"outcome\":{"
        "\"status\":\"state\",\"state\":{\"available\":false,"
        "\"brightness_percent\":70}}}";
    struct cp0_display_state state;

    assert(cp0_display_test_parse_state_response(
               available, strlen(available), 7, &state) == CP0_DISPLAY_OK);
    assert(state.available && state.brightness_percent == 75);
    assert(cp0_display_test_parse_state_response(
               available, strlen(available), 6, &state) == CP0_DISPLAY_FAILED);
    assert(cp0_display_test_parse_state_response(
               unavailable, strlen(unavailable), 8, &state) ==
           CP0_DISPLAY_UNAVAILABLE);
    assert(!state.available && state.brightness_percent == 0);
    assert(cp0_display_test_parse_state_response(
               device_error, strlen(device_error), 9, &state) ==
           CP0_DISPLAY_UNAVAILABLE);
    assert(cp0_display_test_parse_state_response(
               inconsistent, strlen(inconsistent), 10, &state) ==
           CP0_DISPLAY_FAILED);
    return 0;
}
