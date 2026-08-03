#define _GNU_SOURCE

#include "cp0_developer_client.h"
#include "cp0_json.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>

#ifndef CP0_DEVELOPER_SOCKET
#define CP0_DEVELOPER_SOCKET "/run/cardputerzero-devd/control.sock"
#endif
#define CP0_DEVELOPER_FRAME_BYTES 4096U
#define CP0_DEVELOPER_JSON_TOKENS 192U

static uint64_t next_request_id = 1;

static int write_all(int descriptor, const char *buffer, size_t length)
{
    size_t offset = 0;
    while (offset < length) {
        ssize_t count = write(descriptor, buffer + offset, length - offset);
        if (count < 0 && errno == EINTR)
            continue;
        if (count <= 0)
            return -1;
        offset += (size_t)count;
    }
    return 0;
}

static int exchange(const char *request, size_t request_length, char *response,
                    size_t response_capacity, size_t *response_length)
{
    const struct timeval timeout = {.tv_sec = 3, .tv_usec = 0};
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    socklen_t address_length =
        (socklen_t)(offsetof(struct sockaddr_un, sun_path) +
                    strlen(CP0_DEVELOPER_SOCKET) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    size_t length = 0;
    int result = -1;

    if (descriptor < 0 || request == NULL || response == NULL ||
        response_capacity < 2U)
        goto cleanup;
    if (fcntl(descriptor, F_SETFD, FD_CLOEXEC) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        strlen(CP0_DEVELOPER_SOCKET) >= sizeof(address.sun_path))
        goto cleanup;
    memcpy(address.sun_path, CP0_DEVELOPER_SOCKET,
           strlen(CP0_DEVELOPER_SOCKET) + 1U);
#ifdef __APPLE__
    address.sun_len = (uint8_t)address_length;
#endif
    if (connect(descriptor, (const struct sockaddr *)&address,
                address_length) != 0 ||
        write_all(descriptor, request, request_length) != 0)
        goto cleanup;

    while (length < response_capacity - 1U) {
        ssize_t count = read(descriptor, response + length,
                             response_capacity - 1U - length);
        if (count < 0 && errno == EINTR)
            continue;
        if (count <= 0)
            goto cleanup;
        length += (size_t)count;
        char *newline = memchr(response, '\n', length);
        if (newline != NULL) {
            if ((size_t)(newline - response) + 1U != length)
                goto cleanup;
            length--;
            response[length] = '\0';
            *response_length = length;
            result = 0;
            goto cleanup;
        }
    }

cleanup:
    if (descriptor >= 0)
        close(descriptor);
    return result;
}

static int parse_outcome(
    const char *document, size_t length, uint64_t request_id,
    const char *expected_status,
    struct cp0_json_token tokens[CP0_DEVELOPER_JSON_TOKENS],
    size_t *token_count, int *outcome)
{
    int count = cp0_json_parse(document, length, tokens,
                               CP0_DEVELOPER_JSON_TOKENS);
    uint64_t version;
    uint64_t response_id;
    if (count <= 0 || tokens[0].type != CP0_JSON_OBJECT)
        return -1;
    int version_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                            "protocol_version");
    int id_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                       "request_id");
    *outcome = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                   "outcome");
    if (version_token < 0 || id_token < 0 || *outcome < 0 ||
        !cp0_json_get_u64(document, &tokens[version_token], &version) ||
        !cp0_json_get_u64(document, &tokens[id_token], &response_id) ||
        version != 1 || response_id != request_id ||
        tokens[*outcome].type != CP0_JSON_OBJECT)
        return -1;
    int status = cp0_json_object_get(document, tokens, (size_t)count, *outcome,
                                     "status");
    if (status < 0 || !cp0_json_string_equals(document, &tokens[status],
                                              expected_status))
        return -1;
    *token_count = (size_t)count;
    return 0;
}

static bool copy_member(const char *document,
                        const struct cp0_json_token *tokens, size_t token_count,
                        int object, const char *key, char *output,
                        size_t output_capacity)
{
    int value = cp0_json_object_get(document, tokens, token_count, object, key);
    return value >= 0 && cp0_json_copy_string(document, &tokens[value], output,
                                               output_capacity);
}

static bool valid_fingerprint(const char *value)
{
    if (value == NULL || strlen(value) != 50U ||
        strncmp(value, "SHA256:", 7U) != 0)
        return false;
    for (size_t index = 7; index < 50U; index++) {
        unsigned char byte = (unsigned char)value[index];
        if (!((byte >= 'a' && byte <= 'z') ||
              (byte >= 'A' && byte <= 'Z') ||
              (byte >= '0' && byte <= '9') || byte == '+' || byte == '/'))
            return false;
    }
    return true;
}

