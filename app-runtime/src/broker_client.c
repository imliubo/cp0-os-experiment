#include "broker_client.h"

#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
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
#define CP0_NETWORK_URL_BYTES 1024U
#define CP0_NETWORK_BODY_BYTES 2048U

static bool append_bytes(char *output, size_t capacity, size_t *offset,
                         const char *value, size_t length) {
    if (*offset > capacity || length > capacity - *offset)
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

static int32_t decode_result(const char *response) {
    if (strstr(response, "\"status\":\"ok\"") != NULL)
        return CP0_BROKER_OK;
    if (strstr(response, "\"status\":\"permission-pending\"") != NULL)
        return CP0_BROKER_UNAVAILABLE;
    if (strstr(response, "\"code\":\"denied\"") != NULL ||
        strstr(response, "\"code\":\"undeclared\"") != NULL ||
        strstr(response, "\"code\":\"unauthorized\"") != NULL ||
        strstr(response, "\"code\":\"blocked-address\"") != NULL)
        return CP0_BROKER_DENIED;
    if (strstr(response, "\"code\":\"invalid-request\"") != NULL)
        return CP0_BROKER_INVALID_ARGUMENT;
    if (strstr(response, "\"code\":\"resource-exhausted\"") != NULL ||
        strstr(response, "\"code\":\"response-too-large\"") != NULL)
        return CP0_BROKER_RESOURCE_LIMIT;
    if (strstr(response, "\"code\":\"upstream-unavailable\"") != NULL ||
        strstr(response, "\"code\":\"timeout\"") != NULL ||
        strstr(response, "\"code\":\"tls\"") != NULL ||
        strstr(response, "\"code\":\"too-many-redirects\"") != NULL)
        return CP0_BROKER_UNAVAILABLE;
    return CP0_BROKER_INTERNAL;
}

static int32_t broker_exchange(const char *request, size_t request_length,
                               char *response, size_t response_capacity) {
    struct sockaddr_un address;
    const struct timeval timeout = {.tv_sec = 6, .tv_usec = 0};
    const char *socket_path = getenv("CP0_BROKER_SOCKET");
    size_t response_length = 0;
    int descriptor = -1;
    int32_t result = CP0_BROKER_INTERNAL;

    if (request == NULL || request_length == 0U || response == NULL ||
        response_capacity < 2U)
        return CP0_BROKER_INVALID_ARGUMENT;
    if (socket_path == NULL || socket_path[0] == '\0')
        socket_path = CP0_BROKER_SOCKET;
    if (strlen(socket_path) >= sizeof(address.sun_path))
        return CP0_BROKER_UNAVAILABLE;

    descriptor = socket(AF_UNIX, SOCK_STREAM
#ifdef SOCK_CLOEXEC
                                     | SOCK_CLOEXEC
#endif
                        ,
                        0);
    if (descriptor < 0)
        return CP0_BROKER_UNAVAILABLE;
#ifndef SOCK_CLOEXEC
    if (fcntl(descriptor, F_SETFD, FD_CLOEXEC) != 0)
        goto cleanup;
#endif
    if (setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout,
                   sizeof(timeout)) != 0)
        goto cleanup;
    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    memcpy(address.sun_path, socket_path, strlen(socket_path) + 1U);
    if (connect(descriptor, (const struct sockaddr *)&address,
                sizeof(address)) != 0 ||
        write_all(descriptor, request, request_length) != 0)
        goto cleanup;

    while (response_length < response_capacity - 1U) {
        ssize_t count = read(descriptor, response + response_length,
                             response_capacity - 1U - response_length);
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
    result = CP0_BROKER_OK;

cleanup:
    close(descriptor);
    return result;
}

static const char *json_string_field(const char *response, const char *field,
                                     size_t *length) {
    char key[64];
    const char *start;
    const char *end;
    int written;

    written = snprintf(key, sizeof(key), "\"%s\":\"", field);
    if (written <= 0 || (size_t)written >= sizeof(key))
        return NULL;
    start = strstr(response, key);
    if (start == NULL)
        return NULL;
    start += (size_t)written;
    end = strchr(start, '"');
    if (end == NULL)
        return NULL;
    *length = (size_t)(end - start);
    return start;
}

static bool json_u16_field(const char *response, const char *field,
                           uint16_t *value) {
    char key[64];
    const char *start;
    char *end;
    unsigned long parsed;
    int written;

    written = snprintf(key, sizeof(key), "\"%s\":", field);
    if (written <= 0 || (size_t)written >= sizeof(key))
        return false;
    start = strstr(response, key);
    if (start == NULL)
        return false;
    start += (size_t)written;
    errno = 0;
    parsed = strtoul(start, &end, 10);
    if (errno != 0 || end == start || parsed > UINT16_MAX ||
        (*end != ',' && *end != '}'))
        return false;
    *value = (uint16_t)parsed;
    return true;
}

static int decode_base64_digit(char byte) {
    if (byte >= 'A' && byte <= 'Z')
        return byte - 'A';
    if (byte >= 'a' && byte <= 'z')
        return byte - 'a' + 26;
    if (byte >= '0' && byte <= '9')
        return byte - '0' + 52;
    if (byte == '+')
        return 62;
    if (byte == '/')
        return 63;
    return -1;
}

static bool decode_base64(const char *encoded, size_t encoded_length,
                          uint8_t *output, size_t output_capacity,
                          size_t *output_length) {
    size_t input_offset;
    size_t offset = 0;

    if (encoded_length % 4U != 0U ||
        encoded_length > ((CP0_NETWORK_BODY_BYTES + 2U) / 3U) * 4U)
        return false;
    for (input_offset = 0; input_offset < encoded_length; input_offset += 4U) {
        const bool last = input_offset + 4U == encoded_length;
        const bool c_padding = encoded[input_offset + 2U] == '=';
        const bool d_padding = encoded[input_offset + 3U] == '=';
        int a = decode_base64_digit(encoded[input_offset]);
        int b = decode_base64_digit(encoded[input_offset + 1U]);
        int c = c_padding ? 0 : decode_base64_digit(encoded[input_offset + 2U]);
        int d = d_padding ? 0 : decode_base64_digit(encoded[input_offset + 3U]);

        if (a < 0 || b < 0 || c < 0 || d < 0 || (!last && (c_padding || d_padding)) ||
            (c_padding && (!d_padding || (b & 0x0f) != 0)) ||
            (!c_padding && d_padding && (c & 0x03) != 0) ||
            (c_padding && !d_padding))
            return false;
        if (offset >= output_capacity)
            return false;
        output[offset++] = (uint8_t)((a << 2) | (b >> 4));
        if (!c_padding) {
            if (offset >= output_capacity)
                return false;
            output[offset++] = (uint8_t)((b << 4) | (c >> 2));
            if (!d_padding) {
                if (offset >= output_capacity)
                    return false;
                output[offset++] = (uint8_t)((c << 6) | d);
            }
        }
    }
    *output_length = offset;
    return true;
}

int64_t cp0_broker_decode_http_response(const char *response, uint8_t *body,
                                        size_t body_capacity) {
    const char *encoded;
    size_t encoded_length;
    size_t body_length;
    uint16_t status_code;

    if (response == NULL || body == NULL || body_capacity == 0U ||
        body_capacity > CP0_NETWORK_BODY_BYTES)
        return CP0_BROKER_INVALID_ARGUMENT;
    if (strstr(response, "\"status\":\"http-response\"") == NULL)
        return decode_result(response);
    if (!json_u16_field(response, "status_code", &status_code) ||
        status_code < 100U || status_code > 599U)
        return CP0_BROKER_INTERNAL;
    encoded = json_string_field(response, "body_base64", &encoded_length);
    if (encoded == NULL ||
        !decode_base64(encoded, encoded_length, body, body_capacity,
                       &body_length))
        return CP0_BROKER_RESOURCE_LIMIT;
    return ((int64_t)status_code << 32) | (int64_t)body_length;
}

int32_t cp0_broker_post_notification(const uint8_t *title, size_t title_length,
                                     const uint8_t *body, size_t body_length) {
    static const char prefix[] =
        "{\"protocol_version\":1,\"request_id\":1,\"command\":{"
        "\"name\":\"post-notification\",\"title\":";
    static const char between[] = ",\"body\":";
    static const char suffix[] = "}}\n";
    char request[CP0_BROKER_REQUEST_BYTES];
    char response[CP0_BROKER_RESPONSE_BYTES];
    size_t offset = 0;
    int32_t result;

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
    result = broker_exchange(request, offset, response, sizeof(response));
    return result == CP0_BROKER_OK ? decode_result(response) : result;
}

int64_t cp0_broker_http_get(const uint8_t *url, size_t url_length,
                            uint8_t *body, size_t body_capacity) {
    static const char prefix[] =
        "{\"protocol_version\":1,\"request_id\":2,\"command\":{"
        "\"name\":\"http-get\",\"url\":";
    static const char suffix[] = "}}\n";
    char request[CP0_BROKER_REQUEST_BYTES];
    char response[CP0_BROKER_RESPONSE_BYTES];
    size_t offset = 0;
    int32_t result;

    if (url == NULL || url_length <= sizeof("https://") - 1U ||
        url_length > CP0_NETWORK_URL_BYTES || body == NULL ||
        body_capacity == 0U || body_capacity > CP0_NETWORK_BODY_BYTES ||
        memcmp(url, "https://", sizeof("https://") - 1U) != 0)
        return CP0_BROKER_INVALID_ARGUMENT;
    if (!append_bytes(request, sizeof(request), &offset, prefix,
                      sizeof(prefix) - 1U) ||
        !append_json_string(request, sizeof(request), &offset, url, url_length) ||
        !append_bytes(request, sizeof(request), &offset, suffix,
                      sizeof(suffix) - 1U))
        return CP0_BROKER_INVALID_ARGUMENT;
    result = broker_exchange(request, offset, response, sizeof(response));
    if (result != CP0_BROKER_OK)
        return result;
    return cp0_broker_decode_http_response(response, body, body_capacity);
}
