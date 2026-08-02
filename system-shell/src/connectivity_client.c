#define _POSIX_C_SOURCE 200809L

#include "cp0_connectivity_client.h"
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

#ifndef CP0_CONNECTIVITY_SOCKET
#define CP0_CONNECTIVITY_SOCKET \
    "/run/cardputerzero-connectivityd/connectivity.sock"
#endif

#define CP0_CONNECTIVITY_FRAME_BYTES 2048U
#define CP0_CONNECTIVITY_JSON_TOKENS 64U

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
    const struct timeval timeout = {
        .tv_sec = 1,
        .tv_usec = 0,
    };
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    socklen_t address_length =
        (socklen_t)(offsetof(struct sockaddr_un, sun_path) +
                    strlen(CP0_CONNECTIVITY_SOCKET) + 1U);
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
        strlen(CP0_CONNECTIVITY_SOCKET) >= sizeof(address.sun_path))
        goto cleanup;
    memcpy(address.sun_path, CP0_CONNECTIVITY_SOCKET,
           strlen(CP0_CONNECTIVITY_SOCKET) + 1U);
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

static int parse_state_response(const char *document, size_t length,
                                uint64_t request_id,
                                struct cp0_connectivity_state *state)
{
    struct cp0_json_token tokens[CP0_CONNECTIVITY_JSON_TOKENS];
    int count = cp0_json_parse(document, length, tokens,
                               CP0_CONNECTIVITY_JSON_TOKENS);
    uint64_t version;
    uint64_t parsed_id;
    int version_token;
    int id_token;
    int outcome;
    int status;

    if (document == NULL || state == NULL || count <= 0 ||
        tokens[0].type != CP0_JSON_OBJECT)
        return CP0_CONNECTIVITY_FAILED;
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
        return CP0_CONNECTIVITY_FAILED;
    status = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                 "status");
    if (status < 0 || tokens[status].type != CP0_JSON_STRING)
        return CP0_CONNECTIVITY_FAILED;
    if (cp0_json_string_equals(document, &tokens[status], "error")) {
        int code = cp0_json_object_get(document, tokens, (size_t)count,
                                       outcome, "code");
        if (code >= 0 && cp0_json_string_equals(document, &tokens[code],
                                                 "unavailable"))
            return CP0_CONNECTIVITY_UNAVAILABLE;
        return CP0_CONNECTIVITY_FAILED;
    }
    if (!cp0_json_string_equals(document, &tokens[status], "state"))
        return CP0_CONNECTIVITY_FAILED;

    int state_token = cp0_json_object_get(document, tokens, (size_t)count,
                                          outcome, "state");
    if (state_token < 0 || tokens[state_token].type != CP0_JSON_OBJECT)
        return CP0_CONNECTIVITY_FAILED;
    int available_token = cp0_json_object_get(
        document, tokens, (size_t)count, state_token, "available");
    int wifi_available_token = cp0_json_object_get(
        document, tokens, (size_t)count, state_token, "wifi_available");
    int wifi_enabled_token = cp0_json_object_get(
        document, tokens, (size_t)count, state_token, "wifi_enabled");
    int airplane_token = cp0_json_object_get(
        document, tokens, (size_t)count, state_token, "airplane_mode");
    bool available;
    bool wifi_available;
    bool wifi_enabled;
    bool airplane_mode;
    if (available_token < 0 || wifi_available_token < 0 ||
        wifi_enabled_token < 0 || airplane_token < 0 ||
        !cp0_json_get_bool(document, &tokens[available_token], &available) ||
        !cp0_json_get_bool(document, &tokens[wifi_available_token],
                           &wifi_available) ||
        !cp0_json_get_bool(document, &tokens[wifi_enabled_token],
                           &wifi_enabled) ||
        !cp0_json_get_bool(document, &tokens[airplane_token],
                           &airplane_mode) ||
        (!available && (wifi_available || wifi_enabled || airplane_mode)) ||
        (!wifi_available && wifi_enabled) ||
        (airplane_mode && wifi_enabled))
        return CP0_CONNECTIVITY_FAILED;
    *state = (struct cp0_connectivity_state){
        .available = available,
        .wifi_available = wifi_available,
        .wifi_enabled = wifi_enabled,
        .airplane_mode = airplane_mode,
    };
    return available ? CP0_CONNECTIVITY_OK : CP0_CONNECTIVITY_UNAVAILABLE;
}

static int command(const char *name, const char *member,
                   struct cp0_connectivity_state *state)
{
    char request[256];
    char response[CP0_CONNECTIVITY_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int count;

    if (name == NULL || state == NULL)
        return CP0_CONNECTIVITY_FAILED;
    count = snprintf(request, sizeof(request),
                     "{\"protocol_version\":1,\"request_id\":%llu,"
                     "\"command\":{\"name\":\"%s\"%s}}\n",
                     (unsigned long long)request_id, name,
                     member == NULL ? "" : member);
    if (count <= 0 || (size_t)count >= sizeof(request) ||
        exchange(request, (size_t)count, response, sizeof(response),
                 &response_length) != 0)
        return CP0_CONNECTIVITY_FAILED;
    return parse_state_response(response, response_length, request_id, state);
}

int cp0_connectivity_get_state(struct cp0_connectivity_state *state)
{
    return command("get-state", NULL, state);
}

int cp0_connectivity_set_wifi_enabled(
    bool enabled, struct cp0_connectivity_state *state)
{
    return command("set-wifi-enabled",
                   enabled ? ",\"enabled\":true" : ",\"enabled\":false",
                   state);
}

int cp0_connectivity_set_airplane_mode(
    bool enabled, struct cp0_connectivity_state *state)
{
    return command("set-airplane-mode",
                   enabled ? ",\"enabled\":true" : ",\"enabled\":false",
                   state);
}

#ifdef CP0_CONNECTIVITY_CLIENT_TEST
int cp0_connectivity_test_parse_state_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_connectivity_state *state)
{
    return parse_state_response(response, response_length, request_id, state);
}
#endif
