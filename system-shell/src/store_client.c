#define _GNU_SOURCE

#include "cp0_store_client.h"
#include "cp0_json.h"

#include <errno.h>
#include <fcntl.h>
#include <png.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>

#ifndef CP0_STORE_SOCKET
#define CP0_STORE_SOCKET "/run/cardputerzero-store/control.sock"
#endif
#define CP0_STORE_FRAME_BYTES (64U * 1024U)
#define CP0_STORE_JSON_TOKENS 4096U
#define CP0_STORE_CATALOG_LIMIT 64U
#define CP0_STORE_SEARCH_QUERY_CHARS 32U
#define CP0_STORE_MAX_PACKAGE_BYTES (32U * 1024U * 1024U + 4096U)
#define CP0_STORE_MAX_ICON_BYTES (64U * 1024U)
#define CP0_STORE_MAX_SCREENSHOT_BYTES (512U * 1024U)

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

static void close_rights_descriptors(struct msghdr *message)
{
    for (struct cmsghdr *header = CMSG_FIRSTHDR(message); header != NULL;
         header = CMSG_NXTHDR(message, header)) {
        if (header->cmsg_level != SOL_SOCKET || header->cmsg_type != SCM_RIGHTS ||
            header->cmsg_len < CMSG_LEN(0))
            continue;
        size_t bytes = header->cmsg_len - CMSG_LEN(0);
        size_t count = bytes / sizeof(int);
        int *descriptors = (int *)CMSG_DATA(header);
        for (size_t index = 0; index < count; index++) {
            if (descriptors[index] >= 0)
                close(descriptors[index]);
        }
    }
}

static int receive_chunk(int socket_descriptor, char *buffer, size_t capacity,
                         int *received_descriptor)
{
    struct iovec vector = {.iov_base = buffer, .iov_len = capacity};
    union {
        struct cmsghdr alignment;
        unsigned char bytes[CMSG_SPACE(sizeof(int))];
    } control = {0};
    int candidate = -1;
    struct msghdr message = {
        .msg_iov = &vector,
        .msg_iovlen = 1,
        .msg_control = control.bytes,
        .msg_controllen = sizeof(control.bytes),
    };
    ssize_t count;

    do {
#ifdef MSG_CMSG_CLOEXEC
        count = recvmsg(socket_descriptor, &message, MSG_CMSG_CLOEXEC);
#else
        count = recvmsg(socket_descriptor, &message, 0);
#endif
    } while (count < 0 && errno == EINTR);
    if (count <= 0)
        return -1;
    if ((message.msg_flags & (MSG_CTRUNC | MSG_TRUNC)) != 0) {
        close_rights_descriptors(&message);
        return -1;
    }
    for (struct cmsghdr *header = CMSG_FIRSTHDR(&message); header != NULL;
         header = CMSG_NXTHDR(&message, header)) {
        if (header->cmsg_level != SOL_SOCKET || header->cmsg_type != SCM_RIGHTS ||
            header->cmsg_len != CMSG_LEN(sizeof(int)) ||
            received_descriptor == NULL || *received_descriptor >= 0 ||
            candidate >= 0) {
            close_rights_descriptors(&message);
            return -1;
        }
        memcpy(&candidate, CMSG_DATA(header), sizeof(candidate));
        if (candidate < 0 || fcntl(candidate, F_SETFD, FD_CLOEXEC) != 0) {
            if (candidate >= 0)
                close(candidate);
            return -1;
        }
    }
    if (candidate >= 0)
        *received_descriptor = candidate;
    return (int)count;
}

static int exchange(const char *request, size_t request_length, char *response,
                    size_t response_capacity, size_t *response_length,
                    unsigned int timeout_ms, int *received_descriptor)
{
    const struct timeval timeout = {
        .tv_sec = (time_t)(timeout_ms / 1000U),
        .tv_usec = (suseconds_t)((timeout_ms % 1000U) * 1000U),
    };
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    socklen_t address_length =
        (socklen_t)(offsetof(struct sockaddr_un, sun_path) +
                    strlen(CP0_STORE_SOCKET) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    size_t length = 0;
    int result = -1;

    if (received_descriptor != NULL)
        *received_descriptor = -1;

    if (descriptor < 0 || request == NULL || response == NULL ||
        response_capacity < 2 || strlen(CP0_STORE_SOCKET) >= sizeof(address.sun_path))
        goto cleanup;
    if (fcntl(descriptor, F_SETFD, FD_CLOEXEC) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout,
                   sizeof(timeout)) != 0)
        goto cleanup;
    memcpy(address.sun_path, CP0_STORE_SOCKET, strlen(CP0_STORE_SOCKET) + 1U);
#ifdef __APPLE__
    address.sun_len = (uint8_t)address_length;
#endif
    if (connect(descriptor, (const struct sockaddr *)&address,
                address_length) != 0 ||
        write_all(descriptor, request, request_length) != 0)
        goto cleanup;

    while (length < response_capacity - 1U) {
        int count = receive_chunk(descriptor, response + length,
                                  response_capacity - 1U - length,
                                  received_descriptor);
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
    if (result != 0 && received_descriptor != NULL &&
        *received_descriptor >= 0) {
        close(*received_descriptor);
        *received_descriptor = -1;
    }
    if (descriptor >= 0)
        close(descriptor);
    return result;
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

static bool valid_app_id(const char *app_id)
{
    unsigned int parts = 1;
    size_t part_length = 0;
    size_t length;

    if (app_id == NULL || (length = strlen(app_id)) == 0 || length > 128)
        return false;
    for (size_t index = 0; index < length; index++) {
        unsigned char byte = (unsigned char)app_id[index];
        if (byte == '.') {
            if (part_length == 0 || part_length > 32 || app_id[index - 1] == '-')
                return false;
            parts++;
            part_length = 0;
            continue;
        }
        if (part_length == 0 && (byte < 'a' || byte > 'z'))
            return false;
        if (!((byte >= 'a' && byte <= 'z') ||
              (byte >= '0' && byte <= '9') || byte == '-'))
            return false;
        part_length++;
    }
    return parts >= 3 && part_length > 0 && part_length <= 32 &&
           app_id[length - 1] != '-';
}

static bool valid_numeric_identifier(const char *value, size_t length)
{
    if (length == 0 || (length > 1 && value[0] == '0'))
        return false;
    for (size_t index = 0; index < length; index++) {
        if (value[index] < '0' || value[index] > '9')
            return false;
    }
    return true;
}

static bool valid_semver_identifiers(const char *value, size_t length,
                                     bool prerelease)
{
    size_t start = 0;

    if (length == 0)
        return false;
    for (size_t index = 0; index <= length; index++) {
        if (index < length && value[index] != '.') {
            unsigned char byte = (unsigned char)value[index];
            if (!((byte >= 'a' && byte <= 'z') ||
                  (byte >= 'A' && byte <= 'Z') ||
                  (byte >= '0' && byte <= '9') || byte == '-'))
                return false;
            continue;
        }
        size_t identifier_length = index - start;
        if (identifier_length == 0 ||
            (prerelease &&
             value[start] >= '0' && value[start] <= '9' &&
             valid_numeric_identifier(value + start, identifier_length) ==
                 false)) {
            bool numeric = identifier_length > 0;
            for (size_t digit = start; digit < index; digit++)
                numeric = numeric && value[digit] >= '0' && value[digit] <= '9';
            if (identifier_length == 0 || numeric)
                return false;
        }
        start = index + 1U;
    }
    return true;
}

static bool valid_version(const char *version)
{
    size_t length;
    size_t core_end;
    size_t prerelease_start = 0;
    size_t build_start = 0;
    size_t part_start = 0;
    unsigned int core_parts = 0;

    if (version == NULL || (length = strlen(version)) == 0 || length > 64)
        return false;
    core_end = length;
    for (size_t index = 0; index < length; index++) {
        if (version[index] == '+' && build_start == 0) {
            build_start = index + 1U;
            if (prerelease_start == 0)
                core_end = index;
        } else if (version[index] == '-' && prerelease_start == 0 &&
                   build_start == 0) {
            prerelease_start = index + 1U;
            core_end = index;
        }
    }
    if (build_start > 0 &&
        !valid_semver_identifiers(version + build_start,
                                  length - build_start, false))
        return false;
    size_t prerelease_end = build_start > 0 ? build_start - 1U : length;
    if (prerelease_start > 0 &&
        !valid_semver_identifiers(version + prerelease_start,
                                  prerelease_end - prerelease_start, true))
        return false;
    for (size_t index = 0; index <= core_end; index++) {
        if (index < core_end && version[index] != '.')
            continue;
        if (!valid_numeric_identifier(version + part_start, index - part_start))
            return false;
        core_parts++;
        part_start = index + 1U;
    }
    return core_parts == 3;
}

static bool valid_text(const char *value, size_t maximum_chars,
                       size_t maximum_bytes)
{
    size_t length;
    size_t characters = 0;

    if (value == NULL || (length = strlen(value)) == 0 ||
        length > maximum_bytes)
        return false;
    for (size_t index = 0; index < length;) {
        unsigned char byte = (unsigned char)value[index];
        uint32_t codepoint;
        size_t bytes;

        if (byte < 0x80U) {
            codepoint = byte;
            bytes = 1;
        } else if (byte >= 0xc2U && byte <= 0xdfU && index + 1U < length) {
            codepoint = (uint32_t)(byte & 0x1fU) << 6U;
            bytes = 2;
        } else if (byte >= 0xe0U && byte <= 0xefU && index + 2U < length) {
            codepoint = (uint32_t)(byte & 0x0fU) << 12U;
            bytes = 3;
        } else if (byte >= 0xf0U && byte <= 0xf4U && index + 3U < length) {
            codepoint = (uint32_t)(byte & 0x07U) << 18U;
            bytes = 4;
        } else {
            return false;
        }
        for (size_t continuation = 1; continuation < bytes; continuation++) {
            unsigned char next = (unsigned char)value[index + continuation];
            if ((next & 0xc0U) != 0x80U)
                return false;
            codepoint |= (uint32_t)(next & 0x3fU)
                         << (6U * (unsigned int)(bytes - continuation - 1U));
        }
        if ((bytes == 2 && codepoint < 0x80U) ||
            (bytes == 3 && codepoint < 0x800U) ||
            (bytes == 4 && codepoint < 0x10000U) || codepoint > 0x10ffffU ||
            (codepoint >= 0xd800U && codepoint <= 0xdfffU) ||
            codepoint < 0x20U || (codepoint >= 0x7fU && codepoint <= 0x9fU))
            return false;
        characters++;
        if (characters > maximum_chars)
            return false;
        index += bytes;
    }
    return true;
}

static bool valid_error_code(const char *document,
                             const struct cp0_json_token *token)
{
    static const char *codes[] = {
        "invalid-request", "unauthorized", "unconfigured", "unavailable",
        "not-found",       "busy",         "invalid-state", "untrusted",
        "resource-exhausted", "policy-restricted", "insufficient-storage",
        "catalog-changed", "internal",
    };
    for (size_t index = 0; index < sizeof(codes) / sizeof(codes[0]); index++) {
        if (cp0_json_string_equals(document, token, codes[index]))
            return true;
    }
    return false;
}

static int parse_envelope(const char *document, size_t length,
                          uint64_t request_id, struct cp0_json_token *tokens,
                          size_t token_capacity, size_t *token_count, int *data)
{
    int count = cp0_json_parse(document, length, tokens, token_capacity);
    if (count <= 0 || tokens[0].type != CP0_JSON_OBJECT ||
        tokens[0].children != 6)
        return CP0_STORE_RESULT_ERROR;
    int version = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                      "protocol_version");
    int response_id = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                          "request_id");
    int outcome = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                      "outcome");
    uint64_t parsed_version;
    uint64_t parsed_id;
    if (version < 0 || response_id < 0 || outcome < 0 ||
        !cp0_json_get_u64(document, &tokens[version], &parsed_version) ||
        !cp0_json_get_u64(document, &tokens[response_id], &parsed_id) ||
        parsed_version != 1 || parsed_id != request_id ||
        tokens[outcome].type != CP0_JSON_OBJECT)
        return CP0_STORE_RESULT_ERROR;
    int status = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                     "status");
    if (status < 0)
        return CP0_STORE_RESULT_ERROR;
    if (cp0_json_string_equals(document, &tokens[status], "error")) {
        int code = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                       "code");
        int message = cp0_json_object_get(document, tokens, (size_t)count,
                                          outcome, "message");
        char decoded_message[641];
        if (tokens[outcome].children != 6 || code < 0 || message < 0 ||
            !valid_error_code(document, &tokens[code]) ||
            !cp0_json_copy_string(document, &tokens[message], decoded_message,
                                  sizeof(decoded_message)) ||
            !valid_text(decoded_message, 160, 640))
            return CP0_STORE_RESULT_ERROR;
        if (cp0_json_string_equals(document, &tokens[code], "unconfigured"))
            return CP0_STORE_RESULT_UNCONFIGURED;
        if (cp0_json_string_equals(document, &tokens[code], "busy"))
            return CP0_STORE_RESULT_BUSY;
        if (cp0_json_string_equals(document, &tokens[code],
                                   "policy-restricted"))
            return CP0_STORE_RESULT_POLICY_RESTRICTED;
        if (cp0_json_string_equals(document, &tokens[code],
                                   "insufficient-storage"))
            return CP0_STORE_RESULT_INSUFFICIENT_STORAGE;
        if (cp0_json_string_equals(document, &tokens[code],
                                   "catalog-changed"))
            return CP0_STORE_RESULT_CATALOG_CHANGED;
        return CP0_STORE_RESULT_ERROR;
    }
    *data = cp0_json_object_get(document, tokens, (size_t)count, outcome, "data");
    if (tokens[outcome].children != 4 ||
        !cp0_json_string_equals(document, &tokens[status], "ok") || *data < 0 ||
        tokens[*data].type != CP0_JSON_OBJECT)
        return CP0_STORE_RESULT_ERROR;
    *token_count = (size_t)count;
    return CP0_STORE_RESULT_OK;
}

