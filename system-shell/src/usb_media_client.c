#define _POSIX_C_SOURCE 200809L

#include "cp0_usb_media_client.h"
#include "cp0_json.h"

#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>

#ifndef CP0_USB_MEDIA_SOCKET
#define CP0_USB_MEDIA_SOCKET "/run/cardputerzero-usb-mediad/media.sock"
#endif

#define CP0_USB_MEDIA_FRAME_BYTES 4096U
#define CP0_USB_MEDIA_JSON_TOKENS 64U
#define CP0_USB_MEDIA_TIMEOUT_SECONDS 180U

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
        .tv_sec = CP0_USB_MEDIA_TIMEOUT_SECONDS,
        .tv_usec = 0,
    };
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    socklen_t address_length =
        (socklen_t)(offsetof(struct sockaddr_un, sun_path) +
                    strlen(CP0_USB_MEDIA_SOCKET) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    size_t length = 0;
    int result = -1;

    if (descriptor < 0 || request == NULL || response == NULL ||
        response_capacity < 2U ||
        strlen(CP0_USB_MEDIA_SOCKET) >= sizeof(address.sun_path))
        goto cleanup;
    if (fcntl(descriptor, F_SETFD, FD_CLOEXEC) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout,
                   sizeof(timeout)) != 0)
        goto cleanup;
    memcpy(address.sun_path, CP0_USB_MEDIA_SOCKET,
           strlen(CP0_USB_MEDIA_SOCKET) + 1U);
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
        if (newline == NULL)
            continue;
        if ((size_t)(newline - response) + 1U != length)
            goto cleanup;
        length--;
        response[length] = '\0';
        *response_length = length;
        result = 0;
        break;
    }

cleanup:
    if (descriptor >= 0)
        close(descriptor);
    return result;
}

static int parse_response(const char *document, size_t length,
                          uint64_t request_id,
                          struct cp0_usb_media_status *state,
                          char error[CP0_USB_MEDIA_ERROR_MAX + 1])
{
    struct cp0_json_token tokens[CP0_USB_MEDIA_JSON_TOKENS];
    struct cp0_usb_media_status parsed = {0};
    int count = cp0_json_parse(document, length, tokens,
                               CP0_USB_MEDIA_JSON_TOKENS);
    uint64_t version;
    uint64_t parsed_id;
    uint64_t exported;
    uint64_t imported;
    uint64_t rejected;
    uint64_t capacity;
    int version_token;
    int id_token;
    int outcome;
    int status;

    if (error != NULL)
        error[0] = '\0';
    if (document == NULL || state == NULL || count <= 0 ||
        tokens[0].type != CP0_JSON_OBJECT)
        return CP0_USB_MEDIA_FAILED;
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
        return CP0_USB_MEDIA_FAILED;
    status = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                 "status");
    if (status < 0 || tokens[status].type != CP0_JSON_STRING)
        return CP0_USB_MEDIA_FAILED;
    if (cp0_json_string_equals(document, &tokens[status], "error")) {
        int code = cp0_json_object_get(document, tokens, (size_t)count,
                                       outcome, "code");
        int message = cp0_json_object_get(document, tokens, (size_t)count,
                                          outcome, "message");
        if (message < 0 || error == NULL ||
            !cp0_json_copy_string(document, &tokens[message], error,
                                  CP0_USB_MEDIA_ERROR_MAX + 1U))
            return CP0_USB_MEDIA_FAILED;
        if (code >= 0 && cp0_json_string_equals(document, &tokens[code],
                                                 "unavailable"))
            return CP0_USB_MEDIA_UNAVAILABLE;
        if (code >= 0 && cp0_json_string_equals(document, &tokens[code],
                                                 "invalid-state"))
            return CP0_USB_MEDIA_INVALID_STATE;
        return CP0_USB_MEDIA_FAILED;
    }
    if (!cp0_json_string_equals(document, &tokens[status], "state"))
        return CP0_USB_MEDIA_FAILED;

    int state_token = cp0_json_object_get(document, tokens, (size_t)count,
                                          outcome, "state");
    int phase_token;
    int exported_token;
    int imported_token;
    int rejected_token;
    int capacity_token;
    if (state_token < 0 || tokens[state_token].type != CP0_JSON_OBJECT ||
        (phase_token = cp0_json_object_get(document, tokens, (size_t)count,
                                           state_token, "state")) < 0 ||
        (exported_token = cp0_json_object_get(
             document, tokens, (size_t)count, state_token,
             "exported_photos")) < 0 ||
        (imported_token = cp0_json_object_get(
             document, tokens, (size_t)count, state_token,
             "imported_music")) < 0 ||
        (rejected_token = cp0_json_object_get(
             document, tokens, (size_t)count, state_token,
             "rejected_music")) < 0 ||
        (capacity_token = cp0_json_object_get(
             document, tokens, (size_t)count, state_token,
             "capacity_bytes")) < 0 ||
        !cp0_json_get_u64(document, &tokens[exported_token], &exported) ||
        !cp0_json_get_u64(document, &tokens[imported_token], &imported) ||
        !cp0_json_get_u64(document, &tokens[rejected_token], &rejected) ||
        !cp0_json_get_u64(document, &tokens[capacity_token], &capacity) ||
        exported > UINT32_MAX || imported > UINT32_MAX ||
        rejected > UINT32_MAX)
        return CP0_USB_MEDIA_FAILED;
    if (cp0_json_string_equals(document, &tokens[phase_token], "off"))
        parsed.state = CP0_USB_MEDIA_OFF;
    else if (cp0_json_string_equals(document, &tokens[phase_token],
                                    "preparing"))
        parsed.state = CP0_USB_MEDIA_PREPARING;
    else if (cp0_json_string_equals(document, &tokens[phase_token],
                                    "connected"))
        parsed.state = CP0_USB_MEDIA_CONNECTED;
    else if (cp0_json_string_equals(document, &tokens[phase_token],
                                    "importing"))
        parsed.state = CP0_USB_MEDIA_IMPORTING;
    else if (cp0_json_string_equals(document, &tokens[phase_token],
                                    "complete"))
        parsed.state = CP0_USB_MEDIA_COMPLETE;
    else if (cp0_json_string_equals(document, &tokens[phase_token], "error"))
        parsed.state = CP0_USB_MEDIA_ERROR;
    else
        return CP0_USB_MEDIA_FAILED;
    parsed.exported_photos = (uint32_t)exported;
    parsed.imported_music = (uint32_t)imported;
    parsed.rejected_music = (uint32_t)rejected;
    parsed.capacity_bytes = capacity;
    if ((parsed.state == CP0_USB_MEDIA_OFF &&
         (capacity != 0 || exported != 0 || imported != 0 || rejected != 0)) ||
        (parsed.state != CP0_USB_MEDIA_OFF && capacity == 0))
        return CP0_USB_MEDIA_FAILED;
    *state = parsed;
    return CP0_USB_MEDIA_OK;
}

