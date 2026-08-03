#include "cp0_power_client.h"

#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

int cp0_power_test_parse_response(const char *response,
                                  size_t response_length,
                                  uint64_t request_id,
                                  enum cp0_power_action action);

int main(void)
{
    static const char restart[] =
        "{\"protocol_version\":1,\"request_id\":7,\"outcome\":{"
        "\"status\":\"accepted\",\"action\":\"restart\"}}";
    static const char power_off[] =
        "{\"protocol_version\":1,\"request_id\":8,\"outcome\":{"
        "\"status\":\"accepted\",\"action\":\"power-off\"}}";
    static const char failure[] =
        "{\"protocol_version\":1,\"request_id\":9,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"operation\","
        "\"message\":\"system power operation failed\"}}";

    assert(cp0_power_test_parse_response(
               restart, strlen(restart), 7, CP0_POWER_RESTART) == 0);
    assert(cp0_power_test_parse_response(
               power_off, strlen(power_off), 8, CP0_POWER_OFF) == 0);
    assert(cp0_power_test_parse_response(
               restart, strlen(restart), 8, CP0_POWER_RESTART) != 0);
    assert(cp0_power_test_parse_response(
               restart, strlen(restart), 7, CP0_POWER_OFF) != 0);
    assert(cp0_power_test_parse_response(
               failure, strlen(failure), 9, CP0_POWER_RESTART) != 0);
    assert(cp0_power_test_parse_response(
               restart, strlen(restart), 7, (enum cp0_power_action)99) != 0);
    return 0;
}