static bool parse_state(const char *document, const struct cp0_json_token *token,
                        enum cp0_store_app_state *state)
{
    static const char *names[] = {
        "available", "queued",    "downloading", "paused",
        "installing", "installed", "canceled",    "failed",
    };
    for (size_t index = 0; index < sizeof(names) / sizeof(names[0]); index++) {
        if (cp0_json_string_equals(document, token, names[index])) {
            *state = (enum cp0_store_app_state)index;
            return true;
        }
    }
    return false;
}

static int parse_auto_update_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_store_auto_update_status *status)
{
    struct cp0_json_token tokens[128];
    struct cp0_store_auto_update_status decoded = {0};
    size_t token_count;
    int data;
    int result;

    if (status == NULL)
        return CP0_STORE_RESULT_ERROR;
    result = parse_envelope(response, response_length, request_id, tokens,
                            sizeof(tokens) / sizeof(tokens[0]), &token_count,
                            &data);
    if (result != CP0_STORE_RESULT_OK)
        return result;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int enabled = cp0_json_object_get(response, tokens, token_count, data,
                                      "enabled");
    int policy_allowed = cp0_json_object_get(
        response, tokens, token_count, data, "policy_allowed");
    int charging = cp0_json_object_get(response, tokens, token_count, data,
                                       "charging");
    int unmetered = cp0_json_object_get(response, tokens, token_count, data,
                                        "unmetered_network");
    int due = cp0_json_object_get(response, tokens, token_count, data, "due");
    int checking = cp0_json_object_get(response, tokens, token_count, data,
                                       "checking");
    if (tokens[data].children != 14 || kind < 0 || enabled < 0 ||
        policy_allowed < 0 || charging < 0 || unmetered < 0 || due < 0 ||
        checking < 0 ||
        !cp0_json_string_equals(response, &tokens[kind],
                                "auto-update-status") ||
        !cp0_json_get_bool(response, &tokens[enabled], &decoded.enabled) ||
        !cp0_json_get_bool(response, &tokens[policy_allowed],
                           &decoded.policy_allowed) ||
        !cp0_json_get_bool(response, &tokens[charging], &decoded.charging) ||
        !cp0_json_get_bool(response, &tokens[unmetered],
                           &decoded.unmetered_network) ||
        !cp0_json_get_bool(response, &tokens[due], &decoded.due) ||
        !cp0_json_get_bool(response, &tokens[checking], &decoded.checking) ||
        ((!decoded.enabled) && (decoded.due || decoded.checking)))
        return CP0_STORE_RESULT_ERROR;
    *status = decoded;
    return CP0_STORE_RESULT_OK;
}

static bool parse_failure_reason(
    const char *document, const struct cp0_json_token *token,
    enum cp0_store_failure_reason *reason)
{
    static const char *names[] = {
        "network", "storage", "verification", "installer",
        "catalog-changed", "internal",
    };
    for (size_t index = 0; index < sizeof(names) / sizeof(names[0]); index++) {
        if (cp0_json_string_equals(document, token, names[index])) {
            *reason = (enum cp0_store_failure_reason)(index + 1U);
            return true;
        }
    }
    return false;
}

static bool permission_bit(const char *document,
                           const struct cp0_json_token *token, uint16_t *bit)
{
    static const struct {
        const char *name;
        uint16_t bit;
    } permissions[] = {
        {"audio.capture", CP0_STORE_PERMISSION_AUDIO_CAPTURE},
        {"audio.playback", CP0_STORE_PERMISSION_AUDIO_PLAYBACK},
        {"camera.capture", CP0_STORE_PERMISSION_CAMERA_CAPTURE},
        {"documents.open", CP0_STORE_PERMISSION_DOCUMENTS_OPEN},
        {"hardware.gpio", CP0_STORE_PERMISSION_HARDWARE_GPIO},
        {"network.client", CP0_STORE_PERMISSION_NETWORK_CLIENT},
        {"notifications.post", CP0_STORE_PERMISSION_NOTIFICATIONS_POST},
        {"radio.lora", CP0_STORE_PERMISSION_RADIO_LORA},
    };
    for (size_t index = 0; index < sizeof(permissions) / sizeof(permissions[0]);
         index++) {
        if (cp0_json_string_equals(document, token, permissions[index].name)) {
            *bit = permissions[index].bit;
            return true;
        }
    }
    return false;
}

