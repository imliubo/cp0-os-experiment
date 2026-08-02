#include "cp0_provision_client.h"

#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

int cp0_provision_test_parse_state(
    const char *response, size_t length, uint64_t request_id,
    struct cp0_provision_status *status,
    char error[CP0_PROVISION_ERROR_MAX + 1]);
int cp0_provision_test_parse_wifi(
    const char *response, size_t length, uint64_t request_id,
    struct cp0_provision_wifi_list *list,
    char error[CP0_PROVISION_ERROR_MAX + 1]);
bool cp0_provision_test_escape(const char *input, char *output,
                               size_t capacity);

int main(void)
{
    static const char state_response[] =
        "{\"protocol_version\":1,\"request_id\":7,\"outcome\":{"
        "\"status\":\"state\",\"state\":{\"phase\":\"review\","
        "\"locale\":\"en_US.UTF-8\",\"country\":\"US\","
        "\"timezone\":\"America/Los_Angeles\",\"hostname\":\"cp0-one\","
        "\"display_name\":\"Test Owner\",\"username\":\"owner\","
        "\"password_configured\":true,\"network_choice\":{"
        "\"kind\":\"wifi\",\"profile_id\":\"cp0-setup\","
        "\"ssid\":\"Lab WiFi\"},\"ssh_enabled\":true}}}";
    static const char wifi_response[] =
        "{\"protocol_version\":1,\"request_id\":8,\"outcome\":{"
        "\"status\":\"wifi-list\",\"networks\":[{\"ssid\":\"Lab\","
        "\"signal_percent\":91,\"security\":\"wpa3\","
        "\"connected\":false},{\"ssid\":\"Guest\","
        "\"signal_percent\":40,\"security\":\"open\","
        "\"connected\":true}]}}";
    static const char error_response[] =
        "{\"protocol_version\":1,\"request_id\":9,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"repair-required\","
        "\"message\":\"Recovery is required\"}}";
    static const char invalid_signal[] =
        "{\"protocol_version\":1,\"request_id\":10,\"outcome\":{"
        "\"status\":\"wifi-list\",\"networks\":[{\"ssid\":\"Lab\","
        "\"signal_percent\":101,\"security\":\"wpa2\","
        "\"connected\":false}]}}";
    struct cp0_provision_status status;
    struct cp0_provision_wifi_list wifi;
    char error[CP0_PROVISION_ERROR_MAX + 1];
    char escaped[64];

    assert(cp0_provision_test_parse_state(
               state_response, strlen(state_response), 7, &status, error) ==
           CP0_PROVISION_OK);
    assert(status.phase == CP0_PROVISION_REVIEW);
    assert(status.password_configured && status.ssh_enabled);
    assert(status.network_kind == CP0_PROVISION_NETWORK_WIFI);
    assert(strcmp(status.username, "owner") == 0);
    assert(strcmp(status.network_ssid, "Lab WiFi") == 0);
    assert(cp0_provision_test_parse_wifi(wifi_response, strlen(wifi_response),
                                         8, &wifi, error) == CP0_PROVISION_OK);
    assert(wifi.count == 2 && wifi.networks[0].signal_percent == 91);
    assert(wifi.networks[0].security == CP0_PROVISION_WIFI_WPA3);
    assert(wifi.networks[1].connected);
    assert(cp0_provision_test_parse_state(
               error_response, strlen(error_response), 9, &status, error) ==
           CP0_PROVISION_REPAIR_REQUIRED);
    assert(strcmp(error, "Recovery is required") == 0);
    assert(cp0_provision_test_parse_wifi(
               invalid_signal, strlen(invalid_signal), 10, &wifi, error) ==
           CP0_PROVISION_FAILED);
    assert(cp0_provision_test_parse_state(
               state_response, strlen(state_response), 6, &status, error) ==
           CP0_PROVISION_FAILED);
    assert(cp0_provision_test_escape("a\"b\\c\n", escaped,
                                     sizeof(escaped)));
    assert(strcmp(escaped, "\"a\\\"b\\\\c\\u000a\"") == 0);
    return 0;
}
