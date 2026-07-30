#include "broker_client.h"

#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>

#define CP0_BROKER_SOCKET "/run/cardputerzero/broker.sock"
#define CP0_BROKER_REQUEST_BYTES 2048U
#define CP0_BROKER_RESPONSE_BYTES 4096U
#define CP0_NOTIFICATION_TITLE_BYTES 128U
#define CP0_NOTIFICATION_BODY_BYTES 640U

static bool append_bytes(char *output, size_t capacity, size_t *offset,
                         const char *value, size_t length) {
    if (length > capacity - *offset)
        return false;
    memcpy(output + *offset, value, length);
    *offset += length;
    return true;
}

static bool append_json_string(char *output, size_t capacity, size_t *offset,
                               const uint8_t *value, size_t length) {
    size_t index;

    if (!append_bytes(output, capacity, offset, "\"", 1U))
        return false;
    for (index = 0; index < length; index++) {
        char byte = (char)value[index];
        if ((unsigned char)byte < 0x20U)
            return false;
        if (byte == '"' || byte == '\\') {
            if (!append_bytes(output, capacity, offset, "\\", 1U))
                return false;
        }
        if (!append_bytes(output, capacity, offset, &byte, 1U))
            return false;
    }
    return append_bytes(output, capacity, offset, "\"", 1U);
}

static int write_all(int descriptor, const char *buffer, size_t length) {
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

static int32_t decode_response(const char *response) {
    if (strstr(response, "\"status\":\"ok\"") != NULL)
        return CP0_BROKER_OK;
    if (strstr(response, "\"status\":\"permission-pending\"") != NULL)
        return CP0_BROKER_UNAVAILABLE;
    if (strstr(response, "\"code\":\"denied\"") != NULL ||
        strstr(response, "\"code\":\"undeclared\"") != NULL ||
        strstr(response, "\"code\":\"unauthorized\"") != NULL)
        return CP0_BROKER_DENIED;
    if (strstr(response, "\"code\":\"invalid-request\"") != NULL)
        return CP0_BROKER_INVALID_ARGUMENT;
    if (strstr(response, "\"code\":\"resource-exhausted\"") != NULL)
        return CP0_BROKER_RESOURCE_LIMIT;
    return CP0_BROKER_INTERNAL;
}

int32_t cp0_broker_post_notification(const uint8_t *title, size_t title_length,
                                     const uint8_t *body, size_t body_length) {
    static const char prefix[] =
        "{\"protocol_version\":1,\"request_id\":1,\"command\":{"
        "\"name\":\"post-notification\",\"title\":";
    static const char between[] = ",\"body\":";
    static const char suffix[] = "}}\n";
    struct sockaddr_un address;
    const struct timeval timeout = {.tv_sec = 1, .tv_usec = 0};
    char request[CP0_BROKER_REQUEST_BYTES];
    char response[CP0_BROKER_RESPONSE_BYTES];
    size_t offset = 0;
    size_t response_length = 0;
    int descriptor = -1;
    int32_t result = CP0_BROKER_INTERNAL;

    if (title == NULL || body == NULL || title_length == 0U ||
        title_length > CP0_NOTIFICATION_TITLE_BYTES ||
        body_length > CP0_NOTIFICATION_BODY_BYTES)
        return CP0_BROKER_INVALID_ARGUMENT;
    if (!append_bytes(request, sizeof(request), &offset, prefix,
                      sizeof(prefix) - 1U) ||
        !append_json_string(request, sizeof(request), &offset, title,
                            title_length) ||
        !append_bytes(request, sizeof(request), &offset, between,
                      sizeof(between) - 1U) ||
        !append_json_string(request, sizeof(request), &offset, body,
                            body_length) ||
        !append_bytes(request, sizeof(request), &offset, suffix,
                      sizeof(suffix) - 1U))
        return CP0_BROKER_INVALID_ARGUMENT;

    descriptor = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (descriptor < 0)
        return CP0_BROKER_UNAVAILABLE;
    if (setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout,
                   sizeof(timeout)) != 0)
        goto cleanup;
    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    if (strlen(CP0_BROKER_SOCKET) >= sizeof(address.sun_path))
        goto cleanup;
    memcpy(address.sun_path, CP0_BROKER_SOCKET, sizeof(CP0_BROKER_SOCKET));
    if (connect(descriptor, (const struct sockaddr *)&address,
                sizeof(address)) != 0 ||
        write_all(descriptor, request, offset) != 0)
        goto cleanup;

    while (response_length < sizeof(response) - 1U) {
        ssize_t count = read(descriptor, response + response_length,
                             sizeof(response) - 1U - response_length);
        if (count < 0 && errno == EINTR)
            continue;
        if (count < 0)
            goto cleanup;
        if (count == 0)
            break;
        response_length += (size_t)count;
        if (memchr(response, '\n', response_length) != NULL)
            break;
    }
    response[response_length] = '\0';
    if (response_length == 0U || response[response_length - 1U] != '\n')
        goto cleanup;
    result = decode_response(response);

cleanup:
    close(descriptor);
    return result;
}