static bool parse_permissions(const char *document,
                              const struct cp0_json_token *tokens,
                              size_t token_count, int array,
                              uint16_t *permissions)
{
    char previous[32] = {0};
    if (array < 0 || tokens[array].type != CP0_JSON_ARRAY ||
        tokens[array].children > 8)
        return false;
    *permissions = 0;
    for (unsigned int index = 0; index < tokens[array].children; index++) {
        int permission = cp0_json_array_get(tokens, token_count, array, index);
        char name[32];
        uint16_t bit;
        if (permission < 0 ||
            !cp0_json_copy_string(document, &tokens[permission], name,
                                  sizeof(name)) ||
            !permission_bit(document, &tokens[permission], &bit) ||
            (index > 0 && strcmp(previous, name) >= 0) ||
            (*permissions & bit) != 0)
            return false;
        memcpy(previous, name, strlen(name) + 1U);
        *permissions |= bit;
    }
    return true;
}

static bool parse_app(const char *document,
                      const struct cp0_json_token *tokens, size_t token_count,
                      int item, struct cp0_store_app_summary *app)
{
    int package_bytes = cp0_json_object_get(document, tokens, token_count, item,
                                            "package_bytes");
    int state = cp0_json_object_get(document, tokens, token_count, item, "state");
    int progress = cp0_json_object_get(document, tokens, token_count, item,
                                       "progress_percent");
    int failure = cp0_json_object_get(document, tokens, token_count, item,
                                      "failure_reason");
    int permissions = cp0_json_object_get(document, tokens, token_count, item,
                                          "permissions");
    uint64_t parsed_progress;
    if (tokens[item].type != CP0_JSON_OBJECT || package_bytes < 0 || state < 0 ||
        progress < 0 || permissions < 0 ||
        !copy_member(document, tokens, token_count, item, "app_id", app->app_id,
                     sizeof(app->app_id)) ||
        !valid_app_id(app->app_id) ||
        !copy_member(document, tokens, token_count, item, "name", app->name,
                     sizeof(app->name)) ||
        !valid_text(app->name, 32, 128) ||
        !copy_member(document, tokens, token_count, item, "version", app->version,
                     sizeof(app->version)) ||
        !valid_version(app->version) ||
        !copy_member(document, tokens, token_count, item, "summary", app->summary,
                     sizeof(app->summary)) ||
        !valid_text(app->summary, 96, 384) ||
        !cp0_json_get_u64(document, &tokens[package_bytes], &app->package_bytes) ||
        app->package_bytes == 0 || app->package_bytes > CP0_STORE_MAX_PACKAGE_BYTES ||
        !parse_state(document, &tokens[state], &app->state) ||
        !cp0_json_get_u64(document, &tokens[progress], &parsed_progress) ||
        parsed_progress > 100 ||
        !parse_permissions(document, tokens, token_count, permissions,
                           &app->permissions))
        return false;
    app->progress_percent = (uint8_t)parsed_progress;
    app->failure_reason = CP0_STORE_FAILURE_NONE;
    if (((app->state == CP0_STORE_APP_AVAILABLE ||
          app->state == CP0_STORE_APP_QUEUED ||
          app->state == CP0_STORE_APP_CANCELED ||
          app->state == CP0_STORE_APP_FAILED) &&
         app->progress_percent != 0) ||
        ((app->state == CP0_STORE_APP_INSTALLING ||
          app->state == CP0_STORE_APP_INSTALLED) &&
         app->progress_percent != 100) ||
        (app->state == CP0_STORE_APP_FAILED
             ? (tokens[item].children != 18 || failure < 0 ||
                !parse_failure_reason(document, &tokens[failure],
                                      &app->failure_reason))
             : (tokens[item].children != 16 || failure >= 0)))
        return false;

    return true;
}

static int parse_catalog_response(const char *response, size_t response_length,
                                  uint64_t request_id,
                                  struct cp0_store_catalog *catalog)
{
    struct cp0_json_token *tokens =
        calloc(CP0_STORE_JSON_TOKENS, sizeof(*tokens));
    struct cp0_store_catalog decoded = {0};
    size_t token_count;
    int data;
    int result;

    if (tokens == NULL || catalog == NULL) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    result = parse_envelope(response, response_length, request_id, tokens,
                            CP0_STORE_JSON_TOKENS, &token_count, &data);
    if (result != CP0_STORE_RESULT_OK) {
        free(tokens);
        return result;
    }
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int sequence = cp0_json_object_get(response, tokens, token_count, data,
                                       "sequence");
    int expires = cp0_json_object_get(response, tokens, token_count, data,
                                      "expires_unix_seconds");
    int stale = cp0_json_object_get(response, tokens, token_count, data, "stale");
    int apps = cp0_json_object_get(response, tokens, token_count, data, "apps");
    if (tokens[data].children != 10 || kind < 0 || sequence < 0 || expires < 0 ||
        stale < 0 || apps < 0 ||
        !cp0_json_string_equals(response, &tokens[kind], "catalog") ||
        !cp0_json_get_u64(response, &tokens[sequence], &decoded.sequence) ||
        decoded.sequence == 0 ||
        !cp0_json_get_u64(response, &tokens[expires],
                          &decoded.expires_unix_seconds) ||
        decoded.expires_unix_seconds == 0 ||
        !cp0_json_get_bool(response, &tokens[stale], &decoded.stale) ||
        tokens[apps].type != CP0_JSON_ARRAY ||
        tokens[apps].children > CP0_STORE_CATALOG_LIMIT) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    decoded.truncated = tokens[apps].children > CP0_STORE_MAX_APPS;
    decoded.count = tokens[apps].children;
    if (decoded.count > CP0_STORE_MAX_APPS)
        decoded.count = CP0_STORE_MAX_APPS;
    char previous_id[CP0_STORE_APP_ID_BYTES] = {0};
    for (unsigned int index = 0; index < tokens[apps].children; index++) {
        int item = cp0_json_array_get(tokens, token_count, apps, index);
        struct cp0_store_app_summary temporary = {0};
        if (item < 0 || !parse_app(response, tokens, token_count, item, &temporary) ||
            (index > 0 && strcmp(previous_id, temporary.app_id) >= 0)) {
            free(tokens);
            return CP0_STORE_RESULT_ERROR;
        }
        memcpy(previous_id, temporary.app_id, strlen(temporary.app_id) + 1U);
        if (index < CP0_STORE_MAX_APPS)
            decoded.apps[index] = temporary;
    }
    free(tokens);
    *catalog = decoded;
    return CP0_STORE_RESULT_OK;
}

static bool editorial_text_valid(const char *value, size_t maximum_chars,
                                 size_t maximum_bytes)
{
    size_t length;
    return valid_text(value, maximum_chars, maximum_bytes) &&
           (length = strlen(value)) > 0 && value[0] != ' ' &&
           value[length - 1U] != ' ';
}

static int parse_today_response(const char *response, size_t response_length,
                                uint64_t request_id,
                                struct cp0_store_today *today)
{
    struct cp0_json_token *tokens =
        calloc(CP0_STORE_JSON_TOKENS, sizeof(*tokens));
    struct cp0_store_today decoded = {0};
    char seen[1U + CP0_STORE_EDITORIAL_COLLECTION_MAX *
                       CP0_STORE_EDITORIAL_COLLECTION_APP_MAX]
             [CP0_STORE_APP_ID_BYTES] = {{0}};
    size_t seen_count = 0;
    size_t token_count;
    int data;
    int result;

    if (tokens == NULL || today == NULL) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    result = parse_envelope(response, response_length, request_id, tokens,
                            CP0_STORE_JSON_TOKENS, &token_count, &data);
    if (result != CP0_STORE_RESULT_OK) {
        free(tokens);
        return result;
    }
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int sequence = cp0_json_object_get(response, tokens, token_count, data,
                                       "sequence");
    int expires = cp0_json_object_get(response, tokens, token_count, data,
                                      "expires_unix_seconds");
    int stale = cp0_json_object_get(response, tokens, token_count, data, "stale");
    int editorial = cp0_json_object_get(response, tokens, token_count, data,
                                        "editorial");
    if (tokens[data].children != 10 || kind < 0 || sequence < 0 || expires < 0 ||
        stale < 0 || editorial < 0 ||
        !cp0_json_string_equals(response, &tokens[kind], "today") ||
        !cp0_json_get_u64(response, &tokens[sequence], &decoded.sequence) ||
        decoded.sequence == 0 ||
        !cp0_json_get_u64(response, &tokens[expires],
                          &decoded.expires_unix_seconds) ||
        decoded.expires_unix_seconds == 0 ||
        !cp0_json_get_bool(response, &tokens[stale], &decoded.stale)) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    if (cp0_json_is_null(response, &tokens[editorial])) {
        free(tokens);
        *today = decoded;
        return CP0_STORE_RESULT_OK;
    }

