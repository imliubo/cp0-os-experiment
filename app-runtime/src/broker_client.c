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
#define CP0_BROKER_REQUEST_BYTES 4096U
#define CP0_BROKER_RESPONSE_BYTES 4096U
#define CP0_NOTIFICATION_TITLE_BYTES 128U
#define CP0_NOTIFICATION_BODY_BYTES 640U
#define CP0_NETWORK_URL_BYTES 1024U
#define CP0_NETWORK_BODY_BYTES 2048U
#define CP0_DOCUMENT_ID_BYTES 32U
#define CP0_DOCUMENT_MAX_BYTES (16U * 1024U * 1024U)
#define CP0_AUDIO_MAX_FRAMES 1024U
#define CP0_AUDIO_MAX_BYTES (CP0_AUDIO_MAX_FRAMES * 2U)
#define CP0_CAMERA_WIDTH 320U
#define CP0_CAMERA_HEIGHT 170U
#define CP0_CAMERA_FRAME_BYTES (CP0_CAMERA_WIDTH * CP0_CAMERA_HEIGHT * 2U)
#define CP0_LORA_MAX_PAYLOAD_BYTES 64U
#define CP0_LORA_METADATA_BYTES 4U

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

static bool append_base64(char *output, size_t capacity, size_t *offset,
                          const uint8_t *input, size_t length) {
    static const char alphabet[] =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    for (size_t index = 0; index < length; index += 3U) {
        size_t remaining = length - index;
        uint8_t first = input[index];
        uint8_t second = remaining > 1U ? input[index + 1U] : 0U;
        uint8_t third = remaining > 2U ? input[index + 2U] : 0U;
        char encoded[4] = {
            alphabet[first >> 2],
            alphabet[((first & 0x03U) << 4) | (second >> 4)],
            remaining > 1U
                ? alphabet[((second & 0x0fU) << 2) | (third >> 6)]
                : '=',
            remaining > 2U ? alphabet[third & 0x3fU] : '=',
        };
        if (!append_bytes(output, capacity, offset, encoded, sizeof(encoded)))
            return false;
    }
    return true;
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
    if (strstr(response, "\"status\":\"permission-pending\"") != NULL ||
        strstr(response, "\"status\":\"document-selection-pending\"") != NULL)
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
        strstr(response, "\"code\":\"unavailable\"") != NULL ||
        strstr(response, "\"code\":\"timeout\"") != NULL ||
        strstr(response, "\"code\":\"tls\"") != NULL ||
        strstr(response, "\"code\":\"too-many-redirects\"") != NULL)
        return CP0_BROKER_UNAVAILABLE;
    return CP0_BROKER_INTERNAL;
}