static int parse_list_response(const char *response, size_t response_length,
                               uint64_t request_id,
                               struct cp0_developer_access *access)
{
    struct cp0_json_token tokens[CP0_DEVELOPER_JSON_TOKENS];
    struct cp0_developer_access decoded = {0};
    size_t token_count;
    int outcome;
    if (access == NULL ||
        parse_outcome(response, response_length, request_id, "paired-hosts",
                      tokens, &token_count, &outcome) != 0)
        return -1;
    int remaining = cp0_json_object_get(response, tokens, token_count, outcome,
                                        "pairing_remaining_seconds");
    int hosts = cp0_json_object_get(response, tokens, token_count, outcome,
                                    "hosts");
    if (remaining < 0 || hosts < 0 || tokens[hosts].type != CP0_JSON_ARRAY ||
        tokens[hosts].children > CP0_DEVELOPER_MAX_HOSTS)
        return -1;
    if (!cp0_json_is_null(response, &tokens[remaining])) {
        uint64_t parsed_remaining;
        if (!cp0_json_get_u64(response, &tokens[remaining], &parsed_remaining) ||
            parsed_remaining == 0 || parsed_remaining > 600U)
            return -1;
        decoded.pairing_remaining_seconds = (uint16_t)parsed_remaining;
        decoded.pairing_open = true;
    }
    decoded.host_count = tokens[hosts].children;
    for (size_t index = 0; index < decoded.host_count; index++) {
        int host = cp0_json_array_get(tokens, token_count, hosts,
                                      (unsigned int)index);
        int paired_at;
        if (host < 0 || tokens[host].type != CP0_JSON_OBJECT ||
            !copy_member(response, tokens, token_count, host, "label",
                         decoded.hosts[index].label,
                         sizeof(decoded.hosts[index].label)) ||
            !copy_member(response, tokens, token_count, host,
                         "ssh_fingerprint",
                         decoded.hosts[index].ssh_fingerprint,
                         sizeof(decoded.hosts[index].ssh_fingerprint)) ||
            decoded.hosts[index].label[0] == '\0' ||
            !valid_fingerprint(decoded.hosts[index].ssh_fingerprint))
            return -1;
        paired_at = cp0_json_object_get(response, tokens, token_count, host,
                                        "paired_at_unix_seconds");
        if (paired_at < 0 ||
            !cp0_json_get_u64(response, &tokens[paired_at],
                              &decoded.hosts[index].paired_at_unix_seconds))
            return -1;
    }
    *access = decoded;
    return 0;
}

int cp0_developer_list(struct cp0_developer_access *access)
{
    char request[192];
    char response[CP0_DEVELOPER_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"list-paired\"}}\n",
        (unsigned long long)request_id);
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length) != 0)
        return -1;
    return parse_list_response(response, response_length, request_id, access);
}

int cp0_developer_open_pairing(uint16_t duration_seconds,
                               uint16_t *remaining_seconds)
{
    char request[256];
    char response[CP0_DEVELOPER_FRAME_BYTES];
    struct cp0_json_token tokens[CP0_DEVELOPER_JSON_TOKENS];
    size_t response_length;
    size_t token_count;
    int outcome;
    uint64_t parsed_remaining;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"open-pairing\",\"duration_seconds\":%u}}\n",
        (unsigned long long)request_id, (unsigned int)duration_seconds);
    if (remaining_seconds == NULL || duration_seconds < 60U ||
        duration_seconds > 600U || request_length <= 0 ||
        (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length) != 0 ||
        parse_outcome(response, response_length, request_id, "pairing-window",
                      tokens, &token_count, &outcome) != 0)
        return -1;
    int remaining = cp0_json_object_get(response, tokens, token_count, outcome,
                                        "remaining_seconds");
    if (remaining < 0 ||
        !cp0_json_get_u64(response, &tokens[remaining], &parsed_remaining) ||
        parsed_remaining == 0 || parsed_remaining > duration_seconds)
        return -1;
    *remaining_seconds = (uint16_t)parsed_remaining;
    return 0;
}

static int unpair_command(const char *fingerprint, uint8_t *remaining)
{
    char request[320];
    char response[CP0_DEVELOPER_FRAME_BYTES];
    struct cp0_json_token tokens[CP0_DEVELOPER_JSON_TOKENS];
    size_t response_length;
    size_t token_count;
    int outcome;
    uint64_t request_id = next_request_id++;
    int request_length;
    uint64_t count;
    if (fingerprint == NULL) {
        request_length = snprintf(
            request, sizeof(request),
            "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
            "\"name\":\"unpair-all\"}}\n",
            (unsigned long long)request_id);
    } else {
        if (!valid_fingerprint(fingerprint))
            return -1;
        request_length = snprintf(
            request, sizeof(request),
            "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
            "\"name\":\"unpair\",\"host_fingerprint\":\"%s\"}}\n",
            (unsigned long long)request_id, fingerprint);
    }
    if (remaining == NULL || request_length <= 0 ||
        (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length) != 0 ||
        parse_outcome(response, response_length, request_id, "unpaired", tokens,
                      &token_count, &outcome) != 0)
        return -1;
    int paired = cp0_json_object_get(response, tokens, token_count, outcome,
                                     "paired_hosts");
    if (paired < 0 || !cp0_json_get_u64(response, &tokens[paired], &count) ||
        count > CP0_DEVELOPER_MAX_HOSTS)
        return -1;
    *remaining = (uint8_t)count;
    return 0;
}

int cp0_developer_unpair(const char *ssh_fingerprint, uint8_t *remaining)
{
    return unpair_command(ssh_fingerprint, remaining);
}

int cp0_developer_unpair_all(uint8_t *remaining)
{
    return unpair_command(NULL, remaining);
}

#ifdef CP0_DEVELOPER_CLIENT_TEST
int cp0_developer_test_parse_list_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_developer_access *access)
{
    return parse_list_response(response, response_length, request_id, access);
}
#endif