    int headline = cp0_json_object_get(response, tokens, token_count, editorial,
                                       "headline");
    int featured = cp0_json_object_get(response, tokens, token_count, editorial,
                                       "featured");
    int collections = cp0_json_object_get(response, tokens, token_count,
                                          editorial, "collections");
    if (tokens[editorial].type != CP0_JSON_OBJECT ||
        tokens[editorial].children != 6 || headline < 0 || featured < 0 ||
        collections < 0 ||
        !cp0_json_copy_string(response, &tokens[headline], decoded.headline,
                              sizeof(decoded.headline)) ||
        !editorial_text_valid(decoded.headline, 48,
                              CP0_STORE_EDITORIAL_HEADLINE_BYTES - 1U) ||
        !parse_app(response, tokens, token_count, featured, &decoded.featured) ||
        tokens[collections].type != CP0_JSON_ARRAY ||
        tokens[collections].children == 0 ||
        tokens[collections].children > CP0_STORE_EDITORIAL_COLLECTION_MAX) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    memcpy(seen[seen_count++], decoded.featured.app_id,
           strlen(decoded.featured.app_id) + 1U);
    decoded.collection_count = tokens[collections].children;
    for (unsigned int collection_index = 0;
         collection_index < tokens[collections].children; collection_index++) {
        int collection = cp0_json_array_get(tokens, token_count, collections,
                                            collection_index);
        int title = cp0_json_object_get(response, tokens, token_count, collection,
                                        "title");
        int apps = cp0_json_object_get(response, tokens, token_count, collection,
                                       "apps");
        struct cp0_store_editorial_collection *decoded_collection =
            &decoded.collections[collection_index];
        if (collection < 0 || tokens[collection].type != CP0_JSON_OBJECT ||
            tokens[collection].children != 4 || title < 0 || apps < 0 ||
            !cp0_json_copy_string(response, &tokens[title],
                                  decoded_collection->title,
                                  sizeof(decoded_collection->title)) ||
            !editorial_text_valid(decoded_collection->title, 32,
                                  CP0_STORE_EDITORIAL_TITLE_BYTES - 1U) ||
            (collection_index > 0 &&
             strcmp(decoded.collections[0].title,
                    decoded_collection->title) == 0) ||
            tokens[apps].type != CP0_JSON_ARRAY || tokens[apps].children == 0 ||
            tokens[apps].children > CP0_STORE_EDITORIAL_COLLECTION_APP_MAX) {
            free(tokens);
            return CP0_STORE_RESULT_ERROR;
        }
        decoded_collection->count = tokens[apps].children;
        for (unsigned int app_index = 0; app_index < tokens[apps].children;
             app_index++) {
            int app = cp0_json_array_get(tokens, token_count, apps, app_index);
            struct cp0_store_app_summary *decoded_app =
                &decoded_collection->apps[app_index];
            if (app < 0 ||
                !parse_app(response, tokens, token_count, app, decoded_app)) {
                free(tokens);
                return CP0_STORE_RESULT_ERROR;
            }
            for (size_t previous = 0; previous < seen_count; previous++) {
                if (strcmp(seen[previous], decoded_app->app_id) == 0) {
                    free(tokens);
                    return CP0_STORE_RESULT_ERROR;
                }
            }
            memcpy(seen[seen_count++], decoded_app->app_id,
                   strlen(decoded_app->app_id) + 1U);
        }
    }
    decoded.has_editorial = true;
    free(tokens);
    *today = decoded;
    return CP0_STORE_RESULT_OK;
}

static bool valid_search_query(const char *query)
{
    size_t length;

    if (!valid_text(query, CP0_STORE_SEARCH_QUERY_CHARS,
                    CP0_STORE_SEARCH_QUERY_BYTES - 1U))
        return false;
    length = strlen(query);
    if (query[0] == ' ' || query[length - 1U] == ' ')
        return false;
    for (size_t index = 0; index < length; index++) {
        unsigned char byte = (unsigned char)query[index];
        if (!((byte >= 'a' && byte <= 'z') ||
              (byte >= 'A' && byte <= 'Z') ||
              (byte >= '0' && byte <= '9') || byte == ' ' || byte == '.' ||
              byte == '-' || byte == '_'))
            return false;
    }
    return true;
}

static int parse_search_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *expected_query, uint16_t expected_offset,
    uint8_t expected_limit, struct cp0_store_search_results *results)
{
    struct cp0_json_token *tokens =
        calloc(CP0_STORE_JSON_TOKENS, sizeof(*tokens));
    struct cp0_store_search_results decoded = {0};
    size_t token_count;
    int data;
    int result;

    if (tokens == NULL || results == NULL ||
        !valid_search_query(expected_query) || expected_offset > 64U ||
        expected_limit == 0 || expected_limit > CP0_STORE_SEARCH_MAX_APPS) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    result = parse_envelope(response, response_length, request_id, tokens,
                            CP0_STORE_JSON_TOKENS, &token_count, &data);
    if (result != CP0_STORE_RESULT_OK) {
        free(tokens);
        return result;
    }
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int query = cp0_json_object_get(response, tokens, token_count, data, "query");
    int offset = cp0_json_object_get(response, tokens, token_count, data, "offset");
    int limit = cp0_json_object_get(response, tokens, token_count, data, "limit");
    int total = cp0_json_object_get(response, tokens, token_count, data, "total");
    int next = cp0_json_object_get(response, tokens, token_count, data,
                                   "next_offset");
    int sequence = cp0_json_object_get(response, tokens, token_count, data,
                                       "sequence");
    int expires = cp0_json_object_get(response, tokens, token_count, data,
                                      "expires_unix_seconds");
    int stale = cp0_json_object_get(response, tokens, token_count, data, "stale");
    int apps = cp0_json_object_get(response, tokens, token_count, data, "apps");
    uint64_t parsed_offset;
    uint64_t parsed_limit;
    uint64_t parsed_total;
    uint64_t parsed_next = 0;

    if (tokens[data].children != 20 || kind < 0 || query < 0 || offset < 0 ||
        limit < 0 || total < 0 || next < 0 || sequence < 0 || expires < 0 ||
        stale < 0 || apps < 0 ||
        !cp0_json_string_equals(response, &tokens[kind], "search-results") ||
        !cp0_json_copy_string(response, &tokens[query], decoded.query,
                              sizeof(decoded.query)) ||
        strcmp(decoded.query, expected_query) != 0 ||
        !cp0_json_get_u64(response, &tokens[offset], &parsed_offset) ||
        !cp0_json_get_u64(response, &tokens[limit], &parsed_limit) ||
        !cp0_json_get_u64(response, &tokens[total], &parsed_total) ||
        parsed_offset != expected_offset || parsed_limit != expected_limit ||
        parsed_total > CP0_STORE_CATALOG_LIMIT ||
        !cp0_json_get_u64(response, &tokens[sequence], &decoded.sequence) ||
        decoded.sequence == 0 ||
        !cp0_json_get_u64(response, &tokens[expires],
                          &decoded.expires_unix_seconds) ||
        decoded.expires_unix_seconds == 0 ||
        !cp0_json_get_bool(response, &tokens[stale], &decoded.stale) ||
        tokens[apps].type != CP0_JSON_ARRAY ||
        tokens[apps].children > CP0_STORE_SEARCH_MAX_APPS) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    decoded.offset = (uint16_t)parsed_offset;
    decoded.limit = (uint8_t)parsed_limit;
    decoded.total = (uint16_t)parsed_total;
    uint16_t remaining = decoded.total > decoded.offset
                             ? (uint16_t)(decoded.total - decoded.offset)
                             : 0;
    uint16_t expected_count = remaining < decoded.limit ? remaining
                                                        : decoded.limit;
    uint16_t expected_next = (uint16_t)(decoded.offset + expected_count);
    bool should_have_next = expected_next < decoded.total;
    if (tokens[apps].children != expected_count ||
        (should_have_next
             ? (!cp0_json_get_u64(response, &tokens[next], &parsed_next) ||
                parsed_next != expected_next)
             : !cp0_json_is_null(response, &tokens[next]))) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    decoded.has_next = should_have_next;
    decoded.next_offset = should_have_next ? expected_next : 0;
    decoded.count = expected_count;
    for (unsigned int index = 0; index < tokens[apps].children; index++) {
        int item = cp0_json_array_get(tokens, token_count, apps, index);
        if (item < 0 ||
            !parse_app(response, tokens, token_count, item,
                       &decoded.apps[index])) {
            free(tokens);
            return CP0_STORE_RESULT_ERROR;
        }
        for (unsigned int previous = 0; previous < index; previous++) {
            if (strcmp(decoded.apps[previous].app_id,
                       decoded.apps[index].app_id) == 0) {
                free(tokens);
                return CP0_STORE_RESULT_ERROR;
            }
        }
    }
    free(tokens);
    *results = decoded;
    return CP0_STORE_RESULT_OK;
}

static int parse_accepted(const char *response, size_t response_length,
                          uint64_t request_id, const char *expected_kind,
                          const char *expected_app_id)
{
    struct cp0_json_token tokens[64];
    size_t token_count;
    int data;
    int result = parse_envelope(response, response_length, request_id, tokens,
                                64, &token_count, &data);
    if (result != CP0_STORE_RESULT_OK)
        return result;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    unsigned int expected_children = expected_app_id == NULL ? 2U : 6U;
    if (tokens[data].children != expected_children || kind < 0 ||
        !cp0_json_string_equals(response, &tokens[kind], expected_kind))
        return CP0_STORE_RESULT_ERROR;
    if (expected_app_id != NULL) {
        char app_id[CP0_STORE_APP_ID_BYTES];
        char version[CP0_STORE_VERSION_BYTES];
        if (!copy_member(response, tokens, token_count, data, "app_id", app_id,
                         sizeof(app_id)) ||
            strcmp(app_id, expected_app_id) != 0 ||
            !copy_member(response, tokens, token_count, data, "version", version,
                         sizeof(version)) ||
            !valid_version(version))
            return CP0_STORE_RESULT_ERROR;
    }
    return CP0_STORE_RESULT_OK;
}

static bool valid_install_batch(const char *const app_ids[], size_t app_count)
{
    if (app_ids == NULL || app_count == 0 ||
        app_count > CP0_STORE_INSTALL_BATCH_MAX)
        return false;
    for (size_t index = 0; index < app_count; index++) {
        if (!valid_app_id(app_ids[index]) ||
            (index > 0 && strcmp(app_ids[index - 1], app_ids[index]) >= 0))
            return false;
    }
    return true;
}