static int32_t broker_exchange_with_fd(const char *request,
                                       size_t request_length, char *response,
                                       size_t response_capacity,
                                       int *received_descriptor) {
    struct sockaddr_un address;
    const struct timeval timeout = {.tv_sec = 6, .tv_usec = 0};
    const char *socket_path = getenv("CP0_BROKER_SOCKET");
    size_t response_length = 0;
    int descriptor = -1;
    int32_t result = CP0_BROKER_INTERNAL;

    if (received_descriptor != NULL)
        *received_descriptor = -1;

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
        union {
            struct cmsghdr alignment;
            unsigned char bytes[CMSG_SPACE(sizeof(int))];
        } control = {0};
        struct iovec io_vector = {
            .iov_base = response + response_length,
            .iov_len = response_capacity - 1U - response_length,
        };
        struct msghdr message = {
            .msg_iov = &io_vector,
            .msg_iovlen = 1,
            .msg_control = control.bytes,
            .msg_controllen = sizeof(control.bytes),
        };
        ssize_t count = recvmsg(descriptor, &message, 0);
        if (count < 0 && errno == EINTR)
            continue;
        if (count < 0)
            goto cleanup;
        if (count == 0)
            break;
        for (struct cmsghdr *header = CMSG_FIRSTHDR(&message); header != NULL;
             header = CMSG_NXTHDR(&message, header)) {
            int passed_descriptor;
            size_t data_bytes;
            if (header->cmsg_level != SOL_SOCKET ||
                header->cmsg_type != SCM_RIGHTS ||
                header->cmsg_len < CMSG_LEN(0))
                goto cleanup;
            data_bytes = header->cmsg_len - CMSG_LEN(0);
            if (data_bytes != sizeof(int)) {
                size_t descriptor_count = data_bytes / sizeof(int);
                for (size_t index = 0; index < descriptor_count; index++) {
                    memcpy(&passed_descriptor,
                           CMSG_DATA(header) + index * sizeof(int), sizeof(int));
                    if (passed_descriptor >= 0)
                        close(passed_descriptor);
                }
                goto cleanup;
            }
            memcpy(&passed_descriptor, CMSG_DATA(header), sizeof(int));
            if (passed_descriptor < 0 || received_descriptor == NULL ||
                *received_descriptor >= 0 ||
                fcntl(passed_descriptor, F_SETFD, FD_CLOEXEC) != 0) {
                if (passed_descriptor >= 0)
                    close(passed_descriptor);
                goto cleanup;
            }
            *received_descriptor = passed_descriptor;
        }
        if ((message.msg_flags & (MSG_CTRUNC | MSG_TRUNC)) != 0)
            goto cleanup;
        response_length += (size_t)count;
        if (memchr(response, '\n', response_length) != NULL)
            break;
    }
    response[response_length] = '\0';
    if (response_length == 0U || response[response_length - 1U] != '\n')
        goto cleanup;
    result = CP0_BROKER_OK;

cleanup:
    if (result != CP0_BROKER_OK && received_descriptor != NULL &&
        *received_descriptor >= 0) {
        close(*received_descriptor);
        *received_descriptor = -1;
    }
    close(descriptor);
    return result;
}

