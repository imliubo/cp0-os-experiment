#ifndef CP0_DEVELOPER_CLIENT_H
#define CP0_DEVELOPER_CLIENT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define CP0_DEVELOPER_MAX_HOSTS 8
#define CP0_DEVELOPER_LABEL_BYTES 33
#define CP0_DEVELOPER_FINGERPRINT_BYTES 51

struct cp0_developer_host {
    uint64_t paired_at_unix_seconds;
    char label[CP0_DEVELOPER_LABEL_BYTES];
    char ssh_fingerprint[CP0_DEVELOPER_FINGERPRINT_BYTES];
};

struct cp0_developer_access {
    bool pairing_open;
    uint16_t pairing_remaining_seconds;
    size_t host_count;
    struct cp0_developer_host hosts[CP0_DEVELOPER_MAX_HOSTS];
};

int cp0_developer_list(struct cp0_developer_access *access);
int cp0_developer_open_pairing(uint16_t duration_seconds,
                               uint16_t *remaining_seconds);
int cp0_developer_unpair(const char *ssh_fingerprint, uint8_t *remaining);
int cp0_developer_unpair_all(uint8_t *remaining);

#ifdef CP0_DEVELOPER_CLIENT_TEST
int cp0_developer_test_parse_list_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_developer_access *access);
#endif

#endif