static int parse_install_preflight(
    const char *response, size_t response_length, uint64_t request_id,
    uint64_t expected_sequence, const char *const expected_app_ids[],
    size_t expected_app_count, struct cp0_store_install_preflight *preflight)
{
    struct cp0_json_token tokens[256];
    size_t token_count;
    int data;
    uint64_t authorization_id;
    uint64_t catalog_sequence;
    uint64_t required_bytes;
    uint64_t available_bytes;

    if (preflight == NULL || expected_sequence == 0 ||
        !valid_install_batch(expected_app_ids, expected_app_count))
        return CP0_STORE_RESULT_ERROR;
    int result = parse_envelope(response, response_length, request_id, tokens,
                                256, &token_count, &data);
    if (result != CP0_STORE_RESULT_OK)
        return result;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int authorization = cp0_json_object_get(
        response, tokens, token_count, data, "authorization_id");
    int sequence = cp0_json_object_get(response, tokens, token_count, data,
                                       "catalog_sequence");
    int required = cp0_json_object_get(response, tokens, token_count, data,
                                       "required_bytes");
    int available = cp0_json_object_get(response, tokens, token_count, data,
                                        "available_bytes");
    int apps = cp0_json_object_get(response, tokens, token_count, data, "apps");
    if (tokens[data].children != 12 || kind < 0 || authorization < 0 ||
        sequence < 0 || required < 0 || available < 0 || apps < 0 ||
        !cp0_json_string_equals(response, &tokens[kind],
                                "install-preflight") ||
        !cp0_json_get_u64(response, &tokens[authorization],
                          &authorization_id) ||
        !cp0_json_get_u64(response, &tokens[sequence], &catalog_sequence) ||
        !cp0_json_get_u64(response, &tokens[required], &required_bytes) ||
        !cp0_json_get_u64(response, &tokens[available], &available_bytes) ||
        authorization_id == 0 || catalog_sequence != expected_sequence ||
        required_bytes == 0 || available_bytes < required_bytes ||
        tokens[apps].type != CP0_JSON_ARRAY ||
        tokens[apps].children != expected_app_count)
        return CP0_STORE_RESULT_ERROR;
    memset(preflight, 0, sizeof(*preflight));
    preflight->authorization_id = authorization_id;
    preflight->catalog_sequence = catalog_sequence;
    preflight->required_bytes = required_bytes;
    preflight->available_bytes = available_bytes;
    preflight->count = expected_app_count;
    for (size_t index = 0; index < expected_app_count; index++) {
        int item = cp0_json_array_get(tokens, token_count, apps,
                                      (unsigned int)index);
        int permissions;
        int denied;
        struct cp0_store_install_preflight_app *app = &preflight->apps[index];
        if (item < 0 || tokens[item].type != CP0_JSON_OBJECT ||
            tokens[item].children != 8 ||
            !copy_member(response, tokens, token_count, item, "app_id",
                         app->app_id, sizeof(app->app_id)) ||
            strcmp(app->app_id, expected_app_ids[index]) != 0 ||
            !copy_member(response, tokens, token_count, item, "version",
                         app->version, sizeof(app->version)) ||
            !valid_version(app->version))
            return CP0_STORE_RESULT_ERROR;
        permissions = cp0_json_object_get(response, tokens, token_count, item,
                                          "permissions");
        denied = cp0_json_object_get(response, tokens, token_count, item,
                                     "policy_denied_permissions");
        if (!parse_permissions(response, tokens, token_count, permissions,
                               &app->permissions) ||
            !parse_permissions(response, tokens, token_count, denied,
                               &app->policy_denied_permissions) ||
            (app->policy_denied_permissions & ~app->permissions) != 0)
            return CP0_STORE_RESULT_ERROR;
    }
    return CP0_STORE_RESULT_OK;
}

static int parse_install_batch_accepted(
    const char *response, size_t response_length, uint64_t request_id,
    const char *const expected_app_ids[], size_t expected_app_count)
{
    struct cp0_json_token tokens[128];
    size_t token_count;
    int data;

    if (!valid_install_batch(expected_app_ids, expected_app_count))
        return CP0_STORE_RESULT_ERROR;
    int result = parse_envelope(response, response_length, request_id, tokens,
                                128, &token_count, &data);
    if (result != CP0_STORE_RESULT_OK)
        return result;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int apps = cp0_json_object_get(response, tokens, token_count, data, "apps");
    if (tokens[data].children != 4 || kind < 0 || apps < 0 ||
        !cp0_json_string_equals(response, &tokens[kind],
                                "install-batch-accepted") ||
        tokens[apps].type != CP0_JSON_ARRAY ||
        tokens[apps].children != expected_app_count)
        return CP0_STORE_RESULT_ERROR;
    for (size_t index = 0; index < expected_app_count; index++) {
        int item = cp0_json_array_get(tokens, token_count, apps,
                                      (unsigned int)index);
        char app_id[CP0_STORE_APP_ID_BYTES];
        char version[CP0_STORE_VERSION_BYTES];
        if (item < 0 || tokens[item].type != CP0_JSON_OBJECT ||
            tokens[item].children != 4 ||
            !copy_member(response, tokens, token_count, item, "app_id", app_id,
                         sizeof(app_id)) ||
            strcmp(app_id, expected_app_ids[index]) != 0 ||
            !copy_member(response, tokens, token_count, item, "version", version,
                         sizeof(version)) ||
            !valid_version(version))
            return CP0_STORE_RESULT_ERROR;
    }
    return CP0_STORE_RESULT_OK;
}

static const char *control_action_name(enum cp0_store_control_action action)
{
    static const char *names[] = {"pause", "resume", "cancel"};
    return action <= CP0_STORE_CONTROL_CANCEL ? names[action] : NULL;
}

static int parse_control_accepted(
    const char *response, size_t response_length, uint64_t request_id,
    const char *expected_app_id, enum cp0_store_control_action expected_action)
{
    struct cp0_json_token tokens[64];
    size_t token_count;
    int data;
    int result = parse_envelope(response, response_length, request_id, tokens,
                                64, &token_count, &data);
    const char *action_name = control_action_name(expected_action);
    if (result != CP0_STORE_RESULT_OK || action_name == NULL)
        return result == CP0_STORE_RESULT_OK ? CP0_STORE_RESULT_ERROR : result;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int action = cp0_json_object_get(response, tokens, token_count, data,
                                     "action");
    char app_id[CP0_STORE_APP_ID_BYTES];
    char version[CP0_STORE_VERSION_BYTES];
    if (tokens[data].children != 8 || kind < 0 || action < 0 ||
        !cp0_json_string_equals(response, &tokens[kind],
                                "operation-accepted") ||
        !cp0_json_string_equals(response, &tokens[action], action_name) ||
        !copy_member(response, tokens, token_count, data, "app_id", app_id,
                     sizeof(app_id)) || strcmp(app_id, expected_app_id) != 0 ||
        !copy_member(response, tokens, token_count, data, "version", version,
                     sizeof(version)) || !valid_version(version))
        return CP0_STORE_RESULT_ERROR;
    return CP0_STORE_RESULT_OK;
}

static bool valid_https_url(const char *url)
{
    size_t length;
    if (url == NULL || (length = strlen(url)) < 10U ||
        length >= CP0_STORE_URL_BYTES || strncmp(url, "https://", 8) != 0 ||
        strchr(url, '@') != NULL || strchr(url, '#') != NULL ||
        strchr(url + 8, '.') == NULL)
        return false;
    for (size_t index = 0; index < length; index++) {
        unsigned char byte = (unsigned char)url[index];
        if (byte <= 0x20U || byte == 0x7fU)
            return false;
    }
    return true;
}

static bool valid_prose(const char *value, size_t maximum_chars,
                        size_t maximum_bytes)
{
    size_t length;
    char *single_line;
    bool valid;

    if (value == NULL || (length = strlen(value)) == 0 ||
        length > maximum_bytes || value[0] == ' ' || value[0] == '\n' ||
        value[length - 1U] == ' ' || value[length - 1U] == '\n')
        return false;
    single_line = malloc(length + 1U);
    if (single_line == NULL)
        return false;
    for (size_t index = 0; index < length; index++) {
        unsigned char byte = (unsigned char)value[index];
        if (byte < 0x20U && byte != '\n') {
            free(single_line);
            return false;
        }
        single_line[index] = byte == '\n' ? ' ' : (char)byte;
    }
    single_line[length] = '\0';
    valid = valid_text(single_line, maximum_chars, maximum_bytes);
    free(single_line);
    return valid;
}

static bool parse_category(const char *document,
                           const struct cp0_json_token *token,
                           enum cp0_store_category *category)
{
    static const char *names[] = {
        "developer-tools", "education",    "entertainment", "games",
        "hardware",        "media",        "productivity",  "utilities",
    };
    for (size_t index = 0; index < sizeof(names) / sizeof(names[0]); index++) {
        if (cp0_json_string_equals(document, token, names[index])) {
            *category = (enum cp0_store_category)index;
            return true;
        }
    }
    return false;
}

static bool parse_age_rating(const char *document,
                             const struct cp0_json_token *token,
                             enum cp0_store_age_rating *rating)
{
    static const char *names[] = {"4+", "9+", "12+", "17+"};
    for (size_t index = 0; index < sizeof(names) / sizeof(names[0]); index++) {
        if (cp0_json_string_equals(document, token, names[index])) {
            *rating = (enum cp0_store_age_rating)index;
            return true;
        }
    }
    return false;
}

