#include "cp0_developer_client.h"

#include <assert.h>
#include <stdio.h>
#include <string.h>

int main(void)
{
    static const char response[] =
        "{\"protocol_version\":1,\"request_id\":7,\"outcome\":{"
        "\"status\":\"paired-hosts\","
        "\"pairing_remaining_seconds\":599,"
        "\"hosts\":[{\"label\":\"workstation\","
        "\"ssh_fingerprint\":\"SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\","
        "\"developer_key_id\":\"0000000000000000000000000000000000000000000000000000000000000000\","
        "\"paired_at_unix_seconds\":42}]}}";
    struct cp0_developer_access access;
    assert(cp0_developer_test_parse_list_response(
               response, strlen(response), 7, &access) == 0);
    assert(access.pairing_open);
    assert(access.pairing_remaining_seconds == 599U);
    assert(access.host_count == 1);
    assert(strcmp(access.hosts[0].label, "workstation") == 0);
    assert(strcmp(access.hosts[0].ssh_fingerprint,
                  "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA") == 0);
    assert(access.hosts[0].paired_at_unix_seconds == 42);

    static const char closed[] =
        "{\"protocol_version\":1,\"request_id\":8,\"outcome\":{"
        "\"status\":\"paired-hosts\","
        "\"pairing_remaining_seconds\":null,\"hosts\":[]}}";
    assert(cp0_developer_test_parse_list_response(
               closed, strlen(closed), 8, &access) == 0);
    assert(!access.pairing_open && access.host_count == 0);

    assert(cp0_developer_test_parse_list_response(
               response, strlen(response), 8, &access) != 0);
    static const char malformed[] =
        "{\"protocol_version\":1,\"request_id\":9,\"outcome\":{"
        "\"status\":\"paired-hosts\","
        "\"pairing_remaining_seconds\":null,"
        "\"hosts\":[{\"label\":\"bad\","
        "\"ssh_fingerprint\":\"not-a-fingerprint\","
        "\"developer_key_id\":\"00\",\"paired_at_unix_seconds\":1}]}}";
    assert(cp0_developer_test_parse_list_response(
               malformed, strlen(malformed), 9, &access) != 0);
    puts("system shell developer client tests passed");
    return 0;
}