static int command(const char *name, struct cp0_usb_media_status *status,
                   char error[CP0_USB_MEDIA_ERROR_MAX + 1])
{
    char request[256];
    char response[CP0_USB_MEDIA_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int count;

    if (name == NULL || status == NULL)
        return CP0_USB_MEDIA_FAILED;
    count = snprintf(request, sizeof(request),
                     "{\"protocol_version\":1,\"request_id\":%llu,"
                     "\"command\":{\"name\":\"%s\"}}\n",
                     (unsigned long long)request_id, name);
    if (count <= 0 || (size_t)count >= sizeof(request) ||
        exchange(request, (size_t)count, response, sizeof(response),
                 &response_length) != 0) {
        if (error != NULL)
            snprintf(error, CP0_USB_MEDIA_ERROR_MAX + 1U,
                     "USB media service is unavailable");
        return CP0_USB_MEDIA_UNAVAILABLE;
    }
    return parse_response(response, response_length, request_id, status,
                          error);
}

int cp0_usb_media_get_status(
    struct cp0_usb_media_status *status,
    char error[CP0_USB_MEDIA_ERROR_MAX + 1])
{
    return command("get-status", status, error);
}

int cp0_usb_media_start(
    struct cp0_usb_media_status *status,
    char error[CP0_USB_MEDIA_ERROR_MAX + 1])
{
    return command("start", status, error);
}

int cp0_usb_media_stop(
    struct cp0_usb_media_status *status,
    char error[CP0_USB_MEDIA_ERROR_MAX + 1])
{
    return command("stop", status, error);
}

#ifdef CP0_USB_MEDIA_CLIENT_TEST
int cp0_usb_media_test_parse(
    const char *response, size_t length, uint64_t request_id,
    struct cp0_usb_media_status *status,
    char error[CP0_USB_MEDIA_ERROR_MAX + 1])
{
    return parse_response(response, length, request_id, status, error);
}
#endif