static int parse_details_response(const char *response, size_t response_length,
                                  uint64_t request_id,
                                  const char *expected_app_id,
                                  const char *expected_version,
                                  struct cp0_store_app_details *details)
{
    struct cp0_json_token *tokens =
        calloc(CP0_STORE_JSON_TOKENS, sizeof(*tokens));
    struct cp0_store_app_details decoded = {0};
    size_t token_count;
    int data;
    int result;
    uint64_t screenshot_count;

    if (tokens == NULL || details == NULL || !valid_app_id(expected_app_id) ||
        !valid_version(expected_version)) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    result = parse_envelope(response, response_length, request_id, tokens,
                            CP0_STORE_JSON_TOKENS, &token_count, &data);
    if (result != CP0_STORE_RESULT_OK) {
        free(tokens);
        return result;
    }
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int category = cp0_json_object_get(response, tokens, token_count, data,
                                       "category");
    int age = cp0_json_object_get(response, tokens, token_count, data,
                                  "age_rating");
    int screenshots = cp0_json_object_get(response, tokens, token_count, data,
                                           "screenshot_count");
    if (tokens[data].children != 22 || kind < 0 || category < 0 || age < 0 ||
        screenshots < 0 ||
        !cp0_json_string_equals(response, &tokens[kind], "app-details") ||
        !copy_member(response, tokens, token_count, data, "app_id",
                     decoded.app_id, sizeof(decoded.app_id)) ||
        strcmp(decoded.app_id, expected_app_id) != 0 ||
        !copy_member(response, tokens, token_count, data, "version",
                     decoded.version, sizeof(decoded.version)) ||
        strcmp(decoded.version, expected_version) != 0 ||
        !copy_member(response, tokens, token_count, data, "developer",
                     decoded.developer, sizeof(decoded.developer)) ||
        !valid_text(decoded.developer, 80, CP0_STORE_DEVELOPER_BYTES - 1U) ||
        !parse_category(response, &tokens[category], &decoded.category) ||
        !parse_age_rating(response, &tokens[age], &decoded.age_rating) ||
        !copy_member(response, tokens, token_count, data, "privacy_url",
                     decoded.privacy_url, sizeof(decoded.privacy_url)) ||
        !valid_https_url(decoded.privacy_url) ||
        !copy_member(response, tokens, token_count, data, "support_url",
                     decoded.support_url, sizeof(decoded.support_url)) ||
        !valid_https_url(decoded.support_url) ||
        !copy_member(response, tokens, token_count, data, "description",
                     decoded.description, sizeof(decoded.description)) ||
        !valid_prose(decoded.description, 1024,
                     CP0_STORE_DESCRIPTION_BYTES - 1U) ||
        !copy_member(response, tokens, token_count, data, "release_notes",
                     decoded.release_notes, sizeof(decoded.release_notes)) ||
        !valid_prose(decoded.release_notes, 512,
                     CP0_STORE_RELEASE_NOTES_BYTES - 1U) ||
        !cp0_json_get_u64(response, &tokens[screenshots], &screenshot_count) ||
        screenshot_count == 0 ||
        screenshot_count > CP0_STORE_MAX_SCREENSHOTS) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    decoded.screenshot_count = (uint8_t)screenshot_count;
    free(tokens);
    *details = decoded;
    return CP0_STORE_RESULT_OK;
}

enum parsed_media_kind { PARSED_MEDIA_ICON, PARSED_MEDIA_SCREENSHOT };

struct parsed_media {
    enum parsed_media_kind kind;
    uint8_t index;
    struct cp0_store_image_metadata metadata;
};

static bool lower_hex_sha256(const char *digest)
{
    if (digest == NULL || strlen(digest) != 64U)
        return false;
    for (size_t index = 0; index < 64U; index++) {
        char byte = digest[index];
        if (!((byte >= '0' && byte <= '9') ||
              (byte >= 'a' && byte <= 'f')))
            return false;
    }
    return true;
}

static int parse_media_response(const char *response, size_t response_length,
                                uint64_t request_id,
                                const char *expected_app_id,
                                const char *expected_version,
                                enum parsed_media_kind expected_kind,
                                uint8_t expected_index,
                                struct parsed_media *media)
{
    struct cp0_json_token tokens[96];
    size_t token_count;
    int data;
    int result = parse_envelope(response, response_length, request_id, tokens,
                                96, &token_count, &data);
    if (result != CP0_STORE_RESULT_OK)
        return result;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int nested = cp0_json_object_get(response, tokens, token_count, data,
                                     "media");
    char app_id[CP0_STORE_APP_ID_BYTES];
    char version[CP0_STORE_VERSION_BYTES];
    if (tokens[data].children != 8 || kind < 0 || nested < 0 ||
        !cp0_json_string_equals(response, &tokens[kind], "media") ||
        !copy_member(response, tokens, token_count, data, "app_id", app_id,
                     sizeof(app_id)) || strcmp(app_id, expected_app_id) != 0 ||
        !copy_member(response, tokens, token_count, data, "version", version,
                     sizeof(version)) || strcmp(version, expected_version) != 0 ||
        tokens[nested].type != CP0_JSON_OBJECT)
        return CP0_STORE_RESULT_ERROR;

    int media_kind = cp0_json_object_get(response, tokens, token_count, nested,
                                         "kind");
    int sha256 = cp0_json_object_get(response, tokens, token_count, nested,
                                     "sha256");
    int bytes = cp0_json_object_get(response, tokens, token_count, nested,
                                    "bytes");
    int width = cp0_json_object_get(response, tokens, token_count, nested,
                                    "width");
    int height = cp0_json_object_get(response, tokens, token_count, nested,
                                     "height");
    int index = cp0_json_object_get(response, tokens, token_count, nested,
                                    "index");
    uint64_t parsed_width;
    uint64_t parsed_height;
    uint64_t parsed_index = 0;
    struct parsed_media decoded = {.kind = expected_kind,
                                   .index = expected_index};
    unsigned int expected_children =
        expected_kind == PARSED_MEDIA_ICON ? 10U : 12U;
    const char *expected_name =
        expected_kind == PARSED_MEDIA_ICON ? "icon" : "screenshot";
    uint64_t maximum_bytes = expected_kind == PARSED_MEDIA_ICON
                                 ? CP0_STORE_MAX_ICON_BYTES
                                 : CP0_STORE_MAX_SCREENSHOT_BYTES;
    if (tokens[nested].children != expected_children || media_kind < 0 ||
        sha256 < 0 || bytes < 0 || width < 0 || height < 0 ||
        (expected_kind == PARSED_MEDIA_SCREENSHOT && index < 0) ||
        (expected_kind == PARSED_MEDIA_ICON && index >= 0) ||
        !cp0_json_string_equals(response, &tokens[media_kind], expected_name) ||
        !cp0_json_copy_string(response, &tokens[sha256],
                              decoded.metadata.sha256,
                              sizeof(decoded.metadata.sha256)) ||
        !lower_hex_sha256(decoded.metadata.sha256) ||
        !cp0_json_get_u64(response, &tokens[bytes],
                          &decoded.metadata.encoded_bytes) ||
        decoded.metadata.encoded_bytes == 0 ||
        decoded.metadata.encoded_bytes > maximum_bytes ||
        !cp0_json_get_u64(response, &tokens[width], &parsed_width) ||
        !cp0_json_get_u64(response, &tokens[height], &parsed_height) ||
        (expected_kind == PARSED_MEDIA_SCREENSHOT &&
         (!cp0_json_get_u64(response, &tokens[index], &parsed_index) ||
          parsed_index != expected_index)) ||
        (expected_kind == PARSED_MEDIA_ICON &&
         !((parsed_width == 32U && parsed_height == 32U) ||
           (parsed_width == 48U && parsed_height == 48U))) ||
        (expected_kind == PARSED_MEDIA_SCREENSHOT &&
         (parsed_width != 320U || parsed_height != 170U)))
        return CP0_STORE_RESULT_ERROR;
    decoded.metadata.width = (uint16_t)parsed_width;
    decoded.metadata.height = (uint16_t)parsed_height;
    *media = decoded;
    return CP0_STORE_RESULT_OK;
}

struct png_decode_state {
    FILE *input;
    png_structp png;
    png_infop info;
    unsigned char *decoded;
    png_bytep *rows;
};