static int32_t broker_exchange(const char *request, size_t request_length,
                               char *response, size_t response_capacity) {
    return broker_exchange_with_fd(request, request_length, response,
                                   response_capacity, NULL);
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

static bool json_u32_field(const char *response, const char *field,
                           uint32_t *value) {
    char key[64];
    const char *start;
    char *end;
    unsigned long long parsed;
    int written;

    written = snprintf(key, sizeof(key), "\"%s\":", field);
    if (written <= 0 || (size_t)written >= sizeof(key))
        return false;
    start = strstr(response, key);
    if (start == NULL)
        return false;
    start += (size_t)written;
    errno = 0;
    parsed = strtoull(start, &end, 10);
    if (errno != 0 || end == start || parsed > UINT32_MAX ||
        (*end != ',' && *end != '}'))
        return false;
    *value = (uint32_t)parsed;
    return true;
}

static bool json_signed_field(const char *response, const char *field,
                              long minimum, long maximum, long *value) {
    char key[64];
    const char *start;
    char *end;
    long parsed;
    int written;

    written = snprintf(key, sizeof(key), "\"%s\":", field);
    if (written <= 0 || (size_t)written >= sizeof(key))
        return false;
    start = strstr(response, key);
    if (start == NULL)
        return false;
    start += (size_t)written;
    errno = 0;
    parsed = strtol(start, &end, 10);
    if (errno != 0 || end == start || parsed < minimum || parsed > maximum ||
        (*end != ',' && *end != '}'))
        return false;
    *value = parsed;
    return true;
}

static bool json_bool_field(const char *response, const char *field,
                            bool *value) {
    char key[64];
    const char *start;
    const char *end;
    int written;

    written = snprintf(key, sizeof(key), "\"%s\":", field);
    if (written <= 0 || (size_t)written >= sizeof(key))
        return false;
    start = strstr(response, key);
    if (start == NULL)
        return false;
    start += (size_t)written;
    if (strncmp(start, "true", 4U) == 0) {
        *value = true;
        end = start + 4U;
    } else if (strncmp(start, "false", 5U) == 0) {
        *value = false;
        end = start + 5U;
    } else {
        return false;
    }
    return *end == ',' || *end == '}';
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

int32_t cp0_broker_decode_document_response(const char *response,
                                            int received_descriptor,
                                            int *descriptor,
                                            uint32_t *size_bytes) {
    const char *document_id;
    size_t document_id_length;
    uint32_t decoded_size;

    if (response == NULL || descriptor == NULL || size_bytes == NULL) {
        if (received_descriptor >= 0)
            close(received_descriptor);
        return CP0_BROKER_INVALID_ARGUMENT;
    }
    *descriptor = -1;
    *size_bytes = 0;
    if (strstr(response, "\"status\":\"document-opened\"") == NULL) {
        if (received_descriptor >= 0)
            close(received_descriptor);
        return decode_result(response);
    }
    document_id = json_string_field(response, "document_id",
                                    &document_id_length);
    if (received_descriptor < 0 || document_id == NULL ||
        document_id_length != CP0_DOCUMENT_ID_BYTES ||
        !json_u32_field(response, "size_bytes", &decoded_size) ||
        decoded_size > CP0_DOCUMENT_MAX_BYTES) {
        if (received_descriptor >= 0)
            close(received_descriptor);
        return CP0_BROKER_INTERNAL;
    }
    for (size_t index = 0; index < document_id_length; index++) {
        char byte = document_id[index];
        if (!((byte >= '0' && byte <= '9') ||
              (byte >= 'a' && byte <= 'f'))) {
            close(received_descriptor);
            return CP0_BROKER_INTERNAL;
        }
    }
    *descriptor = received_descriptor;
    *size_bytes = decoded_size;
    return CP0_BROKER_OK;
}

int32_t cp0_broker_open_document(int *descriptor, uint32_t *size_bytes) {
    static const char request[] =
        "{\"protocol_version\":1,\"request_id\":3,\"command\":{"
        "\"name\":\"open-document\"}}\n";
    char response[CP0_BROKER_RESPONSE_BYTES];
    int received_descriptor = -1;
    int32_t result;

    if (descriptor == NULL || size_bytes == NULL)
        return CP0_BROKER_INVALID_ARGUMENT;
    *descriptor = -1;
    *size_bytes = 0;
    result = broker_exchange_with_fd(request, sizeof(request) - 1U, response,
                                     sizeof(response), &received_descriptor);
    if (result != CP0_BROKER_OK)
        return result;
    return cp0_broker_decode_document_response(
        response, received_descriptor, descriptor, size_bytes);
}

int32_t cp0_broker_play_audio(const uint8_t *samples, size_t sample_bytes) {
    static const char prefix[] =
        "{\"protocol_version\":1,\"request_id\":4,\"command\":{"
        "\"name\":\"play-audio\",\"samples_base64\":\"";
    static const char suffix[] = "\"}}\n";
    char request[CP0_BROKER_REQUEST_BYTES];
    char response[CP0_BROKER_RESPONSE_BYTES];
    size_t offset = 0;
    uint16_t frames;
    int32_t result;

    if (samples == NULL || sample_bytes == 0U ||
        sample_bytes > CP0_AUDIO_MAX_BYTES || sample_bytes % 2U != 0U)
        return CP0_BROKER_INVALID_ARGUMENT;
    if (!append_bytes(request, sizeof(request), &offset, prefix,
                      sizeof(prefix) - 1U) ||
        !append_base64(request, sizeof(request), &offset, samples, sample_bytes) ||
        !append_bytes(request, sizeof(request), &offset, suffix,
                      sizeof(suffix) - 1U))
        return CP0_BROKER_INVALID_ARGUMENT;
    result = broker_exchange(request, offset, response, sizeof(response));
    if (result != CP0_BROKER_OK)
        return result;
    if (strstr(response, "\"status\":\"audio-played\"") == NULL)
        return decode_result(response);
    if (!json_u16_field(response, "frames", &frames) || frames == 0U ||
        (size_t)frames * 2U != sample_bytes)
        return CP0_BROKER_INTERNAL;
    return CP0_BROKER_OK;
}

int32_t cp0_broker_decode_audio_capture_response(const char *response,
                                                 uint8_t *samples,
                                                 size_t sample_capacity) {
    const char *encoded;
    size_t encoded_length;
    size_t sample_bytes;

    if (response == NULL || samples == NULL || sample_capacity == 0U ||
        sample_capacity > CP0_AUDIO_MAX_BYTES || sample_capacity % 2U != 0U)
        return CP0_BROKER_INVALID_ARGUMENT;
    if (strstr(response, "\"status\":\"audio-captured\"") == NULL)
        return decode_result(response);
    encoded = json_string_field(response, "samples_base64", &encoded_length);
    if (encoded == NULL ||
        !decode_base64(encoded, encoded_length, samples, sample_capacity,
                       &sample_bytes) ||
        sample_bytes != sample_capacity)
        return CP0_BROKER_INTERNAL;
    return (int32_t)sample_bytes;
}

int32_t cp0_broker_capture_audio(uint8_t *samples, size_t sample_capacity) {
    char request[192];
    char response[CP0_BROKER_RESPONSE_BYTES];
    int written;
    int32_t result;

    if (samples == NULL || sample_capacity == 0U ||
        sample_capacity > CP0_AUDIO_MAX_BYTES || sample_capacity % 2U != 0U)
        return CP0_BROKER_INVALID_ARGUMENT;
    written = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":5,\"command\":{"
        "\"name\":\"capture-audio\",\"frames\":%zu}}\n",
        sample_capacity / 2U);
    if (written <= 0 || (size_t)written >= sizeof(request))
        return CP0_BROKER_INTERNAL;
    result = broker_exchange(request, (size_t)written, response, sizeof(response));
    if (result != CP0_BROKER_OK)
        return result;
    return cp0_broker_decode_audio_capture_response(response, samples,
                                                    sample_capacity);
}

int32_t cp0_broker_decode_camera_response(const char *response,
                                          int received_descriptor,
                                          int *descriptor) {
    const char *pixel_format;
    size_t pixel_format_length;
    uint16_t width;
    uint16_t height;
    uint32_t size_bytes;

    if (response == NULL || descriptor == NULL) {
        if (received_descriptor >= 0)
            close(received_descriptor);
        return CP0_BROKER_INVALID_ARGUMENT;
    }
    *descriptor = -1;
    if (strstr(response, "\"status\":\"camera-captured\"") == NULL) {
        if (received_descriptor >= 0)
            close(received_descriptor);
        return decode_result(response);
    }
    pixel_format = json_string_field(response, "pixel_format",
                                     &pixel_format_length);
    if (received_descriptor < 0 ||
        !json_u16_field(response, "width", &width) ||
        !json_u16_field(response, "height", &height) ||
        !json_u32_field(response, "size_bytes", &size_bytes) ||
        width != CP0_CAMERA_WIDTH || height != CP0_CAMERA_HEIGHT ||
        size_bytes != CP0_CAMERA_FRAME_BYTES || pixel_format == NULL ||
        pixel_format_length != sizeof("rgb565-le") - 1U ||
        memcmp(pixel_format, "rgb565-le", sizeof("rgb565-le") - 1U) != 0) {
        if (received_descriptor >= 0)
            close(received_descriptor);
        return CP0_BROKER_INTERNAL;
    }
    *descriptor = received_descriptor;
    return CP0_BROKER_OK;
}

int32_t cp0_broker_capture_camera(int *descriptor) {
    static const char request[] =
        "{\"protocol_version\":1,\"request_id\":6,\"command\":{"
        "\"name\":\"capture-camera\"}}\n";
    char response[CP0_BROKER_RESPONSE_BYTES];
    int received_descriptor = -1;
    int32_t result;

    if (descriptor == NULL)
        return CP0_BROKER_INVALID_ARGUMENT;
    *descriptor = -1;
    result = broker_exchange_with_fd(request, sizeof(request) - 1U, response,
                                     sizeof(response), &received_descriptor);
    if (result != CP0_BROKER_OK)
        return result;
    return cp0_broker_decode_camera_response(response, received_descriptor,
                                             descriptor);
}

static const char *gpio_line_name(uint32_t line) {
    switch (line) {
    case 0:
        return "grove-function";
    case 1:
        return "external-usb-function";
    case 2:
        return "grove-5v-power";
    case 3:
        return "external-5v-power";
    default:
        return NULL;
    }
}

int32_t cp0_broker_decode_gpio_response(const char *response, uint32_t line,
                                        int written, uint32_t expected_value) {
    const char *line_name = gpio_line_name(line);
    const char *returned_line;
    size_t returned_line_length;
    bool value;

    if (response == NULL || line_name == NULL || expected_value > 1U)
        return CP0_BROKER_INVALID_ARGUMENT;
    if ((written != 0 &&
         strstr(response, "\"status\":\"gpio-written\"") == NULL) ||
        (written == 0 && strstr(response, "\"status\":\"gpio-value\"") == NULL)) {
        return decode_result(response);
    }
    returned_line = json_string_field(response, "line", &returned_line_length);
    if (returned_line == NULL || returned_line_length != strlen(line_name) ||
        memcmp(returned_line, line_name, returned_line_length) != 0 ||
        !json_bool_field(response, "value", &value))
        return CP0_BROKER_INTERNAL;
    if (written != 0 && value != (expected_value != 0U))
        return CP0_BROKER_INTERNAL;
    return written != 0 ? CP0_BROKER_OK : (value ? 1 : 0);
}

int32_t cp0_broker_gpio_read(uint32_t line) {
    const char *line_name = gpio_line_name(line);
    char request[192];
    char response[CP0_BROKER_RESPONSE_BYTES];
    int written;
    int32_t result;

    if (line_name == NULL)
        return CP0_BROKER_INVALID_ARGUMENT;
    written = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":7,\"command\":{"
        "\"name\":\"read-gpio\",\"line\":\"%s\"}}\n",
        line_name);
    if (written <= 0 || (size_t)written >= sizeof(request))
        return CP0_BROKER_INTERNAL;
    result = broker_exchange(request, (size_t)written, response, sizeof(response));
    if (result != CP0_BROKER_OK)
        return result;
    return cp0_broker_decode_gpio_response(response, line, 0, 0);
}

