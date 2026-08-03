#define _POSIX_C_SOURCE 200809L

#include "cp0_power_client.h"
#include "cp0_json.h"

#include <errno.h>
#include <fcntl.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>

#ifndef CP0_POWER_SOCKET
#define CP0_POWER_SOCKET "/run/cardputerzero-powerd/power.sock"
#endif

#define CP0_POWER_FRAME_BYTES 1024U
#define CP0_POWER_JSON_TOKENS 32U

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
                    strlen(CP0_POWER_SOCKET) + 1U);
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
        strlen(CP0_POWER_SOCKET) >= sizeof(address.sun_path))
        goto cleanup;
    memcpy(address.sun_path, CP0_POWER_SOCKET,
           strlen(CP0_POWER_SOCKET) + 1U);
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

static int parse_response(const char *document, size_t length,
                          uint64_t request_id, const char *expected_action)
{
    struct cp0_json_token tokens[CP0_POWER_JSON_TOKENS];
    int count =
        cp0_json_parse(document, length, tokens, CP0_POWER_JSON_TOKENS);
    uint64_t version;
    uint64_t parsed_id;
    int version_token;
    int id_token;
    int outcome;
    int status;
    int action;

    if (document == NULL || expected_action == NULL || count <= 0 ||
        tokens[0].type != CP0_JSON_OBJECT)
        return -1;
    version_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                        "protocol_version");
    id_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                   "request_id");
    outcome = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                  "outcome");
    if (version_token < 0 || id_token < 0 || outcome < 0 ||
        !cp0_json_get_u64(document, &tokens[version_token], &version) ||
        !cp0_json_get_u64(document, &tokens[id_token], &parsed_id) ||
        version != 1U || parsed_id != request_id ||
        tokens[outcome].type != CP0_JSON_OBJECT)
        return -1;
    status = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                 "status");
    action = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                 "action");
    if (status < 0 || action < 0 ||
        !cp0_json_string_equals(document, &tokens[status], "accepted") ||
        !cp0_json_string_equals(document, &tokens[action], expected_action))
        return -1;
    return 0;
}

int cp0_power_request(enum cp0_power_action action)
{
    const char *name;
    char request[192];
    char response[CP0_POWER_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length;

    if (action == CP0_POWER_RESTART)
        name = "restart";
    else if (action == CP0_POWER_OFF)
        name = "power-off";
    else
        return -1;
    request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,"
        "\"command\":{\"name\":\"%s\"}}\n",
        (unsigned long long)request_id, name);
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length) != 0)
        return -1;
    return parse_response(response, response_length, request_id, name);
}

#ifdef CP0_POWER_CLIENT_TEST
int cp0_power_test_parse_response(const char *response,
                                  size_t response_length,
                                  uint64_t request_id,
                                  enum cp0_power_action action)
{
    const char *name = action == CP0_POWER_RESTART
                           ? "restart"
                           : (action == CP0_POWER_OFF ? "power-off" : NULL);
    return parse_response(response, response_length, request_id, name);
}
#endif