static int decode_png_descriptor(
    int descriptor, const struct cp0_store_image_metadata *metadata,
    uint32_t *pixels, size_t pixel_capacity)
{
    struct stat status;
    int descriptor_flags = fcntl(descriptor, F_GETFD);
    int open_flags = fcntl(descriptor, F_GETFL);
    int input_descriptor = -1;
    struct png_decode_state *state = NULL;
    int result = -1;

    if (metadata == NULL || pixels == NULL ||
        pixel_capacity < (size_t)metadata->width * metadata->height ||
        descriptor_flags < 0 || (descriptor_flags & FD_CLOEXEC) == 0 ||
        open_flags < 0 || (open_flags & O_ACCMODE) != O_RDONLY ||
        fstat(descriptor, &status) != 0 || !S_ISREG(status.st_mode) ||
        status.st_size < 0 || (uint64_t)status.st_size != metadata->encoded_bytes)
        goto cleanup;
    state = calloc(1, sizeof(*state));
    if (state == NULL)
        goto cleanup;
    input_descriptor = fcntl(descriptor, F_DUPFD_CLOEXEC, 0);
    if (input_descriptor < 0 ||
        (state->input = fdopen(input_descriptor, "rb")) == NULL)
        goto cleanup;
    input_descriptor = -1;
    state->png = png_create_read_struct(PNG_LIBPNG_VER_STRING, NULL, NULL, NULL);
    if (state->png == NULL ||
        (state->info = png_create_info_struct(state->png)) == NULL)
        goto cleanup;
    if (setjmp(png_jmpbuf(state->png)) != 0)
        goto cleanup;
    png_init_io(state->png, state->input);
    png_set_crc_action(state->png, PNG_CRC_ERROR_QUIT, PNG_CRC_ERROR_QUIT);
    png_set_user_limits(state->png, metadata->width, metadata->height);
    png_read_info(state->png, state->info);
    png_uint_32 width = png_get_image_width(state->png, state->info);
    png_uint_32 height = png_get_image_height(state->png, state->info);
    int color_type = png_get_color_type(state->png, state->info);
    int bit_depth = png_get_bit_depth(state->png, state->info);
    bool has_transparency =
        png_get_valid(state->png, state->info, PNG_INFO_tRNS) != 0;
    if (width != metadata->width || height != metadata->height)
        goto cleanup;
    if (bit_depth == 16)
        png_set_strip_16(state->png);
    if (color_type == PNG_COLOR_TYPE_PALETTE)
        png_set_palette_to_rgb(state->png);
    if (color_type == PNG_COLOR_TYPE_GRAY && bit_depth < 8)
        png_set_expand_gray_1_2_4_to_8(state->png);
    if (has_transparency)
        png_set_tRNS_to_alpha(state->png);
    if (color_type == PNG_COLOR_TYPE_GRAY ||
        color_type == PNG_COLOR_TYPE_GRAY_ALPHA)
        png_set_gray_to_rgb(state->png);
    if ((color_type & PNG_COLOR_MASK_ALPHA) == 0 && !has_transparency)
        png_set_add_alpha(state->png, 0xff, PNG_FILLER_AFTER);
    png_read_update_info(state->png, state->info);
    size_t row_bytes = png_get_rowbytes(state->png, state->info);
    if (png_get_channels(state->png, state->info) != 4 ||
        row_bytes != (size_t)width * 4U || height > SIZE_MAX / row_bytes)
        goto cleanup;
    state->decoded = malloc((size_t)height * row_bytes);
    state->rows = malloc((size_t)height * sizeof(*state->rows));
    if (state->decoded == NULL || state->rows == NULL)
        goto cleanup;
    for (png_uint_32 y = 0; y < height; y++)
        state->rows[y] = state->decoded + (size_t)y * row_bytes;
    png_read_image(state->png, state->rows);
    png_read_end(state->png, state->info);
    for (png_uint_32 y = 0; y < height; y++) {
        for (png_uint_32 x = 0; x < width; x++) {
            const unsigned char *source =
                state->decoded + (size_t)y * row_bytes + (size_t)x * 4U;
            pixels[(size_t)y * width + x] =
                ((uint32_t)source[3] << 24U) | ((uint32_t)source[0] << 16U) |
                ((uint32_t)source[1] << 8U) | source[2];
        }
    }
    result = 0;

cleanup:
    if (state != NULL) {
        free(state->rows);
        free(state->decoded);
        if (state->png != NULL)
            png_destroy_read_struct(
                &state->png, state->info == NULL ? NULL : &state->info, NULL);
        if (state->input != NULL)
            fclose(state->input);
        free(state);
    }
    if (input_descriptor >= 0)
        close(input_descriptor);
    return result;
}

#ifdef CP0_STORE_CLIENT_TEST
int cp0_store_test_parse_catalog_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_store_catalog *catalog)
{
    return parse_catalog_response(response, response_length, request_id, catalog);
}

int cp0_store_test_parse_today_response(const char *response,
                                        size_t response_length,
                                        uint64_t request_id,
                                        struct cp0_store_today *today)
{
    return parse_today_response(response, response_length, request_id, today);
}

int cp0_store_test_parse_refresh_response(const char *response,
                                          size_t response_length,
                                          uint64_t request_id)
{
    return parse_accepted(response, response_length, request_id,
                          "refresh-accepted", NULL);
}

int cp0_store_test_parse_auto_update_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_store_auto_update_status *status)
{
    return parse_auto_update_response(response, response_length, request_id,
                                      status);
}

int cp0_store_test_parse_install_response(const char *response,
                                          size_t response_length,
                                          uint64_t request_id,
                                          const char *app_id)
{
    return parse_accepted(response, response_length, request_id,
                          "install-accepted", app_id);
}

int cp0_store_test_parse_install_preflight_response(
    const char *response, size_t response_length, uint64_t request_id,
    uint64_t catalog_sequence, const char *const app_ids[], size_t app_count,
    struct cp0_store_install_preflight *preflight)
{
    return parse_install_preflight(response, response_length, request_id,
                                   catalog_sequence, app_ids, app_count,
                                   preflight);
}

int cp0_store_test_parse_install_batch_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *const app_ids[], size_t app_count)
{
    return parse_install_batch_accepted(response, response_length, request_id,
                                        app_ids, app_count);
}

int cp0_store_test_parse_control_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *app_id, enum cp0_store_control_action action)
{
    return parse_control_accepted(response, response_length, request_id, app_id,
                                  action);
}

int cp0_store_test_parse_search_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *query, uint16_t offset, uint8_t limit,
    struct cp0_store_search_results *results)
{
    return parse_search_response(response, response_length, request_id, query,
                                 offset, limit, results);
}

int cp0_store_test_parse_details_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *app_id, const char *version,
    struct cp0_store_app_details *details)
{
    return parse_details_response(response, response_length, request_id, app_id,
                                  version, details);
}

int cp0_store_test_parse_media_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *app_id, const char *version, bool screenshot, uint8_t index,
    struct cp0_store_image_metadata *metadata)
{
    struct parsed_media media;
    int result = parse_media_response(
        response, response_length, request_id, app_id, version,
        screenshot ? PARSED_MEDIA_SCREENSHOT : PARSED_MEDIA_ICON, index,
        &media);
    if (result == CP0_STORE_RESULT_OK)
        *metadata = media.metadata;
    return result;
}

int cp0_store_test_receive_chunk(int socket_descriptor, char *buffer,
                                 size_t capacity, int *received_descriptor)
{
    return receive_chunk(socket_descriptor, buffer, capacity,
                         received_descriptor);
}

int cp0_store_test_decode_png_descriptor(
    int descriptor, const struct cp0_store_image_metadata *metadata,
    uint32_t *pixels, size_t pixel_capacity)
{
    return decode_png_descriptor(descriptor, metadata, pixels, pixel_capacity);
}
#endif

int cp0_store_list(struct cp0_store_catalog *catalog)
{
    char request[192];
    char *response = malloc(CP0_STORE_FRAME_BYTES);
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"list\"}}\n",
        (unsigned long long)request_id);
    int result = CP0_STORE_RESULT_ERROR;

    if (catalog != NULL && response != NULL && request_length > 0 &&
        (size_t)request_length < sizeof(request) &&
        exchange(request, (size_t)request_length, response,
                 CP0_STORE_FRAME_BYTES, &response_length, 500, NULL) == 0)
        result = parse_catalog_response(response, response_length, request_id,
                                        catalog);
    free(response);
    return result;
}

int cp0_store_today(struct cp0_store_today *today)
{
    char request[192];
    char *response = malloc(CP0_STORE_FRAME_BYTES);
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"today\"}}\n",
        (unsigned long long)request_id);
    int result = CP0_STORE_RESULT_ERROR;

    if (today != NULL && response != NULL && request_length > 0 &&
        (size_t)request_length < sizeof(request) &&
        exchange(request, (size_t)request_length, response,
                 CP0_STORE_FRAME_BYTES, &response_length, 500, NULL) == 0)
        result = parse_today_response(response, response_length, request_id,
                                      today);
    free(response);
    return result;
}

int cp0_store_search(const char *query, uint16_t offset, uint8_t limit,
                     struct cp0_store_search_results *results)
{
    char request[384];
    char *response = malloc(CP0_STORE_FRAME_BYTES);
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int result = CP0_STORE_RESULT_ERROR;

    if (response == NULL || results == NULL || !valid_search_query(query) ||
        offset > CP0_STORE_CATALOG_LIMIT || limit == 0 ||
        limit > CP0_STORE_SEARCH_MAX_APPS) {
        free(response);
        return CP0_STORE_RESULT_ERROR;
    }
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"search\",\"query\":\"%s\",\"offset\":%u,"
        "\"limit\":%u}}\n",
        (unsigned long long)request_id, query, (unsigned int)offset,
        (unsigned int)limit);
    if (request_length > 0 && (size_t)request_length < sizeof(request) &&
        exchange(request, (size_t)request_length, response,
                 CP0_STORE_FRAME_BYTES, &response_length, 500, NULL) == 0)
        result = parse_search_response(response, response_length, request_id,
                                       query, offset, limit, results);
    free(response);
    return result;
}

int cp0_store_refresh(void)
{
    char request[192];
    char response[1024];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"refresh\"}}\n",
        (unsigned long long)request_id);
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 1000, NULL) != 0)
        return CP0_STORE_RESULT_ERROR;
    return parse_accepted(response, response_length, request_id,
                          "refresh-accepted", NULL);
}