int32_t cp0_broker_gpio_write(uint32_t line, uint32_t value) {
    const char *line_name = gpio_line_name(line);
    char request[208];
    char response[CP0_BROKER_RESPONSE_BYTES];
    int written;
    int32_t result;

    if (line_name == NULL || value > 1U)
        return CP0_BROKER_INVALID_ARGUMENT;
    written = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":8,\"command\":{"
        "\"name\":\"write-gpio\",\"line\":\"%s\",\"value\":%s}}\n",
        line_name, value != 0U ? "true" : "false");
    if (written <= 0 || (size_t)written >= sizeof(request))
        return CP0_BROKER_INTERNAL;
    result = broker_exchange(request, (size_t)written, response, sizeof(response));
    if (result != CP0_BROKER_OK)
        return result;
    return cp0_broker_decode_gpio_response(response, line, 1, value);
}

int32_t cp0_broker_lora_send(const uint8_t *payload, size_t payload_length) {
    static const char prefix[] =
        "{\"protocol_version\":1,\"request_id\":9,\"command\":{"
        "\"name\":\"send-lora\",\"payload_base64\":\"";
    static const char suffix[] = "\"}}\n";
    char request[256];
    char response[CP0_BROKER_RESPONSE_BYTES];
    size_t offset = 0;
    uint16_t bytes;
    int32_t result;

    if (payload == NULL || payload_length == 0U ||
        payload_length > CP0_LORA_MAX_PAYLOAD_BYTES)
        return CP0_BROKER_INVALID_ARGUMENT;
    if (!append_bytes(request, sizeof(request), &offset, prefix,
                      sizeof(prefix) - 1U) ||
        !append_base64(request, sizeof(request), &offset, payload,
                       payload_length) ||
        !append_bytes(request, sizeof(request), &offset, suffix,
                      sizeof(suffix) - 1U))
        return CP0_BROKER_INTERNAL;
    result = broker_exchange(request, offset, response, sizeof(response));
    if (result != CP0_BROKER_OK)
        return result;
    if (strstr(response, "\"status\":\"lora-sent\"") == NULL)
        return decode_result(response);
    if (!json_u16_field(response, "bytes", &bytes) ||
        (size_t)bytes != payload_length)
        return CP0_BROKER_INTERNAL;
    return CP0_BROKER_OK;
}