int cp0_store_get_auto_update(struct cp0_store_auto_update_status *status)
{
    char request[192];
    char response[1024];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{\"name\":\"get-auto-update\"}}\n",
        (unsigned long long)request_id);
    if (status == NULL || request_length <= 0 ||
        (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 500, NULL) != 0)
        return CP0_STORE_RESULT_ERROR;
    return parse_auto_update_response(response, response_length, request_id,
                                      status);
}

int cp0_store_set_auto_update(
    bool enabled, struct cp0_store_auto_update_status *status)
{
    char request[224];
    char response[1024];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{\"name\":\"set-auto-update\",\"enabled\":%s}}\n",
        (unsigned long long)request_id, enabled ? "true" : "false");
    if (status == NULL || request_length <= 0 ||
        (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 3000, NULL) != 0)
        return CP0_STORE_RESULT_ERROR;
    return parse_auto_update_response(response, response_length, request_id,
                                      status);
}

int cp0_store_preflight_install(
    uint64_t catalog_sequence, const char *const app_ids[], size_t app_count,
    struct cp0_store_install_preflight *preflight)
{
    char request[2048];
    char response[8192];
    size_t request_offset;
    size_t response_length;
    uint64_t request_id = next_request_id++;

    if (catalog_sequence == 0 || preflight == NULL ||
        !valid_install_batch(app_ids, app_count))
        return CP0_STORE_RESULT_ERROR;
    int written = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"preflight-install\",\"app_ids\":[",
        (unsigned long long)request_id);
    if (written <= 0 || (size_t)written >= sizeof(request))
        return CP0_STORE_RESULT_ERROR;
    request_offset = (size_t)written;
    for (size_t index = 0; index < app_count; index++) {
        written = snprintf(request + request_offset,
                           sizeof(request) - request_offset, "%s\"%s\"",
                           index == 0 ? "" : ",", app_ids[index]);
        if (written <= 0 || (size_t)written >= sizeof(request) - request_offset)
            return CP0_STORE_RESULT_ERROR;
        request_offset += (size_t)written;
    }
    written = snprintf(request + request_offset,
                       sizeof(request) - request_offset,
                       "],\"catalog_sequence\":%llu}}\n",
                       (unsigned long long)catalog_sequence);
    if (written <= 0 || (size_t)written >= sizeof(request) - request_offset)
        return CP0_STORE_RESULT_ERROR;
    request_offset += (size_t)written;
    if (exchange(request, request_offset, response, sizeof(response),
                 &response_length, 1000, NULL) != 0)
        return CP0_STORE_RESULT_ERROR;
    return parse_install_preflight(response, response_length, request_id,
                                   catalog_sequence, app_ids, app_count,
                                   preflight);
}

int cp0_store_install(const char *app_id, uint64_t authorization_id)
{
    char request[384];
    char response[1024];
    size_t response_length;
    uint64_t request_id = next_request_id++;

    if (!valid_app_id(app_id) || authorization_id == 0)
        return CP0_STORE_RESULT_ERROR;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"install\",\"app_id\":\"%s\","
        "\"authorization_id\":%llu}}\n",
        (unsigned long long)request_id, app_id,
        (unsigned long long)authorization_id);
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 1000, NULL) != 0)
        return CP0_STORE_RESULT_ERROR;
    return parse_accepted(response, response_length, request_id,
                          "install-accepted", app_id);
}

int cp0_store_install_batch(const char *const app_ids[], size_t app_count,
                            uint64_t authorization_id)
{
    char request[2048];
    char response[4096];
    size_t request_offset = 0;
    size_t response_length;
    uint64_t request_id = next_request_id++;

    if (!valid_install_batch(app_ids, app_count) || authorization_id == 0)
        return CP0_STORE_RESULT_ERROR;
    int written = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"install-batch\",\"app_ids\":[",
        (unsigned long long)request_id);
    if (written <= 0 || (size_t)written >= sizeof(request))
        return CP0_STORE_RESULT_ERROR;
    request_offset = (size_t)written;
    for (size_t index = 0; index < app_count; index++) {
        written = snprintf(request + request_offset,
                           sizeof(request) - request_offset, "%s\"%s\"",
                           index == 0 ? "" : ",", app_ids[index]);
        if (written <= 0 || (size_t)written >= sizeof(request) - request_offset)
            return CP0_STORE_RESULT_ERROR;
        request_offset += (size_t)written;
    }
    written = snprintf(request + request_offset,
                       sizeof(request) - request_offset,
                       "],\"authorization_id\":%llu}}\n",
                       (unsigned long long)authorization_id);
    if (written <= 0 || (size_t)written >= sizeof(request) - request_offset)
        return CP0_STORE_RESULT_ERROR;
    request_offset += (size_t)written;
    if (exchange(request, request_offset, response, sizeof(response),
                 &response_length, 1000, NULL) != 0)
        return CP0_STORE_RESULT_ERROR;
    return parse_install_batch_accepted(response, response_length, request_id,
                                        app_ids, app_count);
}

int cp0_store_control(const char *app_id,
                      enum cp0_store_control_action action)
{
    char request[384];
    char response[2048];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    const char *action_name = control_action_name(action);

    if (!valid_app_id(app_id) || action_name == NULL)
        return CP0_STORE_RESULT_ERROR;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"control\",\"app_id\":\"%s\",\"action\":\"%s\"}}\n",
        (unsigned long long)request_id, app_id, action_name);
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 1000, NULL) != 0)
        return CP0_STORE_RESULT_ERROR;
    return parse_control_accepted(response, response_length, request_id, app_id,
                                  action);
}

int cp0_store_get_details(const char *app_id, const char *expected_version,
                          struct cp0_store_app_details *details)
{
    char request[384];
    char *response = malloc(CP0_STORE_FRAME_BYTES);
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int result = CP0_STORE_RESULT_ERROR;

    if (response == NULL || details == NULL || !valid_app_id(app_id) ||
        !valid_version(expected_version)) {
        free(response);
        return CP0_STORE_RESULT_ERROR;
    }
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"details\",\"app_id\":\"%s\"}}\n",
        (unsigned long long)request_id, app_id);
    if (request_length > 0 && (size_t)request_length < sizeof(request) &&
        exchange(request, (size_t)request_length, response,
                 CP0_STORE_FRAME_BYTES, &response_length, 5000, NULL) == 0)
        result = parse_details_response(response, response_length, request_id,
                                        app_id, expected_version, details);
    free(response);
    return result;
}

static int get_media(const char *app_id, const char *expected_version,
                     enum parsed_media_kind kind, uint8_t index,
                     uint32_t *pixels, size_t pixel_capacity,
                     struct cp0_store_image_metadata *metadata)
{
    char request[512];
    char *response = malloc(CP0_STORE_FRAME_BYTES);
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int media_descriptor = -1;
    int result = CP0_STORE_RESULT_ERROR;
    int request_length;

    if (response == NULL || pixels == NULL || metadata == NULL ||
        !valid_app_id(app_id) || !valid_version(expected_version) ||
        (kind == PARSED_MEDIA_SCREENSHOT &&
         index >= CP0_STORE_MAX_SCREENSHOTS))
        goto cleanup;
    if (kind == PARSED_MEDIA_ICON) {
        request_length = snprintf(
            request, sizeof(request),
            "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
            "\"name\":\"media\",\"app_id\":\"%s\",\"media\":{"
            "\"kind\":\"icon\"}}}\n",
            (unsigned long long)request_id, app_id);
    } else {
        request_length = snprintf(
            request, sizeof(request),
            "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
            "\"name\":\"media\",\"app_id\":\"%s\",\"media\":{"
            "\"kind\":\"screenshot\",\"index\":%u}}}\n",
            (unsigned long long)request_id, app_id, (unsigned int)index);
    }
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response,
                 CP0_STORE_FRAME_BYTES, &response_length, 5000,
                 &media_descriptor) != 0)
        goto cleanup;
    struct parsed_media parsed;
    result = parse_media_response(response, response_length, request_id,
                                  app_id, expected_version, kind, index,
                                  &parsed);
    if (result != CP0_STORE_RESULT_OK) {
        if (media_descriptor >= 0)
            result = CP0_STORE_RESULT_ERROR;
        goto cleanup;
    }
    if (media_descriptor < 0 ||
        decode_png_descriptor(media_descriptor, &parsed.metadata, pixels,
                              pixel_capacity) != 0) {
        result = CP0_STORE_RESULT_ERROR;
        goto cleanup;
    }
    *metadata = parsed.metadata;

cleanup:
    if (media_descriptor >= 0)
        close(media_descriptor);
    free(response);
    return result;
}

int cp0_store_get_icon(const char *app_id, const char *expected_version,
                       uint32_t *pixels, size_t pixel_capacity,
                       struct cp0_store_image_metadata *metadata)
{
    return get_media(app_id, expected_version, PARSED_MEDIA_ICON, 0, pixels,
                     pixel_capacity, metadata);
}

int cp0_store_get_screenshot(
    const char *app_id, const char *expected_version, uint8_t index,
    uint32_t *pixels, size_t pixel_capacity,
    struct cp0_store_image_metadata *metadata)
{
    return get_media(app_id, expected_version, PARSED_MEDIA_SCREENSHOT, index,
                     pixels, pixel_capacity, metadata);
}