int32_t cp0_broker_decode_lora_response(const char *response,
                                        uint8_t *payload,
                                        size_t payload_capacity,
                                        uint8_t *metadata,
                                        size_t metadata_bytes) {
    const char *encoded;
    size_t encoded_length;
    size_t payload_length;
    long rssi_dbm;
    long snr_quarter_db;
    uint16_t encoded_rssi;

    if (response == NULL || payload == NULL || payload_capacity == 0U ||
        payload_capacity > CP0_LORA_MAX_PAYLOAD_BYTES || metadata == NULL ||
        metadata_bytes != CP0_LORA_METADATA_BYTES)
        return CP0_BROKER_INVALID_ARGUMENT;
    memset(metadata, 0, metadata_bytes);
    if (strstr(response, "\"status\":\"lora-no-packet\"") != NULL)
        return 0;
    if (strstr(response, "\"status\":\"lora-packet\"") == NULL)
        return decode_result(response);
    encoded = json_string_field(response, "payload_base64", &encoded_length);
    if (encoded == NULL ||
        !decode_base64(encoded, encoded_length, payload, payload_capacity,
                       &payload_length) ||
        payload_length == 0U ||
        !json_signed_field(response, "rssi_dbm", -200, 50, &rssi_dbm) ||
        !json_signed_field(response, "snr_quarter_db", INT8_MIN, INT8_MAX,
                           &snr_quarter_db))
        return CP0_BROKER_INTERNAL;
    encoded_rssi = (uint16_t)(int16_t)rssi_dbm;
    metadata[0] = (uint8_t)encoded_rssi;
    metadata[1] = (uint8_t)(encoded_rssi >> 8);
    metadata[2] = (uint8_t)(int8_t)snr_quarter_db;
    metadata[3] = 0U;
    return (int32_t)payload_length;
}

int32_t cp0_broker_lora_receive(uint8_t *payload, size_t payload_capacity,
                                uint8_t *metadata, size_t metadata_bytes,
                                uint32_t timeout_ms) {
    char request[192];
    char response[CP0_BROKER_RESPONSE_BYTES];
    int written;
    int32_t result;

    if (payload == NULL || payload_capacity == 0U ||
        payload_capacity > CP0_LORA_MAX_PAYLOAD_BYTES || metadata == NULL ||
        metadata_bytes != CP0_LORA_METADATA_BYTES || timeout_ms == 0U ||
        timeout_ms > 1000U)
        return CP0_BROKER_INVALID_ARGUMENT;
    written = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":10,\"command\":{"
        "\"name\":\"receive-lora\",\"timeout_ms\":%u}}\n",
        timeout_ms);
    if (written <= 0 || (size_t)written >= sizeof(request))
        return CP0_BROKER_INTERNAL;
    result = broker_exchange(request, (size_t)written, response,
                             sizeof(response));
    if (result != CP0_BROKER_OK)
        return result;
    return cp0_broker_decode_lora_response(
        response, payload, payload_capacity, metadata, metadata_bytes);
}
